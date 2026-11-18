//! `tactical_instance` module: Tactical Instance Generation & Augmented Arc-Length Integrator
//! 遵循 SPEC_V5 第 1.2 节与 plan.txt Phase 02 规范：
//! 1. 7.8125mm (1/128m) 定点量化基座 + SplitMix64 确定性伪随机数发生器
//! 2. 三模式实例生成引擎：梅花桩雷区 (Quincunx)、角平分线斜接散兵防线 (Miter-offset)、1.2m锯齿堑壕 (Zigzag A_zig=1.2m)
//! 3. 测地牛耕扫掠 (Boustrophedon) + 2-Opt 局部优化拓扑巡回
//! 4. 16s 增广弧长 s_tilde 边走边干积分器与 4 态中断恢复 FSM
//! 5. 半成品工事权威持久化与爆破起爆开关物理实体闭环 (DetonatorSwitch)

use serde::{Deserialize, Serialize};

/// 7.8125 mm (1/128 m) 定点量化尺度
pub const QUANT_STEP_M: f32 = 0.0078125;
pub const QUANT_SCALE: f32 = 128.0;

/// 定点量化到 7.8125mm 纯二进制网格
#[inline]
pub fn quantize_pos_78125(pos: [f32; 2]) -> [f32; 2] {
    [
        (pos[0] * QUANT_SCALE).round() / QUANT_SCALE,
        (pos[1] * QUANT_SCALE).round() / QUANT_SCALE,
    ]
}

/// 跨平台逐位确定性 64 位 PRNG (SplitMix64)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitMix64 {
    pub state: u64,
}

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / 16777216.0
    }
}

/// 射线法判定点是否在多边形内部
pub fn point_in_polygon(pt: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let pi = polygon[i];
        let pj = polygon[j];
        if ((pi[1] > pt[1]) != (pj[1] > pt[1]))
            && (pt[0] < (pj[0] - pi[0]) * (pt[1] - pi[1]) / (pj[1] - pi[1]) + pi[0])
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 模式一 · 梅花桩雷区 (Quincunx Minefield) 生成引擎
/// 在多边形包围盒内生成错位交替的雷点，并经 7.8125mm 量化
pub fn generate_quincunx_minefield(polygon: &[[f32; 2]], spacing_m: f32, seed: u64) -> Vec<[f32; 2]> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let s = spacing_m.max(1.0);
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for &p in polygon {
        min_x = min_x.min(p[0]);
        max_x = max_x.max(p[0]);
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    }

    let mut prng = SplitMix64::new(seed);
    let mut mines = Vec::new();
    let mut row_idx = 0;
    let mut y = min_y + s * 0.5;

    while y <= max_y {
        let offset_x = if row_idx % 2 == 1 { s * 0.5 } else { 0.0 };
        let mut x = min_x + offset_x;
        while x <= max_x {
            // 加入定点轻微散布扰动 (+-10% spacing)
            let jitter_x = (prng.next_f32() - 0.5) * 0.2 * s;
            let jitter_y = (prng.next_f32() - 0.5) * 0.2 * s;
            let candidate = quantize_pos_78125([x + jitter_x, y + jitter_y]);
            if point_in_polygon(candidate, polygon) {
                mines.push(candidate);
            }
            x += s;
        }
        y += s * 0.866025; // 等边三角形六边形堆积高度因子 sqrt(3)/2
        row_idx += 1;
    }
    mines
}

/// 模式二 · 矢量角平分线/斜接偏置散兵防线 (Miter-offset foxholes)
/// 沿折线计算每段法向量与转角角平分线偏置，按 spacing_m 插入散兵坑
pub fn generate_miter_offset_foxholes(
    polyline: &[[f32; 2]],
    offset_m: f32,
    spacing_m: f32,
) -> Vec<[f32; 2]> {
    let n = polyline.len();
    if n < 2 {
        return Vec::new();
    }
    let spacing = spacing_m.max(2.0);
    let mut offset_line = Vec::with_capacity(n);

    // 计算各段单位法向量
    let mut seg_normals = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let dx = polyline[i + 1][0] - polyline[i][0];
        let dy = polyline[i + 1][1] - polyline[i][1];
        let len = dx.hypot(dy).max(1e-4);
        seg_normals.push([-dy / len, dx / len]);
    }

    // 顶点角平分线斜接
    offset_line.push([
        polyline[0][0] + seg_normals[0][0] * offset_m,
        polyline[0][1] + seg_normals[0][1] * offset_m,
    ]);

    for i in 1..n - 1 {
        let n1 = seg_normals[i - 1];
        let n2 = seg_normals[i];
        let miter = [n1[0] + n2[0], n1[1] + n2[1]];
        let m_len = miter[0].hypot(miter[1]);
        if m_len < 1e-4 {
            offset_line.push([
                polyline[i][0] + n1[0] * offset_m,
                polyline[i][1] + n1[1] * offset_m,
            ]);
        } else {
            let scale = (2.0 / (1.0 + n1[0] * n2[0] + n1[1] * n2[1]).max(0.1)).min(3.0);
            offset_line.push([
                polyline[i][0] + (miter[0] / m_len) * offset_m * scale,
                polyline[i][1] + (miter[1] / m_len) * offset_m * scale,
            ]);
        }
    }

    offset_line.push([
        polyline[n - 1][0] + seg_normals[n - 2][0] * offset_m,
        polyline[n - 1][1] + seg_normals[n - 2][1] * offset_m,
    ]);

    // 沿偏置折线按步长插值散兵坑
    let mut foxholes = Vec::new();
    foxholes.push(quantize_pos_78125(offset_line[0]));

    for i in 0..offset_line.len() - 1 {
        let p1 = offset_line[i];
        let p2 = offset_line[i + 1];
        let dx = p2[0] - p1[0];
        let dy = p2[1] - p1[1];
        let seg_len = dx.hypot(dy);
        if seg_len < 1e-4 {
            continue;
        }
        let steps = (seg_len / spacing).floor() as usize;
        for s in 1..=steps {
            let t = (s as f32 * spacing) / seg_len;
            if t < 0.99 {
                foxholes.push(quantize_pos_78125([p1[0] + dx * t, p1[1] + dy * t]));
            }
        }
    }
    if let Some(&last) = offset_line.last() {
        foxholes.push(quantize_pos_78125(last));
    }
    foxholes
}

/// 模式三 · 1.2m 锯齿堑壕 (Zigzag Trench, A_zig = 1.2m)
/// 沿折线主干生成防炮震荡锯齿折线
pub fn generate_zigzag_trench(
    polyline: &[[f32; 2]],
    step_m: f32,
    amplitude_m: f32,
) -> Vec<[f32; 2]> {
    let n = polyline.len();
    if n < 2 {
        return Vec::new();
    }
    let a_zig = if amplitude_m <= 0.0 { 1.2 } else { amplitude_m };
    let step = step_m.max(2.0);
    let mut trench_nodes = Vec::new();

    trench_nodes.push(quantize_pos_78125(polyline[0]));
    let mut sign = 1.0f32;

    for i in 0..n - 1 {
        let p1 = polyline[i];
        let p2 = polyline[i + 1];
        let dx = p2[0] - p1[0];
        let dy = p2[1] - p1[1];
        let seg_len = dx.hypot(dy);
        if seg_len < 1e-4 {
            continue;
        }
        let dir = [dx / seg_len, dy / seg_len];
        let normal = [-dir[1], dir[0]];
        let num_zigs = (seg_len / step).floor() as usize;

        for k in 1..=num_zigs {
            let t = (k as f32 * step) / seg_len;
            if t < 0.98 {
                let center_x = p1[0] + dx * t;
                let center_y = p1[1] + dy * t;
                let zig_x = center_x + normal[0] * a_zig * sign;
                let zig_y = center_y + normal[1] * a_zig * sign;
                trench_nodes.push(quantize_pos_78125([zig_x, zig_y]));
                sign = -sign;
            }
        }
    }

    if let Some(&last) = polyline.last() {
        trench_nodes.push(quantize_pos_78125(last));
    }
    trench_nodes
}

/// 测地牛耕扫掠与 2-Opt 巡回路径求解器
pub fn solve_boustrophedon_2opt(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut pts = points.to_vec();
    // 1. 牛耕带状粗排序 (按 Y 划分扫掠带，带内按 X 往复交替)
    pts.sort_by(|a, b| {
        let band_a = (a[1] / 8.0).floor() as i32;
        let band_b = (b[1] / 8.0).floor() as i32;
        if band_a != band_b {
            band_a.cmp(&band_b)
        } else if band_a % 2 == 0 {
            a[0].partial_cmp(&b[0]).unwrap()
        } else {
            b[0].partial_cmp(&a[0]).unwrap()
        }
    });

    // 2. 2-Opt 局部优化消除交叉边
    let n = pts.len();
    let mut improved = true;
    let mut iterations = 0;
    while improved && iterations < 50 {
        improved = false;
        iterations += 1;
        for i in 0..n.saturating_sub(2) {
            for j in (i + 2)..n {
                let d_current = pts[i].distance_to_f32(pts[i + 1])
                    + (if j + 1 < n { pts[j].distance_to_f32(pts[j + 1]) } else { 0.0 });
                let d_new = pts[i].distance_to_f32(pts[j])
                    + (if j + 1 < n { pts[i + 1].distance_to_f32(pts[j + 1]) } else { 0.0 });
                if d_new + 1e-3 < d_current {
                    pts[i + 1..=j].reverse();
                    improved = true;
                }
            }
        }
    }
    pts
}

trait PointDistExt {
    fn distance_to_f32(self, other: [f32; 2]) -> f32;
}

impl PointDistExt for [f32; 2] {
    #[inline]
    fn distance_to_f32(self, other: [f32; 2]) -> f32 {
        (self[0] - other[0]).hypot(self[1] - other[1])
    }
}

/// 增广弧长积分器 4 态中断恢复 FSM
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstructionFsmState {
    Normal,
    PinnedPause,
    EvadeReplan,
    Resumed,
}

/// 增广弧长施工巡回积分器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AugmentedArcIntegrator {
    pub waypoints: Vec<[f32; 2]>,
    pub current_wp_idx: usize,
    pub cur_pos: [f32; 2],
    pub s_tilde: f32, // 累计增广弧长坐标
    pub work_done_sec: f32,
    pub fsm_state: ConstructionFsmState,
    pub completed_instances: Vec<usize>,
    pub semi_finished_progress: f32, // 0.0 ~ 1.0 当前节点半成品进度
}

impl AugmentedArcIntegrator {
    pub fn new(waypoints: Vec<[f32; 2]>) -> Self {
        let cur = waypoints.first().copied().unwrap_or([0.0, 0.0]);
        Self {
            waypoints,
            current_wp_idx: 0,
            cur_pos: cur,
            s_tilde: 0.0,
            work_done_sec: 0.0,
            fsm_state: ConstructionFsmState::Normal,
            completed_instances: Vec::new(),
            semi_finished_progress: 0.0,
        }
    }

    /// 单步 16s 推进积分：结合移动与作业耗时，受压制密度 S 动态折减
    /// lambda_eq = 0.20 m/(person*s), W0 = 单个工事定额人秒 (如 40 人*秒)
    pub fn step_16s(
        &mut self,
        speed_m_s: f32,
        persons: u32,
        suppression: f32,
        nominal_work_sec: f32,
    ) {
        if self.current_wp_idx >= self.waypoints.len() {
            return;
        }

        // 压制状态转移
        if suppression > 0.60 {
            self.fsm_state = ConstructionFsmState::PinnedPause;
            return; // 压制严重原地卧倒暂停
        } else if self.fsm_state == ConstructionFsmState::PinnedPause {
            self.fsm_state = ConstructionFsmState::Resumed;
        } else {
            self.fsm_state = ConstructionFsmState::Normal;
        }

        // 速度与作业折减
        let s_eff = suppression.clamp(0.0, 1.0);
        let sigma_move = (1.0 - 0.5 * s_eff).max(0.25);
        let sigma_work = (1.0 - s_eff).powi(2);

        let mut time_budget_s = 16.0f32;
        let v_move = speed_m_s * sigma_move;
        let lambda_eq = 0.20f32;
        let work_rate = (persons as f32) * sigma_work;

        while time_budget_s > 1e-3 && self.current_wp_idx < self.waypoints.len() {
            let target_pt = self.waypoints[self.current_wp_idx];
            let dist_to_target = self.cur_pos.distance_to_f32(target_pt);

            if dist_to_target > 1e-3 {
                // 行进阶段
                let time_to_reach = dist_to_target / v_move.max(0.1);
                if time_budget_s >= time_to_reach {
                    time_budget_s -= time_to_reach;
                    self.cur_pos = target_pt;
                    self.s_tilde += dist_to_target;
                } else {
                    let frac = time_budget_s / time_to_reach;
                    self.cur_pos[0] += (target_pt[0] - self.cur_pos[0]) * frac;
                    self.cur_pos[1] += (target_pt[1] - self.cur_pos[1]) * frac;
                    self.s_tilde += dist_to_target * frac;
                    break;
                }
            }

            // 到达目标点，就地作业阶段
            let req_work_sec = nominal_work_sec.max(1.0);
            let work_needed = (1.0 - self.semi_finished_progress) * req_work_sec;
            let work_capacity = time_budget_s * work_rate;

            if work_capacity >= work_needed {
                let time_used = work_needed / work_rate.max(0.1);
                time_budget_s -= time_used;
                self.s_tilde += lambda_eq * work_needed;
                self.completed_instances.push(self.current_wp_idx);
                self.semi_finished_progress = 0.0;
                self.current_wp_idx += 1;
            } else {
                let frac = work_capacity / req_work_sec;
                self.semi_finished_progress += frac;
                self.s_tilde += lambda_eq * work_capacity;
                break;
            }
        }
    }
}

/// 爆破起爆开关物理实体 (DetonatorSwitch)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetonatorSwitch {
    pub switch_id: u64,
    pub name: String,
    pub team_id: u8,
    pub switch_pos: [f32; 2],
    pub charge_positions: Vec<[f32; 2]>,
    pub wire_polyline: Vec<[f32; 2]>,
    pub wire_laid: bool,
    pub charges_placed: bool,
    pub safe_standoff_m: f32, // 默认 40.0m
    pub detonated: bool,
}

pub type DetonatorSwitchEntity = DetonatorSwitch;

impl DetonatorSwitch {
    pub fn new(
        switch_id: u64,
        team_id: u8,
        switch_pos: [f32; 2],
        charge_positions: Vec<[f32; 2]>,
        wire_polyline: Vec<[f32; 2]>,
    ) -> Self {
        Self {
            switch_id,
            name: format!("Switch-#{switch_id}"),
            team_id,
            switch_pos: quantize_pos_78125(switch_pos),
            charge_positions,
            wire_polyline,
            wire_laid: false,
            charges_placed: false,
            safe_standoff_m: 40.0,
            detonated: false,
        }
    }

    /// 起爆门禁校验：
    /// 1. 装药已安放；2. 导线已连接；3. 工兵已到达安全开关点 (距离所有炸点 >= safe_standoff_m)
    pub fn can_detonate(&self, operator_pos: [f32; 2]) -> Result<(), &'static str> {
        if self.detonated {
            return Err("Switch has already been detonated");
        }
        if !self.charges_placed {
            return Err("Charges have not been placed yet");
        }
        if !self.wire_laid {
            return Err("Detonation wire has not been laid to switch");
        }
        for &ch in &self.charge_positions {
            if operator_pos.distance_to_f32(ch) < self.safe_standoff_m {
                return Err("Operator is within dangerous blast corridor! Evacuation required before detonation");
            }
        }
        Ok(())
    }

    /// 触发起爆
    pub fn trigger_detonation(&mut self, operator_pos: [f32; 2]) -> Result<Vec<[f32; 2]>, &'static str> {
        self.can_detonate(operator_pos)?;
        self.detonated = true;
        Ok(self.charge_positions.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_02_instance_generation_and_78125mm_quantization() {
        // 1. 定点量化为 7.8125mm (0.0078125) 整数倍
        let raw = [10.123456, 25.654321];
        let q = quantize_pos_78125(raw);
        let rem_x = ((q[0] * QUANT_SCALE).round() - (q[0] * QUANT_SCALE)).abs();
        let rem_y = ((q[1] * QUANT_SCALE).round() - (q[1] * QUANT_SCALE)).abs();
        assert!(rem_x < 1e-4 && rem_y < 1e-4);

        // 2. 梅花桩雷区生成
        let poly = vec![[0.0, 0.0], [50.0, 0.0], [50.0, 50.0], [0.0, 50.0]];
        let mines = generate_quincunx_minefield(&poly, 5.0, 12345);
        assert!(!mines.is_empty(), "雷区内必须生成实体");
        for m in &mines {
            assert!(point_in_polygon(*m, &poly), "所有地雷必须在多边形内部");
        }

        // 3. 散兵坑斜接偏置生成
        let line = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
        let foxholes = generate_miter_offset_foxholes(&line, 5.0, 10.0);
        assert!(foxholes.len() >= 15);

        // 4. 锯齿堑壕生成 (A_zig = 1.2m)
        let trench = generate_zigzag_trench(&line, 8.0, 1.2);
        assert!(trench.len() >= 20);

        // 5. 2-Opt 巡回路径优化
        let tour = solve_boustrophedon_2opt(&mines[0..15]);
        assert_eq!(tour.len(), 15);
    }

    #[test]
    fn test_phase_02_augmented_arc_while_moving_and_working() {
        // 单步 16s 内走一段建一段测试
        // 目标：在 (0, 0) 走向 (10, 0)，耗时 = 10m / (2.0m/s) = 5.0s
        // 剩余 11.0s 进行施工：5人作业，额定 40人*秒
        // 能力 = 11s * 5人 = 55人*秒 >= 40人*秒 -> 成功完成第 1 个点并向第 2 个点推进！
        let wps = vec![[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
        let mut integrator = AugmentedArcIntegrator::new(wps);

        integrator.step_16s(2.0, 5, 0.0, 40.0);
        assert!(!integrator.completed_instances.is_empty(), "16s 内应走完第 1 段并完成施工");
        assert!(integrator.s_tilde > 10.0, "增广弧长必须包含移动距离与等效做工");
    }

    #[test]
    fn test_phase_02_interrupted_pinned_and_resume_preserves_work() {
        // 半路被打断保留进度后续接着修测试
        let wps = vec![[5.0, 0.0], [10.0, 0.0]];
        let mut integrator = AugmentedArcIntegrator::new(wps);
        integrator.cur_pos = [0.0, 0.0];

        // 第 1 步：5人作业，额定 100人*秒。16s 内移动 2.5s (5m / 2m/s)，作业 13.5s * 5 = 67.5人*秒 < 100人*秒
        // 未完工，产生半成品 (0.675)
        integrator.step_16s(2.0, 5, 0.0, 100.0);
        let progress_step1 = integrator.semi_finished_progress;
        assert!(progress_step1 > 0.5 && progress_step1 < 1.0, "半成品工事进度必须在 (0.5, 1.0)");

        // 遭遇重度敌火压制 (suppression = 0.85)，进入 PinnedPause，进度完好保留
        integrator.step_16s(2.0, 5, 0.85, 100.0);
        assert_eq!(integrator.fsm_state, ConstructionFsmState::PinnedPause);
        assert_eq!(integrator.semi_finished_progress, progress_step1, "压制中断期间进度必须严格冻结保留");

        // 压制解除 (suppression = 0.0)，恢复施工并完工转正
        integrator.step_16s(2.0, 5, 0.0, 100.0);
        assert_eq!(integrator.fsm_state, ConstructionFsmState::Resumed);
        assert!(integrator.completed_instances.contains(&0), "恢复后必须接着修完转正");
    }

    #[test]
    fn test_phase_02_detonator_switch_safety_corridor_and_arming() {
        // 安放炸药未铺线与安全撤离前禁止起爆测试
        let charge_pt = [50.0, 50.0];
        let switch_pt = [10.0, 10.0]; // 距炸点 sqrt(40^2 + 40^2) = 56.5m > 40m 安全走廊
        let wire = vec![charge_pt, [30.0, 30.0], switch_pt];

        let mut switch = DetonatorSwitch::new(101, 1, switch_pt, vec![charge_pt], wire);

        // 1. 炸药未放置：禁止起爆
        assert!(switch.can_detonate(switch_pt).is_err());

        // 2. 放置炸药，但未铺设导线：禁止起爆
        switch.charges_placed = true;
        assert!(switch.can_detonate(switch_pt).is_err());

        // 3. 铺设导线，但工兵自身仍停留在炸点附近 (距炸点 10m < 40m)：禁止起爆！
        switch.wire_laid = true;
        let unsafe_operator_pos = [55.0, 55.0];
        assert!(switch.can_detonate(unsafe_operator_pos).is_err(), "工兵在危险爆炸走廊内必须禁止起爆");

        // 4. 工兵撤离至开关点 (56.5m >= 40m)：允许起爆！
        assert!(switch.can_detonate(switch_pt).is_ok(), "工兵撤离安全距离后允许起爆");

        let blast_pts = switch.trigger_detonation(switch_pt).unwrap();
        assert_eq!(blast_pts, vec![charge_pt]);
        assert!(switch.detonated);

        // 重复起爆必须拒绝
        assert!(switch.trigger_detonation(switch_pt).is_err());
    }
}
