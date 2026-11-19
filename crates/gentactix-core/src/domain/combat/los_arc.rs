//! `combat_logistics` module: Unified integer DDA LOS raycaster + ballistic decay,
//! directional cover cos(theta_i), tank-infantry CQB screening, registration sharing,
//! 2D Gaussian HE friendly fire, artillery FireFields, 1h air subtick, recon/fog, and conserving logistics.

use crate::units_ecs::{equipment_spec, BallisticGroup, CommanderSoA, Echelon, UnitId, UnitSoA};
use crate::world_grid::{CellModEntry, WorldGrid, NIGHT_BASE_VIS, WEATHER_VIS};
use serde::{Deserialize, Serialize};

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

