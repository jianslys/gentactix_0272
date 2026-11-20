use crate::commands::TacticalCommand;
use crate::math_ops::WorldPos;
use crate::time_machine::TimeCategory;
use crate::types::{Team, UnitId};
use serde::{Deserialize, Serialize};

/// Maximum cross-tick schedule horizon: 60 days = 5,184,000 s = 324,000 ticks @ 16s.
pub const MAX_STEP_CACHE_DAYS: u64 = 60;
pub const MAX_STEP_CACHE_HORIZON_TICKS: u64 = 324_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldInstanceKind {
    TimedCharge,
    Mine,
    Fortification,
    DroppedItem,
    DroppedWeapon,
    Corpse,
}

/// Physical world instance created immediately inside `WorldState`.
/// Visibility of ALL instances is governed strictly by the I-field (`I_intel`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldInstanceRecord {
    pub instance_id: u64,
    pub kind: WorldInstanceKind,
    pub team: Team,
    #[serde(default)]
    pub owner_unit_id: Option<UnitId>,
    pub pos: WorldPos,
    pub created_tick: u64,
    pub active: bool,
    pub label: String,
    #[serde(default)]
    pub blast_radius_m: f32,
    #[serde(default)]
    pub blast_kpa: f32,
    #[serde(default)]
    pub crater_depth_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveZoneKind {
    Bombardment,
    Assault,
    PermittedAo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveZoneSpec {
    pub kind: WaveZoneKind,
    pub center: WorldPos,
    pub radius_m: f32,
    #[serde(default)]
    pub waypoints: Vec<WorldPos>,
    #[serde(default = "default_g0_weight")]
    pub g0_weight: f32,
}

const fn default_g0_weight() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveWaveDirective {
    pub wave_id: u64,
    pub team: Team,
    pub activated_tick: u64,
    pub zones: Vec<WaveZoneSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduledEventKind {
    CommandTicket {
        cmd_sec_in_tick: u32,
        category: Option<TimeCategory>,
        unit_name: String,
        command: Box<TacticalCommand>,
    },
    TimedDetonation {
        instance_id: u64,
        team: Team,
        pos: WorldPos,
        blast_radius_m: f32,
        blast_kpa: f32,
        crater_depth_m: f32,
    },
    CommanderWave {
        wave_id: u64,
        team: Team,
        zones: Vec<WaveZoneSpec>,
    },
}

/// Pure tick-trigger entry inside `WorldState.step_cache`.
/// `StepCache` ONLY schedules *when* things fire/detonate/activate; it never filters visibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledStepEvent {
    pub event_id: u64,
    pub source_tick: u64,
    pub target_tick: u64,
    pub exact_ms: u32,
    pub team: Team,
    pub kind: ScheduledEventKind,
}
