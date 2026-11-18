//! `sim_loop` module: 5-minute (1/12 h) deterministic subtick orchestrator (`SimWorld`),
//! multi-subtick `run_turn`, SHA-256 `state_fingerprint`, and `.r16` platoon / demo world builders.

use crate::combat_logistics::{
    apply_registration_to_cells, cast_los_ray_dda, dispatch_logistics_step,
    resolve_cell_strike, resolve_cell_strike_bucketed, resolve_registration_bonus, run_air_subtick_1h,
    AirWing, Battery, Depot, FireFields,
};
use crate::command_log::{
    radio_channel_idx, update_war_councils, CommanderDriver, DownlinkPipeline, MathRecursiveDriver,
    OperationalZone,
};
use crate::spatial_autonomy::{step_world_autonomy_5m, SpatialBucketGrid, ORG_ROUT_THRESHOLD};
use crate::units_ecs::{
    aggregate_modifiers, base_reload_sec_for_weapon, classify_unit_mask, complete_reorganize_echelon, equipment_spec,
    tick_overload_faults_5m, ActiveEvent, AtomicAction, CommanderId, CommanderSoA, Echelon,
    EventKind, EventQueue, LockFlags, Rank, UnitId, UnitSoA, UnitState,
};
use crate::fields::{
    run_deterministic_multi_capacity_auction, AuctionBid, DeygoutRadioOperator, SchmidtPostureState,
    UnitZohDirectiveFilter, ALPHA_BLAST, ALPHA_SUPP,
};
use crate::geometry::tactical_instance::{AugmentedArcIntegrator, DetonatorSwitchEntity};
use crate::world_grid::{CellModEntry, FeatureFlags, MobilityClass, TeamId, WorldGrid, FOREST, MOUNTAIN, PLAIN, WATER};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::Path;

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

#[derive(Copy, Clone)]
struct AStarNode {
    cost: f32,
    f: f32,
    r: i32,
    c: i32,
}

impl Eq for AStarNode {}
impl PartialEq for AStarNode {
    fn eq(&self, other: &Self) -> bool {
        self.r == other.r && self.c == other.c
    }
}
impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f)
    }
}
impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn plan_grid_path_f32(grid: &WorldGrid, start: [f32; 2], goal: [f32; 2], mob: MobilityClass) -> Vec<[f32; 2]> {
    let rows = grid.rows as i32;
    let cols = grid.cols as i32;
    let sr = (start[0].round() as i32).clamp(0, rows - 1);
    let sc = (start[1].round() as i32).clamp(0, cols - 1);
    let gr = (goal[0].round() as i32).clamp(0, rows - 1);
    let gc = (goal[1].round() as i32).clamp(0, cols - 1);
    let clamped_goal = [goal[0].clamp(0.0, (rows - 1) as f32), goal[1].clamp(0.0, (cols - 1) as f32)];
    if sr == gr && sc == gc {
        return vec![clamped_goal];
    }

    let n = (rows * cols) as usize;
    let mut g_score = vec![f32::INFINITY; n];
    let mut parent = vec![-1i32; n];
    let mut heap = BinaryHeap::new();
    let s_idx = (sr * cols + sc) as usize;
    let g_idx = (gr * cols + gc) as usize;
    g_score[s_idx] = 0.0;
    heap.push(AStarNode {
        cost: 0.0,
        f: ((gr - sr) as f32).hypot((gc - sc) as f32),
        r: sr,
        c: sc,
    });

    const DIRS: [(i32, i32); 8] = [
        (-1, 0), (1, 0), (0, -1), (0, 1),
        (-1, -1), (-1, 1), (1, -1), (1, 1),
    ];

    let mut found = false;
    while let Some(cur) = heap.pop() {
        let c_idx = (cur.r * cols + cur.c) as usize;
        if c_idx == g_idx {
            found = true;
            break;
        }
        if cur.cost > g_score[c_idx] + 1e-5 {
            continue;
        }
        for &(dr, dc) in &DIRS {
            let nr = cur.r + dr;
            let nc = cur.c + dc;
            if nr < 0 || nr >= rows || nc < 0 || nc >= cols {
                continue;
            }
            let step_cost = grid.move_cost_step(cur.r, cur.c, nr, nc, mob, 1.0);
            if !step_cost.is_finite() {
                continue;
            }
            let dist_mult = if dr != 0 && dc != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            let forest_pen = 1.0 + grid.forest_density_at(nr, nc) * 0.65;
            let next_g = cur.cost + step_cost * dist_mult * forest_pen;
            let n_idx = (nr * cols + nc) as usize;
            if next_g + 1e-5 < g_score[n_idx] {
                g_score[n_idx] = next_g;
                parent[n_idx] = c_idx as i32;
                let h = ((gr - nr) as f32).hypot((gc - nc) as f32) * 0.25 * (grid.cell_m / 1000.0);
                heap.push(AStarNode { cost: next_g, f: next_g + h, r: nr, c: nc });
            }
        }
    }

    if !found {
        return vec![clamped_goal];
    }

    let mut cells = Vec::new();
    let mut cur = g_idx as i32;
    while cur >= 0 && cur != s_idx as i32 {
        let r = cur / cols;
        let c = cur % cols;
        cells.push([r as f32, c as f32]);
        cur = parent[cur as usize];
    }
    cells.reverse();
    if let Some(last) = cells.last_mut() {
        *last = clamped_goal;
    } else {
        cells.push(clamped_goal);
    }
    cells
}

pub struct SimWorld {
    pub grid: WorldGrid,
    pub units: UnitSoA,
    pub commanders: CommanderSoA,
    pub buckets: SpatialBucketGrid,
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
}

impl SimWorld {
    pub fn new(grid: WorldGrid) -> Self {
        let buckets = SpatialBucketGrid::new(grid.rows, grid.cols);
        let epoch = 23400u64;
        let n_cells = grid.rows * grid.cols;
        let cell_h_blast = vec![0.0f32; n_cells];
        let cell_s_supp = vec![0.0f32; n_cells];
        Self {
            grid,
            units: UnitSoA::new(),
            commanders: CommanderSoA::new(),
            buckets,
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
        let total_cells = self.grid.rows * self.grid.cols;
        if self.cell_h_blast.len() < total_cells {
            self.cell_h_blast.resize(total_cells, 0.0);
        }
        if self.cell_s_supp.len() < total_cells {
            self.cell_s_supp.resize(total_cells, 0.0);
        }
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

    fn advance_pos_along_waypoints(
        grid: &WorldGrid,
        cur: [f32; 2],
        waypoints: &mut Vec<[f32; 2]>,
        base_cells_per_5m: f32,
        forest_drag: f32,
    ) -> ([f32; 2], bool, f32) {
        let mut pos = cur;
        let r_i = (pos[0].round() as i32).clamp(0, grid.rows as i32 - 1);
        let c_i = (pos[1].round() as i32).clamp(0, grid.cols as i32 - 1);
        let fd = grid.forest_density_at(r_i, c_i);
        let mut budget = base_cells_per_5m / (1.0 + fd * forest_drag);
        let mut moved = false;
        let mut last_deg = 0.0f32;
        while budget > 1e-4 && !waypoints.is_empty() {
            let tgt = waypoints[0];
            let dr = tgt[0] - pos[0];
            let dc = tgt[1] - pos[1];
            let dist = dr.hypot(dc);
            if dist <= 1e-4 {
                waypoints.remove(0);
                continue;
            }
            last_deg = dc.atan2(-dr).to_degrees();
            if dist <= budget {
                pos = tgt;
                budget -= dist;
                waypoints.remove(0);
                moved = true;
            } else {
                let s = budget / dist;
                pos = [pos[0] + dr * s, pos[1] + dc * s];
                budget = 0.0;
                moved = true;
            }
        }
        (pos, moved, last_deg)
    }

    pub fn advance_unit_along_waypoints_dt(
        &self,
        uid: UnitId,
        mut pos: [f32; 2],
        waypoints: &mut Vec<[f32; 2]>,
        action: &str,
        move_mode: &str,
        dt_hours: f32,
    ) -> ([f32; 2], bool, f32, f32) {
        let mut rem_h = dt_hours;
        let mut moved = false;
        let mut last_deg = self.units.facing_deg[uid.0];
        let rows = self.grid.rows as i32;
        let cols = self.grid.cols as i32;
        while rem_h > 1e-6 && !waypoints.is_empty() {
            let tgt = waypoints[0];
            let dr = tgt[0] - pos[0];
            let dc = tgt[1] - pos[1];
            let dist_px = dr.hypot(dc);
            if dist_px <= 1e-4 {
                waypoints.remove(0);
                continue;
            }
            let r0 = (pos[0].round() as i32).clamp(0, rows - 1);
            let c0 = (pos[1].round() as i32).clamp(0, cols - 1);
            let probe = 1.0f32.min(dist_px) / dist_px;
            let r1 = ((pos[0] + dr * probe).round() as i32).clamp(0, rows - 1);
            let c1 = ((pos[1] + dc * probe).round() as i32).clamp(0, cols - 1);
            let spd_kmh = self
                .units
                .effective_speed_kmh(uid, &self.grid, r0, c0, r1, c1, action, move_mode);
            if spd_kmh <= 0.0 {
                break;
            }
            let px_per_h = spd_kmh * 1000.0 / self.grid.cell_m;
            let budget_px = px_per_h * rem_h;
            let step_px = dist_px.min(budget_px).min(0.95);
            if step_px <= 1e-5 {
                break;
            }
            last_deg = dc.atan2(-dr).to_degrees();
            if step_px >= dist_px - 1e-4 {
                pos = tgt;
                waypoints.remove(0);
            } else {
                let s = step_px / dist_px;
                pos = [pos[0] + dr * s, pos[1] + dc * s];
            }
            rem_h -= step_px / px_per_h;
            moved = true;
        }
        (pos, moved, last_deg, rem_h.max(0.0))
    }

    pub fn advance_unit_along_waypoints_5m(
        &self,
        uid: UnitId,
        pos: [f32; 2],
        waypoints: &mut Vec<[f32; 2]>,
        action: &str,
        move_mode: &str,
        dt_hours: f32,
    ) -> ([f32; 2], bool, f32) {
        let (p, m, d, _) = self.advance_unit_along_waypoints_dt(uid, pos, waypoints, action, move_mode, dt_hours);
        (p, m, d)
    }

    pub fn estimate_path_time_minutes(
        &self,
        uid_opt: Option<UnitId>,
        mob: MobilityClass,
        mut pos: [f32; 2],
        waypoints: &[[f32; 2]],
        action: &str,
        move_mode: &str,
    ) -> (f32, f32, f32) {
        let mut total_h = 0.0f32;
        let mut total_km = 0.0f32;
        let rows = self.grid.rows as i32;
        let cols = self.grid.cols as i32;
        for &tgt in waypoints {
            loop {
                let dr = tgt[0] - pos[0];
                let dc = tgt[1] - pos[1];
                let dist_px = dr.hypot(dc);
                if dist_px <= 1e-4 {
                    pos = tgt;
                    break;
                }
                let step_px = dist_px.min(0.95);
                let s = step_px / dist_px;
                let nxt = [pos[0] + dr * s, pos[1] + dc * s];
                let r0 = (pos[0].round() as i32).clamp(0, rows - 1);
                let c0 = (pos[1].round() as i32).clamp(0, cols - 1);
                let r1 = (nxt[0].round() as i32).clamp(0, rows - 1);
                let c1 = (nxt[1].round() as i32).clamp(0, cols - 1);
                let spd = if let Some(u) = uid_opt {
                    let s_u = self
                        .units
                        .effective_speed_kmh(u, &self.grid, r0, c0, r1, c1, action, move_mode);
                    if s_u <= 0.0 { 0.5 } else { s_u }
                } else {
                    let tc = crate::world_grid::terrain_cost(mob, self.grid.terrain[self.grid.idx(r1, c1)]);
                    let mode_m = match move_mode {
                        "fast" => 1.25,
                        "slow" | "stealth" => 0.55,
                        _ => 1.0,
                    };
                    (18.0 * mode_m / tc.max(0.35)).clamp(2.0, 48.0)
                };
                let slice_km = step_px * self.grid.cell_m / 1000.0;
                total_km += slice_km;
                total_h += slice_km / spd.max(0.2);
                pos = nxt;
                if step_px >= dist_px - 1e-4 {
                    pos = tgt;
                    break;
                }
            }
        }
        let avg_spd = if total_h > 1e-5 { total_km / total_h } else { 0.0 };
        (total_h * 60.0, total_km, avg_spd)
    }

    pub fn resolve_target_pos_f32(&self, target_id: &str) -> Option<[f32; 2]> {
        if let Some(&cid) = self.commanders.str_to_id.get(target_id) {
            if self.commanders.alive[cid.0] {
                return Some(self.commander_pos_f32(cid));
            }
        }
        if let Some(&uid) = self.units.str_to_id.get(target_id) {
            if self.units.alive[uid.0] && self.units.persons(uid) > 0 {
                return Some(self.units.pos[uid.0]);
            }
        }
        None
    }

    fn push_combat_log(&mut self, entry: DirectCombatLogEntry) {
        self.direct_combat_logs.push(entry);
        if self.direct_combat_logs.len() > 100 {
            let drain_n = self.direct_combat_logs.len() - 100;
            self.direct_combat_logs.drain(0..drain_n);
        }
    }

    pub fn primary_weapon_for_unit(&self, uid: UnitId) -> String {
        self.units.equipment[uid.0]
            .iter()
            .filter(|(_, &cnt)| cnt > 0)
            .max_by(|a, b| equipment_spec(a.0).fire[0].total_cmp(&equipment_spec(b.0).fire[0]))
            .map(|(m, _)| m.clone())
            .unwrap_or_else(|| "rifle".to_string())
    }

    pub fn compute_fire_ballistics(weapon_model: &str, dist_m: f32, los_blocked: bool) -> (f32, f32) {
        let spec = equipment_spec(weapon_model);
        let disp_m = (12.0 + dist_m * 0.14).clamp(15.0, 120.0);
        if los_blocked {
            return (disp_m, 0.0);
        }
        let prob = (1.0 - dist_m / (spec.range_m * 1.12)).clamp(0.08, 0.95);
        (disp_m, prob)
    }

    pub fn set_direct_unit_order(
        &mut self,
        uid: UnitId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
    ) {
        let _ = self.try_set_direct_unit_order_ex(
            uid,
            action,
            goal,
            append,
            target_unit_id,
            "normal",
            "stop_to_fire",
            true,
        );
    }

    pub fn try_set_direct_unit_order_ex(
        &mut self,
        uid: UnitId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
        move_mode: &str,
        fire_mode: &str,
        follow_target: bool,
    ) -> Result<(), String> {
        self.try_set_direct_unit_order_v33(
            uid,
            action,
            goal,
            append,
            target_unit_id,
            move_mode,
            fire_mode,
            follow_target,
            None,
            None,
        )
    }

    pub fn try_set_direct_unit_order_v33(
        &mut self,
        uid: UnitId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
        move_mode: &str,
        fire_mode: &str,
        follow_target: bool,
        duration_sec: Option<u32>,
        priority_target: Option<&str>,
    ) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 && action != "retreat" {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let sname = self.units.name[uid.0].clone();
        let pw = self.primary_weapon_for_unit(uid);
        let mm = if matches!(move_mode, "fast" | "slow" | "stealth") { move_mode } else { "normal" };
        let mob = self.units.mobility[uid.0];
        let fm = fire_mode;
        let dur_sec = duration_sec.unwrap_or(300);
        let prio_tgt = priority_target.unwrap_or("any").to_string();

        if action == "stop" {
            let seg = WaypointSegmentConfig {
                start_idx: 0,
                goal: self.units.pos[uid.0],
                action: "stop".into(),
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: false,
                target_unit_id: None,
                duration_sec: dur_sec,
                priority_target: prio_tgt.clone(),
            };
            if append {
                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                    plan.segments.push(seg);
                    return Ok(());
                }
            }
            self.direct_unit_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "stop".into(),
                    waypoints: Vec::new(),
                    target_pos: None,
                    target_unit_id: None,
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    segments: vec![seg],
                    duration_sec: dur_sec,
                    priority_target: prio_tgt,
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: String::new(),
                    weapon_model: pw.clone(),
                    dispersion_m: 0.0,
                    hit_prob: 0.0,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: None, target_name: None,
                kind: "order".into(), weapon_model: pw, dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: "下达【停止】指令，原地驻留待命。".into(),
            });
            return Ok(());
        }

        if action == "retreat" {
            // 不可逆撤退：设置 0.5 小时（30 分钟）不可控狂奔恢复期
            self.units.retreat_recovery_hours[uid.0] = 0.5;
            let cur = self.units.pos[uid.0];
            let team = self.units.team[uid.0];
            // Compute safest rearward direction away from nearest hostile or toward HQ/rear
            let mut threat = [0.0f32; 2];
            for j in 0..self.units.id.len() {
                if !self.units.alive[j] || self.units.team[j] == team || self.units.persons(UnitId(j)) == 0 {
                    continue;
                }
                let d_km = self.grid.distance_km(cur, self.units.pos[j]);
                if d_km <= 3.0 {
                    let w = (-d_km / 0.8).exp();
                    let dr = self.units.pos[j][0] - cur[0];
                    let dc = self.units.pos[j][1] - cur[1];
                    let dm = dr.hypot(dc).max(1e-4);
                    threat[0] += (dr / dm) * w;
                    threat[1] += (dc / dm) * w;
                }
            }
            let (rx, ry) = if threat[0].hypot(threat[1]) > 1e-3 {
                let m = threat[0].hypot(threat[1]);
                (-threat[0] / m, -threat[1] / m)
            } else {
                match team {
                    crate::world_grid::TeamId::Red => (0.0, -1.0),
                    crate::world_grid::TeamId::Blue => (0.0, 1.0),
                }
            };
            let fallback_goal = goal.unwrap_or_else(|| {
                self.grid.clamp_pos([cur[0] + rx * 8.0, cur[1] + ry * 8.0])
            });
            let seg = plan_grid_path_f32(&self.grid, cur, fallback_goal, mob);
            self.direct_unit_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "retreat".into(),
                    waypoints: seg,
                    target_pos: Some(fallback_goal),
                    target_unit_id: None,
                    move_mode: "fast".into(),
                    fire_mode: "fire_on_the_move".into(),
                    follow_target: false,
                    segments: vec![WaypointSegmentConfig {
                        start_idx: 0,
                        goal: fallback_goal,
                        action: "retreat".into(),
                        move_mode: "fast".into(),
                        fire_mode: "fire_on_the_move".into(),
                        follow_target: false,
                        target_unit_id: None,
                        duration_sec: dur_sec,
                        priority_target: prio_tgt.clone(),
                    }],
                    duration_sec: dur_sec,
                    priority_target: prio_tgt,
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: "溃退撤离中（不可逆恢复期）".into(),
                    weapon_model: pw.clone(),
                    dispersion_m: 0.0,
                    hit_prob: 0.0,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.units.autonomy_state[uid.0] = AtomicAction::Retreat;
            self.units.exposed[uid.0] = true;
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: None, target_name: None,
                kind: "order".into(), weapon_model: pw, dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: format!("执行【不可逆撤退】-> 脱离接触向后狂奔 ({:.1}, {:.1})，恢复期内不可控。", fallback_goal[0], fallback_goal[1]),
            });
            return Ok(());
        }

        if action == "attack" {
            let tgt_pos = target_unit_id
                .as_deref()
                .and_then(|tid| self.resolve_target_pos_f32(tid))
                .or(goal);
            let Some(tp) = tgt_pos else { return Err("missing_attack_target".into()); };
            let dist_m = self.grid.distance_km(self.units.pos[uid.0], tp) * 1000.0;
            let (disp_m, hit_prob) = Self::compute_fire_ballistics(&pw, dist_m, false);
            let tname = target_unit_id.as_deref().map(|tid| self.resolve_target_name(tid));
            let seg = WaypointSegmentConfig {
                start_idx: 0,
                goal: tp,
                action: "attack".into(),
                move_mode: "normal".into(),
                fire_mode: fm.into(),
                follow_target,
                target_unit_id: target_unit_id.clone(),
                duration_sec: dur_sec,
                priority_target: prio_tgt.clone(),
            };
            if append {
                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                    plan.segments.push(seg);
                    return Ok(());
                }
            }
            self.direct_unit_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "attack".into(),
                    waypoints: Vec::new(),
                    target_pos: Some(tp),
                    target_unit_id: target_unit_id.clone(),
                    move_mode: "normal".into(),
                    fire_mode: fm.into(),
                    follow_target,
                    segments: vec![seg],
                    duration_sec: dur_sec,
                    priority_target: prio_tgt,
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: format!("准备射击 · 散布{:.0}m · 命中{:.0}%", disp_m, hit_prob * 100.0),
                    weapon_model: pw.clone(),
                    dispersion_m: disp_m,
                    hit_prob,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: target_unit_id, target_name: tname.clone(),
                kind: "order".into(), weapon_model: pw, dist_m, dispersion_m: disp_m, hit_prob,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: format!(
                    "瞄准 {} ({:.0}m, {}, 散布 {:.0}m, 预计命中 {:.0}%)。",
                    tname.as_deref().unwrap_or("指定坐标"),
                    dist_m,
                    if follow_target { "跟随目标" } else { "固定坐标" },
                    disp_m,
                    hit_prob * 100.0
                ),
            });
            return Ok(());
        }

        let Some(goal_pos) = goal else { return Err("missing_goal".into()); };
        let act_norm = if action == "assault" { "assault" } else { "move" };
        let start = if append {
            self.direct_unit_plans
                .get(&sid)
                .and_then(|p| p.waypoints.last().copied())
                .unwrap_or(self.units.pos[uid.0])
        } else {
            self.units.pos[uid.0]
        };
        let mut seg = plan_grid_path_f32(&self.grid, start, goal_pos, mob);
        if seg.is_empty() && (start[0] - goal_pos[0]).hypot(start[1] - goal_pos[1]) > 0.05 {
            seg.push(goal_pos);
        }
        if append {
            if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                let start_idx = plan.waypoints.len();
                if plan.waypoints.is_empty() && plan.segments.is_empty() {
                    plan.waypoints = seg;
                    plan.action = act_norm.into();
                    plan.target_pos = Some(goal_pos);
                }
                plan.segments.push(WaypointSegmentConfig {
                    start_idx,
                    goal: goal_pos,
                    action: act_norm.into(),
                    move_mode: mm.into(),
                    fire_mode: fm.into(),
                    follow_target,
                    target_unit_id: target_unit_id.clone(),
                    duration_sec: dur_sec,
                    priority_target: prio_tgt.clone(),
                });
                return Ok(());
            }
        }
        self.direct_unit_plans.insert(
            sid.clone(),
            DirectOrderPlan {
                action: act_norm.into(),
                waypoints: seg,
                target_pos: Some(goal_pos),
                target_unit_id: target_unit_id.clone(),
                move_mode: mm.into(),
                fire_mode: fm.into(),
                follow_target,
                segments: vec![WaypointSegmentConfig {
                    start_idx: 0,
                    goal: goal_pos,
                    action: act_norm.into(),
                    move_mode: mm.into(),
                    fire_mode: fm.into(),
                    follow_target,
                    target_unit_id: target_unit_id.clone(),
                    duration_sec: dur_sec,
                    priority_target: prio_tgt.clone(),
                }],
                duration_sec: dur_sec,
                priority_target: prio_tgt,
                last_los_blocked: false,
                out_of_ammo: false,
                last_fire_summary: String::new(),
                weapon_model: pw.clone(),
                dispersion_m: 0.0,
                hit_prob: 0.0,
                last_casualties: 0,
                last_org_loss: 0.0,
                last_ammo_used_t: 0.0,
            },
        );
        self.units.autonomy_state[uid.0] = AtomicAction::Move;
        let mode_cn = match mm { "fast" => "快速", "slow" | "stealth" => "隐蔽渗透", _ => "正常" };
        let label_cn = if act_norm == "assault" {
            format!("进攻/{}/{}", mode_cn, if fm == "fire_on_the_move" { "移动射击" } else { "遇敌停下射击" })
        } else {
            format!("移动/{}", mode_cn)
        };
        self.push_combat_log(DirectCombatLogEntry {
            turn: self.turn, clock_hours: self.clock_hours,
            actor_id: sid, actor_name: sname, target_id: None, target_name: None,
            kind: "order".into(), weapon_model: pw, dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
            casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
            message: format!("下达【{}】指令 -> 目标 ({:.1}, {:.1})。", label_cn, goal_pos[0], goal_pos[1]),
        });
        Ok(())
    }

    pub fn rebuild_plan_waypoints(&mut self, uid: UnitId) {
        let sid = self.units.str_id[uid.0].clone();
        let cur_pos = self.units.pos[uid.0];
        let mob = self.units.mobility[uid.0];
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else { return; };
        if plan.segments.is_empty() {
            plan.action = "stop".into();
            plan.waypoints.clear();
            plan.target_pos = None;
            return;
        }
        let seg = &plan.segments[0];
        plan.action = seg.action.clone();
        plan.move_mode = seg.move_mode.clone();
        plan.fire_mode = seg.fire_mode.clone();
        plan.follow_target = seg.follow_target;
        plan.target_unit_id = seg.target_unit_id.clone();
        plan.duration_sec = seg.duration_sec;
        plan.priority_target = seg.priority_target.clone();
        plan.target_pos = Some(seg.goal);
        if seg.action == "move" || seg.action == "assault" || seg.action == "retreat" {
            plan.waypoints = plan_grid_path_f32(&self.grid, cur_pos, seg.goal, mob);
        } else {
            plan.waypoints.clear();
        }
    }

    pub fn delete_task_step(&mut self, uid: UnitId, step_idx: usize) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else {
            return Err("no_active_plan".into());
        };
        if plan.segments.is_empty() {
            self.direct_unit_plans.remove(&sid);
            return Ok(());
        }
        if step_idx >= plan.segments.len() {
            if plan.segments.len() <= 1 {
                self.direct_unit_plans.remove(&sid);
                return Ok(());
            }
            return Err("step_index_out_of_range".into());
        }
        plan.segments.remove(step_idx);
        if plan.segments.is_empty() {
            self.direct_unit_plans.remove(&sid);
            return Ok(());
        }
        self.rebuild_plan_waypoints(uid);
        Ok(())
    }

    pub fn reorder_task_queue(&mut self, uid: UnitId, from_idx: usize, to_idx: usize) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else {
            return Err("no_active_plan".into());
        };
        if from_idx >= plan.segments.len() || to_idx >= plan.segments.len() {
            return Err("step_index_out_of_range".into());
        }
        plan.segments.swap(from_idx, to_idx);
        let _ = plan;
        self.rebuild_plan_waypoints(uid);
        Ok(())
    }

    pub fn update_queued_segment(
        &mut self,
        uid: UnitId,
        seg_idx: usize,
        action: Option<&str>,
        move_mode: Option<&str>,
        fire_mode: Option<&str>,
        follow_target: Option<bool>,
        goal: Option<[f32; 2]>,
    ) -> Result<(), String> {
        self.update_queued_segment_ex(
            uid,
            seg_idx,
            action,
            move_mode,
            fire_mode,
            follow_target,
            goal,
            None,
            None,
        )
    }

    pub fn update_queued_segment_ex(
        &mut self,
        uid: UnitId,
        seg_idx: usize,
        action: Option<&str>,
        move_mode: Option<&str>,
        fire_mode: Option<&str>,
        follow_target: Option<bool>,
        goal: Option<[f32; 2]>,
        duration_sec: Option<u32>,
        priority_target: Option<&str>,
    ) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else {
            return Err("no_active_plan".into());
        };
        let Some(seg) = plan.segments.get_mut(seg_idx) else {
            return Err("segment_index_out_of_range".into());
        };
        if let Some(a) = action {
            if matches!(a, "move" | "assault" | "attack" | "stop") {
                seg.action = a.to_string();
                if seg_idx == 0 { plan.action = a.to_string(); }
            }
        }
        if let Some(m) = move_mode {
            if matches!(m, "normal" | "fast" | "slow" | "stealth") {
                seg.move_mode = m.to_string();
                if seg_idx == 0 { plan.move_mode = m.to_string(); }
            }
        }
        if let Some(f) = fire_mode {
            seg.fire_mode = f.to_string();
            if seg_idx == 0 { plan.fire_mode = f.to_string(); }
        }
        if let Some(ft) = follow_target {
            seg.follow_target = ft;
            if seg_idx == 0 { plan.follow_target = ft; }
        }
        if let Some(d) = duration_sec {
            seg.duration_sec = d;
            if seg_idx == 0 { plan.duration_sec = d; }
        }
        if let Some(pt) = priority_target {
            seg.priority_target = pt.to_string();
            if seg_idx == 0 { plan.priority_target = pt.to_string(); }
        }
        let mut goal_changed = false;
        if let Some(g) = goal {
            seg.goal = g;
            goal_changed = true;
        }
        let _ = plan;
        if goal_changed {
            self.rebuild_plan_waypoints(uid);
        }
        Ok(())
    }

    pub fn format_unit_active_order_text(&self, uid: UnitId) -> String {
        let i = uid.0;
        let sid = &self.units.str_id[i];
        let name = &self.units.name[i];
        let cmdr_txt = match self.units.direct_commander_id[i] {
            Some(cid) => format!("上级指挥：{}", self.commanders.name[cid.0]),
            None => "无上级直属指挥官（独立行动）".to_string(),
        };
        let deploy_txt = if self.units.active_deploy_line_id[i] != 0 {
            format!("部署防线 ID=#{}", self.units.active_deploy_line_id[i])
        } else {
            "暂无展开战线".to_string()
        };
        if self.units.retreat_recovery_hours[i] > 0.0 {
            return format!(
                "【现行作战命令 · 紧急状态】\n单位：{name} ({sid})\n{cmdr_txt}\n状态：不可逆撤退恢复期（剩余 {:.0} 分钟），单位暂时脱离人工接管并全速向安全区脱离接触。",
                self.units.retreat_recovery_hours[i] * 60.0
            );
        }
        if let Some(p) = self.direct_unit_plans.get(sid) {
            let mm_cn = match p.move_mode.as_str() { "fast" => "快速机动（高暴露）", "slow" => "慢速渗透（低暴露）", _ => "正常行军" };
            let fm_cn = if p.fire_mode == "fire_on_the_move" { "移动射击" } else { "遇敌停下射击" };
            let act_cn = match p.action.as_str() {
                "move" => format!("机动推进 ({mm_cn})"),
                "assault" => format!("战术进攻 ({mm_cn} · {fm_cn})"),
                "attack" => format!("定点火力压制 ({})", if p.follow_target { "持续跟随目标" } else { "锁定坐标点" }),
                "retreat" => "全力撤退脱离接触".to_string(),
                _ => "原地待命戒备".to_string(),
            };
            let goal_txt = p.target_pos.map_or("当前位置".into(), |g| format!("坐标 ({:.1}, {:.1})", g[0], g[1]));
            let q_cnt = p.segments.len().saturating_sub(1);
            format!(
                "【现行作战命令 · 军令摘要】\n受令单位：{name} ({sid})\n{cmdr_txt} | {deploy_txt}\n主力任务：{act_cn}\n当前目标：{goal_txt}（当前段已锁定，后续队列可改航段：{q_cnt} 段）"
            )
        } else {
            format!(
                "【现行作战命令 · 自主态势】\n受令单位：{name} ({sid})\n{cmdr_txt} | {deploy_txt}\n当前动作：{}（随所属作战区 AO 与战线场自动维持警戒，未下达直接覆盖命令）",
                self.units.autonomy_state[i].as_str()
            )
        }
    }

    pub fn resolve_target_name(&self, target_id: &str) -> String {
        if let Some(&cid) = self.commanders.str_to_id.get(target_id) {
            return self.commanders.name[cid.0].clone();
        }
        if let Some(&uid) = self.units.str_to_id.get(target_id) {
            return self.units.name[uid.0].clone();
        }
        target_id.to_string()
    }

    pub fn set_direct_cmd_order(
        &mut self,
        cid: CommanderId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
    ) {
        let sid = self.commanders.str_id[cid.0].clone();
        let sname = self.commanders.name[cid.0].clone();
        if action == "stop" {
            self.direct_cmd_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "stop".into(),
                    waypoints: Vec::new(),
                    target_pos: None,
                    target_unit_id: None,
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    segments: Vec::new(),
                    duration_sec: 300,
                    priority_target: "any".into(),
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: String::new(),
                    weapon_model: "rifle".into(),
                    dispersion_m: 0.0,
                    hit_prob: 0.0,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: None, target_name: None,
                kind: "order".into(), weapon_model: "rifle".into(), dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: "下达【停止】指令，原地驻留。".into(),
            });
            return;
        }
        if action == "attack" {
            let tgt_pos = target_unit_id
                .as_deref()
                .and_then(|tid| self.resolve_target_pos_f32(tid))
                .or(goal);
            let Some(tp) = tgt_pos else { return; };
            let dist_m = self.grid.distance_km(self.commander_pos_f32(cid), tp) * 1000.0;
            let (disp_m, hit_prob) = Self::compute_fire_ballistics("rifle", dist_m, false);
            let seg = WaypointSegmentConfig {
                start_idx: 0,
                goal: tp,
                action: "attack".into(),
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: true,
                target_unit_id: target_unit_id.clone(),
                duration_sec: 16,
                priority_target: "any".into(),
            };
            if append {
                if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                    plan.segments.push(seg);
                    return;
                }
            }
            self.direct_cmd_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "attack".into(),
                    waypoints: Vec::new(),
                    target_pos: Some(tp),
                    target_unit_id: target_unit_id.clone(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: true,
                    segments: vec![seg],
                    duration_sec: 16,
                    priority_target: "any".into(),
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: format!("准备射击 · 散布{:.0}m · 命中{:.0}%", disp_m, hit_prob * 100.0),
                    weapon_model: "rifle".into(),
                    dispersion_m: disp_m,
                    hit_prob,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            return;
        }
        let Some(goal_pos) = goal else { return; };
        let cur_pos = self.commander_pos_f32(cid);
        let start = if append {
            self.direct_cmd_plans
                .get(&sid)
                .and_then(|p| p.waypoints.last().copied())
                .unwrap_or(cur_pos)
        } else {
            cur_pos
        };
        let mut seg = plan_grid_path_f32(&self.grid, start, goal_pos, MobilityClass::Wheeled);
        if seg.is_empty() && (start[0] - goal_pos[0]).hypot(start[1] - goal_pos[1]) > 0.05 {
            seg.push(goal_pos);
        }
        if append {
            if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                let start_idx = plan.waypoints.len();
                if plan.waypoints.is_empty() && plan.segments.is_empty() {
                    plan.action = action.to_string();
                    plan.waypoints = seg;
                    plan.target_pos = Some(goal_pos);
                }
                plan.segments.push(WaypointSegmentConfig {
                    start_idx,
                    goal: goal_pos,
                    action: action.into(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    target_unit_id: None,
                    duration_sec: 300,
                    priority_target: "any".into(),
                });
                return;
            }
        }
        self.direct_cmd_plans.insert(
            sid.clone(),
            DirectOrderPlan {
                action: action.to_string(),
                waypoints: seg,
                target_pos: Some(goal_pos),
                target_unit_id: None,
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: false,
                segments: vec![WaypointSegmentConfig {
                    start_idx: 0,
                    goal: goal_pos,
                    action: action.into(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    target_unit_id: None,
                    duration_sec: 300,
                    priority_target: "any".into(),
                }],
                duration_sec: 300,
                priority_target: "any".into(),
                last_los_blocked: false,
                out_of_ammo: false,
                last_fire_summary: String::new(),
                weapon_model: "rifle".into(),
                dispersion_m: 0.0,
                hit_prob: 0.0,
                last_casualties: 0,
                last_org_loss: 0.0,
                last_ammo_used_t: 0.0,
            },
        );
        self.push_combat_log(DirectCombatLogEntry {
            turn: self.turn, clock_hours: self.clock_hours,
            actor_id: sid, actor_name: sname, target_id: None, target_name: None,
            kind: "order".into(), weapon_model: "rifle".into(), dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
            casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
            message: format!("下达【移动】指令 -> 目标 ({:.1}, {:.1})。", goal_pos[0], goal_pos[1]),
        });
    }

    pub fn step_direct_orders_5m(&mut self) -> usize {
        let mut moved_cnt = 0usize;
        let rows = self.grid.rows as i32;
        let cols = self.grid.cols as i32;
        let mut touched_cd_keys: FxHashSet<String> = FxHashSet::default();

        // 1. Units with direct orders
        let unit_keys: Vec<String> = self.direct_unit_plans.keys().cloned().collect();
        for sid in unit_keys {
            let Some(&uid) = self.units.str_to_id.get(&sid) else { continue; };
            if !self.units.alive[uid.0] || self.units.effective_operator_stats(uid).0 == 0 {
                continue;
            }
            let locks = self.units.effective_locks(uid, &self.events.events);

            let mut time_budget = SUBTICK_SECONDS;
            while time_budget > 0.05 {
                let Some(seg) = self.direct_unit_plans.get(&sid).and_then(|p| p.segments.first().cloned()) else {
                    self.direct_unit_plans.remove(&sid);
                    self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                    break;
                };

                let seg_action = seg.action;
                let seg_mm = seg.move_mode;
                let seg_fm = seg.fire_mode;
                let seg_tgt_uid = seg.target_unit_id;
                let seg_follow = seg.follow_target;
                let seg_goal = seg.goal;
                let seg_dur = seg.duration_sec;
                let seg_priority = seg.priority_target;

                // Sync top-level plan metadata with the active segment
                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                    plan.action = seg_action.clone();
                    plan.move_mode = seg_mm.clone();
                    plan.fire_mode = seg_fm.clone();
                    plan.target_unit_id = seg_tgt_uid.clone();
                    plan.follow_target = seg_follow;
                    plan.duration_sec = seg_dur;
                    plan.priority_target = seg_priority.clone();
                    plan.target_pos = Some(seg_goal);
                }

                if seg_action == "stop" {
                    let time_spent = time_budget.min(seg_dur as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;
                    let should_remove = if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    self.units.autonomy_state[uid.0] = AtomicAction::Hold;

                    if new_dur == 0 {
                        if should_remove {
                            self.direct_unit_plans.remove(&sid);
                            break;
                        }
                        self.rebuild_plan_waypoints(uid);
                        continue;
                    } else {
                        break;
                    }
                }

                if seg_action == "attack" {
                    if locks.contains(LockFlags::FORBID_FIRE) {
                        if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.last_fire_summary = "开火受限(状态锁定)".to_string();
                        }
                        self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                        break;
                    }

                    let time_spent = time_budget.min(seg_dur as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;

                    let tgt_uid_opt = seg_tgt_uid.clone();
                    let resolved = if seg_follow {
                        tgt_uid_opt.as_deref().and_then(|tid| {
                            let cand = self.resolve_target_pos_f32(tid)?;
                            let cand_px = (
                                (cand[0].round() as i32).clamp(0, rows - 1),
                                (cand[1].round() as i32).clamp(0, cols - 1),
                            );
                            let los = cast_los_ray_dda(&self.grid, self.units.px[uid.0], cand_px, self.night);
                            let d_km = self.grid.distance_km(self.units.pos[uid.0], cand);
                            if !los.blocked && d_km <= 18.0 { Some(cand) } else { None }
                        })
                    } else {
                        None
                    };
                    let tgt_pos_opt = resolved.or(Some(seg_goal));
                    if let Some(tp) = tgt_pos_opt {
                        let a_pos = self.units.pos[uid.0];
                        let dr = tp[0] - a_pos[0];
                        let dc = tp[1] - a_pos[1];
                        self.units.facing_deg[uid.0] = dc.atan2(-dr).to_degrees();
                        let a_px = self.units.px[uid.0];
                        let t_px = (
                            (tp[0].round() as i32).clamp(0, rows - 1),
                            (tp[1].round() as i32).clamp(0, cols - 1),
                        );
                        let dist_m = self.grid.distance_km(a_pos, tp) * 1000.0;
                        let los = cast_los_ray_dda(&self.grid, a_px, t_px, self.night);
                        let primary_w = self.primary_weapon_for_unit(uid);
                        let spec = equipment_spec(&primary_w);
                        let (disp_m, hit_prob) = Self::compute_fire_ballistics(&primary_w, dist_m, los.blocked);
                        let mut cas_total = 0u32;
                        let mut org_loss_val = 0.0f32;
                        let mut ammo_used = 0.0f32;
                        let mut carry_after = 0.0f32;
                        let mut is_out_of_ammo = self.units.ammo_t[uid.0] <= 0.0005;

                        // Weapon cooldown math
                        let cd_key = format!("{}:{}", sid, primary_w);
                        touched_cd_keys.insert(cd_key.clone());
                        let cur_cd = self.weapon_cooldowns.entry(cd_key).or_insert(0.0);
                        let base_cycle = base_reload_sec_for_weapon(&primary_w);
                        let (eff_crew, _, _, _) = self.units.effective_operator_stats(uid);
                        let eff_crew_ratio = (eff_crew as f32 / spec.crew.max(1) as f32).clamp(0.2, 1.0);
                        let effective_cycle = base_cycle / eff_crew_ratio;

                        let mut shots = 0u32;
                        let mut time_cursor = 0.0f32;
                        while time_cursor + *cur_cd <= time_spent {
                            time_cursor += *cur_cd;
                            shots += 1;
                            *cur_cd = effective_cycle;
                        }
                        let leftover = time_spent - time_cursor;
                        *cur_cd = (*cur_cd - leftover).max(0.0);

                        let summary = if los.blocked && !spec.indirect {
                            "视线被地形/林地遮挡".to_string()
                        } else if is_out_of_ammo {
                            "弹药耗尽".to_string()
                        } else if shots == 0 {
                            format!("火炮装填中 (冷却还剩 {:.0}s)", *cur_cd)
                        } else {
                            let fire_ratio = self.units.fire_eligible_ratio(uid);
                            if fire_ratio > 0.0 {
                                let pwr = self.units.unit_power(uid, dist_m, true, None, 1.0);
                                let cqb_lethality = (280.0 / dist_m.max(15.0)).powf(1.4).clamp(1.5, 110.0) * 2.6;
                                let mut fire_dt = [0.0f32; 6];
                                let mut base_pts = 0.0f32;
                                let shot_time_h = (time_spent / 3600.0) * (shots as f32).min(4.0);
                                for d in 0..6 {
                                    let step_p = pwr[d] * shot_time_h * fire_ratio;
                                    base_pts += step_p;
                                    fire_dt[d] = step_p * cqb_lethality;
                                }
                                if base_pts > 1e-4 {
                                    ammo_used = self.units.consume_soldier_ammo(uid, base_pts * 0.002);
                                    if self.units.ammo_t[uid.0] <= 0.0005 {
                                        is_out_of_ammo = true;
                                    }
                                    let tag = format!("direct{}|u{}", self.subtick, uid.0);
                                    let a_team = self.units.team[uid.0];
                                    let rep = resolve_cell_strike_bucketed(
                                        &mut self.grid,
                                        &mut self.units,
                                        &self.commanders,
                                        &mut self.events,
                                        Some(&self.buckets),
                                        t_px,
                                        a_team,
                                        fire_dt,
                                        &primary_w,
                                        a_pos,
                                        !spec.indirect,
                                        &tag,
                                        self.debug_full_friendly_fire,
                                        Some(uid),
                                    );
                                    cas_total = rep.person_losses + rep.friendly_fire_losses;
                                    org_loss_val = rep.org_loss;
                                    if let Some(ref tid) = tgt_uid_opt {
                                        if let Some(&tcid) = self.commanders.str_to_id.get(tid) {
                                            if let Some(hq_u) = self.commanders.hq_piece_id[tcid.0] {
                                                carry_after = self.units.carry_loss[hq_u.0];
                                            }
                                        } else if let Some(&tuid) = self.units.str_to_id.get(tid) {
                                            carry_after = self.units.carry_loss[tuid.0];
                                        }
                                    }
                                    let tname = tgt_uid_opt.as_deref().map(|t| self.resolve_target_name(t));
                                    let actor_name = self.units.name[uid.0].clone();
                                    let msg = format!(
                                        "{} [{}] 射击 {} ({:.0}m, 散布{:.0}m, 命中{:.0}%, 开火{}发) -> 伤亡 {}人 (累积{:.2}), 组织度 -{:.1}",
                                        actor_name, primary_w, tname.as_deref().unwrap_or("目标格"),
                                        dist_m, disp_m, hit_prob * 100.0, shots, cas_total, carry_after, org_loss_val
                                    );
                                    self.push_combat_log(DirectCombatLogEntry {
                                        turn: self.turn, clock_hours: self.clock_hours,
                                        actor_id: sid.clone(), actor_name,
                                        target_id: tgt_uid_opt.clone(), target_name: tname,
                                        kind: "fire".into(), weapon_model: primary_w.clone(),
                                        dist_m, dispersion_m: disp_m, hit_prob,
                                        casualties: cas_total, org_loss: org_loss_val,
                                        carry_loss_after: carry_after, ammo_used_t: ammo_used,
                                        message: msg,
                                    });
                                    format!("开火{}发 杀伤{}人(累积{:.2}) 组织-{:.1}", shots, cas_total, carry_after, org_loss_val)
                                } else {
                                    "火力准备中".to_string()
                                }
                            } else {
                                "无开火人员".to_string()
                            }
                        };

                        if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.target_pos = Some(tp);
                            plan.last_los_blocked = los.blocked;
                            plan.out_of_ammo = is_out_of_ammo;
                            plan.last_fire_summary = if is_out_of_ammo { "弹药耗尽".to_string() } else { summary };
                            plan.weapon_model = primary_w;
                            plan.dispersion_m = disp_m;
                            plan.hit_prob = hit_prob;
                            plan.last_casualties = cas_total;
                            plan.last_org_loss = org_loss_val;
                            plan.last_ammo_used_t = ammo_used;
                        }
                    }

                    self.units.autonomy_state[uid.0] = AtomicAction::Attack;

                    let should_remove = if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };

                    if new_dur == 0 {
                        if should_remove {
                            self.direct_unit_plans.remove(&sid);
                            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                            break;
                        }
                        self.rebuild_plan_waypoints(uid);
                        continue;
                    } else {
                        break;
                    }
                }

                // Movement actions (move / retreat / assault)
                if locks.intersects(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE) {
                    self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                    break;
                }

                if seg_action == "retreat" || seg_mm == "fast" {
                    self.units.exposed[uid.0] = true;
                } else if matches!(seg_mm.as_str(), "slow" | "stealth") {
                    self.units.exposed[uid.0] = false;
                }

                let cur = self.units.pos[uid.0];
                let mut follow_goal = seg_goal;
                if seg_follow {
                    if let Some(ref tid) = seg_tgt_uid {
                        if let Some(cand) = self.resolve_target_pos_f32(tid) {
                            let cand_px = (
                                (cand[0].round() as i32).clamp(0, rows - 1),
                                (cand[1].round() as i32).clamp(0, cols - 1),
                            );
                            let los = cast_los_ray_dda(&self.grid, self.units.px[uid.0], cand_px, self.night);
                            let d_km = self.grid.distance_km(cur, cand);
                            if !los.blocked && d_km <= 18.0 {
                                follow_goal = cand;
                                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                                    plan.target_pos = Some(cand);
                                }
                            }
                        }
                    }
                }

                let has_wps = self.direct_unit_plans.get(&sid).map_or(false, |p| !p.waypoints.is_empty());
                if !has_wps {
                    let path = plan_grid_path_f32(&self.grid, cur, follow_goal, self.units.mobility[uid.0]);
                    if path.is_empty() {
                        let is_persistent_follow = seg_follow && seg_tgt_uid.is_some() && self.direct_unit_plans.get(&sid).map_or(0, |p| p.segments.len()) <= 1;
                        if is_persistent_follow {
                            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                            break;
                        }
                        let should_remove = if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            true
                        };
                        if should_remove {
                            self.direct_unit_plans.remove(&sid);
                            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                            break;
                        }
                        self.rebuild_plan_waypoints(uid);
                        continue;
                    } else {
                        if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.waypoints = path;
                        }
                    }
                }

                let mut wps_tmp = self.direct_unit_plans.get_mut(&sid).map(|p| std::mem::take(&mut p.waypoints)).unwrap_or_default();
                let (new_pos, moved, deg, rem_h) = self.advance_unit_along_waypoints_dt(
                    uid,
                    cur,
                    &mut wps_tmp,
                    &seg_action,
                    &seg_mm,
                    time_budget / 3600.0,
                );
                let Some(plan) = self.direct_unit_plans.get_mut(&sid) else { break; };
                plan.waypoints = wps_tmp;
                if moved {
                    self.units.pos[uid.0] = new_pos;
                    self.units.px[uid.0] = (
                        (new_pos[0].round() as i32).clamp(0, rows - 1),
                        (new_pos[1].round() as i32).clamp(0, cols - 1),
                    );
                    self.units.facing_deg[uid.0] = deg;
                    self.units.moved_last_turn[uid.0] = true;
                    self.units.autonomy_state[uid.0] = if seg_action == "retreat" { AtomicAction::Retreat } else { AtomicAction::Move };
                    moved_cnt += 1;
                }
                time_budget = rem_h * 3600.0;
                if plan.waypoints.is_empty() {
                    let is_persistent_follow = seg_follow && seg_tgt_uid.is_some() && plan.segments.len() <= 1;
                    if is_persistent_follow {
                        self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                        break;
                    }
                    plan.segments.remove(0);
                    if plan.segments.is_empty() {
                        self.direct_unit_plans.remove(&sid);
                        self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                        break;
                    }
                    self.rebuild_plan_waypoints(uid);
                    continue;
                } else {
                    break;
                }
            }
        }

        // Background cooldown decay for units not actively attacking in this tick
        for (k, cd) in self.weapon_cooldowns.iter_mut() {
            if !touched_cd_keys.contains(k) && *cd > 0.0 {
                *cd = (*cd - SUBTICK_SECONDS).max(0.0);
            }
        }

        // 2. Commanders with direct orders
        let cmd_keys: Vec<String> = self.direct_cmd_plans.keys().cloned().collect();
        for sid in cmd_keys {
            let Some(&cid) = self.commanders.str_to_id.get(&sid) else { continue; };
            let hq_alive = self.commanders.alive[cid.0]
                && self.commanders.hq_piece_id[cid.0].map_or(true, |u| self.units.alive[u.0] && self.units.persons(u) > 0);
            if !hq_alive {
                self.direct_cmd_plans.remove(&sid);
                continue;
            }
            let action = self.direct_cmd_plans.get(&sid).map(|p| p.action.clone()).unwrap_or_default();
            if action == "stop" {
                continue;
            }
            let cmdr_locks = self.commanders.hq_piece_id[cid.0]
                .map(|u| self.units.effective_locks(u, &self.events.events))
                .unwrap_or_default();

            let mut time_budget = SUBTICK_SECONDS;
            while time_budget > 0.05 {
                let Some(seg) = self.direct_cmd_plans.get(&sid).and_then(|p| p.segments.first().cloned()) else {
                    self.direct_cmd_plans.remove(&sid);
                    break;
                };

                let seg_action = seg.action;
                let seg_goal = seg.goal;
                let seg_dur = seg.duration_sec;
                let seg_tgt_uid = seg.target_unit_id;

                if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                    plan.action = seg_action.clone();
                    plan.target_pos = Some(seg_goal);
                    plan.target_unit_id = seg_tgt_uid.clone();
                    plan.duration_sec = seg_dur;
                }

                if seg_action == "stop" {
                    let time_spent = time_budget.min(seg_dur as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;
                    let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    if should_remove {
                        self.direct_cmd_plans.remove(&sid);
                        break;
                    }
                    if new_dur == 0 {
                        continue;
                    } else {
                        break;
                    }
                }

                if seg_action == "attack" {
                    if cmdr_locks.contains(LockFlags::FORBID_FIRE) {
                        if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                            plan.last_fire_summary = "开火受限(状态锁定)".to_string();
                        }
                        break;
                    }
                    let time_spent = time_budget.min(seg_dur.max(16) as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;

                    let tgt_uid_opt = seg_tgt_uid.clone();
                    let resolved = tgt_uid_opt.as_deref().and_then(|tid| self.resolve_target_pos_f32(tid));
                    let tgt_pos_opt = resolved.or(Some(seg_goal));
                    if let Some(tp) = tgt_pos_opt {
                        let a_pos = self.commander_pos_f32(cid);
                        let a_px = self.commanders.px[cid.0];
                        let t_px = (
                            (tp[0].round() as i32).clamp(0, rows - 1),
                            (tp[1].round() as i32).clamp(0, cols - 1),
                        );
                        let dist_m = self.grid.distance_km(a_pos, tp) * 1000.0;
                        let los = cast_los_ray_dda(&self.grid, a_px, t_px, self.night);
                        let (disp_m, hit_prob) = Self::compute_fire_ballistics("rifle", dist_m, los.blocked);
                        let mut cas_total = 0u32;
                        let mut org_loss_val = 0.0f32;
                        let mut ammo_used = 0.0f32;
                        let mut is_out_of_ammo = self.commanders.hq_piece_id[cid.0].map_or(false, |u| self.units.ammo_t[u.0] <= 0.0005);
                        let mut summary = if los.blocked { "视线受阻".to_string() } else if is_out_of_ammo { String::new() } else { "警戒瞄准".to_string() };
                        if let Some(hq_uid) = self.commanders.hq_piece_id[cid.0] {
                            let fire_ratio = self.units.fire_eligible_ratio(hq_uid);
                            if self.units.alive[hq_uid.0] && self.units.persons(hq_uid) > 0 && !los.blocked && !is_out_of_ammo && fire_ratio > 0.0 {
                                let pwr = self.units.unit_power(hq_uid, dist_m, true, None, 1.0);
                                let cqb_lethality = (280.0 / dist_m.max(15.0)).powf(1.4).clamp(1.5, 110.0) * 2.6;
                                let mut fire_dt = [0.0f32; 6];
                                let mut base_pts = 0.0f32;
                                for d in 0..6 {
                                    let step_p = pwr[d] * SUBTICK_HOURS * fire_ratio;
                                    base_pts += step_p;
                                    fire_dt[d] = step_p * cqb_lethality;
                                }
                                ammo_used = self.units.consume_soldier_ammo(hq_uid, base_pts * 0.002);
                                if self.units.ammo_t[hq_uid.0] <= 0.0005 { is_out_of_ammo = true; }
                                let rep = resolve_cell_strike_bucketed(
                                    &mut self.grid, &mut self.units, &self.commanders, &mut self.events,
                                    Some(&self.buckets),
                                    t_px, self.commanders.team[cid.0], fire_dt, "rifle", a_pos, true,
                                    &format!("directcmd{}|c{}", self.subtick, cid.0), self.debug_full_friendly_fire, Some(hq_uid),
                                );
                                cas_total = rep.person_losses + rep.friendly_fire_losses;
                                org_loss_val = rep.org_loss;
                                summary = format!("伤亡{}人 组织度-{:.1}", cas_total, org_loss_val);
                                let tname = tgt_uid_opt.as_deref().map(|t| self.resolve_target_name(t));
                                let actor_name = self.commanders.name[cid.0].clone();
                                let msg = format!(
                                    "{} [rifle] 射击 {} ({:.0}m, 散布{:.0}m) -> 伤亡 {}人, 组织度 -{:.1}",
                                    actor_name, tname.as_deref().unwrap_or("目标格"), dist_m, disp_m, cas_total, org_loss_val
                                );
                                self.push_combat_log(DirectCombatLogEntry {
                                    turn: self.turn, clock_hours: self.clock_hours,
                                    actor_id: sid.clone(), actor_name,
                                    target_id: tgt_uid_opt.clone(), target_name: tname,
                                    kind: "fire".into(), weapon_model: "rifle".into(),
                                    dist_m, dispersion_m: disp_m, hit_prob,
                                    casualties: cas_total, org_loss: org_loss_val,
                                    carry_loss_after: 0.0, ammo_used_t: ammo_used,
                                    message: msg,
                                });
                            }
                        }
                        if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                            plan.target_pos = Some(tp);
                            plan.last_los_blocked = los.blocked;
                            plan.out_of_ammo = is_out_of_ammo;
                            plan.last_fire_summary = if is_out_of_ammo { String::new() } else { summary };
                            plan.weapon_model = "rifle".into();
                            plan.dispersion_m = disp_m;
                            plan.hit_prob = hit_prob;
                            plan.last_casualties = cas_total;
                            plan.last_org_loss = org_loss_val;
                            plan.last_ammo_used_t = ammo_used;
                        }
                    }

                    let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    if should_remove {
                        self.direct_cmd_plans.remove(&sid);
                        break;
                    }
                    if new_dur == 0 {
                        continue;
                    } else {
                        break;
                    }
                }

                // Movement actions (move)
                if cmdr_locks.intersects(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE) {
                    break;
                }

                let cur = self.commander_pos_f32(cid);
                let has_wps = self.direct_cmd_plans.get(&sid).map_or(false, |p| !p.waypoints.is_empty());
                if !has_wps {
                    let path = plan_grid_path_f32(&self.grid, cur, seg_goal, MobilityClass::Wheeled);
                    if path.is_empty() {
                        let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            true
                        };
                        if should_remove {
                            self.direct_cmd_plans.remove(&sid);
                            break;
                        }
                        continue;
                    } else if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        plan.waypoints = path;
                    }
                }

                let mut wps_tmp = self.direct_cmd_plans.get_mut(&sid).map(|p| std::mem::take(&mut p.waypoints)).unwrap_or_default();
                let (new_pos, moved, _deg) = Self::advance_pos_along_waypoints(&self.grid, cur, &mut wps_tmp, 0.65, 0.85);
                if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                    plan.waypoints = wps_tmp;
                }
                if moved {
                    let new_px = (
                        (new_pos[0].round() as i32).clamp(0, rows - 1),
                        (new_pos[1].round() as i32).clamp(0, cols - 1),
                    );
                    self.direct_cmd_pos.insert(sid.clone(), new_pos);
                    self.commanders.px[cid.0] = new_px;
                    if let Some(hq_uid) = self.commanders.hq_piece_id[cid.0] {
                        self.units.pos[hq_uid.0] = new_pos;
                        self.units.px[hq_uid.0] = new_px;
                    }
                    moved_cnt += 1;
                }

                let is_wps_empty = self.direct_cmd_plans.get(&sid).map_or(true, |p| p.waypoints.is_empty());
                if is_wps_empty {
                    let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments.remove(0);
                        }
                        plan.segments.is_empty()
                    } else {
                        true
                    };
                    if should_remove {
                        self.direct_cmd_plans.remove(&sid);
                        break;
                    }
                    continue;
                } else {
                    break;
                }
            }
        }

        moved_cnt
    }

    pub fn state_fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.turn.to_le_bytes());
        h.update(self.subtick.to_le_bytes());
        h.update(self.grid.terrain_revision.to_le_bytes());
        h.update((self.units.id.len() as u64).to_le_bytes());
        for i in 0..self.units.id.len() {
            h.update((self.units.alive[i] as u8).to_le_bytes());
            h.update(self.units.px[i].0.to_le_bytes());
            h.update(self.units.px[i].1.to_le_bytes());
            h.update(((self.units.pos[i][0] * 1000.0).round() as i64).to_le_bytes());
            h.update(((self.units.pos[i][1] * 1000.0).round() as i64).to_le_bytes());
            h.update(self.units.enlisted[i].to_le_bytes());
            h.update(self.units.cadre[i].to_le_bytes());
            h.update(((self.units.org[i] * 100.0).round() as i32).to_le_bytes());
            h.update((self.units.autonomy_state[i] as u8).to_le_bytes());
            h.update(self.units.active_deploy_line_id[i].to_le_bytes());
        }
        for ci in 0..self.commanders.id.len() {
            h.update((self.commanders.alive[ci] as u8).to_le_bytes());
            h.update(self.commanders.px[ci].0.to_le_bytes());
            h.update(self.commanders.px[ci].1.to_le_bytes());
            h.update((self.commanders.rank[ci] as u8).to_le_bytes());
            h.update(((self.commanders.order_churn_score[ci] * 100.0).round() as i32).to_le_bytes());
        }
        let d = h.finalize();
        let mut s = String::with_capacity(64);
        for b in d {
            use std::fmt::Write;
            let _ = write!(&mut s, "{:02x}", b);
        }
        s
    }

    pub fn clock_hms_string(&self) -> String {
        let s = self.clock_seconds % 86400;
        let h = s / 3600;
        let m = (s % 3600) / 60;
        let sec = s % 60;
        format!("{:02}:{:02}:{:02}", h, m, sec)
    }

    #[inline]
    pub fn step_subtick_16s(&mut self, driver: &mut dyn CommanderDriver) -> SubtickReport {
        self.step_subtick_5m(driver)
    }

    pub fn step_subtick_5m(&mut self, driver: &mut dyn CommanderDriver) -> SubtickReport {
        let dt = SUBTICK_HOURS;
        self.subtick += 1;
        self.clock_seconds = self.epoch_seconds + ((self.subtick as u64) << 4);
        self.clock_hours = (self.clock_seconds as f64 / 3600.0) as f32;
        // Activate due AO zones scheduled for this turn
        for z in &self.zones {
            if z.active && z.activate_turn == self.turn {
                self.grid.write_ao_cells(z.zone_id, z.team, z.echelon as u8, &z.cells);
            }
        }
        self.units.delta_persons_5m.fill(0);
        self.units.delta_org_5m.fill(0.0);
        self.units.carry_loss.fill(0.0);

        // 1. Command Hierarchy: Deliver downlinks -> recursive drivers evaluate
        let delivered = self.downlink.tick_deliver(self.clock_hours, &mut self.grid, &mut self.commanders);
        let n_cmd = self.commanders.id.len();
        for ci in 0..n_cmd {
            let cid = CommanderId(ci);
            if let Some(sub_orders) = driver.evaluate_downlink(
                cid,
                self.clock_hours,
                self.subtick,
                &self.grid,
                &self.units,
                &self.commanders,
            ) {
                for (sub_cid, pts, normal, mask) in sub_orders {
                    if self.downlink.record_order_churn(&mut self.commanders, sub_cid, &pts, self.clock_hours) {
                        self.downlink.queue_downlink_to_connected(
                            cid,
                            sub_cid,
                            self.clock_hours,
                            crate::world_grid::FrontlineRole::Deploy,
                            mask,
                            pts,
                            normal,
                            &self.commanders,
                            &self.units,
                            &self.grid,
                        );
                    }
                }
            }
        }

        // 1.1 算子 C: Deygout 刃峰电台阻挡损耗与单单位 <=128B 稀疏二阶 ZOH 指令滤波
        self.ensure_aux_states();
        for ci in 0..self.commanders.id.len() {
            let cid = CommanderId(ci);
            let c_pos = self.commander_pos_f32(cid);
            for uid in 0..self.units.id.len() {
                if !self.units.alive[uid] || self.units.direct_commander_id[uid] != Some(cid) {
                    continue;
                }
                let u_pos = self.units.pos[uid];
                let dist_tot = self.grid.distance_km(c_pos, u_pos) * 1000.0;
                if dist_tot > 1.0 {
                    let mut profile = Vec::with_capacity(3);
                    profile.push((0.0, self.elevation_at_pos(c_pos)));
                    let mid_pos = [(c_pos[0] + u_pos[0]) * 0.5, (c_pos[1] + u_pos[1]) * 0.5];
                    profile.push((dist_tot * 0.5, self.elevation_at_pos(mid_pos)));
                    profile.push((dist_tot, self.elevation_at_pos(u_pos)));
                    let dominant_edge = DeygoutRadioOperator::find_dominant_knife_edge(&profile);
                    if let Some(edge_idx) = dominant_edge {
                        let nu = (profile[edge_idx].1 - profile[0].1).abs() / 10.0;
                        let t_diff = DeygoutRadioOperator::rational_diffraction_transmission(nu);
                        if t_diff < 0.5 {
                            self.units.control_radius_km[uid] = (self.units.control_radius_km[uid] * (0.5 + 0.5 * t_diff)).max(0.1);
                        }
                    }
                }

                let target_w = if self.units.autonomy_state[uid] == AtomicAction::Attack {
                    1.0f32
                } else if self.units.autonomy_state[uid] == AtomicAction::Hold {
                    0.5f32
                } else {
                    0.0f32
                };
                let filter = self.unit_zoh_filters.entry(uid as u32).or_default();
                filter.step_zoh(&[(1, target_w)], 1.0, SUBTICK_SECONDS);
                let rethink = filter.evaluate_schmidt_rethink(&[(1, target_w)], 1.0);
                if let Some(rethink_ticks) = rethink {
                    filter.dwell_ticks = filter.dwell_ticks.saturating_sub(rethink_ticks.min(filter.dwell_ticks.saturating_sub(1)));
                }
            }
        }

        // 1.2 算子 A: 确定性 Tier-B 多容量最小堆拍卖 (G_alloc 分配)
        let mut bids = Vec::new();
        let mut cell_capacities = FxHashMap::default();
        for uid in 0..self.units.id.len() {
            if !self.units.alive[uid] || self.units.persons(UnitId(uid)) == 0 {
                continue;
            }
            let target_px = if let Some(plan) = self.direct_unit_plans.get(&self.units.str_id[uid]) {
                if let Some(wp) = plan.waypoints.first() {
                    self.pos_to_px(*wp)
                } else {
                    self.units.px[uid]
                }
            } else {
                self.units.px[uid]
            };
            if self.grid.in_bounds(target_px.0, target_px.1) {
                let cell_idx = self.grid.idx(target_px.0, target_px.1);
                let center = [target_px.0 as f32 + 0.5, target_px.1 as f32 + 0.5];
                let dist_m = self.grid.distance_km(self.units.pos[uid], center) * 1000.0;
                let cost_fixed = (dist_m * 1000.0) as i64;
                bids.push(AuctionBid {
                    cost_fixed,
                    unit_id: uid as u32,
                    cell_idx,
                });
                cell_capacities.entry(cell_idx).or_insert(1);
            }
        }
        if !bids.is_empty() {
            let allocations = run_deterministic_multi_capacity_auction(bids, &cell_capacities);
            for (winner_uid, cell_idx) in allocations {
                let u_idx = winner_uid as usize;
                if u_idx < self.units.id.len() && self.units.alive[u_idx] {
                    if !self.direct_unit_plans.contains_key(&self.units.str_id[u_idx]) {
                        let ar = (cell_idx / self.grid.cols) as i32;
                        let ac = (cell_idx % self.grid.cols) as i32;
                        if self.grid.in_bounds(ar, ac) && self.units.px[u_idx] == (ar, ac) {
                            self.units.moved_last_turn[u_idx] = false;
                        }
                    }
                }
            }
        }

        // 2. Autonomous tactical step for units (Rayon parallel evaluation + deterministic apply)
        //    Then restore positions for units currently under direct player orders (avoids double-move).
        let saved_direct: FxHashMap<usize, ([f32; 2], (i32, i32), f32)> = self
            .direct_unit_plans
            .keys()
            .filter_map(|sid| self.units.str_to_id.get(sid).copied())
            .map(|u| {
                (
                    u.0,
                    (self.units.pos[u.0], self.units.px[u.0], self.units.facing_deg[u.0]),
                )
            })
            .collect();

        let auto_stats = step_world_autonomy_5m(
            &mut self.grid,
            &mut self.units,
            &self.commanders,
            &mut self.buckets,
            &self.events.events,
            &self.zones,
            dt,
            self.night,
        );

        // Restore direct-controlled units positions (autonomy state may still update, but movement is owned by direct orders)
        for (&u, &(pos, px, facing)) in &saved_direct {
            self.units.pos[u] = pos;
            self.units.px[u] = px;
            self.units.facing_deg[u] = facing;
        }

        // Sync HQ units positions with their commanders
        for ci in 0..self.commanders.id.len() {
            if let Some(hq_u) = self.commanders.hq_piece_id[ci] {
                let cpos = self.commander_pos_f32(CommanderId(ci));
                self.units.pos[hq_u.0] = cpos;
                self.units.px[hq_u.0] = self.commanders.px[ci];
            }
        }

        // 3. Direct orders (player-controlled) -> move along waypoints / advance timed actions
        let direct_moved = self.step_direct_orders_5m();

        // 3.1 算子 D: 工兵 16s 步长增广弧长边走边干施工实体与起爆开关
        self.ensure_aux_states();
        let (grid_rows_i, grid_cols_i) = (self.grid.rows as i32, self.grid.cols as i32);
        let pos_to_px_local = |pos: [f32; 2]| {
            ((pos[0].round() as i32).clamp(0, grid_rows_i - 1), (pos[1].round() as i32).clamp(0, grid_cols_i - 1))
        };

        let engineer_uids: Vec<u32> = self.engineer_integrators.keys().copied().collect();
        for uid in engineer_uids {
            let u_idx = uid as usize;
            if u_idx >= self.units.alive.len() || !self.units.alive[u_idx] {
                continue;
            }
            let supp = self.unit_suppression.get(u_idx).copied().unwrap_or(0.0);
            let persons = self.units.persons(UnitId(u_idx));
            let (cur_pos, completed_wps) = if let Some(integrator) = self.engineer_integrators.get_mut(&uid) {
                integrator.step_16s(3.5, persons, supp, 40.0);
                let completed: Vec<[f32; 2]> = integrator.completed_instances.drain(..)
                    .filter_map(|idx| integrator.waypoints.get(idx).copied())
                    .collect();
                (integrator.cur_pos, completed)
            } else {
                continue;
            };

            self.units.pos[u_idx] = cur_pos;
            let new_px = pos_to_px_local(cur_pos);
            if self.grid.in_bounds(new_px.0, new_px.1) {
                self.units.px[u_idx] = new_px;
            }
            for pt in completed_wps {
                let px = pos_to_px_local(pt);
                if self.grid.in_bounds(px.0, px.1) {
                    let gi = self.grid.idx(px.0, px.1);
                    self.grid.terrain_attrition_day[gi] = (self.grid.terrain_attrition_day[gi] * 0.5).max(0.0);
                }
            }
        }

        let mut exploded_points = Vec::new();
        for detonator in &mut self.detonator_switches {
            if detonator.wire_laid && detonator.charges_placed && !detonator.detonated {
                let op_pos = detonator.switch_pos;
                if let Ok(pts) = detonator.trigger_detonation(op_pos) {
                    exploded_points.extend(pts);
                }
            }
        }
        for pt in exploded_points {
            let px = self.pos_to_px(pt);
            if self.grid.in_bounds(px.0, px.1) {
                let gi = self.grid.idx(px.0, px.1);
                self.cell_h_blast[gi] += 50.0;
                self.cell_s_supp[gi] += 10.0;
            }
        }

        // 4. (reserved) Operational Art continuous movement

        // 5. Autonomous tactical combat resolution for units currently in ATTACK state
        let n_units = self.units.id.len();
        let mut combat_pairs: Vec<(UnitId, UnitId, String, f32)> = Vec::new();
        for uid in 0..n_units {
            if !self.units.alive[uid]
                || self.units.autonomy_state[uid] != AtomicAction::Attack
                || self.units.persons(UnitId(uid)) == 0
                || self.units.ammo_t[uid] <= 0.001
            {
                continue;
            }
            let pos = self.units.pos[uid];
            let px = self.units.px[uid];
            let team = self.units.team[uid];
            let mut best_target: Option<(UnitId, f32)> = None;
            self.buckets.query_neighbors_within_px(px.0, px.1, 15, |eid| {
                let j = eid.0;
                if !self.units.alive[j] || self.units.team[j] == team || self.units.persons(eid) == 0 {
                    return;
                }
                let d_m = self.grid.distance_km(pos, self.units.pos[j]) * 1000.0;
                if d_m <= 3000.0 && best_target.map_or(true, |(_, bd)| d_m < bd) {
                    best_target = Some((eid, d_m));
                }
            });
            if let Some((tid, d_m)) = best_target {
                let primary_w = self.units.equipment[uid]
                    .iter()
                    .max_by(|a, b| equipment_spec(a.0).fire[0].total_cmp(&equipment_spec(b.0).fire[0]))
                    .map(|(m, _)| m.clone())
                    .unwrap_or_else(|| "rifle".to_string());
                combat_pairs.push((UnitId(uid), tid, primary_w, d_m));
            }
        }
        for (aid, tid, w_model, dist_m) in combat_pairs {
            if !self.units.alive[aid.0] || !self.units.alive[tid.0] {
                continue;
            }
            let a_px = self.units.px[aid.0];
            let t_px = self.units.px[tid.0];
            let a_pos = self.units.pos[aid.0];
            let los = cast_los_ray_dda(&self.grid, a_px, t_px, self.night);
            let spec = equipment_spec(&w_model);
            let reg_b = resolve_registration_bonus(
                &self.grid,
                aid,
                &self.units,
                &self.commanders,
                t_px,
                spec.ballistic_group,
            );
            let attacker_posture = self.unit_postures.get(aid.0).copied().unwrap_or(SchmidtPostureState::Active);
            let zeta = attacker_posture.source_factor();
            let pwr = self.units.unit_power(aid, dist_m, !los.blocked, None, (1.0 + reg_b) * zeta);
            let mut fire_dt = [0.0f32; 6];
            let mut total_pts = 0.0f32;
            for d in 0..6 {
                fire_dt[d] = pwr[d] * dt;
                total_pts += fire_dt[d];
            }
            if total_pts > 1e-4 {
                self.units.ammo_t[aid.0] = (self.units.ammo_t[aid.0] - total_pts * 0.01).max(0.0);
                let tag = format!("subtick{}|u{}->u{}", self.subtick, aid.0, tid.0);
                let a_team = self.units.team[aid.0];
                resolve_cell_strike(
                    &mut self.grid,
                    &mut self.units,
                    &self.commanders,
                    &mut self.events,
                    t_px,
                    a_team,
                    fire_dt,
                    &w_model,
                    a_pos,
                    !spec.indirect,
                    &tag,
                );
                self.record_blast_and_suppression(t_px, total_pts, Some(tid));
                apply_registration_to_cells(
                    &mut self.grid,
                    aid,
                    &self.units,
                    &self.commanders,
                    t_px,
                    1.0,
                    &w_model,
                    dist_m,
                    5.0,
                    false,
                );
            }
        }

        // 6. Advance and complete ActiveEvents (including ReorganizeEchelon)
        let mut due_events = Vec::new();
        let mut keep_events = Vec::with_capacity(self.events.events.len());
        for mut ev in self.events.events.drain(..) {
            ev.remaining_hours -= dt;
            if ev.remaining_hours <= 1e-6 {
                due_events.push(ev);
            } else {
                keep_events.push(ev);
            }
        }
        self.events.events = keep_events;
        let completed_cnt = due_events.len();
        for ev in due_events {
            self.complete_event(ev);
        }

        // 7. 5m vehicle overload faults & War Councils & 1h Air subtick & Logistics conservation
        tick_overload_faults_5m(&mut self.units, dt, self.subtick);
        update_war_councils(&mut self.commanders, &mut self.units, &mut self.grid, dt);
        let air_sorties = if self.subtick % 12 == 0 {
            run_air_subtick_1h(&mut self.wings, &self.grid)
        } else {
            0
        };
        let logi_rep = dispatch_logistics_step(
            &self.grid,
            &mut self.depots,
            &mut self.units,
            &self.commanders,
            &self.zones,
            &self.fire_fields,
            dt,
            self.turn,
        );

        // 8. Attrition, org recovery & decay, environment sparse tick, modifier TTL decay, succession
        for uid in 0..self.units.id.len() {
            let u = UnitId(uid);
            if !self.units.alive[uid] || self.units.persons(u) == 0 {
                continue;
            }
            let (r, c) = self.units.px[uid];
            if self.grid.in_bounds(r, c) {
                let gi = self.grid.idx(r, c);
                let rate = self.grid.terrain_attrition_day[gi] * dt / 24.0;
                if rate > 0.0 && self.units.enlisted[uid] > 0 {
                    let cur = self.units.carry_attr[uid] + rate * self.units.enlisted[uid] as f32;
                    let lost = (cur.floor() as u32).min(self.units.enlisted[uid]);
                    self.units.carry_attr[uid] = cur - lost as f32;
                    self.units.enlisted[uid] -= lost;
                    self.units.delta_persons_5m[uid] -= lost as i32;
                }
            }
            let locks = self.units.effective_locks(u, &self.events.events);
            let mods = aggregate_modifiers(&self.units.modifier_slots[uid], 0.0, false);
            if !locks.contains(LockFlags::FORBID_REST)
                && self.units.autonomy_state[uid] == AtomicAction::Hold
                && self.units.delta_org_5m[uid] >= -1e-4
            {
                let rec = 4.0 * dt * self.units.recovery_mult[uid];
                if self.units.org[uid] < self.units.org_cap[uid] {
                    self.units.org[uid] = (self.units.org[uid] + rec).min(self.units.org_cap[uid]);
                }
            }
            if self.units.org[uid] > self.units.org_cap[uid] {
                let excess = self.units.org[uid] - self.units.org_cap[uid];
                self.units.org[uid] = self.units.org_cap[uid] + excess * (-dt / 6.0).exp();
            }
            if mods.org_delta_per_h.abs() > 1e-6 {
                self.units.org[uid] = (self.units.org[uid] + mods.org_delta_per_h * dt).clamp(0.0, 120.0);
            }
            // Expire unit modifier TTLs
            self.units.modifier_slots[uid].retain_mut(|slot| {
                if let Some(ttl) = &mut slot.ttl_hours {
                    *ttl -= dt;
                    *ttl > 1e-6
                } else {
                    true
                }
            });
            if self.units.persons(u) == 0 {
                self.units.alive[uid] = false;
            } else {
                self.units.retreat_recovery_hours[uid] = (self.units.retreat_recovery_hours[uid] - dt).max(0.0);
                if self.units.org[uid] <= ORG_ROUT_THRESHOLD && self.units.retreat_recovery_hours[uid] <= 0.0 {
                    let _ = self.try_set_direct_unit_order_ex(
                        u,
                        "retreat",
                        None,
                        false,
                        None,
                        "fast",
                        "fire_on_the_move",
                        false,
                    );
                }
            }
        }

        for ci in 0..self.commanders.id.len() {
            self.commanders.resolve_succession(CommanderId(ci), &self.units);
            self.commanders.sync_hq_ledger_authority(CommanderId(ci), &self.units);
        }

        // 算子 B-2: 双时间尺度衰减与单位施密特姿态回升
        self.ensure_aux_states();
        for h in &mut self.cell_h_blast {
            *h *= ALPHA_BLAST;
            if *h < 1e-4 {
                *h = 0.0;
            }
        }
        for s in &mut self.cell_s_supp {
            *s *= ALPHA_SUPP;
            if *s < 1e-4 {
                *s = 0.0;
            }
        }
        for uid in 0..self.units.id.len() {
            if uid < self.unit_suppression.len() {
                self.unit_suppression[uid] *= ALPHA_SUPP;
                self.unit_postures[uid] = self.unit_postures[uid].transition(self.unit_suppression[uid]);
            }
        }

        self.grid.tick_environment(dt);
        let fp = self.state_fingerprint();

        SubtickReport {
            subtick: self.subtick,
            clock_hours: self.clock_hours,
            moved_units: auto_stats.moved_count + direct_moved,
            attacking_units: auto_stats.attack_count,
            delivered_orders: delivered,
            completed_events: completed_cnt,
            air_sorties,
            logi_conservation_error: logi_rep.conservation_error,
            fingerprint: fp,
        }
    }

    pub fn execute_deploy_heavy(
        &mut self,
        u: UnitId,
        weapon: &str,
        keep_manned: bool,
        operator_soldier_ids: &[String],
        operator_count: u32,
        tag_id: u64,
    ) -> Result<UnitId, String> {
        if u.0 >= self.units.alive.len() || !self.units.alive[u.0] {
            return Err("actor_dead".into());
        }
        let has_w = self.units.equipment[u.0].get(weapon).copied().unwrap_or(0) > 0;
        if !has_w {
            return Err(format!("weapon_not_owned:{weapon}"));
        }
        if let Some(c) = self.units.equipment[u.0].get_mut(weapon) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                self.units.equipment[u.0].remove(weapon);
            }
        }
        if let Some(cf) = self.units.equipment_full[u.0].get_mut(weapon) {
            *cf = cf.saturating_sub(1);
        }
        let mut eq = FxHashMap::default();
        eq.insert(weapon.to_string(), 1);
        let emp_str = format!("{}-{}-{}", self.units.str_id[u.0], weapon, tag_id);
        let emp_name = format!("{}阵地", equipment_spec(weapon).name);
        let px = self.units.px[u.0];
        let pos = [self.units.pos[u.0][0] + 0.08, self.units.pos[u.0][1] + 0.08];
        let team = self.units.team[u.0];
        let ch = self.units.radio_channel[u.0];
        let cmdr_opt = self.units.direct_commander_id[u.0];

        let emp_id = if keep_manned && self.units.persons(u) > 1 {
            let max_take = self.units.persons(u).saturating_sub(1);
            let target_crew = if !operator_soldier_ids.is_empty() {
                (operator_soldier_ids.len() as u32).min(max_take)
            } else {
                operator_count.max(equipment_spec(weapon).crew).min(max_take)
            };
            let mut moved_sols = Vec::new();
            if !self.units.soldiers[u.0].is_empty() {
                let mut rem = target_crew;
                let mut kept = Vec::with_capacity(self.units.soldiers[u.0].len());
                for s in self.units.soldiers[u.0].drain(..) {
                    let wanted = if !operator_soldier_ids.is_empty() {
                        operator_soldier_ids.iter().any(|id| id == &s.id)
                    } else {
                        true
                    };
                    if rem > 0 && s.status != "dead" && !s.cannot_fire && wanted {
                        rem -= 1;
                        moved_sols.push(s);
                    } else {
                        kept.push(s);
                    }
                }
                if rem > 0 {
                    // Top up from remaining healthy soldiers if requested count > matched ids
                    let mut final_kept = Vec::with_capacity(kept.len());
                    for s in kept {
                        if rem > 0 && s.status != "dead" && !s.cannot_fire && final_kept.len() + rem as usize > 0 {
                            rem -= 1;
                            moved_sols.push(s);
                        } else {
                            final_kept.push(s);
                        }
                    }
                    kept = final_kept;
                }
                self.units.soldiers[u.0] = kept;
                self.units.sync_totals_from_soldiers(u);
            }
            let tc_nco = moved_sols.iter().filter(|s| s.is_cadre && s.role != "officer").count() as u32;
            let tc_off = moved_sols.iter().filter(|s| s.role == "officer").count() as u32;
            let te = moved_sols.len() as u32 - tc_nco - tc_off;
            let id = self.units.spawn_unit_4ledger(
                emp_str, emp_name, team, px, Echelon::Squad, "heavy_emplacement",
                te.max(if moved_sols.is_empty() { target_crew } else { 0 }),
                tc_nco, tc_off, eq, ch,
            );
            if !moved_sols.is_empty() {
                self.units.soldiers[id.0] = moved_sols;
                self.units.sync_totals_from_soldiers(id);
            }
            self.units.operated_by[id.0] = Some(u);
            id
        } else {
            // Detached unmanned heavy emplacement operated by mother squad
            let id = self.units.spawn_unit_4ledger(
                emp_str, emp_name, team, px, Echelon::Squad, "heavy_emplacement",
                0, 0, 0, eq, ch,
            );
            self.units.soldiers[id.0].clear();
            let ammo_share = (self.units.ammo_t[u.0] * 0.18).min(0.25).max(0.02).min(self.units.ammo_t[u.0]);
            self.units.ammo_t[id.0] = ammo_share;
            self.units.operated_by[id.0] = Some(u);
            self.units.assigned_operator_soldiers[id.0] = operator_soldier_ids.to_vec();
            self.units.assigned_operator_count[id.0] = operator_count
                .max(operator_soldier_ids.len() as u32)
                .max(equipment_spec(weapon).crew)
                .max(1);
            id
        };

        self.units.pos[emp_id.0] = pos;
        self.units.unit_state[emp_id.0] = UnitState::Deployed;
        self.units.deployed_weapon[emp_id.0] = Some(weapon.to_string());
        self.units.direct_commander_id[emp_id.0] = cmdr_opt;
        if let Some(cid) = cmdr_opt {
            if cid.0 < self.commanders.subordinate_unit_ids.len()
                && !self.commanders.subordinate_unit_ids[cid.0].contains(&emp_id)
            {
                self.commanders.subordinate_unit_ids[cid.0].push(emp_id);
            }
        }
        self.units.refresh_caps(u);
        self.units.refresh_caps(emp_id);
        Ok(emp_id)
    }

    pub fn execute_pack_heavy(&mut self, receiver: UnitId, target_uid: UnitId, attach: bool) -> Result<(), String> {
        let ti = target_uid.0;
        if ti >= self.units.alive.len() || !self.units.alive[ti] {
            return Err("target_emplacement_dead".into());
        }
        if receiver.0 >= self.units.alive.len() || !self.units.alive[receiver.0] {
            return Err("receiver_dead".into());
        }
        if self.units.team[receiver.0] != self.units.team[ti] {
            return Err("cross_team_pack_forbidden".into());
        }
        if attach || receiver == target_uid {
            self.units.operated_by[ti] = None;
            if attach {
                self.units.unit_state[ti] = UnitState::Carried;
                self.units.attached_to[ti] = Some(receiver);
            } else {
                self.units.unit_state[ti] = UnitState::Mobile;
            }
            self.units.refresh_caps(target_uid);
            return Ok(());
        }

        // Merge heavy weapon, stock, soldiers & weighted org back into receiver
        let ri = receiver.0;
        let p_dst = self.units.persons(receiver) as f32;
        let p_src = self.units.persons(target_uid) as f32;
        let w_org = if (p_dst + p_src) > 0.0 {
            (self.units.org[ri] * p_dst + self.units.org[ti] * p_src) / (p_dst + p_src)
        } else {
            self.units.org[ri]
        };

        let eq_src = std::mem::take(&mut self.units.equipment[ti]);
        for (m, cnt) in eq_src {
            if cnt > 0 {
                *self.units.equipment[ri].entry(m.clone()).or_insert(0) += cnt;
                *self.units.equipment_full[ri].entry(m).or_insert(0) += cnt;
            }
        }
        if let Some(dw) = self.units.deployed_weapon[ti].take() {
            if self.units.equipment[ri].get(&dw).copied().unwrap_or(0) == 0 {
                *self.units.equipment[ri].entry(dw.clone()).or_insert(0) += 1;
                *self.units.equipment_full[ri].entry(dw).or_insert(0) += 1;
            }
        }

        let df = self.units.food_t[ti];
        let da = self.units.ammo_t[ti];
        let dg = self.units.fuel_t[ti];
        self.units.food_t[ti] = 0.0;
        self.units.ammo_t[ti] = 0.0;
        self.units.fuel_t[ti] = 0.0;
        self.units.add_supply_reserving_soldiers(receiver, df, da, dg);

        if !self.units.soldiers[ti].is_empty() {
            let mut sols = std::mem::take(&mut self.units.soldiers[ti]);
            self.units.soldiers[ri].append(&mut sols);
            self.units.sync_totals_from_soldiers(receiver);
        } else {
            self.units.enlisted[ri] += self.units.enlisted[ti];
            self.units.nco[ri] += self.units.nco[ti];
            self.units.officer[ri] += self.units.officer[ti];
            self.units.cadre[ri] = self.units.nco[ri] + self.units.officer[ri];
        }

        self.units.enlisted[ti] = 0;
        self.units.nco[ti] = 0;
        self.units.officer[ti] = 0;
        self.units.cadre[ti] = 0;
        self.units.operated_by[ti] = None;
        self.units.alive[ti] = false;
        self.units.unit_state[ti] = UnitState::Mobile;
        for subs in &mut self.commanders.subordinate_unit_ids {
            subs.retain(|&id| id != target_uid);
        }
        self.units.refresh_caps(receiver);
        self.units.org[ri] = w_org.min(self.units.org_cap[ri]);
        Ok(())
    }

    fn complete_event(&mut self, ev: ActiveEvent) {
        match ev.kind {
            EventKind::ReorganizeEchelon {
                commander_id,
                commander_piece_id,
                target_echelon,
                source_unit_ids,
                target_specs,
                strict_cadre_allocation,
            } => {
                complete_reorganize_echelon(
                    &self.grid,
                    &mut self.units,
                    &mut self.commanders,
                    commander_id,
                    commander_piece_id,
                    target_echelon,
                    &source_unit_ids,
                    &target_specs,
                    strict_cadre_allocation,
                );
            }
            EventKind::DeployHeavyUnit {
                weapon,
                keep_manned,
                operator_soldier_ids,
                operator_count,
            } => {
                let _ = self.execute_deploy_heavy(
                    ev.actor_id,
                    &weapon,
                    keep_manned,
                    &operator_soldier_ids,
                    operator_count,
                    ev.id,
                );
            }
            EventKind::PackHeavyUnit { target_unit_id, attach } => {
                let _ = self.execute_pack_heavy(ev.actor_id, target_unit_id, attach);
            }
            EventKind::Demolition { target_px, target_type } => {
                self.grid.add_exposure(target_px.0, target_px.1, 12.0, 2.0);
                self.grid.cut_wire_at(target_px.0, target_px.1, 1.0, None);
                if target_type == "bridge" {
                    self.grid.destroy_bridge(target_px.0, target_px.1);
                } else if target_type == "terrain" {
                    self.grid.destroy_terrain(target_px.0, target_px.1);
                } else if target_type == "stash" {
                    let k = self.grid.idx(target_px.0, target_px.1) as u32;
                    self.grid.cell_stash.remove(&k);
                }
            }
            EventKind::Engineering { op, target_px } => {
                let i_px = self.grid.idx(
                    target_px.0.clamp(0, (self.grid.rows as i32) - 1),
                    target_px.1.clamp(0, (self.grid.cols as i32) - 1),
                );
                if op == "build_bridge" {
                    self.grid.repair_bridge(target_px.0, target_px.1);
                } else if op == "repair_terrain" || op == "repair_airfield" {
                    self.grid.repair_terrain(target_px.0, target_px.1);
                } else if op == "lay_mines" {
                    let mut entry = CellModEntry::neutral("mine:field");
                    entry.mine_density = 0.70;
                    entry.speed_mult = 0.45;
                    self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                    let mf = if self.units.team[ev.actor_id.0] == TeamId::Red {
                        FeatureFlags::MINEFIELD_RED
                    } else {
                        FeatureFlags::MINEFIELD_BLUE
                    };
                    self.grid.feature_flags[i_px] |= mf.bits();
                    self.grid.terrain_revision += 1;
                } else if op == "clear_mines" {
                    self.grid.remove_cell_modifier(target_px.0, target_px.1, "mine:field");
                    self.grid.feature_flags[i_px] &= !(FeatureFlags::MINEFIELD_RED | FeatureFlags::MINEFIELD_BLUE).bits();
                    self.grid.terrain_revision += 1;
                } else if op == "fortify" {
                    let mut entry = CellModEntry::neutral("fort:foxhole");
                    entry.fortification_bonus = 0.22;
                    entry.cover_bonus = 0.14;
                    self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                } else {
                    let mut entry = CellModEntry::neutral("fort:trench");
                    entry.fortification_bonus = 0.35;
                    entry.cover_bonus = 0.20;
                    self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                    self.grid.feature_flags[i_px] |= FeatureFlags::TRENCH.bits();
                    self.grid.terrain_revision += 1;
                }
            }
            EventKind::ResupplyPickup { food_t, ammo_t, fuel_t } => {
                let px = self.units.px[ev.actor_id.0];
                let rows = self.grid.rows as i32;
                let cols = self.grid.cols as i32;
                let mut picked_k = None;
                'outer: for dr in -1..=1 {
                    for dc in -1..=1 {
                        let rr = px.0 + dr;
                        let cc = px.1 + dc;
                        if rr >= 0 && rr < rows && cc >= 0 && cc < cols {
                            let k = self.grid.idx(rr, cc) as u32;
                            if self.grid.cell_stash.contains_key(&k) {
                                picked_k = Some(k);
                                break 'outer;
                            }
                        }
                    }
                }
                if let Some(k) = picked_k {
                    let mut remove_empty = false;
                    if let Some(st) = self.grid.cell_stash.get_mut(&k) {
                        let f_req = if food_t > 0.0 { food_t } else { st.food_t };
                        let a_req = if ammo_t > 0.0 { ammo_t } else { st.ammo_t };
                        let g_req = if fuel_t > 0.0 { fuel_t } else { st.fuel_t };
                        let df = f_req.min(st.food_t).min((self.units.cap_food_t[ev.actor_id.0] - self.units.food_t[ev.actor_id.0]).max(0.0));
                        let da = a_req.min(st.ammo_t).min((self.units.cap_ammo_t[ev.actor_id.0] - self.units.ammo_t[ev.actor_id.0]).max(0.0));
                        let dg = g_req.min(st.fuel_t).min((self.units.cap_fuel_t[ev.actor_id.0] - self.units.fuel_t[ev.actor_id.0]).max(0.0));
                        st.food_t -= df;
                        st.ammo_t -= da;
                        st.fuel_t -= dg;
                        self.units.add_supply_reserving_soldiers(ev.actor_id, df, da, dg);
                        if st.food_t <= 1e-4 && st.ammo_t <= 1e-4 && st.fuel_t <= 1e-4 && st.equipment.is_empty() {
                            remove_empty = true;
                        }
                    }
                    if remove_empty {
                        self.grid.cell_stash.remove(&k);
                    }
                }
            }
            EventKind::FormCommandSquad {
                preferred_name,
                dissolve_if_empty,
            } => {
                let u = ev.actor_id;
                let has_cadre = self.units.alive[u.0] && (self.units.nco[u.0] + self.units.officer[u.0]) > 0;
                if has_cadre || !dissolve_if_empty {
                    self.units.kind[u.0] = "hq_squad".to_string();
                    self.units.unit_mask[u.0] = classify_unit_mask(&self.units.kind[u.0], &self.units.equipment[u.0]);
                    if !preferred_name.trim().is_empty() {
                        self.units.name[u.0] = preferred_name;
                    }
                    let existing_cid = self
                        .commanders
                        .hq_piece_id
                        .iter()
                        .position(|&hp| hp == Some(u))
                        .map(CommanderId);
                    let has_officer = self.units.officer[u.0] > 0;
                    let target_rank = if has_officer { Rank::Lieutenant } else { Rank::Sergeant };
                    let cid = match existing_cid {
                        Some(ec) => {
                            self.commanders.alive[ec.0] = true;
                            self.commanders.rank[ec.0] = target_rank;
                            self.commanders.px[ec.0] = self.units.px[u.0];
                            ec
                        }
                        None => {
                            let new_sid = format!("cmd-hq-{}", self.units.str_id[u.0]);
                            let cname = format!("{} · 指挥部", self.units.name[u.0]);
                            let nc = self.commanders.spawn_commander(
                                new_sid,
                                cname,
                                self.units.team[u.0],
                                self.units.px[u.0],
                                Echelon::Platoon,
                                target_rank,
                                self.units.radio_channel[u.0],
                            );
                            self.commanders.hq_piece_id[nc.0] = Some(u);
                            if let Some(parent_cid) = self.units.direct_commander_id[u.0] {
                                self.commanders.link_subordinate_cmd(parent_cid, nc);
                            } else {
                                self.units.direct_commander_id[u.0] = Some(nc);
                            }
                            nc
                        }
                    };
                    self.units.refresh_caps(u);
                    self.commanders.sync_hq_ledger_authority(cid, &self.units);
                    self.push_combat_log(DirectCombatLogEntry {
                        turn: self.turn,
                        clock_hours: self.clock_hours,
                        actor_id: self.units.str_id[u.0].clone(),
                        actor_name: self.units.name[u.0].clone(),
                        target_id: None,
                        target_name: None,
                        kind: "order".into(),
                        weapon_model: "radio".into(),
                        dist_m: 0.0,
                        dispersion_m: 0.0,
                        hit_prob: 0.0,
                        casualties: 0,
                        org_loss: 0.0,
                        carry_loss_after: 0.0,
                        ammo_used_t: 0.0,
                        message: format!(
                            "【组建指挥班】完成（{}），正式赋予指挥班身份并解锁「任务」指挥权限。",
                            if has_officer { "军官正职" } else { "士官临时代理·排级上限" }
                        ),
                    });
                }
            }
            EventKind::LayWire { target_px } => {
                let a_px = self.units.px[ev.actor_id.0];
                let team = self.units.team[ev.actor_id.0];
                self.grid.lay_wire_line(a_px, target_px, team);
            }
            EventKind::CargoTransfer {
                target_unit_id,
                food_t,
                ammo_t,
                fuel_t,
                equipment,
            } => {
                let s_idx = ev.actor_id.0;
                let t_idx = target_unit_id.0;
                if s_idx < self.units.id.len() && t_idx < self.units.id.len() && self.units.alive[s_idx] && self.units.alive[t_idx] {
                    let df = food_t.max(0.0).min(self.units.food_t[s_idx]);
                    let da = ammo_t.max(0.0).min(self.units.ammo_t[s_idx]);
                    let dg = fuel_t.max(0.0).min(self.units.fuel_t[s_idx]);
                    self.units.food_t[s_idx] -= df;
                    self.units.food_t[t_idx] = (self.units.food_t[t_idx] + df).min(self.units.cap_food_t[t_idx].max(self.units.food_t[t_idx] + df));
                    self.units.ammo_t[s_idx] -= da;
                    self.units.ammo_t[t_idx] += da;
                    self.units.fuel_t[s_idx] -= dg;
                    self.units.fuel_t[t_idx] += dg;
                    for (m, cnt) in equipment {
                        let avail = self.units.equipment[s_idx].get(&m).copied().unwrap_or(0);
                        let mv = cnt.min(avail);
                        if mv > 0 {
                            if let Some(c) = self.units.equipment[s_idx].get_mut(&m) {
                                *c -= mv;
                            }
                            *self.units.equipment[t_idx].entry(m).or_insert(0) += mv;
                        }
                    }
                    self.units.refresh_caps(ev.actor_id);
                    self.units.refresh_caps(target_unit_id);
                }
            }
            EventKind::RadioDecoy { target_px, ttl_h } => {
                let mut entry = CellModEntry::neutral("decoy:radio");
                entry.decoy_signal = 1.0;
                entry.ttl_hours = Some(ttl_h.max(0.5));
                self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                self.grid.add_exposure(target_px.0, target_px.1, 8.0, 2.0);
            }
            EventKind::FireWeapon { weapon, target_px, radius_px } => {
                let aid = ev.actor_id;
                if self.units.alive[aid.0] {
                    let d_m = self.grid.distance_km(self.units.pos[aid.0], [target_px.0 as f32, target_px.1 as f32]) * 1000.0;
                    let los = cast_los_ray_dda(&self.grid, self.units.px[aid.0], target_px, self.night);
                    let spec = equipment_spec(&weapon);
                    let reg_b = resolve_registration_bonus(&self.grid, aid, &self.units, &self.commanders, target_px, spec.ballistic_group);
                    let pwr = self.units.unit_power(aid, d_m, !los.blocked, Some(&weapon), 1.0 + reg_b);
                    let mut fp = [0.0f32; 6];
                    for d in 0..6 {
                        fp[d] = pwr[d] * SUBTICK_HOURS;
                    }
                    let tag = format!("ev{}|u{}", ev.id, aid.0);
                    let a_team = self.units.team[aid.0];
                    let a_pos = self.units.pos[aid.0];
                    resolve_cell_strike(
                        &mut self.grid,
                        &mut self.units,
                        &self.commanders,
                        &mut self.events,
                        target_px,
                        a_team,
                        fp,
                        &weapon,
                        a_pos,
                        !spec.indirect,
                        &tag,
                    );
                    apply_registration_to_cells(&mut self.grid, aid, &self.units, &self.commanders, target_px, radius_px, &weapon, d_m, 5.0, false);
                }
            }
            EventKind::RegisterFire { weapon, target_px, radius_px } => {
                let aid = ev.actor_id;
                let d_m = self.grid.distance_km(self.units.pos[aid.0], [target_px.0 as f32, target_px.1 as f32]) * 1000.0;
                apply_registration_to_cells(&mut self.grid, aid, &self.units, &self.commanders, target_px, radius_px, &weapon, d_m, 15.0, true);
            }
            _ => {}
        }
    }

    pub fn run_turn(&mut self, hours: f32, driver: &mut dyn CommanderDriver) -> TurnReport {
        self.pre_game_editable = false;
        self.turn += 1;
        let n_ticks = (hours * (3600.0 / SUBTICK_SECONDS)).round().max(1.0) as u32;
        let mut last_fp = String::new();
        for _ in 0..n_ticks {
            let sr = self.step_subtick_5m(driver);
            last_fp = sr.fingerprint;
        }
        TurnReport {
            turn: self.turn,
            clock_hours: self.clock_hours,
            subtick_count: n_ticks,
            total_hours: n_ticks as f32 * SUBTICK_HOURS,
            last_fingerprint: last_fp,
        }
    }

    pub fn run_turn_default(&mut self, hours: f32) -> TurnReport {
        let mut drv = MathRecursiveDriver;
        self.run_turn(hours, &mut drv)
    }
}

pub fn find_passable_cell(grid: &WorldGrid, pref_r: i32, pref_c: i32) -> (i32, i32) {
    let (cr, cc) = grid.clamp_rc(pref_r, pref_c);
    let ok = |r: i32, c: i32| -> bool {
        grid.in_bounds(r, c) && matches!(grid.terrain[grid.idx(r, c)], PLAIN | FOREST)
    };
    if ok(cr, cc) {
        return (cr, cc);
    }
    for rad in 1..25 {
        let mut best: Option<(i32, i32, i32)> = None;
        for r in (cr - rad)..=(cr + rad) {
            for c in (cc - rad)..=(cc + rad) {
                if ok(r, c) {
                    let d2 = (r - cr).pow(2) + (c - cc).pow(2);
                    if best.map_or(true, |b| d2 < b.2) {
                        best = Some((r, c, d2));
                    }
                }
            }
        }
        if let Some((r, c, _)) = best {
            return (r, c);
        }
    }
    (cr, cc)
}

fn eq_map(items: &[(&str, u32)]) -> FxHashMap<String, u32> {
    let mut m = FxHashMap::default();
    for &(k, v) in items {
        m.insert(k.to_string(), v);
    }
    m
}

pub fn build_r16_platoon_world(folder: &Path, tile_xy: (u32, u32), flow_threshold: u16) -> Result<SimWorld, String> {
    let (tx, ty) = tile_xy;
    let r16_p = folder.join(format!("1100_x{tx}_y{ty}.r16"));
    let diff_p = folder.join(format!("diffuse_x0_y0_x{tx}_y{ty}.png"));
    let riv_p = folder.join(format!("river_x{tx}_y{ty}.png"));
    let mut grid = WorldGrid::from_r16_diffuse_river(
        &r16_p,
        if diff_p.exists() { Some(diff_p.as_path()) } else { None },
        if riv_p.exists() { Some(riv_p.as_path()) } else { None },
        flow_threshold,
    )?;

    let (ar, ac) = find_passable_cell(&grid, 696, 2700);
    let pos_sq1 = find_passable_cell(&grid, ar, ac - 1);
    let pos_sq2 = find_passable_cell(&grid, ar, ac + 1);
    let pos_sq3 = find_passable_cell(&grid, ar + 1, ac - 1);
    let pos_stash = find_passable_cell(&grid, ar - 1, ac - 2);
    let pos_blue = find_passable_cell(&grid, ar - 1, ac + 4);

    grid.stash_put_resource(pos_stash.0, pos_stash.1, Some(TeamId::Red), 1.8, 2.5, 0.2, 0.0, Some(radio_channel_idx("A")), 1);
    grid.stash_put_equipment(pos_stash.0, pos_stash.1, Some(TeamId::Red), Some(radio_channel_idx("A")), &[("radio".to_string(), 1)]);

    let mut w = SimWorld::new(grid);
    w.role_echelon = Echelon::Platoon;
    w.r16_source = Some(r16_p.to_string_lossy().to_string());

    let u1 = w.units.spawn_unit("red-sq-1", "红1排1班(步兵)", TeamId::Red, pos_sq1, Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u2 = w.units.spawn_unit("red-sq-2", "红1排2班(步兵)", TeamId::Red, pos_sq2, Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u3 = w.units.spawn_unit("red-sq-3", "红1排3班(步兵)", TeamId::Red, pos_sq3, Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let umg = w.units.spawn_unit("red-sq-mg", "红1排机炮火力班", TeamId::Red, (ar, ac), Echelon::Squad, "mg_squad", 7, 1, eq_map(&[("hmg", 1), ("mortar_81", 1), ("rifle", 4), ("radio", 1)]), 0);
    let _ub = w.units.spawn_unit("blue-recon-1", "蓝方前出警戒班", TeamId::Blue, pos_blue, Echelon::Squad, "recon_squad", 6, 1, eq_map(&[("rifle", 5), ("radio", 1)]), 1);

    let cmd = w.commanders.spawn_commander("cmd-pl-1", "红1排排长", TeamId::Red, (ar, ac), Echelon::Platoon, Rank::Lieutenant, 0);
    for u in [u1, u2, u3, umg] {
        w.commanders.link_subordinate_unit(cmd, u, &mut w.units);
    }

    w.depots.push(Depot {
        str_id: "depot-red-1".into(),
        team: TeamId::Red,
        px: pos_stash,
        food_t: 15.0,
        ammo_t: 25.0,
        fuel_t: 10.0,
        replacements: 20.0,
    });

    Ok(w)
}

pub fn build_demo_world() -> SimWorld {
    let grid = WorldGrid::synthetic_grid(64, 64, 200.0);
    let mut w = SimWorld::new(grid);
    let cmd_co = w.commanders.spawn_commander("cmd-co-1", "红1连连长", TeamId::Red, (24, 20), Echelon::Company, Rank::Captain, 0);
    let cmd_pl = w.commanders.spawn_commander("cmd-pl-1", "红1排排长", TeamId::Red, (24, 24), Echelon::Platoon, Rank::Lieutenant, 0);
    w.commanders.link_subordinate_cmd(cmd_co, cmd_pl);

    let u1 = w.units.spawn_unit("red-sq-1", "红1排1班", TeamId::Red, (23, 25), Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u2 = w.units.spawn_unit("red-sq-2", "红1排2班", TeamId::Red, (25, 25), Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u_tk = w.units.spawn_unit("red-tk-1", "红坦支援组", TeamId::Red, (24, 24), Echelon::Squad, "tank_squad", 4, 1, eq_map(&[("tank_medium", 1), ("radio", 1)]), 0);
    let _ub = w.units.spawn_unit("blue-sq-1", "蓝方警戒班", TeamId::Blue, (24, 30), Echelon::Squad, "rifle_squad", 8, 1, eq_map(&[("rifle", 8), ("radio", 1)]), 1);
    let _ub2 = w.units.spawn_unit("blue-tk-2", "蓝方掩护组", TeamId::Blue, (18, 50), Echelon::Squad, "tank_squad", 4, 1, eq_map(&[("tank_medium", 1)]), 1);

    for u in [u1, u2, u_tk] {
        w.commanders.link_subordinate_unit(cmd_pl, u, &mut w.units);
    }
    w.depots.push(Depot {
        str_id: "depot-red-1".into(),
        team: TeamId::Red,
        px: (24, 18),
        food_t: 20.0,
        ammo_t: 30.0,
        fuel_t: 20.0,
        replacements: 30.0,
    });
    w
}

/// 16x16 Art Test World: 8m pixel container PLAIN base with MOUNTAIN, WATER & scattered FOREST tree-line placements.
pub fn build_art_16_world() -> SimWorld {
    let mut grid = WorldGrid::new_plain(16, 16, 8.0);
    // 1. Mountain showcase (deep cold iron black #07080c):
    let mtn_cells = [(0, 0), (0, 1), (1, 0), (1, 1)];
    for (r, c) in mtn_cells {
        let gi = grid.idx(r, c);
        grid.terrain[gi] = MOUNTAIN;
        grid.base_terrain[gi] = MOUNTAIN;
        grid.elevation_m[gi] = 320.0;
        grid.raw_u16[gi] = 320;
    }
    // 2. Water showcase (deep navy #163a6c):
    let water_cells = [(14, 14), (14, 15), (15, 14), (15, 15)];
    for (r, c) in water_cells {
        let gi = grid.idx(r, c);
        grid.terrain[gi] = WATER;
        grid.base_terrain[gi] = WATER;
    }
    // 3. Continuous forest density samples (0.0..=1.0): sparse -> medium -> dense
    let tree_cells: [(i32, i32, f32); 16] = [
        (2, 3, 0.95), (2, 4, 0.85), (3, 3, 0.78), (3, 4, 0.65),
        (4, 11, 0.30), (4, 12, 0.25), (5, 11, 0.45), (6, 6, 0.55),
        (8, 2, 0.90), (9, 2, 1.00), (9, 3, 0.72), (10, 9, 0.35),
        (10, 10, 0.50), (11, 9, 0.28), (12, 13, 0.60), (13, 4, 0.40),
    ];
    for (r, c, d) in tree_cells {
        grid.set_forest_density(r, c, d);
    }
    grid.sync_feature_flags_from_terrain();
    grid.refresh_attrition();

    let mut w = SimWorld::new(grid);
    let cmd_pl = w.commanders.spawn_commander(
        "cmd-pl-1",
        "红1排排部",
        TeamId::Red,
        (8, 8),
        Echelon::Platoon,
        Rank::Lieutenant,
        0,
    );
    let hq_uid = w.units.spawn_unit_4ledger(
        "cmd-pl-1-hq",
        "红1排排部 · 指挥班",
        TeamId::Red,
        (8, 8),
        Echelon::Squad,
        "hq_squad",
        2,
        1,
        1,
        eq_map(&[("rifle", 2), ("radio", 1)]),
        0,
    );
    w.commanders.hq_piece_id[cmd_pl.0] = Some(hq_uid);
    w.units.direct_commander_id[hq_uid.0] = Some(cmd_pl);
    w.commanders.sync_hq_ledger_authority(cmd_pl, &w.units);
    let u1 = w.units.spawn_unit_4ledger(
        "red-sq-1",
        "红1排1班",
        TeamId::Red,
        (8, 6),
        Echelon::Squad,
        "rifle_squad",
        10,
        1,
        0,
        eq_map(&[("rifle", 9), ("lmg", 1), ("radio", 1)]),
        0,
    );
    w.commanders.link_subordinate_unit(cmd_pl, u1, &mut w.units);

    let u_arty = w.units.spawn_unit_4ledger(
        "red-arty-1",
        "红重炮1班 (203mm)",
        TeamId::Red,
        (7, 5),
        Echelon::Squad,
        "howitzer_squad",
        8,
        1,
        0,
        eq_map(&[("howitzer_203", 1), ("rifle", 7), ("radio", 1)]),
        0,
    );
    w.commanders.link_subordinate_unit(cmd_pl, u_arty, &mut w.units);

    let _ub = w.units.spawn_unit_4ledger(
        "blue-sq-1",
        "蓝方前哨班",
        TeamId::Blue,
        (8, 12),
        Echelon::Squad,
        "rifle_squad",
        8,
        1,
        0,
        eq_map(&[("rifle", 8), ("radio", 1)]),
        1,
    );
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_log::MathRecursiveDriver;
    use crate::fields::{SchmidtPostureState, ALPHA_BLAST, ALPHA_SUPP};
    use crate::geometry::tactical_instance::{AugmentedArcIntegrator, ConstructionFsmState, DetonatorSwitchEntity};

    #[test]
    fn test_v6_phase01_sim_loop_golden_operators_integration() {
        let mut sim = build_demo_world();
        let mut driver = MathRecursiveDriver::default();

        // 1. 验证主循环推进与电台 ZOH 稀疏滤波器接入
        assert!((ALPHA_BLAST - 0.1690).abs() < 1e-4, "ALPHA_BLAST 必须精确锁死 0.1690");
        assert!((ALPHA_SUPP - 0.6412).abs() < 1e-4, "ALPHA_SUPP 必须精确锁死 0.6412");

        sim.step_subtick_16s(&mut driver);
        assert!(!sim.unit_zoh_filters.is_empty(), "主循环必须自动维护单位 ZOH 滤波器");
        for (_, filter) in &sim.unit_zoh_filters {
            assert!(filter.entries.len() <= 16, "稀疏 ZOH 模态项不超过 16");
            assert!(std::mem::size_of_val(filter) <= 128, "单个单位 ZOH 滤波器内存占用严格 <= 128 字节");
            assert!(filter.dwell_ticks >= 1, "ZOH 滤波器随 16s 步长累加 dwell_ticks");
        }

        // 2. 验证受击格双时间尺度衰减与单位姿态坍塌机
        let target_px = (20, 20);
        let target_uid = UnitId(0);
        sim.record_blast_and_suppression(target_px, 10.0, Some(target_uid));
        let cidx = sim.grid.idx(target_px.0, target_px.1);
        let h_blast_init = sim.cell_h_blast[cidx];
        let s_supp_init = sim.cell_s_supp[cidx];
        assert!(h_blast_init >= 100.0, "受到重击后目标格 H_blast 骤升");
        assert!(s_supp_init >= 10.0, "受到重击后目标格 S_supp 骤升");
        assert_eq!(
            sim.unit_postures[target_uid.0],
            SchmidtPostureState::Prone,
            "高压制直接击穿施密特死区，单位进入 Prone 伏卧态"
        );
        assert_eq!(
            sim.unit_postures[target_uid.0].source_factor(),
            0.02,
            "Prone 态发射辐射极大坍塌至 2%"
        );

        // 推进 2 个 16s 步长 (32 秒后)
        sim.step_subtick_16s(&mut driver);
        sim.step_subtick_16s(&mut driver);

        let h_blast_32s = sim.cell_h_blast[cidx];
        let s_supp_32s = sim.cell_s_supp[cidx];
        // H_blast 经历快衰减 (ALPHA_BLAST^2)
        assert!(
            h_blast_32s < h_blast_init * 0.05,
            "32s 后快衰减爆炸危险已基本消除 (剩余 < 5%)"
        );
        // S_supp 经历慢恢复 (ALPHA_SUPP^2 ~ 0.41)
        assert!(
            s_supp_32s > s_supp_init * 0.35,
            "32s 后敌方依然处于显著压制恢复期，突击黄金窗口确立"
        );

        // 3. 验证工兵增广弧长 s_tilde 边走边干施工实体与半截保留
        let u_eng = 0u32;
        let waypoints = vec![[15.0, 15.0], [15.0, 18.0]];
        let mut integrator = AugmentedArcIntegrator::new(waypoints);
        integrator.cur_pos = [15.0, 15.0];
        sim.engineer_integrators.insert(u_eng, integrator);

        // 重压制下暂停测试
        sim.unit_suppression[u_eng as usize] = 2.0;
        sim.step_subtick_16s(&mut driver);
        assert_eq!(
            sim.engineer_integrators[&u_eng].fsm_state,
            ConstructionFsmState::PinnedPause,
            "严重压制下工兵进入 PinnedPause 暂停"
        );

        // 恢复作业并推进
        sim.unit_suppression[u_eng as usize] = 0.0;
        sim.unit_postures[u_eng as usize] = SchmidtPostureState::Active;
        sim.step_subtick_16s(&mut driver);
        assert!(
            sim.engineer_integrators[&u_eng].s_tilde > 0.0,
            "工兵恢复作业后增广弧长 s_tilde 推进大于 0"
        );

        // 4. 验证起爆开关实体门禁与触发
        let mut switch = DetonatorSwitchEntity::new(
            101,
            sim.units.team[0] as u8,
            [10.0, 60.0],
            vec![[10.0, 15.0]],
            vec![[10.0, 60.0], [10.0, 15.0]],
        );
        switch.charges_placed = true;
        switch.wire_laid = true;
        // 操作员在安全距离外 (距离 45m >= 40m)
        assert!(switch.can_detonate([10.0, 60.0]).is_ok(), "安全距离外允许起爆");
        sim.detonator_switches.push(switch);

        // 推进触发起爆
        sim.step_subtick_16s(&mut driver);
        assert!(sim.detonator_switches[0].detonated, "起爆开关成功引爆");
    }
}
