//! `units_ecs` module: Contiguous `UnitId(usize)` / `CommanderId(usize)` SoA storage,
//! bitflags `LockFlags`, single-source `ModifierSlot` table, enum `ActiveEvent`,
//! and three-ledger TOE (`enlisted / cadre / equipment`). Zero `meta` string dicts.

use crate::world_grid::{
    MobilityClass, PriorityTarget, TeamId, WorldGrid, LINE_MASK_ARMOR, LINE_MASK_ARTY_LOG,
    LINE_MASK_HEAVY_AT, LINE_MASK_INFANTRY, LINE_MASK_RECON,
};
use bitflags::bitflags;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

#[derive(Debug, Clone)]
pub struct EquipmentSpec {
    pub model: &'static str,
    pub name: &'static str,
    pub range_m: f32,
    pub min_range_m: f32,
    pub indirect: bool,
    pub deployable: bool,
    pub towable: bool,
    pub crew: u32,
    pub deploy_hours: f32,
    pub mass_t: f32,
    pub frag_radius_m: f32,
    pub ballistic_group: BallisticGroup,
    pub is_heavy: bool,
    pub is_vehicle: bool,
    pub radio_km: f32,
    pub ammo_type: &'static str,
    pub fire: [f32; 6], // indexed by DamageType as usize
}

pub fn equipment_spec(model: &str) -> &'static EquipmentSpec {
    static RIFLE: EquipmentSpec = EquipmentSpec {
        model: "rifle", name: "步枪", range_m: 400.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 0.004,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [1.0, 0.8, 0.0, 0.0, 0.0, 0.0],
    };
    static SMG: EquipmentSpec = EquipmentSpec {
        model: "smg", name: "冲锋枪", range_m: 200.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 0.004,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [1.6, 1.2, 0.0, 0.0, 0.0, 0.0],
    };
    static LMG: EquipmentSpec = EquipmentSpec {
        model: "lmg", name: "轻机枪", range_m: 800.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.01,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [2.4, 3.0, 0.3, 0.0, 0.0, 0.0],
    };
    static HMG: EquipmentSpec = EquipmentSpec {
        model: "hmg", name: "重机枪", range_m: 1500.0, min_range_m: 0.0, indirect: false,
        deployable: true, towable: false, crew: 3, deploy_hours: 0.5, mass_t: 0.045,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectHeavyMg, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [4.2, 6.0, 1.0, 0.2, 0.0, 0.0],
    };
    static ATR: EquipmentSpec = EquipmentSpec {
        model: "atr", name: "反坦克枪", range_m: 600.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.018,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectAntiTank, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "at",
        fire: [0.2, 0.3, 1.2, 2.5, 0.2, 0.0],
    };
    static BAZOOKA: EquipmentSpec = EquipmentSpec {
        model: "bazooka", name: "反坦克火箭筒", range_m: 250.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.01,
        frag_radius_m: 5.0, ballistic_group: BallisticGroup::HeatShaped, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "at",
        fire: [0.5, 0.5, 2.0, 8.0, 1.5, 0.5],
    };
    static MORTAR_60: EquipmentSpec = EquipmentSpec {
        model: "mortar_60", name: "60mm迫击炮", range_m: 1800.0, min_range_m: 100.0, indirect: true,
        deployable: true, towable: false, crew: 2, deploy_hours: 0.25, mass_t: 0.02,
        frag_radius_m: 18.0, ballistic_group: BallisticGroup::IndirectMortar, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "mortar",
        fire: [3.2, 4.0, 1.2, 0.3, 0.6, 0.0],
    };
    static MORTAR_81: EquipmentSpec = EquipmentSpec {
        model: "mortar_81", name: "81mm迫击炮", range_m: 3000.0, min_range_m: 150.0, indirect: true,
        deployable: true, towable: false, crew: 3, deploy_hours: 0.5, mass_t: 0.06,
        frag_radius_m: 28.0, ballistic_group: BallisticGroup::IndirectMortar, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "mortar",
        fire: [6.0, 7.5, 2.2, 0.5, 1.5, 0.0],
    };
    static MORTAR_120: EquipmentSpec = EquipmentSpec {
        model: "mortar_120", name: "120mm重迫击炮", range_m: 5700.0, min_range_m: 300.0, indirect: true,
        deployable: true, towable: true, crew: 4, deploy_hours: 0.5, mass_t: 0.28,
        frag_radius_m: 42.0, ballistic_group: BallisticGroup::IndirectMortar, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "mortar",
        fire: [11.0, 12.0, 4.0, 1.2, 4.0, 0.0],
    };
    static AT_GUN_50: EquipmentSpec = EquipmentSpec {
        model: "at_gun_50", name: "50mm反坦克炮", range_m: 2000.0, min_range_m: 0.0, indirect: false,
        deployable: true, towable: true, crew: 4, deploy_hours: 0.5, mass_t: 0.9,
        frag_radius_m: 12.0, ballistic_group: BallisticGroup::DirectAntiTank, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "at",
        fire: [1.5, 1.5, 4.0, 10.0, 2.0, 0.0],
    };
    static FLAK_88: EquipmentSpec = EquipmentSpec {
        model: "flak_88", name: "88mm高平两用炮", range_m: 8000.0, min_range_m: 0.0, indirect: false,
        deployable: true, towable: true, crew: 5, deploy_hours: 0.75, mass_t: 5.0,
        frag_radius_m: 25.0, ballistic_group: BallisticGroup::DirectCannon, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [4.0, 4.0, 8.0, 18.0, 5.0, 0.0],
    };
    static HOWITZER_105: EquipmentSpec = EquipmentSpec {
        model: "howitzer_105", name: "105mm榴弹炮", range_m: 11200.0, min_range_m: 500.0, indirect: true,
        deployable: true, towable: true, crew: 6, deploy_hours: 0.75, mass_t: 2.0,
        frag_radius_m: 38.0, ballistic_group: BallisticGroup::IndirectHowitzer, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [12.0, 12.0, 6.0, 2.5, 8.0, 0.0],
    };
    static HOWITZER_152: EquipmentSpec = EquipmentSpec {
        model: "howitzer_152", name: "152mm重榴弹炮", range_m: 17200.0, min_range_m: 800.0, indirect: true,
        deployable: true, towable: true, crew: 8, deploy_hours: 1.0, mass_t: 5.5,
        frag_radius_m: 55.0, ballistic_group: BallisticGroup::IndirectHowitzer, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [20.0, 18.0, 10.0, 5.0, 16.0, 0.0],
    };
    static HOWITZER_203: EquipmentSpec = EquipmentSpec {
        model: "howitzer_203", name: "203mm超重型榴弹炮", range_m: 18000.0, min_range_m: 1000.0, indirect: true,
        deployable: true, towable: true, crew: 8, deploy_hours: 1.5, mass_t: 17.5,
        frag_radius_m: 80.0, ballistic_group: BallisticGroup::IndirectHowitzer, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [35.0, 30.0, 18.0, 12.0, 28.0, 0.0],
    };
    static RADIO: EquipmentSpec = EquipmentSpec {
        model: "radio", name: "野战电台", range_m: 0.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 0.015,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 8.0, ammo_type: "small_arms",
        fire: [0.0; 6],
    };
    static HQ_RADIO: EquipmentSpec = EquipmentSpec {
        model: "hq_radio", name: "指挥部大功率电台", range_m: 0.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.12,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 25.0, ammo_type: "small_arms",
        fire: [0.0; 6],
    };
    static TRUCK: EquipmentSpec = EquipmentSpec {
        model: "truck", name: "卡车", range_m: 0.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 3.5,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: true, radio_km: 0.0, ammo_type: "small_arms",
        fire: [0.0; 6],
    };
    static HALFTRACK: EquipmentSpec = EquipmentSpec {
        model: "halftrack", name: "半履带装甲车", range_m: 1500.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 7.5,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectHeavyMg, is_heavy: false,
        is_vehicle: true, radio_km: 8.0, ammo_type: "small_arms",
        fire: [4.0, 5.0, 1.5, 0.3, 0.2, 0.0],
    };
    static TANK_MEDIUM: EquipmentSpec = EquipmentSpec {
        model: "tank_medium", name: "中型坦克", range_m: 2500.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 4, deploy_hours: 0.0, mass_t: 28.0,
        frag_radius_m: 15.0, ballistic_group: BallisticGroup::DirectCannon, is_heavy: true,
        is_vehicle: true, radio_km: 8.0, ammo_type: "tank",
        fire: [5.0, 5.0, 8.0, 14.0, 5.0, 0.0],
    };
    static TANK_HEAVY: EquipmentSpec = EquipmentSpec {
        model: "tank_heavy", name: "重型坦克", range_m: 3200.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 5, deploy_hours: 0.0, mass_t: 46.0,
        frag_radius_m: 20.0, ballistic_group: BallisticGroup::DirectCannon, is_heavy: true,
        is_vehicle: true, radio_km: 10.0, ammo_type: "tank",
        fire: [6.0, 6.0, 10.0, 22.0, 8.0, 0.0],
    };

    match model {
        "rifle" => return &RIFLE,
        "smg" => return &SMG,
        "lmg" => return &LMG,
        "hmg" => return &HMG,
        "atr" | "at_rifle" => return &ATR,
        "bazooka" => return &BAZOOKA,
        "mortar_60" => return &MORTAR_60,
        "mortar_81" => return &MORTAR_81,
        "mortar_120" => return &MORTAR_120,
        "at_gun_50" => return &AT_GUN_50,
        "flak_88" => return &FLAK_88,
        "howitzer_105" => return &HOWITZER_105,
        "howitzer_152" => return &HOWITZER_152,
        "howitzer_203" | "203mm" | "wpn_203" => return &HOWITZER_203,
        "radio" => return &RADIO,
        "hq_radio" => return &HQ_RADIO,
        "truck" => return &TRUCK,
        "halftrack" => return &HALFTRACK,
        "tank_medium" => return &TANK_MEDIUM,
        "tank_heavy" => return &TANK_HEAVY,
        _ => {}
    }

    // 德国历史装备库 (items_catalog) 映射
    if matches!(
        model,
        "wpn_luger_p08"
            | "wpn_walther_p38"
            | "wpn_walther_ppk"
            | "wpn_leuchtpistole_34"
            | "wpn_kar98k"
            | "wpn_kar98k_zf39"
            | "wpn_g41w"
            | "wpn_g43"
            | "wpn_fg42"
            | "wpn_g33_40"
            | "wpn_stg44"
            | "wpn_vg1_5"
    ) {
        return &RIFLE;
    }

    if matches!(
        model,
        "wpn_mp38_40"
            | "wpn_mp41"
            | "wpn_flammenwerfer_35"
            | "wpn_flammenwerfer_41"
    ) {
        return &SMG;
    }

    if matches!(
        model,
        "wpn_mg34" | "wpn_mg42" | "wpn_mg13" | "wpn_mg15"
    ) {
        return &LMG;
    }

    if matches!(
        model,
        "wpn_mg08"
            | "wpn_flak30_20"
            | "wpn_flak38_20"
            | "wpn_gebirgsflak38_20"
            | "wpn_flakvierling38_20"
            | "wpn_flak18_37"
            | "wpn_flak36_37"
            | "wpn_flak37_37"
            | "wpn_flak43_37"
    ) {
        return &HMG;
    }

    if matches!(model, "wpn_pzb39" | "wpn_grb39") {
        return &ATR;
    }

    if matches!(
        model,
        "wpn_panzerfaust_30_klein"
            | "wpn_panzerfaust_30"
            | "wpn_panzerfaust_60"
            | "wpn_panzerfaust_100"
            | "wpn_panzerschreck_rpzb54"
    ) {
        return &BAZOOKA;
    }

    if matches!(model, "wpn_legrw36") {
        return &MORTAR_60;
    }

    if matches!(model, "wpn_grw34" | "wpn_kzgrw42" | "wpn_nbw35_10cm") {
        return &MORTAR_81;
    }

    if matches!(model, "wpn_nbw40_10cm" | "wpn_grw42_120") {
        return &MORTAR_120;
    }

    if matches!(
        model,
        "wpn_spzb41"
            | "wpn_pak36"
            | "wpn_lepak41"
            | "wpn_pak38"
            | "wpn_pak40"
            | "wpn_pak41_75"
            | "wpn_pak43_cruciform"
            | "wpn_pak43_41"
            | "wpn_pak44_128"
    ) {
        return &AT_GUN_50;
    }

    if matches!(
        model,
        "wpn_flak18_88"
            | "wpn_flak36_88"
            | "wpn_flak37_88"
            | "wpn_flak41_88"
            | "wpn_flak38_105"
            | "wpn_flak39_105"
            | "wpn_flak40_128"
            | "wpn_flakzwilling40_128"
    ) {
        return &FLAK_88;
    }

    if matches!(
        model,
        "wpn_leig18"
            | "wpn_legebig18"
            | "wpn_ig37"
            | "wpn_ig42"
            | "wpn_gebg36_75"
            | "wpn_lefh18"
            | "wpn_lefh18m"
            | "wpn_lefh18_40"
            | "wpn_gebh40_105"
            | "wpn_sig33"
    ) {
        return &HOWITZER_105;
    }

    if matches!(
        model,
        "wpn_sk18_10cm"
            | "wpn_sfh18_15cm"
            | "wpn_sfh36_15cm"
            | "wpn_k18_15cm"
            | "wpn_k18_17cm"
            | "wpn_mrs18_21cm"
            | "wpn_nbw41_15cm"
            | "wpn_nbw42_21cm"
            | "wpn_nbw42_30cm"
            | "wpn_wurfrahmen40"
    ) {
        return &HOWITZER_152;
    }

    if matches!(
        model,
        "comm_feldfu_b" | "comm_torn_fu_d2" | "comm_ff33" | "comm_feldkabel_reel"
    ) {
        return &RADIO;
    }
    if matches!(model, "comm_torn_fu_b1") {
        return &HQ_RADIO;
    }

    if let Some(item) = crate::catalog::global_catalog().get(model) {
        if item.kind == "comm" {
            return &RADIO;
        }
        if let Some(ref w) = item.weapon {
            if w.indirect {
                if w.towable {
                    return &HOWITZER_105;
                } else if w.is_heavy_weapon {
                    return &MORTAR_81;
                } else {
                    return &MORTAR_60;
                }
            }
            if let Some(subcat) = item.attr_str("subcategory") {
                match subcat {
                    "at_gun" | "heavy_at_gun" | "superheavy_at_gun" | "squeeze_bore_at" => return &AT_GUN_50,
                    "heavy_aa_at" | "super_heavy_aa_at" | "heavy_aa_gun" | "super_heavy_aa_gun" | "heavy_cannon" | "super_heavy_cannon" => return &FLAK_88,
                    "field_howitzer" | "heavy_howitzer" | "siege_howitzer" | "heavy_rocket_launcher" | "heavy_rocket_crate" => return &HOWITZER_152,
                    "infantry_gun" | "heavy_infantry_gun" | "mountain_howitzer" | "mountain_infantry_gun" => return &HOWITZER_105,
                    "mortar" => return &MORTAR_81,
                    "heavy_mortar" => return &MORTAR_120,
                    "at_rifle" | "grenade_rifle" => return &ATR,
                    "at_rocket_launcher" | "disposable_at" | "rocket_launcher" => return &BAZOOKA,
                    "gpmg" | "lmg" => return &LMG,
                    "hmg_tripod" | "fortress_twin_aa" | "autocannon_aa" | "quad_autocannon_aa" | "med_autocannon_aa" => return &HMG,
                    "smg" | "flamethrower" => return &SMG,
                    _ => {}
                }
            }
            if w.is_heavy_weapon {
                return &HMG;
            }
        }
    }

    &RIFLE
}

/// 武器基准装填冷却时间（秒）
pub fn base_reload_sec_for_weapon(model: &str) -> f32 {
    let m = model.trim().to_ascii_lowercase();
    match m.as_str() {
        "howitzer_203" | "203mm" | "wpn_203" | "wpn_mrs18_21cm" => 92.0, // 超重型火炮 (80~96s，规格基准92s)
        "howitzer_152" | "152mm" | "wpn_sfh18_15cm" | "wpn_k18_15cm" | "ml20" | "sfh18" => 32.0, // 重加榴炮 (32s / 恰好2步)
        "howitzer_105" | "105mm" | "wpn_lefh18" | "lefh18" | "wpn_sig33" => 16.0, // 轻型榴弹炮 (16s / 恰好1步)
        "mortar_120" | "wpn_granatwerfer42_120" => 8.0,
        "flak_88" | "at_gun_50" => 6.0,
        "mortar_81" | "wpn_grw34_81" => 4.0, // 中迫击炮 (4s / 单步4发)
        "mortar_60" => 3.0,
        "lmg" | "hmg" | "wpn_mg34" | "wpn_mg42" => 2.0, // 机枪点射
        "smg" => 2.0,
        _ => 3.0, // 栓动/半自动步枪单兵基准
    }
}


#[derive(Debug, Clone, Copy)]
pub struct VehicleSpec {
    pub model: &'static str,
    pub mobility: MobilityClass,
    /// Base speed in km/h on cost=1.0 terrain (e.g. truck=24 -> ~12 offroad / ~55 highway).
    pub base_speed_kmh: f32,
    pub road_max_kmh: f32,
    pub min_crew: u32,
    pub is_armored: bool,
}

pub fn vehicle_spec(model: &str) -> Option<VehicleSpec> {
    let m = model.trim().to_ascii_lowercase();
    match m.as_str() {
        "truck" | "opel_blitz" => Some(VehicleSpec {
            model: "truck",
            mobility: MobilityClass::Wheeled,
            base_speed_kmh: 24.0,
            road_max_kmh: 55.0,
            min_crew: 1,
            is_armored: false,
        }),
        "armor_car" | "sdkfz_222" => Some(VehicleSpec {
            model: "armor_car",
            mobility: MobilityClass::Wheeled,
            base_speed_kmh: 26.0,
            road_max_kmh: 65.0,
            min_crew: 2,
            is_armored: true,
        }),
        "halftrack" | "sdkfz_251" => Some(VehicleSpec {
            model: "halftrack",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 22.0,
            road_max_kmh: 48.0,
            min_crew: 1,
            is_armored: true,
        }),
        "tank_light" | "tank_lt" | "pz2" => Some(VehicleSpec {
            model: "tank_light",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 21.0,
            road_max_kmh: 44.0,
            min_crew: 3,
            is_armored: true,
        }),
        "tank_medium" | "tank" | "pz4" => Some(VehicleSpec {
            model: "tank_medium",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 19.0,
            road_max_kmh: 38.0,
            min_crew: 4,
            is_armored: true,
        }),
        "tank_heavy" | "tiger" => Some(VehicleSpec {
            model: "tank_heavy",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 14.0,
            road_max_kmh: 26.0,
            min_crew: 4,
            is_armored: true,
        }),
        _ if equipment_spec(model).is_vehicle => Some(VehicleSpec {
            model: "vehicle_generic",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 18.0,
            road_max_kmh: 40.0,
            min_crew: 1,
            is_armored: true,
        }),
        _ => None,
    }
}

/// Multi-crew heavy weapon bonus: 1 operator suffers solo penalty; 3-4 operators grant sustained suppression/reload buff.
#[inline]
pub fn heavy_crew_buff_mult(crew_count: u32, weapon: &str) -> f32 {
    if crew_count == 0 {
        return 0.0;
    }
    let spec = equipment_spec(weapon);
    if !spec.is_heavy && !spec.deployable && spec.crew <= 1 {
        return 1.0;
    }
    match crew_count {
        1 => 0.70,
        2 => 1.00,
        3 => 1.18,
        _ => 1.32,
    }
}

/// Shared ballistic accuracy decay (no hard range cutoff).
#[inline]
pub fn weapon_accuracy_decay(dist_m: f32, range_m: f32) -> f32 {
    let r = range_m.max(50.0);
    1.0 / (1.0 + (dist_m / r).powi(2))
}

/// Shared ballistic damage decay with CQB small-arms surge & AT close-range penalty.
#[inline]
pub fn weapon_damage_decay(model: &str, dist_m: f32, range_m: f32) -> f32 {
    let spec = equipment_spec(model);
    if dist_m <= 50.0 {
        return match model {
            "rifle" | "smg" | "lmg" => 2.0,
            "at_gun_50" | "flak_88" => 0.15,
            _ => 1.0,
        };
    }
    match spec.ballistic_group {
        BallisticGroup::IndirectMortar | BallisticGroup::IndirectHowitzer | BallisticGroup::HeatShaped => 1.0,
        _ => (-0.65 * (dist_m - 50.0).max(0.0) / range_m.max(50.0)).exp(),
    }
}

/// 3-anchor power-law fire registration curve (50m->0.60, 300m->0.20, 3000m->0.01/240).
pub fn compute_registration_delta(weapon: &str, dist_m: f32, duration_min: f32) -> (f32, f32) {
    let range_m = equipment_spec(weapon).range_m.max(50.0);
    let d_eq = (dist_m * 400.0 / range_m).max(1.0);
    let rate_per_min = if d_eq <= 50.0 {
        0.60 / 15.0
    } else if d_eq <= 300.0 {
        let t = (d_eq - 50.0) / 250.0;
        let p50 = 0.60_f32;
        let p300 = 0.20_f32;
        (p50 + t * (p300 - p50)) / 15.0
    } else {
        let ratio = (300.0 / d_eq).clamp(0.0, 1.0);
        let p300 = 0.20 / 15.0;
        let p3000 = 0.01 / 240.0;
        let k = (ratio.powf(1.6)).max(p3000 / p300);
        (p300 * k).max(p3000 * (3000.0 / d_eq).min(1.0))
    };
    let delta = rate_per_min * duration_min.max(1.0);
    let cap = if d_eq <= 300.0 {
        0.60
    } else {
        (0.60 * (300.0 / d_eq).powf(1.6)).min(0.60)
    };
    (delta, cap)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModifierSource {
    OrderChurn,
    WarCouncilSync,
    OverloadAxleSnap,
    OverloadGearbox,
    OverloadEngineOverheat,
    MineDamaged,
    RapidMove,
    ResupplyPickup,
    EventWork,
    ReorganizeEchelon,
    CellSuppression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierSlot {
    pub source: ModifierSource,
    pub tag: &'static str,
    pub locks: LockFlags,
    pub speed_mult: f32,
    pub fuel_mult: f32,
    pub combat_power_mult: f32,
    pub recon_mult: f32,
    pub kc_mult: f32,
    pub tau_mult: f32,
    pub shock_m_add: f32,
    pub org_delta_per_h: f32,
    pub exposure_delta: f32,
    pub ttl_hours: Option<f32>,
}

impl Default for ModifierSlot {
    fn default() -> Self {
        Self {
            source: ModifierSource::EventWork,
            tag: "NEUTRAL",
            locks: LockFlags::empty(),
            speed_mult: 1.0,
            fuel_mult: 1.0,
            combat_power_mult: 1.0,
            recon_mult: 1.0,
            kc_mult: 1.0,
            tau_mult: 1.0,
            shock_m_add: 0.0,
            org_delta_per_h: 0.0,
            exposure_delta: 0.0,
            ttl_hours: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedModifiers {
    pub locks: LockFlags,
    pub speed_mult: f32,
    pub fuel_mult: f32,
    pub combat_power_mult: f32,
    pub recon_mult: f32,
    pub kc_mult: f32,
    pub tau_mult: f32,
    pub shock_m_add: f32,
    pub org_delta_per_h: f32,
    pub exposure_delta: f32,
    pub tags: Vec<&'static str>,
}

impl AggregatedModifiers {
    pub fn neutral() -> Self {
        Self {
            locks: LockFlags::empty(),
            speed_mult: 1.0,
            fuel_mult: 1.0,
            combat_power_mult: 1.0,
            recon_mult: 1.0,
            kc_mult: 1.0,
            tau_mult: 1.0,
            shock_m_add: 0.0,
            org_delta_per_h: 0.0,
            exposure_delta: 0.0,
            tags: Vec::new(),
        }
    }
}

pub fn set_modifier_slot(slots: &mut Vec<ModifierSlot>, slot: ModifierSlot) {
    if let Some(ex) = slots.iter_mut().find(|s| s.source == slot.source) {
        *ex = slot;
    } else {
        slots.push(slot);
    }
}

pub fn aggregate_modifiers(slots: &[ModifierSlot], churn_score: f32, war_council_sync_active: bool) -> AggregatedModifiers {
    let mut agg = AggregatedModifiers::neutral();
    for s in slots {
        agg.locks |= s.locks;
        agg.speed_mult *= s.speed_mult;
        agg.fuel_mult *= s.fuel_mult;
        agg.combat_power_mult *= s.combat_power_mult;
        agg.recon_mult *= s.recon_mult;
        agg.kc_mult *= s.kc_mult;
        agg.tau_mult *= s.tau_mult;
        agg.shock_m_add += s.shock_m_add;
        agg.org_delta_per_h += s.org_delta_per_h;
        agg.exposure_delta += s.exposure_delta;
        agg.tags.push(s.tag);
    }

    // Order Churn Debuff + War Council Sync coexistence (never deletes ORDER_CHURN_DEBUFF!)
    let mut churn_speed = 1.0f32;
    let mut churn_power = 1.0f32;
    let mut churn_org_dh = 0.0f32;
    if churn_score > 1.0 {
        let s = churn_score - 1.0;
        churn_speed = (1.0 - 0.10 * s).max(0.55);
        churn_power = (1.0 - 0.08 * s).max(0.60);
        churn_org_dh = -1.5 * s;
        agg.speed_mult *= churn_speed;
        agg.combat_power_mult *= churn_power;
        agg.org_delta_per_h += churn_org_dh;
        agg.kc_mult *= (-0.25 * s).exp().max(0.30);
        agg.tau_mult *= 1.0 + 0.45 * s;
        agg.shock_m_add += (0.08 * s).min(0.45);
        agg.tags.push("ORDER_CHURN_DEBUFF");
        agg.tags.push("ORDER_CONFUSION");
    }

    if war_council_sync_active {
        let comp_speed = (1.0 / churn_speed.max(0.1)) * 1.08;
        let comp_power = (1.0 / churn_power.max(0.1)) * 1.10;
        let comp_org_dh = if churn_org_dh < 0.0 { -churn_org_dh + 1.0 } else { 1.0 };
        agg.speed_mult *= comp_speed;
        agg.combat_power_mult *= comp_power;
        agg.org_delta_per_h += comp_org_dh;
        agg.kc_mult = 1.15;
        agg.tau_mult = 0.75;
        agg.shock_m_add = 0.0;
        agg.tags.push("WAR_COUNCIL_SYNC_BUFF");
    }

    agg.locks = agg.locks.normalized();
    agg
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgTargetSpec {
    pub str_id: String,
    pub name: String,
    pub echelon: Echelon,
    pub enlisted: u32,
    pub cadre: u32,
    pub equipment: Vec<(String, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    RapidMove,
    ResupplyPickup {
        food_t: f32,
        ammo_t: f32,
        fuel_t: f32,
    },
    Demolition {
        target_px: (i32, i32),
        target_type: String,
    },
    DeployHeavyUnit {
        weapon: String,
        keep_manned: bool,
        operator_soldier_ids: Vec<String>,
        operator_count: u32,
    },
    PackHeavyUnit {
        target_unit_id: UnitId,
        attach: bool,
    },
    OperateHeavyUnit {
        target_unit_id: UnitId,
    },
    RepairDebuff {
        target_unit_id: UnitId,
    },
    FireWeapon {
        weapon: String,
        target_px: (i32, i32),
        radius_px: f32,
    },
    RegisterFire {
        weapon: String,
        target_px: (i32, i32),
        radius_px: f32,
    },
    Engineering {
        op: String,
        target_px: (i32, i32),
    },
    RadioDecoy {
        target_px: (i32, i32),
        ttl_h: f32,
    },
    FormCommandSquad {
        preferred_name: String,
        dissolve_if_empty: bool,
    },
    LayWire {
        target_px: (i32, i32),
    },
    CargoTransfer {
        target_unit_id: UnitId,
        food_t: f32,
        ammo_t: f32,
        fuel_t: f32,
        equipment: Vec<(String, u32)>,
    },
    ReorganizeEchelon {
        commander_id: CommanderId,
        commander_piece_id: Option<UnitId>,
        target_echelon: Echelon,
        source_unit_ids: Vec<UnitId>,
        target_specs: Vec<ReorgTargetSpec>,
        strict_cadre_allocation: bool,
    },
}

impl EventKind {
    pub fn locks(&self) -> LockFlags {
        match self {
            EventKind::RapidMove => LockFlags::FORBID_REST | LockFlags::FORBID_DEPLOY,
            EventKind::ResupplyPickup { .. } => LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE | LockFlags::FORBID_REST,
            EventKind::Demolition { .. }
            | EventKind::DeployHeavyUnit { .. }
            | EventKind::PackHeavyUnit { .. }
            | EventKind::OperateHeavyUnit { .. }
            | EventKind::RepairDebuff { .. }
            | EventKind::FireWeapon { .. }
            | EventKind::RegisterFire { .. }
            | EventKind::Engineering { .. }
            | EventKind::FormCommandSquad { .. }
            | EventKind::LayWire { .. }
            | EventKind::CargoTransfer { .. }
            | EventKind::ReorganizeEchelon { .. } => {
                LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE | LockFlags::FORBID_REST | LockFlags::FORBID_DEPLOY
            }
            EventKind::RadioDecoy { .. } => LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE,
        }
    }

    pub fn combat_power_mult(&self) -> f32 {
        match self {
            EventKind::RapidMove => 0.75,
            EventKind::RepairDebuff { .. } | EventKind::Engineering { .. } => 0.5,
            EventKind::LayWire { .. } | EventKind::CargoTransfer { .. } | EventKind::FormCommandSquad { .. } => 0.6,
            EventKind::ReorganizeEchelon { .. } => 0.8,
            EventKind::RegisterFire { .. } => 0.85,
            _ => 1.0,
        }
    }

    pub fn speed_mult(&self) -> f32 {
        match self {
            EventKind::RapidMove => 1.4,
            EventKind::ReorganizeEchelon { .. } => 0.0,
            _ => 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveEvent {
    pub id: u64,
    pub actor_id: UnitId,
    pub remaining_hours: f32,
    pub labor_persons: u32,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CounterEvaluation {
    pub restricted_echelon: Echelon,
    pub org_cap_limit: f32,
    pub empty_cadre: bool,
    pub recovery_rate_mult: f32,
    pub display_tier_bonus: i8,
    pub mobility: MobilityClass,
}

#[inline]
pub fn infer_nco_officer(kind: &str, echelon: Echelon, cadre: u32) -> (u32, u32) {
    if cadre == 0 {
        return (0, 0);
    }
    let kl = kind.to_ascii_lowercase();
    let is_hq = kl.contains("hq") || kl.contains("cmd") || kl.contains("command");
    if is_hq || (echelon as u8) >= (Echelon::Platoon as u8) {
        if cadre == 1 {
            (0, 1)
        } else {
            let off = if is_hq { (cadre / 2).max(1) } else { (cadre / 3).max(1) };
            (cadre - off, off)
        }
    } else {
        (cadre, 0)
    }
}

pub fn evaluate_counter_and_caps(
    enlisted: u32,
    cadre: u32,
    equipment: &FxHashMap<String, u32>,
    declared_echelon: Echelon,
) -> CounterEvaluation {
    let (nco, officer) = infer_nco_officer("", declared_echelon, cadre);
    evaluate_counter_and_caps_4ledger(enlisted, nco, officer, equipment, declared_echelon)
}

pub fn evaluate_counter_and_caps_4ledger(
    enlisted: u32,
    nco: u32,
    officer: u32,
    equipment: &FxHashMap<String, u32>,
    declared_echelon: Echelon,
) -> CounterEvaluation {
    let cadre = nco + officer;
    let total = enlisted + cadre;
    let empty_cadre = cadre == 0 && total > 0;
    let has_vehicle = equipment.iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle);
    let has_armor = equipment.get("tank_medium").copied().unwrap_or(0) > 0
        || equipment.get("tank_heavy").copied().unwrap_or(0) > 0;
    let has_heavy = equipment.iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_heavy);

    let max_by_manpower = if total <= 15 {
        Echelon::Squad
    } else if total <= 55 {
        Echelon::Platoon
    } else if total <= 220 {
        Echelon::Company
    } else if total <= 900 {
        Echelon::Battalion
    } else if total <= 3000 {
        Echelon::Regiment
    } else if total <= 8000 {
        Echelon::Brigade
    } else {
        Echelon::Division
    };

    let max_by_cadre = if cadre == 0 {
        Echelon::Squad
    } else if officer == 0 {
        // 士官代理上限：最高仅允许排级临时指挥
        Echelon::Platoon
    } else if cadre <= 2 {
        Echelon::Platoon
    } else if cadre <= 8 {
        Echelon::Company
    } else {
        Echelon::Division
    };

    let restricted = if (declared_echelon as u8) <= (max_by_manpower as u8) {
        declared_echelon
    } else {
        max_by_manpower
    };

    let mut org_cap_limit: f32 = if empty_cadre { ORG_CAP_FLOOR } else { 100.0 };
    if total > 0 && (restricted as u8) > (max_by_cadre as u8) {
        org_cap_limit = org_cap_limit.min(65.0);
    }

    let recovery_rate_mult = if empty_cadre {
        0.20
    } else if cadre >= 2 {
        1.15
    } else {
        1.0
    };

    let mobility = if has_armor || equipment.get("halftrack").copied().unwrap_or(0) > 0 {
        MobilityClass::Tracked
    } else if has_vehicle {
        MobilityClass::Wheeled
    } else {
        MobilityClass::Foot
    };

    let display_tier_bonus = if has_heavy || has_armor { 1 } else { 0 };

    CounterEvaluation {
        restricted_echelon: restricted,
        org_cap_limit,
        empty_cadre,
        recovery_rate_mult,
        display_tier_bonus,
        mobility,
    }
}

#[derive(Debug, Clone)]
pub struct UnitSoA {
    pub id: Vec<UnitId>,
    pub str_id: Vec<String>,
    pub name: Vec<String>,
    pub team: Vec<TeamId>,
    pub alive: Vec<bool>,
    pub echelon: Vec<Echelon>,
    pub restricted_echelon: Vec<Echelon>,
    pub kind: Vec<String>,
    pub unit_mask: Vec<u8>,
    pub mobility: Vec<MobilityClass>,

    pub pos: Vec<[f32; 2]>,
    pub px: Vec<(i32, i32)>,
    pub facing_deg: Vec<f32>,
    pub tactical_face_vec: Vec<[f32; 2]>,
    pub moved_last_turn: Vec<bool>,
    pub footprint_px: Vec<u8>,

    // Four-ledger TOE: enlisted / nco / officer / equipment (cadre = nco + officer kept synced)
    pub enlisted: Vec<u32>,
    pub nco: Vec<u32>,
    pub officer: Vec<u32>,
    pub cadre: Vec<u32>,
    pub enlisted_full: Vec<u32>,
    pub nco_full: Vec<u32>,
    pub officer_full: Vec<u32>,
    pub cadre_full: Vec<u32>,
    pub org: Vec<f32>,
    pub org_cap: Vec<f32>,
    pub recovery_mult: Vec<f32>,
    pub empty_cadre: Vec<bool>,
    pub can_operate_radio: Vec<bool>,
    pub heavy_op_cap: Vec<f32>,
    pub is_acting_nco_cmd: Vec<bool>,
    pub retreat_recovery_hours: Vec<f32>,
    pub carry_loss: Vec<f32>,
    pub carry_attr: Vec<f32>,
    pub equipment: Vec<FxHashMap<String, u32>>,
    pub equipment_full: Vec<FxHashMap<String, u32>>,

    // Heavy weapon / vehicle state
    pub vehicle_model: Vec<Option<String>>,
    pub deployed_weapon: Vec<Option<String>>,
    pub unit_state: Vec<UnitState>,
    pub attached_to: Vec<Option<UnitId>>,
    pub operated_by: Vec<Option<UnitId>>,
    pub assigned_operator_count: Vec<u32>,
    pub assigned_operator_soldiers: Vec<Vec<String>>,
    pub overload_ratio: Vec<f32>,
    pub display_tier_bonus: Vec<i8>,

    // Comms & ROE
    pub radio_channel: Vec<u8>,
    pub radio_silence: Vec<bool>,
    pub control_radius_km: Vec<f32>,
    pub roe: Vec<RoeMode>,

    // Logistics stocks
    pub food_t: Vec<f32>,
    pub ammo_t: Vec<f32>,
    pub fuel_t: Vec<f32>,
    pub replacements: Vec<f32>,
    pub cap_food_t: Vec<f32>,
    pub cap_ammo_t: Vec<f32>,
    pub cap_fuel_t: Vec<f32>,

    // Strong-typed autonomy & hierarchy fields (replaced Python piece.meta!)
    pub direct_commander_id: Vec<Option<CommanderId>>,
    pub active_deploy_line_id: Vec<u32>,
    pub autonomy_state: Vec<AtomicAction>,
    pub autonomy_dir: Vec<[f32; 2]>,
    pub autonomy_deployed: Vec<bool>,
    pub exclamation_buff: Vec<AlertMark>,
    pub k_c: Vec<f32>,
    pub was_disconnected: Vec<bool>,
    pub alpha_stealth: Vec<f32>,
    pub exposed: Vec<bool>,
    pub delta_persons_5m: Vec<i32>,
    pub delta_org_5m: Vec<f32>,
    pub planned_route: Vec<Vec<(i32, i32)>>,
    pub reorganizing_event_id: Vec<Option<u64>>,
    pub modifier_slots: Vec<Vec<ModifierSlot>>,

    // Skin customization
    pub skin_color: Vec<String>,
    pub skin_badge: Vec<String>,
    pub skin_label: Vec<Option<String>>,

    // Individual soldier HP & carried ground bodies (wounded/KIA)
    pub soldiers: Vec<Vec<SoldierState>>,
    pub carried_bodies: Vec<Vec<GroundBody>>,
    pub task_scheduler: Vec<TaskTimingScheduler>,

    pub str_to_id: FxHashMap<String, UnitId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoldierItem {
    pub kind: String,  // "weapon" | "radio" | "ammo" | "ration" | "gear"
    pub model: String, // e.g. "rifle", "lmg", "radio", "ammo", "ration"
    pub name: String,
    pub count: u32,
    pub ammo_t: f32,
    pub slot: String,  // "hand" | "pack"
    // Reserved Men-of-War style weight & 2D grid dimensions (not enforced yet)
    #[serde(default)]
    pub weight_kg: f32,
    #[serde(default = "default_grid_dim")]
    pub grid_w: u8,
    #[serde(default = "default_grid_dim")]
    pub grid_h: u8,
    #[serde(default)]
    pub grid_x: u8,
    #[serde(default)]
    pub grid_y: u8,
}

fn default_grid_dim() -> u8 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoldierState {
    pub id: String,
    pub name: String,
    pub hp: f32,
    pub max_hp: f32,
    pub status: String, // "healthy" | "light" | "heavy" | "dead"
    pub weapon: String,
    pub ammo_t: f32,
    pub is_cadre: bool,
    #[serde(default = "default_soldier_role")]
    pub role: String, // "enlisted" | "nco" | "officer"
    pub items: Vec<SoldierItem>,
    // Hand occupancy flags: >= 2 items in hand forbids firing & operating heavy weapons
    #[serde(default)]
    pub cannot_fire: bool,
    #[serde(default)]
    pub cannot_operate_heavy: bool,
    // Reserved separate limits: hand weight does NOT count toward pack weight
    #[serde(default)]
    pub max_hand_weight_kg: Option<f32>,
    #[serde(default)]
    pub max_pack_weight_kg: Option<f32>,
    #[serde(default = "default_pack_cols")]
    pub pack_cols: u8,
    #[serde(default = "default_pack_rows")]
    pub pack_rows: u8,
}

fn default_soldier_role() -> String { "enlisted".to_string() }
fn default_pack_cols() -> u8 { 8 }
fn default_pack_rows() -> u8 { 6 }

impl SoldierState {
    pub fn hand_weight_kg(&self) -> f32 {
        self.items.iter().filter(|it| it.slot == "hand").map(|it| it.weight_kg * (it.count.max(1) as f32)).sum()
    }

    pub fn pack_weight_kg(&self) -> f32 {
        self.items.iter().filter(|it| it.slot != "hand").map(|it| it.weight_kg * (it.count.max(1) as f32)).sum()
    }

    pub fn can_place_in_pack(&self, gw: u8, gh: u8, gx: u8, gy: u8) -> bool {
        if gx + gw > self.pack_cols || gy + gh > self.pack_rows {
            return false;
        }
        for it in &self.items {
            if it.slot == "hand" { continue; }
            let overlap_x = gx < it.grid_x + it.grid_w && gx + gw > it.grid_x;
            let overlap_y = gy < it.grid_y + it.grid_h && gy + gh > it.grid_y;
            if overlap_x && overlap_y {
                return false;
            }
        }
        true
    }

    pub fn refresh_hand_occupancy_flags(&mut self) {
        let hand_n = self.items.iter().filter(|it| it.slot == "hand" && it.count > 0).count();
        self.cannot_fire = hand_n >= 2;
        self.cannot_operate_heavy = hand_n >= 2;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundBody {
    pub id: String,
    pub kind: String, // "wounded" (red skull) | "kia" (black skull)
    pub soldier: SoldierState,
    pub origin_unit_id: String,
    pub origin_unit_name: String,
    pub team: TeamId,
    pub pos: [f32; 2],
    pub looted: bool,
}

impl Default for UnitSoA {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitSoA {
    pub fn new() -> Self {
        Self {
            id: Vec::new(),
            str_id: Vec::new(),
            name: Vec::new(),
            team: Vec::new(),
            alive: Vec::new(),
            echelon: Vec::new(),
            restricted_echelon: Vec::new(),
            kind: Vec::new(),
            unit_mask: Vec::new(),
            mobility: Vec::new(),
            pos: Vec::new(),
            px: Vec::new(),
            facing_deg: Vec::new(),
            tactical_face_vec: Vec::new(),
            moved_last_turn: Vec::new(),
            footprint_px: Vec::new(),
            enlisted: Vec::new(),
            nco: Vec::new(),
            officer: Vec::new(),
            cadre: Vec::new(),
            enlisted_full: Vec::new(),
            nco_full: Vec::new(),
            officer_full: Vec::new(),
            cadre_full: Vec::new(),
            org: Vec::new(),
            org_cap: Vec::new(),
            recovery_mult: Vec::new(),
            empty_cadre: Vec::new(),
            can_operate_radio: Vec::new(),
            heavy_op_cap: Vec::new(),
            is_acting_nco_cmd: Vec::new(),
            retreat_recovery_hours: Vec::new(),
            carry_loss: Vec::new(),
            carry_attr: Vec::new(),
            equipment: Vec::new(),
            equipment_full: Vec::new(),
            vehicle_model: Vec::new(),
            deployed_weapon: Vec::new(),
            unit_state: Vec::new(),
            attached_to: Vec::new(),
            operated_by: Vec::new(),
            assigned_operator_count: Vec::new(),
            assigned_operator_soldiers: Vec::new(),
            overload_ratio: Vec::new(),
            display_tier_bonus: Vec::new(),
            radio_channel: Vec::new(),
            radio_silence: Vec::new(),
            control_radius_km: Vec::new(),
            roe: Vec::new(),
            food_t: Vec::new(),
            ammo_t: Vec::new(),
            fuel_t: Vec::new(),
            replacements: Vec::new(),
            cap_food_t: Vec::new(),
            cap_ammo_t: Vec::new(),
            cap_fuel_t: Vec::new(),
            direct_commander_id: Vec::new(),
            active_deploy_line_id: Vec::new(),
            autonomy_state: Vec::new(),
            autonomy_dir: Vec::new(),
            autonomy_deployed: Vec::new(),
            exclamation_buff: Vec::new(),
            k_c: Vec::new(),
            was_disconnected: Vec::new(),
            alpha_stealth: Vec::new(),
            exposed: Vec::new(),
            delta_persons_5m: Vec::new(),
            delta_org_5m: Vec::new(),
            planned_route: Vec::new(),
            reorganizing_event_id: Vec::new(),
            modifier_slots: Vec::new(),
            skin_color: Vec::new(),
            skin_badge: Vec::new(),
            skin_label: Vec::new(),
            soldiers: Vec::new(),
            carried_bodies: Vec::new(),
            task_scheduler: Vec::new(),
            str_to_id: FxHashMap::default(),
        }
    }

    pub fn spawn_unit(
        &mut self,
        str_id: impl Into<String>,
        name: impl Into<String>,
        team: TeamId,
        px: (i32, i32),
        echelon: Echelon,
        kind: &str,
        enlisted: u32,
        cadre: u32,
        equipment: FxHashMap<String, u32>,
        radio_channel: u8,
    ) -> UnitId {
        let (nco, officer) = infer_nco_officer(kind, echelon, cadre);
        self.spawn_unit_4ledger(str_id, name, team, px, echelon, kind, enlisted, nco, officer, equipment, radio_channel)
    }

    pub fn spawn_unit_4ledger(
        &mut self,
        str_id: impl Into<String>,
        name: impl Into<String>,
        team: TeamId,
        px: (i32, i32),
        echelon: Echelon,
        kind: &str,
        enlisted: u32,
        nco: u32,
        officer: u32,
        equipment: FxHashMap<String, u32>,
        radio_channel: u8,
    ) -> UnitId {
        let uid = UnitId(self.id.len());
        let s_id = str_id.into();
        let cadre = nco + officer;
        let eval = evaluate_counter_and_caps_4ledger(enlisted, nco, officer, &equipment, echelon);
        let mask = classify_unit_mask(kind, &equipment);
        let persons = (enlisted + cadre) as f32;
        let has_veh = equipment.iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle);
        let cap_f = (persons * 0.015).max(0.15);
        let cap_a = (persons * 0.03 + equipment.values().sum::<u32>() as f32 * 0.05).max(0.25);
        let cap_g = if has_veh { 1.2 } else { 0.05 };

        self.str_to_id.insert(s_id.clone(), uid);
        self.id.push(uid);
        self.str_id.push(s_id);
        self.name.push(name.into());
        self.team.push(team);
        self.alive.push(true);
        self.echelon.push(echelon);
        self.restricted_echelon.push(eval.restricted_echelon);
        self.kind.push(kind.to_string());
        self.unit_mask.push(mask);
        self.mobility.push(eval.mobility);
        self.pos.push([px.0 as f32, px.1 as f32]);
        self.px.push(px);
        self.facing_deg.push(match team {
            TeamId::Red => 90.0,
            TeamId::Blue => 270.0,
        });
        self.tactical_face_vec.push(match team {
            TeamId::Red => [0.0, 1.0],
            TeamId::Blue => [0.0, -1.0],
        });
        self.moved_last_turn.push(false);
        self.footprint_px.push(1);
        self.enlisted.push(enlisted);
        self.nco.push(nco);
        self.officer.push(officer);
        self.cadre.push(cadre);
        self.enlisted_full.push(enlisted);
        self.nco_full.push(nco);
        self.officer_full.push(officer);
        self.cadre_full.push(cadre);
        self.org.push(eval.org_cap_limit);
        self.org_cap.push(eval.org_cap_limit);
        self.recovery_mult.push(eval.recovery_rate_mult);
        self.empty_cadre.push(eval.empty_cadre);
        self.can_operate_radio.push(cadre > 0);
        let veh = equipment.iter().find(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle).map(|(m, _)| m.clone());
        let heavy_w = if equipment.len() == 1 {
            equipment.iter().find(|(m, &n)| n > 0 && equipment_spec(m).deployable).map(|(m, _)| m.clone())
        } else {
            None
        };
        self.heavy_op_cap.push(heavy_operator_efficiency_4ledger(
            nco,
            officer,
            enlisted + cadre,
            heavy_w.as_deref().unwrap_or("rifle"),
        ));
        self.is_acting_nco_cmd.push(officer == 0 && nco > 0);
        self.retreat_recovery_hours.push(0.0);
        self.carry_loss.push(0.0);
        self.carry_attr.push(0.0);
        self.equipment_full.push(equipment.clone());
        self.equipment.push(equipment);
        self.vehicle_model.push(veh);
        self.deployed_weapon.push(heavy_w);
        self.unit_state.push(UnitState::Mobile);
        self.attached_to.push(None);
        self.operated_by.push(None);
        self.assigned_operator_count.push(0);
        self.assigned_operator_soldiers.push(Vec::new());
        self.overload_ratio.push(1.0);
        self.display_tier_bonus.push(eval.display_tier_bonus);
        self.radio_channel.push(radio_channel);
        self.radio_silence.push(false);
        self.control_radius_km.push(1.5);
        self.roe.push(RoeMode::ReturnFire);
        self.food_t.push(cap_f);
        self.ammo_t.push(cap_a);
        self.fuel_t.push(cap_g);
        self.replacements.push(0.0);
        self.cap_food_t.push(cap_f);
        self.cap_ammo_t.push(cap_a);
        self.cap_fuel_t.push(cap_g);
        self.direct_commander_id.push(None);
        self.active_deploy_line_id.push(0);
        self.autonomy_state.push(AtomicAction::Hold);
        self.autonomy_dir.push([0.0, 0.0]);
        self.autonomy_deployed.push(false);
        self.exclamation_buff.push(AlertMark::None);
        self.k_c.push(1.20);
        self.was_disconnected.push(false);
        self.alpha_stealth.push(if kind == "recon_squad" { 0.85 } else { 0.35 });
        self.exposed.push(false);
        self.delta_persons_5m.push(0);
        self.delta_org_5m.push(0.0);
        self.planned_route.push(Vec::new());
        self.reorganizing_event_id.push(None);
        self.modifier_slots.push(Vec::new());
        self.skin_color.push(match team {
            TeamId::Red => "#ef4444".to_string(),
            TeamId::Blue => "#3b82f6".to_string(),
        });
        self.skin_badge.push("default".to_string());
        self.skin_label.push(None);
        let mut w_pool: Vec<String> = Vec::new();
        let mut eq_keys: Vec<String> = self.equipment[uid.0].keys().cloned().collect();
        eq_keys.sort();
        for m in eq_keys {
            let cnt = *self.equipment[uid.0].get(&m).unwrap_or(&0);
            if !equipment_spec(&m).is_vehicle {
                for _ in 0..cnt {
                    w_pool.push(m.clone());
                }
            }
        }
        let total_p = (enlisted + cadre).max(1) as f32;
        let per_sol_ammo = cap_a / total_p;
        let per_sol_food = cap_f / total_p;
        let mut sol_vec = Vec::with_capacity((enlisted + cadre) as usize);
        let mut idx_seq = 0usize;
        let build_items = |w: &str, ammo_q: f32, food_q: f32| -> Vec<SoldierItem> {
            let mut list = Vec::new();
            if w == "radio" {
                list.push(SoldierItem {
                    kind: "radio".to_string(),
                    model: "radio".to_string(),
                    name: "背负电台".to_string(),
                    count: 1,
                    ammo_t: 0.0,
                    slot: "pack".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 0, grid_y: 0,
                });
            } else if !w.is_empty() {
                let wname = match w {
                    "lmg" => "轻机枪",
                    "rifle" => "自动步枪",
                    "mortar_60" => "60mm迫击炮",
                    _ => w,
                };
                list.push(SoldierItem {
                    kind: "weapon".to_string(),
                    model: w.to_string(),
                    name: wname.to_string(),
                    count: 1,
                    ammo_t: 0.0,
                    slot: "hand".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 0, grid_y: 0,
                });
            }
            if ammo_q > 0.0 {
                list.push(SoldierItem {
                    kind: "ammo".to_string(),
                    model: "ammo".to_string(),
                    name: "随身弹药".to_string(),
                    count: 1,
                    ammo_t: ammo_q,
                    slot: "pack".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 1, grid_y: 0,
                });
            }
            if food_q > 0.0 {
                list.push(SoldierItem {
                    kind: "ration".to_string(),
                    model: "ration".to_string(),
                    name: "单兵口粮".to_string(),
                    count: 1,
                    ammo_t: food_q,
                    slot: "pack".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 2, grid_y: 0,
                });
            }
            list
        };
        for o_i in 0..officer {
            let w = w_pool.get(idx_seq).cloned().unwrap_or_else(|| "rifle".to_string());
            idx_seq += 1;
            sol_vec.push(SoldierState {
                id: format!("{}-o{}", self.str_id[uid.0], o_i + 1),
                name: format!("军官 #{}", o_i + 1),
                hp: 100.0,
                max_hp: 100.0,
                status: "healthy".to_string(),
                weapon: w.clone(),
                ammo_t: per_sol_ammo,
                is_cadre: true,
                role: "officer".to_string(),
                items: build_items(&w, per_sol_ammo, per_sol_food),
                cannot_fire: false,
                cannot_operate_heavy: false,
                max_hand_weight_kg: None,
                max_pack_weight_kg: None,
                pack_cols: 8,
                pack_rows: 6,
            });
        }
        for c_i in 0..nco {
            let w = w_pool.get(idx_seq).cloned().unwrap_or_else(|| "rifle".to_string());
            idx_seq += 1;
            sol_vec.push(SoldierState {
                id: format!("{}-c{}", self.str_id[uid.0], c_i + 1),
                name: format!("士官 #{}", c_i + 1),
                hp: 100.0,
                max_hp: 100.0,
                status: "healthy".to_string(),
                weapon: w.clone(),
                ammo_t: per_sol_ammo,
                is_cadre: true,
                role: "nco".to_string(),
                items: build_items(&w, per_sol_ammo, per_sol_food),
                cannot_fire: false,
                cannot_operate_heavy: false,
                max_hand_weight_kg: None,
                max_pack_weight_kg: None,
                pack_cols: 8,
                pack_rows: 6,
            });
        }
        for e_i in 0..enlisted {
            let w = w_pool.get(idx_seq).cloned().unwrap_or_else(|| "rifle".to_string());
            idx_seq += 1;
            sol_vec.push(SoldierState {
                id: format!("{}-s{}", self.str_id[uid.0], e_i + 1),
                name: format!("士兵 #{}", e_i + 1),
                hp: 100.0,
                max_hp: 100.0,
                status: "healthy".to_string(),
                weapon: w.clone(),
                ammo_t: per_sol_ammo,
                is_cadre: false,
                role: "enlisted".to_string(),
                items: build_items(&w, per_sol_ammo, per_sol_food),
                cannot_fire: false,
                cannot_operate_heavy: false,
                max_hand_weight_kg: None,
                max_pack_weight_kg: None,
                pack_cols: 8,
                pack_rows: 6,
            });
        }
        self.soldiers.push(sol_vec);
        self.carried_bodies.push(Vec::new());
        self.task_scheduler.push(TaskTimingScheduler::new_for_echelon(echelon));
        self.sync_totals_from_soldiers(uid);
        self.refresh_caps(uid);
        uid
    }

    pub fn set_nco_officer(&mut self, id: UnitId, nco: u32, officer: u32) {
        let i = id.0;
        self.nco[i] = nco;
        self.officer[i] = officer;
        self.cadre[i] = nco + officer;
        // Synchronize soldier roles to match explicit 4-ledger edit
        let mut off_rem = officer;
        let mut nco_rem = nco;
        for s in &mut self.soldiers[i] {
            if s.status == "dead" {
                continue;
            }
            if off_rem > 0 {
                s.is_cadre = true;
                s.role = "officer".to_string();
                off_rem -= 1;
            } else if nco_rem > 0 {
                s.is_cadre = true;
                s.role = "nco".to_string();
                nco_rem -= 1;
            } else {
                s.is_cadre = false;
                s.role = "enlisted".to_string();
            }
        }
        self.refresh_caps(id);
    }

    pub fn sync_totals_from_soldiers(&mut self, id: UnitId) {
        let i = id.0;
        let mut new_eq: FxHashMap<String, u32> = FxHashMap::default();
        // 保留车辆、班组重武器、牵引炮与多人操作装备，防止单兵减员/随身装备覆盖时被意外清除
        for (m, &cnt) in &self.equipment[i] {
            let spec = equipment_spec(m);
            let is_crew_served = spec.is_vehicle || spec.is_heavy || spec.crew > 1 || spec.towable;
            if is_crew_served && cnt > 0 {
                new_eq.insert(m.clone(), cnt);
            }
        }
        let mut sum_ammo = 0.0f32;
        let mut sum_food = 0.0f32;
        for s in &mut self.soldiers[i] {
            if s.status == "dead" {
                continue;
            }
            let mut s_ammo = 0.0f32;
            let mut primary_w = String::new();
            for it in &s.items {
                if (it.kind == "weapon" || it.kind == "radio") && it.count > 0 && !it.model.is_empty() {
                    let spec = equipment_spec(&it.model);
                    let is_crew_served = spec.is_vehicle || spec.is_heavy || spec.crew > 1 || spec.towable;
                    if !is_crew_served {
                        *new_eq.entry(it.model.clone()).or_insert(0) += it.count;
                    }
                    if it.slot == "hand" {
                        primary_w = it.model.clone();
                    }
                } else if it.kind == "ammo" {
                    s_ammo += it.ammo_t.max(0.0);
                } else if it.kind == "ration" {
                    sum_food += it.ammo_t.max(0.0);
                }
            }
            if primary_w.is_empty() {
                if let Some(it) = s.items.iter().find(|it| it.kind == "weapon" && it.count > 0) {
                    primary_w = it.model.clone();
                } else if let Some(it) = s.items.iter().find(|it| it.kind == "radio" && it.count > 0) {
                    primary_w = it.model.clone();
                }
            }
            s.weapon = primary_w;
            s.ammo_t = s_ammo;
            s.refresh_hand_occupancy_flags();
            sum_ammo += s_ammo;
        }
        self.equipment[i] = new_eq;
        if !self.soldiers[i].is_empty() {
            self.ammo_t[i] = sum_ammo;
            self.food_t[i] = sum_food;
            self.officer[i] = self.soldiers[i].iter().filter(|s| s.role == "officer" && s.status != "dead").count() as u32;
            self.nco[i] = self.soldiers[i].iter().filter(|s| s.is_cadre && s.role != "officer" && s.status != "dead").count() as u32;
            self.cadre[i] = self.nco[i] + self.officer[i];
            self.enlisted[i] = self.soldiers[i].iter().filter(|s| !s.is_cadre && s.status != "dead").count() as u32;
        }
    }

    pub fn add_supply_reserving_soldiers(&mut self, id: UnitId, food_add: f32, ammo_add: f32, fuel_add: f32) {
        let i = id.0;
        let f_room = (self.cap_food_t[i] - self.food_t[i]).max(0.0);
        let a_room = (self.cap_ammo_t[i] - self.ammo_t[i]).max(0.0);
        let g_room = (self.cap_fuel_t[i] - self.fuel_t[i]).max(0.0);
        let df = food_add.max(0.0).min(f_room);
        let da = ammo_add.max(0.0).min(a_room);
        let dg = fuel_add.max(0.0).min(g_room);
        self.food_t[i] += df;
        self.ammo_t[i] += da;
        self.fuel_t[i] += dg;
        if da > 1e-6 && !self.soldiers[i].is_empty() {
            let live_n = self.soldiers[i].iter().filter(|s| s.status != "dead").count().max(1) as f32;
            let each_a = da / live_n;
            for s in &mut self.soldiers[i] {
                if s.status == "dead" {
                    continue;
                }
                s.ammo_t += each_a;
                if let Some(it) = s.items.iter_mut().find(|it| it.kind == "ammo") {
                    it.ammo_t += each_a;
                } else {
                    s.items.push(SoldierItem {
                        kind: "ammo".to_string(),
                        model: "ammo".to_string(),
                        name: "随身弹药".to_string(),
                        count: 1,
                        ammo_t: each_a,
                        slot: "pack".to_string(),
                        weight_kg: 0.0,
                        grid_w: 1,
                        grid_h: 1,
                        grid_x: 1,
                        grid_y: 0,
                    });
                }
            }
        }
    }

    pub fn fire_eligible_ratio(&self, id: UnitId) -> f32 {
        let sols = &self.soldiers[id.0];
        if sols.is_empty() {
            if let Some(op) = self.operated_by[id.0] {
                if op.0 < self.alive.len() && self.alive[op.0] && !self.soldiers[op.0].is_empty() {
                    let op_sols = &self.soldiers[op.0];
                    let ok = op_sols.iter().filter(|s| !s.cannot_fire && !s.cannot_operate_heavy).count();
                    return (ok as f32) / (op_sols.len() as f32);
                }
            }
            return if self.effective_operator_stats(id).0 > 0 { 1.0 } else { 0.0 };
        }
        let ok = sols.iter().filter(|s| !s.cannot_fire).count();
        (ok as f32) / (sols.len() as f32)
    }

    pub fn consume_soldier_ammo(&mut self, id: UnitId, amount: f32) -> f32 {
        let i = id.0;
        if amount <= 0.0 {
            return 0.0;
        }
        if self.soldiers[i].is_empty() {
            let avail = self.ammo_t[i].max(0.0);
            let take = amount.min(avail);
            self.ammo_t[i] = (avail - take).max(0.0);
            if take < amount && self.ammo_t[i] <= 1e-6 {
                if let Some(op) = self.operated_by[i] {
                    if op.0 < self.alive.len() && self.alive[op.0] && op != id {
                        return take + self.consume_soldier_ammo(op, amount - take);
                    }
                }
            }
            return take;
        }
        let tot: f32 = self.soldiers[i].iter().map(|s| s.ammo_t.max(0.0)).sum();
        if tot <= 1e-6 {
            self.ammo_t[i] = 0.0;
            return 0.0;
        }
        let take = amount.min(tot);
        let ratio = ((tot - take) / tot).clamp(0.0, 1.0);
        for s in &mut self.soldiers[i] {
            s.ammo_t = (s.ammo_t * ratio).max(0.0);
            for it in &mut s.items {
                if it.kind == "ammo" {
                    it.ammo_t = (it.ammo_t * ratio).max(0.0);
                }
            }
        }
        self.ammo_t[i] = self.soldiers[i].iter().map(|s| s.ammo_t).sum();
        take
    }

    #[inline]
    pub fn persons(&self, id: UnitId) -> u32 {
        let i = id.0;
        self.enlisted[i] + self.cadre[i]
    }

    #[inline]
    pub fn is_directional_hull(&self, id: UnitId) -> bool {
        let i = id.0;
        self.vehicle_model[i].is_some()
            || self.equipment[i].iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle)
    }

    pub fn refresh_caps(&mut self, id: UnitId) {
        let i = id.0;
        let ledger_cadre = self.nco[i] + self.officer[i];
        if ledger_cadre != self.cadre[i] {
            if self.cadre[i] < ledger_cadre {
                let mut drop = ledger_cadre - self.cadre[i];
                let d_nco = drop.min(self.nco[i]);
                self.nco[i] -= d_nco;
                drop -= d_nco;
                self.officer[i] = self.officer[i].saturating_sub(drop);
            } else {
                self.nco[i] += self.cadre[i] - ledger_cadre;
            }
        }
        self.cadre[i] = self.nco[i] + self.officer[i];
        if self.kind[i] == "hq_squad" && self.cadre[i] == 0 {
            self.kind[i] = "infantry_squad".to_string();
            self.unit_mask[i] = classify_unit_mask(&self.kind[i], &self.equipment[i]);
        }
        let eval = evaluate_counter_and_caps_4ledger(
            self.enlisted[i],
            self.nco[i],
            self.officer[i],
            &self.equipment[i],
            self.echelon[i],
        );
        self.restricted_echelon[i] = eval.restricted_echelon;
        self.org_cap[i] = eval.org_cap_limit;
        self.org[i] = self.org[i].min(eval.org_cap_limit);
        self.empty_cadre[i] = eval.empty_cadre;
        self.recovery_mult[i] = eval.recovery_rate_mult;
        self.display_tier_bonus[i] = eval.display_tier_bonus;
        self.mobility[i] = self
            .vehicle_model[i]
            .as_deref()
            .and_then(vehicle_spec)
            .map(|vs| vs.mobility)
            .unwrap_or(eval.mobility);
        self.can_operate_radio[i] = self.cadre[i] > 0;
        let (eff_p, eff_nco, eff_off, _) = self.effective_operator_stats(id);
        self.heavy_op_cap[i] = heavy_operator_efficiency_4ledger(
            eff_nco,
            eff_off,
            eff_p,
            self.deployed_weapon[i].as_deref().unwrap_or("rifle"),
        );
        self.is_acting_nco_cmd[i] = self.officer[i] == 0 && self.nco[i] > 0;
    }

    /// Returns `(eff_persons, eff_nco, eff_officer, crew_buff_mult)`.
    /// Supports manned units as well as unmanned heavy emplacements/vehicles linked via `operated_by`.
    pub fn effective_operator_stats(&self, id: UnitId) -> (u32, u32, u32, f32) {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return (0, 0, 0, 0.0);
        }
        let own_p = self.persons(id);
        let main_w = self
            .deployed_weapon[i]
            .as_deref()
            .or_else(|| {
                self.equipment[i]
                    .iter()
                    .find(|(m, &c)| c > 0 && (equipment_spec(m).is_heavy || equipment_spec(m).deployable || equipment_spec(m).is_vehicle))
                    .map(|(m, _)| m.as_str())
            })
            .unwrap_or("rifle");
        if own_p > 0 {
            let buff = heavy_crew_buff_mult(own_p.min(4), main_w);
            return (own_p, self.nco[i], self.officer[i], buff);
        }
        if let Some(op) = self.operated_by[i] {
            if op.0 < self.alive.len() && self.alive[op.0] && self.persons(op) > 0 {
                let d_px = (self.pos[i][0] - self.pos[op.0][0]).hypot(self.pos[i][1] - self.pos[op.0][1]);
                if d_px <= 2.6 {
                    let mut crew = 0u32;
                    let mut nco_c = 0u32;
                    let mut off_c = 0u32;
                    if !self.assigned_operator_soldiers[i].is_empty() && !self.soldiers[op.0].is_empty() {
                        for s in &self.soldiers[op.0] {
                            if s.status != "dead"
                                && !s.cannot_fire
                                && !s.cannot_operate_heavy
                                && self.assigned_operator_soldiers[i].iter().any(|sid| sid == &s.id)
                            {
                                crew += 1;
                                if s.role == "officer" {
                                    off_c += 1;
                                } else if s.is_cadre {
                                    nco_c += 1;
                                }
                            }
                        }
                    }
                    if crew == 0 {
                        let req = if self.assigned_operator_count[i] > 0 {
                            self.assigned_operator_count[i]
                        } else {
                            equipment_spec(main_w).crew.max(2)
                        };
                        let avail = if self.soldiers[op.0].is_empty() {
                            self.persons(op)
                        } else {
                            self.soldiers[op.0]
                                .iter()
                                .filter(|s| s.status != "dead" && !s.cannot_fire && !s.cannot_operate_heavy)
                                .count() as u32
                        };
                        crew = req.min(avail);
                        nco_c = self.nco[op.0].min(crew);
                        off_c = self.officer[op.0].min(crew.saturating_sub(nco_c));
                    }
                    let buff = heavy_crew_buff_mult(crew, main_w);
                    return (crew, nco_c, off_c, buff);
                }
            }
        }
        (0, 0, 0, 0.0)
    }

    pub fn set_heavy_operators(&mut self, id: UnitId, op_uid: Option<UnitId>, soldier_ids: Vec<String>, op_count: u32) -> Result<(), String> {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return Err("target_unit_dead".into());
        }
        if let Some(op) = op_uid {
            if op.0 >= self.alive.len() || !self.alive[op.0] || self.team[op.0] != self.team[i] || self.persons(op) == 0 {
                return Err("invalid_operator_unit".into());
            }
            let d_px = (self.pos[i][0] - self.pos[op.0][0]).hypot(self.pos[i][1] - self.pos[op.0][1]);
            if d_px > 2.6 {
                return Err("operator_unit_too_far".into());
            }
        }
        self.operated_by[i] = op_uid;
        self.assigned_operator_soldiers[i] = soldier_ids;
        self.assigned_operator_count[i] = op_count;
        self.refresh_caps(id);
        Ok(())
    }

    pub fn unit_has_engineer_cap(&self, id: UnitId) -> bool {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return false;
        }
        let kl = self.kind[i].to_ascii_lowercase();
        let sl = self.str_id[i].to_ascii_lowercase();
        if kl.contains("eng")
            || kl.contains("sapper")
            || kl.contains("pion")
            || sl.contains("eng")
            || sl.contains("pion")
            || self.name[i].contains("工兵")
            || self.name[i].contains("爆破")
        {
            return true;
        }
        for (m, &cnt) in &self.equipment[i] {
            if cnt == 0 {
                continue;
            }
            let ml = m.to_ascii_lowercase();
            if matches!(
                ml.as_str(),
                "explosives" | "satchel" | "mine_detector" | "bridge_kit" | "flamethrower" | "wpn_flammenwerfer_35"
            ) || ml.contains("engineer")
                || ml.contains("mine")
                || ml.contains("explosive")
            {
                return true;
            }
        }
        self.soldiers[i].iter().any(|s| {
            if s.status == "dead" {
                return false;
            }
            let rl = s.role.to_ascii_lowercase();
            rl.contains("eng")
                || rl.contains("sapper")
                || rl.contains("pion")
                || s.items.iter().any(|it| {
                    it.count > 0
                        && matches!(
                            it.model.as_str(),
                            "explosives" | "satchel" | "mine_detector" | "bridge_kit" | "flamethrower" | "wpn_flammenwerfer_35"
                        )
                })
        })
    }

    pub fn effective_speed_kmh(
        &self,
        id: UnitId,
        grid: &WorldGrid,
        r0: i32,
        c0: i32,
        r1: i32,
        c1: i32,
        action: &str,
        move_mode: &str,
    ) -> f32 {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return 0.0;
        }
        if self.unit_state[i] == UnitState::Deployed {
            return 0.0;
        }
        let (eff_persons, _, _, _) = self.effective_operator_stats(id);
        let mob = self.mobility[i];
        let vspec = self
            .vehicle_model[i]
            .as_deref()
            .and_then(vehicle_spec)
            .or_else(|| {
                self.equipment[i]
                    .iter()
                    .filter(|(_, &c)| c > 0)
                    .filter_map(|(m, _)| vehicle_spec(m))
                    .min_by(|a, b| a.base_speed_kmh.total_cmp(&b.base_speed_kmh))
            });
        let mut base_kmh = if let Some(vs) = vspec {
            if eff_persons == 0 {
                return 0.0;
            }
            let crew_mult = (eff_persons as f32 / vs.min_crew.max(1) as f32).clamp(0.45, 1.0);
            vs.base_speed_kmh * crew_mult
        } else {
            if eff_persons == 0 {
                return 0.0;
            }
            let mut foot_spd = if self.kind[i].contains("recon") { 4.3 } else { 3.8 };
            let heavy_drag_cnt: u32 = self.equipment[i]
                .iter()
                .filter(|(m, &c)| c > 0 && {
                    let s = equipment_spec(m);
                    s.is_heavy || s.towable
                })
                .map(|(_, &c)| c)
                .sum();
            if heavy_drag_cnt > 0 {
                foot_spd *= 0.80f32.powi(heavy_drag_cnt.min(3) as i32).max(0.52);
            }
            foot_spd
        };

        let i1 = grid.idx(r1.clamp(0, (grid.rows as i32) - 1), c1.clamp(0, (grid.cols as i32) - 1));
        let t1 = grid.terrain[i1];
        let tc = crate::world_grid::terrain_cost(mob, t1);
        if !tc.is_finite() {
            return 0.0;
        }
        let cm1 = grid.get_cell_modifiers(r1, c1, mob);
        if cm1.impassable {
            return 0.0;
        }
        if !cm1.ignore_slope_limit && grid.slope_deg[i1] > mob.slope_limit_deg() {
            return 0.0;
        }
        let w1 = (grid.weather[i1] as usize).min(5);
        let w_mult = if cm1.is_road {
            crate::world_grid::WEATHER_ROAD[w1]
        } else {
            crate::world_grid::WEATHER_OFFROAD[w1]
        };
        let horiz_m = (((r1 - r0) as f32).hypot((c1 - c0) as f32) * grid.cell_m).max(1.0);
        let i0 = grid.idx(r0.clamp(0, (grid.rows as i32) - 1), c0.clamp(0, (grid.cols as i32) - 1));
        let dz = grid.elevation_m[i1] - grid.elevation_m[i0];
        let grade = dz / horiz_m;
        let slope_mult = if grade >= 0.0 {
            1.0 + mob.uphill_k() * grade
        } else {
            (1.0 + 0.45 * grade).max(0.85)
        };
        let overload_mult = (1.0 - self.carry_loss[i] * 0.45).clamp(0.45, 1.0);
        let mode_mult = match move_mode {
            "fast" => 1.25,
            "slow" | "stealth" => 0.55,
            _ => 1.0,
        };
        let act_mult = match action {
            "retreat" => 1.30,
            "assault" => 0.85,
            _ => 1.0,
        };
        base_kmh = (base_kmh * cm1.speed_mult * overload_mult * mode_mult * act_mult)
            / (tc.max(0.25) * w_mult * slope_mult);
        if let Some(vs) = vspec {
            let cap = if move_mode == "fast" || action == "retreat" {
                vs.road_max_kmh * 1.15
            } else {
                vs.road_max_kmh
            };
            base_kmh = base_kmh.min(cap);
        }
        base_kmh.max(0.2)
    }

    pub fn unit_power(&self, id: UnitId, dist_m: f32, has_los: bool, only_weapon: Option<&str>, acc_mult: f32) -> [f32; 6] {
        let mut totals = [0.0f32; 6];
        let i = id.0;
        let (eff_persons, eff_nco, eff_officer, _) = self.effective_operator_stats(id);
        if eff_persons == 0 || !self.alive[i] {
            return totals;
        }
        let base_org = if self.persons(id) > 0 {
            self.org[i]
        } else if let Some(op) = self.operated_by[i] {
            self.org[op.0]
        } else {
            self.org[i]
        };
        let org_mult = (0.3 + 0.7 * (base_org / 100.0)).clamp(0.15, 1.3);
        let mut needed_small_crew = 0u32;
        let mut has_small_arms = false;
        for (model, &cnt) in &self.equipment[i] {
            if cnt == 0 {
                continue;
            }
            if let Some(ow) = only_weapon {
                if model != ow {
                    continue;
                }
            }
            let spec = equipment_spec(model);
            if !spec.indirect && !has_los {
                continue;
            }
            let acc = (weapon_accuracy_decay(dist_m, spec.range_m) * acc_mult).min(1.5);
            let dmg_dec = weapon_damage_decay(model, dist_m, spec.range_m);
            let is_crew_wpn = spec.is_heavy || spec.deployable || spec.crew >= 2;
            let crew_scale = if is_crew_wpn {
                let op_eff = heavy_operator_efficiency_4ledger(eff_nco, eff_officer, eff_persons, model);
                let buff = heavy_crew_buff_mult(eff_persons.min(4), model);
                op_eff * buff
            } else {
                has_small_arms = true;
                needed_small_crew += spec.crew.max(1) * cnt;
                1.0
            };
            let scale = cnt as f32 * acc * dmg_dec * org_mult * crew_scale;
            for d in 0..6 {
                totals[d] += spec.fire[d] * scale;
            }
        }
        if has_small_arms && needed_small_crew > 0 && eff_persons < needed_small_crew {
            let small_manning = (eff_persons as f32 / needed_small_crew as f32).clamp(0.1, 1.0);
            // If unit has only small arms, scale whole total; if mixed, scaling conservatively preserves crew buff on heavy weapons.
            let has_only_small = !self.equipment[i].iter().any(|(m, &c)| c > 0 && {
                let sp = equipment_spec(m);
                sp.is_heavy || sp.deployable || sp.crew >= 2
            });
            if has_only_small {
                for d in 0..6 {
                    totals[d] *= small_manning;
                }
            }
        }
        totals
    }

    pub fn set_counter_skin(
        &mut self,
        id: UnitId,
        color: Option<&str>,
        badge: Option<&str>,
        custom_label: Option<&str>,
        attempt_level: Option<&str>,
    ) -> Result<(), String> {
        let i = id.0;
        if let Some(lvl) = attempt_level {
            if Echelon::from_str(lvl) != self.restricted_echelon[i] {
                return Err("forbidden_counter_echelon_tamper".into());
            }
        }
        if let Some(c) = color {
            self.skin_color[i] = c.to_string();
        }
        if let Some(b) = badge {
            self.skin_badge[i] = b.to_string();
        }
        if let Some(lbl) = custom_label {
            self.skin_label[i] = if lbl.is_empty() { None } else { Some(lbl.to_string()) };
        }
        Ok(())
    }

    pub fn split_unit(&mut self, src: UnitId, take_enlisted: u32, take_cadre: u32, new_str_id: &str) -> Result<UnitId, String> {
        let i = src.0;
        if !self.alive[i] {
            return Err("source_dead".into());
        }
        let te = take_enlisted.min(self.enlisted[i]);
        let tc = take_cadre.min(self.cadre[i]);
        if te + tc == 0 {
            return Err("empty_split".into());
        }
        if self.enlisted[i] - te + self.cadre[i] - tc == 0 {
            return Err("cannot_empty_source".into());
        }
        let total_before = self.persons(src).max(1) as f32;
        self.enlisted[i] -= te;
        let take_nco = tc.min(self.nco[i]);
        let take_off = (tc - take_nco).min(self.officer[i]);
        self.nco[i] -= take_nco;
        self.officer[i] -= take_off;
        self.cadre[i] = self.nco[i] + self.officer[i];

        let mut split_eq = FxHashMap::default();
        if let Some(&r_cnt) = self.equipment[i].get("rifle") {
            let take_r = (r_cnt / 2).min(te + tc);
            if take_r > 0 {
                self.equipment[i].insert("rifle".to_string(), r_cnt - take_r);
                split_eq.insert("rifle".to_string(), take_r);
            }
        }

        let frac = (te + tc) as f32 / total_before;
        let f_take = self.food_t[i] * frac;
        let a_take = self.ammo_t[i] * frac;
        let g_take = self.fuel_t[i] * frac;
        self.food_t[i] -= f_take;
        self.ammo_t[i] -= a_take;
        self.fuel_t[i] -= g_take;
        self.refresh_caps(src);

        let mut moved_soldiers: Vec<SoldierState> = Vec::with_capacity((te + tc) as usize);
        if !self.soldiers[i].is_empty() {
            let mut rem_off = take_off;
            let mut rem_nco = take_nco;
            let mut rem_enl = te;
            let mut kept = Vec::with_capacity(self.soldiers[i].len());
            for mut s in self.soldiers[i].drain(..) {
                if s.status != "dead" {
                    if s.role == "officer" && rem_off > 0 {
                        rem_off -= 1;
                        s.id = format!("{}-o{}", new_str_id, take_off - rem_off);
                        moved_soldiers.push(s);
                        continue;
                    }
                    if s.is_cadre && s.role != "officer" && rem_nco > 0 {
                        rem_nco -= 1;
                        s.id = format!("{}-c{}", new_str_id, take_nco - rem_nco);
                        moved_soldiers.push(s);
                        continue;
                    }
                    if !s.is_cadre && rem_enl > 0 {
                        rem_enl -= 1;
                        s.id = format!("{}-e{}", new_str_id, te - rem_enl);
                        moved_soldiers.push(s);
                        continue;
                    }
                }
                kept.push(s);
            }
            self.soldiers[i] = kept;
        }

        let team = self.team[i];
        let px = self.px[i];
        let pos = [self.pos[i][0] + 0.08, self.pos[i][1] + 0.08];
        let kind = self.kind[i].clone();
        let ch = self.radio_channel[i];
        let cmdr = self.direct_commander_id[i];
        let org_val = self.org[i];
        let name = format!("{}-分", self.name[i]);
        let child = self.spawn_unit_4ledger(new_str_id, name, team, px, Echelon::Squad, &kind, te, take_nco, take_off, split_eq, ch);
        if !moved_soldiers.is_empty() {
            self.soldiers[child.0] = moved_soldiers;
        }
        self.sync_totals_from_soldiers(src);
        self.sync_totals_from_soldiers(child);
        self.pos[child.0] = pos;
        self.direct_commander_id[child.0] = cmdr;
        self.org[child.0] = org_val.min(self.org_cap[child.0]);
        self.food_t[child.0] = f_take;
        self.ammo_t[child.0] = a_take;
        self.fuel_t[child.0] = g_take;
        Ok(child)
    }

    pub fn merge_squads_direct(&mut self, dst: UnitId, src: UnitId) -> Result<u32, String> {
        let di = dst.0;
        let si = src.0;
        if di == si || !self.alive[di] || !self.alive[si] {
            return Err("invalid_merge_units".into());
        }
        if self.team[di] != self.team[si] {
            return Err("cross_team_merge_forbidden".into());
        }
        if (self.echelon[di] as u8) > (Echelon::Squad as u8) || (self.echelon[si] as u8) > (Echelon::Squad as u8) {
            return Err("platoon_merge_requires_reorganize_event".into());
        }
        let total = self.persons(dst) + self.persons(src);
        if total > DIRECT_MERGE_MAX_PERSONS {
            return Err(format!("exceeds_squad_direct_merge_cap:{total}>15"));
        }
        let p_dst = self.persons(dst) as f32;
        let p_src = self.persons(src) as f32;
        let w_org = (self.org[di] * p_dst + self.org[si] * p_src) / (p_dst + p_src).max(1.0);

        self.enlisted[di] += self.enlisted[si];
        self.nco[di] += self.nco[si];
        self.officer[di] += self.officer[si];
        self.cadre[di] = self.nco[di] + self.officer[di];
        self.enlisted_full[di] += self.enlisted_full[si];
        self.nco_full[di] += self.nco_full[si];
        self.officer_full[di] += self.officer_full[si];
        self.cadre_full[di] = self.nco_full[di] + self.officer_full[di];
        let src_eq = std::mem::take(&mut self.equipment[si]);
        for (k, v) in src_eq {
            *self.equipment[di].entry(k.clone()).or_insert(0) += v;
            *self.equipment_full[di].entry(k).or_insert(0) += v;
        }
        self.food_t[di] += self.food_t[si];
        self.ammo_t[di] += self.ammo_t[si];
        self.fuel_t[di] += self.fuel_t[si];
        self.replacements[di] += self.replacements[si];

        if !self.soldiers[si].is_empty() {
            let mut moved = std::mem::take(&mut self.soldiers[si]);
            self.soldiers[di].append(&mut moved);
            self.sync_totals_from_soldiers(dst);
        }

        self.enlisted[si] = 0;
        self.nco[si] = 0;
        self.officer[si] = 0;
        self.cadre[si] = 0;
        self.alive[si] = false;
        self.refresh_caps(dst);
        self.org[di] = w_org.min(self.org_cap[di]);
        Ok(total)
    }

    pub fn effective_locks(&self, id: UnitId, events: &[ActiveEvent]) -> LockFlags {
        let mut locks = LockFlags::empty();
        for s in &self.modifier_slots[id.0] {
            locks |= s.locks;
        }
        if self.unit_state[id.0] == UnitState::Deployed {
            locks.insert(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE);
        }
        if self.reorganizing_event_id[id.0].is_some() {
            locks |= LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE | LockFlags::FORBID_REST | LockFlags::FORBID_DEPLOY;
        }
        for ev in events {
            if ev.actor_id == id {
                locks |= ev.kind.locks();
            } else if let EventKind::ReorganizeEchelon {
                commander_piece_id,
                source_unit_ids,
                ..
            } = &ev.kind
            {
                if *commander_piece_id == Some(id) || source_unit_ids.contains(&id) {
                    locks |= ev.kind.locks();
                }
            }
        }
        locks.normalized()
    }
}

pub fn classify_unit_mask(kind: &str, equipment: &FxHashMap<String, u32>) -> u8 {
    let kl = kind.to_ascii_lowercase();
    if kl.contains("tank") || kl.contains("armor") || equipment.get("tank_medium").copied().unwrap_or(0) > 0 || equipment.get("tank_heavy").copied().unwrap_or(0) > 0 {
        return LINE_MASK_ARMOR;
    }
    if kl.contains("recon") {
        return LINE_MASK_RECON;
    }
    if kl.contains("howitzer") || kl.contains("arty") || kl.contains("porter") || kl.contains("transport") || kl.contains("hq") {
        return LINE_MASK_ARTY_LOG;
    }
    if kl.contains("mg") || kl.contains("at") || kl.contains("mortar") || equipment.get("hmg").copied().unwrap_or(0) > 0 || equipment.get("at_gun_50").copied().unwrap_or(0) > 0 {
        return LINE_MASK_HEAVY_AT;
    }
    LINE_MASK_INFANTRY
}

pub fn heavy_operator_efficiency_4ledger(nco: u32, officer: u32, persons: u32, weapon: &str) -> f32 {
    if persons == 0 {
        return 0.0;
    }
    let req = equipment_spec(weapon).crew.max(1);
    let man = (persons as f32 / req as f32).clamp(0.1, 1.0);
    // 士官(NCO)专职管重武器与现场操炮效能；无士官有军官则有轻微折损，全无骨干大幅折损
    let cadre_bonus = if nco >= 2 {
        1.0
    } else if nco == 1 {
        0.90
    } else if officer > 0 {
        0.75
    } else {
        0.55
    };
    man * cadre_bonus
}

pub fn heavy_operator_efficiency(cadre: u32, persons: u32, weapon: &str) -> f32 {
    heavy_operator_efficiency_4ledger(cadre, 0, persons, weapon)
}

#[derive(Debug, Clone)]
pub struct CommanderSoA {
    pub id: Vec<CommanderId>,
    pub str_id: Vec<String>,
    pub name: Vec<String>,
    pub team: Vec<TeamId>,
    pub echelon: Vec<Echelon>,
    pub rank: Vec<Rank>,
    pub px: Vec<(i32, i32)>,
    pub alive: Vec<bool>,
    pub radio_channel: Vec<u8>,
    pub radio_silence: Vec<bool>,
    pub hq_piece_id: Vec<Option<UnitId>>,
    pub attached_to: Vec<Option<UnitId>>,
    pub cadre_piece_id: Vec<Option<UnitId>>,
    pub parent_commander_id: Vec<Option<CommanderId>>,
    pub subordinate_cmd_ids: Vec<Vec<CommanderId>>,
    pub subordinate_unit_ids: Vec<Vec<UnitId>>,

    pub last_px: Vec<(i32, i32)>,
    pub still_hours: Vec<f32>,
    pub council_streak: Vec<u32>,
    pub council_cooldown_hours: Vec<f32>,
    pub briefing_buff_hours: Vec<f32>,
    pub council_ao_hours: Vec<f32>,
    pub war_council_sync_hours: Vec<f32>,
    pub council_partner_id: Vec<Option<CommanderId>>,
    pub council_radius_km: Vec<f32>,
    pub council_completed: Vec<bool>,
    pub post_council_exposure_hours: Vec<f32>,
    pub post_council_exposure_val: Vec<f32>,

    pub order_churn_score: Vec<f32>,
    pub last_order_sig: Vec<u64>,
    pub last_auto_order_clock_hours: Vec<f32>,
    pub parent_order_seq: Vec<u64>,
    pub assigned_parent_line_pts: Vec<Vec<[f32; 2]>>,
    pub modifier_slots: Vec<Vec<ModifierSlot>>,

    pub boldness: Vec<f32>,
    pub patience: Vec<f32>,
    pub nco_deputy_scale: Vec<f32>,
    pub staff: Vec<u32>,
    pub staff_full: Vec<u32>,
    pub str_to_id: FxHashMap<String, CommanderId>,
}

impl Default for CommanderSoA {
    fn default() -> Self {
        Self::new()
    }
}

impl CommanderSoA {
    pub fn new() -> Self {
        Self {
            id: Vec::new(),
            str_id: Vec::new(),
            name: Vec::new(),
            team: Vec::new(),
            echelon: Vec::new(),
            rank: Vec::new(),
            px: Vec::new(),
            alive: Vec::new(),
            radio_channel: Vec::new(),
            radio_silence: Vec::new(),
            hq_piece_id: Vec::new(),
            attached_to: Vec::new(),
            cadre_piece_id: Vec::new(),
            parent_commander_id: Vec::new(),
            subordinate_cmd_ids: Vec::new(),
            subordinate_unit_ids: Vec::new(),
            last_px: Vec::new(),
            still_hours: Vec::new(),
            council_streak: Vec::new(),
            council_cooldown_hours: Vec::new(),
            briefing_buff_hours: Vec::new(),
            council_ao_hours: Vec::new(),
            war_council_sync_hours: Vec::new(),
            council_partner_id: Vec::new(),
            council_radius_km: Vec::new(),
            council_completed: Vec::new(),
            post_council_exposure_hours: Vec::new(),
            post_council_exposure_val: Vec::new(),
            order_churn_score: Vec::new(),
            last_order_sig: Vec::new(),
            last_auto_order_clock_hours: Vec::new(),
            parent_order_seq: Vec::new(),
            assigned_parent_line_pts: Vec::new(),
            modifier_slots: Vec::new(),
            boldness: Vec::new(),
            patience: Vec::new(),
            nco_deputy_scale: Vec::new(),
            staff: Vec::new(),
            staff_full: Vec::new(),
            str_to_id: FxHashMap::default(),
        }
    }

    pub fn spawn_commander(
        &mut self,
        str_id: impl Into<String>,
        name: impl Into<String>,
        team: TeamId,
        px: (i32, i32),
        echelon: Echelon,
        rank: Rank,
        radio_channel: u8,
    ) -> CommanderId {
        let cid = CommanderId(self.id.len());
        let s_id = str_id.into();
        self.str_to_id.insert(s_id.clone(), cid);
        self.id.push(cid);
        self.str_id.push(s_id);
        self.name.push(name.into());
        self.team.push(team);
        self.echelon.push(echelon);
        self.rank.push(rank);
        self.px.push(px);
        self.alive.push(true);
        self.radio_channel.push(radio_channel);
        self.radio_silence.push(false);
        self.hq_piece_id.push(None);
        self.attached_to.push(None);
        self.cadre_piece_id.push(None);
        self.parent_commander_id.push(None);
        self.subordinate_cmd_ids.push(Vec::new());
        self.subordinate_unit_ids.push(Vec::new());
        self.last_px.push(px);
        self.still_hours.push(0.0);
        self.council_streak.push(0);
        self.council_cooldown_hours.push(0.0);
        self.briefing_buff_hours.push(0.0);
        self.council_ao_hours.push(0.0);
        self.war_council_sync_hours.push(0.0);
        self.council_partner_id.push(None);
        self.council_radius_km.push(1.0);
        self.council_completed.push(false);
        self.post_council_exposure_hours.push(0.0);
        self.post_council_exposure_val.push(0.0);
        self.order_churn_score.push(0.0);
        self.last_order_sig.push(0);
        self.last_auto_order_clock_hours.push(-9999.0);
        self.parent_order_seq.push(0);
        self.assigned_parent_line_pts.push(Vec::new());
        self.modifier_slots.push(Vec::new());
        self.boldness.push(0.5);
        self.patience.push(0.5);
        self.nco_deputy_scale.push(1.0);
        let sf = echelon.default_staff_full();
        self.staff_full.push(sf);
        self.staff.push(sf);
        cid
    }

    pub fn set_staff(&mut self, cid: CommanderId, staff: u32) {
        if cid.0 < self.staff.len() {
            self.staff[cid.0] = staff;
        }
    }

    pub fn set_staff_full(&mut self, cid: CommanderId, staff_full: u32) {
        if cid.0 < self.staff_full.len() {
            self.staff_full[cid.0] = staff_full;
        }
    }

    pub fn commander_staff_efficiency(&self, cid: CommanderId) -> f32 {
        let sf = self.staff_full.get(cid.0).copied().unwrap_or(0);
        if sf == 0 {
            1.0
        } else {
            let cur = self.staff.get(cid.0).copied().unwrap_or(sf);
            (cur as f32 / sf as f32).clamp(0.0, 1.0)
        }
    }

    pub fn sync_hq_ledger_authority(&mut self, cid: CommanderId, units: &UnitSoA) {
        let i = cid.0;
        if i >= self.id.len() || !self.alive[i] {
            return;
        }
        if let Some(uid) = self.hq_piece_id[i] {
            if units.alive[uid.0] && units.kind[uid.0] == "hq_squad" {
                let off = units.officer[uid.0];
                let nco = units.nco[uid.0];
                self.nco_deputy_scale[i] = if off > 0 {
                    1.0
                } else if nco > 0 {
                    0.65
                } else {
                    0.0
                };
            } else {
                self.nco_deputy_scale[i] = 0.0;
            }
        }
    }

    pub fn link_subordinate_unit(&mut self, parent: CommanderId, unit: UnitId, units: &mut UnitSoA) {
        if let Some(old_p) = units.direct_commander_id[unit.0] {
            self.subordinate_unit_ids[old_p.0].retain(|&u| u != unit);
        }
        units.direct_commander_id[unit.0] = Some(parent);
        if !self.subordinate_unit_ids[parent.0].contains(&unit) {
            self.subordinate_unit_ids[parent.0].push(unit);
        }
    }

    pub fn link_subordinate_cmd(&mut self, parent: CommanderId, child: CommanderId) {
        if parent == child {
            return;
        }
        if let Some(old_p) = self.parent_commander_id[child.0] {
            self.subordinate_cmd_ids[old_p.0].retain(|&c| c != child);
        }
        self.parent_commander_id[child.0] = Some(parent);
        if !self.subordinate_cmd_ids[parent.0].contains(&child) {
            self.subordinate_cmd_ids[parent.0].push(child);
        }
    }

    pub fn resolve_succession(&mut self, cid: CommanderId, units: &UnitSoA) -> Option<String> {
        let i = cid.0;
        if !self.alive[i] {
            return None;
        }
        let hq_ok = self.hq_piece_id[i].map_or(true, |uid| units.alive[uid.0] && units.persons(uid) > 0);
        if hq_ok {
            return None;
        }
        if let Some(next_rank) = self.rank[i].demotion_next() {
            self.rank[i] = next_rank;
            self.order_churn_score[i] += 1.5;
            return Some(format!("{} decapitated -> demoted to {}", self.str_id[i], next_rank.as_str()));
        }
        self.alive[i] = false;
        Some(format!("{} destroyed with no succession", self.str_id[i]))
    }

    pub fn is_war_council_sync_active(&self, cid: CommanderId, grid: &WorldGrid) -> bool {
        let i = cid.0;
        if !self.alive[i] || self.war_council_sync_hours[i] <= 0.0 {
            return false;
        }
        if let Some(pid) = self.council_partner_id[i] {
            if !self.alive[pid.0] {
                return false;
            }
            let dist = grid.distance_km(
                [self.px[i].0 as f32, self.px[i].1 as f32],
                [self.px[pid.0].0 as f32, self.px[pid.0].1 as f32],
            );
            return dist <= self.council_radius_km[i] + 1e-6;
        }
        true
    }
}

#[inline]
pub fn deterministic_unit_float(seed_tag: &str, unit_id: UnitId, tick: u32) -> f32 {
    let mut h = Sha256::new();
    h.update(seed_tag.as_bytes());
    h.update(unit_id.0.to_le_bytes());
    h.update(tick.to_le_bytes());
    let dig = h.finalize();
    let v = u32::from_le_bytes([dig[0], dig[1], dig[2], dig[3]]);
    (v as f64 / u32::MAX as f64) as f32
}

pub fn tick_overload_faults_5m(units: &mut UnitSoA, dt_hours: f32, tick: u32) -> Vec<(UnitId, &'static str)> {
    let mut faults = Vec::new();
    for i in 0..units.id.len() {
        if !units.alive[i] || units.overload_ratio[i] <= 1.0 + 1e-6 || !units.moved_last_turn[i] {
            continue;
        }
        let ratio = units.overload_ratio[i];
        let base_p = (0.18 * (ratio - 1.0)).min(0.85);
        let p_step = 1.0 - (1.0 - base_p).powf(dt_hours / 0.25);
        let roll = deterministic_unit_float("overload", UnitId(i), tick);
        if roll < p_step {
            let fault_idx = ((roll * 997.0) as usize) % 3;
            let (src, tag, locks, spd) = match fault_idx {
                0 => (ModifierSource::OverloadAxleSnap, "FAULT_AXLE_SNAP", LockFlags::FORBID_MOVE | LockFlags::FORBID_TOW, 0.0),
                1 => (ModifierSource::OverloadGearbox, "FAULT_GEARBOX", LockFlags::FORBID_TOW, 0.45),
                _ => (ModifierSource::OverloadEngineOverheat, "FAULT_ENGINE_OVERHEAT", LockFlags::empty(), 0.65),
            };
            set_modifier_slot(
                &mut units.modifier_slots[i],
                ModifierSlot {
                    source: src,
                    tag,
                    locks,
                    speed_mult: spd,
                    ttl_hours: Some(2.0),
                    ..Default::default()
                },
            );
            faults.push((UnitId(i), tag));
        }
    }
    faults
}

#[derive(Debug, Default)]
pub struct EventQueue {
    pub next_id: u64,
    pub events: Vec<ActiveEvent>,
}

impl EventQueue {
    pub fn push(&mut self, actor_id: UnitId, duration_h: f32, labor_persons: u32, kind: EventKind) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.events.push(ActiveEvent {
            id,
            actor_id,
            remaining_hours: duration_h,
            labor_persons,
            kind,
        });
        id
    }

    pub fn start_reorganize_echelon(
        &mut self,
        units: &mut UnitSoA,
        commanders: &CommanderSoA,
        commander_id: CommanderId,
        commander_piece_id: Option<UnitId>,
        target_echelon: Echelon,
        source_unit_ids: Vec<UnitId>,
        target_specs: Vec<ReorgTargetSpec>,
        strict_cadre_allocation: bool,
        duration_h: Option<f32>,
    ) -> Result<u64, String> {
        if !commanders.alive[commander_id.0] {
            return Err("commander_unavailable".into());
        }
        let team = commanders.team[commander_id.0];
        let valid_srcs: Vec<UnitId> = source_unit_ids
            .into_iter()
            .filter(|&u| u.0 < units.id.len() && units.alive[u.0] && units.team[u.0] == team)
            .collect();
        if valid_srcs.is_empty() {
            return Err("no_source_units".into());
        }
        let cmd_piece = commander_piece_id.or(commanders.hq_piece_id[commander_id.0]);
        let actor = cmd_piece.unwrap_or(valid_srcs[0]);
        let labor: u32 = valid_srcs.iter().map(|&u| units.persons(u)).sum();
        let dur = duration_h.unwrap_or_else(|| target_echelon.reorg_duration_h());
        let eid = self.push(
            actor,
            dur,
            labor,
            EventKind::ReorganizeEchelon {
                commander_id,
                commander_piece_id: cmd_piece,
                target_echelon,
                source_unit_ids: valid_srcs.clone(),
                target_specs,
                strict_cadre_allocation,
            },
        );
        for u in valid_srcs {
            units.reorganizing_event_id[u.0] = Some(eid);
        }
        if let Some(cp) = cmd_piece {
            units.reorganizing_event_id[cp.0] = Some(eid);
        }
        Ok(eid)
    }

    /// Any attack on the highest commanding unit aborts reorganization immediately!
    pub fn notify_unit_attacked(
        &mut self,
        units: &mut UnitSoA,
        commanders: &CommanderSoA,
        attacked_unit: Option<UnitId>,
        attacker_team: TeamId,
        cell_rc: Option<(i32, i32)>,
    ) -> Vec<u64> {
        let mut aborted = Vec::new();
        self.events.retain(|ev| {
            if let EventKind::ReorganizeEchelon {
                commander_id,
                commander_piece_id,
                source_unit_ids,
                ..
            } = &ev.kind
            {
                let c_team = commanders.team[commander_id.0];
                if c_team == attacker_team {
                    return true;
                }
                let hit = match attacked_unit {
                    Some(uid) if *commander_piece_id == Some(uid) => true,
                    _ => {
                        if let Some(rc) = cell_rc {
                            commander_piece_id.is_none() && commanders.px[commander_id.0] == rc
                        } else {
                            false
                        }
                    }
                };
                if hit {
                    aborted.push(ev.id);
                    for &su in source_unit_ids {
                        units.reorganizing_event_id[su.0] = None;
                    }
                    if let Some(cp) = commander_piece_id {
                        units.reorganizing_event_id[cp.0] = None;
                    }
                    return false;
                }
            }
            true
        });
        aborted
    }

    pub fn start_form_command_squad(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        preferred_name: String,
        dissolve_if_empty: bool,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        if (units.nco[actor_id.0] + units.officer[actor_id.0]) == 0 {
            return Err("requires_nco_or_officer_cadre".into());
        }
        if units.kind[actor_id.0] == "hq_squad" {
            return Err("already_command_squad".into());
        }
        let labor = units.persons(actor_id);
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            labor,
            EventKind::FormCommandSquad {
                preferred_name,
                dissolve_if_empty,
            },
        ))
    }

    pub fn start_lay_wire(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_px: (i32, i32),
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        Ok(self.push(actor_id, duration_h.max(0.05), units.persons(actor_id), EventKind::LayWire { target_px }))
    }

    pub fn start_cargo_transfer(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_unit_id: UnitId,
        food_t: f32,
        ammo_t: f32,
        fuel_t: f32,
        equipment: Vec<(String, u32)>,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len()
            || target_unit_id.0 >= units.id.len()
            || !units.alive[actor_id.0]
            || !units.alive[target_unit_id.0]
        {
            return Err("invalid_cargo_transfer_units".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::CargoTransfer {
                target_unit_id,
                food_t,
                ammo_t,
                fuel_t,
                equipment,
            },
        ))
    }

    pub fn start_engineering(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        op: impl Into<String>,
        target_px: (i32, i32),
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        let op_s = op.into();
        let (ax, ay) = units.px[actor_id.0];
        let cheb = (target_px.0 - ax).abs().max((target_px.1 - ay).abs());
        if cheb > 1 {
            return Err("target_out_of_engineering_reach".into());
        }
        if op_s != "fortify" && !units.unit_has_engineer_cap(actor_id) {
            return Err("engineer_capability_required".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::Engineering {
                op: op_s,
                target_px,
            },
        ))
    }

    pub fn start_demolition(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_px: (i32, i32),
        target_type: impl Into<String>,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        let (ax, ay) = units.px[actor_id.0];
        let cheb = (target_px.0 - ax).abs().max((target_px.1 - ay).abs());
        if cheb > 1 {
            return Err("target_out_of_demolition_reach".into());
        }
        if !units.unit_has_engineer_cap(actor_id) {
            return Err("engineer_capability_required".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::Demolition {
                target_px,
                target_type: target_type.into(),
            },
        ))
    }

    pub fn cancel_event(
        &mut self,
        units: &mut UnitSoA,
        event_id: u64,
        owner_team: TeamId,
    ) -> Result<(), String> {
        let Some(pos) = self.events.iter().position(|ev| ev.id == event_id) else {
            return Err("event_not_found".into());
        };
        let actor = self.events[pos].actor_id;
        if actor.0 < units.team.len() && units.team[actor.0] != owner_team {
            return Err("forbidden_cross_team_event_cancel".into());
        }
        let ev = self.events.remove(pos);
        if let EventKind::ReorganizeEchelon {
            commander_piece_id,
            source_unit_ids,
            ..
        } = ev.kind
        {
            for su in source_unit_ids {
                if su.0 < units.reorganizing_event_id.len() {
                    units.reorganizing_event_id[su.0] = None;
                }
            }
            if let Some(cp) = commander_piece_id {
                if cp.0 < units.reorganizing_event_id.len() {
                    units.reorganizing_event_id[cp.0] = None;
                }
            }
        }
        Ok(())
    }

    pub fn start_radio_decoy(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_px: (i32, i32),
        ttl_h: f32,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::RadioDecoy { target_px, ttl_h },
        ))
    }
}

pub fn complete_reorganize_echelon(
    grid: &WorldGrid,
    units: &mut UnitSoA,
    commanders: &mut CommanderSoA,
    commander_id: CommanderId,
    commander_piece_id: Option<UnitId>,
    target_echelon: Echelon,
    source_unit_ids: &[UnitId],
    target_specs: &[ReorgTargetSpec],
    strict_cadre_allocation: bool,
) -> Vec<UnitId> {
    let team = commanders.team[commander_id.0];
    let anchor = commander_piece_id
        .map(|u| units.px[u.0])
        .unwrap_or(commanders.px[commander_id.0]);
    let spawn_px = grid.find_nearest_literal_ground_px(anchor.0, anchor.1);

    let mut pool_enl = 0u32;
    let mut pool_nco = 0u32;
    let mut pool_off = 0u32;
    let mut pool_eq: FxHashMap<String, u32> = FxHashMap::default();
    let mut pool_food = 0.0f32;
    let mut pool_ammo = 0.0f32;
    let mut pool_fuel = 0.0f32;
    let mut org_num = 0.0f32;
    let mut org_den = 0.0f32;
    let mut ch = commanders.radio_channel[commander_id.0];

    for &uid in source_unit_ids {
        let i = uid.0;
        if !units.alive[i] {
            continue;
        }
        units.reorganizing_event_id[i] = None;
        ch = units.radio_channel[i];
        let p = units.persons(uid).max(1) as f32;
        pool_enl += units.enlisted[i];
        pool_nco += units.nco[i];
        pool_off += units.officer[i];
        org_num += units.org[i] * p;
        org_den += p;
        for (m, &n) in &units.equipment[i] {
            *pool_eq.entry(m.clone()).or_insert(0) += n;
        }
        pool_food += units.food_t[i];
        pool_ammo += units.ammo_t[i];
        pool_fuel += units.fuel_t[i];

        units.enlisted[i] = 0;
        units.nco[i] = 0;
        units.officer[i] = 0;
        units.cadre[i] = 0;
        units.alive[i] = false;
        commanders.subordinate_unit_ids[commander_id.0].retain(|&x| x != uid);
    }
    if let Some(cp) = commander_piece_id {
        units.reorganizing_event_id[cp.0] = None;
    }

    let mean_org = org_num / org_den.max(1.0);
    let pool_cad = pool_nco + pool_off;
    let specs_owned: Vec<ReorgTargetSpec>;
    let specs_ref = if target_specs.is_empty() {
        let total = pool_enl + pool_cad;
        let eq_vec: Vec<(String, u32)> = pool_eq.iter().map(|(k, &v)| (k.clone(), v)).collect();
        specs_owned = vec![ReorgTargetSpec {
            str_id: format!("{}-{}-reorg", commanders.str_id[commander_id.0], target_echelon.as_str()),
            name: format!("整编{}", target_echelon.as_str()),
            echelon: target_echelon,
            enlisted: pool_enl.max(if total == 0 { 0 } else { 0 }),
            cadre: pool_cad,
            equipment: eq_vec,
        }];
        &specs_owned
    } else {
        target_specs
    };

    let mut rem_e = pool_enl;
    let mut rem_n = pool_nco;
    let mut rem_o = pool_off;
    let mut rem_eq = pool_eq;
    let mut spawned = Vec::new();
    for (idx, sp) in specs_ref.iter().enumerate() {
        let te = sp.enlisted.min(rem_e);
        rem_e -= te;
        // 分配骨干：优先分配军官，再分配士官，防止降级为0
        let to = sp.cadre.min(rem_o);
        rem_o -= to;
        let tn = (sp.cadre.saturating_sub(to)).min(rem_n);
        rem_n -= tn;

        let mut eq_map = FxHashMap::default();
        for (m, n) in &sp.equipment {
            let av = (*n).min(*rem_eq.get(m).unwrap_or(&0));
            if av > 0 {
                eq_map.insert(m.clone(), av);
                *rem_eq.entry(m.clone()).or_insert(0) -= av;
            }
        }
        let kind = if to > 0 && sp.echelon >= Echelon::Company {
            "hq_squad"
        } else {
            "infantry_squad"
        };
        let uid = units.spawn_unit_4ledger(
            &sp.str_id, &sp.name, team, spawn_px, sp.echelon, kind, te, tn, to, eq_map, ch,
        );
        units.pos[uid.0] = [
            spawn_px.0 as f32 + ((idx % 3) as f32 - 1.0) * 0.08,
            spawn_px.1 as f32 + ((idx / 3) as f32 - 1.0) * 0.08,
        ];
        units.org[uid.0] = mean_org.min(units.org_cap[uid.0]);
        commanders.link_subordinate_unit(commander_id, uid, units);
        spawned.push(uid);
    }

    if let Some(&last_u) = spawned.last() {
        let rem_cad = rem_n + rem_o;
        if rem_e > 0 || rem_eq.values().sum::<u32>() > 0 || (rem_cad > 0 && !strict_cadre_allocation) {
            let i = last_u.0;
            units.enlisted[i] += rem_e;
            if !strict_cadre_allocation {
                units.nco[i] += rem_n;
                units.officer[i] += rem_o;
                units.cadre[i] = units.nco[i] + units.officer[i];
            }
            for (m, n) in rem_eq {
                if n > 0 {
                    *units.equipment[i].entry(m).or_insert(0) += n;
                }
            }
            units.refresh_caps(last_u);
        }
    }

    let tot_p = spawned.iter().map(|&u| units.persons(u)).sum::<u32>().max(1) as f32;
    for &u in &spawned {
        let frac = units.persons(u) as f32 / tot_p;
        units.food_t[u.0] = pool_food * frac;
        units.ammo_t[u.0] = pool_ammo * frac;
        units.fuel_t[u.0] = pool_fuel * frac;
    }
    spawned
}
