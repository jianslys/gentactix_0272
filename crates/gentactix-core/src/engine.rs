use crate::catalog::ItemCatalog;
use crate::clock::ballistics_field::{resolve_and_commit_radial_blast, RadialBlastField};
use crate::clock::{TickOrchestrator, TickReport};
use crate::commands::{
    execute_command_immediate, validate_tactical_command_access, CommandOutcome, TacticalCommand,
};
use crate::math_ops::WorldPos;
use crate::preview::{
    preview_engineer_work, preview_fire_arc, preview_los_between, preview_move_path,
    PreviewLosArcResult, PreviewLosLineResult, PreviewPathResult, PreviewWorkResult,
};
use crate::ruleset::{ScenarioSpec, SimRuleset};
use crate::state::{CombatLogEntry, DerivedState, TickScratch, WorldState};
use crate::step_cache::{
    ActiveWaveDirective, ScheduledEventKind, ScheduledStepEvent, WaveZoneKind,
};
use crate::time_machine::{
    mix_time_unit_hash, query_time_lut, TimeMachineFramePayload,
};
use crate::types::{EngineerWorkKind, MoveStance, Team, UnitId};
use crate::visibility::{
    build_egress_playback_frame, build_observed_unit_detail, build_observed_view,
    ObservedStateView, VisibleUnitView,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStepOutput {
    pub report: TickReport,
    pub playback: TimeMachineFramePayload,
    pub activated_commands: Vec<CommandOutcome>,
}

/// Top-level 8-Pillar Pure Simulation Engine assembly.
pub struct PureEngine {
    pub catalog: ItemCatalog,
    pub world: WorldState,
    pub derived: DerivedState,
    pub scratch: TickScratch,
    pub last_report: Option<TickReport>,
}

impl PureEngine {
    pub fn new_blank(
        rows: usize,
        cols: usize,
        base_elevation_m: f32,
        ruleset: SimRuleset,
    ) -> Result<Self, String> {
        let catalog = ItemCatalog::embedded().map_err(|errs| errs.join("; "))?;
        let world = WorldState::new_blank(rows, cols, base_elevation_m, ruleset);
        let mut derived = DerivedState::new(world.terrain.rows, world.terrain.cols);
        derived.rebuild_if_needed(&world);
        Ok(Self {
            catalog,
            world,
            derived,
            scratch: TickScratch::default(),
            last_report: None,
        })
    }

    pub fn from_scenario(spec: &ScenarioSpec) -> Result<Self, String> {
        let catalog = ItemCatalog::embedded().map_err(|errs| errs.join("; "))?;
        let world = spec.build_world(&catalog)?;
        let mut derived = DerivedState::new(world.terrain.rows, world.terrain.cols);
        derived.rebuild_if_needed(&world);
        Ok(Self {
            catalog,
            world,
            derived,
            scratch: TickScratch::default(),
            last_report: None,
        })
    }

    pub fn submit_command(&mut self, mut cmd: TacticalCommand, cmd_sec_in_tick: u32) -> CommandOutcome {
        if let TacticalCommand::CancelStepEvent { event_id } = cmd {
            let hint = self.derived.step_event_index.get(&event_id).copied();
            let outcome = match self.world.cancel_step_event(event_id, hint) {
                Some(removed) => {
                    if let ScheduledEventKind::TimedDetonation { instance_id, .. } = removed.kind {
                        if let Some(inst) = self.world.world_instances.get_mut(&instance_id) {
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
            };
            self.derived.sync_step_event_index(&self.world);
            self.derived.rebuild_if_needed(&self.world);
            return outcome;
        }

        let access = match validate_tactical_command_access(&self.world, &cmd) {
            Ok(acc) => acc,
            Err((code, detail)) => return CommandOutcome::reject(&code, detail),
        };

        if let TacticalCommand::IssueCommanderWave { delay_ticks, .. } = &mut cmd {
            *delay_ticks = delay_ticks.saturating_add(access.delay_ticks);
        }

        if let Some(cat) = cmd.time_category() {
            if self.world.ruleset.enable_time_machine_deferral || access.delay_ticks > 0 {
                let team = cmd
                    .subject_unit_id()
                    .and_then(|id| self.world.units.get(&id.0))
                    .map(|u| u.team)
                    .unwrap_or(Team::Neutral);
                let unit_name = cmd
                    .subject_unit_id()
                    .and_then(|id| self.world.units.get(&id.0))
                    .map(|u| u.callsign.clone())
                    .unwrap_or_else(|| "global".to_string());

                let t_cmd_clamped = cmd_sec_in_tick.clamp(1, 16);
                let issued_abs_sec = self.world.tick * 16 + (t_cmd_clamped as u64);
                let hash_m = mix_time_unit_hash(issued_abs_sec, &unit_name);
                let lut = query_time_lut(cat, hash_m);
                let tm_skip = if self.world.ruleset.enable_time_machine_deferral {
                    1 + lut.turn_skip as u64
                } else {
                    0
                };
                let activate_tick = self
                    .world
                    .tick
                    .saturating_add(access.delay_ticks)
                    .saturating_add(tm_skip);
                let ev_id = self.world.alloc_event_id();
                let scheduled = ScheduledStepEvent {
                    event_id: ev_id,
                    source_tick: self.world.tick,
                    target_tick: activate_tick,
                    exact_ms: lut.exact_ms,
                    team,
                    kind: ScheduledEventKind::CommandTicket {
                        cmd_sec_in_tick: t_cmd_clamped,
                        category: Some(cat),
                        unit_name,
                        command: Box::new(cmd),
                    },
                };
                if let Err(e) = self.world.schedule_step_event(scheduled) {
                    return CommandOutcome::reject("SCHEDULE_FAILED", e);
                }
                self.derived.sync_step_event_index(&self.world);
                let is_remote = access.delay_ticks > 0;
                let code = if is_remote {
                    "DEFERRED_REMOTE_COMMAND"
                } else {
                    "DEFERRED_TO_TIME_MACHINE"
                };
                let detail = if is_remote {
                    format!("{}; activates at tick {activate_tick}", access.detail)
                } else {
                    format!("deferred via {:?} until tick {activate_tick}", cat)
                };
                return CommandOutcome {
                    accepted: true,
                    deferred_until_tick: Some(activate_tick),
                    ticket_id: Some(ev_id),
                    remote_delay_ticks: is_remote.then_some(access.delay_ticks),
                    via_runner: is_remote.then_some(access.via_runner),
                    code: code.into(),
                    detail,
                    created_id: None,
                };
            }
        }

        let outcome = execute_command_immediate(&mut self.world, &self.catalog, &cmd);
        self.derived.sync_step_event_index(&self.world);
        self.derived.rebuild_if_needed(&self.world);
        outcome
    }

    pub fn step_16s(&mut self, observer_team: Team) -> EngineStepOutput {
        let target_tick = self.world.tick.saturating_add(1);
        let due = self.world.drain_due_step_events(target_tick);

        let mut activated_commands = Vec::with_capacity(due.len());
        for ev in due {
            match ev.kind {
                ScheduledEventKind::CommandTicket { command, .. } => {
                    let out =
                        execute_command_immediate(&mut self.world, &self.catalog, &command);
                    activated_commands.push(out);
                }
                ScheduledEventKind::TimedDetonation {
                    instance_id,
                    team: _,
                    pos,
                    blast_radius_m,
                    blast_kpa,
                    crater_depth_m,
                } => {
                    let was_active = self
                        .world
                        .world_instances
                        .get_mut(&instance_id)
                        .is_some_and(|inst| {
                            let prev = inst.active;
                            inst.active = false;
                            prev
                        });
                    if was_active {
                        apply_area_detonation(
                            &mut self.world,
                            target_tick,
                            pos,
                            blast_radius_m,
                            blast_kpa,
                            crater_depth_m,
                            format!("timed charge #{instance_id} detonated"),
                        );
                        activated_commands.push(CommandOutcome::ok(
                            "TIMED_DETONATION",
                            format!("instance #{instance_id} detonated at tick {target_tick}"),
                            Some(instance_id as u32),
                        ));
                    }
                }
                ScheduledEventKind::CommanderWave {
                    wave_id,
                    team,
                    zones,
                } => {
                    for z in &zones {
                        if z.kind == WaveZoneKind::Bombardment {
                            apply_area_detonation(
                                &mut self.world,
                                target_tick,
                                z.center,
                                z.radius_m.max(16.0),
                                120.0 * z.g0_weight.max(0.5),
                                1.5,
                                format!("commander wave #{wave_id} bombardment"),
                            );
                        }
                    }
                    self.world.active_wave_directives.push(ActiveWaveDirective {
                        wave_id,
                        team,
                        activated_tick: target_tick,
                        zones,
                    });
                    self.world.unit_revision = self.world.unit_revision.wrapping_add(1);
                    activated_commands.push(CommandOutcome::ok(
                        "COMMANDER_WAVE_ACTIVATED",
                        format!("wave #{wave_id} active at tick {target_tick}"),
                        Some(wave_id as u32),
                    ));
                }
            }
        }

        self.derived.sync_step_event_index(&self.world);
        self.derived.rebuild_if_needed(&self.world);
        let report = TickOrchestrator::step_16s(
            &mut self.world,
            &mut self.derived,
            &mut self.scratch,
            &self.catalog,
        );
        let playback = build_egress_playback_frame(&self.world, &report, observer_team);
        self.last_report = Some(report.clone());
        EngineStepOutput {
            report,
            playback,
            activated_commands,
        }
    }

    pub fn step_n_ticks(&mut self, n: u32, observer_team: Team) -> Vec<EngineStepOutput> {
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            out.push(self.step_16s(observer_team));
        }
        out
    }

    pub fn observe(&self, observer_team: Team) -> ObservedStateView {
        let recent_logs = self
            .last_report
            .as_ref()
            .map(|r| r.tick_logs.as_slice())
            .unwrap_or(&[]);
        build_observed_view(&self.world, observer_team, recent_logs)
    }

    pub fn observe_unit_detail(&self, observer_team: Team, unit_id: UnitId) -> Option<VisibleUnitView> {
        let recent_logs = self
            .last_report
            .as_ref()
            .map(|r| r.tick_logs.as_slice())
            .unwrap_or(&[]);
        build_observed_unit_detail(&self.world, observer_team, unit_id, recent_logs)
    }

    pub fn preview_move(
        &self,
        unit_id: UnitId,
        goal: WorldPos,
        stance: MoveStance,
    ) -> Result<PreviewPathResult, String> {
        preview_move_path(&self.world, unit_id, goal, stance)
    }

    pub fn preview_fire_arc(
        &self,
        unit_id: UnitId,
        center_angle_deg: f32,
        half_arc_deg: f32,
        ray_count: usize,
        max_dist_m: f32,
    ) -> Result<PreviewLosArcResult, String> {
        preview_fire_arc(
            &self.world,
            unit_id,
            center_angle_deg,
            half_arc_deg,
            ray_count,
            max_dist_m,
        )
    }

    pub fn preview_los(
        &self,
        from: WorldPos,
        to: WorldPos,
        max_vision_m: f32,
    ) -> PreviewLosLineResult {
        preview_los_between(&self.world, from, to, max_vision_m)
    }

    pub fn preview_engineer_work(
        &self,
        unit_id: UnitId,
        work: EngineerWorkKind,
        control_points: &[WorldPos],
    ) -> Result<PreviewWorkResult, String> {
        preview_engineer_work(&self.world, unit_id, work, control_points)
    }

    pub fn save_json(&self) -> Result<String, String> {
        self.world.save_json()
    }

    pub fn load_json(&mut self, json: &str) -> Result<(), String> {
        let world = WorldState::load_json(json)?;
        self.world = world;
        self.derived = DerivedState::new(self.world.terrain.rows, self.world.terrain.cols);
        self.derived.rebuild_if_needed(&self.world);
        self.scratch.clear_and_assert_empty();
        self.last_report = None;
        Ok(())
    }
}

fn apply_area_detonation(
    world: &mut WorldState,
    tick: u64,
    center: WorldPos,
    blast_radius_m: f32,
    blast_kpa: f32,
    crater_depth_m: f32,
    message: String,
) {
    let burst_z = world.terrain.elevation_at(center).unwrap_or(0.0) + 0.5;
    let totals = resolve_and_commit_radial_blast(
        world,
        tick,
        &RadialBlastField {
            source_id: UnitId(0),
            center,
            burst_z,
            radius_m: blast_radius_m,
            peak_kpa: blast_kpa,
            fragments_total: 120,
            frag_armor_pen_mm: 22.0,
            frag_damage: 14.0,
            crater_depth_m,
            det_seed: tick ^ 0x517cc1b727220a95,
        },
    );
    world.combat_logs.push(CombatLogEntry {
        tick,
        actor_id: UnitId(0),
        target_id: totals.primary_target_id,
        kind: "detonation".into(),
        dist_m: totals.primary_dist_m,
        casualties: totals.casualties,
        hp_loss: totals.hp_loss,
        suppression_add: totals.suppression_add.max(1.5),
        ammo_spent: 0,
        message,
    });
    world.unit_revision = world.unit_revision.wrapping_add(1);
}
