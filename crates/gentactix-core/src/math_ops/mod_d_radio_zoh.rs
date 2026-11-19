use serde::{Deserialize, Serialize};

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

