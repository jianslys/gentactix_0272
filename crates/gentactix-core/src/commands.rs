use crate::catalog::ItemCatalog;
use crate::clock::generate_work_points;
use crate::math_ops::{CellCoord, DetonatorSwitchEntity, WorldPos};
use crate::ruleset::{SimRuleset, UnitSpawnSpec};
use crate::state::WorldState;
use crate::terrain::TerrainKind;
use crate::time_machine::TimeCategory;
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
            | Self::SetNight { .. } => None,
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
            | Self::TriggerDetonator { unit_id, .. } => Some(*unit_id),
            Self::SpawnPlatoon { .. }
            | Self::SpawnUnit { .. }
            | Self::DespawnUnit { .. }
            | Self::EditTerrain { .. }
            | Self::SetRuleset { .. }
            | Self::SetNight { .. } => None,
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
    pub code: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_id: Option<u32>,
}

impl CommandOutcome {
    fn ok(code: &str, detail: impl Into<String>, created_id: Option<u32>) -> Self {
        Self {
            accepted: true,
            deferred_until_tick: None,
            ticket_id: None,
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
            code: code.to_string(),
            detail: detail.into(),
            created_id: None,
        }
    }
}

pub fn validate_tactical_command_access(
    world: &WorldState,
    cmd: &TacticalCommand,
) -> Result<(), (String, String)> {
    let Some(uid) = cmd.subject_unit_id() else {
        return Ok(());
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
                        let limit = hq.effective_radio_range_m.max(1.0);
                        if dist > limit {
                            return Err((
                                "OUT_OF_COMMAND_RADIUS".into(),
                                format!(
                                    "unit {} is {:.1}m from HQ '{}' (effective radio {:.1}m)",
                                    uid.0, dist, hq.callsign, limit
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
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
            for pt in blast_pts {
                if let Ok(cell) = world.terrain.geom.world_to_cell(WorldPos::new(pt[0], pt[1])) {
                    let _ = world
                        .terrain
                        .mutate_cell(cell, Some(-2.0), None, None, Some(0.5));
                    if let Ok(cidx) = world.terrain.idx(cell) {
                        if cidx < world.cell_h_blast.len() {
                            world.cell_h_blast[cidx] += 120.0;
                            world.cell_s_supp[cidx] += 1.2;
                        }
                    }
                }
            }
            world.unit_revision = world.unit_revision.wrapping_add(1);
            CommandOutcome::ok("DETONATED", format!("switch #{switch_id} triggered"), None)
        }
        Err(msg) => CommandOutcome::reject("DETONATION_BLOCKED", msg),
    }
}
