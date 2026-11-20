use crate::catalog::ItemCatalog;
use crate::damage_model::{select_armor_facing, SoldierRecord};
use crate::math_ops::{CellCoord, SchmidtPostureState, WorldPos, TACTICAL_CELL_SIZE_M};
use crate::state::{DerivedState, UnitRecord, WorldState};
use crate::step_cache::WorldInstanceKind;
use crate::terrain::{TerrainGrid, TerrainKind};
use crate::types::{FireDiscipline, Team, UnitId, TICK_SECONDS_F32};
use rayon::prelude::*;

const CONE_H_BINS: usize = 8;
const CONE_V_BINS: usize = 4;
const CONE_RAYS: usize = CONE_H_BINS * CONE_V_BINS;
const BLAST_AZ_BINS: usize = 16;

#[derive(Debug, Clone)]
pub struct DirectConeField {
    pub event_id: u64,
    pub source_id: UnitId,
    pub source_team: Team,
    pub origin: WorldPos,
    pub muzzle_z: f32,
    pub aim_pos: WorldPos,
    pub aim_dist_m: f32,
    pub max_path_m: f32,
    pub dir: [f32; 2],
    pub tan_half_h: f32,
    pub s_cone_min: f32,
    pub s_cone_max: f32,
    pub rounds_fired: u32,
    pub dmg_decay_coeff: f32,
    pub damage: [f32; 6],
    pub armor_pen_mm: f32,
    pub frag_radius_m: f32,
    pub ignore_trench_cover: f32,
    pub suppressive_acc_mult: f32,
    pub det_seed: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct CorridorCandidate {
    pub bucket_idx: u32,
    pub slot_idx: u32,
    pub unit_id: u32,
    pub dist_along: f32,
    pub lat_v: f32,
    pub radial_r_sq: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct UnitHitDelta {
    pub target_id: UnitId,
    pub dist_m: f32,
    pub is_overhead: bool,
    pub absorbed_fraction: f32,
    pub rounds_absorbed: f32,
    pub dmg_mult: f32,
    pub suppression_add: f32,
    pub det_seed: u64,
}

const INLINE_HIT_CAP: usize = 5;

/// Zero-heap, zero-memset small buffer for typical per-cone hit deltas (<= 4 hits inline).
pub struct HitList {
    inline: [std::mem::MaybeUninit<UnitHitDelta>; INLINE_HIT_CAP],
    inline_len: u8,
    overflow: Vec<UnitHitDelta>,
}

impl HitList {
    #[inline]
    pub fn new() -> Self {
        Self {
            inline: [const { std::mem::MaybeUninit::uninit() }; INLINE_HIT_CAP],
            inline_len: 0,
            overflow: Vec::new(),
        }
    }

    #[inline]
    pub fn push(&mut self, item: UnitHitDelta) {
        if self.inline_len == u8::MAX {
            self.overflow.push(item);
        } else if (self.inline_len as usize) < INLINE_HIT_CAP {
            self.inline[self.inline_len as usize].write(item);
            self.inline_len += 1;
        } else {
            let existing = unsafe {
                std::slice::from_raw_parts(
                    self.inline.as_ptr() as *const UnitHitDelta,
                    self.inline_len as usize,
                )
            };
            self.overflow.reserve(INLINE_HIT_CAP + 4);
            self.overflow.extend_from_slice(existing);
            self.overflow.push(item);
            self.inline_len = u8::MAX;
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        if self.inline_len == u8::MAX {
            self.overflow.clear();
        }
        self.inline_len = 0;
    }

    #[inline]
    pub fn as_slice(&self) -> &[UnitHitDelta] {
        if self.inline_len == u8::MAX {
            self.overflow.as_slice()
        } else {
            // SAFETY: elements `0..inline_len` are initialized `UnitHitDelta` values before `inline_len` increments,
            // and `MaybeUninit<UnitHitDelta>` has identical layout to `UnitHitDelta`.
            unsafe {
                std::slice::from_raw_parts(
                    self.inline.as_ptr() as *const UnitHitDelta,
                    self.inline_len as usize,
                )
            }
        }
    }
}

impl Clone for HitList {
    #[inline]
    fn clone(&self) -> Self {
        if self.inline_len == u8::MAX {
            Self {
                inline: [const { std::mem::MaybeUninit::uninit() }; INLINE_HIT_CAP],
                inline_len: u8::MAX,
                overflow: self.overflow.clone(),
            }
        } else {
            let mut out = Self::new();
            for &item in self.as_slice() {
                out.push(item);
            }
            out
        }
    }
}

impl std::fmt::Debug for HitList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl Default for HitList {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for HitList {
    type Target = [UnitHitDelta];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a HitList {
    type Item = &'a UnitHitDelta;
    type IntoIter = std::slice::Iter<'a, UnitHitDelta>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[derive(Debug, Clone)]
pub struct ConeFireResolution {
    pub event_id: u64,
    pub attacker_id: UnitId,
    pub source_team: Team,
    pub aim_cell: CellCoord,
    pub rounds_fired: u32,
    pub is_dud: bool,
    pub frag_radius_m: f32,
    pub blast_kpa: f32,
    pub crater_depth_m: f32,
    pub incoming_dir: [f32; 2],
    pub base_damage: [f32; 6],
    pub armor_pen_mm: f32,
    pub ignore_trench_cover: f32,
    pub hits: HitList,
    pub corridor_threat_cells: Vec<(usize, f32)>,
}

impl ConeFireResolution {
    #[inline]
    pub fn empty_slot() -> Self {
        Self {
            event_id: 0,
            attacker_id: UnitId(0),
            source_team: Team::Neutral,
            aim_cell: CellCoord::new(0, 0),
            rounds_fired: 0,
            is_dud: false,
            frag_radius_m: 0.0,
            blast_kpa: 0.0,
            crater_depth_m: 0.0,
            incoming_dir: [1.0, 0.0],
            base_damage: [0.0; 6],
            armor_pen_mm: 0.0,
            ignore_trench_cover: 0.0,
            hits: HitList::new(),
            corridor_threat_cells: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RadialBlastField {
    pub source_id: UnitId,
    pub center: WorldPos,
    pub burst_z: f32,
    pub radius_m: f32,
    pub peak_kpa: f32,
    pub fragments_total: u32,
    pub frag_armor_pen_mm: f32,
    pub frag_damage: f32,
    pub crater_depth_m: f32,
    pub det_seed: u64,
}

#[derive(Debug, Default, Clone)]
pub struct AppliedCombatTotals {
    pub casualties: u16,
    pub hp_loss: f32,
    pub suppression_add: f32,
    pub primary_target_id: Option<UnitId>,
    pub primary_dist_m: f32,
}

#[inline]
pub fn unit_muzzle_height_m(posture: SchmidtPostureState, is_vehicle: bool) -> f32 {
    if is_vehicle {
        1.9
    } else {
        match posture {
            SchmidtPostureState::Active => 1.4,
            SchmidtPostureState::Pinned => 0.9,
            SchmidtPostureState::Prone => 0.35,
        }
    }
}

#[inline]
fn splitmix_step(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

#[inline]
fn terrain_blocking_floor_slope_grid(
    terrain: &TerrainGrid,
    origin: WorldPos,
    muzzle_z: f32,
    target: WorldPos,
    dist_m: f32,
) -> f32 {
    if dist_m <= TACTICAL_CELL_SIZE_M * 0.5 {
        return f32::NEG_INFINITY;
    }
    let steps = ((dist_m - 2.0) / TACTICAL_CELL_SIZE_M).floor().max(0.0) as usize;
    if steps == 0 {
        return f32::NEG_INFINITY;
    }

    let inv_cell = 1.0 / TACTICAL_CELL_SIZE_M;
    let ox = terrain.geom.origin_m[0];
    let oy = terrain.geom.origin_m[1];
    let cols = terrain.cols;
    let rows = terrain.rows;
    let inv_dist = 1.0 / dist_m;
    let step_c = (target.x - origin.x) * inv_dist;
    let step_r = (target.y - origin.y) * inv_dist;
    let orig_c = ((origin.x - ox) * inv_cell).floor() as isize;
    let orig_r = ((origin.y - oy) * inv_cell).floor() as isize;
    let tgt_c = ((target.x - ox) * inv_cell).floor() as isize;
    let tgt_r = ((target.y - oy) * inv_cell).floor() as isize;

    let mut fx = (origin.x - ox) * inv_cell;
    let mut fy = (origin.y - oy) * inv_cell;
    let mut max_slope = f32::NEG_INFINITY;
    let mut last_idx = usize::MAX;

    for s in 1..=steps {
        fx += step_c;
        fy += step_r;
        let c = fx.floor() as isize;
        let r = fy.floor() as isize;
        if c < 0 || r < 0 || c >= cols as isize || r >= rows as isize {
            break;
        }
        if (c == orig_c && r == orig_r) || (c == tgt_c && r == tgt_r) {
            continue;
        }
        let cidx = (r as usize) * cols + (c as usize);
        if cidx == last_idx {
            continue;
        }
        last_idx = cidx;
        let crest = if terrain.kind[cidx] == TerrainKind::Urban {
            6.0
        } else {
            0.0
        };
        let d = (s as f32) * TACTICAL_CELL_SIZE_M;
        let slope = (terrain.elevation_m[cidx] + crest - muzzle_z) / d;
        if slope > max_slope {
            max_slope = slope;
        }
    }
    max_slope
}

#[inline]
fn terrain_blocking_floor_slope_fast(
    terrain: &TerrainGrid,
    derived: &DerivedState,
    origin: WorldPos,
    muzzle_z: f32,
    target: WorldPos,
    z_base: f32,
    dist_m: f32,
) -> f32 {
    if dist_m <= TACTICAL_CELL_SIZE_M * 0.5
        || muzzle_z.min(z_base) >= derived.terrain_max_crest_z
    {
        return f32::NEG_INFINITY;
    }
    terrain_blocking_floor_slope_grid(terrain, origin, muzzle_z, target, dist_m)
}

pub fn build_direct_cone_field(
    terrain: &TerrainGrid,
    catalog: &ItemCatalog,
    attacker: &UnitRecord,
    aim_pos: WorldPos,
    clock_seconds: u64,
    event_id: u64,
    enforce_ammo: bool,
    enforce_posture: bool,
) -> Option<(DirectConeField, bool, f32, f32)> {
    if attacker.fire_discipline == FireDiscipline::HoldFire
        || (enforce_ammo && attacker.ammo_rounds == 0)
    {
        return None;
    }

    let (
        combat_rpm,
        base_acc,
        _acc_decay,
        dmg_decay,
        dmg_vec,
        armor_pen_mm,
        frag_radius_m,
        ignore_trench_cover,
        is_dud,
    ) = if let Some(cp) = attacker.profile_idx.and_then(|idx| catalog.compact_profile(idx)) {
        (
            cp.combat_rpm,
            cp.base_accuracy,
            cp.acc_decay_coeff,
            cp.dmg_decay_coeff,
            cp.damage,
            cp.armor_pen_mm,
            cp.frag_radius_m,
            cp.ignore_trench_cover,
            cp.is_dud,
        )
    } else {
        let mount_refs: Vec<&str> = attacker.mount_ids.iter().map(String::as_str).collect();
        let profile_opt = if !attacker.weapon_id.is_empty() && !attacker.ammo_id.is_empty() {
            catalog
                .resolve_attack_profile(
                    &attacker.weapon_id,
                    &attacker.ammo_id,
                    attacker.fuze_id.as_deref(),
                    &mount_refs,
                )
                .ok()
        } else {
            None
        };
        match profile_opt {
            Some(p) => (
                p.combat_rpm,
                p.base_accuracy,
                p.acc_decay_coeff,
                p.dmg_decay_coeff,
                p.damage,
                p.armor_pen_mm,
                p.frag_radius_m,
                p.ignore_trench_cover,
                p.is_dud,
            ),
            None => (
                30.0,
                0.55,
                0.002,
                0.001,
                [1.0, 0.4, 0.2, 0.0, 0.0, 0.0],
                2.0,
                0.0,
                0.0,
                false,
            ),
        }
    };

    let posture_factor = if enforce_posture {
        attacker.posture.source_factor()
    } else {
        1.0
    };
    let fire_mult = match attacker.fire_discipline {
        FireDiscipline::HoldFire => 0.0,
        FireDiscipline::ReturnFire => 0.75,
        FireDiscipline::FireAtWill => 1.0,
        FireDiscipline::Suppressive => 1.4,
    };
    let squad_scale = (attacker.men as f32 / attacker.max_men.max(1) as f32).clamp(0.25, 1.0);
    let mut rounds_fired =
        ((combat_rpm / 60.0) * TICK_SECONDS_F32 * posture_factor * fire_mult * squad_scale)
            .round()
            .max(1.0) as u32;
    if enforce_ammo {
        rounds_fired = rounds_fired.min(attacker.ammo_rounds);
    }
    if rounds_fired == 0 || is_dud {
        return Some((
            DirectConeField {
                event_id,
                source_id: attacker.id,
                source_team: attacker.team,
                origin: attacker.pos,
                muzzle_z: 0.0,
                aim_pos,
                aim_dist_m: 0.0,
                max_path_m: 0.0,
                dir: [1.0, 0.0],
                tan_half_h: 0.02,
                s_cone_min: -0.02,
                s_cone_max: 0.02,
                rounds_fired,
                dmg_decay_coeff: dmg_decay,
                damage: dmg_vec,
                armor_pen_mm,
                frag_radius_m,
                ignore_trench_cover,
                suppressive_acc_mult: 1.0,
                det_seed: event_id ^ clock_seconds,
            },
            true,
            0.0,
            0.0,
        ));
    }

    let dx = aim_pos.x - attacker.pos.x;
    let dy = aim_pos.y - attacker.pos.y;
    let raw_dist = (dx * dx + dy * dy).sqrt();
    let dir = if raw_dist > 1e-3 {
        [dx / raw_dist, dy / raw_dist]
    } else {
        let rad = attacker.facing_deg.to_radians();
        [rad.cos(), rad.sin()]
    };
    let aim_dist_m = raw_dist.max(8.0);
    let max_path_m = (aim_dist_m * 1.45).clamp(240.0, 960.0);

    let origin_elev = terrain.elevation_at(attacker.pos).unwrap_or(0.0);
    let muzzle_z = origin_elev + unit_muzzle_height_m(attacker.posture, attacker.is_vehicle);
    let aim_elev = terrain.elevation_at(aim_pos).unwrap_or(origin_elev);
    let s_aim = ((aim_elev + 0.90) - muzzle_z) / aim_dist_m;

    let disc_spread = if attacker.fire_discipline == FireDiscipline::Suppressive {
        1.45
    } else {
        1.0
    };
    let suppressive_acc_mult = if attacker.fire_discipline == FireDiscipline::Suppressive {
        0.75
    } else {
        1.0
    };
    let inaccuracy = (1.0 - base_acc).clamp(0.06, 0.70);
    let tan_half_h = (inaccuracy * 0.14 * disc_spread).clamp(0.018, 0.22);
    let s_half_v = (inaccuracy * 0.08 * disc_spread).clamp(0.020, 0.09);

    let blast_kpa = if frag_radius_m > 0.0 {
        (dmg_vec[4].max(10.0) * 2.5).clamp(20.0, 300.0)
    } else {
        0.0
    };
    let crater_depth_m = if dmg_vec[4] >= 30.0 {
        (dmg_vec[4] * 0.025).clamp(0.2, 2.5)
    } else {
        0.0
    };

    let det_seed = splitmix_step(
        clock_seconds
            ^ ((attacker.id.0 as u64) << 33)
            ^ ((aim_pos.x.to_bits() as u64) << 16)
            ^ (aim_pos.y.to_bits() as u64),
    );
    Some((
        DirectConeField {
            event_id,
            source_id: attacker.id,
            source_team: attacker.team,
            origin: attacker.pos,
            muzzle_z,
            aim_pos,
            aim_dist_m,
            max_path_m,
            dir,
            tan_half_h,
            s_cone_min: s_aim - s_half_v,
            s_cone_max: s_aim + s_half_v,
            rounds_fired,
            dmg_decay_coeff: dmg_decay,
            damage: dmg_vec,
            armor_pen_mm,
            frag_radius_m,
            ignore_trench_cover,
            suppressive_acc_mult,
            det_seed,
        },
        false,
        blast_kpa,
        crater_depth_m,
    ))
}

pub fn resolve_direct_cone_readonly(
    terrain: &TerrainGrid,
    derived: &DerivedState,
    field: &DirectConeField,
    is_dud: bool,
    blast_kpa: f32,
    crater_depth_m: f32,
    collect_threat_cells: bool,
    scratch_candidates: &mut Vec<CorridorCandidate>,
) -> ConeFireResolution {
    let mut out = ConeFireResolution::empty_slot();
    resolve_direct_cone_readonly_into(
        terrain,
        derived,
        field,
        is_dud,
        blast_kpa,
        crater_depth_m,
        collect_threat_cells,
        scratch_candidates,
        &mut out,
    );
    out
}

pub fn resolve_direct_cone_readonly_into(
    terrain: &TerrainGrid,
    derived: &DerivedState,
    field: &DirectConeField,
    is_dud: bool,
    blast_kpa: f32,
    crater_depth_m: f32,
    collect_threat_cells: bool,
    scratch_candidates: &mut Vec<CorridorCandidate>,
    out: &mut ConeFireResolution,
) {
    let aim_cell = terrain
        .geom
        .world_to_cell(field.aim_pos)
        .unwrap_or_else(|_| CellCoord::new(0, 0));

    out.event_id = field.event_id;
    out.attacker_id = field.source_id;
    out.source_team = field.source_team;
    out.aim_cell = aim_cell;
    out.rounds_fired = field.rounds_fired;
    out.incoming_dir = field.dir;
    out.base_damage = field.damage;
    out.armor_pen_mm = field.armor_pen_mm;
    out.ignore_trench_cover = field.ignore_trench_cover;
    out.hits.clear();
    out.corridor_threat_cells.clear();

    if is_dud || field.rounds_fired == 0 || field.max_path_m <= 1.0 {
        out.is_dud = is_dud;
        out.frag_radius_m = 0.0;
        out.blast_kpa = 0.0;
        out.crater_depth_m = 0.0;
        return;
    }

    out.is_dud = false;
    out.frag_radius_m = field.frag_radius_m;
    out.blast_kpa = blast_kpa;
    out.crater_depth_m = crater_depth_m;

    scratch_candidates.clear();
    let end_x = field.origin.x + field.dir[0] * field.max_path_m;
    let end_y = field.origin.y + field.dir[1] * field.max_path_m;
    let pad = field.max_path_m * field.tan_half_h + 8.0;
    let min_x = field.origin.x.min(end_x) - pad;
    let max_x = field.origin.x.max(end_x) + pad;
    let min_y = field.origin.y.min(end_y) - pad;
    let max_y = field.origin.y.max(end_y) + pad;

    let inv_bucket_m = 1.0 / ((derived.bucket_cells as f32) * TACTICAL_CELL_SIZE_M);
    let ox = terrain.geom.origin_m[0];
    let oy = terrain.geom.origin_m[1];
    if !derived.spatial_bucket_proxies.is_empty() {
        let min_bc = (((min_x - ox) * inv_bucket_m).floor() as isize)
            .clamp(0, derived.bucket_cols as isize - 1) as usize;
        let max_bc = (((max_x - ox) * inv_bucket_m).floor() as isize)
            .clamp(0, derived.bucket_cols as isize - 1) as usize;
        let min_br = (((min_y - oy) * inv_bucket_m).floor() as isize)
            .clamp(0, derived.bucket_rows as isize - 1) as usize;
        let max_br = (((max_y - oy) * inv_bucket_m).floor() as isize)
            .clamp(0, derived.bucket_rows as isize - 1) as usize;

        let vx = -field.dir[1];
        let vy = field.dir[0];
        for br in min_br..=max_br {
            for bc in min_bc..=max_bc {
                let bidx = br * derived.bucket_cols + bc;
                for (slot_idx, proxy) in derived.spatial_bucket_proxies[bidx].iter().enumerate() {
                    if proxy.id == field.source_id {
                        continue;
                    }
                    let rx = proxy.pos.x - field.origin.x;
                    let ry = proxy.pos.y - field.origin.y;
                    let dist_along = rx * field.dir[0] + ry * field.dir[1];
                    if dist_along <= 1.0 || dist_along > field.max_path_m {
                        continue;
                    }
                    let lat_v = rx * vx + ry * vy;
                    let max_lat = dist_along * field.tan_half_h + proxy.half_width_m;
                    if lat_v.abs() > max_lat {
                        continue;
                    }
                    let radial_r_sq = rx * rx + ry * ry;
                    scratch_candidates.push(CorridorCandidate {
                        bucket_idx: bidx as u32,
                        slot_idx: slot_idx as u32,
                        unit_id: proxy.id.0,
                        dist_along,
                        lat_v,
                        radial_r_sq,
                    });
                }
            }
        }
    }

    // Strict front-to-back distance queue: r_1 <= r_2 <= ... <= r_k.
    scratch_candidates.sort_unstable_by(|a, b| {
        a.radial_r_sq
            .to_bits()
            .cmp(&b.radial_r_sq.to_bits())
            .then_with(|| a.unit_id.cmp(&b.unit_id))
    });

    // Stack-only 8x4 = 32 micro-beamlets conserving total flux = 1.0 (ZERO heap allocations, ZERO atan2!).
    let mut beam_flux = [1.0f32 / (CONE_RAYS as f32); CONE_RAYS];
    let mut total_rem_flux = 1.0f32;
    let h_span = 2.0 * field.tan_half_h;
    let h_step = h_span / (CONE_H_BINS as f32);
    let inv_h_step = (CONE_H_BINS as f32) / h_span;
    let v_span = (field.s_cone_max - field.s_cone_min).max(1e-4);
    let v_step = v_span / (CONE_V_BINS as f32);
    let inv_v_step = (CONE_V_BINS as f32) / v_span;
    let mut v_overhead = [false; CONE_V_BINS];
    for (v, slot) in v_overhead.iter_mut().enumerate() {
        let vb_mid = field.s_cone_min + ((v as f32) + 0.5) * v_step;
        *slot = vb_mid > 0.03;
    }

    for (cand_idx, cand) in scratch_candidates.iter().enumerate() {
        if total_rem_flux <= 1e-6 {
            break;
        }
        let p = &derived.spatial_bucket_proxies[cand.bucket_idx as usize][cand.slot_idx as usize];
        let inv_r = 1.0 / cand.dist_along.max(1.0);
        let u_lo = (cand.lat_v - p.half_width_m) * inv_r;
        let u_hi = (cand.lat_v + p.half_width_m) * inv_r;
        if u_hi <= -field.tan_half_h || u_lo >= field.tan_half_h {
            continue;
        }
        let h_start = (((u_lo + field.tan_half_h) * inv_h_step).floor() as isize)
            .clamp(0, (CONE_H_BINS - 1) as isize) as usize;
        let h_end = (((u_hi + field.tan_half_h) * inv_h_step).floor() as isize)
            .clamp(0, (CONE_H_BINS - 1) as isize) as usize;

        let s_low_raw = (p.z_base - field.muzzle_z) * inv_r;
        let s_high_raw = (p.z_base + p.h_posture - field.muzzle_z) * inv_r;
        if s_high_raw <= field.s_cone_min || s_low_raw >= field.s_cone_max {
            continue;
        }
        let v_end = (((s_high_raw - field.s_cone_min) * inv_v_step).floor() as isize)
            .clamp(0, (CONE_V_BINS - 1) as isize) as usize;
        let v_start_raw = (((s_low_raw - field.s_cone_min) * inv_v_step).floor() as isize)
            .clamp(0, (CONE_V_BINS - 1) as isize) as usize;

        let terrain_floor_slope = if field.muzzle_z.min(p.z_base) >= derived.terrain_max_crest_z {
            f32::NEG_INFINITY
        } else {
            let mut any_flux = false;
            for h in h_start..=h_end {
                let base = h * CONE_V_BINS;
                for v in v_start_raw..=v_end {
                    if beam_flux[base + v] > 1e-7 {
                        any_flux = true;
                        break;
                    }
                }
                if any_flux {
                    break;
                }
            }
            if !any_flux {
                continue;
            }
            let radial_r = cand.radial_r_sq.sqrt();
            terrain_blocking_floor_slope_fast(
                terrain,
                derived,
                field.origin,
                field.muzzle_z,
                p.pos,
                p.z_base,
                radial_r,
            )
        };
        let eff_s_low = s_low_raw.max(terrain_floor_slope);
        if eff_s_low >= s_high_raw || eff_s_low >= field.s_cone_max {
            // Completely shielded by intermediate mountain ridge or urban wall.
            continue;
        }
        let v_start = if eff_s_low <= s_low_raw {
            v_start_raw
        } else {
            (((eff_s_low - field.s_cone_min) * inv_v_step).floor() as isize)
                .clamp(0, (CONE_V_BINS - 1) as isize) as usize
        };

        let mut fv_arr = [1.0f32; CONE_V_BINS];
        {
            let vb0 = field.s_cone_min + (v_start as f32) * v_step;
            let vb1 = vb0 + v_step;
            fv_arr[v_start] =
                ((vb1.min(s_high_raw) - vb0.max(eff_s_low)) * inv_v_step).clamp(0.0, 1.0);
        }
        if v_end > v_start {
            let vb0 = field.s_cone_min + (v_end as f32) * v_step;
            let vb1 = vb0 + v_step;
            fv_arr[v_end] =
                ((vb1.min(s_high_raw) - vb0.max(eff_s_low)) * inv_v_step).clamp(0.0, 1.0);
        }

        let mut absorbed_fraction = 0.0f32;
        let mut overhead_weighted_flux = 0.0f32;

        for h in h_start..=h_end {
            let fh = if h > h_start && h < h_end {
                1.0
            } else {
                let hb0 = -field.tan_half_h + (h as f32) * h_step;
                let hb1 = hb0 + h_step;
                ((hb1.min(u_hi) - hb0.max(u_lo)) * inv_h_step).clamp(0.0, 1.0)
            };
            if fh <= 0.0 {
                continue;
            }
            let fh_occ = fh * p.occlusion_fraction;
            let base = h * CONE_V_BINS;
            for v in v_start..=v_end {
                let fv = fv_arr[v];
                if fv <= 0.0 {
                    continue;
                }
                let cell_idx = base + v;
                let rem_flux = beam_flux[cell_idx];
                if rem_flux <= 1e-7 {
                    continue;
                }
                // Solid vehicles/ridges deduct 100% of covered window flux permanently (zero overpenetration!).
                // Porous infantry squads absorb proportional to live soldier projection and let gap flux pass!
                let deduct = (fh_occ * fv).min(1.0);
                let taken = rem_flux * deduct;
                beam_flux[cell_idx] = (rem_flux - taken).max(0.0);
                absorbed_fraction += taken;
                if v_overhead[v] {
                    overhead_weighted_flux += taken;
                }
            }
        }
        total_rem_flux -= absorbed_fraction;

        if absorbed_fraction <= 1e-6 || !p.team.is_hostile_to(field.source_team) {
            // Friendly / neutral / zero-flux absorbers already consumed path beam_flux when solid/porous,
            // preventing bullets from passing through obstacles without dealing friendly fire here.
            continue;
        }

        let d = cand.radial_r_sq.sqrt().max(0.0);
        let dmg_mult = 1.0 / (1.0 + field.dmg_decay_coeff.max(0.0) * d);
        let rounds_absorbed = (field.rounds_fired as f32) * absorbed_fraction * field.suppressive_acc_mult;
        let supp_channel = (field.damage[1] * dmg_mult).max(field.damage[0] * dmg_mult * 0.60);
        let suppression_add = (supp_channel
            * (rounds_absorbed.max(0.1)).sqrt()
            * 0.19)
            .clamp(0.0, 3.0);

        out.hits.push(UnitHitDelta {
            target_id: p.id,
            dist_m: d,
            is_overhead: overhead_weighted_flux > absorbed_fraction * 0.65,
            absorbed_fraction,
            rounds_absorbed,
            dmg_mult,
            suppression_add,
            det_seed: splitmix_step(field.det_seed ^ (cand_idx as u64 + 1) ^ ((p.id.0 as u64) << 11)),
        });
    }

    // Real-time I-field directional threat corridor propagation (zeroes out past solid ridges or tanks!).
    if collect_threat_cells {
        let step_m = TACTICAL_CELL_SIZE_M;
        let steps = (field.max_path_m / step_m).floor() as usize;
        let mut center_blocked_dist = field.max_path_m;
        for cand in scratch_candidates.iter() {
            let p = &derived.spatial_bucket_proxies[cand.bucket_idx as usize][cand.slot_idx as usize];
            if p.is_vehicle && cand.lat_v.abs() <= p.half_width_m + 1.5 {
                center_blocked_dist = center_blocked_dist.min(cand.dist_along);
                break;
            }
        }
        let mut last_idx = usize::MAX;
        for s in 1..=steps {
            let d = (s as f32) * step_m;
            if d > center_blocked_dist {
                break;
            }
            let p = WorldPos::new(field.origin.x + field.dir[0] * d, field.origin.y + field.dir[1] * d);
            let Ok(cell) = terrain.geom.world_to_cell(p) else {
                break;
            };
            let Ok(cidx) = terrain.idx(cell) else {
                break;
            };
            let crest = if terrain.kind[cidx] == TerrainKind::Urban { 6.0 } else { 0.0 };
            let ridge_slope = (terrain.elevation_m[cidx] + crest - field.muzzle_z) / d;
            if ridge_slope > field.s_cone_max {
                break;
            }
            if cidx != last_idx {
                let density = ((field.rounds_fired as f32) / 120.0) / (1.0 + d * 0.012);
                out.corridor_threat_cells.push((cidx, density.clamp(0.05, 2.5)));
                last_idx = cidx;
            }
        }
    }
}

std::thread_local! {
    static THREAD_CANDIDATE_SCRATCH: std::cell::RefCell<Vec<CorridorCandidate>> =
        std::cell::RefCell::new(Vec::with_capacity(64));
}

/// Executes a batch of direct-fire cone fields cleanly in parallel into a reusable buffer.
pub fn resolve_direct_cone_batch_into(
    terrain: &TerrainGrid,
    derived: &DerivedState,
    jobs: &[(DirectConeField, bool, f32, f32)],
    collect_threat_cells: bool,
    out: &mut Vec<ConeFireResolution>,
) {
    out.resize_with(jobs.len(), ConeFireResolution::empty_slot);
    if jobs.len() >= 16 {
        jobs.par_chunks(64)
            .zip(out.par_chunks_mut(64))
            .for_each(|(job_chunk, out_chunk)| {
                THREAD_CANDIDATE_SCRATCH.with(|cell| {
                    let mut buf = cell.borrow_mut();
                    for ((field, is_dud, kpa, crater), slot) in
                        job_chunk.iter().zip(out_chunk.iter_mut())
                    {
                        resolve_direct_cone_readonly_into(
                            terrain,
                            derived,
                            field,
                            *is_dud,
                            *kpa,
                            *crater,
                            collect_threat_cells,
                            &mut buf,
                            slot,
                        );
                    }
                });
            });
    } else {
        let mut buf = Vec::with_capacity(32);
        for ((field, is_dud, kpa, crater), slot) in jobs.iter().zip(out.iter_mut()) {
            resolve_direct_cone_readonly_into(
                terrain,
                derived,
                field,
                *is_dud,
                *kpa,
                *crater,
                collect_threat_cells,
                &mut buf,
                slot,
            );
        }
    }
    if !jobs.windows(2).all(|w| w[0].0.event_id <= w[1].0.event_id) {
        out.sort_unstable_by_key(|r| r.event_id);
    }
}

/// Executes a batch of direct-fire cone fields cleanly in parallel with deterministic sorted output.
pub fn resolve_direct_cone_batch_parallel(
    terrain: &TerrainGrid,
    derived: &DerivedState,
    jobs: &[(DirectConeField, bool, f32, f32)],
    collect_threat_cells: bool,
) -> Vec<ConeFireResolution> {
    let mut results = Vec::new();
    resolve_direct_cone_batch_into(terrain, derived, jobs, collect_threat_cells, &mut results);
    results
}

/// Applies one unit's absorbed Eulerian hit delta onto either Tank 4-facing RHA+8 subsystems
/// (zero overpenetration) or Per-Soldier 4-region records.
pub fn apply_hit_delta_to_unit(
    unit: &mut UnitRecord,
    target_cover: f32,
    enforce_posture: bool,
    incoming_dir: [f32; 2],
    base_damage: [f32; 6],
    armor_pen_mm: f32,
    ignore_trench_cover: f32,
    hit: &UnitHitDelta,
) -> (u16, f32, f32, u16) {
    if !unit.alive {
        return (0, 0.0, 0.0, 0);
    }

    let eff_cover = (target_cover * (1.0 - ignore_trench_cover)).clamp(0.0, 0.85);
    let supp_delta = (hit.suppression_add * (1.0 - eff_cover * 0.4)).clamp(0.0, 3.0);

    if unit.is_vehicle {
        let facing = select_armor_facing(incoming_dir, unit.facing_deg, hit.is_overhead);
        let round_kinetic = (base_damage[3] * hit.dmg_mult)
            .max(base_damage[2] * hit.dmg_mult * 0.5)
            .max(base_damage[4] * hit.dmg_mult * 0.4)
            .max(0.5);
        let impact_energy = hit.rounds_absorbed * 0.28 * round_kinetic;
        let prev_hp = unit.hp;
        let mut crew_killed = 0u16;
        if let Some(vd) = &mut unit.vehicle_damage {
            let outcome = vd.apply_ballistic_impact(
                facing,
                armor_pen_mm,
                impact_energy,
                hit.det_seed,
            );
            crew_killed = outcome.crew_killed;
            unit.hp = (unit.hp - outcome.hull_hp_loss).max(0.0);
        } else {
            unit.hp = (unit.hp - impact_energy * 0.3).max(0.0);
        }
        unit.suppression += supp_delta * 0.65;
        if enforce_posture {
            unit.posture = unit.posture.transition(unit.suppression);
        }
        unit.recompute_damage_aggregates();
        let hp_loss = (prev_hp - unit.hp).max(0.0);
        return (crew_killed, hp_loss, supp_delta * 0.65, crew_killed);
    }

    // Infantry: strict per-soldier 4-region accounting.
    let posture_def = if enforce_posture {
        match unit.posture {
            SchmidtPostureState::Active => 1.0,
            SchmidtPostureState::Pinned => 0.72,
            SchmidtPostureState::Prone => 0.45,
        }
    } else {
        1.0
    };
    let eff_hits = hit.rounds_absorbed * (1.0 - eff_cover) * posture_def * 0.24;
    let per_round_ap = ((base_damage[0] + 0.25 * base_damage[2] + 0.35 * base_damage[5])
        * hit.dmg_mult)
        .max(0.5);

    let prev_hp = unit.hp;
    let prev_men = unit.men;
    let mut corpses_spawned: u16 = 0;

    if !unit.soldiers.is_empty() && eff_hits > 0.01 {
        let mut total_energy = eff_hits * per_round_ap;
        let alive_count = unit.soldiers.iter().filter(|s| s.alive).count();
        if alive_count > 0 {
            let strikes = (eff_hits.ceil() as usize).clamp(1, alive_count.min(4));
            let dmg_per_strike = total_energy / (strikes as f32);
            let mut rng = hit.det_seed;
            for _ in 0..strikes {
                if total_energy <= 0.0 {
                    break;
                }
                rng = splitmix_step(rng);
                let live_indices: [usize; 32] = {
                    let mut arr = [0usize; 32];
                    let mut n = 0usize;
                    for (idx, s) in unit.soldiers.iter().enumerate() {
                        if s.alive && n < 32 {
                            arr[n] = idx;
                            n += 1;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                    let pick = arr[(rng as usize) % n];
                    let part = SoldierRecord::select_hit_part(rng >> 12);
                    let res = unit.soldiers[pick].apply_hit(part, dmg_per_strike, supp_delta * 0.3);
                    if res.killed {
                        corpses_spawned = corpses_spawned.saturating_add(1);
                    }
                    total_energy -= dmg_per_strike;
                    arr
                };
                let _ = live_indices;
            }
            unit.recompute_damage_aggregates();
        }
    } else {
        let hp_loss = (eff_hits * per_round_ap).clamp(0.0, unit.hp);
        unit.hp = (unit.hp - hp_loss).max(0.0);
        if unit.hp <= 0.0 {
            unit.alive = false;
            unit.men = 0;
        }
    }

    unit.suppression += supp_delta;
    if enforce_posture {
        unit.posture = unit.posture.transition(unit.suppression);
    }
    let hp_loss = (prev_hp - unit.hp).max(0.0);
    let casualties = prev_men.saturating_sub(unit.men);
    (casualties, hp_loss, supp_delta, corpses_spawned)
}

/// Resolves a point-of-detonation RadialBlastField with terrain shelter, front-to-back tank/solid occlusion,
/// zero overpenetration on armor, per-soldier fragment damage, and I-field hazard feedback.
pub fn resolve_and_commit_radial_blast(
    world: &mut WorldState,
    tick: u64,
    blast: &RadialBlastField,
) -> AppliedCombatTotals {
    let mut totals = AppliedCombatTotals::default();
    let Ok(center_cell) = world.terrain.geom.world_to_cell(blast.center) else {
        return totals;
    };

    if blast.crater_depth_m > 0.0 {
        let _ = world.terrain.mutate_cell(
            center_cell,
            Some(-blast.crater_depth_m.abs()),
            None,
            None,
            Some(0.60),
        );
    }

    let r_cells = world
        .terrain
        .geom
        .kernel_radius_to_cell_support(blast.radius_m);
    for dr in -r_cells..=r_cells {
        for dc in -r_cells..=r_cells {
            let cell = CellCoord::new(center_cell.r + dr, center_cell.c + dc);
            let Ok(cidx) = world.terrain.idx(cell) else {
                continue;
            };
            let d = world
                .terrain
                .geom
                .cell_distance_m(center_cell, cell)
                .unwrap_or(f32::MAX);
            if d > blast.radius_m {
                continue;
            }
            let atten = (1.0 - d / blast.radius_m.max(1.0)).max(0.0);
            let center_p = world.terrain.geom.cell_center_m(cell).unwrap_or(blast.center);
            let ridge_slope = terrain_blocking_floor_slope_grid(
                &world.terrain,
                blast.center,
                blast.burst_z,
                center_p,
                d,
            );
            let target_top_slope = (world.terrain.elevation_m[cidx] + 2.0 - blast.burst_z) / d.max(4.0);
            if ridge_slope > target_top_slope && d > TACTICAL_CELL_SIZE_M {
                continue;
            }
            if cidx < world.cell_h_blast.len() {
                world.cell_h_blast[cidx] += blast.peak_kpa * atten;
            }
            if cidx < world.cell_s_supp.len() {
                world.cell_s_supp[cidx] += 1.5 * atten;
            }
            world.inject_team_intel(Team::Red, cidx, 0.35 * atten);
            world.inject_team_intel(Team::Blue, cidx, 0.35 * atten);
        }
    }

    // Collect units in blast radius & sort front-to-back!
    let mut blast_targets: Vec<(f32, u32, WorldPos, bool)> = world
        .units
        .values()
        .filter(|u| u.alive)
        .filter_map(|u| {
            let d = u.pos.distance_to(blast.center);
            (d <= blast.radius_m).then_some((d, u.id.0, u.pos, u.is_vehicle))
        })
        .collect();
    blast_targets.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut az_solid_blocked = [false; BLAST_AZ_BINS];
    let mut spawn_corpses: Vec<(UnitId, Team, WorldPos, u16)> = Vec::new();
    let enforce_posture = world.ruleset.enforce_suppression_posture;

    for (d, uid, pos, is_veh) in blast_targets {
        let ridge_slope = terrain_blocking_floor_slope_grid(
            &world.terrain,
            blast.center,
            blast.burst_z,
            pos,
            d,
        );
        let z_t = world.terrain.elevation_at(pos).unwrap_or(0.0);
        let h_t = if is_veh { 2.5 } else { 1.4 };
        let top_slope = (z_t + h_t - blast.burst_z) / d.max(2.0);
        if ridge_slope > top_slope && d > TACTICAL_CELL_SIZE_M {
            continue;
        }

        let dx = pos.x - blast.center.x;
        let dy = pos.y - blast.center.y;
        let az_bin = if d > 0.5 {
            let ang = dy.atan2(dx) + std::f32::consts::PI;
            let b = ((ang / (2.0 * std::f32::consts::PI)) * (BLAST_AZ_BINS as f32)) as usize;
            b.min(BLAST_AZ_BINS - 1)
        } else {
            0
        };
        if d > 2.0 && az_solid_blocked[az_bin] {
            // Solid tank strictly in front shields rear unit along that blast sector!
            continue;
        }
        if is_veh && d > 1.0 {
            az_solid_blocked[az_bin] = true;
        }

        let atten = (1.0 - d / blast.radius_m.max(1.0)).clamp(0.05, 1.0);
        let seed = splitmix_step(blast.det_seed ^ (uid as u64));
        let incoming_dir = if d > 1e-3 {
            [dx / d, dy / d]
        } else {
            [1.0, 0.0]
        };
        let frag_energy = (blast.fragments_total as f32 * 0.08 * atten).max(2.0) * blast.frag_damage;
        let blast_dmg = [
            blast.frag_damage * atten,
            1.8 * atten,
            blast.frag_damage * 0.5 * atten,
            blast.frag_damage * 0.8 * atten,
            blast.peak_kpa * atten,
            0.0,
        ];
        let unit_hit = UnitHitDelta {
            target_id: UnitId(uid),
            dist_m: d,
            is_overhead: d <= 4.0,
            absorbed_fraction: atten,
            rounds_absorbed: (frag_energy / 3.0).max(1.0),
            dmg_mult: 1.0,
            suppression_add: 1.4 * atten,
            det_seed: seed,
        };
        let target_cell = world.terrain.geom.world_to_cell(pos).ok();
        let target_cover = target_cell
            .and_then(|c| world.terrain.idx(c).ok())
            .map(|i| world.terrain.kind[i].cover_factor() + world.terrain.cover_bonus[i])
            .unwrap_or(0.0);

        if let Some(u) = world.units.get_mut(&uid) {
            let team = u.team;
            let (cas, hp_loss, supp, corpses) = apply_hit_delta_to_unit(
                u,
                target_cover,
                enforce_posture,
                incoming_dir,
                blast_dmg,
                blast.frag_armor_pen_mm,
                0.75,
                &unit_hit,
            );
            totals.casualties = totals.casualties.saturating_add(cas);
            totals.hp_loss += hp_loss;
            totals.suppression_add += supp;
            if totals.primary_target_id.is_none() {
                totals.primary_target_id = Some(UnitId(uid));
                totals.primary_dist_m = d;
            }
            if corpses > 0 {
                spawn_corpses.push((UnitId(uid), team, pos, corpses));
            }
        }
    }

    for (uid, team, pos, cnt) in spawn_corpses {
        for i in 0..cnt {
            let _ = world.create_world_instance(
                WorldInstanceKind::Corpse,
                team,
                Some(uid),
                pos,
                format!("corpse_blast_{tick}_{}_{}", uid.0, i),
                0.0,
                0.0,
                0.0,
            );
        }
    }
    totals
}
