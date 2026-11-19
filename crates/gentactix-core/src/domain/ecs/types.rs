//! `units_ecs` module: Contiguous `UnitId(usize)` / `CommanderId(usize)` SoA storage,
//! bitflags `LockFlags`, single-source `ModifierSlot` table, enum `ActiveEvent`,
//! and three-ledger TOE (`enlisted / cadre / equipment`). Zero `meta` string dicts.

use crate::world_grid::PriorityTarget;
use bitflags::bitflags;
use serde::{Deserialize, Serialize};

pub const PERSON_DURABILITY: f32 = 15.0;
pub const SOFT_DURABILITY: f32 = 12.0;
pub const ARMOR_DURABILITY: f32 = 40.0;
pub const ORG_DURABILITY: f32 = 25.0;
pub const ORG_CAP_FLOOR: f32 = 25.0;
pub const CADRE_LOSS_ORG_PENALTY: f32 = 40.0;
pub const DIRECT_MERGE_MAX_PERSONS: u32 = 15;

pub const RADIO_CHANNELS: [&str; 7] = ["A", "B", "C", "D", "E", "F", "G"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommanderId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Echelon {
    Team = 0,
    Squad = 1,
    HalfPlatoon = 2,
    Platoon = 3,
    Company = 4,
    Battalion = 5,
    Regiment = 6,
    Brigade = 7,
    Division = 8,
}

impl Echelon {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "team" | "trupp" | "组" => Echelon::Team,
            "squad" | "gruppe" | "班" => Echelon::Squad,
            "half_platoon" | "halfplatoon" | "halbzug" | "半排" => Echelon::HalfPlatoon,
            "platoon" | "zug" | "排" => Echelon::Platoon,
            "company" | "kompanie" | "连" => Echelon::Company,
            "battalion" | "bataillon" | "营" => Echelon::Battalion,
            "regiment" | "团" => Echelon::Regiment,
            "brigade" | "旅" => Echelon::Brigade,
            "division" | "师" => Echelon::Division,
            _ => Echelon::Squad,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Echelon::Team => "team",
            Echelon::Squad => "squad",
            Echelon::HalfPlatoon => "half_platoon",
            Echelon::Platoon => "platoon",
            Echelon::Company => "company",
            Echelon::Battalion => "battalion",
            Echelon::Regiment => "regiment",
            Echelon::Brigade => "brigade",
            Echelon::Division => "division",
        }
    }

    pub fn default_staff_full(self) -> u32 {
        match self {
            Echelon::Team | Echelon::Squad | Echelon::HalfPlatoon | Echelon::Platoon => 0,
            Echelon::Company => 2,
            Echelon::Battalion => 4,
            Echelon::Regiment => 8,
            Echelon::Brigade => 12,
            Echelon::Division => 24,
        }
    }

    pub fn cadence_ticks(self) -> u32 {
        match self {
            Echelon::Team | Echelon::Squad => 1,
            Echelon::HalfPlatoon => 2,
            Echelon::Platoon => 3,
            Echelon::Company => 6,
            Echelon::Battalion => 12,
            Echelon::Regiment => 36,
            Echelon::Brigade => 48,
            Echelon::Division => 72,
        }
    }

    pub fn propagation_delay_min(self) -> f32 {
        match self {
            Echelon::Division => 30.0,
            Echelon::Brigade => 25.0,
            Echelon::Regiment => 20.0,
            Echelon::Battalion => 10.0,
            Echelon::Company => 5.0,
            Echelon::Platoon | Echelon::HalfPlatoon | Echelon::Squad | Echelon::Team => 2.0,
        }
    }

    pub fn hard_cooldown_min(self) -> f32 {
        match self {
            Echelon::Platoon | Echelon::HalfPlatoon | Echelon::Squad | Echelon::Team => 20.0,
            Echelon::Company => 45.0,
            Echelon::Battalion => 90.0,
            Echelon::Regiment => 180.0,
            Echelon::Brigade => 240.0,
            Echelon::Division => 360.0,
        }
    }

    pub fn default_role_hours(self) -> f32 {
        match self {
            Echelon::Team | Echelon::Squad => 5.0 / 60.0,
            Echelon::HalfPlatoon | Echelon::Platoon => 0.25,
            Echelon::Company => 0.5,
            Echelon::Battalion => 1.0,
            Echelon::Regiment => 2.0,
            Echelon::Brigade => 4.0,
            Echelon::Division => 6.0,
        }
    }

    pub fn reorg_duration_h(self) -> f32 {
        match self {
            Echelon::Team | Echelon::Squad | Echelon::HalfPlatoon | Echelon::Platoon => 0.25,
            Echelon::Company => 1.0,
            Echelon::Battalion => 2.0,
            Echelon::Regiment => 3.0,
            Echelon::Brigade => 4.0,
            Echelon::Division => 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Rank {
    General = 0,
    Colonel = 1,
    LtCol = 2,
    Major = 3,
    Captain = 4,
    Lieutenant = 5,
    Sergeant = 6,
    Corporal = 7,
}

impl Rank {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "general" => Rank::General,
            "colonel" => Rank::Colonel,
            "ltcol" | "lt_col" => Rank::LtCol,
            "major" => Rank::Major,
            "captain" => Rank::Captain,
            "lieutenant" => Rank::Lieutenant,
            "sergeant" => Rank::Sergeant,
            "corporal" => Rank::Corporal,
            _ => Rank::Lieutenant,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Rank::General => "general",
            Rank::Colonel => "colonel",
            Rank::LtCol => "ltcol",
            Rank::Major => "major",
            Rank::Captain => "captain",
            Rank::Lieutenant => "lieutenant",
            Rank::Sergeant => "sergeant",
            Rank::Corporal => "corporal",
        }
    }

    pub fn demotion_next(self) -> Option<Self> {
        match self {
            Rank::General => Some(Rank::Colonel),
            Rank::Colonel => Some(Rank::LtCol),
            Rank::LtCol => Some(Rank::Major),
            Rank::Major => Some(Rank::Captain),
            Rank::Captain => Some(Rank::Lieutenant),
            Rank::Lieutenant => Some(Rank::Sergeant),
            Rank::Sergeant => Some(Rank::Corporal),
            Rank::Corporal => None,
        }
    }

    pub fn base_radius_km(self) -> f32 {
        match self {
            Rank::General => 30.0,
            Rank::Colonel => 18.0,
            Rank::LtCol => 12.0,
            Rank::Major => 6.0,
            Rank::Captain => 3.5,
            Rank::Lieutenant => 1.8,
            Rank::Sergeant => 0.8,
            Rank::Corporal => 0.5,
        }
    }

    pub fn ao_max_area_px(self) -> usize {
        match self {
            Rank::General => 2400,
            Rank::Colonel => 900,
            Rank::LtCol => 450,
            Rank::Major => 160,
            Rank::Captain => 64,
            Rank::Lieutenant => 25,
            Rank::Sergeant => 9,
            Rank::Corporal => 4,
        }
    }

    pub fn ao_max_count(self) -> usize {
        match self {
            Rank::General => 3,
            Rank::Colonel | Rank::LtCol | Rank::Major | Rank::Captain => 2,
            Rank::Lieutenant | Rank::Sergeant | Rank::Corporal => 1,
        }
    }
}

/// 火力强度姿态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FireIntensityMode {
    /// 急速射击：1.0 满负荷打出武器库 base_rpm 理论极限射速
    RapidFire,
    /// 正常射击：采用武器库 combat_rpm 实战射速
    NormalFire,
    /// 试探射击：低速 (0.3 ~ 0.5 * combat_rpm)
    ProbingFire,
    /// 精准射击：特定狙击手/精确射手门禁
    PrecisionFire,
}

impl Default for FireIntensityMode {
    fn default() -> Self {
        FireIntensityMode::NormalFire
    }
}

impl FireIntensityMode {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "rapid" | "rapid_fire" | "rapidfire" => FireIntensityMode::RapidFire,
            "probing" | "probing_fire" | "probingfire" => FireIntensityMode::ProbingFire,
            "precision" | "precision_fire" | "precisionfire" => FireIntensityMode::PrecisionFire,
            _ => FireIntensityMode::NormalFire,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FireIntensityMode::RapidFire => "rapid",
            FireIntensityMode::NormalFire => "normal",
            FireIntensityMode::ProbingFire => "probing",
            FireIntensityMode::PrecisionFire => "precision",
        }
    }
}

/// 射击任务参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireTask {
    pub target: (f32, f32),
    pub duration_sec: u32,
    pub priority_target: PriorityTarget,
    pub fire_mode: FireIntensityMode,
}

/// 任务与事件执行阶段状态机
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskLifecyclePhase {
    /// 1. 启动阶段：前置受领准备期 (收报/传令/散开/架枪，原地警戒，不可机动与开火)
    QueuePrep {
        remaining_sec: u32,
    },
    /// 2. 正常执行阶段：正在推进当前步骤 (消耗本回合时间预算)
    Executing {
        step_idx: usize,
    },
    /// 3. 任务交替摩擦阶段：上个任务刚结清，部队制动/放下背囊/构筑射界
    InterTaskTransition {
        completed_step: usize,
        next_step: usize,
        remaining_friction_sec: u32,
    },
    /// 4. 终止收尾阶段：全队列结清，就地清点伤亡弹药、简易急救、构筑环形防御
    TailConsolidation {
        remaining_sec: u32,
    },
    /// 5. 完工静止状态：正常转入常规就地防御驻留态
    IdleDefend,
    /// 6. 异常终止：被敌军突击击溃、高压制溃退、或被指挥官紧急打断
    Aborted {
        reason: String,
    },
}

impl Default for TaskLifecyclePhase {
    fn default() -> Self {
        TaskLifecyclePhase::IdleDefend
    }
}

/// 单位身上的时序预算调度器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTimingScheduler {
    /// 当前时序所处阶段
    pub phase: TaskLifecyclePhase,
    /// 前置准备时间基准 (班组5s / 排级15s / 连营30s)
    pub base_prep_sec: u32,
    /// 任务间转场固定摩擦基准 (默认 10s~20s)
    pub base_transition_sec: u32,
    /// 队列收尾巩固基准 (默认 15s)
    pub base_tail_sec: u32,
    /// 本步长 (16秒) 剩余可用时间预算 (初始 16s)
    pub turn_budget_remaining_sec: u32,
}

impl TaskTimingScheduler {
    pub const DEFAULT_STEP_SECONDS: u32 = 16;

    pub fn new_for_echelon(echelon: Echelon) -> Self {
        let base_prep_sec = match echelon {
            Echelon::Team | Echelon::Squad => 2,
            Echelon::HalfPlatoon | Echelon::Platoon => 4,
            _ => 8,
        };
        Self {
            phase: TaskLifecyclePhase::IdleDefend,
            base_prep_sec,
            base_transition_sec: 2,
            base_tail_sec: 2,
            turn_budget_remaining_sec: Self::DEFAULT_STEP_SECONDS,
        }
    }

    pub fn reset_turn_budget(&mut self) {
        self.turn_budget_remaining_sec = Self::DEFAULT_STEP_SECONDS;
    }

    pub fn init_queue(&mut self, total_steps: usize) {
        if total_steps == 0 {
            self.phase = TaskLifecyclePhase::IdleDefend;
            return;
        }
        self.phase = TaskLifecyclePhase::QueuePrep {
            remaining_sec: self.base_prep_sec,
        };
    }

    /// Advance scheduling clock with 3-phase transitions & Epsilon threshold check
    pub fn advance_turn_budget(
        &mut self,
        total_steps: usize,
        mut is_step_done: impl FnMut(usize) -> bool,
        mut complete_step: impl FnMut(usize),
    ) {
        while self.turn_budget_remaining_sec > 0 {
            match self.phase.clone() {
                TaskLifecyclePhase::QueuePrep { remaining_sec } => {
                    if self.turn_budget_remaining_sec >= remaining_sec {
                        self.turn_budget_remaining_sec -= remaining_sec;
                        self.phase = TaskLifecyclePhase::Executing { step_idx: 0 };
                    } else {
                        self.phase = TaskLifecyclePhase::QueuePrep {
                            remaining_sec: remaining_sec - self.turn_budget_remaining_sec,
                        };
                        self.turn_budget_remaining_sec = 0;
                        break;
                    }
                }
                TaskLifecyclePhase::Executing { step_idx } => {
                    if is_step_done(step_idx) {
                        complete_step(step_idx);
                        let next = step_idx + 1;
                        if next < total_steps {
                            self.phase = TaskLifecyclePhase::InterTaskTransition {
                                completed_step: step_idx,
                                next_step: next,
                                remaining_friction_sec: self.base_transition_sec,
                            };
                        } else {
                            self.phase = TaskLifecyclePhase::TailConsolidation {
                                remaining_sec: self.base_tail_sec,
                            };
                        }
                    } else {
                        break;
                    }
                }
                TaskLifecyclePhase::InterTaskTransition {
                    completed_step,
                    next_step,
                    remaining_friction_sec,
                } => {
                    if self.turn_budget_remaining_sec >= remaining_friction_sec {
                        self.turn_budget_remaining_sec -= remaining_friction_sec;
                        self.phase = TaskLifecyclePhase::Executing { step_idx: next_step };
                    } else {
                        self.phase = TaskLifecyclePhase::InterTaskTransition {
                            completed_step,
                            next_step,
                            remaining_friction_sec: remaining_friction_sec - self.turn_budget_remaining_sec,
                        };
                        self.turn_budget_remaining_sec = 0;
                        break;
                    }
                }
                TaskLifecyclePhase::TailConsolidation { remaining_sec } => {
                    if self.turn_budget_remaining_sec >= remaining_sec {
                        self.turn_budget_remaining_sec -= remaining_sec;
                        self.phase = TaskLifecyclePhase::IdleDefend;
                    } else {
                        self.phase = TaskLifecyclePhase::TailConsolidation {
                            remaining_sec: remaining_sec - self.turn_budget_remaining_sec,
                        };
                        self.turn_budget_remaining_sec = 0;
                        break;
                    }
                }
                TaskLifecyclePhase::IdleDefend | TaskLifecyclePhase::Aborted { .. } => {
                    break;
                }
            }
        }
    }

    /// Epsilon critical absorption: maneuver < 5m
    #[inline]
    pub fn is_maneuver_epsilon_absorbed(dist_remaining_m: f32) -> bool {
        dist_remaining_m < 5.0
    }

    /// Epsilon critical absorption: fire < 15s or out of ammo
    #[inline]
    pub fn is_fire_epsilon_absorbed(duration_remaining_sec: u32, out_of_ammo: bool) -> bool {
        duration_remaining_sec < 15 || out_of_ammo
    }

    /// Engine never auto-aborts tasks on taking fire. Only abort on death or rout.
    #[inline]
    pub fn notify_combat_stress(&mut self, is_dead: bool, is_routing: bool) {
        if is_dead {
            self.phase = TaskLifecyclePhase::Aborted {
                reason: "unit_destroyed".into(),
            };
        } else if is_routing {
            self.phase = TaskLifecyclePhase::Aborted {
                reason: "morale_collapsed_rout".into(),
            };
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    pub struct LockFlags: u16 {
        const HOLD_STILL    = 1 << 0;
        const FORBID_MOVE   = 1 << 1;
        const FORBID_FIRE   = 1 << 2;
        const FORBID_TOW    = 1 << 3;
        const FORBID_RADIO  = 1 << 4;
        const FORBID_REST   = 1 << 5;
        const FORBID_DEPLOY = 1 << 6;
    }
}

impl LockFlags {
    #[inline]
    pub fn normalized(mut self) -> Self {
        if self.contains(LockFlags::HOLD_STILL) {
            self.insert(LockFlags::FORBID_MOVE);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DamageType {
    AntiPersonnel = 0,
    Suppression = 1,
    AntiSoft = 2,
    AntiArmor = 3,
    Destructive = 4,
    Incendiary = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BallisticGroup {
    DirectSmallArms,
    DirectHeavyMg,
    DirectAntiTank,
    DirectCannon,
    IndirectMortar,
    IndirectHowitzer,
    HeatShaped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AtomicAction {
    Hold = 0,
    Move = 1,
    Attack = 2,
    Pickup = 3,
    Embark = 4,
    Retreat = 5,
}

impl AtomicAction {
    pub fn as_str(self) -> &'static str {
        match self {
            AtomicAction::Hold => "HOLD",
            AtomicAction::Move => "MOVE",
            AtomicAction::Attack => "ATTACK",
            AtomicAction::Pickup => "PICKUP",
            AtomicAction::Embark => "EMBARK",
            AtomicAction::Retreat => "RETREAT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlertMark {
    None = 0,
    RedAutonomous = 1,
    YellowRigid = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitState {
    Mobile = 0,
    Deployed = 1,
    Carried = 2,
}

impl UnitState {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitState::Mobile => "mobile",
            UnitState::Deployed => "deployed",
            UnitState::Carried => "carried",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoeMode {
    FireAtWill,
    ReturnFire,
    HoldFire,
    Ambush,
}

