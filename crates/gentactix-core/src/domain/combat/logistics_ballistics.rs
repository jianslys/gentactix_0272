use super::*;
use crate::command_log::{compute_operational_ao_buff, OperationalZone};
use crate::units_ecs::{equipment_spec, CommanderSoA, UnitId, UnitSoA};
use crate::world_grid::{PriorityTarget, ShotPacket, TeamId, WorldGrid};
use serde::{Deserialize, Serialize};


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
