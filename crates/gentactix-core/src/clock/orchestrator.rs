use crate::catalog::ItemCatalog;
use crate::clock::autonomy::propose_autonomous_intents;
use crate::clock::ballistics_field::{
    apply_hit_delta_to_unit, build_direct_cone_field, resolve_and_commit_radial_blast,
    resolve_direct_cone_batch_parallel, ConeFireResolution, DirectConeField, RadialBlastField,
};
use crate::clock::los_combat::check_los;
use crate::clock::pathfinding::{
    constrain_segment_motion, find_path_world, is_cell_passable, segment_has_obstacle,
};
use crate::clock::phases::TickStage;
use crate::math_ops::{
    apply_g_work_consumption, generate_miter_offset_foxholes, generate_quincunx_minefield,
    generate_zigzag_trench, quantize_pos_78125, transition_vis_tier_hysteresis,
    AugmentedArcIntegrator, CellCoord, SchmidtPostureState, WorldPos, ALPHA_BLAST, ALPHA_INTEL,
    ALPHA_SUPP,
};
use crate::state::{
    CombatLogEntry, DerivedState, EventRecord, EventTerminalStatus, IntentCandidate, IntentSource,
    MotionProposal, TickScratch, UnitRecord, UnitStartBaseline, WorldState,
};
use crate::step_cache::WorldInstanceKind;
use crate::types::{
    EngineerWorkKind, FireDiscipline, Team, UnitId, UnitOrder, TICK_SECONDS_F32,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickReport {
    pub tick: u64,
    pub clock_seconds: u64,
    pub stages_executed: Vec<TickStage>,
    pub moved_units: usize,
    pub fire_events: usize,
    pub engineer_events: usize,
    pub smoke_events: usize,
    pub mine_events: usize,
    pub casualties_total: u16,
    pub prior_positions: BTreeMap<u32, WorldPos>,
    #[serde(default)]
    pub unit_start_baselines: BTreeMap<u32, UnitStartBaseline>,
    #[serde(default)]
    pub motion_waypoints: BTreeMap<u32, Vec<WorldPos>>,
    pub tick_logs: Vec<CombatLogEntry>,
    pub authoritative_sha256: String,
}

enum EventDetail {
    Fire {
        aim_pos: WorldPos,
    },
    Smoke {
        target_cell: CellCoord,
    },
    Engineer {
        work: EngineerWorkKind,
        waypoints: Vec<WorldPos>,
    },
    MineTrigger {
        cell: CellCoord,
    },
}

enum PendingMutation {
    FireCone(ConeFireResolution),
    SmokeScreen {
        unit_id: UnitId,
        target_cell: CellCoord,
    },
    EngineerWork {
        unit_id: UnitId,
        work: EngineerWorkKind,
        integrator: AugmentedArcIntegrator,
        new_points: Vec<WorldPos>,
        work_delta: f32,
        completed_total: usize,
    },
    MineDetonation {
        unit_id: UnitId,
        cell: CellCoord,
    },
}

pub struct TickOrchestrator;

impl TickOrchestrator {
    pub fn step_n_ticks(
        world: &mut WorldState,
        derived: &mut DerivedState,
        scratch: &mut TickScratch,
        catalog: &ItemCatalog,
        n: u32,
    ) -> Vec<TickReport> {
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            out.push(Self::step_16s(world, derived, scratch, catalog));
        }
        out
    }

    pub fn step_16s(
        world: &mut WorldState,
        derived: &mut DerivedState,
        scratch: &mut TickScratch,
        catalog: &ItemCatalog,
    ) -> TickReport {
        let mut stages_executed = Vec::with_capacity(13);

        // Stage 0: BeginTick
        assert!(
            scratch.is_completely_empty(),
            "TickScratch must start empty at BeginTick"
        );
        for (&uid, unit) in &world.units {
            if unit.alive {
                scratch.prior_positions.insert(uid, unit.pos);
                scratch.unit_start_baselines.insert(
                    uid,
                    UnitStartBaseline {
                        hp: unit.hp,
                        men: unit.men,
                        ammo_rounds: unit.ammo_rounds,
                        suppression: unit.suppression,
                        eff_speed_scale: unit.effective_speed_scale(),
                    },
                );
            }
        }
        stages_executed.push(TickStage::BeginTick);

        // Stage 1: ClockAndSchedule
        world.advance_clock_one_tick();
        for unit in world.units.values_mut() {
            if unit.alive && unit.cooldown_s > 0.0 {
                unit.cooldown_s = (unit.cooldown_s - TICK_SECONDS_F32).max(0.0);
            }
        }
        stages_executed.push(TickStage::ClockAndSchedule);

        // Stage 2: PersistentFieldEvolve
        for h in &mut world.cell_h_blast {
            if *h > 0.0 {
                *h *= ALPHA_BLAST;
                if *h < 1e-4 {
                    *h = 0.0;
                }
            }
        }
        for s in &mut world.cell_s_supp {
            if *s > 0.0 {
                *s *= ALPHA_SUPP;
                if *s < 1e-4 {
                    *s = 0.0;
                }
            }
        }
        for t_idx in 0..2 {
            for s in &mut world.team_s_threat[t_idx] {
                if *s > 0.0 {
                    *s *= ALPHA_SUPP;
                    if *s < 1e-4 {
                        *s = 0.0;
                    }
                }
            }
        }
        world.terrain.decay_smoke(0.85);
        for t_idx in 0..2 {
            let len = world.team_i_intel[t_idx].len().min(world.team_i_vis_tier[t_idx].len());
            for i in 0..len {
                let next_val = world.team_i_intel[t_idx][i] * ALPHA_INTEL;
                world.team_i_intel[t_idx][i] = if next_val < 1e-5 { 0.0 } else { next_val };
                world.team_i_vis_tier[t_idx][i] = transition_vis_tier_hysteresis(
                    world.team_i_vis_tier[t_idx][i],
                    world.team_i_intel[t_idx][i],
                );
            }
        }
        let mut bleed_corpses: Vec<(UnitId, Team, WorldPos, u16, String)> = Vec::new();
        let mut stage2_unit_mutated = false;
        for unit in world.units.values_mut() {
            if !unit.alive {
                continue;
            }
            if !unit.is_vehicle && !unit.soldiers.is_empty() {
                let mut bled_deaths: u16 = 0;
                for soldier in &mut unit.soldiers {
                    if soldier.tick_bleeding_16s() {
                        bled_deaths = bled_deaths.saturating_add(1);
                    }
                }
                if bled_deaths > 0 {
                    unit.recompute_damage_aggregates();
                    stage2_unit_mutated = true;
                    bleed_corpses.push((
                        unit.id,
                        unit.team,
                        unit.pos,
                        bled_deaths,
                        format!("{} lost {} soldier(s) to exsanguination", unit.callsign, bled_deaths),
                    ));
                    if !unit.alive {
                        continue;
                    }
                }
            }
            if unit.suppression > 0.0 {
                unit.suppression *= ALPHA_SUPP;
                if unit.suppression < 1e-4 {
                    unit.suppression = 0.0;
                }
            }
            let prev_posture = unit.posture;
            unit.posture = if world.ruleset.enforce_suppression_posture {
                unit.posture.transition(unit.suppression)
            } else {
                SchmidtPostureState::Active
            };
            if unit.posture != prev_posture {
                stage2_unit_mutated = true;
            }
        }
        if stage2_unit_mutated {
            world.unit_revision = world.unit_revision.wrapping_add(1);
        }
        let mut tick_logs = Vec::new();
        for (uid, team, pos, deaths, msg) in bleed_corpses {
            for idx in 0..deaths {
                let _ = world.create_world_instance(
                    WorldInstanceKind::Corpse,
                    team,
                    Some(uid),
                    pos,
                    format!("corpse_{}_{}_{}", world.tick, uid.0, idx),
                    0.0,
                    0.0,
                    0.0,
                );
            }
            let entry = CombatLogEntry {
                tick: world.tick,
                actor_id: uid,
                target_id: Some(uid),
                kind: "bleed_out".into(),
                dist_m: 0.0,
                casualties: deaths,
                hp_loss: 0.0,
                suppression_add: 0.0,
                ammo_spent: 0,
                message: msg,
            };
            world.combat_logs.push(entry.clone());
            tick_logs.push(entry);
            world.unit_revision = world.unit_revision.wrapping_add(1);
        }
        stages_executed.push(TickStage::PersistentFieldEvolve);

        // Stage 3: DerivedRefresh (stateless Deygout radio recalculation)
        if !derived.rebuild_if_needed(world) {
            derived.sync_transient_fields(world);
        }
        for (&uid, unit) in &mut world.units {
            if unit.alive {
                let t_diff = derived.unit_radio_t_diff.get(&uid).copied().unwrap_or(1.0);
                unit.effective_radio_range_m =
                    unit.base_radio_range_m * t_diff * unit.effective_radio_scale();
            }
        }
        stages_executed.push(TickStage::DerivedRefresh);

        // Stage 4: SenseAndArbitrate (DirectOrder strictly overrides Autonomy)
        for (&uid, unit) in &world.units {
            if unit.alive && unit.order != UnitOrder::Hold {
                scratch.submit_intent(IntentCandidate {
                    unit_id: UnitId(uid),
                    source: IntentSource::DirectOrder,
                    priority: 100,
                    order: unit.order.clone(),
                });
            }
        }
        propose_autonomous_intents(world, derived, scratch);
        stages_executed.push(TickStage::SenseAndArbitrate);

        // Stage 5: ProposeMotion
        for (&uid, cand) in &scratch.intents {
            let Some(unit) = world.units.get(&uid) else {
                continue;
            };
            if !unit.alive {
                continue;
            }
            if let Some((prop, mid_pts)) = build_motion_proposal(world, unit, &cand.order) {
                if !mid_pts.is_empty() {
                    scratch.motion_waypoints.insert(uid, mid_pts);
                }
                scratch.motion_proposals.push(prop);
            }
        }
        stages_executed.push(TickStage::ProposeMotion);

        // Stage 6: ConstrainMotion (anti-tunneling <= 3.5m sweep + max 2 units/cell)
        let mut cell_occupancy: BTreeMap<(i32, i32), usize> = BTreeMap::new();
        for unit in world.units.values() {
            if unit.alive {
                if let Ok(cell) = world.terrain.geom.world_to_cell(unit.pos) {
                    *cell_occupancy.entry((cell.r, cell.c)).or_insert(0) += 1;
                }
            }
        }
        for prop in &scratch.motion_proposals {
            let Some(unit) = world.units.get(&prop.unit_id.0) else {
                continue;
            };
            let mut safe_to = if let Some(wpts) = scratch.motion_waypoints.get_mut(&prop.unit_id.0) {
                let mut cur = prop.from;
                let mut kept = Vec::with_capacity(wpts.len());
                let mut truncated = false;
                for &wp in wpts.iter() {
                    let s = constrain_segment_motion(&world.terrain, unit.mobility, cur, wp);
                    let short = s.distance_to(wp) > 0.5;
                    cur = s;
                    if short {
                        truncated = true;
                        break;
                    }
                    kept.push(wp);
                }
                let final_to = if truncated {
                    cur
                } else {
                    constrain_segment_motion(&world.terrain, unit.mobility, cur, prop.to)
                };
                *wpts = kept;
                final_to
            } else {
                constrain_segment_motion(&world.terrain, unit.mobility, prop.from, prop.to)
            };
            if let (Ok(c_from), Ok(c_to)) = (
                world.terrain.geom.world_to_cell(prop.from),
                world.terrain.geom.world_to_cell(safe_to),
            ) {
                if c_to != c_from {
                    let count_to = cell_occupancy.get(&(c_to.r, c_to.c)).copied().unwrap_or(0);
                    if count_to >= 2 {
                        safe_to = prop.from;
                        scratch.motion_waypoints.remove(&prop.unit_id.0);
                    } else {
                        if let Some(cnt) = cell_occupancy.get_mut(&(c_from.r, c_from.c)) {
                            *cnt = cnt.saturating_sub(1);
                        }
                        *cell_occupancy.entry((c_to.r, c_to.c)).or_insert(0) += 1;
                    }
                }
            }
            scratch.constrained_motions.push(MotionProposal {
                unit_id: prop.unit_id,
                from: prop.from,
                to: safe_to,
                speed_mps: prop.speed_mps,
            });
        }
        stages_executed.push(TickStage::ConstrainMotion);

        // Stage 7: CommitMotion (SINGLE POSITION WRITE POINT)
        let mut moved_units = 0usize;
        let mut pending_mines: Vec<(UnitId, CellCoord)> = Vec::new();
        for m in &scratch.constrained_motions {
            let Some(unit) = world.units.get_mut(&m.unit_id.0) else {
                continue;
            };
            if m.to.distance_to(unit.pos) > 1e-3 {
                unit.facing_deg = (m.to.y - unit.pos.y)
                    .atan2(m.to.x - unit.pos.x)
                    .to_degrees();
                unit.pos = m.to;
                moved_units += 1;
                if let Ok(c) = world.terrain.geom.world_to_cell(m.to) {
                    if let Ok(idx) = world.terrain.idx(c) {
                        if world.terrain.mine_density.get(idx).copied().unwrap_or(0.0) > 0.05 {
                            pending_mines.push((unit.id, c));
                        }
                    }
                }
            }
            if let UnitOrder::Move { waypoints, .. } = &mut unit.order {
                while let Some(&wp) = waypoints.first() {
                    if unit.pos.distance_to(wp) <= 4.0 {
                        waypoints.remove(0);
                    } else {
                        break;
                    }
                }
                if waypoints.is_empty() {
                    unit.order = UnitOrder::Hold;
                }
            }
        }
        if moved_units > 0 {
            world.unit_revision = world.unit_revision.wrapping_add(1);
        }
        stages_executed.push(TickStage::CommitMotion);

        // Stage 8: EmitEvents
        derived.rebuild_if_needed(world);
        let mut details: BTreeMap<u64, EventDetail> = BTreeMap::new();
        for (uid_mine, cell) in pending_mines {
            let eid = world.alloc_event_id();
            scratch.events.push(EventRecord {
                event_id: eid,
                tick: world.tick,
                source: uid_mine,
                kind: "mine_trigger".into(),
                target_cell: Some(cell),
            });
            details.insert(eid, EventDetail::MineTrigger { cell });
        }

        let unit_ids: Vec<u32> = world.units.keys().copied().collect();
        for uid in unit_ids {
            let Some(unit) = world.units.get(&uid).filter(|u| u.alive).cloned() else {
                continue;
            };
            let active_order = scratch
                .intents
                .get(&uid)
                .map(|c| c.order.clone())
                .unwrap_or_else(|| unit.order.clone());

            match &active_order {
                UnitOrder::PopSmoke { target } => {
                    if let Ok(tcell) = world.terrain.geom.world_to_cell(*target) {
                        let eid = world.alloc_event_id();
                        scratch.events.push(EventRecord {
                            event_id: eid,
                            tick: world.tick,
                            source: unit.id,
                            kind: "smoke".into(),
                            target_cell: Some(tcell),
                        });
                        details.insert(eid, EventDetail::Smoke { target_cell: tcell });
                    }
                    continue;
                }
                UnitOrder::BuildWork { work, waypoints } => {
                    let eid = world.alloc_event_id();
                    let tcell = world.terrain.geom.world_to_cell(unit.pos).ok();
                    scratch.events.push(EventRecord {
                        event_id: eid,
                        tick: world.tick,
                        source: unit.id,
                        kind: "engineer_work".into(),
                        target_cell: tcell,
                    });
                    details.insert(
                        eid,
                        EventDetail::Engineer {
                            work: *work,
                            waypoints: waypoints.clone(),
                        },
                    );
                    continue;
                }
                _ => {}
            }

            if !unit.can_fire_main_weapon() || unit.fire_discipline == FireDiscipline::HoldFire {
                continue;
            }
            if let Some((aim_pos, aim_cell)) = select_fire_aim(world, derived, &unit, &active_order) {
                let eid = world.alloc_event_id();
                scratch.events.push(EventRecord {
                    event_id: eid,
                    tick: world.tick,
                    source: unit.id,
                    kind: "fire".into(),
                    target_cell: Some(aim_cell),
                });
                details.insert(
                    eid,
                    EventDetail::Fire { aim_pos },
                );
            }
        }
        stages_executed.push(TickStage::EmitEvents);

        // Stage 9: ResolveEvents
        let events_snapshot = scratch.events.clone();
        let mut mutations: Vec<PendingMutation> = Vec::new();
        let mut fire_jobs: Vec<(DirectConeField, bool, f32, f32)> = Vec::new();
        for ev in &events_snapshot {
            match details.get(&ev.event_id) {
                Some(EventDetail::MineTrigger { cell }) => {
                    let _ = scratch.blast_workspace.splat_explosion(
                        &world.terrain.geom,
                        *cell,
                        16.0,
                        110.0,
                        60,
                        ev.event_id,
                    );
                    mutations.push(PendingMutation::MineDetonation {
                        unit_id: ev.source,
                        cell: *cell,
                    });
                    scratch.terminal_statuses.insert(
                        ev.event_id,
                        EventTerminalStatus::Resolved {
                            code: "MINE_DETONATED".into(),
                            detail: "unit stepped onto minefield".into(),
                        },
                    );
                }
                Some(EventDetail::Smoke { target_cell }) => {
                    let has_grenade = world.units.get(&ev.source.0).is_some_and(|u| u.smoke_grenades > 0);
                    if has_grenade {
                        mutations.push(PendingMutation::SmokeScreen {
                            unit_id: ev.source,
                            target_cell: *target_cell,
                        });
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Resolved {
                                code: "SMOKE_DEPLOYED".into(),
                                detail: "smoke screen deployed".into(),
                            },
                        );
                    } else {
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Rejected {
                                code: "NO_SMOKE_GRENADE".into(),
                                detail: "unit out of smoke grenades".into(),
                            },
                        );
                    }
                }
                Some(EventDetail::Engineer { work, waypoints }) => {
                    if let Some(unit) = world.units.get(&ev.source.0) {
                        let pts = generate_work_points(*work, waypoints);
                        let prior_p = scratch.prior_positions.get(&unit.id.0).copied().unwrap_or(unit.pos);
                        let mut integrator = world
                            .engineer_integrators
                            .get(&unit.id.0)
                            .cloned()
                            .unwrap_or_else(|| {
                                let raw: Vec<[f32; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
                                let mut init = AugmentedArcIntegrator::new(raw);
                                init.cur_pos = [prior_p.x, prior_p.y];
                                init
                            });
                        let before_done = integrator.completed_instances.len();
                        let semi_before = integrator.semi_finished_progress;
                        integrator.cur_pos = [prior_p.x, prior_p.y];
                        integrator.step_16s(unit.base_speed_mps, unit.men as u32, unit.suppression, 40.0);
                        // Keep integrator cursor synchronized with Stage 7 physical constrained position.
                        integrator.cur_pos = [unit.pos.x, unit.pos.y];
                        let after_done = integrator.completed_instances.len();
                        let newly_done: Vec<WorldPos> = integrator.completed_instances[before_done..]
                            .iter()
                            .filter_map(|&idx| integrator.waypoints.get(idx).map(|p| WorldPos::new(p[0], p[1])))
                            .collect();
                        let work_delta = (((after_done - before_done) as f32)
                            + integrator.semi_finished_progress
                            - semi_before)
                            .max(0.0)
                            * 40.0;
                        mutations.push(PendingMutation::EngineerWork {
                            unit_id: unit.id,
                            work: *work,
                            integrator,
                            new_points: newly_done,
                            work_delta,
                            completed_total: pts.len(),
                        });
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Resolved {
                                code: "ENGINEER_STEP".into(),
                                detail: "augmented arc step processed".into(),
                            },
                        );
                    }
                }
                Some(EventDetail::Fire { aim_pos }) => {
                    let Some(attacker) = world.units.get(&ev.source.0) else {
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Rejected {
                                code: "MISSING_ATTACKER".into(),
                                detail: "attacker vanished".into(),
                            },
                        );
                        continue;
                    };
                    if let Some(job) = build_direct_cone_field(
                        &world.terrain,
                        catalog,
                        attacker,
                        *aim_pos,
                        world.clock_seconds,
                        ev.event_id,
                        world.ruleset.enforce_ammo_consumption,
                        world.ruleset.enforce_suppression_posture,
                    ) {
                        fire_jobs.push(job);
                    } else {
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Rejected {
                                code: "OUT_OF_AMMO_OR_DUD".into(),
                                detail: "no rounds fired".into(),
                            },
                        );
                    }
                }
                None => {}
            }
        }

        if !fire_jobs.is_empty() {
            let resolutions =
                resolve_direct_cone_batch_parallel(&world.terrain, derived, &fire_jobs, true);
            for res in resolutions {
                if res.is_dud || res.rounds_fired == 0 {
                    scratch.terminal_statuses.insert(
                        res.event_id,
                        EventTerminalStatus::Rejected {
                            code: "OUT_OF_AMMO_OR_DUD".into(),
                            detail: "no rounds fired".into(),
                        },
                    );
                    continue;
                }
                if res.frag_radius_m > 0.0 {
                    let _ = scratch.blast_workspace.splat_explosion(
                        &world.terrain.geom,
                        res.aim_cell,
                        res.frag_radius_m.min(32.0),
                        res.blast_kpa,
                        res.rounds_fired * 20,
                        res.event_id,
                    );
                }
                scratch.terminal_statuses.insert(
                    res.event_id,
                    EventTerminalStatus::Resolved {
                        code: "FIRE_FIELD_RESOLVED".into(),
                        detail: format!("hits={}", res.hits.len()),
                    },
                );
                mutations.push(PendingMutation::FireCone(res));
            }
        }
        stages_executed.push(TickStage::ResolveEvents);

        // Stage 10: CommitPersistent (SINGLE COMBAT / TERRAIN / WORK WRITE POINT)
        let mut fire_events = 0usize;
        let mut engineer_events = 0usize;
        let mut smoke_events = 0usize;
        let mut mine_events = 0usize;
        let mut casualties_total = 0u16;
        let mut state_mutated = false;

        for m in mutations {
            match m {
                PendingMutation::FireCone(cone) => {
                    fire_events += 1;
                    state_mutated = true;
                    if world.ruleset.enforce_ammo_consumption {
                        if let Some(att) = world.units.get_mut(&cone.attacker_id.0) {
                            att.ammo_rounds = att.ammo_rounds.saturating_sub(cone.rounds_fired);
                        }
                    }

                    // Feedback unblocked fire corridor into opposing team's I_threat (zero in shielded shadows!).
                    let enemy_teams: &[usize] = match cone.source_team {
                        Team::Red => &[1],
                        Team::Blue => &[0],
                        Team::Neutral => &[0, 1],
                    };
                    for &(cidx, dens) in &cone.corridor_threat_cells {
                        for &ti in enemy_teams {
                            if cidx < world.team_s_threat[ti].len() {
                                world.team_s_threat[ti][cidx] += dens;
                            }
                        }
                    }

                    if let Ok(aim_idx) = world.terrain.idx(cone.aim_cell) {
                        let aim_reached = cone
                            .corridor_threat_cells
                            .iter()
                            .any(|&(cidx, _)| cidx == aim_idx);
                        if aim_reached && aim_idx < world.cell_s_supp.len() {
                            world.cell_s_supp[aim_idx] += 0.25;
                        }
                    }

                    let mut total_cas: u16 = 0;
                    let mut total_hp_loss: f32 = 0.0;
                    let mut total_supp: f32 = 0.0;
                    let mut primary_tgt: Option<UnitId> = None;
                    let mut primary_dist: f32 = 0.0;
                    let mut corpses_to_spawn: Vec<(UnitId, Team, WorldPos, u16)> = Vec::new();
                    let enforce_posture = world.ruleset.enforce_suppression_posture;

                    for hit in &cone.hits {
                        let (t_cell, t_cover) = world
                            .units
                            .get(&hit.target_id.0)
                            .and_then(|u| world.terrain.geom.world_to_cell(u.pos).ok())
                            .map(|c| {
                                let cov = world
                                    .terrain
                                    .idx(c)
                                    .ok()
                                    .map(|i| world.terrain.kind[i].cover_factor() + world.terrain.cover_bonus[i])
                                    .unwrap_or(0.0);
                                (Some(c), cov)
                            })
                            .unwrap_or((None, 0.0));
                        if let Some(c) = t_cell {
                            if let Ok(cidx) = world.terrain.idx(c) {
                                if cidx < world.cell_s_supp.len() {
                                    world.cell_s_supp[cidx] += hit.suppression_add;
                                }
                            }
                        }
                        if let Some(tgt) = world.units.get_mut(&hit.target_id.0) {
                            let team = tgt.team;
                            let tpos = tgt.pos;
                            let (cas, hp_loss, supp, corpses) = apply_hit_delta_to_unit(
                                tgt,
                                t_cover,
                                enforce_posture,
                                cone.incoming_dir,
                                cone.base_damage,
                                cone.armor_pen_mm,
                                cone.ignore_trench_cover,
                                hit,
                            );
                            total_cas = total_cas.saturating_add(cas);
                            total_hp_loss += hp_loss;
                            total_supp += supp;
                            if primary_tgt.is_none() {
                                primary_tgt = Some(hit.target_id);
                                primary_dist = hit.dist_m;
                            }
                            if corpses > 0 {
                                corpses_to_spawn.push((hit.target_id, team, tpos, corpses));
                            }
                        }
                    }
                    for (uid, team, tpos, cnt) in corpses_to_spawn {
                        for idx in 0..cnt {
                            let _ = world.create_world_instance(
                                WorldInstanceKind::Corpse,
                                team,
                                Some(uid),
                                tpos,
                                format!("corpse_fire_{}_{}_{}", world.tick, uid.0, idx),
                                0.0,
                                0.0,
                                0.0,
                            );
                        }
                    }

                    if cone.frag_radius_m > 0.0 {
                        if let Ok(center) = world.terrain.geom.cell_center_m(cone.aim_cell) {
                            let burst_z = world.terrain.elevation_at(center).unwrap_or(0.0) + 0.4;
                            let blast_tot = resolve_and_commit_radial_blast(
                                world,
                                world.tick,
                                &RadialBlastField {
                                    source_id: cone.attacker_id,
                                    center,
                                    burst_z,
                                    radius_m: cone.frag_radius_m.min(32.0),
                                    peak_kpa: cone.blast_kpa,
                                    fragments_total: cone.rounds_fired.saturating_mul(20),
                                    frag_armor_pen_mm: 18.0,
                                    frag_damage: 10.0,
                                    crater_depth_m: cone.crater_depth_m,
                                    det_seed: (cone.attacker_id.0 as u64) ^ world.clock_seconds,
                                },
                            );
                            total_cas = total_cas.saturating_add(blast_tot.casualties);
                            total_hp_loss += blast_tot.hp_loss;
                            total_supp += blast_tot.suppression_add;
                            if primary_tgt.is_none() {
                                primary_tgt = blast_tot.primary_target_id;
                                primary_dist = blast_tot.primary_dist_m;
                            }
                        }
                    }

                    casualties_total = casualties_total.saturating_add(total_cas);
                    let att_name = world
                        .units
                        .get(&cone.attacker_id.0)
                        .map(|u| u.callsign.as_str())
                        .unwrap_or("unit");
                    let entry = CombatLogEntry {
                        tick: world.tick,
                        actor_id: cone.attacker_id,
                        target_id: primary_tgt,
                        kind: "fire".into(),
                        dist_m: primary_dist,
                        casualties: total_cas,
                        hp_loss: total_hp_loss,
                        suppression_add: total_supp,
                        ammo_spent: cone.rounds_fired,
                        message: format!(
                            "{att_name} cone_fire rounds={} hits={} cas={total_cas} hp_loss={total_hp_loss:.1} supp+={total_supp:.2}",
                            cone.rounds_fired,
                            cone.hits.len()
                        ),
                    };
                    world.combat_logs.push(entry.clone());
                    tick_logs.push(entry);
                }
                PendingMutation::SmokeScreen {
                    unit_id,
                    target_cell,
                } => {
                    smoke_events += 1;
                    state_mutated = true;
                    if let Some(u) = world.units.get_mut(&unit_id.0) {
                        u.smoke_grenades = u.smoke_grenades.saturating_sub(1);
                        if matches!(u.order, UnitOrder::PopSmoke { .. }) {
                            u.order = UnitOrder::Hold;
                        }
                    }
                    let _ = world
                        .terrain
                        .mutate_cell(target_cell, None, None, None, Some(0.90));
                    for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let nc = CellCoord::new(target_cell.r + dr, target_cell.c + dc);
                        let _ = world.terrain.mutate_cell(nc, None, None, None, Some(0.45));
                    }
                }
                PendingMutation::EngineerWork {
                    unit_id,
                    work,
                    integrator,
                    new_points,
                    work_delta,
                    completed_total,
                } => {
                    engineer_events += 1;
                    state_mutated = true;
                    let eng_team = world
                        .units
                        .get(&unit_id.0)
                        .map(|u| u.team)
                        .unwrap_or(Team::Neutral);
                    for pt in &new_points {
                        if let Ok(cell) = world.terrain.geom.world_to_cell(*pt) {
                            match work {
                                EngineerWorkKind::FoxholeLine | EngineerWorkKind::ZigzagTrench => {
                                    let _ = world.terrain.mutate_cell(
                                        cell,
                                        None,
                                        None,
                                        Some(0.35),
                                        None,
                                    );
                                    let _ = world.create_world_instance(
                                        WorldInstanceKind::Fortification,
                                        eng_team,
                                        Some(unit_id),
                                        *pt,
                                        "fortification",
                                        0.0,
                                        0.0,
                                        0.0,
                                    );
                                }
                                EngineerWorkKind::Minefield => {
                                    let _ = world.terrain.add_mine_density(cell, 0.80);
                                    let _ = world.create_world_instance(
                                        WorldInstanceKind::Mine,
                                        eng_team,
                                        Some(unit_id),
                                        *pt,
                                        "mine",
                                        8.0,
                                        90.0,
                                        0.5,
                                    );
                                }
                                EngineerWorkKind::Demolition => {
                                    let _ = world.terrain.mutate_cell(
                                        cell,
                                        Some(-2.0),
                                        None,
                                        None,
                                        None,
                                    );
                                    let _ = world.create_world_instance(
                                        WorldInstanceKind::TimedCharge,
                                        eng_team,
                                        Some(unit_id),
                                        *pt,
                                        "demolition_charge",
                                        16.0,
                                        120.0,
                                        2.0,
                                    );
                                }
                            }
                            if let Ok(cidx) = world.terrain.idx(cell) {
                                if let Some(rem) = world.g_work_remaining.get_mut(&cidx) {
                                    apply_g_work_consumption(rem, 40.0);
                                }
                            }
                        }
                    }
                    if work_delta > 0.0 {
                        if let Ok(cur_cell) = world.terrain.geom.world_to_cell(WorldPos::new(
                            integrator.cur_pos[0],
                            integrator.cur_pos[1],
                        )) {
                            if let Ok(cidx) = world.terrain.idx(cur_cell) {
                                if let Some(rem) = world.g_work_remaining.get_mut(&cidx) {
                                    apply_g_work_consumption(rem, work_delta);
                                }
                            }
                        }
                    }
                    let finished = integrator.current_wp_idx >= completed_total.max(1);
                    world.engineer_integrators.insert(unit_id.0, integrator);
                    if finished {
                        if let Some(u) = world.units.get_mut(&unit_id.0) {
                            if matches!(u.order, UnitOrder::BuildWork { .. }) {
                                u.order = UnitOrder::Hold;
                            }
                        }
                    }
                }
                PendingMutation::MineDetonation { unit_id, cell } => {
                    mine_events += 1;
                    state_mutated = true;
                    let _ = world.terrain.add_mine_density(cell, -0.50);
                    let center = world
                        .terrain
                        .geom
                        .cell_center_m(cell)
                        .unwrap_or_else(|_| WorldPos::new(0.0, 0.0));
                    let burst_z = world.terrain.elevation_at(center).unwrap_or(0.0) + 0.2;
                    let tot = resolve_and_commit_radial_blast(
                        world,
                        world.tick,
                        &RadialBlastField {
                            source_id: unit_id,
                            center,
                            burst_z,
                            radius_m: 16.0,
                            peak_kpa: 110.0,
                            fragments_total: 60,
                            frag_armor_pen_mm: 20.0,
                            frag_damage: 16.0,
                            crater_depth_m: 0.6,
                            det_seed: world.clock_seconds ^ (unit_id.0 as u64),
                        },
                    );
                    casualties_total = casualties_total.saturating_add(tot.casualties);
                }
            }
        }
        if state_mutated {
            world.unit_revision = world.unit_revision.wrapping_add(1);
        }
        update_team_i_intel_after_commit(world, &tick_logs);
        stages_executed.push(TickStage::CommitPersistent);

        // Stage 11: Validate
        assert_eq!(
            world.clock_seconds,
            world.epoch_seconds.saturating_add(world.tick << 4),
            "16s integer clock invariant violated"
        );
        for unit in world.units.values() {
            if unit.alive {
                assert!(
                    world.terrain.geom.world_to_cell(unit.pos).is_ok(),
                    "alive unit '{}' escaped terrain bounds at {:?}",
                    unit.callsign,
                    unit.pos
                );
                assert!(unit.hp.is_finite() && unit.suppression.is_finite());
            }
        }
        assert_eq!(
            scratch.terminal_statuses.len(),
            scratch.events.len(),
            "every emitted event must carry a terminal status"
        );
        stages_executed.push(TickStage::Validate);

        // Stage 12: PublishAndDrop
        stages_executed.push(TickStage::PublishAndDrop);
        let prior_positions = std::mem::take(&mut scratch.prior_positions);
        let unit_start_baselines = std::mem::take(&mut scratch.unit_start_baselines);
        let motion_waypoints = std::mem::take(&mut scratch.motion_waypoints);
        world.terrain.clear_dirty_halo();
        if !derived.rebuild_if_needed(world) {
            derived.sync_transient_fields(world);
        }
        scratch.clear_and_assert_empty();

        TickReport {
            tick: world.tick,
            clock_seconds: world.clock_seconds,
            stages_executed,
            moved_units,
            fire_events,
            engineer_events,
            smoke_events,
            mine_events,
            casualties_total,
            prior_positions,
            unit_start_baselines,
            motion_waypoints,
            tick_logs,
            authoritative_sha256: world.authoritative_sha256(),
        }
    }
}

fn build_motion_proposal(
    world: &WorldState,
    unit: &UnitRecord,
    order: &UnitOrder,
) -> Option<(MotionProposal, Vec<WorldPos>)> {
    let (goal, stance_mult) = match order {
        UnitOrder::Move { waypoints, stance } => (*waypoints.first()?, stance.speed_mult()),
        UnitOrder::AttackUnit {
            target,
            chase: true,
            ..
        } => {
            let tgt = world.units.get(&target.0).filter(|t| t.alive)?;
            let los = check_los(&world.terrain, unit.pos, tgt.pos, unit.vision_m, world.night);
            if unit.pos.distance_to(tgt.pos) > unit.vision_m * 0.60 || !los.visible {
                (tgt.pos, 1.0)
            } else {
                return None;
            }
        }
        UnitOrder::BuildWork { work, waypoints } => {
            let pts = generate_work_points(*work, waypoints);
            if pts.is_empty() {
                return None;
            }
            let mut preview = world
                .engineer_integrators
                .get(&unit.id.0)
                .cloned()
                .unwrap_or_else(|| {
                    let raw: Vec<[f32; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
                    AugmentedArcIntegrator::new(raw)
                });
            preview.cur_pos = [unit.pos.x, unit.pos.y];
            let wp = *pts.get(preview.current_wp_idx)?;
            if segment_has_obstacle(&world.terrain, unit.mobility, unit.pos, wp) {
                (wp, 1.0)
            } else {
                preview.step_16s(unit.base_speed_mps, unit.men as u32, unit.suppression, 40.0);
                let to = WorldPos::new(preview.cur_pos[0], preview.cur_pos[1]);
                if unit.pos.distance_to(to) <= 1e-3 {
                    return None;
                }
                return Some((
                    MotionProposal {
                        unit_id: unit.id,
                        from: unit.pos,
                        to,
                        speed_mps: unit.base_speed_mps,
                    },
                    Vec::new(),
                ));
            }
        }
        _ => return None,
    };

    let cell = world.terrain.geom.world_to_cell(unit.pos).ok()?;
    let idx = world.terrain.idx(cell).ok()?;
    let terrain_cost = if is_cell_passable(&world.terrain, unit.mobility, cell) {
        world.terrain.kind[idx].move_cost(unit.mobility).unwrap_or(1.0)
    } else {
        1.0
    };
    let supp_mult = if world.ruleset.enforce_suppression_posture {
        match unit.posture {
            SchmidtPostureState::Active => (1.0 - 0.5 * unit.suppression.clamp(0.0, 1.0)).max(0.25),
            SchmidtPostureState::Pinned => 0.45,
            SchmidtPostureState::Prone => 0.25,
        }
    } else {
        1.0
    };
    let eff_scale = unit.effective_speed_scale();
    if eff_scale <= 0.0 {
        return None;
    }
    let speed_mps =
        (unit.base_speed_mps * eff_scale * stance_mult * supp_mult) / terrain_cost.max(0.5);
    let step_dist = speed_mps * TICK_SECONDS_F32;

    let mut mid_waypoints: Vec<WorldPos> = Vec::new();
    let to = if segment_has_obstacle(&world.terrain, unit.mobility, unit.pos, goal) {
        let path = find_path_world(&world.terrain, unit.mobility, unit.pos, goal, 12_000)?;
        let mut cursor = unit.pos;
        let mut rem = step_dist;
        let mut end_pos = unit.pos;
        for wp in path {
            if cursor.distance_to(wp) <= 0.5 {
                continue;
            }
            let seg = cursor.distance_to(wp);
            if rem <= seg {
                let f = (rem / seg).clamp(0.0, 1.0);
                end_pos = WorldPos::new(
                    cursor.x + (wp.x - cursor.x) * f,
                    cursor.y + (wp.y - cursor.y) * f,
                );
                rem = 0.0;
                break;
            } else {
                mid_waypoints.push(wp);
                cursor = wp;
                end_pos = wp;
                rem -= seg;
            }
        }
        let _ = rem;
        end_pos
    } else {
        let dist = unit.pos.distance_to(goal);
        if dist <= step_dist || dist <= 1e-4 {
            goal
        } else {
            let f = step_dist / dist;
            WorldPos::new(
                unit.pos.x + (goal.x - unit.pos.x) * f,
                unit.pos.y + (goal.y - unit.pos.y) * f,
            )
        }
    };

    Some((
        MotionProposal {
            unit_id: unit.id,
            from: unit.pos,
            to,
            speed_mps,
        },
        mid_waypoints,
    ))
}

fn select_fire_aim(
    world: &WorldState,
    derived: &DerivedState,
    unit: &UnitRecord,
    active_order: &UnitOrder,
) -> Option<(WorldPos, CellCoord)> {
    if let UnitOrder::AttackPos { target, .. } = active_order {
        if let Ok(cell) = world.terrain.geom.world_to_cell(*target) {
            return Some((*target, cell));
        }
        return None;
    }

    if let UnitOrder::AttackUnit { target, .. } = active_order {
        if let Some(enemy) = world.units.get(&target.0).filter(|e| e.alive && e.team.is_hostile_to(unit.team)) {
            let los = check_los(&world.terrain, unit.pos, enemy.pos, unit.vision_m, world.night);
            if los.visible {
                if let Ok(cell) = world.terrain.geom.world_to_cell(enemy.pos) {
                    return Some((enemy.pos, cell));
                }
            }
        }
        return None;
    }

    let nearby = derived.query_units_in_radius(world, unit.pos, unit.vision_m);
    let mut best: Option<(f32, u32, WorldPos, CellCoord)> = None;
    for uid in nearby {
        let Some(enemy) = world.units.get(&uid.0) else {
            continue;
        };
        if !enemy.alive || !enemy.team.is_hostile_to(unit.team) {
            continue;
        }
        let los = check_los(&world.terrain, unit.pos, enemy.pos, unit.vision_m, world.night);
        if !los.visible {
            continue;
        }
        let Ok(cell) = world.terrain.geom.world_to_cell(enemy.pos) else {
            continue;
        };
        let replace = match best {
            None => true,
            Some((bd, bid, ..)) => los.dist_m < bd - 1e-3 || ((los.dist_m - bd).abs() <= 1e-3 && uid.0 < bid),
        };
        if replace {
            best = Some((los.dist_m, uid.0, enemy.pos, cell));
        }
    }
    best.map(|(_, _, pos, cell)| (pos, cell))
}

pub fn generate_work_points(work: EngineerWorkKind, waypoints: &[WorldPos]) -> Vec<WorldPos> {
    if waypoints.is_empty() {
        return Vec::new();
    }
    let raw: Vec<[f32; 2]> = waypoints.iter().map(|p| [p.x, p.y]).collect();
    let pts = match work {
        EngineerWorkKind::Minefield if raw.len() >= 3 => generate_quincunx_minefield(&raw, 8.0, 2026),
        EngineerWorkKind::FoxholeLine if raw.len() >= 2 => generate_miter_offset_foxholes(&raw, 0.0, 8.0),
        EngineerWorkKind::ZigzagTrench if raw.len() >= 2 => generate_zigzag_trench(&raw, 8.0, 1.2),
        _ => raw.into_iter().map(quantize_pos_78125).collect(),
    };
    pts.into_iter().map(|p| WorldPos::new(p[0], p[1])).collect()
}

fn update_team_i_intel_after_commit(world: &mut WorldState, tick_logs: &[CombatLogEntry]) {
    let mut cell_targets: BTreeMap<usize, WorldPos> = BTreeMap::new();
    for u in world.units.values() {
        if u.alive {
            if let Ok(cell) = world.terrain.geom.world_to_cell(u.pos) {
                if let Ok(cidx) = world.terrain.idx(cell) {
                    cell_targets.entry(cidx).or_insert(u.pos);
                }
            }
        }
    }
    for inst in world.world_instances.values() {
        if inst.active {
            if let Ok(cell) = world.terrain.geom.world_to_cell(inst.pos) {
                if let Ok(cidx) = world.terrain.idx(cell) {
                    cell_targets.entry(cidx).or_insert(inst.pos);
                }
            }
        }
    }

    let observers: Vec<(Team, WorldPos, f32)> = world
        .units
        .values()
        .filter(|u| u.alive && u.team.index().is_some())
        .map(|u| (u.team, u.pos, u.vision_m))
        .collect();

    let mut injections: Vec<(Team, usize, f32)> = Vec::new();
    for &(obs_team, obs_pos, obs_vision_m) in &observers {
        if let Ok(obs_cell) = world.terrain.geom.world_to_cell(obs_pos) {
            if let Ok(obs_cidx) = world.terrain.idx(obs_cell) {
                injections.push((obs_team, obs_cidx, 2.5));
            }
        }
        let effective_max = obs_vision_m * if world.night { 0.45 } else { 1.0 };
        for (&cidx, &target_pos) in &cell_targets {
            if obs_pos.distance_to(target_pos) > effective_max {
                continue;
            }
            let los = check_los(&world.terrain, obs_pos, target_pos, obs_vision_m, world.night);
            if los.visible {
                let j_dda = if los.dist_m <= obs_vision_m * 0.55 && los.concealment < 0.35 {
                    1.65
                } else {
                    0.75
                };
                injections.push((obs_team, cidx, j_dda));
            }
        }
    }

    for log in tick_logs {
        if log.kind != "fire" {
            continue;
        }
        let Some(actor) = world.units.get(&log.actor_id.0).filter(|u| u.alive) else {
            continue;
        };
        let Ok(actor_cell) = world.terrain.geom.world_to_cell(actor.pos) else {
            continue;
        };
        let Ok(actor_cidx) = world.terrain.idx(actor_cell) else {
            continue;
        };
        for &(obs_team, obs_pos, obs_vision_m) in &observers {
            if obs_team != actor.team && obs_pos.distance_to(actor.pos) <= obs_vision_m * 1.2 {
                injections.push((obs_team, actor_cidx, 0.28));
            }
        }
    }

    for (team, cidx, j) in injections {
        world.inject_team_intel(team, cidx, j);
    }
}
