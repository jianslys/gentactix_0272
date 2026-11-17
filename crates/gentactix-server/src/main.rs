#![recursion_limit = "256"]
//! Gentactix V3.1 Axum HTTP Server: 256x256 RGBA viewport tiles, binary terrain stream,
//! commander readable-scope filtered state JSON, Layer 3 shared-DDA weapon LOS, and 5m commit turns.

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use gentactix_core::combat_logistics::{compute_fire_arc, gather_intel};
use gentactix_core::command_log::{
    can_commander_write_line, collect_commander_uplink_logs, commander_has_officer_or_acting_nco,
    get_commander_readable_scope, radio_channel_idx, register_zone, validate_command_link,
    validate_commander_link,
};
use gentactix_core::sim_loop::{build_art_16_world, build_demo_world, build_r16_platoon_world, plan_grid_path_f32, SimWorld};
use gentactix_core::units_ecs::{equipment_spec, CommanderId, Echelon, EventKind, ReorgTargetSpec, UnitId, UnitState, RADIO_CHANNELS};
use gentactix_core::world_grid::{
    FrontlineRole, MobilityClass, TeamId, WorldGrid, AIRFIELD, BRIDGE, DIRT_ROAD, FOREST, HIGHWAY,
    LINE_MASK_ALL, MOUNTAIN, PLAIN, RAIL, SWAMP, URBAN, WATER,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Parser, Debug)]
#[command(name = "gentactix-server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 8767)]
    port: u16,
    #[arg(long, default_value = r"E:\商单1100\New folder")]
    r16_dir: PathBuf,
    #[arg(long, default_value = "1,1")]
    tile_xy: String,
    #[arg(long, default_value_t = 100)]
    flow_threshold: u16,
    #[arg(long)]
    demo: bool,
    #[arg(long)]
    r16: bool,
    #[arg(long, default_value_t = true)]
    art16: bool,
    #[arg(long, default_value = r"D:\tools\gentactix\web\index.html")]
    web_index: PathBuf,
    #[arg(long, default_value = r"D:\tools\gentactix\web\assets")]
    web_assets: PathBuf,
}

#[derive(Default, Clone)]
struct MultiplayerHub {
    seat_tokens: HashMap<String, String>,
    ready_commanders: HashSet<String>,
    state_revision: u64,
}

#[derive(Clone)]
struct AppState {
    world: Arc<Mutex<SimWorld>>,
    mp: Arc<Mutex<MultiplayerHub>>,
    web_index: PathBuf,
    web_assets: PathBuf,
    r16_dir: PathBuf,
}

#[derive(Deserialize)]
struct StateQuery {
    commander_id: Option<String>,
    team: Option<String>,
    seat_token: Option<String>,
}

fn serialize_state(w: &SimWorld, commander_str: Option<&str>, observer_team: Option<&str>) -> Value {
    let cid_opt = commander_str
        .and_then(|s| w.commanders.str_to_id.get(s).copied())
        .or_else(|| if w.commanders.id.is_empty() { None } else { Some(CommanderId(0)) });

    let team = observer_team
        .map(TeamId::from_str)
        .or_else(|| cid_opt.map(|c| w.commanders.team[c.0]))
        .unwrap_or(TeamId::Red);

    let g = &w.grid;
    let total_cells = g.rows * g.cols;
    let large_grid = total_cells > 128 * 128;

    let scope = match cid_opt {
        Some(cid) => get_commander_readable_scope(cid, &w.units, &w.commanders, g),
        None => Default::default(),
    };

    let hq_piece_set: std::collections::HashSet<UnitId> = w.commanders.hq_piece_id.iter().filter_map(|x| *x).collect();
    let actor_dead = cid_opt.map_or(false, |c| !w.commanders.alive[c.0]);
    let own_pieces: Vec<Value> = (0..w.units.id.len())
        .filter(|&i| {
            w.units.alive[i]
                && w.units.team[i] == team
                && (w.units.persons(UnitId(i)) > 0
                    || w.units.vehicle_model[i].is_some()
                    || w.units.deployed_weapon[i].is_some()
                    || w.units.kind[i] == "heavy_emplacement")
        })
        .filter(|&i| {
            if actor_dead {
                !hq_piece_set.contains(&UnitId(i))
            } else {
                cid_opt.is_none()
                    || scope.readable_unit_ids.contains(&UnitId(i))
                    || w.units.operated_by[i].map_or(false, |op| scope.readable_unit_ids.contains(&op))
            }
        })
        .map(|i| {
            let u = UnitId(i);
            let eq_vec: Vec<Value> = w.units.equipment[i]
                .iter()
                .filter(|(_, &n)| n > 0)
                .map(|(m, &n)| json!({ "model": m, "count": n, "range_m": equipment_spec(m).range_m }))
                .collect();
            let pw = w.primary_weapon_for_unit(u);
            let pw_range = equipment_spec(&pw).range_m;
            let (eff_crew, _, _, op_buff_mult) = w.units.effective_operator_stats(u);
            let base_speed_kmh = w.units.effective_speed_kmh(
                u,
                g,
                w.units.px[i].0,
                w.units.px[i].1,
                w.units.px[i].0,
                w.units.px[i].1,
                "move",
                "normal",
            );
            let unit_state_s = match w.units.unit_state[i] {
                UnitState::Mobile => "mobile",
                UnitState::Deployed => "deployed",
                UnitState::Carried => "carried",
            };
            let is_heavy_emp = w.units.kind[i] == "heavy_emplacement" || w.units.unit_state[i] == UnitState::Deployed;
            let active_events: Vec<Value> = w
                .events
                .events
                .iter()
                .filter(|ev| ev.actor_id == u)
                .map(|ev| {
                    let kname = match &ev.kind {
                        EventKind::FormCommandSquad { .. } => "form_command_squad",
                        EventKind::Engineering { op, .. } => op.as_str(),
                        EventKind::Demolition { .. } => "demolition",
                        EventKind::LayWire { .. } => "lay_wire",
                        EventKind::CargoTransfer { .. } => "cargo_transfer",
                        EventKind::RadioDecoy { .. } => "radio_decoy",
                        EventKind::DeployHeavyUnit { .. } => "deploy_heavy_unit",
                        EventKind::PackHeavyUnit { .. } => "pack_heavy_unit",
                        EventKind::OperateHeavyUnit { .. } => "operate_heavy_unit",
                        EventKind::ResupplyPickup { .. } => "resupply_pickup",
                        EventKind::ReorganizeEchelon { .. } => "reorganize",
                        _ => "event",
                    };
                    json!({
                        "id": ev.id,
                        "kind": kname,
                        "remaining_hours": ev.remaining_hours,
                    })
                })
                .collect();
            json!({
                "id": w.units.str_id[i],
                "name": w.units.name[i],
                "kind": &w.units.kind[i],
                "is_hq_piece": hq_piece_set.contains(&u),
                "is_heavy_emplacement": is_heavy_emp,
                "unit_state": unit_state_s,
                "deployed_weapon": &w.units.deployed_weapon[i],
                "vehicle_model": &w.units.vehicle_model[i],
                "operated_by": w.units.operated_by[i].map(|op| w.units.str_id[op.0].clone()),
                "operator_soldier_ids": &w.units.assigned_operator_soldiers[i],
                "assigned_operator_count": w.units.assigned_operator_count[i],
                "effective_crew": eff_crew,
                "operator_buff_mult": op_buff_mult,
                "base_speed_kmh": base_speed_kmh,
                "mobility": w.units.mobility[i].as_str(),
                "has_engineer_cap": w.units.unit_has_engineer_cap(u),
                "team": w.units.team[i].as_str(),
                "level": w.units.restricted_echelon[i].as_str(),
                "declared_level": w.units.echelon[i].as_str(),
                "px": [w.units.px[i].0, w.units.px[i].1],
                "pos": [w.units.pos[i][0], w.units.pos[i][1]],
                "persons": w.units.persons(u),
                "enlisted": w.units.enlisted[i],
                "nco": w.units.nco[i],
                "officer": w.units.officer[i],
                "cadre": w.units.cadre[i],
                "can_operate_radio": w.units.can_operate_radio[i],
                "heavy_op_cap": w.units.heavy_op_cap[i],
                "is_acting_nco_cmd": w.units.is_acting_nco_cmd[i],
                "retreat_recovery_hours": w.units.retreat_recovery_hours[i],
                "org": w.units.org[i],
                "org_cap": w.units.org_cap[i],
                "ammo_t": w.units.ammo_t[i],
                "ammo_cap_t": w.units.cap_ammo_t[i].max(0.001),
                "carry_loss": w.units.carry_loss[i],
                "delta_persons_5m": w.units.delta_persons_5m[i],
                "delta_org_5m": w.units.delta_org_5m[i],
                "primary_weapon": pw,
                "primary_range_m": pw_range,
                "weapon_cooldown_sec": w.weapon_cooldowns.get(&format!("{}:{}", w.units.str_id[i], pw)).copied().unwrap_or(0.0),
                "empty_cadre": w.units.empty_cadre[i],
                "facing_deg": w.units.facing_deg[i],
                "tactical_face_vec": w.units.tactical_face_vec[i],
                "is_directional_hull": w.units.is_directional_hull(u),
                "radio_channel": RADIO_CHANNELS[w.units.radio_channel[i] as usize % 7],
                "radio_silence": w.units.radio_silence[i],
                "autonomy_state": w.units.autonomy_state[i].as_str(),
                "active_deploy_line_id": w.units.active_deploy_line_id[i],
                "k_c": w.units.k_c[i],
                "was_disconnected": w.units.was_disconnected[i],
                "unit_mask": w.units.unit_mask[i],
                "direct_order": w.direct_unit_plans.get(&w.units.str_id[i]),
                "task_queue": w.direct_unit_plans.get(&w.units.str_id[i]).map(|plan| {
                    plan.segments.iter().enumerate().map(|(sidx, s)| {
                        json!({
                            "step_id": sidx + 1,
                            "action": s.action,
                            "goal": s.goal,
                            "move_mode": s.move_mode,
                            "fire_mode": s.fire_mode,
                            "follow_target": s.follow_target,
                            "target_unit_id": s.target_unit_id,
                            "duration_sec": s.duration_sec,
                            "priority_target": s.priority_target,
                        })
                    }).collect::<Vec<_>>()
                }).unwrap_or_default(),
                "order_doc": w.format_unit_active_order_text(u),
                "active_events": active_events,
                "weapons": eq_vec,
                "soldiers": &w.units.soldiers[i],
                "carried_bodies": &w.units.carried_bodies[i],
                "food_t": w.units.food_t[i],
                "fuel_t": w.units.fuel_t[i],
                "readable": cid_opt.is_none() || scope.readable_unit_ids.contains(&u),
                "counter": {
                    "restricted_echelon": w.units.restricted_echelon[i].as_str(),
                    "display_tier_bonus": w.units.display_tier_bonus[i],
                    "color": w.units.skin_color[i],
                    "custom_label": w.units.skin_label[i],
                    "badge_style": w.units.skin_badge[i],
                }
            })
        })
        .collect();

    let intel = gather_intel(team, g, &w.units, w.night);
    let visible_enemies: Vec<Value> = intel
        .into_iter()
        .filter(|c| {
            let Some(cid) = cid_opt else { return true; };
            let cpos = [c.px.0 as f32, c.px.1 as f32];
            if g.distance_km(w.commander_pos_f32(cid), cpos) <= 4.5 {
                return true;
            }
            scope.readable_unit_ids.iter().any(|&su| g.distance_km(w.units.pos[su.0], cpos) <= 4.5)
        })
        .map(|c| {
            json!({
                "id": w.units.str_id[c.unit_id.0],
                "px": [c.px.0, c.px.1],
                "fog_level": c.fog_level,
                "facing_deg": c.facing_deg,
                "est_persons": c.est_persons,
            })
        })
        .collect();

    let commanders_out: Vec<Value> = (0..w.commanders.id.len())
        .filter(|&i| w.commanders.team[i] == team)
        .map(|i| {
            let cid = CommanderId(i);
            let sub_strs: Vec<String> = w.commanders.subordinate_unit_ids[i]
                .iter()
                .map(|u| w.units.str_id[u.0].clone())
                .collect();
            let hq_u = w.commanders.hq_piece_id[i];
            let hq_persons = hq_u.map(|u| w.units.persons(u)).unwrap_or(3);
            let hq_nco = hq_u.map(|u| w.units.nco[u.0]).unwrap_or(0);
            let hq_officer = hq_u.map(|u| w.units.officer[u.0]).unwrap_or(1);
            let hq_cadre = hq_u.map(|u| w.units.cadre[u.0]).unwrap_or(1);
            let hq_org = hq_u.map(|u| w.units.org[u.0]).unwrap_or(100.0);
            let hq_org_cap = hq_u.map(|u| w.units.org_cap[u.0]).unwrap_or(100.0);
            let hq_ammo_t = hq_u.map(|u| w.units.ammo_t[u.0]).unwrap_or(0.0);
            let hq_ammo_cap_t = hq_u.map(|u| w.units.cap_ammo_t[u.0].max(0.001)).unwrap_or(1.0);
            let hq_carry_loss = hq_u.map(|u| w.units.carry_loss[u.0]).unwrap_or(0.0);
            let hq_delta_org = hq_u.map(|u| w.units.delta_org_5m[u.0]).unwrap_or(0.0);
            let hq_pw = hq_u.map(|u| w.primary_weapon_for_unit(u)).unwrap_or_else(|| "rifle".into());
            let hq_pw_range = equipment_spec(&hq_pw).range_m;
            let (has_off, is_acting_nco) = commander_has_officer_or_acting_nco(cid, &w.commanders, &w.units);
            json!({
                "id": w.commanders.str_id[i],
                "name": w.commanders.name[i],
                "team": w.commanders.team[i].as_str(),
                "echelon": w.commanders.echelon[i].as_str(),
                "rank": w.commanders.rank[i].as_str(),
                "px": [w.commanders.px[i].0, w.commanders.px[i].1],
                "pos": w.commander_pos_f32(cid),
                "direct_order": w.direct_cmd_plans.get(&w.commanders.str_id[i]),
                "task_queue": w.direct_cmd_plans.get(&w.commanders.str_id[i]).map(|plan| {
                    plan.segments.iter().enumerate().map(|(sidx, s)| {
                        json!({
                            "step_id": sidx + 1,
                            "action": s.action,
                            "goal": s.goal,
                            "move_mode": s.move_mode,
                            "fire_mode": s.fire_mode,
                            "follow_target": s.follow_target,
                            "target_unit_id": s.target_unit_id,
                            "duration_sec": s.duration_sec,
                            "priority_target": s.priority_target,
                        })
                    }).collect::<Vec<_>>()
                }).or_else(|| {
                    hq_u.and_then(|u| w.direct_unit_plans.get(&w.units.str_id[u.0])).map(|plan| {
                        plan.segments.iter().enumerate().map(|(sidx, s)| {
                            json!({
                                "step_id": sidx + 1,
                                "action": s.action,
                                "goal": s.goal,
                                "move_mode": s.move_mode,
                                "fire_mode": s.fire_mode,
                                "follow_target": s.follow_target,
                                "target_unit_id": s.target_unit_id,
                                "duration_sec": s.duration_sec,
                                "priority_target": s.priority_target,
                            })
                        }).collect::<Vec<_>>()
                    })
                }).unwrap_or_default(),
                "alive": w.commanders.alive[i],
                "persons": hq_persons,
                "hq_nco": hq_nco,
                "hq_officer": hq_officer,
                "has_officer": has_off,
                "is_acting_nco_deputy": is_acting_nco,
                "cadre": hq_cadre,
                "hq_org": hq_org,
                "hq_org_cap": hq_org_cap,
                "hq_ammo_t": hq_ammo_t,
                "hq_ammo_cap_t": hq_ammo_cap_t,
                "hq_carry_loss": hq_carry_loss,
                "hq_delta_org_5m": hq_delta_org,
                "hq_soldiers": hq_u.map(|u| w.units.soldiers[u.0].clone()).unwrap_or_default(),
                "hq_carried_bodies": hq_u.map(|u| w.units.carried_bodies[u.0].clone()).unwrap_or_default(),
                "hq_food_t": hq_u.map(|u| w.units.food_t[u.0]).unwrap_or(0.0),
                "hq_fuel_t": hq_u.map(|u| w.units.fuel_t[u.0]).unwrap_or(0.0),
                "primary_weapon": hq_pw,
                "primary_range_m": hq_pw_range,
                "radio_channel": RADIO_CHANNELS[w.commanders.radio_channel[i] as usize % 7],
                "radio_silence": w.commanders.radio_silence[i],
                "order_churn_score": w.commanders.order_churn_score[i],
                "war_council_active": w.commanders.war_council_sync_hours[i] > 0.0 || w.commanders.briefing_buff_hours[i] > 0.0,
                "max_area_px": gentactix_core::command_log::effective_commander_max_area_px(cid, &w.commanders),
                "radius_km": gentactix_core::command_log::effective_commander_radius_km(cid, &w.commanders, &w.units, g),
                "subordinate_ids": sub_strs,
                "is_self": Some(cid) == cid_opt,
                "readable": scope.readable_cmd_ids.contains(&cid),
                "proximity_readable": scope.proximity_cmd_ids.contains(&cid),
            })
        })
        .collect();

    let mut layer1_links = Vec::new();
    if let Some(cid) = cid_opt {
        let c_pos = w.commander_pos_f32(cid);
        for &su in &w.commanders.subordinate_unit_ids[cid.0] {
            if scope.readable_unit_ids.contains(&su) {
                let via = validate_command_link(cid, su, &w.commanders, &w.units, g);
                if via.is_connected() {
                    layer1_links.push(json!({
                        "from": [c_pos[0], c_pos[1]],
                        "to": [w.units.pos[su.0][0], w.units.pos[su.0][1]],
                        "target_id": w.units.str_id[su.0],
                        "via": via.as_str(),
                        "kind": "subordinate",
                    }));
                }
            }
        }
        for &sc in &w.commanders.subordinate_cmd_ids[cid.0] {
            if scope.readable_cmd_ids.contains(&sc) {
                let via = validate_commander_link(cid, sc, &w.commanders, &w.units, g);
                if via.is_connected() {
                    let sc_pos = w.commander_pos_f32(sc);
                    layer1_links.push(json!({
                        "from": [c_pos[0], c_pos[1]],
                        "to": [sc_pos[0], sc_pos[1]],
                        "target_id": w.commanders.str_id[sc.0],
                        "via": via.as_str(),
                        "kind": "subordinate_cmdr",
                    }));
                }
            }
        }
    }

    let ao_out: Vec<Value> = w.zones
        .iter()
        .filter(|z| z.active && z.team == team)
        .map(|z| {
            json!({
                "zone_id": z.name_id,
                "commander_id": w.commanders.str_id[z.commander_id.0],
                "echelon": z.echelon.as_str(),
                "area_px": z.area_px(),
                "activate_turn": z.activate_turn,
                "cells": z.cells.iter().map(|&(r, c)| [r, c]).collect::<Vec<_>>(),
                "writable": Some(z.commander_id) == cid_opt,
            })
        })
        .collect();

    let frontlines_out: Vec<Value> = g.frontlines
        .iter()
        .filter(|fl| fl.active && fl.team == team)
        .map(|fl| {
            json!({
                "line_id": fl.name_id,
                "numeric_id": fl.line_id,
                "role": fl.role.as_str(),
                "source_deploy_line_id": fl.source_deploy_line_id,
                "commander_id": w.commanders.str_id.get(fl.commander_id).cloned().unwrap_or_default(),
                "unit_type_mask": fl.unit_type_mask,
                "attraction_weight": fl.attraction_weight,
                "front_normal": fl.front_normal,
                "points_px": fl.points_px,
            })
        })
        .collect();

    let stashes: Vec<Value> = g.cell_stash
        .iter()
        .filter(|(_, st)| st.owner_team.map_or(true, |t| t == team))
        .map(|(&idx, st)| {
            let r = idx as usize / g.cols;
            let c = idx as usize % g.cols;
            json!({
                "px": [r, c],
                "resources": { "food": st.food_t, "ammo": st.ammo_t, "fuel": st.fuel_t, "replacements": st.replacements },
                "equipment": st.equipment,
            })
        })
        .collect();

    let focus_px = cid_opt
        .map(|c| [w.commanders.px[c.0].0, w.commanders.px[c.0].1])
        .unwrap_or([g.rows as i32 / 2, g.cols as i32 / 2]);

    let uplink_logs = cid_opt
        .map(|c| collect_commander_uplink_logs(c, &w.units, &w.commanders, g))
        .unwrap_or_default();

    let mut grid_obj = json!({
        "rows": g.rows,
        "cols": g.cols,
        "cell_m": g.cell_m,
        "large_grid": large_grid,
        "terrain_revision": g.terrain_revision,
        "elev_min": g.elev_min_m,
        "elev_max": g.elev_max_m,
        "focus_px": focus_px,
    });
    if !large_grid {
        let mut t_rows = Vec::with_capacity(g.rows);
        let mut e_rows = Vec::with_capacity(g.rows);
        let mut d_rows = Vec::with_capacity(g.rows);
        for r in 0..g.rows {
            let s = r * g.cols;
            t_rows.push(&g.terrain[s..s + g.cols]);
            e_rows.push(&g.elevation_m[s..s + g.cols]);
            d_rows.push(&g.forest_density[s..s + g.cols]);
        }
        grid_obj["terrain"] = json!(t_rows);
        grid_obj["elevation"] = json!(e_rows);
        grid_obj["forest_density"] = json!(d_rows);
    }

    let readable_str_ids: HashSet<String> = if cid_opt.is_some() {
        let mut s = HashSet::new();
        for &ru in &scope.readable_unit_ids {
            s.insert(w.units.str_id[ru.0].clone());
        }
        for &rc in &scope.readable_cmd_ids {
            s.insert(w.commanders.str_id[rc.0].clone());
        }
        s
    } else {
        HashSet::new()
    };
    let filtered_combat_logs: Vec<_> = if cid_opt.is_some() && !readable_str_ids.is_empty() {
        w.direct_combat_logs
            .iter()
            .filter(|e| {
                readable_str_ids.contains(&e.actor_id)
                    || e.target_id.as_ref().map_or(false, |t| readable_str_ids.contains(t))
            })
            .cloned()
            .collect()
    } else {
        w.direct_combat_logs.clone()
    };

    json!({
        "ok": true,
        "ground_bodies": &g.ground_bodies,
        "turn": w.turn,
        "subtick": w.subtick,
        "clock_hours": w.clock_hours,
        "step_seconds": gentactix_core::sim_loop::SUBTICK_SECONDS,
        "pre_game_editable": w.pre_game_editable && w.turn == 0,
        "active_commander_id": cid_opt.map(|c| w.commanders.str_id[c.0].clone()),
        "team": team.as_str(),
        "r16_source": w.r16_source,
        "grid": grid_obj,
        "commanders": commanders_out,
        "own_pieces": own_pieces,
        "visible_enemies": visible_enemies,
        "layer1_links": layer1_links,
        "ao_zones": ao_out,
        "typed_frontlines": frontlines_out,
        "stashes": stashes,
        "uplink_logs": uplink_logs,
        "debug_full_friendly_fire": w.debug_full_friendly_fire,
        "direct_combat_logs": filtered_combat_logs,
        "readable_count": scope.readable_unit_ids.len() + scope.readable_cmd_ids.len(),
    })
}

async fn get_index(State(st): State<AppState>) -> impl IntoResponse {
    match std::fs::read(&st.web_index) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(bytes))
            .unwrap(),
        Err(e) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(format!("missing index.html: {e}")))
            .unwrap(),
    }
}

async fn get_asset(
    State(st): State<AppState>,
    AxumPath(rel): AxumPath<String>,
) -> impl IntoResponse {
    if rel.contains("..") || rel.starts_with('/') || rel.starts_with('\\') {
        return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap();
    }
    let full = st.web_assets.join(rel.trim_start_matches('/'));
    match std::fs::read(&full) {
        Ok(bytes) => {
            let ct = if rel.ends_with(".svg") {
                "image/svg+xml; charset=utf-8"
            } else if rel.ends_with(".png") {
                "image/png"
            } else if rel.ends_with(".jpg") || rel.ends_with(".jpeg") {
                "image/jpeg"
            } else if rel.ends_with(".webp") {
                "image/webp"
            } else {
                "application/octet-stream"
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, ct)
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap(),
    }
}

async fn get_tile(
    State(st): State<AppState>,
    AxumPath((_z, tr, tc)): AxumPath<(usize, usize, usize)>,
) -> impl IntoResponse {
    let w = st.world.lock().unwrap();
    let tile_size = 256usize;
    let (h, wid, rgba) = w.grid.encode_tile_rgba(tr, tc, tile_size);
    let rev = w.grid.terrain_revision;
    if h == 0 || wid == 0 {
        return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
    }
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(rgba))
        .unwrap();
    resp.headers_mut().insert("x-tile-width", HeaderValue::from_str(&wid.to_string()).unwrap());
    resp.headers_mut().insert("x-tile-height", HeaderValue::from_str(&h.to_string()).unwrap());
    resp.headers_mut().insert("x-terrain-rev", HeaderValue::from_str(&rev.to_string()).unwrap());
    resp
}

async fn get_terrain_raw(State(st): State<AppState>) -> impl IntoResponse {
    let raw = {
        let w = st.world.lock().unwrap();
        w.grid.encode_terrain_raw_bin()
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(raw))
        .unwrap()
}

fn check_mp_seat_auth(mp: &MultiplayerHub, cid_opt: Option<&str>, token_opt: Option<&str>) -> bool {
    let Some(cid) = cid_opt else { return true; };
    match mp.seat_tokens.get(cid) {
        Some(expected) => token_opt.map_or(false, |t| t == expected),
        None => true,
    }
}

fn mp_status_json(mp: &MultiplayerHub) -> Value {
    let mut claimed: Vec<&String> = mp.seat_tokens.keys().collect();
    claimed.sort();
    let mut ready: Vec<&String> = mp.ready_commanders.iter().collect();
    ready.sort();
    let all_ready = !mp.seat_tokens.is_empty() && mp.seat_tokens.keys().all(|k| mp.ready_commanders.contains(k));
    json!({
        "claimed_seats": claimed,
        "ready_commanders": ready,
        "all_ready": all_ready,
        "state_revision": mp.state_revision,
    })
}

async fn get_mp_status(State(st): State<AppState>) -> Json<Value> {
    let mp = st.mp.lock().unwrap();
    let mut v = mp_status_json(&mp);
    v["ok"] = json!(true);
    Json(v)
}

async fn post_mp_claim_seat(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let Some(cid) = body.get("commander_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "commander_id_required"})));
    };
    let req_tok = body.get("seat_token").and_then(|v| v.as_str());
    let release = body.get("release").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut mp = st.mp.lock().unwrap();
    if release {
        if let Some(exp) = mp.seat_tokens.get(cid) {
            if req_tok.map_or(true, |t| t != exp) {
                return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "seat_token_mismatch"})));
            }
        }
        mp.seat_tokens.remove(cid);
        mp.ready_commanders.remove(cid);
        mp.state_revision += 1;
        let mut res = mp_status_json(&mp);
        res["ok"] = json!(true);
        return (StatusCode::OK, Json(res));
    }
    if let Some(exp) = mp.seat_tokens.get(cid) {
        if req_tok != Some(exp.as_str()) {
            return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "seat_already_claimed"})));
        }
        let mut res = mp_status_json(&mp);
        res["ok"] = json!(true);
        res["commander_id"] = json!(cid);
        res["seat_token"] = json!(exp);
        return (StatusCode::OK, Json(res));
    }
    let tok = req_tok
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("seat-{cid}-{}", mp.state_revision + 1));
    mp.seat_tokens.insert(cid.to_string(), tok.clone());
    mp.state_revision += 1;
    let mut res = mp_status_json(&mp);
    res["ok"] = json!(true);
    res["commander_id"] = json!(cid);
    res["seat_token"] = json!(tok);
    (StatusCode::OK, Json(res))
}

async fn post_mp_ready(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let Some(cid) = body.get("commander_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "commander_id_required"})));
    };
    let req_tok = body.get("seat_token").and_then(|v| v.as_str());
    let ready = body.get("ready").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut mp = st.mp.lock().unwrap();
    if !check_mp_seat_auth(&mp, Some(cid), req_tok) {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "seat_token_mismatch"})));
    }
    if ready {
        mp.ready_commanders.insert(cid.to_string());
    } else {
        mp.ready_commanders.remove(cid);
    }
    mp.state_revision += 1;
    let mut res = mp_status_json(&mp);
    res["ok"] = json!(true);
    res["ready"] = json!(ready);
    (StatusCode::OK, Json(res))
}

async fn get_state(State(st): State<AppState>, Query(q): Query<StateQuery>) -> (StatusCode, Json<Value>) {
    let mp = st.mp.lock().unwrap();
    if !check_mp_seat_auth(&mp, q.commander_id.as_deref(), q.seat_token.as_deref()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"ok": false, "reason": "seat_token_required_or_mismatch"})),
        );
    }
    let mp_snap = mp_status_json(&mp);
    let rev = mp.state_revision;
    drop(mp);
    let w = st.world.lock().unwrap();
    let mut v = serialize_state(&w, q.commander_id.as_deref(), q.team.as_deref());
    v["state_revision"] = json!(rev);
    v["mp"] = mp_snap;
    (StatusCode::OK, Json(v))
}

async fn post_validate_ao(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let w = st.world.lock().unwrap();
    let player_s = body.get("player_commander_id").and_then(|v| v.as_str());
    let target_s = body.get("commander_id").and_then(|v| v.as_str()).or(player_s);
    let Some(target_s) = target_s else {
        return (StatusCode::OK, Json(json!({"ok": false, "reason": "commander_missing"})));
    };
    let Some(&target_cid) = w.commanders.str_to_id.get(target_s) else {
        return (StatusCode::OK, Json(json!({"ok": false, "reason": "commander_unavailable"})));
    };
    if let Some(ps) = player_s {
        if let Some(&p_cid) = w.commanders.str_to_id.get(ps) {
            if !can_commander_write_line(p_cid, target_cid) {
                return (StatusCode::OK, Json(json!({"ok": false, "reason": "forbidden_write_other_commander_line"})));
            }
        }
    }
    let zspec = body.get("ao_spec").cloned().unwrap_or(json!({}));
    let ech = zspec
        .get("echelon")
        .and_then(|v| v.as_str())
        .map(Echelon::from_str)
        .unwrap_or(w.commanders.echelon[target_cid.0]);

    let cells: Vec<(i32, i32)> = if let Some(arr) = zspec.get("cells").and_then(|v| v.as_array()) {
        arr.iter()
            .filter_map(|p| {
                let pa = p.as_array()?;
                Some((pa.first()?.as_i64()? as i32, pa.get(1)?.as_i64()? as i32))
            })
            .collect()
    } else if let (Some(c), Some(rad)) = (zspec.get("center").and_then(|v| v.as_array()), zspec.get("radius_px").and_then(|v| v.as_f64())) {
        let cr = c[0].as_i64().unwrap_or(0) as i32;
        let cc = c[1].as_i64().unwrap_or(0) as i32;
        let lim = rad.ceil() as i32;
        let r2 = rad * rad;
        let mut v = Vec::new();
        for dr in -lim..=lim {
            for dc in -lim..=lim {
                if ((dr * dr + dc * dc) as f64) <= r2 {
                    v.push((cr + dr, cc + dc));
                }
            }
        }
        v
    } else if zspec.get("shape").and_then(|v| v.as_str()) == Some("box") {
        let p1 = zspec.get("p1").and_then(|v| v.as_array());
        let p2 = zspec.get("p2").and_then(|v| v.as_array());
        if let (Some(a), Some(b)) = (p1, p2) {
            let r0 = a.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let c0 = a.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let r1 = b.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let c1 = b.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let mut v = Vec::new();
            for r in r0.min(r1)..=r0.max(r1) {
                for c in c0.min(c1)..=c0.max(c1) {
                    v.push((r, c));
                }
            }
            v
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let mut dry_zones = w.zones.clone();
    let mut dry_g_dummy = WorldGrid::new_plain(8, 8, 200.0);
    // For dry validation use real grid bounds
    dry_g_dummy.rows = w.grid.rows;
    dry_g_dummy.cols = w.grid.cols;
    let res = register_zone(
        &mut dry_zones,
        format!("draft-{target_s}"),
        target_cid,
        ech,
        cells.clone(),
        w.turn,
        1,
        &w.commanders,
        &mut dry_g_dummy,
    );
    let max_area = gentactix_core::command_log::effective_commander_max_area_px(target_cid, &w.commanders);
    match res {
        Ok(_) => (StatusCode::OK, Json(json!({
            "ok": true, "reason": "ok", "area_px": cells.len(), "max_area_px": max_area,
            "cells": cells.iter().map(|&(r, c)| [r, c]).collect::<Vec<_>>(),
        }))),
        Err(reason) => (StatusCode::OK, Json(json!({
            "ok": false, "reason": reason, "area_px": cells.len(), "max_area_px": max_area, "cells": [],
        }))),
    }
}

async fn post_weapon_los(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let w = st.world.lock().unwrap();
    let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
    };
    let Some(&uid) = w.units.str_to_id.get(pid) else {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
    };
    let wm = body
        .get("weapon_model")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| w.units.equipment[uid.0].keys().next().cloned().unwrap_or_else(|| "rifle".into()));
    let spec = equipment_spec(&wm);
    let max_scan = (spec.range_m * 1.4).max(2400.0);
    let arc = compute_fire_arc(&w.grid, w.units.pos[uid.0], spec.range_m, spec.min_range_m, spec.indirect, w.night, max_scan);

    let my_team = w.units.team[uid.0];
    let mut targetable = Vec::new();
    for c in &arc {
        for ei in 0..w.units.id.len() {
            if w.units.alive[ei] && w.units.team[ei] != my_team && w.units.px[ei] == (c.r, c.c) && w.units.persons(UnitId(ei)) > 0 {
                targetable.push(json!({"id": w.units.str_id[ei], "name": w.units.name[ei], "px": [c.r, c.c], "eff": c.eff}));
            }
        }
    }
    let cells_arr: Vec<Value> = arc.iter().map(|c| json!([c.r, c.c, c.eff])).collect();
    (StatusCode::OK, Json(json!({
        "ok": true, "piece_id": pid, "weapon_model": wm, "range_m": spec.range_m,
        "indirect": spec.indirect, "cells": cells_arr, "targetable_enemies": targetable,
    })))
}

async fn post_edit_pixel(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let mut w = st.world.lock().unwrap();
    if !w.pre_game_editable || w.turn > 0 {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "map_editing_locked_after_game_start"})));
    }
    let cells_json = body.get("cells").and_then(|v| v.as_array()).cloned().or_else(|| {
        body.get("px").map(|p| vec![p.clone()])
    });
    let Some(cells) = cells_json else {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "no_cells_specified"})));
    };
    let tc_opt = body.get("terrain_code").and_then(|v| v.as_u64()).map(|v| v as u8);
    let feat_opt = body.get("feature_id").and_then(|v| v.as_str());
    let elev_opt = body.get("elevation").and_then(|v| v.as_f64()).map(|v| v as f32);

    let mapped_tc = tc_opt.or_else(|| {
        feat_opt.and_then(|f| match f {
            "plain" => Some(PLAIN),
            "forest" => Some(FOREST),
            "mountain" => Some(MOUNTAIN),
            "swamp" => Some(SWAMP),
            "water" => Some(WATER),
            "urban" => Some(URBAN),
            "dirt_road" => Some(DIRT_ROAD),
            "highway" => Some(HIGHWAY),
            "rail" => Some(RAIL),
            "bridge" => Some(BRIDGE),
            "airfield" => Some(AIRFIELD),
            _ => None,
        })
    });

    let mut updated = 0usize;
    for rc in cells {
        if let Some(arr) = rc.as_array() {
            let r = arr.first().and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
            let c = arr.get(1).and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
            if !w.grid.in_bounds(r, c) {
                continue;
            }
            let gi = w.grid.idx(r, c);
            if let Some(tc) = mapped_tc {
                w.grid.terrain[gi] = tc;
            }
            if let Some(ev) = elev_opt {
                w.grid.elevation_m[gi] = ev;
            }
            updated += 1;
        }
    }
    if elev_opt.is_some() {
        w.grid.recompute_slope_horn();
    }
    w.grid.sync_feature_flags_from_terrain();
    w.grid.refresh_attrition();
    w.grid.terrain_revision += 1;
    (StatusCode::OK, Json(json!({"ok": true, "updated_cells": updated})))
}

fn authorize_unit_op(
    w: &SimWorld,
    player_s: Option<&str>,
    target_uid: UnitId,
) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(ps) = player_s else { return Ok(()); };
    let Some(&pcid) = w.commanders.str_to_id.get(ps) else {
        return Err((StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "unknown_player_commander"}))));
    };
    if w.units.team[target_uid.0] != w.commanders.team[pcid.0] {
        return Err((StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "forbidden_cross_team_op"}))));
    }
    Ok(())
}

fn authorize_commander_op(
    w: &SimWorld,
    player_s: Option<&str>,
    target_cid: CommanderId,
) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(ps) = player_s else { return Ok(()); };
    let Some(&pcid) = w.commanders.str_to_id.get(ps) else {
        return Err((StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "unknown_player_commander"}))));
    };
    if w.commanders.team[target_cid.0] != w.commanders.team[pcid.0] {
        return Err((StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "forbidden_cross_team_op"}))));
    }
    Ok(())
}

async fn post_estimate_plan_time(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let w = st.world.lock().unwrap();
    let pid = body.get("piece_id").and_then(|v| v.as_str()).unwrap_or("");
    let (uid_opt, mob, mut cur_pos) = if let Some(&uid) = w.units.str_to_id.get(pid) {
        (Some(uid), w.units.mobility[uid.0], w.units.pos[uid.0])
    } else if let Some(&cid) = w.commanders.str_to_id.get(pid) {
        if let Some(hq) = w.commanders.hq_piece_id[cid.0] {
            (Some(hq), w.units.mobility[hq.0], w.units.pos[hq.0])
        } else {
            (None, MobilityClass::Foot, w.commander_pos_f32(cid))
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
    };

    let empty_steps = Vec::new();
    let steps = body.get("steps").and_then(|v| v.as_array()).unwrap_or(&empty_steps);
    let mut step_out = Vec::with_capacity(steps.len());
    let mut total_min = 0.0f32;
    let mut total_km = 0.0f32;
    for (idx, step) in steps.iter().enumerate() {
        let action = step.get("action").and_then(|v| v.as_str()).unwrap_or("move");
        let move_mode = step.get("move_mode").and_then(|v| v.as_str()).unwrap_or("normal");
        let goal_opt = step.get("goal").and_then(|v| v.as_array()).and_then(|a| {
            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
        });
        let Some(goal) = goal_opt else {
            step_out.push(json!({
                "step": idx + 1,
                "action": action,
                "move_mode": move_mode,
                "duration_min": 0.0,
                "dist_km": 0.0,
                "speed_kmh": 0.0,
            }));
            continue;
        };
        let pts = plan_grid_path_f32(&w.grid, cur_pos, goal, mob);
        let (dur_min, dist_km, avg_spd) =
            w.estimate_path_time_minutes(uid_opt, mob, cur_pos, &pts, action, move_mode);
        total_min += dur_min;
        total_km += dist_km;
        cur_pos = goal;
        step_out.push(json!({
            "step": idx + 1,
            "action": action,
            "move_mode": move_mode,
            "duration_min": (dur_min * 10.0).round() / 10.0,
            "eta_str": format!("+{:.1}分", total_min),
            "dist_km": (dist_km * 100.0).round() / 100.0,
            "speed_kmh": (avg_spd * 10.0).round() / 10.0,
            "path": pts,
        }));
    }
    (StatusCode::OK, Json(json!({
        "ok": true,
        "piece_id": pid,
        "steps": step_out,
        "total_min": (total_min * 10.0).round() / 10.0,
        "total_km": (total_km * 100.0).round() / 100.0,
    })))
}

async fn post_unit_op(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let mut w = st.world.lock().unwrap();
    let op = body.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let player_s = body.get("player_commander_id").and_then(|v| v.as_str()).or_else(|| body.get("commander_id").and_then(|v| v.as_str()));
    match op {
        "split_squad" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let te = body.get("take_enlisted").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
            let tc = body.get("take_cadre").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let nid = format!("{}-sp{}", pid, w.units.id.len());
            match w.units.split_unit(uid, te, tc, &nid) {
                Ok(child) => {
                    if let Some(cmd) = w.units.direct_commander_id[uid.0] {
                        let w_ref: &mut SimWorld = &mut *w;
                        w_ref.commanders.link_subordinate_unit(cmd, child, &mut w_ref.units);
                    }
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "new_piece_id": nid, "state": st_json})))
                }
                Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": e}))),
            }
        }
        "merge_squad_pieces" => {
            let Some(a_s) = body.get("target_piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "target_piece_id_required"})));
            };
            let Some(b_s) = body.get("source_piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "source_piece_id_required"})));
            };
            let (Some(&du), Some(&su)) = (w.units.str_to_id.get(a_s), w.units.str_to_id.get(b_s)) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "unit_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, du) { return err; }
            if let Err(err) = authorize_unit_op(&w, player_s, su) { return err; }
            match w.units.merge_squads_direct(du, su) {
                Ok(tot) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "total_persons": tot, "state": st_json})))
                }
                Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": e}))),
            }
        }
        "start_reorganize_echelon" => {
            let cid_s = body.get("commander_id").and_then(|v| v.as_str()).or(player_s).unwrap_or("");
            let Some(&cid) = w.commanders.str_to_id.get(cid_s) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "commander_not_found"})));
            };
            if let Err(err) = authorize_commander_op(&w, player_s, cid) { return err; }
            let src_ids: Vec<UnitId> = body
                .get("source_unit_ids")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().and_then(|s| w.units.str_to_id.get(s).copied()))
                        .collect()
                })
                .unwrap_or_default();
            let ech_s = body.get("target_echelon").and_then(|v| v.as_str()).unwrap_or("platoon");
            let target_ech = Echelon::from_str(ech_s);
            let name_s = body.get("name").and_then(|v| v.as_str()).unwrap_or("混编整编分队").to_string();
            let mut tot_e = 0u32;
            let mut tot_c = 0u32;
            let mut eq_agg: HashMap<String, u32> = HashMap::new();
            for &su in &src_ids {
                if su.0 < w.units.alive.len() && w.units.alive[su.0] {
                    tot_e += w.units.enlisted[su.0];
                    tot_c += w.units.cadre[su.0];
                    for (m, &cnt) in &w.units.equipment[su.0] {
                        if cnt > 0 {
                            *eq_agg.entry(m.clone()).or_insert(0) += cnt;
                        }
                    }
                }
            }
            let specs = vec![ReorgTargetSpec {
                str_id: format!("{}-reorg-{}", cid_s, w.units.id.len()),
                name: name_s,
                echelon: target_ech,
                enlisted: tot_e,
                cadre: tot_c,
                equipment: eq_agg.into_iter().collect(),
            }];
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).map(|x| x as f32);
            let hq_piece = w.commanders.hq_piece_id[cid.0];
            let res = {
                let w_mut: &mut SimWorld = &mut *w;
                w_mut.events.start_reorganize_echelon(
                    &mut w_mut.units,
                    &w_mut.commanders,
                    cid,
                    hq_piece,
                    target_ech,
                    src_ids,
                    specs,
                    true,
                    dur_h,
                )
            };
            match res {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "set_counter_skin" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let res = w.units.set_counter_skin(
                uid,
                body.get("color").and_then(|v| v.as_str()),
                body.get("badge_style").and_then(|v| v.as_str()),
                body.get("custom_label").and_then(|v| v.as_str()),
                body.get("attempt_level").and_then(|v| v.as_str()),
            );
            match res {
                Ok(_) => (StatusCode::OK, Json(json!({"ok": true, "piece_id": pid, "level": w.units.restricted_echelon[uid.0].as_str()}))),
                Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": e}))),
            }
        }
        "write_typed_frontline" => {
            let target_s = body.get("commander_id").and_then(|v| v.as_str()).or(player_s).unwrap_or("");
            let target_cid = w.commanders.str_to_id.get(target_s).copied().unwrap_or(CommanderId(0));
            let (has_off, is_acting_nco) = commander_has_officer_or_acting_nco(target_cid, &w.commanders, &w.units);
            if !has_off && !is_acting_nco {
                return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "requires_officer_or_nco_deputy_for_frontline"})));
            }
            let target_hq_alive = w.commanders.alive[target_cid.0]
                && w.commanders.hq_piece_id[target_cid.0].map_or(true, |u| w.units.alive[u.0] && w.units.persons(u) > 0);
            if !target_hq_alive {
                return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "commander_dead_depot"})));
            }
            if let Some(ps) = player_s {
                if let Some(&pcid) = w.commanders.str_to_id.get(ps) {
                    if !can_commander_write_line(pcid, target_cid) {
                        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "forbidden_write_other_commander_line"})));
                    }
                }
            }
            let pts: Vec<[f32; 2]> = body
                .get("points_px")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| {
                            let pa = p.as_array()?;
                            Some([pa.first()?.as_f64()? as f32, pa.get(1)?.as_f64()? as f32])
                        })
                        .collect()
                })
                .unwrap_or_default();
            if pts.len() < 2 {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "frontline_needs_at_least_2_points"})));
            }
            let lid = body
                .get("line_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("fl-{}-{}", target_s, w.grid.frontlines.len()));
            let role = body
                .get("role")
                .and_then(|v| v.as_str())
                .map(FrontlineRole::from_str)
                .unwrap_or(FrontlineRole::Deploy);
            let mask = body.get("unit_type_mask").and_then(|v| v.as_u64()).unwrap_or(LINE_MASK_ALL as u64) as u8;
            let weight = body.get("attraction_weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let team = w.commanders.team[target_cid.0];
            let num_id = w.grid.write_typed_frontline(&lid, team, target_cid.0, 0, role, 0, 0, mask, weight, None, &pts, 4);
            (StatusCode::OK, Json(json!({"ok": true, "line_id": lid, "numeric_id": num_id})))
        }
        "set_radio" | "radio_silence" | "set_radio_channel" => {
            let eid = body
                .get("entity_id")
                .or_else(|| body.get("piece_id"))
                .or_else(|| body.get("commander_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ch_opt = body.get("radio_channel").and_then(|v| v.as_str()).map(radio_channel_idx);
            let sil_opt = body.get("radio_silence").and_then(|v| v.as_bool());
            if let Some(&uid) = w.units.str_to_id.get(eid) {
                if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
                if let Some(ch) = ch_opt { w.units.radio_channel[uid.0] = ch; }
                if let Some(s) = sil_opt { w.units.radio_silence[uid.0] = s; }
                st.mp.lock().unwrap().state_revision += 1;
                let st_json = serialize_state(&w, player_s, None);
                (StatusCode::OK, Json(json!({"ok": true, "entity_id": eid, "state": st_json})))
            } else if let Some(&cid) = w.commanders.str_to_id.get(eid) {
                if let Err(err) = authorize_commander_op(&w, player_s, cid) { return err; }
                if let Some(ch) = ch_opt { w.commanders.radio_channel[cid.0] = ch; }
                if let Some(s) = sil_opt { w.commanders.radio_silence[cid.0] = s; }
                if let Some(hq) = w.commanders.hq_piece_id[cid.0] {
                    if let Some(ch) = ch_opt { w.units.radio_channel[hq.0] = ch; }
                    if let Some(s) = sil_opt { w.units.radio_silence[hq.0] = s; }
                }
                st.mp.lock().unwrap().state_revision += 1;
                let st_json = serialize_state(&w, player_s, None);
                (StatusCode::OK, Json(json!({"ok": true, "entity_id": eid, "state": st_json})))
            } else {
                (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "entity_not_found"})))
            }
        }
        "load_r16_platoon" => {
            let dir = body.get("folder").and_then(|v| v.as_str()).map(PathBuf::from).unwrap_or_else(|| st.r16_dir.clone());
            match build_r16_platoon_world(&dir, (1, 1), 100) {
                Ok(nw) => {
                    *w = nw;
                    (StatusCode::OK, Json(json!({"ok": true, "r16_source": w.r16_source})))
                }
                Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": e}))),
            }
        }
        "reset_demo" => {
            *w = build_demo_world();
            (StatusCode::OK, Json(json!({"ok": true})))
        }
        "load_art16" => {
            *w = build_art_16_world();
            (StatusCode::OK, Json(json!({"ok": true})))
        }
        "direct_action" => {
            let actor_kind = body.get("actor_kind").and_then(|v| v.as_str()).unwrap_or("commander");
            let actor_id = body.get("actor_id").and_then(|v| v.as_str()).or(player_s).unwrap_or("");
            let target_kind = body.get("target_kind").and_then(|v| v.as_str()).unwrap_or("unit");
            let target_id = body.get("target_id").or_else(|| body.get("piece_id")).and_then(|v| v.as_str()).unwrap_or("");
            let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("move");
            if !matches!(action, "move" | "attack" | "assault" | "retreat" | "stop") {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "invalid_action"})));
            }
            let append = body.get("append").and_then(|v| v.as_bool()).unwrap_or(false);
            let move_mode = body.get("move_mode").and_then(|v| v.as_str()).unwrap_or("normal");
            let fire_mode = body.get("fire_mode").and_then(|v| v.as_str()).unwrap_or("stop_to_fire");
            let follow_target = body.get("follow_target").and_then(|v| v.as_bool()).unwrap_or(true);
            let target_unit_id = body.get("target_unit_id").and_then(|v| v.as_str()).map(str::to_string);
            let goal = body.get("goal").and_then(|v| v.as_array()).and_then(|a| {
                Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
            });
            if !matches!(action, "stop" | "retreat") && goal.is_none() && target_unit_id.is_none() {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "goal_required"})));
            }

            // 权限校验：班组只能直接控制自己；指挥官只能直接控制自己或指挥范围内的直属班组
            if actor_kind == "unit" {
                if target_kind != "unit" || target_id != actor_id {
                    return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "unit_can_only_control_self"})));
                }
                if let Some(&tuid) = w.units.str_to_id.get(target_id) {
                    if let Err(err) = authorize_unit_op(&w, player_s, tuid) { return err; }
                }
            } else if actor_kind == "commander" {
                let Some(&acid) = w.commanders.str_to_id.get(actor_id) else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "actor_commander_not_found"})));
                };
                if let Err(err) = authorize_commander_op(&w, player_s, acid) { return err; }
                let actor_hq_alive = w.commanders.alive[acid.0]
                    && w.commanders.hq_piece_id[acid.0].map_or(true, |u| w.units.alive[u.0] && w.units.persons(u) > 0);
                if !actor_hq_alive {
                    return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "commander_dead_depot"})));
                }
                if target_kind == "commander" {
                    if target_id != actor_id {
                        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "cannot_control_other_commander"})));
                    }
                } else if target_kind == "unit" {
                    let Some(&tuid) = w.units.str_to_id.get(target_id) else {
                        return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "target_unit_not_found"})));
                    };
                    if w.units.direct_commander_id[tuid.0] != Some(acid) {
                        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "not_direct_subordinate"})));
                    }
                    let cpos = w.commander_pos_f32(acid);
                    let upos = w.units.pos[tuid.0];
                    let dist_km = ((cpos[0] - upos[0]).hypot(cpos[1] - upos[1])) * w.grid.cell_m / 1000.0;
                    let max_r_km = gentactix_core::command_log::effective_commander_radius_km(acid, &w.commanders, &w.units, &w.grid);
                    if dist_km > max_r_km + 1e-4 {
                        return (StatusCode::FORBIDDEN, Json(json!({
                            "ok": false,
                            "reason": "out_of_command_radius",
                            "dist_km": dist_km,
                            "max_radius_km": max_r_km
                        })));
                    }
                } else {
                    return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "invalid_target_kind"})));
                }
            } else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "invalid_actor_kind"})));
            }

            if target_kind == "unit" {
                let Some(&tuid) = w.units.str_to_id.get(target_id) else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "target_unit_not_found"})));
                };
                let duration_sec = body.get("duration_sec").and_then(|v| v.as_u64()).map(|v| v as u32);
                let priority_target = body.get("priority_target").and_then(|v| v.as_str());
                if let Err(reason) = w.try_set_direct_unit_order_v33(
                    tuid,
                    action,
                    goal,
                    append,
                    target_unit_id,
                    move_mode,
                    fire_mode,
                    follow_target,
                    duration_sec,
                    priority_target,
                ) {
                    return (StatusCode::CONFLICT, Json(json!({"ok": false, "reason": reason})));
                }
            } else {
                let Some(&tcid) = w.commanders.str_to_id.get(target_id) else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "target_commander_not_found"})));
                };
                let tgt_hq_alive = w.commanders.alive[tcid.0]
                    && w.commanders.hq_piece_id[tcid.0].map_or(true, |u| w.units.alive[u.0] && w.units.persons(u) > 0);
                if !tgt_hq_alive {
                    return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "commander_dead_depot"})));
                }
                w.set_direct_cmd_order(tcid, action, goal, append, target_unit_id);
            }
            st.mp.lock().unwrap().state_revision += 1;
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "set_debug_flags" => {
            if let Some(v) = body.get("debug_full_friendly_fire").and_then(|v| v.as_bool()) {
                w.debug_full_friendly_fire = v;
            }
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "clear_frontlines" => {
            let cid_s = body.get("commander_id").and_then(|v| v.as_str()).or(player_s).unwrap_or("");
            let target_ids: Vec<String> = body.get("line_ids")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| {
                    x.as_str().map(|s| s.to_string()).or_else(|| x.as_u64().map(|n| n.to_string()))
                }).collect())
                .unwrap_or_default();
            if !target_ids.is_empty() {
                let cid_opt = w.commanders.str_to_id.get(cid_s).copied();
                for fl in &mut w.grid.frontlines {
                    let id_match = target_ids.iter().any(|tid| tid == &fl.name_id || tid == &fl.line_id.to_string());
                    let cmd_match = cid_opt.map_or(true, |cid| fl.commander_id == cid.0);
                    if id_match && cmd_match {
                        fl.active = false;
                    }
                }
            } else if let Some(&cid) = w.commanders.str_to_id.get(cid_s) {
                for fl in &mut w.grid.frontlines {
                    if fl.commander_id == cid.0 {
                        fl.active = false;
                    }
                }
            } else {
                for fl in &mut w.grid.frontlines {
                    fl.active = false;
                }
            }
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "pickup_body" => {
            let target_kind = body.get("target_kind").and_then(|v| v.as_str()).unwrap_or("unit");
            let target_id = body.get("target_id").and_then(|v| v.as_str()).unwrap_or("");
            let body_id = body.get("body_id").and_then(|v| v.as_str()).unwrap_or("");
            let uid_opt = if target_kind == "commander" {
                w.commanders.str_to_id.get(target_id).copied().and_then(|c| w.commanders.hq_piece_id[c.0])
            } else {
                w.units.str_to_id.get(target_id).copied()
            };
            let Some(uid) = uid_opt else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "unit_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            if !w.units.alive[uid.0] || w.units.persons(uid) == 0 {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "unit_not_alive"})));
            }
            let Some(pos_idx) = w.grid.ground_bodies.iter().position(|b| b.id == body_id) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "body_not_found"})));
            };
            let upos = w.units.pos[uid.0];
            let bpos = w.grid.ground_bodies[pos_idx].pos;
            let d_px = (upos[0] - bpos[0]).hypot(upos[1] - bpos[1]);
            if d_px > 1.35 {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "too_far_to_pickup"})));
            }
            let mut gb = w.grid.ground_bodies.remove(pos_idx);
            if gb.kind == "weapon" {
                if !w.units.soldiers[uid.0].is_empty() {
                    for it in gb.soldier.items.drain(..) {
                        w.units.soldiers[uid.0][0].items.push(it);
                    }
                    w.units.sync_totals_from_soldiers(uid);
                }
            } else {
                if !gb.looted && !w.units.soldiers[uid.0].is_empty() {
                    for it in gb.soldier.items.drain(..) {
                        w.units.soldiers[uid.0][0].items.push(it);
                    }
                    gb.looted = true;
                    gb.soldier.ammo_t = 0.0;
                    w.units.sync_totals_from_soldiers(uid);
                }
                w.units.carried_bodies[uid.0].push(gb);
            }
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "drop_body" => {
            let target_kind = body.get("target_kind").and_then(|v| v.as_str()).unwrap_or("unit");
            let target_id = body.get("target_id").and_then(|v| v.as_str()).unwrap_or("");
            let body_id = body.get("body_id").and_then(|v| v.as_str()).unwrap_or("");
            let uid_opt = if target_kind == "commander" {
                w.commanders.str_to_id.get(target_id).copied().and_then(|c| w.commanders.hq_piece_id[c.0])
            } else {
                w.units.str_to_id.get(target_id).copied()
            };
            let Some(uid) = uid_opt else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "unit_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let Some(c_idx) = w.units.carried_bodies[uid.0].iter().position(|b| b.id == body_id) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "carried_body_not_found"})));
            };
            let mut gb = w.units.carried_bodies[uid.0].remove(c_idx);
            gb.pos = w.units.pos[uid.0];
            w.grid.ground_bodies.push(gb);
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "move_soldier_item" => {
            let from_kind = body.get("from_kind").and_then(|v| v.as_str()).unwrap_or("soldier");
            let to_kind = body.get("to_kind").and_then(|v| v.as_str()).unwrap_or("soldier");
            let item_idx = body.get("item_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let to_slot = body.get("to_slot").and_then(|v| v.as_str()).unwrap_or("pack");
            let to_slot_norm = if to_slot == "hand" { "hand" } else { "pack" };

            let moved_item: gentactix_core::units_ecs::SoldierItem;
            let from_pos: [f32; 2];
            let mut from_unit_str = String::new();
            let mut from_unit_name = String::new();
            let mut from_team = TeamId::Red;
            let mut dirty_from_uid: Option<UnitId> = None;

            if to_kind == "soldier" {
                let t_uk = body.get("to_unit_kind").and_then(|v| v.as_str()).unwrap_or("unit");
                let t_uid_s = body.get("to_unit_id").and_then(|v| v.as_str()).unwrap_or("");
                let pre_tuid = if t_uk == "commander" {
                    w.commanders.str_to_id.get(t_uid_s).copied().and_then(|c| w.commanders.hq_piece_id[c.0])
                } else {
                    w.units.str_to_id.get(t_uid_s).copied()
                };
                if let Some(tuid) = pre_tuid {
                    if let Err(err) = authorize_unit_op(&w, player_s, tuid) { return err; }
                }
            }

            if from_kind == "soldier" {
                let uk = body.get("from_unit_kind").and_then(|v| v.as_str()).unwrap_or("unit");
                let uid_s = body.get("from_unit_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid_s = body.get("from_soldier_id").and_then(|v| v.as_str()).unwrap_or("");
                let fuid_opt = if uk == "commander" {
                    w.commanders.str_to_id.get(uid_s).copied().and_then(|c| w.commanders.hq_piece_id[c.0])
                } else {
                    w.units.str_to_id.get(uid_s).copied()
                };
                let Some(fuid) = fuid_opt else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "from_unit_not_found"})));
                };
                if let Err(err) = authorize_unit_op(&w, player_s, fuid) { return err; }
                from_pos = w.units.pos[fuid.0];
                from_unit_str = w.units.str_id[fuid.0].clone();
                from_unit_name = w.units.name[fuid.0].clone();
                from_team = w.units.team[fuid.0];
                let Some(si) = w.units.soldiers[fuid.0].iter().position(|s| s.id == sid_s) else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "from_soldier_not_found"})));
                };
                if item_idx >= w.units.soldiers[fuid.0][si].items.len() {
                    return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "item_index_out_of_bounds"})));
                }
                moved_item = w.units.soldiers[fuid.0][si].items.remove(item_idx);
                dirty_from_uid = Some(fuid);
            } else if from_kind == "ground" {
                let bid = body.get("from_body_id").and_then(|v| v.as_str()).unwrap_or("");
                let Some(bi) = w.grid.ground_bodies.iter().position(|b| b.id == bid) else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "ground_body_not_found"})));
                };
                from_pos = w.grid.ground_bodies[bi].pos;
                if item_idx >= w.grid.ground_bodies[bi].soldier.items.len() {
                    return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "item_index_out_of_bounds"})));
                }
                let it = w.grid.ground_bodies[bi].soldier.items.remove(item_idx);
                if w.grid.ground_bodies[bi].kind == "weapon" && w.grid.ground_bodies[bi].soldier.items.is_empty() {
                    w.grid.ground_bodies.remove(bi);
                }
                moved_item = it;
            } else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "invalid_from_kind"})));
            }

            let mut item = moved_item;
            if to_kind == "ground" {
                item.slot = "pack".to_string();
                let w_id = format!("body-wpn-drop-{}-{}", item.model, w.grid.ground_bodies.len());
                let w_sol = gentactix_core::units_ecs::SoldierState {
                    id: w_id.clone(),
                    name: item.name.clone(),
                    hp: 100.0,
                    max_hp: 100.0,
                    status: "weapon".to_string(),
                    weapon: item.model.clone(),
                    ammo_t: if item.kind == "ammo" { item.ammo_t } else { 0.0 },
                    is_cadre: false,
                    role: "enlisted".to_string(),
                    cannot_fire: false,
                    cannot_operate_heavy: false,
                    max_hand_weight_kg: None,
                    max_pack_weight_kg: None,
                    pack_cols: 8,
                    pack_rows: 6,
                    items: vec![item],
                };
                w.grid.ground_bodies.push(gentactix_core::units_ecs::GroundBody {
                    id: w_id,
                    kind: "weapon".to_string(),
                    soldier: w_sol,
                    origin_unit_id: from_unit_str,
                    origin_unit_name: from_unit_name,
                    team: from_team,
                    pos: from_pos,
                    looted: false,
                });
                if let Some(fuid) = dirty_from_uid {
                    w.units.sync_totals_from_soldiers(fuid);
                }
            } else if to_kind == "soldier" {
                let uk = body.get("to_unit_kind").and_then(|v| v.as_str()).unwrap_or("unit");
                let uid_s = body.get("to_unit_id").and_then(|v| v.as_str()).unwrap_or("");
                let sid_s = body.get("to_soldier_id").and_then(|v| v.as_str()).unwrap_or("");
                let tuid_opt = if uk == "commander" {
                    w.commanders.str_to_id.get(uid_s).copied().and_then(|c| w.commanders.hq_piece_id[c.0])
                } else {
                    w.units.str_to_id.get(uid_s).copied()
                };
                let Some(tuid) = tuid_opt else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "to_unit_not_found"})));
                };
                let tpos = w.units.pos[tuid.0];
                if (from_pos[0] - tpos[0]).hypot(from_pos[1] - tpos[1]) > 1.35 {
                    if let Some(fuid) = dirty_from_uid {
                        w.units.sync_totals_from_soldiers(fuid);
                    }
                    return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "too_far_to_transfer"})));
                }
                let Some(tsi) = w.units.soldiers[tuid.0].iter().position(|s| s.id == sid_s) else {
                    return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "to_soldier_not_found"})));
                };
                item.slot = to_slot_norm.to_string();
                if let Some(gx) = body.get("to_grid_x").and_then(|v| v.as_u64()) {
                    let cols = w.units.soldiers[tuid.0][tsi].pack_cols.max(1) as u64;
                    item.grid_x = gx.min(cols - 1) as u8;
                }
                if let Some(gy) = body.get("to_grid_y").and_then(|v| v.as_u64()) {
                    let rows = w.units.soldiers[tuid.0][tsi].pack_rows.max(1) as u64;
                    item.grid_y = gy.min(rows - 1) as u8;
                }
                w.units.soldiers[tuid.0][tsi].items.push(item);
                w.units.sync_totals_from_soldiers(tuid);
                if let Some(fuid) = dirty_from_uid {
                    if fuid != tuid {
                        w.units.sync_totals_from_soldiers(fuid);
                    }
                }
            } else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "invalid_to_kind"})));
            }
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "update_queued_segment" => {
            let pid_opt = body.get("piece_id").or_else(|| body.get("target_id")).and_then(|v| v.as_str());
            let Some(pid) = pid_opt else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let uid_opt = if let Some(&c) = w.commanders.str_to_id.get(pid) {
                w.commanders.hq_piece_id[c.0]
            } else {
                w.units.str_to_id.get(pid).copied()
            };
            let Some(uid) = uid_opt else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let seg_idx = body.get("segment_index").or_else(|| body.get("step_index")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let action = body.get("action").and_then(|v| v.as_str());
            let move_mode = body.get("move_mode").and_then(|v| v.as_str());
            let fire_mode = body.get("fire_mode").and_then(|v| v.as_str());
            let follow_target = body.get("follow_target").and_then(|v| v.as_bool());
            let goal = body.get("goal").and_then(|v| v.as_array()).and_then(|a| {
                Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
            });
            let duration_sec = body.get("duration_sec").and_then(|v| v.as_u64()).map(|v| v as u32);
            let priority_target = body.get("priority_target").and_then(|v| v.as_str());
            match w.update_queued_segment_ex(uid, seg_idx, action, move_mode, fire_mode, follow_target, goal, duration_sec, priority_target) {
                Ok(_) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "delete_task_step" => {
            let pid_opt = body.get("piece_id").or_else(|| body.get("target_id")).and_then(|v| v.as_str());
            let Some(pid) = pid_opt else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let uid_opt = if let Some(&c) = w.commanders.str_to_id.get(pid) {
                w.commanders.hq_piece_id[c.0]
            } else {
                w.units.str_to_id.get(pid).copied()
            };
            let Some(uid) = uid_opt else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let step_idx = body.get("step_index").or_else(|| body.get("step_idx")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            match w.delete_task_step(uid, step_idx) {
                Ok(_) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "clear_task_queue" => {
            let pid_opt = body.get("piece_id").or_else(|| body.get("target_id")).and_then(|v| v.as_str());
            let Some(pid) = pid_opt else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let uid_opt = if let Some(&c) = w.commanders.str_to_id.get(pid) {
                w.commanders.hq_piece_id[c.0]
            } else {
                w.units.str_to_id.get(pid).copied()
            };
            let Some(uid) = uid_opt else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let sid = w.units.str_id[uid.0].clone();
            w.direct_unit_plans.remove(&sid);
            st.mp.lock().unwrap().state_revision += 1;
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        "transfer_soldiers" => {
            let src_s = body.get("src_piece_id").or_else(|| body.get("source_piece_id")).and_then(|v| v.as_str());
            let dst_s = body.get("dst_piece_id").or_else(|| body.get("target_piece_id")).and_then(|v| v.as_str());
            let soldier_ids: Vec<String> = body.get("soldier_ids").and_then(|v| v.as_array()).map(|arr| {
                arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
            }).unwrap_or_default();
            if src_s.is_none() || dst_s.is_none() || soldier_ids.is_empty() {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "missing_transfer_params"})));
            }
            let (Some(&su), Some(&du)) = (w.units.str_to_id.get(src_s.unwrap()), w.units.str_to_id.get(dst_s.unwrap())) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "unit_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, su) { return err; }
            if let Err(err) = authorize_unit_op(&w, player_s, du) { return err; }
            
            let mut moved_soldiers = Vec::new();
            let mut kept = Vec::new();
            for s in w.units.soldiers[su.0].drain(..) {
                if soldier_ids.contains(&s.id) {
                    moved_soldiers.push(s);
                } else {
                    kept.push(s);
                }
            }
            if kept.is_empty() {
                w.units.soldiers[su.0] = kept;
                w.units.soldiers[su.0].append(&mut moved_soldiers);
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "cannot_empty_source_unit"})));
            }
            w.units.soldiers[su.0] = kept;
            w.units.soldiers[du.0].append(&mut moved_soldiers);
            w.units.sync_totals_from_soldiers(su);
            w.units.sync_totals_from_soldiers(du);
            st.mp.lock().unwrap().state_revision += 1;
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "transferred": soldier_ids.len(), "state": st_json})))
        }
        "swap_soldiers" => {
            let u1_s = body.get("src_piece_id").or_else(|| body.get("unit_1_id")).and_then(|v| v.as_str());
            let u2_s = body.get("dst_piece_id").or_else(|| body.get("unit_2_id")).and_then(|v| v.as_str());
            let s1_s = body.get("soldier_1_id").and_then(|v| v.as_str());
            let s2_s = body.get("soldier_2_id").and_then(|v| v.as_str());
            if u1_s.is_none() || u2_s.is_none() || s1_s.is_none() || s2_s.is_none() {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "missing_swap_params"})));
            }
            let (Some(&su), Some(&du)) = (w.units.str_to_id.get(u1_s.unwrap()), w.units.str_to_id.get(u2_s.unwrap())) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "unit_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, su) { return err; }
            if let Err(err) = authorize_unit_op(&w, player_s, du) { return err; }
            let s1_id = s1_s.unwrap();
            let s2_id = s2_s.unwrap();
            let idx1 = w.units.soldiers[su.0].iter().position(|s| s.id == s1_id);
            let idx2 = w.units.soldiers[du.0].iter().position(|s| s.id == s2_id);
            if idx1.is_none() || idx2.is_none() {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "soldier_not_found"})));
            }
            let sol1 = w.units.soldiers[su.0].remove(idx1.unwrap());
            let sol2 = w.units.soldiers[du.0].remove(idx2.unwrap());
            w.units.soldiers[su.0].push(sol2);
            w.units.soldiers[du.0].push(sol1);
            w.units.sync_totals_from_soldiers(su);
            w.units.sync_totals_from_soldiers(du);
            st.mp.lock().unwrap().state_revision += 1;
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "swapped": true, "state": st_json})))
        }
        "reorder_task_queue" => {
            let pid_opt = body.get("piece_id").or_else(|| body.get("target_id")).and_then(|v| v.as_str());
            let Some(pid) = pid_opt else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let uid_opt = if let Some(&c) = w.commanders.str_to_id.get(pid) {
                w.commanders.hq_piece_id[c.0]
            } else {
                w.units.str_to_id.get(pid).copied()
            };
            let Some(uid) = uid_opt else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let from_idx = body.get("from_index").or_else(|| body.get("from_idx")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let to_idx = body.get("to_index").or_else(|| body.get("to_idx")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            match w.reorder_task_queue(uid, from_idx, to_idx) {
                Ok(_) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "form_command_squad" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let pref = body.get("preferred_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let eid_res = {
                let w_mut: &mut SimWorld = &mut *w;
                w_mut.events.start_form_command_squad(&w_mut.units, uid, pref, true, dur_h)
            };
            match eid_res {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "engineering" | "fortify" | "dig_trench" | "lay_mines" | "clear_mines" | "build_bridge" | "repair_terrain" | "repair_airfield" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let eng_op = if op == "engineering" {
                body.get("eng_op").and_then(|v| v.as_str()).unwrap_or("dig_trench")
            } else {
                op
            };
            let fallback_px = w.units.px[uid.0];
            let target_px = body.get("target_px").and_then(|v| v.as_array()).and_then(|a| {
                Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))
            }).unwrap_or(fallback_px);
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let w_mut: &mut SimWorld = &mut *w;
            match w_mut.events.start_engineering(&w_mut.units, uid, eng_op, target_px, dur_h) {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "demolition" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let fallback_px = w.units.px[uid.0];
            let target_px = body.get("target_px").and_then(|v| v.as_array()).and_then(|a| {
                Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))
            }).unwrap_or(fallback_px);
            let target_type = body.get("target_type").and_then(|v| v.as_str()).unwrap_or("bridge");
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let w_mut: &mut SimWorld = &mut *w;
            match w_mut.events.start_demolition(&w_mut.units, uid, target_px, target_type, dur_h) {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "radio_decoy" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let fallback_px = w.units.px[uid.0];
            let target_px = body.get("target_px").and_then(|v| v.as_array()).and_then(|a| {
                Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))
            }).unwrap_or(fallback_px);
            let ttl_h = body.get("ttl_h").and_then(|v| v.as_f64()).unwrap_or(2.0) as f32;
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let w_mut: &mut SimWorld = &mut *w;
            match w_mut.events.start_radio_decoy(&w_mut.units, uid, target_px, ttl_h, dur_h) {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "lay_wire" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let Some(target_px) = body.get("target_px").and_then(|v| v.as_array()).and_then(|a| {
                Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))
            }) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "target_px_required"})));
            };
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let w_mut: &mut SimWorld = &mut *w;
            match w_mut.events.start_lay_wire(&w_mut.units, uid, target_px, dur_h) {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "cargo_transfer" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(tid_s) = body.get("target_piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "target_piece_id_required"})));
            };
            let (Some(&uid), Some(&tuid)) = (w.units.str_to_id.get(pid), w.units.str_to_id.get(tid_s)) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "unit_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            if let Err(err) = authorize_unit_op(&w, player_s, tuid) { return err; }
            let food_t = body.get("food_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let ammo_t = body.get("ammo_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let fuel_t = body.get("fuel_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let w_mut: &mut SimWorld = &mut *w;
            match w_mut.events.start_cargo_transfer(&w_mut.units, uid, tuid, food_t, ammo_t, fuel_t, Vec::new(), dur_h) {
                Ok(eid) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "resupply_pickup" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let food_t = body.get("food_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let ammo_t = body.get("ammo_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let fuel_t = body.get("fuel_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.15) as f32;
            let labor = w.units.persons(uid);
            let eid = w.events.push(uid, dur_h.max(0.05), labor, EventKind::ResupplyPickup { food_t, ammo_t, fuel_t });
            st.mp.lock().unwrap().state_revision += 1;
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
        }
        "deploy_heavy" | "deploy_heavy_unit" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let weapon = body.get("weapon").and_then(|v| v.as_str()).unwrap_or("lmg").to_string();
            if w.units.equipment[uid.0].get(&weapon).copied().unwrap_or(0) == 0 {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": format!("weapon_not_owned:{weapon}")})));
            }
            let unmanned_opt = body.get("unmanned").and_then(|v| v.as_bool());
            let keep_manned = unmanned_opt
                .map(|u| !u)
                .or_else(|| body.get("keep_manned").and_then(|v| v.as_bool()))
                .unwrap_or(true);
            let op_sols: Vec<String> = body
                .get("operator_soldier_ids")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let op_cnt = body.get("operator_count").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
            let immediate = body.get("immediate").and_then(|v| v.as_bool()).unwrap_or(false);
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            if immediate {
                let tag = w.events.next_id + 1;
                w.events.next_id = tag;
                match w.execute_deploy_heavy(uid, &weapon, keep_manned, &op_sols, op_cnt, tag) {
                    Ok(emp) => {
                        st.mp.lock().unwrap().state_revision += 1;
                        let st_json = serialize_state(&w, player_s, None);
                        (StatusCode::OK, Json(json!({"ok": true, "emplacement_id": w.units.str_id[emp.0], "immediate": true, "state": st_json})))
                    }
                    Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
                }
            } else {
                let labor = w.units.persons(uid);
                let eid = w.events.push(uid, dur_h.max(0.05), labor, EventKind::DeployHeavyUnit {
                    weapon,
                    keep_manned,
                    operator_soldier_ids: op_sols,
                    operator_count: op_cnt,
                });
                st.mp.lock().unwrap().state_revision += 1;
                let st_json = serialize_state(&w, player_s, None);
                (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
            }
        }
        "operate_heavy_unit" => {
            let Some(op_pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(tgt_pid) = body.get("target_unit_id").or_else(|| body.get("target_piece_id")).and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "target_unit_id_required"})));
            };
            let (Some(&op_uid), Some(&tgt_uid)) = (w.units.str_to_id.get(op_pid), w.units.str_to_id.get(tgt_pid)) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, op_uid) { return err; }
            if let Err(err) = authorize_unit_op(&w, player_s, tgt_uid) { return err; }
            let op_sols: Vec<String> = body
                .get("operator_soldier_ids")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let op_cnt = body.get("operator_count").and_then(|v| v.as_u64()).unwrap_or(op_sols.len() as u64).max(1) as u32;
            match w.units.set_heavy_operators(tgt_uid, Some(op_uid), op_sols, op_cnt) {
                Ok(_) => {
                    let (eff_crew, _, _, buff_mult) = w.units.effective_operator_stats(tgt_uid);
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({
                        "ok": true,
                        "effective_crew": eff_crew,
                        "operator_buff_mult": buff_mult,
                        "state": st_json
                    })))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "pack_heavy" | "pack_heavy_unit" => {
            let Some(pid) = body.get("piece_id").and_then(|v| v.as_str()) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "piece_id_required"})));
            };
            let Some(&uid) = w.units.str_to_id.get(pid) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "piece_not_found"})));
            };
            if let Err(err) = authorize_unit_op(&w, player_s, uid) { return err; }
            let target_uid = body
                .get("target_unit_id")
                .or_else(|| body.get("target_piece_id"))
                .and_then(|v| v.as_str())
                .and_then(|s| w.units.str_to_id.get(s).copied())
                .unwrap_or(uid);
            if let Err(err) = authorize_unit_op(&w, player_s, target_uid) { return err; }
            let attach = body.get("attach").and_then(|v| v.as_bool()).unwrap_or(false);
            let immediate = body.get("immediate").and_then(|v| v.as_bool()).unwrap_or(false);
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            if immediate {
                match w.execute_pack_heavy(uid, target_uid, attach) {
                    Ok(_) => {
                        st.mp.lock().unwrap().state_revision += 1;
                        let st_json = serialize_state(&w, player_s, None);
                        (StatusCode::OK, Json(json!({"ok": true, "immediate": true, "state": st_json})))
                    }
                    Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
                }
            } else {
                let labor = w.units.persons(uid).max(w.units.effective_operator_stats(uid).0);
                let eid = w.events.push(uid, dur_h.max(0.05), labor, EventKind::PackHeavyUnit { target_unit_id: target_uid, attach });
                st.mp.lock().unwrap().state_revision += 1;
                let st_json = serialize_state(&w, player_s, None);
                (StatusCode::OK, Json(json!({"ok": true, "event_id": eid, "state": st_json})))
            }
        }
        "cancel_event" => {
            let Some(ev_id) = body.get("event_id").and_then(|v| {
                v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            }) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": "event_id_required"})));
            };
            let owner_team = player_s
                .and_then(|ps| w.commanders.str_to_id.get(ps).copied())
                .map(|c| w.commanders.team[c.0])
                .or_else(|| {
                    body.get("piece_id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| w.units.str_to_id.get(s).copied())
                        .map(|u| w.units.team[u.0])
                })
                .unwrap_or(TeamId::Red);
            let w_mut: &mut SimWorld = &mut *w;
            match w_mut.events.cancel_event(&mut w_mut.units, ev_id, owner_team) {
                Ok(_) => {
                    st.mp.lock().unwrap().state_revision += 1;
                    let st_json = serialize_state(&w, player_s, None);
                    (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
                }
                Err(reason) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": reason}))),
            }
        }
        "war_council" => {
            let cid_s = body.get("commander_id").and_then(|v| v.as_str()).or(player_s).unwrap_or("");
            let Some(&cid) = w.commanders.str_to_id.get(cid_s) else {
                return (StatusCode::NOT_FOUND, Json(json!({"ok": false, "reason": "commander_not_found"})));
            };
            if let Err(err) = authorize_commander_op(&w, player_s, cid) { return err; }
            let dur_h = body.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(2.0) as f32;
            if let Some(p_s) = body.get("partner_id").and_then(|v| v.as_str()) {
                if let Some(&pid) = w.commanders.str_to_id.get(p_s) {
                    w.commanders.council_partner_id[cid.0] = Some(pid);
                    w.commanders.council_partner_id[pid.0] = Some(cid);
                    w.commanders.war_council_sync_hours[pid.0] = dur_h;
                }
            }
            w.commanders.war_council_sync_hours[cid.0] = dur_h;
            w.commanders.briefing_buff_hours[cid.0] = dur_h;
            w.commanders.council_ao_hours[cid.0] = dur_h;
            w.commanders.order_churn_score[cid.0] = (w.commanders.order_churn_score[cid.0] - 1.5).max(0.0);
            st.mp.lock().unwrap().state_revision += 1;
            let st_json = serialize_state(&w, player_s, None);
            (StatusCode::OK, Json(json!({"ok": true, "state": st_json})))
        }
        _ => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "reason": format!("unknown_op:{op}")}))),
    }
}

async fn post_commit_turn(State(st): State<AppState>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let sync_barrier = body.get("sync_barrier").and_then(|v| v.as_bool()).unwrap_or(false);
    let force_advance = body.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    {
        let mp = st.mp.lock().unwrap();
        if sync_barrier && !force_advance && mp.seat_tokens.len() > 1 {
            let all_ready = mp.seat_tokens.keys().all(|k| mp.ready_commanders.contains(k));
            if !all_ready {
                let mut res = mp_status_json(&mp);
                res["ok"] = json!(false);
                res["reason"] = json!("waiting_for_other_players_ready");
                return (StatusCode::CONFLICT, Json(res));
            }
        }
    }

    let mut w = st.world.lock().unwrap();
    let player_s = body.get("player_commander_id").and_then(|v| v.as_str());
    let player_cid = player_s.and_then(|s| w.commanders.str_to_id.get(s).copied());

    // Forward queued radio_silence / lay_wire / cargo_transfer payloads attached to turn commit
    if let Some(rs_arr) = body.get("radio_silence").and_then(|v| v.as_array()) {
        for item in rs_arr {
            let eid = item.get("entity_id").or_else(|| item.get("piece_id")).and_then(|v| v.as_str()).unwrap_or("");
            let sil = item.get("radio_silence").and_then(|v| v.as_bool());
            let ch = item.get("radio_channel").and_then(|v| v.as_str()).map(radio_channel_idx);
            if let Some(&uid) = w.units.str_to_id.get(eid) {
                if authorize_unit_op(&w, player_s, uid).is_err() { continue; }
                if let Some(s) = sil { w.units.radio_silence[uid.0] = s; }
                if let Some(c) = ch { w.units.radio_channel[uid.0] = c; }
            } else if let Some(&cid) = w.commanders.str_to_id.get(eid) {
                if authorize_commander_op(&w, player_s, cid).is_err() { continue; }
                if let Some(s) = sil { w.commanders.radio_silence[cid.0] = s; }
                if let Some(c) = ch { w.commanders.radio_channel[cid.0] = c; }
            }
        }
    }
    if let Some(lw_arr) = body.get("lay_wire").and_then(|v| v.as_array()) {
        for item in lw_arr {
            let pid = item.get("piece_id").and_then(|v| v.as_str()).unwrap_or("");
            if let (Some(&uid), Some(tp)) = (
                w.units.str_to_id.get(pid),
                item.get("target_px").and_then(|v| v.as_array()).and_then(|a| Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))),
            ) {
                if authorize_unit_op(&w, player_s, uid).is_err() { continue; }
                let dur = item.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
                let w_mut: &mut SimWorld = &mut *w;
                let _ = w_mut.events.start_lay_wire(&w_mut.units, uid, tp, dur);
            }
        }
    }
    if let Some(ct_arr) = body.get("cargo_transfer").and_then(|v| v.as_array()) {
        for item in ct_arr {
            let pid = item.get("piece_id").and_then(|v| v.as_str()).unwrap_or("");
            let tid = item.get("target_piece_id").and_then(|v| v.as_str()).unwrap_or("");
            if let (Some(&uid), Some(&tuid)) = (w.units.str_to_id.get(pid), w.units.str_to_id.get(tid)) {
                if authorize_unit_op(&w, player_s, uid).is_err() || authorize_unit_op(&w, player_s, tuid).is_err() { continue; }
                let f = item.get("food_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let a = item.get("ammo_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let g = item.get("fuel_t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let dur = item.get("duration_h").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
                let w_mut: &mut SimWorld = &mut *w;
                let _ = w_mut.events.start_cargo_transfer(&w_mut.units, uid, tuid, f, a, g, Vec::new(), dur);
            }
        }
    }

    // Process drafted frontlines
    if let Some(fls) = body.get("typed_frontlines").and_then(|v| v.as_array()) {
        for fl in fls {
            let tgt_s = fl.get("commander_id").and_then(|v| v.as_str()).or(player_s).unwrap_or("");
            let tgt_cid = w.commanders.str_to_id.get(tgt_s).copied().unwrap_or(CommanderId(0));
            if let Some(pc) = player_cid {
                if !can_commander_write_line(pc, tgt_cid) {
                    return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "reason": "forbidden_write_other_commander_line"})));
                }
            }
            let pts: Vec<[f32; 2]> = fl
                .get("points_px")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| {
                            let pa = p.as_array()?;
                            Some([pa.first()?.as_f64()? as f32, pa.get(1)?.as_f64()? as f32])
                        })
                        .collect()
                })
                .unwrap_or_default();
            if pts.len() >= 2 {
                let lid = fl.get("line_id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| format!("fl-t{}-{}", w.turn, w.grid.frontlines.len()));
                let role = fl.get("role").and_then(|v| v.as_str()).map(FrontlineRole::from_str).unwrap_or(FrontlineRole::Deploy);
                let mask = fl.get("unit_type_mask").and_then(|v| v.as_u64()).unwrap_or(LINE_MASK_ALL as u64) as u8;
                let weight = fl.get("attraction_weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                let team = w.commanders.team[tgt_cid.0];
                w.grid.write_typed_frontline(&lid, team, tgt_cid.0, 0, role, 0, 0, mask, weight, None, &pts, 4);
            }
        }
    }

    let hours = body.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
    let rep = w.run_turn_default(hours);
    let mp_snap = {
        let mut mp = st.mp.lock().unwrap();
        mp.state_revision += 1;
        mp.ready_commanders.clear();
        mp_status_json(&mp)
    };
    let mut state = serialize_state(&w, player_s, None);
    state["state_revision"] = mp_snap["state_revision"].clone();
    state["mp"] = mp_snap.clone();
    (StatusCode::OK, Json(json!({"ok": true, "report": rep, "state": state, "mp": mp_snap})))
}

async fn post_import_r16(State(_st): State<AppState>) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({"ok": false, "reason": "use unit_op load_r16_platoon or server CLI --r16-dir"})))
}

async fn get_catalog() -> (StatusCode, Json<Value>) {
    let cat = gentactix_core::catalog::global_catalog();
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "version": cat.version,
            "schema": cat.schema,
            "source": cat.source,
            "count": cat.len(),
            "items": cat.to_raw_items(),
        })),
    )
}

async fn post_catalog_reload() -> (StatusCode, Json<Value>) {
    match gentactix_core::catalog::reload_global_catalog(None) {
        Ok(cat) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "version": cat.version,
                "schema": cat.schema,
                "source": cat.source,
                "count": cat.len(),
                "items": cat.to_raw_items(),
            })),
        ),
        Err(errs) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "errors": errs,
            })),
        ),
    }
}

fn parse_tile_xy(s: &str) -> (u32, u32) {
    let mut it = s.split(',');
    let x = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(1);
    let y = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(1);
    (x, y)
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let tile_xy = parse_tile_xy(&args.tile_xy);
    let world = if args.demo {
        println!("Loaded 64x64 synthetic demo world");
        build_demo_world()
    } else if args.r16 && args.r16_dir.is_dir() {
        match build_r16_platoon_world(&args.r16_dir, tile_xy, args.flow_threshold) {
            Ok(w) => {
                println!(
                    "Loaded 1:1 .r16 world ({}x{}) from {}",
                    w.grid.rows,
                    w.grid.cols,
                    args.r16_dir.display()
                );
                w
            }
            Err(e) => {
                eprintln!("Fallback to 16x16 art test world ({e})");
                build_art_16_world()
            }
        }
    } else {
        println!("Loaded 16x16 art test world (all PLAIN + FOREST vector tree placements)");
        build_art_16_world()
    };

    let cat = gentactix_core::catalog::global_catalog();
    println!(
        "Loaded single-source item catalog v{} ({} items) from {}",
        cat.version,
        cat.len(),
        cat.source
    );

    let state = AppState {
        world: Arc::new(Mutex::new(world)),
        mp: Arc::new(Mutex::new(MultiplayerHub::default())),
        web_index: args.web_index,
        web_assets: args.web_assets,
        r16_dir: args.r16_dir,
    };

    let app = Router::new()
        .route("/", get(get_index))
        .route("/index.html", get(get_index))
        .route("/assets/*path", get(get_asset))
        .route("/api/tile/:z/:r/:c", get(get_tile))
        .route("/api/terrain_raw.bin", get(get_terrain_raw))
        .route("/api/state", get(get_state))
        .route("/api/mp/status", get(get_mp_status))
        .route("/api/mp/claim_seat", post(post_mp_claim_seat))
        .route("/api/mp/ready", post(post_mp_ready))
        .route("/api/validate_ao", post(post_validate_ao))
        .route("/api/weapon_los", post(post_weapon_los))
        .route("/api/estimate_plan_time", post(post_estimate_plan_time))
        .route("/api/edit_pixel", post(post_edit_pixel))
        .route("/api/unit_op", post(post_unit_op))
        .route("/api/commit_turn", post(post_commit_turn))
        .route("/api/import_r16", post(post_import_r16))
        .route("/api/catalog", get(get_catalog))
        .route("/api/catalog/reload", post(post_catalog_reload))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse().expect("invalid host/port");
    println!("Gentactix V3.1 Rust Server listening on http://{}/", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        let mut w = build_demo_world();
        if let Some(i0) = (0..w.units.id.len()).find(|&i| w.units.alive[i] && w.units.team[i] == TeamId::Red) {
            *w.units.equipment[i0].entry("hmg".to_string()).or_insert(0) += 1;
            w.events.push(UnitId(i0), 1.0, 4, EventKind::Engineering {
                op: "fortify".to_string(),
                target_px: w.units.px[i0],
            });
        }
        AppState {
            world: Arc::new(Mutex::new(w)),
            mp: Arc::new(Mutex::new(MultiplayerHub::default())),
            web_index: PathBuf::from(r"D:\tools\gentactix\web\index.html"),
            web_assets: PathBuf::from(r"D:\tools\gentactix\web\assets"),
            r16_dir: PathBuf::from(r"E:\商单1100\New folder"),
        }
    }

    #[tokio::test]
    async fn test_v33_server_contracts_auth_and_estimate() {
        let st = test_state();
        let (red_uid, red_goal, blue_uid, red_cmd) = {
            let w = st.world.lock().unwrap();
            let ri = (0..w.units.id.len()).find(|&i| w.units.alive[i] && w.units.team[i] == TeamId::Red).unwrap();
            let ru = w.units.str_id[ri].clone();
            let rg = [w.units.pos[ri][0] + 0.8, w.units.pos[ri][1]];
            let bu = (0..w.units.id.len()).find(|&i| w.units.alive[i] && w.units.team[i] == TeamId::Blue).map(|i| w.units.str_id[i].clone()).unwrap();
            let rc = w.units.direct_commander_id[ri]
                .map(|c| w.commanders.str_id[c.0].clone())
                .unwrap_or_else(|| {
                    (0..w.commanders.id.len()).find(|&i| w.commanders.alive[i] && w.commanders.team[i] == TeamId::Red).map(|i| w.commanders.str_id[i].clone()).unwrap()
                });
            (ru, rg, bu, rc)
        };

        // 1. /api/estimate_plan_time has NO max(5, ...) floor & returns eta_str
        let (sc, Json(res)) = post_estimate_plan_time(
            State(st.clone()),
            Json(json!({
                "piece_id": red_uid,
                "steps": [{"action": "move", "move_mode": "fast", "goal": red_goal}]
            })),
        )
        .await;
        assert_eq!(sc, StatusCode::OK);
        assert_eq!(res["ok"], json!(true));
        let dur = res["total_min"].as_f64().unwrap_or(99.0);
        assert!(dur > 0.0 && dur < 5.0, "short 1-cell move must not be clamped to >= 5 min, got {dur}");
        assert!(res["steps"][0]["eta_str"].as_str().unwrap_or("").contains("分"));

        // 2. Cross-team unit_op must return 403 FORBIDDEN
        let (sc_auth, _) = post_unit_op(
            State(st.clone()),
            Json(json!({
                "op": "direct_action",
                "player_commander_id": red_cmd,
                "piece_id": blue_uid,
                "action": "stop"
            })),
        )
        .await;
        assert_eq!(sc_auth, StatusCode::FORBIDDEN);

        // 3. Detached unmanned heavy emplacement stays visible in serialize_state & enemy events stay hidden
        let (sc_dep, Json(dep_res)) = post_unit_op(
            State(st.clone()),
            Json(json!({
                "op": "deploy_heavy_unit",
                "player_commander_id": red_cmd,
                "piece_id": red_uid,
                "weapon": "hmg",
                "keep_manned": false,
                "operator_count": 2,
                "immediate": true
            })),
        )
        .await;
        assert_eq!(sc_dep, StatusCode::OK);
        assert_eq!(dep_res["ok"], json!(true));
        let emp_id = dep_res["emplacement_id"].as_str().unwrap().to_string();
        assert!(
            dep_res["state"]["own_pieces"].as_array().unwrap().iter().any(|p| p["id"].as_str() == Some(emp_id.as_str())),
            "unmanned heavy emplacement must remain visible in own_pieces"
        );

        {
            let w = st.world.lock().unwrap();
            let red_st = serialize_state(&w, Some(&red_cmd), Some("red"));
            let red_ev_cnt: usize = red_st["own_pieces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["active_events"].as_array().map_or(0, |a| a.len()))
                .sum();
            assert!(red_ev_cnt > 0, "red commander should see own active events");
            let blue_st = serialize_state(&w, None, Some("blue"));
            let blue_sees_red_ev: usize = blue_st["own_pieces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["active_events"].as_array().map_or(0, |a| a.len()))
                .sum();
            assert_eq!(blue_sees_red_ev, 0, "blue state must never include red active events");
            assert!(blue_st["visible_enemies"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e.get("active_events").is_none()));
        }
    }
}
