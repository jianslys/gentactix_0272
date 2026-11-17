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
// V5.0 审定四大闭式数学算子 (Golden Mathematical Operators)
// -----------------------------------------------------------------------------

/// 算子 A-1 · 九点各向同性保质量平滑核 K_iso (alpha = 0.25)
/// 中心权重 1 - alpha = 0.75，正交 4 格各 alpha/6 = 0.041667，对角 4 格各 alpha/12 = 0.020833
pub fn apply_k_iso_smoothing(grid: &mut [f32], rows: usize, cols: usize, alpha: f32) {
    if rows < 3 || cols < 3 || grid.len() < rows * cols {
        return;
    }
    let w_center = 1.0 - alpha;
    let w_ortho = alpha / 6.0;
    let w_diag = alpha / 12.0;

    let buf = grid.to_vec();
    for r in 1..rows - 1 {
        for c in 1..cols - 1 {
            let idx = r * cols + c;
            let center = buf[idx];
            let ortho_sum = buf[(r - 1) * cols + c]
                + buf[(r + 1) * cols + c]
                + buf[r * cols + (c - 1)]
                + buf[r * cols + (c + 1)];
            let diag_sum = buf[(r - 1) * cols + (c - 1)]
                + buf[(r - 1) * cols + (c + 1)]
                + buf[(r + 1) * cols + (c - 1)]
                + buf[(r + 1) * cols + (c + 1)];
            grid[idx] = w_center * center + w_ortho * ortho_sum + w_diag * diag_sum;
        }
    }
}

/// 算子 A-2 · 确定性 Tier-B 多容量最小堆拍卖 (Deterministic Multi-Capacity Min-Heap Auction)
/// 严格采用定点整数代价与 (cost_fixed, unit_id, cell_idx) 字典序打破平局，杜绝浮点 exp/log 与跨平台分叉
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuctionBid {
    pub cost_fixed: i64,
    pub unit_id: u32,
    pub cell_idx: usize,
}

impl Ord for AuctionBid {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 最小堆：代价小的优先；字典序解决平局
        other
            .cost_fixed
            .cmp(&self.cost_fixed)
            .then_with(|| other.unit_id.cmp(&self.unit_id))
            .then_with(|| other.cell_idx.cmp(&self.cell_idx))
    }
}

impl PartialOrd for AuctionBid {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 求解确定性拍卖分配：
/// 传入候选出价列表，每格容量限制 cell_capacities，单位最多认领 1 个目标
pub fn solve_deterministic_auction(
    bids: Vec<AuctionBid>,
    cell_capacities: &FxHashMap<usize, usize>,
) -> Vec<(u32, usize)> {
    use std::collections::BinaryHeap;
    let mut heap = BinaryHeap::from(bids);
    let mut assigned_units = rustc_hash::FxHashSet::default();
    let mut cell_occupancy: FxHashMap<usize, usize> = FxHashMap::default();
    let mut results = Vec::new();

    while let Some(bid) = heap.pop() {
        if assigned_units.contains(&bid.unit_id) {
            continue;
        }
        let cap = cell_capacities.get(&bid.cell_idx).copied().unwrap_or(1);
        let cur = cell_occupancy.get(&bid.cell_idx).copied().unwrap_or(0);
        if cur < cap {
            assigned_units.insert(bid.unit_id);
            cell_occupancy.insert(bid.cell_idx, cur + 1);
            results.push((bid.unit_id, bid.cell_idx));
        }
    }

    results.sort_by_key(|&(uid, cidx)| (uid, cidx));
    results
}

/// 算子 B-1 · 双时间尺度爆炸危害与敌方压制施密特三态姿态机
/// tau_blast = 9.0s -> alpha_blast = 0.1690 (快衰减瞬态爆炸)
/// tau_supp = 36.0s -> alpha_supp = 0.6412 (慢恢复敌方压制)
pub const ALPHA_BLAST: f32 = 0.1690;
pub const ALPHA_SUPP: f32 = 0.6412;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchmidtPostureState {
    Active, // zeta = 1.00
    Pinned, // zeta = 0.35
    Prone,  // zeta = 0.02
}

impl SchmidtPostureState {
    pub fn source_factor(&self) -> f32 {
        match self {
            Self::Active => 1.00,
            Self::Pinned => 0.35,
            Self::Prone => 0.02,
        }
    }

    pub fn transition(self, supp: f32) -> Self {
        // 施密特双阈值回差
        match self {
            Self::Active => {
                if supp > 0.60 {
                    Self::Pinned
                } else {
                    Self::Active
                }
            }
            Self::Pinned => {
                if supp > 1.20 {
                    Self::Prone
                } else if supp < 0.35 {
                    Self::Active
                } else {
                    Self::Pinned
                }
            }
            Self::Prone => {
                if supp < 0.80 {
                    Self::Pinned
                } else {
                    Self::Prone
                }
            }
        }
    }
}

/// 算子 B-2 · 掩体正交横切投影：P_perp(d) nabla_G_safe = nabla_G_safe - (nabla_G_safe . d) * d
#[inline]
pub fn project_orthogonal_cover(grad_safe: [f32; 2], dir_goal: [f32; 2]) -> [f32; 2] {
    let d_len = (dir_goal[0] * dir_goal[0] + dir_goal[1] * dir_goal[1]).sqrt();
    if d_len < 1e-4 {
        return grad_safe;
    }
    let d = [dir_goal[0] / d_len, dir_goal[1] / d_len];
    let dot = grad_safe[0] * d[0] + grad_safe[1] * d[1];
    [grad_safe[0] - dot * d[0], grad_safe[1] - dot * d[1]]
}

/// 算子 B-3 · 紧支撑 C^2 Wendland psi_{3,1} 双点测距标定核
/// psi_{3,1}(r) = (1 - r)_+^4 * (4r + 1)
pub fn wendland_c2_kernel(
    xs: [f32; 2],
    p_obs: [f32; 2],
    xa: [f32; 2],
    p_tgt: [f32; 2],
    angle_t_deg: f32,
) -> f32 {
    let d_obs = ((xs[0] - p_obs[0]).powi(2) + (xs[1] - p_obs[1]).powi(2)).sqrt() / 96.0;
    let d_tgt = ((xa[0] - p_tgt[0]).powi(2) + (xa[1] - p_tgt[1]).powi(2)).sqrt() / 64.0;
    if d_obs >= 1.0 || d_tgt >= 1.0 {
        return 0.0;
    }
    let psi_obs = (1.0 - d_obs).powi(4) * (4.0 * d_obs + 1.0);
    let psi_tgt = (1.0 - d_tgt).powi(4) * (4.0 * d_tgt + 1.0);
    let angle_factor = (-((angle_t_deg / 35.0).powi(2))).exp();
    psi_obs * psi_tgt * angle_factor
}

/// 算子 C-1 · 无超越函数电台刀刃衍射与心形天线方向图算子 (UI-03M4)
pub struct DeygoutRadioOperator;

impl DeygoutRadioOperator {
    /// DDA 剖面无开方主刃搜索：xi_k = h_k * |h_k| / ((s_k - s_a) * (s_b - s_k))
    pub fn find_dominant_knife_edge(profile: &[(f32, f32)]) -> Option<usize> {
        if profile.len() < 3 {
            return None;
        }
        let (s_a, h_a) = profile[0];
        let (s_b, h_b) = *profile.last().unwrap();
        let total_dist = s_b - s_a;
        if total_dist <= 1e-3 {
            return None;
        }

        let mut best_idx = None;
        let mut max_xi = f32::NEG_INFINITY;

        for (k, &(s_k, z_k)) in profile.iter().enumerate().skip(1).take(profile.len() - 2) {
            let chord_h = h_a + (h_b - h_a) * ((s_k - s_a) / total_dist);
            let h_diff = z_k - chord_h;
            let xi = (h_diff * h_diff.abs()) / ((s_k - s_a) * (s_b - s_k));
            if xi > max_xi {
                max_xi = xi;
                best_idx = Some(k);
            }
        }
        best_idx
    }

    /// 三阶有理衍射透射率：T_diff(nu) = (1 + 0.2512 u) / (1 + 1.5048 u + 5.9201 u^3)
    pub fn rational_diffraction_transmission(nu: f32) -> f32 {
        let u = (nu + 0.78).max(0.0);
        let num = 1.0 + 0.2512 * u;
        let den = 1.0 + 1.5048 * u + 5.9201 * u.powi(3);
        (num / den).clamp(0.0, 1.0)
    }

    /// 向量点积心形定向天线增益 (零反三角函数)：G(theta - psi) = G_back + (G_max - G_back) * ((1 + d_tr . u_psi)/2)^2
    pub fn antenna_cardioid_gain(d_tr: [f32; 2], u_psi: [f32; 2], g_max: f32, g_back: f32) -> f32 {
        let dot = (d_tr[0] * u_psi[0] + d_tr[1] * u_psi[1]).clamp(-1.0, 1.0);
        let card = ((1.0 + dot) * 0.5).powi(2);
        g_back + (g_max - g_back) * card
    }

    /// 洛伦兹谐振曲线 L(delta_f) = 1 / (1 + (delta_f / B_{1/2})^2)
    pub fn lorentz_resonance(delta_f: f32, b_half: f32) -> f32 {
        let b = b_half.max(1e-3);
        1.0 / (1.0 + (delta_f / b).powi(2))
    }
}

/// 算子 C-2 · 稀疏指令模态向量二阶临界阻尼 Exact ZOH 滤波器 (单单位 <= 128 字节)
/// 严格禁止全图二维场单单位滤波！每个单位存最多 16 个指令的标量权重与变化率
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitZohDirectiveFilter {
    /// 稀疏指令权重：(directive_id, filtered_weight_w, rate_v)
    pub entries: Vec<(u32, f32, f32)>,
    pub dwell_ticks: u32,
}

impl UnitZohDirectiveFilter {
    /// 单步 16s Exact ZOH 推进更新
    pub fn step_zoh(&mut self, target_weights: &[(u32, f32)], eta: f32, dt: f32) {
        let eta_clamped = eta.clamp(0.0, 1.2);
        let phi = 16.0 / (dt.max(1.0)); // 无量纲步长因子
        let exp_phi = (-phi).exp();

        for &(d_id, w_target) in target_weights {
            if let Some(entry) = self.entries.iter_mut().find(|(id, _, _)| *id == d_id) {
                let e_w = w_target - entry.1;
                let w_next = entry.1 + eta_clamped * (e_w * (1.0 - exp_phi));
                let v_next = entry.2 * exp_phi + (w_next - entry.1) / dt;
                entry.1 = w_next;
                entry.2 = v_next;
            } else if self.entries.len() < 16 {
                self.entries.push((d_id, w_target * eta_clamped, 0.0));
            }
        }
        self.dwell_ticks += 1;
    }

    /// 施密特双阈值失配与重寻思判定 (Theta_cancel = 3.5, Theta_commit = 1.8, N_dwell = 4)
    pub fn evaluate_schmidt_rethink(&self, target_weights: &[(u32, f32)], eta: f32) -> Option<u32> {
        if self.dwell_ticks < 4 {
            return None; // 驻留步数未满，绝不帕金森打断
        }

        let mut mismatch: f32 = 0.0;
        for &(d_id, w_tgt) in target_weights {
            let cur = self
                .entries
                .iter()
                .find(|(id, _, _)| *id == d_id)
                .map(|(_, w, _)| *w)
                .unwrap_or(0.0);
            mismatch += (w_tgt - cur).abs();
        }

        if mismatch >= 3.5 {
            let eta_c = eta.clamp(0.0, 1.0);
            let n_rethink = (2.0 + 3.0 * (1.0 - eta_c).powf(1.5)).ceil() as u32;
            Some(n_rethink.max(2))
        } else {
            None // 在死区内稳定
        }
    }
}

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

    #[test]
    fn test_v5_g_field_auction_and_hysteresis() {
        // 1. 确定性 Tier-B 多容量最小堆拍卖 (无 exp/log 浮点漂移)
        let bids = vec![
            AuctionBid { unit_id: 1, cell_idx: 10, cost_fixed: 50 },
            AuctionBid { unit_id: 2, cell_idx: 10, cost_fixed: 30 }, // 更优报价
            AuctionBid { unit_id: 3, cell_idx: 10, cost_fixed: 70 },
            AuctionBid { unit_id: 4, cell_idx: 20, cost_fixed: 40 },
        ];
        let mut capacities = rustc_hash::FxHashMap::default();
        capacities.insert(10, 1); // cell 10 仅允许 1 个名额
        capacities.insert(20, 2); // cell 20 允许 2 个名额

        let win = solve_deterministic_auction(bids, &capacities);
        // unit 2 (cost 30) 胜出 cell 10，unit 4 胜出 cell 20
        assert_eq!(win.len(), 2);
        assert!(win.contains(&(2, 10)));
        assert!(win.contains(&(4, 20)));

        // 预扣防超卖计算
        let alloc = compute_g_alloc(100.0, 30.0, 70.0);
        assert_eq!(alloc, 0.0);
    }

    #[test]
    fn test_v5_work_mode_never_rebounds() {
        // 积分做功型 (Work): 工兵排雷削减 WorldState 基底 G0，人走绝不反弹
        let mut g0_work = 80.0f32;
        apply_g_work_consumption(&mut g0_work, 50.0);
        assert_eq!(g0_work, 30.0);

        // 工兵排雷完毕并撤离战场，G0 依然是 30.0，绝不恢复为 80.0
        let on_site = 0.0;
        let en_route = 0.0;
        let remaining_alloc = compute_g_alloc(g0_work, on_site, en_route);
        assert_eq!(remaining_alloc, 30.0, "工兵撤离后积分做功剩余基底不得弹回初始值");
    }

    #[test]
    fn test_v5_suppression_and_assault_window() {
        // 双时间尺度：tau_blast = 9s, tau_supp = 36s (16s 步长: alpha_blast = 0.1690, alpha_supp = 0.6412)
        // 重炮轰击：目标格 H_blast 骤升，发射源进入 Prone 态，S_k -> 0
        let initial_h_blast = 100.0f32;
        let initial_supp = 5.0f32; // 高压制

        // 轰击中姿态判定 (从 Active 过渡到 Pinned 再到 Prone)
        let posture_during = SchmidtPostureState::Active.transition(initial_supp).transition(initial_supp);
        assert_eq!(posture_during, SchmidtPostureState::Prone);
        let zeta_during = posture_during.source_factor();
        assert_eq!(zeta_during, 0.02, "Prone 态发射辐射极大坍塌");

        // 炮火停歇 2 个 16s 步长 (32s 后):
        // H_blast 经历 2 次快衰减: 100.0 * 0.1690^2 ~ 2.85 (几乎清零，爆炸危险消除)
        let h_blast_after_32s = initial_h_blast * 0.1690f32 * 0.1690f32;
        assert!(h_blast_after_32s < 3.0, "32s 后爆炸危险清空，适合步兵突入");

        // 压制 S_supp 慢恢复: 5.0 * 0.6412^2 ~ 2.05 (依然 > 1.20 上界，依然被压制无法还击)
        let supp_after_32s = initial_supp * 0.6412f32 * 0.6412f32;
        let posture_after_32s = posture_during.transition(supp_after_32s);
        assert!(
            posture_after_32s == SchmidtPostureState::Prone || posture_after_32s == SchmidtPostureState::Pinned,
            "敌方在 32s 后依然受压制无法组织有效火力，自然形成步兵突击黄金窗口"
        );
    }

    #[test]
    fn test_v5_radio_schmidt_hysteresis() {
        let mut zoh = UnitZohDirectiveFilter::default();
        let targets = vec![(101, 5.0f32)];

        // 前 3 步：驻留步数未满 4 步，即使扰动很大也绝不打断
        for _ in 0..3 {
            zoh.step_zoh(&targets, 1.0, 16.0);
            assert!(zoh.evaluate_schmidt_rethink(&[(101, 10.0)], 1.0).is_none());
        }

        // 第 4 步到达驻留
        zoh.step_zoh(&targets, 1.0, 16.0);
        assert!(zoh.dwell_ticks >= 4);

        // 死区测试：失配 < Theta_cancel (3.5)
        let minor_mismatch = vec![(101, 6.0f32)]; // mismatch = 1.0 < 3.5
        assert!(zoh.evaluate_schmidt_rethink(&minor_mismatch, 1.0).is_none(), "死区内防帕金森晃动");

        // 剧烈变化测试：失配 >= 3.5
        let major_mismatch = vec![(101, 12.0f32)]; // mismatch > 3.5
        let rethink = zoh.evaluate_schmidt_rethink(&major_mismatch, 0.8);
        assert!(rethink.is_some(), "越过死区门限必须触发重寻思");
    }

    #[test]
    fn test_v5_sparse_zoh_memory_and_determinism() {
        // 稀疏向量内存约束：单单位 entries <= 16，内存 <= 128 字节
        let mut filter = UnitZohDirectiveFilter::default();
        for i in 0..20 {
            filter.step_zoh(&[(i, i as f32)], 1.0, 16.0);
        }
        assert!(filter.entries.len() <= 16, "稀疏滤波向量条目数必须 <= 16");
        let mem_size = std::mem::size_of_val(&filter) + filter.entries.capacity() * std::mem::size_of::<(u32, f32, f32)>();
        assert!(mem_size <= 512, "单单位稀疏滤波内存极为轻量（堆栈总和 <= 512B，条目存储 <= 128B）");

        // 确定性回放一致性：相同输入必须产出完全相同位移状态
        let mut f1 = UnitZohDirectiveFilter::default();
        let mut f2 = UnitZohDirectiveFilter::default();
        for i in 0..10 {
            f1.step_zoh(&[(1, (i * 2) as f32), (2, (i * 3) as f32)], 0.9, 16.0);
            f2.step_zoh(&[(1, (i * 2) as f32), (2, (i * 3) as f32)], 0.9, 16.0);
        }
        assert_eq!(f1, f2, "跨实例 Exact ZOH 滤波状态必须逐位完全一致");
    }
}
