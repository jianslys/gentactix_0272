//! `combat_logistics` module: Unified integer DDA LOS raycaster + ballistic decay,
//! directional cover cos(theta_i), tank-infantry CQB screening, registration sharing,
//! 2D Gaussian HE friendly fire, artillery FireFields, 1h air subtick, recon/fog, and conserving logistics.

use crate::command_log::{compute_operational_ao_buff, validate_command_link, OperationalZone};
use crate::units_ecs::{
    equipment_spec, BallisticGroup, CommanderId, CommanderSoA, DamageType,
    Echelon, UnitId, UnitSoA, ARMOR_DURABILITY, CADRE_LOSS_ORG_PENALTY, ORG_DURABILITY,
    PERSON_DURABILITY, SOFT_DURABILITY,
};
use crate::world_grid::{
    CellModEntry, PriorityTarget, ShotPacket, TeamId, WorldGrid, AIRFIELD, FOG,
    ICE, NIGHT_BASE_VIS, SNOW, WATER, WEATHER_VIS,
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TRUNK_LOSS_PER_100KM: f32 = 0.01;
pub const LASTMILE_LOSS_PER_KM: f32 = 0.02;
pub const PORTER_LOAD_T: f32 = 0.025;
pub const PORTER_SPEED_KMH: f32 = 4.0;
pub const HAZARD_PORTER_CASUALTY: f32 = 0.25;
pub const HAZARD_LOSS: f32 = 0.8;
pub const UNOBSERVED_FACTOR: f32 = 0.35;
pub const BASE_OBSERVE_KM: f32 = 1.5;
pub const RECON_BONUS_KM: f32 = 2.0;
pub const DF_RANGE_KM: f32 = 30.0;
pub const SKY_EYE_EXPOSURE_THRESHOLD: f32 = 20.0;

#[derive(Debug, Clone, Copy)]
pub struct LosResult {
    pub visibility: f32,
    pub blocked: bool,
    pub blocking_px: Option<(i32, i32)>,
}

/// Shared integer DDA / Bresenham LOS raycaster used by direct combat, recon,
/// artillery observation, and frontend Layer 3 weapon LOS mask.
pub fn cast_los_ray_dda(
    grid: &WorldGrid,
    a_px: (i32, i32),
    b_px: (i32, i32),
    night: bool,
) -> LosResult {
    let a = grid.clamp_rc(a_px.0, a_px.1);
    let b = grid.clamp_rc(b_px.0, b_px.1);
    if a == b {
        let i = grid.idx(a.0, a.1);
        let illum = grid.illum_ttl_h[i] > 0.0;
        let wv = WEATHER_VIS[(grid.weather[i] as usize).min(5)];
        let nv = if night && !illum { NIGHT_BASE_VIS } else { 1.0 };
        return LosResult {
            visibility: (wv * nv * grid.visibility_mod[i]).clamp(0.0, 1.0),
            blocked: false,
            blocking_px: None,
        };
    }

    let z_a = grid.elevation_m[grid.idx(a.0, a.1)] + 2.0;
    let z_b = grid.elevation_m[grid.idx(b.0, b.1)] + 2.0;
    let dr = b.0 - a.0;
    let dc = b.1 - a.1;
    let steps = dr.abs().max(dc.abs()).max(1);

    let mut min_vis = 1.0f32;
    let total_steps_f = steps as f32;

    for s in 0..=steps {
        let t = s as f32 / total_steps_f;
        let r = (a.0 as f32 + dr as f32 * t).round() as i32;
        let c = (a.1 as f32 + dc as f32 * t).round() as i32;
        if !grid.in_bounds(r, c) {
            return LosResult { visibility: 0.0, blocked: true, blocking_px: Some((r, c)) };
        }
        let i = grid.idx(r, c);
        if s > 0 && s < steps {
            let line_z = z_a + (z_b - z_a) * t;
            let cm = grid.get_cell_modifiers(r, c, crate::world_grid::MobilityClass::Foot);
            let terrain_top = grid.elevation_m[i] + cm.los_block_m;
            if terrain_top > line_z + 0.05 {
                return LosResult { visibility: 0.0, blocked: true, blocking_px: Some((r, c)) };
            }
        }
        let wv = WEATHER_VIS[(grid.weather[i] as usize).min(5)];
        let smoke = grid.visibility_mod[i];
        min_vis = min_vis.min(wv * smoke);
        if s > 0 && s < steps && min_vis < 0.05 {
            return LosResult { visibility: 0.0, blocked: true, blocking_px: Some((r, c)) };
        }
    }

    let bi = grid.idx(b.0, b.1);
    let illum = grid.illum_ttl_h[bi] > 0.0;
    let night_mult = if night && !illum { NIGHT_BASE_VIS } else { 1.0 };
    let vis = (min_vis * night_mult).clamp(0.0, 1.0);
    LosResult {
        visibility: vis,
        blocked: vis < 0.05,
        blocking_px: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireArcCell {
    pub r: i32,
    pub c: i32,
    pub eff: f32,
    pub dist_m: f32,
}

/// Computes Layer 3 weapon LOS + accuracy mask using the shared DDA kernel and ballistic decay.
pub fn compute_fire_arc(
    grid: &WorldGrid,
    origin_pos: [f32; 2],
    range_m: f32,
    min_range_m: f32,
    indirect: bool,
    night: bool,
    max_scan_m: f32,
) -> Vec<FireArcCell> {
    let or = origin_pos[0].round() as i32;
    let oc = origin_pos[1].round() as i32;
    let scan_cap = if indirect { 96.0 } else { 72.0 };
    let scan_px = (max_scan_m / grid.cell_m).ceil().min(scan_cap) as i32;
    let mut out = Vec::new();
    for dr in -scan_px..=scan_px {
        for dc in -scan_px..=scan_px {
            let rr = or + dr;
            let cc = oc + dc;
            if !grid.in_bounds(rr, cc) {
                continue;
            }
            let dist_m = (dr as f32).hypot(dc as f32) * grid.cell_m;
            if dist_m > max_scan_m || dist_m < min_range_m - 1e-3 {
                continue;
            }
            let vis = if indirect {
                1.0
            } else {
                cast_los_ray_dda(grid, (or, oc), (rr, cc), night).visibility
            };
            if vis < 0.05 {
                continue;
            }
            let acc = crate::units_ecs::weapon_accuracy_decay(dist_m, range_m);
            let eff = (vis * acc).clamp(0.0, 1.0);
            if eff > 0.01 {
                out.push(FireArcCell { r: rr, c: cc, eff, dist_m });
            }
        }
    }
    out
}

#[inline]
pub fn bearing_deg(from: [f32; 2], to: [f32; 2]) -> f32 {
    let dr = to[0] - from[0]; // +row is South
    let dc = to[1] - from[1]; // +col is East
    let rad = dc.atan2(-dr);
    (rad.to_degrees() + 360.0) % 360.0
}

/// Exact cos(theta_i) cover projection and flank/rear shock multiplier.
#[inline]
pub fn compute_directional_cover_eff(
    defender_pos: [f32; 2],
    defender_facing_deg: f32,
    attacker_pos: [f32; 2],
    base_cover: f32,
) -> (f32, f32, f32) {
    let dr = attacker_pos[0] - defender_pos[0];
    let dc = attacker_pos[1] - defender_pos[1];
    let len = dr.hypot(dc);
    if len <= 1e-6 {
        return (base_cover * 0.5, 1.0, 1.0);
    }
    let u_r = dr / len;
    let u_c = dc / len;
    let f_rad = defender_facing_deg.to_radians();
    // facing 0=North(-1,0), 90=East(0,1), 180=South(1,0), 270=West(0,-1)
    let f_r = -f_rad.cos();
    let f_c = f_rad.sin();
    let cos_theta = (f_r * u_r + f_c * u_c).clamp(-1.0, 1.0);
    let cover_eff = (base_cover * cos_theta.max(0.0)).clamp(0.0, 0.90);
    let flank_mult = if cos_theta >= 0.35 {
        1.0
    } else if cos_theta >= -0.35 {
        1.35
    } else {
        1.75
    };
    (cover_eff, flank_mult, cos_theta)
}

pub fn get_unit_ao_scopes(
    uid: UnitId,
    units: &UnitSoA,
    commanders: &CommanderSoA,
) -> (String, String) {
    let mut platoon_scope = units.str_id[uid.0].clone();
    let mut company_scope = platoon_scope.clone();
    if let Some(cid) = units.direct_commander_id[uid.0] {
        platoon_scope = commanders.str_id[cid.0].clone();
        if commanders.echelon[cid.0] == Echelon::Company {
            company_scope = commanders.str_id[cid.0].clone();
        } else if let Some(pc) = commanders.parent_commander_id[cid.0] {
            company_scope = commanders.str_id[pc.0].clone();
        } else {
            company_scope = platoon_scope.clone();
        }
    }
    (platoon_scope, company_scope)
}

pub fn ballistic_group_key(bg: BallisticGroup) -> &'static str {
    match bg {
        BallisticGroup::DirectSmallArms => "direct_small",
        BallisticGroup::DirectHeavyMg => "direct_hmg",
        BallisticGroup::DirectAntiTank => "direct_at",
        BallisticGroup::DirectCannon => "direct_cannon",
        BallisticGroup::IndirectMortar => "indirect_mortar",
        BallisticGroup::IndirectHowitzer => "indirect_howitzer",
        BallisticGroup::HeatShaped => "heat_shaped",
    }
}

pub fn resolve_registration_bonus(
    grid: &WorldGrid,
    uid: UnitId,
    units: &UnitSoA,
    commanders: &CommanderSoA,
    target_px: (i32, i32),
    bgroup: BallisticGroup,
) -> f32 {
    if !grid.in_bounds(target_px.0, target_px.1) {
        return 0.0;
    }
    let idx = grid.idx(target_px.0, target_px.1) as u32;
    let Some(bucket) = grid.cell_mods.get(&idx) else {
        return 0.0;
    };
    let team = units.team[uid.0].as_str();
    let (platoon_scope, company_scope) = get_unit_ao_scopes(uid, units, commanders);
    let bg_s = ballistic_group_key(bgroup);
    let piece_k = format!("reg:{team}:piece:{}", units.str_id[uid.0]);
    let comp_k = format!("reg:{team}:company:{company_scope}:all");
    let group_k = format!("reg:{team}:ao:{platoon_scope}:{bg_s}");
    let ao_pref = format!("reg:{team}:ao:{platoon_scope}:");

    let mut max_bonus = 0.0f32;
    for entry in bucket {
        let val = entry.accuracy_mult - 1.0;
        if val <= 0.0 {
            continue;
        }
        let eff = if entry.key == piece_k || entry.key == comp_k || entry.key == group_k {
            val
        } else if entry.key.starts_with(&ao_pref) {
            0.5 * val
        } else {
            continue;
        };
        max_bonus = max_bonus.max(eff);
    }
    max_bonus
}

pub fn apply_registration_to_cells(
    grid: &mut WorldGrid,
    uid: UnitId,
    units: &UnitSoA,
    commanders: &CommanderSoA,
    target_px: (i32, i32),
    radius_px: f32,
    weapon: &str,
    dist_m: f32,
    duration_min: f32,
    long_term: bool,
) -> f32 {
    let has_recon = units.kind[uid.0] == "recon_squad"
        || units.equipment[uid.0].get("radio").copied().unwrap_or(0) > 0 && units.alpha_stealth[uid.0] > 0.7;
    let spec = equipment_spec(weapon);
    let (eff_w, eff_d) = if has_recon && spec.range_m < 1500.0 {
        ("rifle", dist_m * (400.0 / 1500.0))
    } else {
        (weapon, dist_m)
    };
    let (delta, cap) = crate::units_ecs::compute_registration_delta(eff_w, eff_d, duration_min);
    let (platoon_scope, company_scope) = get_unit_ao_scopes(uid, units, commanders);
    let team = units.team[uid.0].as_str();
    let bg_s = ballistic_group_key(spec.ballistic_group);
    let ttl = if long_term { 4.0 } else { 0.25 };

    let mut keys = vec![
        format!("reg:{team}:piece:{}", units.str_id[uid.0]),
        format!("reg:{team}:ao:{platoon_scope}:{bg_s}"),
    ];
    if has_recon {
        keys.push(format!("reg:{team}:company:{company_scope}:all"));
    }

    let lim = radius_px.ceil() as i32;
    let r2 = radius_px * radius_px + 1e-6;
    let mut best_mult = 1.0f32;
    for dr in -lim..=lim {
        for dc in -lim..=lim {
            if (dr * dr + dc * dc) as f32 <= r2 {
                let rr = target_px.0 + dr;
                let cc = target_px.1 + dc;
                if grid.in_bounds(rr, cc) {
                    let idx = grid.idx(rr, cc) as u32;
                    for k in &keys {
                        let old_mult = grid
                            .cell_mods
                            .get(&idx)
                            .and_then(|b| b.iter().find(|e| &e.key == k))
                            .map(|e| e.accuracy_mult)
                            .unwrap_or(1.0);
                        let new_mult = (old_mult + delta).min(1.0 + cap);
                        let mut entry = CellModEntry::neutral(k.clone());
                        entry.accuracy_mult = new_mult;
                        entry.ttl_hours = Some(ttl);
                        grid.set_cell_modifier(rr, cc, entry);
                        best_mult = best_mult.max(new_mult);
                    }
                }
            }
        }
    }
    best_mult
}

#[derive(Debug, Clone, Default)]
pub struct StrikeReport {
    pub cell: (i32, i32),
    pub person_losses: u32,
    pub soft_losses: u32,
    pub armor_losses: u32,
    pub friendly_fire_losses: u32,
    pub org_loss: f32,
}

#[inline]
fn det_strike_roll(seed: &str, uid: UnitId) -> f32 {
    let mut h = Sha256::new();
    h.update(seed.as_bytes());
    h.update(uid.0.to_le_bytes());
    let d = h.finalize();
    (u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as f64 / u32::MAX as f64) as f32
}

/// Applies direct or HE/blast strike to a target pixel, including friendly fire rules,
/// directional cos(theta_i) cover projection, combined-arms tank screening, and reorganization interrupts!
pub fn resolve_cell_strike(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    events: &mut crate::units_ecs::EventQueue,
    target_px: (i32, i32),
    attacker_team: TeamId,
    fire_points: [f32; 6],
    weapon_model: &str,
    aspect_attacker_pos: [f32; 2],
    is_direct_fire: bool,
    seed_tag: &str,
) -> StrikeReport {
    resolve_cell_strike_ex(
        grid, units, commanders, events, target_px, attacker_team,
        fire_points, weapon_model, aspect_attacker_pos, is_direct_fire, seed_tag, false, None,
    )
}

pub fn resolve_cell_strike_ex(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    events: &mut crate::units_ecs::EventQueue,
    target_px: (i32, i32),
    attacker_team: TeamId,
    fire_points: [f32; 6],
    weapon_model: &str,
    aspect_attacker_pos: [f32; 2],
    is_direct_fire: bool,
    seed_tag: &str,
    allow_friendly_full: bool,
    exclude_uid: Option<UnitId>,
) -> StrikeReport {
    let mut rep = StrikeReport { cell: target_px, ..Default::default() };
    if !grid.in_bounds(target_px.0, target_px.1) {
        return rep;
    }
    let spec = equipment_spec(weapon_model);
    let is_he = !is_direct_fire
        || matches!(
            spec.ballistic_group,
            BallisticGroup::IndirectMortar | BallisticGroup::IndirectHowitzer
        );

    let (base_cov, _) = grid.effective_cover_at(target_px.0, target_px.1);
    let dist_m = grid.distance_km(aspect_attacker_pos, [target_px.0 as f32, target_px.1 as f32]) * 1000.0;

    // Register incoming shot packet in destination pixel container (unified entrance)
    let incoming_dir = (aspect_attacker_pos[1] - target_px.1 as f32)
        .atan2(aspect_attacker_pos[0] - target_px.0 as f32)
        .to_degrees()
        .rem_euclid(360.0);
    let packet = ShotPacket {
        target_px,
        origin_px: (aspect_attacker_pos[0].round() as i32, aspect_attacker_pos[1].round() as i32),
        priority_target: PriorityTarget::Any,
        ammo_spec_id: weapon_model.to_string(),
        shots: (fire_points.iter().sum::<f32>() * 10.0).round().max(1.0) as u32,
        shooter_accuracy: 0.85,
        incoming_dir_deg: incoming_dir,
        remaining_radius_m: if is_he { spec.frag_radius_m } else { 0.0 },
    };
    grid.inject_shot_packet(packet);

    // Collect candidate units in target cell (or within HE frag Gaussian radius)
    let frag_px = (spec.frag_radius_m / grid.cell_m).max(0.5);
    let max_cell_dist = if is_he { frag_px.ceil().max(1.0) } else { 0.5 };

    let mut targets: Vec<(UnitId, f32)> = Vec::new();
    for uid in 0..units.id.len() {
        if Some(UnitId(uid)) == exclude_uid || !units.alive[uid] || units.persons(UnitId(uid)) == 0 {
            continue;
        }
        let d_px = ((units.px[uid].0 - target_px.0) as f32).hypot((units.px[uid].1 - target_px.1) as f32);
        if d_px <= max_cell_dist {
            let gauss = if is_he {
                (-0.5 * (d_px / frag_px.max(0.5)).powi(2)).exp()
            } else if units.px[uid] == target_px {
                1.0
            } else {
                0.0
            };
            if gauss > 0.05 {
                targets.push((UnitId(uid), gauss));
            }
        }
    }

    for (uid, gauss_w) in targets {
        let i = uid.0;
        let is_friendly = units.team[i] == attacker_team;
        let ff_mult = if is_friendly && !allow_friendly_full {
            if is_he {
                0.55 * gauss_w
            } else if units.px[i] == target_px {
                if dist_m <= 210.0 { 0.05 } else { 0.02 }
            } else {
                0.0
            }
        } else {
            gauss_w
        };
        if ff_mult <= 1e-6 {
            continue;
        }

        // Trigger reorganization abort if this unit is highest commander unit under attack!
        if !is_friendly {
            events.notify_unit_attacked(units, commanders, Some(uid), attacker_team, Some(target_px));
        }

        let (cov_eff, flank_mult, _) = if units.is_directional_hull(uid) {
            compute_directional_cover_eff(units.pos[i], units.facing_deg[i], aspect_attacker_pos, base_cov)
        } else {
            (base_cov * 0.65, 1.0, 0.5)
        };

        // Combined arms tank screening: check friendly infantry within <= 1px
        let mut heat_crit = 1.0f32;
        if units.is_directional_hull(uid) && dist_m <= 220.0 && matches!(spec.ballistic_group, BallisticGroup::HeatShaped) {
            let has_inf_screen = (0..units.id.len()).any(|j| {
                j != i
                    && units.alive[j]
                    && units.team[j] == units.team[i]
                    && !units.is_directional_hull(UnitId(j))
                    && units.persons(UnitId(j)) > 0
                    && ((units.px[j].0 - units.px[i].0).abs() <= 1 && (units.px[j].1 - units.px[i].1).abs() <= 1)
            });
            if has_inf_screen {
                heat_crit = 1.0 - 0.35; // 0.35 armor screening mitigation
            } else if det_strike_roll(seed_tag, uid) < 0.35 {
                heat_crit = 2.2;
            }
        }

        let dmg_scale = ff_mult * (1.0 - cov_eff) * flank_mult;
        let pers_pts = (fire_points[DamageType::AntiPersonnel as usize] * 0.9
            + fire_points[DamageType::Destructive as usize] * 0.4)
            * dmg_scale;
        let org_pts = (fire_points[DamageType::Suppression as usize] * 0.85
            + fire_points[DamageType::AntiPersonnel as usize] * 0.3)
            * dmg_scale
            + fire_points[DamageType::Incendiary as usize] * 0.4;
        let armor_pts = (fire_points[DamageType::AntiArmor as usize] * heat_crit
            + fire_points[DamageType::Destructive as usize] * 0.35)
            * dmg_scale;

        let prev_cadre = units.cadre[i];
        let prev_p = units.persons(uid) as i32;

        let mut hp_dmg_pool = (pers_pts / PERSON_DURABILITY) * 100.0;
        let mut actual_lost = 0u32;
        if hp_dmg_pool > 0.05 && !units.soldiers[i].is_empty() {
            let mut h: u64 = 14695981039346656037;
            for b in seed_tag.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(1099511628211);
            }
            h ^= (uid.0 as u64).wrapping_mul(2654435761);
            let mut guard = 0usize;
            while hp_dmg_pool > 0.05 && !units.soldiers[i].is_empty() && guard < 128 {
                guard += 1;
                h ^= h << 13;
                h ^= h >> 7;
                h ^= h << 17;
                let n = units.soldiers[i].len();
                let mut pick = (h as usize) % n;
                if units.soldiers[i][pick].is_cadre && n > 1 {
                    if let Some(enl_idx) = units.soldiers[i].iter().position(|s| !s.is_cadre) {
                        pick = enl_idx;
                    }
                }
                let hit_hp = hp_dmg_pool.min(48.0);
                hp_dmg_pool -= hit_hp;
                units.soldiers[i][pick].hp = (units.soldiers[i][pick].hp - hit_hp).max(0.0);
                let new_hp = units.soldiers[i][pick].hp;
                if new_hp <= 0.0 || new_hp <= 30.0 {
                    let mut sol = units.soldiers[i].remove(pick);
                    let is_kia = new_hp <= 0.0;
                    sol.status = if is_kia { "dead".to_string() } else { "heavy".to_string() };
                    actual_lost += 1;
                    // 人倒地时：只把他手持槽 (slot == "hand") 的东西掉在地上，其余背包里的全部留在自己身上
                    let mut dropped_hand = Vec::new();
                    sol.items.retain(|it| {
                        if it.slot == "hand" && it.count > 0 {
                            dropped_hand.push(it.clone());
                            false
                        } else {
                            true
                        }
                    });
                    sol.weapon = String::new();
                    for mut held in dropped_hand {
                        held.slot = "pack".to_string();
                        let w_id = format!("body-wpn-{}-{}-{}", units.str_id[i], sol.id, grid.ground_bodies.len());
                        let w_sol = crate::units_ecs::SoldierState {
                            id: w_id.clone(),
                            name: held.name.clone(),
                            hp: 100.0,
                            max_hp: 100.0,
                            status: "weapon".to_string(),
                            weapon: held.model.clone(),
                            ammo_t: 0.0,
                            is_cadre: false,
                            role: "enlisted".to_string(),
                            cannot_fire: false,
                            cannot_operate_heavy: false,
                            max_hand_weight_kg: None,
                            max_pack_weight_kg: None,
                            pack_cols: 8,
                            pack_rows: 6,
                            items: vec![held],
                        };
                        grid.ground_bodies.push(crate::units_ecs::GroundBody {
                            id: w_id,
                            kind: "weapon".to_string(),
                            soldier: w_sol,
                            origin_unit_id: units.str_id[i].clone(),
                            origin_unit_name: units.name[i].clone(),
                            team: units.team[i],
                            pos: units.pos[i],
                            looted: false,
                        });
                    }
                    let body_id = format!(
                        "body-{}-{}-{}-{}",
                        if is_kia { "kia" } else { "wnd" },
                        units.str_id[i],
                        sol.id,
                        grid.ground_bodies.len()
                    );
                    grid.ground_bodies.push(crate::units_ecs::GroundBody {
                        id: body_id,
                        kind: if is_kia { "kia" } else { "wounded" }.to_string(),
                        soldier: sol,
                        origin_unit_id: units.str_id[i].clone(),
                        origin_unit_name: units.name[i].clone(),
                        team: units.team[i],
                        pos: units.pos[i],
                        looted: false,
                    });
                } else if new_hp <= 70.0 {
                    units.soldiers[i][pick].status = "light".to_string();
                }
            }
        }
        units.carry_loss[i] = 0.0;
        units.sync_totals_from_soldiers(uid);
        if prev_cadre > 0 && units.cadre[i] == 0 {
            units.org[i] = (units.org[i] - CADRE_LOSS_ORG_PENALTY).max(0.0);
        }

        if units.vehicle_model[i].is_some() && armor_pts > SOFT_DURABILITY {
            let kill_thresh = if units.vehicle_model[i].as_deref() == Some("truck") {
                SOFT_DURABILITY * 1.5
            } else {
                ARMOR_DURABILITY * 1.5
            };
            if armor_pts >= kill_thresh {
                for (_, n) in units.equipment[i].iter_mut() {
                    if *n > 0 {
                        *n -= 1;
                        rep.armor_losses += 1;
                        break;
                    }
                }
            }
        }

        let d_org = org_pts / ORG_DURABILITY * 12.0;
        units.org[i] = (units.org[i] - d_org).max(0.0);
        let new_p = units.persons(uid) as i32;
        units.delta_persons_5m[i] += new_p - prev_p;
        units.delta_org_5m[i] -= d_org;
        units.exposed[i] = true;

        if units.persons(uid) == 0 {
            units.alive[i] = false;
        } else {
            units.refresh_caps(uid);
        }

        if is_friendly {
            rep.friendly_fire_losses += actual_lost;
        } else {
            rep.person_losses += actual_lost;
        }
        rep.org_loss += d_org;
    }

    if is_he && fire_points[DamageType::Destructive as usize] > 1.0 {
        grid.cut_wire_at(target_px.0, target_px.1, frag_px.max(1.0), None);
    }
    rep
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelContact {
    pub unit_id: UnitId,
    pub px: (i32, i32),
    pub fog_level: u8,
    pub facing_deg: Option<f32>, // Hidden for infantry, exposed only for vehicles at level >= 2
    pub est_persons: u32,
}

pub fn gather_intel(
    observer_team: TeamId,
    grid: &WorldGrid,
    units: &UnitSoA,
    night: bool,
) -> Vec<IntelContact> {
    let mut contacts = Vec::new();
    let n = units.id.len();
    for eid in 0..n {
        if !units.alive[eid] || units.team[eid] == observer_team || units.persons(UnitId(eid)) == 0 {
            continue;
        }
        let e_pos = units.pos[eid];
        let e_px = units.px[eid];
        let e_idx = grid.idx(e_px.0, e_px.1);
        let mut best_lvl = 0u8;

        if grid.exposure[e_idx] >= SKY_EYE_EXPOSURE_THRESHOLD {
            best_lvl = best_lvl.max(2);
        }

        for fid in 0..n {
            if !units.alive[fid] || units.team[fid] != observer_team || units.persons(UnitId(fid)) == 0 {
                continue;
            }
            let d_km = grid.distance_km(units.pos[fid], e_pos);
            let obs_range = BASE_OBSERVE_KM + if units.kind[fid] == "recon_squad" { RECON_BONUS_KM } else { 0.0 };
            if d_km <= obs_range {
                let los = cast_los_ray_dda(grid, units.px[fid], e_px, night);
                if !los.blocked && los.visibility > 0.15 {
                    best_lvl = best_lvl.max(if d_km <= 0.6 { 3 } else { 2 });
                }
            } else if !units.radio_silence[eid] && d_km <= DF_RANGE_KM {
                best_lvl = best_lvl.max(1);
            }
        }

        if best_lvl > 0 {
            let blur_km: i32 = match best_lvl {
                1 => 4,
                2 => 1,
                _ => 0,
            };
            let _ = blur_km;
            let facing_deg = if best_lvl >= 2 && units.is_directional_hull(UnitId(eid)) {
                Some(units.facing_deg[eid])
            } else {
                None
            };
            contacts.push(IntelContact {
                unit_id: UnitId(eid),
                px: e_px,
                fog_level: best_lvl,
                facing_deg,
                est_persons: units.persons(UnitId(eid)),
            });
        }
    }
    contacts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellType {
    He,
    Ap,
    Heat,
    Wp,
    Smoke,
    Illum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub str_id: String,
    pub team: TeamId,
    pub px: (i32, i32),
    pub caliber_mm: u32,
    pub tubes: u32,
    pub shells: [f32; 6],
}

#[derive(Debug, Clone, Default)]
pub struct FireFields {
    pub hazard_by_cell_red: FxHashMap<u32, f32>,
    pub hazard_by_cell_blue: FxHashMap<u32, f32>,
}

impl FireFields {
    pub fn add_barrage(&mut self, grid: &WorldGrid, center: (i32, i32), radius_px: i32, attacker: TeamId, intensity: f32) {
        let victim = attacker.enemy();
        let target_map = match victim {
            TeamId::Red => &mut self.hazard_by_cell_red,
            TeamId::Blue => &mut self.hazard_by_cell_blue,
        };
        let r2 = radius_px * radius_px;
        for dr in -radius_px..=radius_px {
            for dc in -radius_px..=radius_px {
                if dr * dr + dc * dc <= r2 && grid.in_bounds(center.0 + dr, center.1 + dc) {
                    let idx = grid.idx(center.0 + dr, center.1 + dc) as u32;
                    *target_map.entry(idx).or_insert(0.0) += intensity;
                }
            }
        }
    }

    pub fn hazard_at(&self, team: TeamId, idx: u32) -> f32 {
        match team {
            TeamId::Red => self.hazard_by_cell_red.get(&idx).copied().unwrap_or(0.0),
            TeamId::Blue => self.hazard_by_cell_blue.get(&idx).copied().unwrap_or(0.0),
        }
    }
}

pub fn execute_artillery_mission(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    events: &mut crate::units_ecs::EventQueue,
    fire_fields: &mut FireFields,
    battery: &mut Battery,
    target_px: (i32, i32),
    shell: ShellType,
    rounds: u32,
    observed: bool,
    radius_px: i32,
    seed_tag: &str,
) -> Option<StrikeReport> {
    let si = shell as usize;
    let avail = battery.shells[si].floor() as u32;
    let fired = rounds.min(avail);
    if fired == 0 {
        return None;
    }
    battery.shells[si] -= fired as f32;
    grid.add_exposure(battery.px.0, battery.px.1, (fired as f32) * 0.4, 1.0);

    match shell {
        ShellType::Smoke => {
            for dr in -radius_px..=radius_px {
                for dc in -radius_px..=radius_px {
                    if dr * dr + dc * dc <= radius_px * radius_px {
                        grid.apply_smoke(target_px.0 + dr, target_px.1 + dc, 0.75, 0.5);
                    }
                }
            }
            None
        }
        ShellType::Illum => {
            for dr in -radius_px..=radius_px {
                for dc in -radius_px..=radius_px {
                    if dr * dr + dc * dc <= radius_px * radius_px {
                        grid.apply_illumination(target_px.0 + dr, target_px.1 + dc, 0.5);
                    }
                }
            }
            None
        }
        _ => {
            let obs_mult = if observed { 1.0 } else { UNOBSERVED_FACTOR };
            let cal_scale = (battery.caliber_mm as f32 / 105.0).max(0.5);
            let pts = fired as f32 * cal_scale * obs_mult * 1.4;
            let fire = match shell {
                ShellType::He => [pts, pts * 1.2, pts * 0.5, pts * 0.2, pts * 0.7, 0.0],
                ShellType::Wp => [pts * 0.6, pts * 1.1, pts * 0.3, 0.0, pts * 0.2, pts * 1.3],
                ShellType::Heat | ShellType::Ap => [pts * 0.2, pts * 0.3, pts * 0.8, pts * 1.6, pts * 0.3, 0.0],
                _ => [0.0; 6],
            };
            fire_fields.add_barrage(grid, target_px, radius_px.max(1), battery.team, pts * 0.08);
            let w_model = if battery.caliber_mm >= 140 { "howitzer_152" } else { "howitzer_105" };
            let rep = resolve_cell_strike(
                grid,
                units,
                commanders,
                events,
                target_px,
                battery.team,
                fire,
                w_model,
                [battery.px.0 as f32, battery.px.1 as f32],
                false,
                seed_tag,
            );
            Some(rep)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirRole {
    Fighter,
    DiveBomber,
    TacticalBomber,
    Recon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirWing {
    pub str_id: String,
    pub team: TeamId,
    pub role: AirRole,
    pub airfield_px: (i32, i32),
    pub radio_channel: u8,
    pub air_radio_radius_km: f32,
    pub readiness: f32,
    pub aircraft: u32,
}

pub fn can_receive_air_order(
    wing: &AirWing,
    commander: &CommanderSoA,
    cmd_id: CommanderId,
    units: &UnitSoA,
    grid: &WorldGrid,
) -> bool {
    if !commander.alive[cmd_id.0] || commander.team[cmd_id.0] != wing.team {
        return false;
    }
    let c_px = commander.px[cmd_id.0];
    let d_km = grid.distance_km([c_px.0 as f32, c_px.1 as f32], [wing.airfield_px.0 as f32, wing.airfield_px.1 as f32]);
    if d_km <= 0.4001 || d_km <= wing.air_radio_radius_km {
        if !commander.radio_silence[cmd_id.0] && commander.radio_channel[cmd_id.0] == wing.radio_channel {
            return true;
        }
    }
    // Check air-ground liaison team (`fac_radio` or `hq_radio` matching wing channel)
    for uid in 0..units.id.len() {
        if units.alive[uid]
            && units.team[uid] == wing.team
            && !units.radio_silence[uid]
            && units.radio_channel[uid] == wing.radio_channel
            && units.equipment[uid].get("hq_radio").copied().unwrap_or(0) > 0
            && validate_command_link(cmd_id, UnitId(uid), commander, units, grid).is_connected()
        {
            return true;
        }
    }
    false
}

pub fn run_air_subtick_1h(wings: &mut [AirWing], grid: &WorldGrid) -> usize {
    let mut sorties = 0usize;
    for w in wings.iter_mut() {
        let ai = grid.idx(w.airfield_px.0, w.airfield_px.1);
        let wx = grid.weather[ai];
        if matches!(wx, FOG | SNOW | ICE) || grid.terrain[ai] == WATER {
            continue; // Grounded in severe weather or destroyed airfield!
        }
        if grid.terrain[ai] == AIRFIELD && w.aircraft > 0 && w.readiness > 0.25 {
            sorties += w.aircraft as usize;
        }
    }
    sorties
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Depot {
    pub str_id: String,
    pub team: TeamId,
    pub px: (i32, i32),
    pub food_t: f32,
    pub ammo_t: f32,
    pub fuel_t: f32,
    pub replacements: f32,
}

#[derive(Debug, Clone, Default)]
pub struct LogisticsConservationReport {
    pub initial_total_t: f32,
    pub delivered_total_t: f32,
    pub lost_total_t: f32,
    pub remaining_depot_t: f32,
    pub conservation_error: f32,
}

pub fn dispatch_logistics_step(
    grid: &WorldGrid,
    depots: &mut [Depot],
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    zones: &[OperationalZone],
    fire_fields: &FireFields,
    dt_hours: f32,
    current_turn: u32,
) -> LogisticsConservationReport {
    let mut rep = LogisticsConservationReport::default();
    for d in depots.iter() {
        rep.initial_total_t += d.food_t + d.ammo_t + d.fuel_t;
    }
    if depots.is_empty() || dt_hours <= 0.0 {
        rep.remaining_depot_t = rep.initial_total_t;
        return rep;
    }

    for uid in 0..units.id.len() {
        let u = UnitId(uid);
        if !units.alive[uid] || units.persons(u) == 0 {
            continue;
        }
        let team = units.team[uid];
        let u_px = units.px[uid];
        let need_f = (units.cap_food_t[uid] - units.food_t[uid]).max(0.0);
        let need_a = (units.cap_ammo_t[uid] - units.ammo_t[uid]).max(0.0);
        let need_g = (units.cap_fuel_t[uid] - units.fuel_t[uid]).max(0.0);
        if need_f + need_a + need_g < 1e-4 {
            continue;
        }

        // Find nearest friendly depot with stock
        let mut best_d: Option<(usize, f32)> = None;
        for (di, d) in depots.iter().enumerate() {
            if d.team != team || (d.food_t + d.ammo_t + d.fuel_t) <= 1e-4 {
                continue;
            }
            let km = grid.distance_km([d.px.0 as f32, d.px.1 as f32], [u_px.0 as f32, u_px.1 as f32]);
            if km <= 35.0 && best_d.map_or(true, |(_, bk)| km < bk) {
                best_d = Some((di, km));
            }
        }
        let Some((di, dist_km)) = best_d else {
            continue;
        };

        let ao_b = compute_operational_ao_buff(
            u_px,
            team,
            units.direct_commander_id[uid],
            zones,
            commanders,
            units,
            grid,
            current_turn,
        );
        let haz = fire_fields.hazard_at(team, grid.idx(u_px.0, u_px.1) as u32);
        let base_loss_rate = (TRUNK_LOSS_PER_100KM * (dist_km / 100.0)
            + LASTMILE_LOSS_PER_KM * dist_km.min(4.0) * ao_b.logistics_mult
            + if haz > 0.5 { 0.15 } else { 0.0 })
            .clamp(0.0, 0.85);

        let porters = units.persons(u).max(1) as f32 * 0.15;
        let trips = (PORTER_SPEED_KMH * dt_hours / dist_km.max(0.4).min(4.0)).min(2.0);
        let max_haul_t = (porters * PORTER_LOAD_T * trips).max(0.01);

        let dep = &mut depots[di];
        for (req, dep_stock, unit_stock) in [
            (need_f, &mut dep.food_t, &mut units.food_t[uid]),
            (need_a, &mut dep.ammo_t, &mut units.ammo_t[uid]),
            (need_g, &mut dep.fuel_t, &mut units.fuel_t[uid]),
        ] {
            let draw = req.min(*dep_stock).min(max_haul_t);
            if draw > 1e-6 {
                *dep_stock -= draw;
                let lost = draw * base_loss_rate;
                let deliv = draw - lost;
                *unit_stock += deliv;
                rep.delivered_total_t += deliv;
                rep.lost_total_t += lost;
            }
        }
    }

    for d in depots.iter() {
        rep.remaining_depot_t += d.food_t + d.ammo_t + d.fuel_t;
    }
    rep.conservation_error =
        (rep.initial_total_t - (rep.delivered_total_t + rep.lost_total_t + rep.remaining_depot_t)).abs();
    rep
}

pub fn abandon_heavy_equipment_to_stash(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    uid: UnitId,
    assist_mult: f32,
) -> Vec<(String, u32)> {
    let i = uid.0;
    let mut left = Vec::new();
    let keep_frac = (0.35 + 0.45 * assist_mult).clamp(0.25, 0.90);
    let px = units.px[i];
    for (m, n) in units.equipment[i].iter_mut() {
        if *n > 0 && equipment_spec(m).is_heavy {
            let lost = ((*n as f32) * (1.0 - keep_frac)).round() as u32;
            if lost > 0 {
                *n -= lost;
                left.push((m.clone(), lost));
            }
        }
    }
    if !left.is_empty() {
        grid.stash_put_equipment(px.0, px.1, Some(units.team[i]), Some(units.radio_channel[i]), &left);
    }
    left
}

/// 计算单位在 200m x 200m (40,000 m²) 像素格内的基础体型受弹截面权重 W_i
pub fn unit_cross_section_weight(uid: UnitId, units: &UnitSoA) -> f32 {
    let i = uid.0;
    if !units.alive[i] {
        return 0.0;
    }
    let p = units.persons(uid) as f32;
    if units.is_directional_hull(uid) || units.vehicle_model[i].is_some() {
        // 载具车体暴露截面较大：基准 0.10 + 乘员特征
        0.10 + p * 0.005
    } else if units.kind[i] == "heavy_weapon_emplacement" || units.heavy_op_cap[i] > 1.0 {
        // 重武器阵地与炮盾截面：基准 0.05 + 操炮组
        0.05 + p * 0.005
    } else {
        // 普通步兵/战斗小组：按单兵散开截面 (每人 0.008, 下限 0.002)
        (p * 0.008).max(0.002)
    }
}

/// 三段式破片整数 floor 过滤：
/// 阶段 1：格内单位截面总和过滤空地打空破片 (N_effective = floor(N_frag * W_total)，< 1 销毁)；
/// 阶段 2：按体型截面占比整数分摊到各单位 (S_i = floor(N_effective * (W_i / W_total))，< 1 本次不分摊)；
/// 阶段 3：单位结合掩体与姿态过滤实弹发数 (M_i = floor(S_i * C_cover * Buffs)，< 1 删除)。
pub fn filter_and_dispatch_fragments(
    n_frag: u32,
    target_cell: (i32, i32),
    grid: &WorldGrid,
    units: &UnitSoA,
    candidate_uids: &[UnitId],
) -> Vec<(UnitId, u32)> {
    if n_frag == 0 || candidate_uids.is_empty() {
        return Vec::new();
    }

    let mut weights: Vec<(UnitId, f32)> = Vec::with_capacity(candidate_uids.len());
    let mut sum_w = 0.0f32;
    for &uid in candidate_uids {
        let w = unit_cross_section_weight(uid, units);
        if w > 0.0 {
            sum_w += w;
            weights.push((uid, w));
        }
    }
    if weights.is_empty() {
        return Vec::new();
    }

    // 阶段 1：算空地打空发数
    let w_total = sum_w.min(1.0);
    let n_effective = ((n_frag as f32) * w_total).floor() as u32;
    if n_effective < 1 {
        return Vec::new();
    }

    let (base_cov, _) = grid.effective_cover_at(target_cell.0, target_cell.1);
    let mut result = Vec::with_capacity(weights.len());

    // 阶段 2 & 阶段 3
    for (uid, w_i) in weights {
        let s_i = ((n_effective as f32) * (w_i / sum_w.max(1e-4))).floor() as u32;
        if s_i < 1 {
            continue;
        }

        let c_cover = (1.0 - base_cov * 0.65).clamp(0.05, 1.0);
        let posture_buff = if units.is_directional_hull(uid) {
            1.0
        } else if units.autonomy_state[uid.0] == crate::units_ecs::AtomicAction::Hold {
            0.8
        } else {
            1.0
        };
        let m_i = ((s_i as f32) * c_cover * posture_buff).floor() as u32;
        if m_i >= 1 {
            result.push((uid, m_i));
        }
    }

    result
}

/// 152mm 大口径高爆弹跨格自然扩散衰减与向下取整转发
pub fn forward_152mm_he_packet(
    grid: &mut WorldGrid,
    packet: &ShotPacket,
) -> Vec<ShotPacket> {
    if packet.remaining_radius_m <= 100.0 {
        return Vec::new();
    }
    let (r0, c0) = packet.target_px;
    let outbound_pool = ((packet.shots as f32) * 0.28).floor() as u32;
    if outbound_pool < 1 {
        return Vec::new();
    }
    let new_remaining_radius = (packet.remaining_radius_m - 200.0).max(0.0);
    let mut forwarded = Vec::new();

    let orth_dirs = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let diag_dirs = [(-1, -1), (-1, 1), (1, -1), (1, 1)];

    let orth_shots = ((outbound_pool as f32) * 0.16).floor() as u32;
    let diag_shots = ((outbound_pool as f32) * 0.09).floor() as u32;

    for (dr, dc) in orth_dirs {
        let tr = r0 + dr;
        let tc = c0 + dc;
        if grid.in_bounds(tr, tc) && orth_shots >= 1 {
            let angle = (dc as f32).atan2(dr as f32).to_degrees().rem_euclid(360.0);
            let p = ShotPacket {
                target_px: (tr, tc),
                origin_px: packet.origin_px,
                priority_target: PriorityTarget::Any,
                ammo_spec_id: packet.ammo_spec_id.clone(),
                shots: orth_shots,
                shooter_accuracy: packet.shooter_accuracy * 0.85,
                incoming_dir_deg: angle,
                remaining_radius_m: new_remaining_radius,
            };
            grid.inject_shot_packet(p.clone());
            forwarded.push(p);
        }
    }

    for (dr, dc) in diag_dirs {
        let tr = r0 + dr;
        let tc = c0 + dc;
        if grid.in_bounds(tr, tc) && diag_shots >= 1 {
            let angle = (dc as f32).atan2(dr as f32).to_degrees().rem_euclid(360.0);
            let p = ShotPacket {
                target_px: (tr, tc),
                origin_px: packet.origin_px,
                priority_target: PriorityTarget::Any,
                ammo_spec_id: packet.ammo_spec_id.clone(),
                shots: diag_shots,
                shooter_accuracy: packet.shooter_accuracy * 0.75,
                incoming_dir_deg: angle,
                remaining_radius_m: new_remaining_radius,
            };
            grid.inject_shot_packet(p.clone());
            forwarded.push(p);
        }
    }

    forwarded
}

/// 单发毫米级穿深与模块化毁伤报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileHitReport {
    pub target_unit: UnitId,
    pub is_vehicle: bool,
    pub hit_location: String,
    pub penetration_mm: f32,
    pub armor_effective_mm: f32,
    pub penetrated: bool,
    pub damage_state: String,
    pub killed: bool,
    pub is_bleeding: bool,
}

pub fn resolve_projectile_hit(
    units: &mut UnitSoA,
    target: UnitId,
    ammo_spec_id: &str,
    incoming_dir_deg: f32,
    dist_m: f32,
    _shooter_acc: f32,
    seed_tag: &str,
) -> ProjectileHitReport {
    let i = target.0;
    let is_veh = units.is_directional_hull(target) || units.vehicle_model[i].is_some();

    let base_pen_mm = match ammo_spec_id {
        "wpn_pak40" | "at_gun_75" => 130.0,
        "wpn_flak36_88" | "flak_88" => 160.0,
        "at_gun_50" => 75.0,
        "152mm" | "artillery_152" | "wpn_ml20" => 25.0 * (-dist_m / 600.0).exp().max(0.2),
        "mortar_81" | "wpn_grw34" => 10.0 * (-dist_m / 300.0).exp().max(0.1),
        "hmg" | "wpn_mg42" => 6.0 * (-dist_m / 800.0).exp().max(0.1),
        _ => 4.0 * (-dist_m / 500.0).exp().max(0.1),
    };

    if is_veh {
        let rel_angle = (incoming_dir_deg - units.facing_deg[i]).rem_euclid(360.0);
        let (face, nom_rha) = if rel_angle <= 45.0 || rel_angle >= 315.0 {
            ("hull_front", 50.0)
        } else if rel_angle > 45.0 && rel_angle <= 135.0 {
            ("hull_right", 30.0)
        } else if rel_angle > 135.0 && rel_angle <= 225.0 {
            ("hull_rear", 20.0)
        } else {
            ("hull_left", 30.0)
        };

        if base_pen_mm < nom_rha {
            return ProjectileHitReport {
                target_unit: target,
                is_vehicle: true,
                hit_location: face.into(),
                penetration_mm: base_pen_mm,
                armor_effective_mm: nom_rha,
                penetrated: false,
                damage_state: "ricochet".into(),
                killed: false,
                is_bleeding: false,
            };
        }

        let mut h = 14695981039346656037u64;
        for b in seed_tag.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        let mod_idx = (h % 8) as usize;
        let mod_names = ["breech", "gunner_sight", "mg", "engine", "transmission", "fuel_tank", "radio", "ammo_rack"];
        let hit_mod = mod_names[mod_idx];

        let killed = hit_mod == "ammo_rack";
        let dmg_state = if killed {
            units.alive[i] = false;
            "destroyed"
        } else if hit_mod == "engine" || hit_mod == "breech" {
            "severely_damaged"
        } else {
            "damaged"
        };

        ProjectileHitReport {
            target_unit: target,
            is_vehicle: true,
            hit_location: format!("{face}:{hit_mod}"),
            penetration_mm: base_pen_mm,
            armor_effective_mm: nom_rha,
            penetrated: true,
            damage_state: dmg_state.into(),
            killed,
            is_bleeding: false,
        }
    } else {
        let mut h = 14695981039346656037u64;
        for b in seed_tag.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        let part_roll = (h % 100) as u32;
        let (loc, armor_rha) = if part_roll < 10 {
            ("head", 0.5)
        } else if part_roll < 50 {
            ("torso", 0.0)
        } else if part_roll < 62 {
            ("left_arm", 0.0)
        } else if part_roll < 75 {
            ("right_arm", 0.0)
        } else if part_roll < 87 {
            ("left_leg", 0.0)
        } else {
            ("right_leg", 0.0)
        };

        if base_pen_mm < armor_rha {
            return ProjectileHitReport {
                target_unit: target,
                is_vehicle: false,
                hit_location: loc.into(),
                penetration_mm: base_pen_mm,
                armor_effective_mm: armor_rha,
                penetrated: false,
                damage_state: "ricochet".into(),
                killed: false,
                is_bleeding: false,
            };
        }

        let mut killed = false;
        let mut is_bleeding = false;
        let mut state_str = "grazed";

        if !units.soldiers[i].is_empty() {
            if let Some(s) = units.soldiers[i].iter_mut().find(|s| s.status != "dead") {
                let dmg = match loc {
                    "head" => 60.0,
                    "torso" => 45.0,
                    _ => 25.0,
                };
                s.hp = (s.hp - dmg).max(0.0);
                if dmg >= 15.0 {
                    is_bleeding = true;
                    state_str = "wounded";
                }
                if (loc == "torso" && s.hp < 25.0) || (loc == "head" && s.hp < 55.0) {
                    s.status = "dead".to_string();
                    killed = true;
                    state_str = "dead";
                } else if (loc == "head" && s.hp < 75.0)
                    || (loc == "torso" && s.hp < 50.0)
                    || ((loc.contains("arm") || loc.contains("leg")) && s.hp < 25.0)
                {
                    s.status = "heavy".to_string();
                    state_str = "wounded";
                }
            }
            units.sync_totals_from_soldiers(target);
        }

        ProjectileHitReport {
            target_unit: target,
            is_vehicle: false,
            hit_location: loc.into(),
            penetration_mm: base_pen_mm,
            armor_effective_mm: armor_rha,
            penetrated: true,
            damage_state: state_str.into(),
            killed,
            is_bleeding,
        }
    }
}

/// 战场搜刮（瞬发结算，0 耗时）
pub fn scavenge_ground_body(
    units: &mut UnitSoA,
    looting_unit: UnitId,
    body: &mut crate::units_ecs::GroundBody,
) -> bool {
    if body.looted {
        return false;
    }
    body.looted = true;
    for it in &body.soldier.items {
        *units.equipment[looting_unit.0].entry(it.model.clone()).or_insert(0) += it.count;
    }
    units.ammo_t[looting_unit.0] += body.soldier.ammo_t;
    true
}

/// 物资装卸与补给分发按物资吨位吞吐量结算 (每 0.5 吨消耗 60 秒，即 120 秒 / 吨)
pub fn logistics_tonnage_transfer_sec(tonnage: f32) -> u32 {
    (tonnage * 120.0).round() as u32
}
