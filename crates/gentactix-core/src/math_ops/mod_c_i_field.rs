use super::geometry_8m::{CellCoord, GridGeometry};
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

/// 算子 B-1 · 双时间尺度爆炸危害与敌方压制施密特三态姿态机
/// tau_blast = 9.0s -> alpha_blast = 0.1690 (快衰减瞬态爆炸)
/// tau_supp = 36.0s -> alpha_supp = 0.6412 (慢恢复敌方压制)
pub const ALPHA_BLAST: f32 = 0.1690;
pub const ALPHA_SUPP: f32 = 0.6412;

/// 认知层情报清晰度 I_intel 演化参数（半衰期 T_1/2 = 64.0s -> alpha_intel = 0.8409）与 0.05 迟滞显影门限
pub const ALPHA_INTEL: f32 = 0.8409;
pub const INTEL_SUSPECTED_ENTER: f32 = 0.20;
pub const INTEL_SUSPECTED_EXIT: f32 = 0.15;
pub const INTEL_SPOTTED_ENTER: f32 = 0.45;
pub const INTEL_SPOTTED_EXIT: f32 = 0.40;
pub const INTEL_FULL_ENTER: f32 = 0.75;
pub const INTEL_FULL_EXIT: f32 = 0.70;

/// 对数生存概率融合：I(t+dt) = 1 - (1 - prev) * exp(-J)
#[inline]
pub fn fuse_intel_log_survival(prev_intel: f32, j_evidence: f32) -> f32 {
    if j_evidence <= 0.0 {
        return prev_intel.clamp(0.0, 1.0);
    }
    let p = prev_intel.clamp(0.0, 1.0);
    (1.0 - (1.0 - p) * (-j_evidence).exp()).clamp(0.0, 1.0)
}

/// 0.05 迟滞回差死区显影门限转移函数
#[inline]
pub fn transition_vis_tier_hysteresis(
    prev: crate::types::VisTier,
    intel: f32,
) -> crate::types::VisTier {
    use crate::types::VisTier;
    let v = intel.clamp(0.0, 1.0);
    match prev {
        VisTier::Full => {
            if v >= INTEL_FULL_EXIT {
                VisTier::Full
            } else if v >= INTEL_SPOTTED_EXIT {
                VisTier::Spotted
            } else if v >= INTEL_SUSPECTED_EXIT {
                VisTier::Suspected
            } else {
                VisTier::Hidden
            }
        }
        VisTier::Spotted => {
            if v >= INTEL_FULL_ENTER {
                VisTier::Full
            } else if v >= INTEL_SPOTTED_EXIT {
                VisTier::Spotted
            } else if v >= INTEL_SUSPECTED_EXIT {
                VisTier::Suspected
            } else {
                VisTier::Hidden
            }
        }
        VisTier::Suspected => {
            if v >= INTEL_FULL_ENTER {
                VisTier::Full
            } else if v >= INTEL_SPOTTED_ENTER {
                VisTier::Spotted
            } else if v >= INTEL_SUSPECTED_EXIT {
                VisTier::Suspected
            } else {
                VisTier::Hidden
            }
        }
        VisTier::Hidden => {
            if v >= INTEL_FULL_ENTER {
                VisTier::Full
            } else if v >= INTEL_SPOTTED_ENTER {
                VisTier::Spotted
            } else if v >= INTEL_SUSPECTED_ENTER {
                VisTier::Suspected
            } else {
                VisTier::Hidden
            }
        }
    }
}

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

