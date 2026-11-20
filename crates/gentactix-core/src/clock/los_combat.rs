use crate::catalog::ItemCatalog;
use crate::math_ops::{CellCoord, SchmidtPostureState, WorldPos, TACTICAL_CELL_SIZE_M};
use crate::ruleset::SimRuleset;
use crate::state::UnitRecord;
use crate::terrain::{TerrainGrid, TerrainKind};
use crate::time_machine::mix_time_unit_hash;
use crate::types::{FireDiscipline, TICK_SECONDS_F32};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LosResult {
    pub visible: bool,
    pub dist_m: f32,
    pub concealment: f32,
    pub blocked_by_terrain: bool,
    pub blocked_by_smoke: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FireOutcome {
    pub rounds_fired: u32,
    pub casualties: u16,
    pub hp_loss: f32,
    pub suppression_add: f32,
    pub frag_radius_m: f32,
    pub blast_kpa: f32,
    pub crater_depth_m: f32,
    pub hit_prob: f32,
}

/// Full 8m Amanatides & Woo grid DDA line-of-sight check (no legacy cell cutoff).
pub fn check_los(
    terrain: &TerrainGrid,
    from: WorldPos,
    to: WorldPos,
    max_vision_m: f32,
    night: bool,
) -> LosResult {
    let dist_m = from.distance_to(to);
    let effective_vision_m = max_vision_m * if night { 0.45 } else { 1.0 };
    if dist_m > effective_vision_m {
        return LosResult {
            visible: false,
            dist_m,
            concealment: 0.0,
            blocked_by_terrain: false,
            blocked_by_smoke: false,
        };
    }

    let Ok(start_cell) = terrain.geom.world_to_cell(from) else {
        return LosResult {
            visible: false,
            dist_m,
            concealment: 0.0,
            blocked_by_terrain: true,
            blocked_by_smoke: false,
        };
    };
    let Ok(end_cell) = terrain.geom.world_to_cell(to) else {
        return LosResult {
            visible: false,
            dist_m,
            concealment: 0.0,
            blocked_by_terrain: true,
            blocked_by_smoke: false,
        };
    };

    let start_idx = terrain.idx(start_cell).unwrap_or(0);
    let end_idx = terrain.idx(end_cell).unwrap_or(0);
    if start_cell == end_cell {
        let smoke = terrain.smoke_density[start_idx];
        let smoke_blocked = smoke * 1.2 >= 1.0;
        return LosResult {
            visible: !smoke_blocked,
            dist_m,
            concealment: (terrain.kind[start_idx].los_opacity() * 0.45 + smoke).clamp(0.0, 1.0),
            blocked_by_terrain: false,
            blocked_by_smoke: smoke_blocked,
        };
    }

    let h_start = terrain.elevation_m[start_idx] + 1.8;
    let h_end = terrain.elevation_m[end_idx] + 1.5;

    let cell_m = TACTICAL_CELL_SIZE_M;
    let sx = (from.x - terrain.geom.origin_m[0]) / cell_m;
    let sy = (from.y - terrain.geom.origin_m[1]) / cell_m;
    let ex = (to.x - terrain.geom.origin_m[0]) / cell_m;
    let ey = (to.y - terrain.geom.origin_m[1]) / cell_m;

    let dx = ex - sx;
    let dy = ey - sy;

    let mut c = start_cell.c;
    let mut r = start_cell.r;
    let step_c = if dx > 0.0 { 1 } else if dx < 0.0 { -1 } else { 0 };
    let step_r = if dy > 0.0 { 1 } else if dy < 0.0 { -1 } else { 0 };

    let t_delta_x = if dx.abs() > 1e-6 { 1.0 / dx.abs() } else { f32::INFINITY };
    let t_delta_y = if dy.abs() > 1e-6 { 1.0 / dy.abs() } else { f32::INFINITY };

    let mut t_max_x = if step_c > 0 {
        ((c + 1) as f32 - sx) * t_delta_x
    } else if step_c < 0 {
        (sx - c as f32) * t_delta_x
    } else {
        f32::INFINITY
    };

    let mut t_max_y = if step_r > 0 {
        ((r + 1) as f32 - sy) * t_delta_y
    } else if step_r < 0 {
        (sy - r as f32) * t_delta_y
    } else {
        f32::INFINITY
    };

    let mut opacity_acc = 0.0f32;
    let mut smoke_acc = 0.0f32;
    let max_steps = (terrain.rows + terrain.cols) * 2 + 4;

    for _ in 0..max_steps {
        let t_enter = t_max_x.min(t_max_y).clamp(0.0, 1.0);
        if t_max_x < t_max_y {
            c += step_c;
            t_max_x += t_delta_x;
        } else if t_max_y < t_max_x {
            r += step_r;
            t_max_y += t_delta_y;
        } else {
            c += step_c;
            r += step_r;
            t_max_x += t_delta_x;
            t_max_y += t_delta_y;
        }

        let cur = CellCoord::new(r, c);
        let Ok(idx) = terrain.idx(cur) else {
            break;
        };

        let is_end = cur == end_cell;
        if !is_end {
            let obstacle_crest = if terrain.kind[idx] == TerrainKind::Urban {
                6.0
            } else {
                0.0
            };
            let cell_h = terrain.elevation_m[idx] + obstacle_crest;
            let chord_h = h_start + (h_end - h_start) * t_enter;
            if cell_h > chord_h {
                return LosResult {
                    visible: false,
                    dist_m,
                    concealment: opacity_acc.clamp(0.0, 1.0),
                    blocked_by_terrain: true,
                    blocked_by_smoke: false,
                };
            }
            opacity_acc += terrain.kind[idx].los_opacity() * 0.45;
        }

        let smoke = terrain.smoke_density[idx];
        if smoke > 0.0 {
            smoke_acc += smoke * 1.2;
            opacity_acc += smoke * 1.2;
        }

        if opacity_acc >= 1.0 {
            return LosResult {
                visible: false,
                dist_m,
                concealment: 1.0,
                blocked_by_terrain: false,
                blocked_by_smoke: smoke_acc >= 0.35,
            };
        }

        if is_end {
            break;
        }
    }

    LosResult {
        visible: true,
        dist_m,
        concealment: opacity_acc.clamp(0.0, 0.99),
        blocked_by_terrain: false,
        blocked_by_smoke: false,
    }
}

pub fn resolve_fire_exchange(
    clock_seconds: u64,
    ruleset: &SimRuleset,
    catalog: &ItemCatalog,
    attacker: &UnitRecord,
    target: &UnitRecord,
    los: &LosResult,
    target_cell_cover: f32,
) -> FireOutcome {
    if attacker.fire_discipline == FireDiscipline::HoldFire
        || (ruleset.enforce_ammo_consumption && attacker.ammo_rounds == 0)
    {
        return zero_fire_outcome();
    }

    let (
        combat_rpm,
        base_acc,
        acc_decay,
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

    let posture_factor = if ruleset.enforce_suppression_posture {
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
    if ruleset.enforce_ammo_consumption {
        rounds_fired = rounds_fired.min(attacker.ammo_rounds);
    }
    if rounds_fired == 0 || is_dud {
        return FireOutcome {
            rounds_fired,
            casualties: 0,
            hp_loss: 0.0,
            suppression_add: 0.0,
            frag_radius_m: 0.0,
            blast_kpa: 0.0,
            crater_depth_m: 0.0,
            hit_prob: 0.0,
        };
    }

    let d = los.dist_m.max(0.0);
    let acc_at_d = (base_acc / (1.0 + acc_decay.max(0.0) * d)).clamp(0.0, 1.0);
    let dmg_mult = 1.0 / (1.0 + dmg_decay.max(0.0) * d);
    let acc_disc = if attacker.fire_discipline == FireDiscipline::Suppressive {
        0.70
    } else {
        1.0
    };
    let effective_cover = (target_cell_cover * (1.0 - ignore_trench_cover)).clamp(0.0, 0.85);
    let posture_def = if ruleset.enforce_suppression_posture {
        match target.posture {
            SchmidtPostureState::Active => 1.0,
            SchmidtPostureState::Pinned => 0.75,
            SchmidtPostureState::Prone => 0.50,
        }
    } else {
        1.0
    };
    let hit_prob = (acc_at_d
        * acc_disc
        * (1.0 - los.concealment * 0.5)
        * (1.0 - effective_cover)
        * posture_def)
        .clamp(0.01, 0.95);

    let per_round_dmg = if target.armor_mm > 0.0 {
        let pen_ratio = (armor_pen_mm / target.armor_mm.max(1.0)).clamp(0.0, 1.25);
        dmg_vec[3] * dmg_mult * pen_ratio
    } else {
        (dmg_vec[0] + 0.25 * dmg_vec[2] + 0.35 * dmg_vec[5]) * dmg_mult
    };

    let expected_hits = (rounds_fired as f32) * hit_prob * 0.16;
    let raw_hp_loss = (per_round_dmg * expected_hits).clamp(0.0, target.hp);

    let supp_channel = dmg_vec[1].max(dmg_vec[0] * 0.60) * dmg_mult;
    let suppression_add = (supp_channel
        * hit_prob
        * (rounds_fired as f32).sqrt()
        * 0.22
        * (1.0 - effective_cover * 0.4))
        .clamp(0.0, 3.0);

    let hash_frac = ((mix_time_unit_hash(clock_seconds, &attacker.callsign) & 0xFFFF) as f32) / 65536.0;
    let hp_per_man = (target.max_hp / target.max_men.max(1) as f32).max(1.0);
    let exact_cas = raw_hp_loss / hp_per_man;
    let mut casualties = exact_cas.floor() as u16;
    if exact_cas.fract() > hash_frac {
        casualties = casualties.saturating_add(1);
    }
    casualties = casualties.min(target.men);

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

    FireOutcome {
        rounds_fired,
        casualties,
        hp_loss: raw_hp_loss,
        suppression_add,
        frag_radius_m,
        blast_kpa,
        crater_depth_m,
        hit_prob,
    }
}

const fn zero_fire_outcome() -> FireOutcome {
    FireOutcome {
        rounds_fired: 0,
        casualties: 0,
        hp_loss: 0.0,
        suppression_add: 0.0,
        frag_radius_m: 0.0,
        blast_kpa: 0.0,
        crater_depth_m: 0.0,
        hit_prob: 0.0,
    }
}
