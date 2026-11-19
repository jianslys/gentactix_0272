use super::*;
use crate::units_ecs::{equipment_spec, BallisticGroup, CommanderSoA, DamageType, UnitId, UnitSoA, ARMOR_DURABILITY, CADRE_LOSS_ORG_PENALTY, ORG_DURABILITY, PERSON_DURABILITY, SOFT_DURABILITY};
use crate::world_grid::{PriorityTarget, ShotPacket, TeamId, WorldGrid};
use sha2::{Digest, Sha256};

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
    resolve_cell_strike_bucketed(
        grid, units, commanders, events, None, target_px, attacker_team,
        fire_points, weapon_model, aspect_attacker_pos, is_direct_fire, seed_tag,
        allow_friendly_full, exclude_uid,
    )
}

pub fn resolve_cell_strike_bucketed(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    events: &mut crate::units_ecs::EventQueue,
    buckets: Option<&crate::spatial_autonomy::SpatialBucketGrid>,
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

    // =========================================================================
    // 第一拍 (P2G 射弹注入 8m 网格)
    // =========================================================================
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

    // =========================================================================
    // 第二拍 (Grid-to-Field 场展开与压制传递)
    // =========================================================================
    let target_gi = grid.idx(target_px.0, target_px.1) as u32;
    let registered_shots = grid
        .pixel_containers
        .get(&target_gi)
        .map_or(1, |c| c.incoming_packets.iter().map(|p| p.shots).sum::<u32>());
    let base_suppr = (registered_shots as f32 * 0.1).max(0.5) * (if is_he { 1.5 } else { 1.0 });

    if let Some(c) = grid.pixel_containers.get_mut(&target_gi) {
        c.suppression_density = (c.suppression_density + base_suppr).min(100.0);
    }
    if (target_gi as usize) < grid.exposure.len() {
        grid.exposure[target_gi as usize] = (grid.exposure[target_gi as usize] + 0.25).min(1.0);
        if !grid.exposure_active.contains(&target_gi) {
            grid.exposure_active.push(target_gi);
        }
    }

    let frag_px = (spec.frag_radius_m / grid.cell_m).max(0.5);
    if is_he && frag_px > 0.5 {
        let rad_cells = frag_px.ceil() as i32;
        let r_min = (target_px.0 - rad_cells).max(0);
        let r_max = (target_px.0 + rad_cells).min(grid.rows as i32 - 1);
        let c_min = (target_px.1 - rad_cells).max(0);
        let c_max = (target_px.1 + rad_cells).min(grid.cols as i32 - 1);
        for r in r_min..=r_max {
            for c in c_min..=c_max {
                if r == target_px.0 && c == target_px.1 {
                    continue;
                }
                let d_px = ((r - target_px.0) as f32).hypot((c - target_px.1) as f32);
                if d_px <= frag_px {
                    let falloff = (-0.5 * (d_px / frag_px.max(0.5)).powi(2)).exp();
                    let ngi = grid.idx(r, c) as u32;
                    let n_c = grid.pixel_containers.entry(ngi).or_default();
                    n_c.suppression_density = (n_c.suppression_density + base_suppr * 0.5 * falloff).min(100.0);
                }
            }
        }
    }

    // =========================================================================
    // 第三拍 (G2P 从受击 8m 网格向驻留单位结算，真实消费 incoming_packets)
    // =========================================================================
    let consumed_packets = if let Some(c) = grid.pixel_containers.get_mut(&target_gi) {
        std::mem::take(&mut c.incoming_packets)
    } else {
        Vec::new()
    };

    let total_packet_shots: u32 = consumed_packets.iter().map(|p| p.shots).sum();
    let effective_shots = total_packet_shots.max(registered_shots).max(1);

    // 双时间尺度更新: tau_supp = 36s (alpha_supp = 0.6412)
    let alpha_supp = 0.6412f32;
    let supp_delta = (effective_shots as f32 * 0.12).min(15.0);
    if let Some(c) = grid.pixel_containers.get_mut(&target_gi) {
        c.suppression_density = (c.suppression_density * alpha_supp + supp_delta).min(100.0);
    }

    // Collect candidate units in target cell (or within HE frag Gaussian radius)
    let max_cell_dist = if is_he { frag_px.ceil().max(1.0) } else { 0.5 };
    let max_cell_i = max_cell_dist.ceil() as i32;

    let mut targets: Vec<(UnitId, f32)> = Vec::new();
    let mut evaluate_target_candidate = |uid: UnitId| {
        let u = uid.0;
        if Some(uid) == exclude_uid || !units.alive[u] || units.persons(uid) == 0 {
            return;
        }
        let d_px = ((units.px[u].0 - target_px.0) as f32).hypot((units.px[u].1 - target_px.1) as f32);
        if d_px <= max_cell_dist {
            let gauss = if is_he {
                (-0.5 * (d_px / frag_px.max(0.5)).powi(2)).exp()
            } else if units.px[u] == target_px {
                1.0
            } else {
                0.0
            };
            if gauss > 0.05 {
                targets.push((uid, gauss));
            }
        }
    };

    // 真实空间桶查询，绝不进行全图无限制扫表
    if let Some(b) = buckets {
        b.query_neighbors_within_px(target_px.0, target_px.1, max_cell_i, &mut evaluate_target_candidate);
    } else {
        for uid in 0..units.id.len() {
            let (ur, uc) = units.px[uid];
            if (ur - target_px.0).abs() <= max_cell_i && (uc - target_px.1).abs() <= max_cell_i {
                evaluate_target_candidate(UnitId(uid));
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

        let cos_theta = ((incoming_dir - units.facing_deg[i]).to_radians().cos()).abs().clamp(0.2, 1.0);
        let (cov_eff, flank_mult, _) = if units.is_directional_hull(uid) {
            compute_directional_cover_eff(units.pos[i], units.facing_deg[i], aspect_attacker_pos, base_cov)
        } else {
            (base_cov * 0.65 * (1.0 - 0.15 * (1.0 - cos_theta)), 1.0, 0.5)
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

