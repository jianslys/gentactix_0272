use crate::clock::TickReport;
use crate::damage_model::{SoldierRecord, VehicleDamageState};
use crate::math_ops::{SchmidtPostureState, WorldPos};
use crate::ruleset::ObjectiveSpec;
use crate::state::{CombatLogEntry, UnitRecord, WorldState};
use crate::step_cache::{ScheduledEventKind, WorldInstanceKind};
use crate::time_machine::{
    mix_time_unit_hash, query_time_lut, ParticleTrackNode, PlaybackEvent, TimeCategory,
    TimeMachineFramePayload, UnitTickBaseline,
};
pub use crate::types::VisTier;
use crate::types::{PlatoonId, Team, UnitId, UnitOrder, UnitRole};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum 16s tactical movement skirt outside friendly vision (64.0m = 8 cells).
pub const PRERENDER_SKIRT_MARGIN_M: f32 = 64.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibleUnitView {
    pub id: UnitId,
    pub callsign: String,
    pub team: Team,
    pub platoon_id: Option<PlatoonId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<UnitRole>,
    pub pos: WorldPos,
    pub facing_deg: f32,
    pub alive: bool,
    pub vis_tier: VisTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub men: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppression: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posture: Option<SchmidtPostureState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_rounds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<UnitOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_vehicle: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soldiers: Option<Vec<SoldierRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle_damage: Option<VehicleDamageState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibleInstanceView {
    pub instance_id: u64,
    pub kind: WorldInstanceKind,
    pub team: Team,
    pub pos: WorldPos,
    pub vis_tier: VisTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_unit_id: Option<UnitId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledEventSummary {
    pub event_id: u64,
    pub source_tick: u64,
    pub target_tick: u64,
    pub sec_in_tick: u32,
    pub exact_ms: u32,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_unit_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_pos: Option<WorldPos>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedStateView {
    pub tick: u64,
    pub clock_seconds: u64,
    pub observer_team: Team,
    pub fog_enforced: bool,
    pub units: Vec<VisibleUnitView>,
    #[serde(default)]
    pub visible_instances: Vec<VisibleInstanceView>,
    #[serde(default)]
    pub step_cache_queue: Vec<ScheduledEventSummary>,
    pub objectives: Vec<ObjectiveSpec>,
    pub authoritative_sha256: String,
}

/// Evaluates cell visibility purely from the cognitive I-field (`team_i_vis_tier` backed by `I_intel`).
pub fn evaluate_cell_visibility(
    world: &WorldState,
    pos: WorldPos,
    observer_team: Team,
) -> VisTier {
    if !world.ruleset.enforce_fog_of_war {
        return VisTier::Full;
    }
    let Some(t_idx) = observer_team.index() else {
        return VisTier::Full;
    };
    let Ok(cell) = world.terrain.geom.world_to_cell(pos) else {
        return VisTier::Hidden;
    };
    let Ok(cidx) = world.terrain.idx(cell) else {
        return VisTier::Hidden;
    };
    world
        .team_i_vis_tier
        .get(t_idx)
        .and_then(|grid| grid.get(cidx).copied())
        .unwrap_or(VisTier::Hidden)
}

pub fn evaluate_unit_visibility(
    world: &WorldState,
    unit: &UnitRecord,
    observer_team: Team,
    _recent_logs: &[CombatLogEntry],
) -> VisTier {
    if !world.ruleset.enforce_fog_of_war || unit.team == observer_team || unit.team == Team::Neutral
    {
        return VisTier::Full;
    }
    if !unit.alive {
        return VisTier::Hidden;
    }
    evaluate_cell_visibility(world, unit.pos, observer_team)
}

fn build_visible_unit_view(
    unit: &UnitRecord,
    tier: VisTier,
    is_friendly_or_open: bool,
    include_detailed_damage: bool,
) -> VisibleUnitView {
    let pos = if tier == VisTier::Suspected {
        WorldPos::new(
            (unit.pos.x / 16.0).floor() * 16.0 + 8.0,
            (unit.pos.y / 16.0).floor() * 16.0 + 8.0,
        )
    } else {
        unit.pos
    };
    let (soldiers, vehicle_damage) = if include_detailed_damage && tier == VisTier::Full && is_friendly_or_open {
        if unit.is_vehicle {
            (None, unit.vehicle_damage.clone())
        } else {
            (Some(unit.soldiers.clone()), None)
        }
    } else {
        (None, None)
    };

    match tier {
        VisTier::Full if is_friendly_or_open => VisibleUnitView {
            id: unit.id,
            callsign: unit.callsign.clone(),
            team: unit.team,
            platoon_id: unit.platoon_id,
            role: Some(unit.role),
            pos,
            facing_deg: unit.facing_deg,
            alive: unit.alive,
            vis_tier: VisTier::Full,
            men: Some(unit.men),
            hp: Some(unit.hp),
            suppression: Some(unit.suppression),
            posture: Some(unit.posture),
            weapon_id: Some(unit.weapon_id.clone()),
            ammo_rounds: Some(unit.ammo_rounds),
            order: Some(unit.order.clone()),
            is_vehicle: Some(unit.is_vehicle),
            soldiers,
            vehicle_damage,
        },
        VisTier::Full => VisibleUnitView {
            id: unit.id,
            callsign: unit.callsign.clone(),
            team: unit.team,
            platoon_id: unit.platoon_id,
            role: Some(unit.role),
            pos,
            facing_deg: unit.facing_deg,
            alive: unit.alive,
            vis_tier: VisTier::Full,
            men: Some(unit.men),
            hp: Some(unit.hp),
            suppression: Some(unit.suppression),
            posture: Some(unit.posture),
            weapon_id: Some(unit.weapon_id.clone()),
            ammo_rounds: None,
            order: None,
            is_vehicle: Some(unit.is_vehicle),
            soldiers: None,
            vehicle_damage: None,
        },
        VisTier::Spotted => VisibleUnitView {
            id: unit.id,
            callsign: unit.callsign.clone(),
            team: unit.team,
            platoon_id: None,
            role: Some(unit.role),
            pos,
            facing_deg: unit.facing_deg,
            alive: unit.alive,
            vis_tier: VisTier::Spotted,
            men: None,
            hp: None,
            suppression: None,
            posture: None,
            weapon_id: None,
            ammo_rounds: None,
            order: None,
            is_vehicle: None,
            soldiers: None,
            vehicle_damage: None,
        },
        VisTier::Suspected | VisTier::Hidden => VisibleUnitView {
            id: unit.id,
            callsign: format!("Unknown-{}", unit.id.0),
            team: unit.team,
            platoon_id: None,
            role: None,
            pos,
            facing_deg: 0.0,
            alive: unit.alive,
            vis_tier: VisTier::Suspected,
            men: None,
            hp: None,
            suppression: None,
            posture: None,
            weapon_id: None,
            ammo_rounds: None,
            order: None,
            is_vehicle: None,
            soldiers: None,
            vehicle_damage: None,
        },
    }
}

pub fn build_observed_unit_detail(
    world: &WorldState,
    observer_team: Team,
    unit_id: UnitId,
    recent_logs: &[CombatLogEntry],
) -> Option<VisibleUnitView> {
    let unit = world.units.get(&unit_id.0)?;
    if unit.team == observer_team && !unit.alive {
        return None;
    }
    let tier = evaluate_unit_visibility(world, unit, observer_team, recent_logs);
    if tier == VisTier::Hidden {
        return None;
    }
    let is_friendly_or_open = !world.ruleset.enforce_fog_of_war || unit.team == observer_team;
    Some(build_visible_unit_view(unit, tier, is_friendly_or_open, true))
}

pub fn build_observed_view(
    world: &WorldState,
    observer_team: Team,
    recent_logs: &[CombatLogEntry],
) -> ObservedStateView {
    let fog_enforced = world.ruleset.enforce_fog_of_war;
    let mut units = Vec::new();

    for unit in world.units.values() {
        if unit.team == observer_team && !unit.alive {
            continue;
        }
        let tier = evaluate_unit_visibility(world, unit, observer_team, recent_logs);
        if tier == VisTier::Hidden {
            continue;
        }
        let is_friendly_or_open = !fog_enforced || unit.team == observer_team;
        units.push(build_visible_unit_view(unit, tier, is_friendly_or_open, false));
    }

    let mut visible_instances = Vec::new();
    for inst in world.world_instances.values() {
        if !inst.active {
            continue;
        }
        let tier = if !fog_enforced || inst.team == observer_team {
            VisTier::Full
        } else {
            evaluate_cell_visibility(world, inst.pos, observer_team)
        };
        if tier == VisTier::Hidden {
            continue;
        }
        let pos = if tier == VisTier::Suspected {
            WorldPos::new(
                (inst.pos.x / 16.0).floor() * 16.0 + 8.0,
                (inst.pos.y / 16.0).floor() * 16.0 + 8.0,
            )
        } else {
            inst.pos
        };
        visible_instances.push(VisibleInstanceView {
            instance_id: inst.instance_id,
            kind: inst.kind,
            team: inst.team,
            pos,
            vis_tier: tier,
            label: if tier >= VisTier::Spotted {
                Some(inst.label.clone())
            } else {
                None
            },
            owner_unit_id: if tier >= VisTier::Full && inst.team == observer_team {
                inst.owner_unit_id
            } else {
                None
            },
        });
    }

    let mut step_cache_queue = Vec::new();
    for bucket in world.step_cache.values() {
        for ev in bucket {
            if fog_enforced && ev.team != observer_team && ev.team != Team::Neutral {
                continue;
            }
            let (kind, subject_unit_id, target_pos, label) = match &ev.kind {
                ScheduledEventKind::CommandTicket {
                    unit_name,
                    command,
                    ..
                } => (
                    "task_command".to_string(),
                    command.subject_unit_id().map(|u| u.0),
                    None,
                    Some(unit_name.clone()),
                ),
                ScheduledEventKind::TimedDetonation {
                    instance_id,
                    pos,
                    ..
                } => (
                    "detonation".to_string(),
                    None,
                    Some(*pos),
                    Some(format!("charge_{instance_id}")),
                ),
                ScheduledEventKind::CommanderWave { wave_id, zones, .. } => (
                    "g_field_wave".to_string(),
                    None,
                    zones.first().map(|z| z.center),
                    Some(format!("wave_{wave_id}")),
                ),
            };
            step_cache_queue.push(ScheduledEventSummary {
                event_id: ev.event_id,
                source_tick: ev.source_tick,
                target_tick: ev.target_tick,
                sec_in_tick: ((ev.exact_ms.saturating_sub(1)) / 1000) + 1,
                exact_ms: ev.exact_ms,
                kind,
                subject_unit_id,
                target_pos,
                label,
            });
        }
    }

    ObservedStateView {
        tick: world.tick,
        clock_seconds: world.clock_seconds,
        observer_team,
        fog_enforced,
        units,
        visible_instances,
        step_cache_queue,
        objectives: world.objectives.clone(),
        authoritative_sha256: world.authoritative_sha256(),
    }
}

/// Builds Time Machine Egress playback timeline with t=0s baselines, segmented keyframes,
/// mid-tick casualty stop_exact_ms clamping, and prerender skirt (visibility=0 ring).
pub fn build_egress_playback_frame(
    world: &WorldState,
    report: &TickReport,
    observer_team: Team,
) -> TimeMachineFramePayload {
    let fog_enforced = world.ruleset.enforce_fog_of_war;
    let mut tiers: BTreeMap<u32, VisTier> = BTreeMap::new();
    for (&uid, unit) in &world.units {
        let tier = if unit.alive {
            evaluate_unit_visibility(world, unit, observer_team, &report.tick_logs)
        } else if report.unit_start_baselines.contains_key(&uid) {
            if !fog_enforced || unit.team == observer_team || unit.team == Team::Neutral {
                VisTier::Full
            } else {
                let start = report.prior_positions.get(&uid).copied().unwrap_or(unit.pos);
                evaluate_cell_visibility(world, unit.pos, observer_team)
                    .max(evaluate_cell_visibility(world, start, observer_team))
            }
        } else {
            VisTier::Hidden
        };
        tiers.insert(uid, tier);
    }

    let mut playback_events = Vec::new();
    let mut causal_stop_ms: BTreeMap<u32, u32> = BTreeMap::new();
    for log in &report.tick_logs {
        let actor_name = world
            .units
            .get(&log.actor_id.0)
            .map(|u| u.callsign.as_str())
            .unwrap_or("unknown");
        let cat = match log.kind.as_str() {
            "fire" => TimeCategory::Bullet,
            "bleed_out" | "smoke" | "engineer_work" | "mine_trigger" => TimeCategory::Event,
            _ => TimeCategory::Shell,
        };
        let h = mix_time_unit_hash(report.clock_seconds, actor_name);
        let lut = query_time_lut(cat, h);

        if log.casualties > 0 || log.hp_loss > 0.0 {
            if let Some(tid) = log.target_id {
                causal_stop_ms
                    .entry(tid.0)
                    .and_modify(|m| *m = (*m).min(lut.exact_ms))
                    .or_insert(lut.exact_ms);
            } else {
                causal_stop_ms
                    .entry(log.actor_id.0)
                    .and_modify(|m| *m = (*m).min(lut.exact_ms))
                    .or_insert(lut.exact_ms);
            }
        }

        let actor_vis = tiers.get(&log.actor_id.0).copied().unwrap_or(VisTier::Hidden);
        let target_vis = log
            .target_id
            .and_then(|t| tiers.get(&t.0).copied())
            .unwrap_or(VisTier::Hidden);
        let actor_friendly = world
            .units
            .get(&log.actor_id.0)
            .is_some_and(|u| u.team == observer_team);
        let target_friendly = log
            .target_id
            .and_then(|t| world.units.get(&t.0))
            .is_some_and(|u| u.team == observer_team);
        if !actor_friendly
            && !target_friendly
            && actor_vis < VisTier::Suspected
            && target_vis < VisTier::Suspected
        {
            continue;
        }

        let terrain_cell = log
            .target_id
            .and_then(|t| world.units.get(&t.0))
            .and_then(|u| world.terrain.geom.world_to_cell(u.pos).ok())
            .map(|c| [c.r, c.c]);

        playback_events.push(PlaybackEvent {
            exact_ms: lut.exact_ms,
            category: cat,
            event_kind: log.kind.clone(),
            source_piece_id: Some(log.actor_id.0.to_string()),
            target_piece_id: log.target_id.map(|t| t.0.to_string()),
            exact_hp_loss: Some(log.hp_loss.round() as u32),
            exact_supp_add: Some(log.suppression_add),
            hp_delta: Some(log.hp_loss),
            casualties: Some(log.casualties),
            ammo_spent: (!fog_enforced || actor_friendly).then_some(log.ammo_spent),
            terrain_cell,
            entity_id: None,
            directive_id: None,
        });
    }
    playback_events.sort_by_key(|ev| ev.exact_ms);

    let mut all_ids: BTreeSet<u32> = world.units.keys().copied().collect();
    all_ids.extend(report.unit_start_baselines.keys().copied());

    let mut unit_baselines = Vec::new();
    let mut particle_tracks = Vec::new();
    for uid in all_ids {
        let Some(unit) = world.units.get(&uid) else {
            continue;
        };
        let start_base = report.unit_start_baselines.get(&uid).copied();
        if !unit.alive && start_base.is_none() {
            continue;
        }
        let tier = tiers.get(&uid).copied().unwrap_or(VisTier::Hidden);
        let is_friendly_or_open = !fog_enforced || unit.team == observer_team;
        let is_visible = is_friendly_or_open || tier >= VisTier::Spotted;

        if is_visible {
            if let Some(sb) = start_base {
                let reveal_stats = is_friendly_or_open || tier == VisTier::Full;
                unit_baselines.push(UnitTickBaseline {
                    piece_id: unit.id.0.to_string(),
                    team: unit.team.as_str().to_string(),
                    hp_start: reveal_stats.then_some(sb.hp),
                    men_start: reveal_stats.then_some(sb.men),
                    ammo_start: is_friendly_or_open.then_some(sb.ammo_rounds),
                    suppression_start: reveal_stats.then_some(sb.suppression),
                });
            }
        }

        let start = report.prior_positions.get(&uid).copied().unwrap_or(unit.pos);
        let pos_start = [start.x, start.y];
        let pos_end = [unit.pos.x, unit.pos.y];
        let immobilized_mid_tick = !unit.alive
            || start_base.is_some_and(|sb| sb.eff_speed_scale > 0.0 && unit.effective_speed_scale() <= 0.0);
        let stop_exact_ms = if immobilized_mid_tick {
            Some(causal_stop_ms.get(&uid).copied().unwrap_or(16_000).clamp(1, 16_000))
        } else {
            None
        };
        let end_ms = stop_exact_ms.unwrap_or(16_000).max(1);
        let keyframes = build_track_keyframes(
            start,
            report.motion_waypoints.get(&uid).map(Vec::as_slice).unwrap_or(&[]),
            unit.pos,
            end_ms,
        );

        if is_visible {
            particle_tracks.push(ParticleTrackNode {
                piece_id: unit.id.0.to_string(),
                team: unit.team.as_str().to_string(),
                pos_start,
                pos_end,
                initial_visibility: 1,
                is_prerender_skirt: false,
                keyframes,
                stop_exact_ms,
            });
        } else if unit.alive {
            let in_skirt = world.units.values().any(|obs| {
                obs.alive
                    && obs.team == observer_team
                    && obs.pos.distance_to(unit.pos) <= obs.vision_m + PRERENDER_SKIRT_MARGIN_M
            });
            if in_skirt {
                particle_tracks.push(ParticleTrackNode {
                    piece_id: unit.id.0.to_string(),
                    team: unit.team.as_str().to_string(),
                    pos_start,
                    pos_end,
                    initial_visibility: 0,
                    is_prerender_skirt: true,
                    keyframes,
                    stop_exact_ms: None,
                });
            }
        }
    }

    TimeMachineFramePayload {
        tick: report.tick,
        window_ms: [0, 16000],
        unit_baselines,
        particle_tracks,
        playback_events,
    }
}

fn build_track_keyframes(
    start: WorldPos,
    mid_waypoints: &[WorldPos],
    end: WorldPos,
    end_ms: u32,
) -> Vec<(u32, [f32; 2])> {
    let mut pts = Vec::with_capacity(mid_waypoints.len() + 2);
    pts.push(start);
    for &wp in mid_waypoints {
        if pts.last().is_some_and(|p| p.distance_to(wp) > 0.25) && wp.distance_to(end) > 0.25 {
            pts.push(wp);
        }
    }
    if pts.last().is_some_and(|p| p.distance_to(end) > 0.25) {
        pts.push(end);
    }

    if pts.len() <= 1 {
        let p0 = [start.x, start.y];
        return vec![(0, p0), (end_ms, p0)];
    }

    let mut total_len = 0.0f32;
    let mut cum_lens = Vec::with_capacity(pts.len());
    cum_lens.push(0.0f32);
    for w in pts.windows(2) {
        total_len += w[0].distance_to(w[1]);
        cum_lens.push(total_len);
    }
    let denom = total_len.max(1e-4);
    pts.iter()
        .zip(cum_lens.iter())
        .enumerate()
        .map(|(i, (p, &cum))| {
            let t = if i == 0 {
                0
            } else if i + 1 == pts.len() {
                end_ms
            } else {
                ((cum / denom) * (end_ms as f32)).round().clamp(1.0, (end_ms - 1).max(1) as f32) as u32
            };
            (t, [p.x, p.y])
        })
        .collect()
}
