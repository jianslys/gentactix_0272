//! `spatial_autonomy` module: Linear O(N) CSR `SpatialBucketGrid`, zero-allocation 9x9 terrain scan,
//! Rayon parallel decision evaluation with deterministic UnitId-ordered apply,
//! dual-line provenance, heavy-weapon anti-jitter (+H_DEPLOYED), ambush stealth trade-off, and disconnection k_C drift.

use crate::combat_logistics::{bearing_deg, cast_los_ray_dda};
use crate::command_log::{validate_command_link, OperationalZone};
use crate::units_ecs::{
    aggregate_modifiers, equipment_spec, ActiveEvent, AlertMark, AtomicAction, CommanderSoA,
    LockFlags, UnitId, UnitSoA, UnitState,
};
use crate::world_grid::{
    base_cover, FrontlineRole, MobilityClass, WorldGrid, FOREST, HIGHWAY, MOUNTAIN, PLAIN, URBAN,
    WATER,
};
use rayon::prelude::*;

pub const KC_BASE: f32 = 1.20;
pub const DELTA_KC: f32 = 0.55;
pub const KT_BASE: f32 = 1.30;
pub const KF_BASE: f32 = 0.90;
pub const K_TERRAIN_0: f32 = 0.055;
pub const H_STICK: f32 = 0.18;
pub const H_DEPLOYED: f32 = 1.35;
pub const K_STEALTH_COMPROMISE: f32 = 1.65;
pub const THREAT_DECAY_KM: f32 = 0.60;
pub const D_MARCH_KM: f32 = 2.0;
pub const ORG_ROUT_THRESHOLD: f32 = 12.0;

pub const BASE_SPEED_KMH: [f32; 3] = [3.5, 12.0, 12.0]; // Foot, Wheeled, Tracked

/// Zero-churn CSR linked bucket grid rebuilt in O(N) every 5-minute subtick.
/// Strictly eliminates O(N^2) spatial scans and per-tick Vec heap allocations.
pub struct SpatialBucketGrid {
    pub bucket_shift: u32, // 2 => 4x4 px = 800m buckets
    pub b_rows: usize,
    pub b_cols: usize,
    pub cell_head: Vec<i32>,
    pub unit_next: Vec<i32>,
    pub occupied_buckets: Vec<u32>,
}

impl SpatialBucketGrid {
    pub fn new(rows: usize, cols: usize) -> Self {
        let bucket_shift = 2u32;
        let b_rows = ((rows + (1 << bucket_shift) - 1) >> bucket_shift).max(1);
        let b_cols = ((cols + (1 << bucket_shift) - 1) >> bucket_shift).max(1);
        Self {
            bucket_shift,
            b_rows,
            b_cols,
            cell_head: vec![-1; b_rows * b_cols],
            unit_next: Vec::new(),
            occupied_buckets: Vec::new(),
        }
    }

    pub fn rebuild(&mut self, units: &UnitSoA, rows: usize, cols: usize) {
        let needed_r = ((rows + (1 << self.bucket_shift) - 1) >> self.bucket_shift).max(1);
        let needed_c = ((cols + (1 << self.bucket_shift) - 1) >> self.bucket_shift).max(1);
        if needed_r != self.b_rows || needed_c != self.b_cols {
            self.b_rows = needed_r;
            self.b_cols = needed_c;
            self.cell_head = vec![-1; needed_r * needed_c];
            self.occupied_buckets.clear();
        } else {
            for &b in &self.occupied_buckets {
                self.cell_head[b as usize] = -1;
            }
            self.occupied_buckets.clear();
        }

        let n = units.id.len();
        self.unit_next.clear();
        self.unit_next.resize(n, -1);

        // Iterate reverse so linked list head order stays ascending by UnitId!
        for uid in (0..n).rev() {
            if !units.alive[uid] || units.persons(UnitId(uid)) == 0 {
                continue;
            }
            let (r, c) = units.px[uid];
            if r < 0 || c < 0 {
                continue;
            }
            let br = ((r as usize) >> self.bucket_shift).min(self.b_rows - 1);
            let bc = ((c as usize) >> self.bucket_shift).min(self.b_cols - 1);
            let b_idx = br * self.b_cols + bc;
            if self.cell_head[b_idx] == -1 {
                self.occupied_buckets.push(b_idx as u32);
            }
            self.unit_next[uid] = self.cell_head[b_idx];
            self.cell_head[b_idx] = uid as i32;
        }
    }

    #[inline]
    pub fn query_neighbors_within_px<F>(&self, r: i32, c: i32, radius_px: i32, mut f: F)
    where
        F: FnMut(UnitId),
    {
        let br_c = (r.max(0) as usize) >> self.bucket_shift;
        let bc_c = (c.max(0) as usize) >> self.bucket_shift;
        let b_rad = ((radius_px + (1 << self.bucket_shift) - 1) >> self.bucket_shift).max(1) as isize;

        let r0 = (br_c as isize - b_rad).max(0) as usize;
        let r1 = (br_c as isize + b_rad).min(self.b_rows as isize - 1) as usize;
        let c0 = (bc_c as isize - b_rad).max(0) as usize;
        let c1 = (bc_c as isize + b_rad).min(self.b_cols as isize - 1) as usize;

        for br in r0..=r1 {
            let row_off = br * self.b_cols;
            for bc in c0..=c1 {
                let mut cur = self.cell_head[row_off + bc];
                while cur >= 0 {
                    let uid = cur as usize;
                    f(UnitId(uid));
                    cur = self.unit_next[uid];
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AutonomyDecision {
    pub action: AtomicAction,
    pub dir: [f32; 2],
    pub new_pos: [f32; 2],
    pub new_px: (i32, i32),
    pub moved: bool,
    pub new_facing_deg: f32,
    pub new_tactical_face_vec: [f32; 2],
    pub active_deploy_line_id: u32,
    pub k_c: f32,
    pub was_disconnected: bool,
    pub alert: AlertMark,
    pub autonomy_deployed: bool,
    pub exposed: bool,
    pub triggered_auto_retreat: bool,
}

#[derive(Debug, Default, Clone)]
pub struct AutonomyStepStats {
    pub units_evaluated: usize,
    pub fast_skipped: usize,
    pub moved_count: usize,
    pub attack_count: usize,
    pub fire_strikes: Vec<(UnitId, UnitId, f32)>, // (attacker, target, dist_m)
}

pub fn evaluate_unit_5m(
    uid: UnitId,
    grid: &WorldGrid,
    units: &UnitSoA,
    commanders: &CommanderSoA,
    buckets: &SpatialBucketGrid,
    events: &[ActiveEvent],
    dt_hours: f32,
    night: bool,
) -> Option<AutonomyDecision> {
    let i = uid.0;
    if !units.alive[i] || units.persons(uid) == 0 || units.attached_to[i].is_some() {
        return None;
    }

    let pos = units.pos[i];
    let px = units.px[i];
    let team = units.team[i];
    let mob = units.mobility[i];
    let locks = units.effective_locks(uid, events);
    let mods = aggregate_modifiers(&units.modifier_slots[i], 0.0, false);

    // 1. Disconnection & k_C drift evaluation
    let (connected, cmd_mods_kc, shock_m_add) = match units.direct_commander_id[i] {
        Some(cid) => {
            let link = validate_command_link(cid, uid, commanders, units, grid);
            let cm = aggregate_modifiers(
                &commanders.modifier_slots[cid.0],
                commanders.order_churn_score[cid.0],
                commanders.is_war_council_sync_active(cid, grid),
            );
            (link.is_connected(), cm.kc_mult, cm.shock_m_add)
        }
        None => (true, 1.0, 0.0),
    };

    let (next_kc, next_alert, next_disc) = if !connected {
        let kc = (units.k_c[i] + DELTA_KC * dt_hours * 2.0).clamp(0.65, 2.5);
        let mark = if kc >= 1.5 { AlertMark::RedAutonomous } else { AlertMark::YellowRigid };
        (kc, mark, true)
    } else {
        (KC_BASE * cmd_mods_kc, AlertMark::None, false)
    };

    // 2. O(1) Bucket Neighbor Scan for Nearby Enemies & Friendlies (within D_MARCH_KM = 10px)
    let search_px = (D_MARCH_KM * 1000.0 / grid.cell_m).ceil() as i32;
    let mut threat_vec = [0.0f32; 2];
    let mut friend_vec = [0.0f32; 2];
    let mut nearest_enemy: Option<(UnitId, f32, [f32; 2])> = None;
    let mut enemy_count = 0u32;

    buckets.query_neighbors_within_px(px.0, px.1, search_px, |nid| {
        let j = nid.0;
        if j == i || !units.alive[j] || units.persons(nid) == 0 {
            return;
        }
        let npos = units.pos[j];
        let dr = npos[0] - pos[0];
        let dc = npos[1] - pos[1];
        let d_px = dr.hypot(dc);
        let d_km = d_px * grid.cell_m / 1000.0;
        if d_km > D_MARCH_KM {
            return;
        }
        if units.team[j] != team {
            enemy_count += 1;
            let w = (-d_km / THREAT_DECAY_KM).exp();
            if d_px > 1e-4 {
                threat_vec[0] += (dr / d_px) * w;
                threat_vec[1] += (dc / d_px) * w;
            }
            if nearest_enemy.map_or(true, |(_, bd, _)| d_km < bd) {
                nearest_enemy = Some((nid, d_km, npos));
            }
        } else if d_km < 0.45 && d_px > 1e-4 {
            // Friendly short-range anti-clumping repulsion
            let w = (0.45 - d_km) / 0.45;
            friend_vec[0] -= (dr / d_px) * w;
            friend_vec[1] -= (dc / d_px) * w;
        }
    });

    // 3. Fast-Path Idle Skip (critical for 50,000+ squads on 4096x4096):
    // If claimed on a DEPLOY_LINE, zero enemies within 2 km, no locks/events needing 9x9 step, and stationary
    let sample = grid.sample_ao_and_frontline_field(
        pos,
        team,
        units.unit_mask[i],
        units.active_deploy_line_id[i],
        units.direct_commander_id[i].map(|c| c.0),
    );
    let mut active_deploy_id = units.active_deploy_line_id[i];
    if sample.has_deploy_line && sample.deploy_line_id != 0 {
        active_deploy_id = sample.deploy_line_id;
    }

    let triggered_auto_retreat = units.org[i] <= ORG_ROUT_THRESHOLD && units.retreat_recovery_hours[i] <= 0.0;
    let is_routing = units.retreat_recovery_hours[i] > 0.0 || units.org[i] <= ORG_ROUT_THRESHOLD;

    if active_deploy_id != 0
        && !is_routing
        && !sample.has_move_or_attack
        && !sample.has_global_action
        && sample.in_own_ao
        && enemy_count == 0
        && !units.moved_last_turn[i]
        && units.reorganizing_event_id[i].is_none()
        && friend_vec[0].abs() < 0.1
        && friend_vec[1].abs() < 0.1
    {
        return None; // Signals fast-path skipped; keeps exact Hold state & zero work!
    }

    // 4. Composite Movement Vector: k_C * C_order + k_T * T_threat + k_F * F_friend + G_action
    let mut cmd_vec = [0.0f32; 2];
    if sample.has_move_or_attack {
        cmd_vec = sample.move_or_attack_vec;
    } else if sample.has_matching_line {
        cmd_vec = sample.fl_pull;
    }
    if !sample.in_own_ao {
        cmd_vec[0] += sample.ao_pull[0] * 1.4;
        cmd_vec[1] += sample.ao_pull[1] * 1.4;
    }
    if sample.has_global_action {
        cmd_vec[0] += sample.global_action_vec[0] * 0.35;
        cmd_vec[1] += sample.global_action_vec[1] * 0.35;
    }

    // 5. Stack-only 9x9 Terrain Micro-Adjustment Scan (zero heap alloc!)
    let mut terrain_pull = [0.0f32; 2];
    let cur_score = cell_tactical_score(grid, px.0, px.1, units.unit_mask[i], mob);
    let mut best_cell_score = cur_score;
    for dr in -4..=4 {
        for dc in -4..=4 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = px.0 + dr;
            let nc = px.1 + dc;
            if !grid.in_bounds(nr, nc) || grid.terrain[grid.idx(nr, nc)] == WATER {
                continue;
            }
            let sc = cell_tactical_score(grid, nr, nc, units.unit_mask[i], mob);
            let d = (dr as f32).hypot(dc as f32);
            let adj = sc - d * 0.04;
            if adj > best_cell_score + 0.08 {
                best_cell_score = adj;
                terrain_pull = [dr as f32 / d, dc as f32 / d];
            }
        }
    }

    // “威胁归指挥官判断”：正常受命状态（未断连，或黄!未达红!）禁止威胁合矢量扭曲移动方向；
    // 仅在断连挂红!（RedAutonomous）或溃退状态（is_routing）下允许自主绕避威胁。
    let allow_threat_avoidance = next_alert == AlertMark::RedAutonomous || is_routing;
    let k_t_eff = if allow_threat_avoidance { KT_BASE + shock_m_add } else { 0.0 };
    let total_vec = [
        next_kc * cmd_vec[0] - k_t_eff * 0.25 * threat_vec[0] + KF_BASE * friend_vec[0] + K_TERRAIN_0 * terrain_pull[0],
        next_kc * cmd_vec[1] - k_t_eff * 0.25 * threat_vec[1] + KF_BASE * friend_vec[1] + K_TERRAIN_0 * terrain_pull[1],
    ];
    let vec_mag = total_vec[0].hypot(total_vec[1]);
    let threat_mag = threat_vec[0].hypot(threat_vec[1]);

    // 6. Utility Evaluation: HOLD vs MOVE vs ATTACK (including H_DEPLOYED & ambush stealth trade-off)
    let heavy_deployed = units.deployed_weapon[i].is_some()
        && (units.unit_state[i] == UnitState::Deployed || units.autonomy_deployed[i]);
    let hold_hyst = if units.autonomy_state[i] == AtomicAction::Hold { H_STICK } else { 0.0 }
        + if heavy_deployed && !sample.has_move_or_attack { H_DEPLOYED } else { 0.0 };
    let move_hyst = if units.autonomy_state[i] == AtomicAction::Move { H_STICK } else { 0.0 };
    let atk_hyst = if units.autonomy_state[i] == AtomicAction::Attack { H_STICK } else { 0.0 };

    let mut u_hold = 0.25 + hold_hyst;
    let mut u_move = if locks.intersects(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE) {
        -999.0
    } else if heavy_deployed && !sample.has_move_or_attack {
        vec_mag * 0.5 + move_hyst - 0.8
    } else {
        vec_mag * 0.95 + move_hyst
    };
    if sample.has_move_or_attack && !locks.contains(LockFlags::FORBID_MOVE) {
        u_move += 0.85;
    }

    let mut u_attack = -999.0f32;
    let mut new_exposed = units.exposed[i];
    let mut face_target_pos = [pos[0] + sample.fl_normal[0], pos[1] + sample.fl_normal[1]];

    if let Some((_eid, e_km, e_pos)) = nearest_enemy {
        face_target_pos = e_pos;
        if !locks.contains(LockFlags::FORBID_FIRE) {
            let e_px = (e_pos[0].round() as i32, e_pos[1].round() as i32);
            let los = cast_los_ray_dda(grid, px, e_px, night);
            if !los.blocked && los.visibility > 0.1 {
                let primary_range = units.equipment[i]
                    .keys()
                    .map(|m| equipment_spec(m).range_m)
                    .fold(400.0f32, f32::max);
                let dist_m = e_km * 1000.0;
                let p_hit = crate::units_ecs::weapon_accuracy_decay(dist_m, primary_range) * los.visibility;
                let stealth_pen = if !units.exposed[i] {
                    K_STEALTH_COMPROMISE * units.alpha_stealth[i]
                } else {
                    0.0
                };
                u_attack = p_hit * 2.1 * (0.4 + 0.6 * units.org[i] / 100.0) + atk_hyst - stealth_pen;
                if matches!(sample.best_line_role, Some(FrontlineRole::Attack)) {
                    u_attack += 0.55;
                }
            }
        }
    }

    // Soft-pack heavy weapon if ordered along MOVE/ATTACK_LINE
    if heavy_deployed && sample.has_move_or_attack && u_move > u_hold {
        u_hold -= H_DEPLOYED;
    }

    let (chosen_action, move_dir) = if is_routing {
        let rx = -threat_vec[0] + 0.3 * friend_vec[0];
        let ry = -threat_vec[1] + 0.3 * friend_vec[1];
        let rm = rx.hypot(ry);
        let dir = if rm > 1e-4 {
            [rx / rm, ry / rm]
        } else {
            match team {
                crate::world_grid::TeamId::Red => [0.0, -1.0],
                crate::world_grid::TeamId::Blue => [0.0, 1.0],
            }
        };
        (AtomicAction::Retreat, dir)
    } else if u_attack >= u_move && u_attack >= u_hold {
        new_exposed = true;
        (AtomicAction::Attack, [face_target_pos[0] - pos[0], face_target_pos[1] - pos[1]])
    } else if u_move > u_hold && vec_mag > 0.08 {
        (AtomicAction::Move, [total_vec[0] / vec_mag, total_vec[1] / vec_mag])
    } else {
        (AtomicAction::Hold, sample.fl_normal)
    };

    // 7. Continuous 5-minute physical advancement along `move_dir`
    let mut new_pos = pos;
    let mut new_px = px;
    let mut moved = false;
    let mut new_facing = units.facing_deg[i];
    let mut new_tact_vec = units.tactical_face_vec[i];

    let is_retreating = chosen_action == AtomicAction::Retreat;
    if (chosen_action == AtomicAction::Move && !locks.intersects(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE))
        || (is_retreating && !locks.contains(LockFlags::FORBID_MOVE))
    {
        let dir_r = if move_dir[0].abs() > 0.25 { move_dir[0].signum() as i32 } else { 0 };
        let dir_c = if move_dir[1].abs() > 0.25 { move_dir[1].signum() as i32 } else { 0 };
        let nr = (px.0 + dir_r).clamp(0, (grid.rows as i32) - 1);
        let nc = (px.1 + dir_c).clamp(0, (grid.cols as i32) - 1);
        let mut rem_h = dt_hours;
        let mut p_cur = pos;
        let mut px_cur = px;
        let act_s = if is_retreating { "retreat" } else { "move" };
        let dir_norm = move_dir[0].hypot(move_dir[1]).max(1e-5);
        let ux = move_dir[0] / dir_norm;
        let uy = move_dir[1] / dir_norm;
        while rem_h > 1e-6 {
            let eff_spd_kmh = units.effective_speed_kmh(
                uid,
                grid,
                px_cur.0,
                px_cur.1,
                nr,
                nc,
                act_s,
                "normal",
            ) * mods.speed_mult;
            if eff_spd_kmh <= 0.0 {
                break;
            }
            let px_per_h = eff_spd_kmh * 1000.0 / grid.cell_m;
            let step_px = (px_per_h * rem_h).min(0.90);
            if step_px <= 1e-4 {
                break;
            }
            let cand_pos = grid.clamp_pos([p_cur[0] + ux * step_px, p_cur[1] + uy * step_px]);
            let cand_px = (cand_pos[0].round() as i32, cand_pos[1].round() as i32);
            let step_cost = if cand_px == px_cur {
                0.0
            } else {
                grid.move_cost_step(px_cur.0, px_cur.1, cand_px.0, cand_px.1, mob, 1.0)
            };
            if !step_cost.is_finite() {
                break;
            }
            p_cur = cand_pos;
            px_cur = cand_px;
            rem_h -= step_px / px_per_h;
            moved = (p_cur[0] - pos[0]).hypot(p_cur[1] - pos[1]) > 1e-4;
            if dir_norm <= step_px {
                break;
            }
        }
        if moved {
            new_pos = p_cur;
            new_px = px_cur;
            if is_retreating {
                new_facing = bearing_deg(pos, new_pos);
                new_tact_vec = move_dir;
            } else if units.is_directional_hull(uid) {
                new_facing = bearing_deg(pos, new_pos);
            } else if threat_mag > 0.05 {
                let t_dir = [threat_vec[0] / threat_mag, threat_vec[1] / threat_mag];
                new_tact_vec = t_dir;
                new_facing = bearing_deg(pos, [pos[0] + t_dir[0], pos[1] + t_dir[1]]);
            } else {
                new_tact_vec = move_dir;
            }
        }
    } else if threat_mag > 0.05 {
        let t_dir = [threat_vec[0] / threat_mag, threat_vec[1] / threat_mag];
        new_tact_vec = t_dir;
        new_facing = bearing_deg(pos, [pos[0] + t_dir[0], pos[1] + t_dir[1]]);
    } else if !units.is_directional_hull(uid) {
        let len = move_dir[0].hypot(move_dir[1]);
        if len > 1e-4 {
            new_tact_vec = [move_dir[0] / len, move_dir[1] / len];
        }
    }

    let next_auto_deployed = if units.deployed_weapon[i].is_some() {
        !matches!(chosen_action, AtomicAction::Move | AtomicAction::Retreat)
            && (units.autonomy_deployed[i] || active_deploy_id != 0)
    } else {
        false
    };

    Some(AutonomyDecision {
        action: chosen_action,
        dir: move_dir,
        new_pos,
        new_px,
        moved,
        new_facing_deg: new_facing,
        new_tactical_face_vec: new_tact_vec,
        active_deploy_line_id: active_deploy_id,
        k_c: next_kc,
        was_disconnected: next_disc,
        alert: next_alert,
        autonomy_deployed: next_auto_deployed,
        exposed: new_exposed,
        triggered_auto_retreat,
    })
}

#[inline]
fn cell_tactical_score(grid: &WorldGrid, r: i32, c: i32, unit_mask: u8, mob: MobilityClass) -> f32 {
    let i = grid.idx(r, c);
    let t = grid.terrain[i];
    if t == WATER {
        return -10.0;
    }
    let cov = match t {
        FOREST => 0.35,
        MOUNTAIN => 0.40,
        URBAN => 0.50,
        _ => 0.0,
    };
    let elev_bonus = (grid.elevation_m[i] - grid.elev_min_m) / (grid.elev_max_m - grid.elev_min_m).max(1.0) * 0.25;
    let road_bonus = if t == HIGHWAY && mob != MobilityClass::Foot { 0.30 } else { 0.0 };
    if (unit_mask & 0b0100) != 0 || t == MOUNTAIN {
        cov + elev_bonus
    } else if (unit_mask & 0b0010) != 0 && t == PLAIN {
        0.25 + road_bonus
    } else {
        let _ = (base_cover(URBAN), URBAN);
        cov + road_bonus + elev_bonus * 0.4
    }
}

/// Executes 1 deterministic 5-minute autonomy subtick over ALL units using Rayon + CSR buckets.
pub fn step_world_autonomy_5m(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    buckets: &mut SpatialBucketGrid,
    events: &[ActiveEvent],
    _zones: &[OperationalZone],
    dt_hours: f32,
    night: bool,
) -> AutonomyStepStats {
    buckets.rebuild(units, grid.rows, grid.cols);

    let n = units.id.len();
    let decisions: Vec<Option<AutonomyDecision>> = (0..n)
        .into_par_iter()
        .map(|uid| evaluate_unit_5m(UnitId(uid), grid, units, commanders, buckets, events, dt_hours, night))
        .collect();

    let mut stats = AutonomyStepStats::default();
    for (uid, dec_opt) in decisions.into_iter().enumerate() {
        if !units.alive[uid] || units.persons(UnitId(uid)) == 0 {
            continue;
        }
        stats.units_evaluated += 1;
        let Some(dec) = dec_opt else {
            stats.fast_skipped += 1;
            units.moved_last_turn[uid] = false;
            continue;
        };
        units.autonomy_state[uid] = dec.action;
        units.autonomy_dir[uid] = dec.dir;
        units.pos[uid] = dec.new_pos;
        units.px[uid] = dec.new_px;
        units.moved_last_turn[uid] = dec.moved;
        units.facing_deg[uid] = dec.new_facing_deg;
        units.tactical_face_vec[uid] = dec.new_tactical_face_vec;
        units.active_deploy_line_id[uid] = dec.active_deploy_line_id;
        units.k_c[uid] = dec.k_c;
        units.was_disconnected[uid] = dec.was_disconnected;
        units.exclamation_buff[uid] = dec.alert;
        units.autonomy_deployed[uid] = dec.autonomy_deployed;
        units.exposed[uid] = dec.exposed;
        if dec.triggered_auto_retreat {
            units.retreat_recovery_hours[uid] = 0.5;
        }
        if dec.moved {
            stats.moved_count += 1;
        }
        if dec.action == AtomicAction::Attack {
            stats.attack_count += 1;
        }
    }
    stats
}
