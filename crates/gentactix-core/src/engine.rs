use crate::catalog::ItemCatalog;
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
use crate::state::{DerivedState, TickScratch, WorldState};
use crate::time_machine::{TimeMachineFramePayload, TimeMachinePendingQueue};
use crate::types::{EngineerWorkKind, MoveStance, Team, UnitId};
use crate::visibility::{build_egress_playback_frame, build_observed_view, ObservedStateView};
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
    pub ingress_queue: TimeMachinePendingQueue,
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
            ingress_queue: TimeMachinePendingQueue::new(),
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
            ingress_queue: TimeMachinePendingQueue::new(),
            last_report: None,
        })
    }

    pub fn submit_command(&mut self, cmd: TacticalCommand, cmd_sec_in_tick: u32) -> CommandOutcome {
        if self.world.ruleset.enable_time_machine_deferral {
            if let Some(cat) = cmd.time_category() {
                if let Err((code, detail)) = validate_tactical_command_access(&self.world, &cmd) {
                    return CommandOutcome::reject(&code, detail);
                }
                let unit_name = cmd
                    .subject_unit_id()
                    .and_then(|id| self.world.units.get(&id.0))
                    .map(|u| u.callsign.clone())
                    .unwrap_or_else(|| "global".to_string());
                let payload = match serde_json::to_value(&cmd) {
                    Ok(v) => v,
                    Err(e) => return CommandOutcome::reject("SERIALIZE_ERROR", e.to_string()),
                };
                let ticket = self.ingress_queue.enqueue_operation(
                    self.world.tick,
                    cmd_sec_in_tick,
                    cat,
                    &unit_name,
                    16,
                    payload,
                );
                return CommandOutcome {
                    accepted: true,
                    deferred_until_tick: Some(ticket.activate_tick),
                    ticket_id: Some(ticket.ticket_id),
                    code: "DEFERRED_TO_TIME_MACHINE".into(),
                    detail: format!(
                        "deferred via {:?} until tick {}",
                        cat, ticket.activate_tick
                    ),
                    created_id: None,
                };
            }
        }

        let outcome = execute_command_immediate(&mut self.world, &self.catalog, &cmd);
        self.derived.rebuild_if_needed(&self.world);
        outcome
    }

    pub fn step_16s(&mut self, observer_team: Team) -> EngineStepOutput {
        let target_tick = self.world.tick.saturating_add(1);
        let mut due = self.ingress_queue.drain_due_operations(target_tick);
        due.sort_by_key(|t| t.ticket_id);

        let mut activated_commands = Vec::with_capacity(due.len());
        for ticket in due {
            match serde_json::from_value::<TacticalCommand>(ticket.raw_op_payload) {
                Ok(cmd) => {
                    let out = execute_command_immediate(&mut self.world, &self.catalog, &cmd);
                    activated_commands.push(out);
                }
                Err(e) => {
                    activated_commands.push(CommandOutcome::reject(
                        "TICKET_DECODE_ERROR",
                        e.to_string(),
                    ));
                }
            }
        }

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
        self.ingress_queue = TimeMachinePendingQueue::new();
        self.last_report = None;
        Ok(())
    }
}
