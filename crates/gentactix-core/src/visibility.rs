use crate::clock::{check_los, TickReport};
use crate::math_ops::{SchmidtPostureState, WorldPos};
use crate::ruleset::ObjectiveSpec;
use crate::state::{CombatLogEntry, UnitRecord, WorldState};
use crate::time_machine::{
    mix_time_unit_hash, query_time_lut, ParticleTrackNode, PlaybackEvent, TimeCategory,
    TimeMachineFramePayload,
};
use crate::types::{PlatoonId, Team, UnitId, UnitOrder, UnitRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maximum 16s tactical movement skirt outside friendly vision (64.0m = 8 cells).
pub const PRERENDER_SKIRT_MARGIN_M: f32 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisTier {
    Hidden = 0,
    Suspected = 1,
    Spotted = 2,
    Full = 3,
}

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedStateView {
    pub tick: u64,
    pub clock_seconds: u64,
    pub observer_team: Team,
    pub fog_enforced: bool,
    pub units: Vec<VisibleUnitView>,
    pub objectives: Vec<ObjectiveSpec>,
    pub authoritative_sha256: String,
}

pub fn evaluate_unit_visibility(
    world: &WorldState,
    unit: &UnitRecord,
    observer_team: Team,
    recent_logs: &[CombatLogEntry],
) -> VisTier {
    if !world.ruleset.enforce_fog_of_war || unit.team == observer_team || unit.team == Team::Neutral
    {
        return VisTier::Full;
    }
    if !unit.alive {
        return VisTier::Hidden;
    }

    let mut best = VisTier::Hidden;
    for obs in world.units.values() {
        if !obs.alive || obs.team != observer_team {
            continue;
        }
        let los = check_los(&world.terrain, obs.pos, unit.pos, obs.vision_m, world.night);
        if los.visible {
            if los.dist_m <= obs.vision_m * 0.55 && los.concealment < 0.35 {
                return VisTier::Full;
            }
            best = VisTier::Spotted;
        }
    }

    if best == VisTier::Hidden {
        let fired_recently = recent_logs
            .iter()
            .any(|l| l.kind == "fire" && l.actor_id == unit.id);
        if fired_recently {
            let heard = world.units.values().any(|obs| {
                obs.alive && obs.team == observer_team && obs.pos.distance_to(unit.pos) <= obs.vision_m * 1.2
            });
            if heard {
                return VisTier::Suspected;
            }
        }
    }
    best
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
        let pos = if tier == VisTier::Suspected {
            WorldPos::new(
                (unit.pos.x / 16.0).floor() * 16.0 + 8.0,
                (unit.pos.y / 16.0).floor() * 16.0 + 8.0,
            )
        } else {
            unit.pos
        };

        let view = match tier {
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
            },
        };
        units.push(view);
    }

    ObservedStateView {
        tick: world.tick,
        clock_seconds: world.clock_seconds,
        observer_team,
        fog_enforced,
        units,
        objectives: world.objectives.clone(),
        authoritative_sha256: world.authoritative_sha256(),
    }
}

/// Builds Time Machine Egress playback timeline with exact_ms and prerender skirt (visibility=0 ring).
pub fn build_egress_playback_frame(
    world: &WorldState,
    report: &TickReport,
    observer_team: Team,
) -> TimeMachineFramePayload {
    let mut tiers: BTreeMap<u32, VisTier> = BTreeMap::new();
    for (&uid, unit) in &world.units {
        tiers.insert(
            uid,
            evaluate_unit_visibility(world, unit, observer_team, &report.tick_logs),
        );
    }

    let mut particle_tracks = Vec::new();
    for (&uid, unit) in &world.units {
        if !unit.alive {
            continue;
        }
        let pos_end = [unit.pos.x, unit.pos.y];
        let start = report.prior_positions.get(&uid).copied().unwrap_or(unit.pos);
        let pos_start = [start.x, start.y];

        if unit.team == observer_team || !world.ruleset.enforce_fog_of_war {
            particle_tracks.push(ParticleTrackNode {
                piece_id: unit.id.0.to_string(),
                team: unit.team.as_str().to_string(),
                pos_start,
                pos_end,
                initial_visibility: 1,
                is_prerender_skirt: false,
            });
            continue;
        }

        let tier = tiers.get(&uid).copied().unwrap_or(VisTier::Hidden);
        if tier >= VisTier::Spotted {
            particle_tracks.push(ParticleTrackNode {
                piece_id: unit.id.0.to_string(),
                team: unit.team.as_str().to_string(),
                pos_start,
                pos_end,
                initial_visibility: 1,
                is_prerender_skirt: false,
            });
        } else {
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
                });
            }
        }
    }

    let mut playback_events = Vec::new();
    for log in &report.tick_logs {
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

        let actor_name = world
            .units
            .get(&log.actor_id.0)
            .map(|u| u.callsign.as_str())
            .unwrap_or("unknown");
        let cat = if log.kind == "fire" {
            TimeCategory::Bullet
        } else {
            TimeCategory::Event
        };
        let h = mix_time_unit_hash(report.clock_seconds, actor_name);
        let lut = query_time_lut(cat, h);
        playback_events.push(PlaybackEvent {
            exact_ms: lut.exact_ms,
            category: cat,
            event_kind: log.kind.clone(),
            source_piece_id: Some(log.actor_id.0.to_string()),
            target_piece_id: log.target_id.map(|t| t.0.to_string()),
            exact_hp_loss: Some(log.hp_loss.round() as u32),
            exact_supp_add: Some(log.suppression_add),
            entity_id: None,
            directive_id: None,
        });
    }

    TimeMachineFramePayload {
        tick: report.tick,
        window_ms: [0, 16000],
        particle_tracks,
        playback_events,
    }
}
