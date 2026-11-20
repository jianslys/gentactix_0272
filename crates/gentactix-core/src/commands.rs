use crate::catalog::ItemCatalog;
use crate::clock::ballistics_field::{resolve_and_commit_radial_blast, RadialBlastField};
use crate::clock::generate_work_points;
use crate::math_ops::{CellCoord, DetonatorSwitchEntity, WorldPos};
use crate::ruleset::{SimRuleset, UnitSpawnSpec};
use crate::state::WorldState;
use crate::step_cache::{
    ScheduledEventKind, ScheduledStepEvent, WaveZoneSpec, WorldInstanceKind,
    MAX_STEP_CACHE_HORIZON_TICKS,
};
use crate::terrain::TerrainKind;
use crate::time_machine::{mix_time_unit_hash, query_time_lut, TimeCategory};
use crate::types::{
    EngineerWorkKind, FireDiscipline, MoveStance, Team, UnitId, UnitOrder,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TacticalCommand {
    MoveUnit {
        unit_id: UnitId,
        waypoints: Vec<WorldPos>,
        #[serde(default)]
        stance: MoveStance,
    },
    AttackPos {
        unit_id: UnitId,
        target: WorldPos,
        #[serde(default)]
        fire_mode: FireDiscipline,
    },
    AttackUnit {
        unit_id: UnitId,
        target_id: UnitId,
        #[serde(default)]
        chase: bool,
        #[serde(default)]
        fire_mode: FireDiscipline,
    },
    HoldUnit {
        unit_id: UnitId,
    },
    SetFireDiscipline {
        unit_id: UnitId,
        discipline: FireDiscipline,
    },
    PopSmoke {
        unit_id: UnitId,
        target: WorldPos,
    },
    BuildWork {
        unit_id: UnitId,
        work: EngineerWorkKind,
        waypoints: Vec<WorldPos>,
    },
    ArmDetonator {
        unit_id: UnitId,
        switch_pos: WorldPos,
        charge_positions: Vec<WorldPos>,
        wire_polyline: Vec<WorldPos>,
    },
    TriggerDetonator {
        unit_id: UnitId,
        switch_id: u64,
    },
    SpawnPlatoon {
        name: String,
        team: Team,
    },
    SpawnUnit {
        spec: Box<UnitSpawnSpec>,
    },
    DespawnUnit {
        unit_id: UnitId,
    },
    EditTerrain {
        cell: CellCoord,
        #[serde(default)]
        elev_delta: Option<f32>,
        #[serde(default)]
        terrain_kind: Option<TerrainKind>,
        #[serde(default)]
        cover_add: Option<f32>,
        #[serde(default)]
        smoke_add: Option<f32>,
    },
    SetRuleset {
        ruleset: SimRuleset,
    },
    SetNight {
        night: bool,
    },
    ScheduleTimedCharge {
        unit_id: UnitId,
        pos: WorldPos,
        delay_ticks: u64,
        #[serde(default = "default_charge_radius")]
        blast_radius_m: f32,
        #[serde(default = "default_charge_kpa")]
        blast_kpa: f32,
        #[serde(default = "default_charge_crater")]
        crater_depth_m: f32,
    },
    CancelStepEvent {
        event_id: u64,
    },
    IssueCommanderWave {
        team: Team,
        #[serde(default)]
        hq_unit_id: Option<UnitId>,
        delay_ticks: u64,
        zones: Vec<WaveZoneSpec>,
    },
}

const fn default_charge_radius() -> f32 {
    24.0
}
const fn default_charge_kpa() -> f32 {
    140.0
}
const fn default_charge_crater() -> f32 {
    2.0
}

impl TacticalCommand {
    #[inline]
    pub const fn time_category(&self) -> Option<TimeCategory> {
        match self {
            Self::MoveUnit { .. } => Some(TimeCategory::Movement),
            Self::AttackPos { .. } | Self::AttackUnit { .. } => Some(TimeCategory::Bullet),
            Self::HoldUnit { .. } | Self::SetFireDiscipline { .. } | Self::PopSmoke { .. } => {
                Some(TimeCategory::Command)
            }
            Self::BuildWork { .. } | Self::ArmDetonator { .. } | Self::TriggerDetonator { .. } => {
                Some(TimeCategory::Event)
            }
            Self::SpawnPlatoon { .. }
            | Self::SpawnUnit { .. }
            | Self::DespawnUnit { .. }
            | Self::EditTerrain { .. }
            | Self::SetRuleset { .. }
            | Self::SetNight { .. }
            | Self::ScheduleTimedCharge { .. }
            | Self::CancelStepEvent { .. }
            | Self::IssueCommanderWave { .. } => None,
        }
    }

    #[inline]
    pub const fn subject_unit_id(&self) -> Option<UnitId> {
        match self {
            Self::MoveUnit { unit_id, .. }
            | Self::AttackPos { unit_id, .. }
            | Self::AttackUnit { unit_id, .. }
            | Self::HoldUnit { unit_id }
            | Self::SetFireDiscipline { unit_id, .. }
            | Self::PopSmoke { unit_id, .. }
            | Self::BuildWork { unit_id, .. }
            | Self::ArmDetonator { unit_id, .. }
            | Self::TriggerDetonator { unit_id, .. }
            | Self::ScheduleTimedCharge { unit_id, .. } => Some(*unit_id),
            Self::IssueCommanderWave { hq_unit_id, .. } => *hq_unit_id,
            Self::SpawnPlatoon { .. }
            | Self::SpawnUnit { .. }
            | Self::DespawnUnit { .. }
            | Self::EditTerrain { .. }
            | Self::SetRuleset { .. }
            | Self::SetNight { .. }
            | Self::CancelStepEvent { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deferred_until_tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_delay_ticks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_runner: Option<bool>,
    pub code: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_id: Option<u32>,
}

impl CommandOutcome {
    pub(crate) fn ok(code: &str, detail: impl Into<String>, created_id: Option<u32>) -> Self {
        Self {
            accepted: true,
            deferred_until_tick: None,
            ticket_id: None,
            remote_delay_ticks: None,
            via_runner: None,
            code: code.to_string(),
            detail: detail.into(),
            created_id,
        }
    }

    pub fn reject(code: &str, detail: impl Into<String>) -> Self {
        Self {
            accepted: false,
            deferred_until_tick: None,
            ticket_id: None,
            remote_delay_ticks: None,
            via_runner: None,
            code: code.to_string(),
            detail: detail.into(),
            created_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAccessOffset {
    pub delay_ticks: u64,
    pub via_runner: bool,
    pub detail: String,
}

pub fn validate_tactical_command_access(
    world: &WorldState,
    cmd: &TacticalCommand,
) -> Result<CommandAccessOffset, (String, String)> {
    let Some(uid) = cmd.subject_unit_id() else {
        return Ok(CommandAccessOffset {
            delay_ticks: 0,
            via_runner: false,
            detail: String::new(),
        });
    };
    let Some(unit) = world.units.get(&uid.0) else {
        return Err(("UNIT_NOT_FOUND".into(), format!("unknown unit {}", uid.0)));
    };
    if !unit.alive {
        return Err(("UNIT_DESTROYED".into(), format!("unit {} is destroyed", uid.0)));
    }
    if world.ruleset.enforce_command_radius {
        if let Some(pid) = unit.platoon_id {
            if let Some(hq_uid) = world.platoons.get(&pid.0).and_then(|p| p.hq_unit) {
                if hq_uid != uid {
                    if let Some(hq) = world.units.get(&hq_uid.0).filter(|h| h.alive) {
                        let dist = hq.pos.distance_to(unit.pos);
                        let direct_limit = hq.effective_radio_range_m.max(8.0);
                        if dist > direct_limit {
                            let eta = if unit.base_radio_range_m > 1.0 {
                                (unit.effective_radio_range_m / unit.base_radio_range_m).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let runner_ticks = ((dist / 64.0).ceil() as u64)
                                .clamp(1, MAX_STEP_CACHE_HORIZON_TICKS);
                            if eta <= 0.05 {
                                return Ok(CommandAccessOffset {
                                    delay_ticks: runner_ticks,
                                    via_runner: true,
                                    detail: format!(
                                        "remote runner courier +{runner_ticks} ticks (+{}s), radio blackout (dist={dist:.1}m)",
                                        runner_ticks * 16
                                    ),
                                });
                            }
                            let excess = dist - direct_limit;
                            let radio_ticks = ((excess / (80.0 * eta)).ceil() as u64)
                                .clamp(1, MAX_STEP_CACHE_HORIZON_TICKS);
                            if runner_ticks < radio_ticks {
                                return Ok(CommandAccessOffset {
                                    delay_ticks: runner_ticks,
                                    via_runner: true,
                                    detail: format!(
                                        "remote runner courier +{runner_ticks} ticks (+{}s, dist={dist:.1}m > {direct_limit:.1}m)",
                                        runner_ticks * 16
                                    ),
                                });
                            }
                            return Ok(CommandAccessOffset {
                                delay_ticks: radio_ticks,
                                via_runner: false,
                                detail: format!(
                                    "remote radio relay +{radio_ticks} ticks (+{}s, eta={eta:.2}, dist={dist:.1}m > {direct_limit:.1}m)",
                                    radio_ticks * 16
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(CommandAccessOffset {
        delay_ticks: 0,
        via_runner: false,
        detail: String::new(),
    })
}

pub fn execute_command_immediate(
    world: &mut WorldState,
    catalog: &ItemCatalog,
    cmd: &TacticalCommand,
) -> CommandOutcome {
    if let Err((code, detail)) = validate_tactical_command_access(world, cmd) {
        return CommandOutcome::reject(&code, detail);
    }

    match cmd {
        TacticalCommand::MoveUnit {
            unit_id,
            waypoints,
            stance,
        } => {
            if waypoints.is_empty() {
                return CommandOutcome::reject("EMPTY_WAYPOINTS", "move requires >= 1 waypoint");
            }
            for &wp in waypoints {
                if let Err(e) = world.terrain.geom.world_to_cell(wp) {
                    return CommandOutcome::reject("OUT_OF_BOUNDS", e.to_string());
                }
            }
            let u = world.units.get_mut(&unit_id.0).unwrap();
            u.order = UnitOrder::Move {
                waypoints: waypoints.clone(),
                stance: *stance,
            };
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("ORDER_ACCEPTED", "move order set", None)
        }
        TacticalCommand::AttackPos {
            unit_id,
            target,
            fire_mode,
        } => {
            if let Err(e) = world.terrain.geom.world_to_cell(*target) {
                return CommandOutcome::reject("OUT_OF_BOUNDS", e.to_string());
            }
            let u = world.units.get_mut(&unit_id.0).unwrap();
            u.fire_discipline = *fire_mode;
            u.order = UnitOrder::AttackPos {
                target: *target,
                fire_mode: *fire_mode,
            };
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("ORDER_ACCEPTED", "attack_pos order set", None)
        }
        TacticalCommand::AttackUnit {
            unit_id,
            target_id,
            chase,
            fire_mode,
        } => {
            let Some(tgt) = world.units.get(&target_id.0) else {
                return CommandOutcome::reject("TARGET_NOT_FOUND", "target unit missing");
            };
            if !tgt.alive {
                return CommandOutcome::reject("TARGET_DESTROYED", "target already destroyed");
            }
            let u = world.units.get_mut(&unit_id.0).unwrap();
            u.fire_discipline = *fire_mode;
            u.order = UnitOrder::AttackUnit {
                target: *target_id,
                chase: *chase,
                fire_mode: *fire_mode,
            };
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("ORDER_ACCEPTED", "attack_unit order set", None)
        }
        TacticalCommand::HoldUnit { unit_id } => {
            world.units.get_mut(&unit_id.0).unwrap().order = UnitOrder::Hold;
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("ORDER_ACCEPTED", "unit holding", None)
        }
        TacticalCommand::SetFireDiscipline {
            unit_id,
            discipline,
        } => {
            world.units.get_mut(&unit_id.0).unwrap().fire_discipline = *discipline;
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("DISCIPLINE_SET", "fire discipline updated", None)
        }
        TacticalCommand::PopSmoke { unit_id, target } => {
            if let Err(e) = world.terrain.geom.world_to_cell(*target) {
                return CommandOutcome::reject("OUT_OF_BOUNDS", e.to_string());
            }
            let u = world.units.get_mut(&unit_id.0).unwrap();
            if u.smoke_grenades == 0 {
                return CommandOutcome::reject("NO_SMOKE_GRENADE", "no smoke grenades left");
            }
            u.order = UnitOrder::PopSmoke { target: *target };
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("SMOKE_ORDERED", "smoke order queued", None)
        }
        TacticalCommand::BuildWork {
            unit_id,
            work,
            waypoints,
        } => {
            if waypoints.is_empty() {
                return CommandOutcome::reject("EMPTY_WAYPOINTS", "build_work needs waypoints");
            }
            for &wp in waypoints {
                if let Err(e) = world.terrain.geom.world_to_cell(wp) {
                    return CommandOutcome::reject("OUT_OF_BOUNDS", e.to_string());
                }
            }
            world.engineer_integrators.remove(&unit_id.0);
            for pt in generate_work_points(*work, waypoints) {
                if let Ok(cell) = world.terrain.geom.world_to_cell(pt) {
                    if let Ok(cidx) = world.terrain.idx(cell) {
                        world.g_work_remaining.entry(cidx).or_insert(40.0);
                    }
                }
            }
            world.units.get_mut(&unit_id.0).unwrap().order = UnitOrder::BuildWork {
                work: *work,
                waypoints: waypoints.clone(),
            };
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("BUILD_WORK_ORDERED", "engineer work assigned", None)
        }
        TacticalCommand::ArmDetonator {
            unit_id,
            switch_pos,
            charge_positions,
            wire_polyline,
        } => execute_arm_detonator(world, *unit_id, *switch_pos, charge_positions, wire_polyline),
        TacticalCommand::TriggerDetonator { unit_id, switch_id } => {
            execute_trigger_detonator(world, *unit_id, *switch_id)
        }
        TacticalCommand::SpawnPlatoon { name, team } => {
            let pid = world.spawn_platoon(name.clone(), *team);
            CommandOutcome::ok("PLATOON_SPAWNED", format!("platoon {}", pid.0), Some(pid.0))
        }
        TacticalCommand::SpawnUnit { spec } => match world.spawn_unit(spec, catalog) {
            Ok(uid) => CommandOutcome::ok("UNIT_SPAWNED", format!("unit {}", uid.0), Some(uid.0)),
            Err(e) => CommandOutcome::reject("SPAWN_FAILED", e),
        },
        TacticalCommand::DespawnUnit { unit_id } => {
            if world.despawn_unit(*unit_id) {
                CommandOutcome::ok("UNIT_DESPAWNED", format!("unit {} removed", unit_id.0), None)
            } else {
                CommandOutcome::reject("UNIT_NOT_FOUND", "unknown unit")
            }
        }
        TacticalCommand::EditTerrain {
            cell,
            elev_delta,
            terrain_kind,
            cover_add,
            smoke_add,
        } => {
            if !world.ruleset.allow_runtime_map_edit {
                return CommandOutcome::reject(
                    "MAP_EDIT_DISABLED",
                    "ruleset prohibits runtime map editing",
                );
            }
            match world
                .terrain
                .mutate_cell(*cell, *elev_delta, *terrain_kind, *cover_add, *smoke_add)
            {
                Ok(()) => CommandOutcome::ok("TERRAIN_EDITED", "cell mutated", None),
                Err(e) => CommandOutcome::reject("OUT_OF_BOUNDS", e.to_string()),
            }
        }
        TacticalCommand::SetRuleset { ruleset } => {
            world.ruleset = ruleset.clone();
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("RULESET_UPDATED", "ruleset updated", None)
        }
        TacticalCommand::SetNight { night } => {
            world.night = *night;
            CommandOutcome::ok("NIGHT_UPDATED", format!("night={night}"), None)
        }
        TacticalCommand::ScheduleTimedCharge {
            unit_id,
            pos,
            delay_ticks,
            blast_radius_m,
            blast_kpa,
            crater_depth_m,
        } => {
            if *delay_ticks == 0 || *delay_ticks > MAX_STEP_CACHE_HORIZON_TICKS {
                return CommandOutcome::reject(
                    "INVALID_DELAY_TICKS",
                    format!("delay_ticks must be in 1..={MAX_STEP_CACHE_HORIZON_TICKS}"),
                );
            }
            let team = world.units[&unit_id.0].team;
            let unit_name = world.units[&unit_id.0].callsign.clone();
            let target_tick = world.tick.saturating_add(*delay_ticks);
            let inst_id = match world.create_world_instance(
                WorldInstanceKind::TimedCharge,
                team,
                Some(*unit_id),
                *pos,
                format!("timed_charge_{unit_name}"),
                *blast_radius_m,
                *blast_kpa,
                *crater_depth_m,
            ) {
                Ok(id) => id,
                Err(e) => return CommandOutcome::reject("OUT_OF_BOUNDS", e),
            };
            let h = mix_time_unit_hash(world.clock_seconds, &unit_name);
            let exact_ms = query_time_lut(TimeCategory::Event, h).exact_ms;
            match world.schedule_step_event(ScheduledStepEvent {
                event_id: 0,
                source_tick: world.tick,
                target_tick,
                exact_ms,
                team,
                kind: ScheduledEventKind::TimedDetonation {
                    instance_id: inst_id,
                    team,
                    pos: *pos,
                    blast_radius_m: *blast_radius_m,
                    blast_kpa: *blast_kpa,
                    crater_depth_m: *crater_depth_m,
                },
            }) {
                Ok(eid) => CommandOutcome {
                    accepted: true,
                    deferred_until_tick: Some(target_tick),
                    ticket_id: Some(eid),
                    remote_delay_ticks: None,
                    via_runner: None,
                    code: "TIMED_CHARGE_SCHEDULED".into(),
                    detail: format!("instance #{inst_id} created, detonates at tick {target_tick}"),
                    created_id: Some(inst_id as u32),
                },
                Err(e) => CommandOutcome::reject("SCHEDULE_FAILED", e),
            }
        }
        TacticalCommand::CancelStepEvent { event_id } => {
            match world.cancel_step_event(*event_id, None) {
                Some(removed) => {
                    if let ScheduledEventKind::TimedDetonation { instance_id, .. } = removed.kind {
                        if let Some(inst) = world.world_instances.get_mut(&instance_id) {
                            inst.active = false;
                        }
                    }
                    CommandOutcome::ok(
                        "STEP_EVENT_CANCELLED",
                        format!("cancelled event #{event_id}"),
                        None,
                    )
                }
                None => CommandOutcome::reject(
                    "EVENT_NOT_FOUND",
                    format!("step_cache event #{event_id} not found"),
                ),
            }
        }
        TacticalCommand::IssueCommanderWave {
            team,
            hq_unit_id: _,
            delay_ticks,
            zones,
        } => {
            if zones.is_empty() {
                return CommandOutcome::reject("EMPTY_WAVE", "commander wave requires >= 1 zone");
            }
            if *delay_ticks > MAX_STEP_CACHE_HORIZON_TICKS {
                return CommandOutcome::reject(
                    "INVALID_DELAY_TICKS",
                    format!("delay_ticks must be <= {MAX_STEP_CACHE_HORIZON_TICKS}"),
                );
            }
            for z in zones {
                if let Err(e) = world.terrain.geom.world_to_cell(z.center) {
                    return CommandOutcome::reject("OUT_OF_BOUNDS", e.to_string());
                }
            }
            let target_tick = world.tick.saturating_add((*delay_ticks).max(1));
            let h = mix_time_unit_hash(world.clock_seconds, team.as_str());
            let exact_ms = query_time_lut(TimeCategory::Command, h).exact_ms;
            let wave_id = world.alloc_event_id();
            match world.schedule_step_event(ScheduledStepEvent {
                event_id: wave_id,
                source_tick: world.tick,
                target_tick,
                exact_ms,
                team: *team,
                kind: ScheduledEventKind::CommanderWave {
                    wave_id,
                    team: *team,
                    zones: zones.clone(),
                },
            }) {
                Ok(eid) => CommandOutcome {
                    accepted: true,
                    deferred_until_tick: Some(target_tick),
                    ticket_id: Some(eid),
                    remote_delay_ticks: None,
                    via_runner: None,
                    code: "COMMANDER_WAVE_SCHEDULED".into(),
                    detail: format!("wave #{wave_id} activates at tick {target_tick}"),
                    created_id: Some(wave_id as u32),
                },
                Err(e) => CommandOutcome::reject("SCHEDULE_FAILED", e),
            }
        }
    }
}

fn team_code(t: Team) -> u8 {
    match t {
        Team::Red => 1,
        Team::Blue => 2,
        Team::Neutral => 0,
    }
}

fn execute_arm_detonator(
    world: &mut WorldState,
    unit_id: UnitId,
    switch_pos: WorldPos,
    charge_positions: &[WorldPos],
    wire_polyline: &[WorldPos],
) -> CommandOutcome {
    if charge_positions.is_empty() || wire_polyline.len() < 2 {
        return CommandOutcome::reject(
            "INVALID_DETONATOR",
            "detonator requires >= 1 charge and >= 2 wire points",
        );
    }
    if world.terrain.geom.world_to_cell(switch_pos).is_err()
        || charge_positions
            .iter()
            .any(|&p| world.terrain.geom.world_to_cell(p).is_err())
        || wire_polyline
            .iter()
            .any(|&p| world.terrain.geom.world_to_cell(p).is_err())
    {
        return CommandOutcome::reject("OUT_OF_BOUNDS", "detonator coordinate out of bounds");
    }
    let team_id = team_code(world.units[&unit_id.0].team);
    let switch_id = (world.detonators.len() as u64) + 1;
    let charges: Vec<[f32; 2]> = charge_positions.iter().map(|p| [p.x, p.y]).collect();
    let wire: Vec<[f32; 2]> = wire_polyline.iter().map(|p| [p.x, p.y]).collect();
    let mut sw = DetonatorSwitchEntity::new(
        switch_id,
        team_id,
        [switch_pos.x, switch_pos.y],
        charges,
        wire,
    );
    sw.charges_placed = true;
    sw.wire_laid = true;
    world.detonators.push(sw);
    let team = world.units[&unit_id.0].team;
    for (idx, &p) in charge_positions.iter().enumerate() {
        let _ = world.create_world_instance(
            WorldInstanceKind::TimedCharge,
            team,
            Some(unit_id),
            p,
            format!("detonator_{switch_id}_charge_{idx}"),
            24.0,
            120.0,
            2.0,
        );
    }
    CommandOutcome::ok(
        "DETONATOR_ARMED",
        format!("switch #{switch_id} armed"),
        Some(switch_id as u32),
    )
}

fn execute_trigger_detonator(
    world: &mut WorldState,
    unit_id: UnitId,
    switch_id: u64,
) -> CommandOutcome {
    let unit = &world.units[&unit_id.0];
    let op_pos = [unit.pos.x, unit.pos.y];
    let team_id = team_code(unit.team);
    let Some(sw) = world.detonators.iter_mut().find(|d| d.switch_id == switch_id) else {
        return CommandOutcome::reject("SWITCH_NOT_FOUND", format!("unknown switch {switch_id}"));
    };
    if sw.team_id != team_id {
        return CommandOutcome::reject("TEAM_MISMATCH", "cannot trigger hostile detonator");
    }
    match sw.trigger_detonation(op_pos) {
        Ok(blast_pts) => {
            let prefix = format!("detonator_{switch_id}_charge_");
            for inst in world.world_instances.values_mut() {
                if inst.label.starts_with(&prefix) {
                    inst.active = false;
                }
            }
            for (idx, pt) in blast_pts.into_iter().enumerate() {
                let center = WorldPos::new(pt[0], pt[1]);
                let burst_z = world.terrain.elevation_at(center).unwrap_or(0.0) + 0.4;
                let _ = resolve_and_commit_radial_blast(
                    world,
                    world.tick,
                    &RadialBlastField {
                        source_id: unit_id,
                        center,
                        burst_z,
                        radius_m: 24.0,
                        peak_kpa: 120.0,
                        fragments_total: 100,
                        frag_armor_pen_mm: 20.0,
                        frag_damage: 12.0,
                        crater_depth_m: 2.0,
                        det_seed: switch_id ^ (idx as u64 + 1),
                    },
                );
            }
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("DETONATED", format!("switch #{switch_id} triggered"), None)
        }
        Err(msg) => CommandOutcome::reject("DETONATION_BLOCKED", msg),
    }
}
