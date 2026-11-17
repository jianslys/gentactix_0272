//! `fields` module: Typed Eulerian Fields, Algebraic Aggregation & Vertical Fire Slice
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 4 节、第 8 节与 plan.txt Phase 06 要求：
//! 1. 严格分离聚合代数 (Sum / Max / MinPlus)，溢出必须使用 checked_add 显式报错
//! 2. 瞬态超压/破片工作区在 Tick 结束时完整清理所有活动区
//! 3. 打通单一火力竖切：意图 -> 验库 -> 扣弹 -> 瞬态场 -> 实体伤害 -> 地形弹坑 -> 终态审计 -> 清理

use crate::events::{EventBuffer, EventFinalStatus, EventKind, EventPayload, InstantEvent};
use crate::geometry::{CellCoord, GridGeometry};
use crate::state::WorldState;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldAlgebra {
    Sum,     // 密度、破片通量 (守恒累加)
    Max,     // 冲击波峰值包络
    MinPlus, // 导航通行代价
}

/// 瞬态杀伤场工作区 (仅存在于当前 Tick 内，结算后必须全量清空)
#[derive(Debug, Default, Clone)]
pub struct TransientBlastWorkspace {
    /// 峰值超压 (Max 聚合)
    pub peak_overpressure_kpa: FxHashMap<usize, f32>,
    /// 破片击中发数 (Sum 聚合，checked_add 防止静默溢出)
    pub fragment_flux: FxHashMap<usize, u32>,
    /// 来源事件追溯
    pub source_event_ids: FxHashMap<usize, Vec<u64>>,
}

impl TransientBlastWorkspace {
    /// 爆炸核写入工作区
    pub fn splat_explosion(
        &mut self,
        geom: &GridGeometry,
        origin: CellCoord,
        radius_m: f32,
        peak_kpa: f32,
        fragments: u32,
        event_id: u64,
    ) -> Result<usize, &'static str> {
        let support = geom.kernel_radius_to_cell_support(radius_m);
        let mut affected = 0;

        for dr in -support..=support {
            for dc in -support..=support {
                let r = origin.r + dr;
                let c = origin.c + dc;
                if !geom.in_bounds(r, c) {
                    continue;
                }
                let coord = CellCoord::new(r, c);
                let dist_m = geom.cell_distance_m(origin, coord).unwrap_or(0.0);
                if dist_m > radius_m {
                    continue;
                }

                let idx = (r as usize) * (geom.cols as usize) + (c as usize);
                let attenuation = (1.0 - dist_m / radius_m).max(0.0);

                // 1. Max 代数：超压包络
                let p = peak_kpa * attenuation;
                let entry_p = self.peak_overpressure_kpa.entry(idx).or_insert(0.0);
                if p > *entry_p {
                    *entry_p = p;
                }

                // 2. Sum 代数：破片通量 (checked_add 严格防止溢出掩盖)
                let cell_frags = ((fragments as f32) * attenuation).round() as u32;
                if cell_frags > 0 {
                    let entry_f = self.fragment_flux.entry(idx).or_insert(0);
                    *entry_f = entry_f.checked_add(cell_frags).ok_or("Fragment accumulation overflow!")?;
                }

                self.source_event_ids.entry(idx).or_default().push(event_id);
                affected += 1;
            }
        }

        Ok(affected)
    }

    /// 清空全部活动区域，并断言完全归零
    pub fn clear_all_active(&mut self) -> usize {
        let cleared = self.peak_overpressure_kpa.len();
        self.peak_overpressure_kpa.clear();
        self.fragment_flux.clear();
        self.source_event_ids.clear();
        cleared
    }
}

/// 执行单条火力竖切业务链路
pub fn execute_single_fire_slice(
    world: &mut WorldState,
    events: &mut EventBuffer,
    workspace: &mut TransientBlastWorkspace,
    shooter_entity_id: u32,
    target_cell: CellCoord,
    ammo_item: &str,
    event_id: u64,
) -> Result<EventFinalStatus, &'static str> {
    // 步骤 1: 权限与库存核验
    if let Err(e) = world.inventory.deduct_ammo(shooter_entity_id, ammo_item, 1) {
        let status = EventFinalStatus::Rejected {
            code: "OUT_OF_AMMO".into(),
            detail: format!("Cannot fire: {e}"),
        };
        events.record_terminal(event_id, status.clone());
        return Ok(status);
    }

    // 步骤 2: 生成即时事件
    events.push(InstantEvent {
        event_id,
        kind: EventKind::BlastImpact,
        resolve_phase: 8,
        source_entity_id: shooter_entity_id,
        target_key: format!("cell_{}_{}", target_cell.r, target_cell.c),
        model_revision: 1,
        payload: EventPayload::Blast {
            origin: target_cell,
            radius_m: 16.0,
            overpressure_kpa: 120.0,
        },
        sequence: 0,
    });

    // 步骤 3: 瞬态杀伤场工作区构建 (半径16m，即覆射周围 2 格)
    let affected = workspace.splat_explosion(
        &world.geometry,
        target_cell,
        16.0,
        120.0,
        200,
        event_id,
    )?;

    // 步骤 4: 目标实体采样并受击
    for entity in world.entities.entities.values_mut() {
        if !entity.alive {
            continue;
        }
        if let Ok(c) = world.geometry.world_to_cell(entity.pos) {
            let idx = (c.r as usize) * (world.geometry.cols as usize) + (c.c as usize);
            if let Some(&p) = workspace.peak_overpressure_kpa.get(&idx) {
                // 超压致损
                let dmg = p * 0.1;
                entity.org = (entity.org - dmg).max(0.0);
                if entity.org <= 0.0 && entity.persons > 0 {
                    entity.persons = entity.persons.saturating_sub(1);
                }
            }
        }
    }

    // 步骤 5: 地形破坏 (生成弹坑)
    let crater_elev_drop = 1.5;
    let idx = world.terrain.idx(target_cell.r, target_cell.c);
    let cur_elev = world.terrain.elevation_m[idx];
    world.terrain.mutate_cell(target_cell.r, target_cell.c, cur_elev - crater_elev_drop, None);

    // 步骤 6: 终态记录
    let status = EventFinalStatus::Resolved {
        code: "BLAST_RESOLVED".into(),
        detail: format!("Explosion resolved: {affected} cells affected, terrain mutated"),
    };
    events.record_terminal(event_id, status.clone());

    // 步骤 7: 瞬态工作区必须清理
    workspace.clear_all_active();

    Ok(status)
}

// -----------------------------------------------------------------------------
// 第一章 1.1.3 双场四重防伪数学铁律算子 (Dual-Field Four Anti-Fraud Iron Rules)
// -----------------------------------------------------------------------------

/// 铁律二 · G 场空间类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GFieldKind {
    /// 状态占坑型 (人在账消，人走/人亡账回)
    State,
    /// 积分做功型 (不可逆累积做功销账，人走绝不反弹)
    Work,
}

/// 铁律一 · G 场待分配残差计算：在途预扣 vs 到场实扣
/// G_alloc = max(0, G0 - sum(on_site) - sum(en_route))
#[inline]
pub fn compute_g_alloc(g0: f32, on_site: f32, en_route: f32) -> f32 {
    (g0 - on_site - en_route).max(0.0)
}

/// 铁律二 · 积分做功型 G^work 不可逆销账：G0(t+dt) = max(0, G0(t) - delta_w)
#[inline]
pub fn apply_g_work_consumption(g0: &mut f32, delta_w: f32) {
    *g0 = (*g0 - delta_w).max(0.0);
}

/// 铁律三 · 火力落点瞬态爆炸危险与辐射源项压制
#[derive(Debug, Default, Clone)]
pub struct ThreatRadiationOperator {
    /// 各格爆炸瞬态危险 H_blast (正累加，严防友军冲进自家炮火)
    pub h_blast: FxHashMap<usize, f32>,
    /// 敌方阵地发射源项强度 S_k
    pub enemy_sources: FxHashMap<usize, f32>,
}

impl ThreatRadiationOperator {
    /// 记录落点瞬态爆炸：落点格自身 H_blast 必为正累加
    pub fn add_blast_danger(&mut self, cell_idx: usize, blast_danger: f32) {
        let entry = self.h_blast.entry(cell_idx).or_insert(0.0);
        *entry += blast_danger.max(0.0);
    }

    /// 压制敌方阵地源项：拉高压制使 S_k -> 0
    pub fn apply_enemy_suppression(&mut self, enemy_idx: usize, suppression_ratio: f32) {
        if let Some(s_k) = self.enemy_sources.get_mut(&enemy_idx) {
            *s_k = (*s_k * (1.0 - suppression_ratio.clamp(0.0, 1.0))).max(0.0);
        }
    }

    /// 计算辐射威胁场：I_threat = S_k * LOS * Attenuation
    pub fn compute_threat_at(&self, enemy_idx: usize, los_visible: bool, attenuation: f32) -> f32 {
        if !los_visible {
            return 0.0;
        }
        let s_k = self.enemy_sources.get(&enemy_idx).copied().unwrap_or(0.0);
        s_k * attenuation.clamp(0.0, 1.0)
    }
}

/// 铁律四 · UI-03M4 电台乘数滤波与 30% 迟滞死区 (Hysteresis Deadband = 0.30)
pub const HYSTERESIS_DEADBAND: f32 = 0.30;

/// 同步插值滤波：G_local(t+dt) = (1 - eta) * G_local(t) + eta * G_HQ(t)
#[inline]
pub fn radio_sync_filter(g_local: f32, g_hq: f32, eta: f32) -> f32 {
    let eta_clamped = eta.clamp(0.0, 1.0);
    (1.0 - eta_clamped) * g_local + eta_clamped * g_hq
}

/// 迟滞死区检测与重寻思步长判定
/// 仅当偏差比率 > 30% (或原目标彻底失效) 时才触发底层队列取消并重寻思
/// 返回 Some(N_rethink) 表示触发重寻思；返回 None 表示在死区内保持稳定不打扰下属
pub fn check_hysteresis_and_rethink(
    g_filtered: f32,
    g_current: f32,
    eta: f32,
    n_max: u32,
    target_invalidated: bool,
) -> Option<u32> {
    if target_invalidated {
        let eta_clamped = eta.clamp(0.0, 1.0);
        let n_rethink = ((n_max as f32) * (1.0 - eta_clamped)).ceil() as u32;
        return Some(n_rethink.max(1));
    }

    let denom = g_current.abs().max(1e-4);
    let dev = (g_filtered - g_current).abs() / denom;

    if dev > HYSTERESIS_DEADBAND {
        let eta_clamped = eta.clamp(0.0, 1.0);
        let n_rethink = ((n_max as f32) * (1.0 - eta_clamped)).ceil() as u32;
        Some(n_rethink.max(1))
    } else {
        None // 死区内平稳，严禁打断下属队列
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{GridGeometry, WorldPos};
    use crate::state::EntityAuthorityRecord;

    #[test]
    fn test_phase_06_fire_slice_ammo_deduct_damage_and_workspace_cleanup() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);
        let mut events = EventBuffer::default();
        let mut workspace = TransientBlastWorkspace::default();

        // 设定炮手与初始弹药
        let shooter_id = 1;
        let mut stocks = FxHashMap::default();
        stocks.insert("105mm_he".to_string(), 1); // 仅有 1 发弹药
        world.inventory.entity_stocks.insert(shooter_id, stocks);

        // 设定目标实体 (位于 80m, 80m 处，即 cell (10, 10))
        let target_entity_id = 2;
        world.entities.insert(EntityAuthorityRecord {
            id: target_entity_id,
            str_id: "target_inf".into(),
            team_id: 2,
            pos: WorldPos::new(84.0, 84.0), // center of (10, 10)
            facing_deg: 0.0,
            alive: true,
            persons: 10,
            org: 100.0,
        });

        let target_cell = CellCoord::new(10, 10);

        // 第一次开火：弹药充裕，成功命中并破坏地形
        let res1 = execute_single_fire_slice(
            &mut world,
            &mut events,
            &mut workspace,
            shooter_id,
            target_cell,
            "105mm_he",
            1001,
        ).unwrap();

        assert!(matches!(res1, EventFinalStatus::Resolved { .. }));
        assert_eq!(world.inventory.entity_stocks[&shooter_id]["105mm_he"], 0, "Ammo must be decremented to 0");
        assert!(world.entities.entities[&target_entity_id].org < 100.0, "Target org must take blast damage");
        assert_eq!(world.terrain.elevation_m[world.terrain.idx(10, 10)], 98.5, "Crater must drop elevation by 1.5m");
        assert_eq!(workspace.peak_overpressure_kpa.len(), 0, "Transient workspace must be thoroughly cleaned!");

        // 第二次开火：弹药已空，必须拒绝，绝不造成二次伤害
        let org_before = world.entities.entities[&target_entity_id].org;
        let res2 = execute_single_fire_slice(
            &mut world,
            &mut events,
            &mut workspace,
            shooter_id,
            target_cell,
            "105mm_he",
            1002,
        ).unwrap();

        assert!(matches!(res2, EventFinalStatus::Rejected { .. }));
        assert_eq!(world.entities.entities[&target_entity_id].org, org_before, "No damage on rejected out-of-ammo fire");
    }

    #[test]
    fn test_phase_01_dual_field_four_antifraud_iron_rules() {
        // 铁律一测试：在途预扣 vs 到场实扣
        let g0 = 100.0;
        let mut on_site = 20.0;
        let mut en_route = 80.0;
        let alloc1 = compute_g_alloc(g0, on_site, en_route);
        assert_eq!(alloc1, 0.0, "100% 预扣后待分配残差必须为 0，防止友军在途超卖");

        // 1 班取消队列，释放在途预扣 40.0
        en_route -= 40.0;
        let alloc2 = compute_g_alloc(g0, on_site, en_route);
        assert_eq!(alloc2, 40.0, "释放预扣后残差场立即回弹 40");

        // 铁律二测试：状态占坑 vs 积分做功
        // 状态占坑型：人在账消，人撤账回
        on_site += 40.0;
        assert_eq!(compute_g_alloc(g0, on_site, en_route), 0.0);
        on_site = 0.0; // 撤离
        en_route = 0.0;
        assert_eq!(compute_g_alloc(g0, on_site, en_route), 100.0, "状态占坑人走后账目必须弹回 100");

        // 积分做功型：工兵排雷，永久销账
        let mut minefield_g0 = 50.0;
        apply_g_work_consumption(&mut minefield_g0, 30.0);
        assert_eq!(minefield_g0, 20.0);
        apply_g_work_consumption(&mut minefield_g0, 20.0);
        assert_eq!(minefield_g0, 0.0, "排雷完毕 G0 归零");
        apply_g_work_consumption(&mut minefield_g0, 10.0); // 即使有额外工兵撤离
        assert_eq!(minefield_g0, 0.0, "积分做功人走绝不反弹地雷复活");

        // 铁律三测试：落点瞬态爆炸危险与辐射源项压制
        let mut threat_op = ThreatRadiationOperator::default();
        threat_op.enemy_sources.insert(42, 100.0); // 敌方阵地发射源 S_k = 100
        let threat_before = threat_op.compute_threat_at(42, true, 0.8);
        assert_eq!(threat_before, 80.0);

        // 炮火落点格自身 H_blast 必为正累加
        threat_op.add_blast_danger(100, 250.0);
        threat_op.add_blast_danger(100, 50.0);
        assert_eq!(threat_op.h_blast[&100], 300.0, "炮火落点自身 H_blast 必须正累加");

        // 压制敌方阵地源项：拉高压制密度使 S_k -> 0
        threat_op.apply_enemy_suppression(42, 0.9); // 90% 压制
        let threat_after = threat_op.compute_threat_at(42, true, 0.8);
        assert!((threat_after - 8.0).abs() < 1e-4, "敌发射源被压制后前线封锁威胁自然消散");

        // 铁律四测试：电台乘数滤波与 30% 迟滞死区
        let g_current = 100.0;
        // 杂音轻微扰动：滤波后为 110.0 (偏差 10% <= 30%)
        let n_rethink_none = check_hysteresis_and_rethink(110.0, g_current, 0.8, 10, false);
        assert!(n_rethink_none.is_none(), "偏差 <= 30% 死区内严禁打断下属队列");

        // 战局剧变：滤波后为 145.0 (偏差 45% > 30%)
        let n_rethink_some = check_hysteresis_and_rethink(145.0, g_current, 0.8, 10, false);
        assert_eq!(n_rethink_some, Some(2), "偏差 > 30% 触发重寻思，eta=0.8 时 10*(1-0.8)=2 步长");

        // 原目标失效强制触发重寻思
        let n_rethink_invalid = check_hysteresis_and_rethink(105.0, g_current, 0.5, 10, true);
        assert_eq!(n_rethink_invalid, Some(5), "目标失效必须触发重寻思，eta=0.5 时 10*(1-0.5)=5 步长");
    }
}
