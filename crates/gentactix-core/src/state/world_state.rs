use crate::combat_logistics::{AirWing, Battery, Depot, FireFields};
use crate::command_log::{DownlinkPipeline, OperationalZone};
use crate::fields::{SchmidtPostureState, UnitZohDirectiveFilter};
use crate::geometry::tactical_instance::{AugmentedArcIntegrator, DetonatorSwitchEntity};
use crate::state::{SimDerivedState, SimTickScratch};
use crate::units_ecs::{CommanderId, CommanderSoA, Echelon, EventQueue, UnitId, UnitSoA};
use crate::world_grid::WorldGrid;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

pub const SUBTICK_SECONDS: f32 = 16.0;
pub const SUBTICK_HOURS: f32 = SUBTICK_SECONDS / 3600.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtickReport {
    pub subtick: u32,
    pub clock_hours: f32,
    pub moved_units: usize,
    pub attacking_units: usize,
    pub delivered_orders: usize,
    pub completed_events: usize,
    pub air_sorties: usize,
    pub logi_conservation_error: f32,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnReport {
    pub turn: u32,
    pub clock_hours: f32,
    pub subtick_count: u32,
    pub total_hours: f32,
    pub last_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectCombatLogEntry {
    pub turn: u32,
    pub clock_hours: f32,
    pub actor_id: String,
    pub actor_name: String,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub kind: String, // "fire" | "move" | "order"
    pub weapon_model: String,
    pub dist_m: f32,
    pub dispersion_m: f32,
    pub hit_prob: f32,
    pub casualties: u32,
    pub org_loss: f32,
    pub carry_loss_after: f32,
    pub ammo_used_t: f32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaypointSegmentConfig {
    pub start_idx: usize,
    pub goal: [f32; 2],
    pub action: String,    // "move" | "assault" | "attack" | "stop"
    pub move_mode: String, // "normal" | "fast" | "slow"
    pub fire_mode: String, // "stop_to_fire" | "fire_on_the_move" | "rapid" | "normal" | "probing" | "precision"
    pub follow_target: bool,
    pub target_unit_id: Option<String>,
    #[serde(default = "default_duration_sec")]
    pub duration_sec: u32,
    #[serde(default = "default_priority_target_str")]
    pub priority_target: String,
}

fn default_move_mode() -> String { "normal".to_string() }
fn default_fire_mode() -> String { "stop_to_fire".to_string() }
fn default_duration_sec() -> u32 { 300 }
fn default_priority_target_str() -> String { "any".to_string() }

pub fn parse_priority_target(s: &str) -> crate::world_grid::PriorityTarget {
    let s_clean = s.trim().to_ascii_lowercase();
    if s_clean == "armor" {
        crate::world_grid::PriorityTarget::Armor
    } else if s_clean == "infantry" {
        crate::world_grid::PriorityTarget::Infantry
    } else if s_clean == "heavy_weapon" || s_clean == "heavyweapon" {
        crate::world_grid::PriorityTarget::HeavyWeapon
    } else if let Some(uid) = s.strip_prefix("unit:") {
        crate::world_grid::PriorityTarget::Unit(uid.trim().to_string())
    } else {
        crate::world_grid::PriorityTarget::Any
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectOrderPlan {
    pub action: String, // "move" | "attack" | "assault" | "retreat" | "stop"
    pub waypoints: Vec<[f32; 2]>,
    pub target_pos: Option<[f32; 2]>,
    pub target_unit_id: Option<String>,
    #[serde(default = "default_move_mode")]
    pub move_mode: String,
    #[serde(default = "default_fire_mode")]
    pub fire_mode: String,
    #[serde(default)]
    pub follow_target: bool,
    #[serde(default)]
    pub segments: Vec<WaypointSegmentConfig>,
    #[serde(default = "default_duration_sec")]
    pub duration_sec: u32,
    #[serde(default = "default_priority_target_str")]
    pub priority_target: String,
    pub last_los_blocked: bool,
    pub out_of_ammo: bool,
    pub last_fire_summary: String,
    pub weapon_model: String,
    pub dispersion_m: f32,
    pub hit_prob: f32,
    pub last_casualties: u32,
    pub last_org_loss: f32,
    pub last_ammo_used_t: f32,
}

pub struct SimWorld {
    // --- Container 1: Authoritative World State ---
    pub grid: WorldGrid,
    pub units: UnitSoA,
    pub commanders: CommanderSoA,
    pub zones: Vec<OperationalZone>,
    pub downlink: DownlinkPipeline,
    pub events: EventQueue,
    pub depots: Vec<Depot>,
    pub batteries: Vec<Battery>,
    pub wings: Vec<AirWing>,
    pub fire_fields: FireFields,
    pub direct_unit_plans: FxHashMap<String, DirectOrderPlan>,
    pub direct_cmd_plans: FxHashMap<String, DirectOrderPlan>,
    pub direct_cmd_pos: FxHashMap<String, [f32; 2]>,
    pub debug_full_friendly_fire: bool,
    pub direct_combat_logs: Vec<DirectCombatLogEntry>,
    pub weapon_cooldowns: FxHashMap<String, f32>,
    pub turn: u32,
    pub subtick: u32,
    pub epoch_seconds: u64, // 默认 23400 (即 06:30:00)
    pub clock_seconds: u64, // 始终满足 epoch_seconds + (self.subtick as u64 * 16)
    pub clock_hours: f32,
    pub role_echelon: Echelon,
    pub night: bool,
    pub pre_game_editable: bool,
    pub r16_source: Option<String>,
    pub unit_postures: Vec<SchmidtPostureState>,
    pub unit_suppression: Vec<f32>,
    pub cell_h_blast: Vec<f32>,
    pub cell_s_supp: Vec<f32>,
    pub unit_zoh_filters: FxHashMap<u32, UnitZohDirectiveFilter>,
    pub engineer_integrators: FxHashMap<u32, AugmentedArcIntegrator>,
    pub detonator_switches: Vec<DetonatorSwitchEntity>,
    pub base_control_radius_km: Vec<f32>,

    // --- Container 2 & 3: Tripartite Derived & Ephemeral Scratch ---
    pub derived: SimDerivedState,
    pub scratch: SimTickScratch,
    pub last_tick_phases: usize,
}

impl SimWorld {
    pub fn new(grid: WorldGrid) -> Self {
        let derived = SimDerivedState::new(grid.rows, grid.cols);
        let epoch = 23400u64;
        let n_cells = grid.rows * grid.cols;
        let cell_h_blast = vec![0.0f32; n_cells];
        let cell_s_supp = vec![0.0f32; n_cells];
        Self {
            grid,
            units: UnitSoA::new(),
            commanders: CommanderSoA::new(),
            zones: Vec::new(),
            downlink: DownlinkPipeline::default(),
            events: EventQueue::default(),
            depots: Vec::new(),
            batteries: Vec::new(),
            wings: Vec::new(),
            fire_fields: FireFields::default(),
            direct_unit_plans: FxHashMap::default(),
            direct_cmd_plans: FxHashMap::default(),
            direct_cmd_pos: FxHashMap::default(),
            debug_full_friendly_fire: false,
            direct_combat_logs: Vec::new(),
            weapon_cooldowns: FxHashMap::default(),
            turn: 0,
            subtick: 0,
            epoch_seconds: epoch,
            clock_seconds: epoch,
            clock_hours: (epoch as f64 / 3600.0) as f32,
            role_echelon: Echelon::Division,
            night: false,
            pre_game_editable: true,
            r16_source: None,
            unit_postures: Vec::new(),
            unit_suppression: Vec::new(),
            cell_h_blast,
            cell_s_supp,
            unit_zoh_filters: FxHashMap::default(),
            engineer_integrators: FxHashMap::default(),
            detonator_switches: Vec::new(),
            base_control_radius_km: Vec::new(),
            derived,
            scratch: SimTickScratch::default(),
            last_tick_phases: 0,
        }
    }

    pub fn ensure_aux_states(&mut self) {
        let n = self.units.id.len();
        if self.unit_postures.len() < n {
            self.unit_postures.resize(n, SchmidtPostureState::Active);
        }
        if self.unit_suppression.len() < n {
            self.unit_suppression.resize(n, 0.0);
        }
        if self.base_control_radius_km.len() < n {
            for i in self.base_control_radius_km.len()..n {
                self.base_control_radius_km.push(self.units.control_radius_km[i]);
            }
        }
        if self.derived.unit_radio_t_diff.len() < n {
            self.derived.unit_radio_t_diff.resize(n, 1.0);
        }
        let total_cells = self.grid.rows * self.grid.cols;
        if self.cell_h_blast.len() < total_cells {
            self.cell_h_blast.resize(total_cells, 0.0);
        }
        if self.cell_s_supp.len() < total_cells {
            self.cell_s_supp.resize(total_cells, 0.0);
        }
    }

    pub fn rebuild_derived_state(&mut self) {
        self.derived.rebuild_if_needed(&self.grid, &self.units);
    }

    pub fn record_blast_and_suppression(&mut self, t_px: (i32, i32), total_pts: f32, target_unit: Option<UnitId>) {
        self.ensure_aux_states();
        if self.grid.in_bounds(t_px.0, t_px.1) {
            let cidx = self.grid.idx(t_px.0, t_px.1);
            if cidx < self.cell_h_blast.len() {
                self.cell_h_blast[cidx] += total_pts * 15.0;
                self.cell_s_supp[cidx] += total_pts * 1.5;
            }
        }
        if let Some(tid) = target_unit {
            if tid.0 < self.unit_suppression.len() {
                self.unit_suppression[tid.0] += total_pts * 0.8;
                let cur = self.unit_postures[tid.0];
                self.unit_postures[tid.0] = cur.transition(self.unit_suppression[tid.0]).transition(self.unit_suppression[tid.0]);
            }
        }
    }

    #[inline]
    pub fn pos_to_px(&self, pos: [f32; 2]) -> (i32, i32) {
        let r = (pos[0].round() as i32).clamp(0, self.grid.rows as i32 - 1);
        let c = (pos[1].round() as i32).clamp(0, self.grid.cols as i32 - 1);
        (r, c)
    }

    #[inline]
    pub fn elevation_at_pos(&self, pos: [f32; 2]) -> f32 {
        let (r, c) = self.pos_to_px(pos);
        if self.grid.in_bounds(r, c) {
            self.grid.elevation_m[self.grid.idx(r, c)]
        } else {
            0.0
        }
    }

    pub fn set_role(&mut self, echelon: Echelon) -> f32 {
        self.role_echelon = echelon;
        echelon.default_role_hours()
    }

    pub fn commander_pos_f32(&self, cid: CommanderId) -> [f32; 2] {
        let sid = &self.commanders.str_id[cid.0];
        self.direct_cmd_pos
            .get(sid)
            .copied()
            .unwrap_or_else(|| [self.commanders.px[cid.0].0 as f32, self.commanders.px[cid.0].1 as f32])
    }
}
