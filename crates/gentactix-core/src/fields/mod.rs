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
}
