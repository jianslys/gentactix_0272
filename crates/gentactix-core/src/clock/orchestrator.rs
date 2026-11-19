use crate::catalog::ItemCatalog;
use crate::clock::autonomy::propose_autonomous_intents;
use crate::clock::los_combat::{check_los, resolve_fire_exchange, LosResult};
use crate::clock::pathfinding::{
    constrain_segment_motion, find_path_world, is_cell_passable, segment_has_obstacle,
};
use crate::clock::phases::TickStage;
use crate::math_ops::{
    apply_g_work_consumption, generate_miter_offset_foxholes, generate_quincunx_minefield,
    generate_zigzag_trench, quantize_pos_78125, AugmentedArcIntegrator, CellCoord,
    SchmidtPostureState, WorldPos, ALPHA_BLAST, ALPHA_SUPP,
};
use crate::state::{
    CombatLogEntry, DerivedState, EventRecord, EventTerminalStatus, IntentCandidate, IntentSource,
    MotionProposal, TickScratch, UnitRecord, WorldState,
};
use crate::types::{
    EngineerWorkKind, FireDiscipline, UnitId, UnitOrder, TICK_SECONDS_F32,
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
    pub tick_logs: Vec<CombatLogEntry>,
    pub authoritative_sha256: String,
}

enum EventDetail {
    Fire {
        target_id: Option<UnitId>,
        target_cell: CellCoord,
        los: LosResult,
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
    FireDamage {
        attacker_id: UnitId,
        target_id: Option<UnitId>,
        target_cell: CellCoord,
        rounds_fired: u32,
        casualties: u16,
        hp_loss: f32,
        suppression_add: f32,
        blast_kpa: f32,
        crater_depth_m: f32,
        dist_m: f32,
        message: String,
    },
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
        world.terrain.decay_smoke(0.85);
        for unit in world.units.values_mut() {
            if !unit.alive {
                continue;
            }
            if unit.suppression > 0.0 {
                unit.suppression *= ALPHA_SUPP;
                if unit.suppression < 1e-4 {
                    unit.suppression = 0.0;
                }
            }
            unit.posture = if world.ruleset.enforce_suppression_posture {
                unit.posture.transition(unit.suppression)
            } else {
                SchmidtPostureState::Active
            };
        }
        stages_executed.push(TickStage::PersistentFieldEvolve);

        // Stage 3: DerivedRefresh (stateless Deygout radio recalculation)
        derived.rebuild_if_needed(world);
        for (&uid, unit) in &mut world.units {
            if unit.alive {
                let t_diff = derived.unit_radio_t_diff.get(&uid).copied().unwrap_or(1.0);
                unit.effective_radio_range_m = unit.base_radio_range_m * t_diff;
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
            if let Some(prop) = build_motion_proposal(world, unit, &cand.order) {
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
            let mut safe_to = constrain_segment_motion(&world.terrain, unit.mobility, prop.from, prop.to);
            if let (Ok(c_from), Ok(c_to)) = (
                world.terrain.geom.world_to_cell(prop.from),
                world.terrain.geom.world_to_cell(safe_to),
            ) {
                if c_to != c_from {
                    let count_to = cell_occupancy.get(&(c_to.r, c_to.c)).copied().unwrap_or(0);
                    if count_to >= 2 {
                        safe_to = prop.from;
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

            if unit.fire_discipline == FireDiscipline::HoldFire {
                continue;
            }
            if let Some((tgt_id, tcell, los)) = select_fire_target(world, derived, &unit, &active_order) {
                let eid = world.alloc_event_id();
                scratch.events.push(EventRecord {
                    event_id: eid,
                    tick: world.tick,
                    source: unit.id,
                    kind: "fire".into(),
                    target_cell: Some(tcell),
                });
                details.insert(
                    eid,
                    EventDetail::Fire {
                        target_id: tgt_id,
                        target_cell: tcell,
                        los,
                    },
                );
            }
        }
        stages_executed.push(TickStage::EmitEvents);

        // Stage 9: ResolveEvents
        let events_snapshot = scratch.events.clone();
        let mut mutations: Vec<PendingMutation> = Vec::new();
        for ev in &events_snapshot {
            match details.get(&ev.event_id) {
                Some(EventDetail::MineTrigger { cell }) => {
                    let _ = scratch.blast_workspace.splat_explosion(
                        &world.terrain.geom,
                        *cell,
                        12.0,
                        110.0,
                        40,
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
                Some(EventDetail::Fire {
                    target_id,
                    target_cell,
                    los,
                }) => {
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
                    let target_rec = target_id.and_then(|tid| world.units.get(&tid.0));
                    let Some(tgt) = target_rec else {
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Rejected {
                                code: "MISSING_TARGET".into(),
                                detail: "target missing".into(),
                            },
                        );
                        continue;
                    };
                    let t_cover = world
                        .terrain
                        .idx(*target_cell)
                        .map(|i| world.terrain.kind[i].cover_factor() + world.terrain.cover_bonus[i])
                        .unwrap_or(0.0);
                    let outcome = resolve_fire_exchange(
                        world.clock_seconds,
                        &world.ruleset,
                        catalog,
                        attacker,
                        tgt,
                        los,
                        t_cover,
                    );
                    if outcome.rounds_fired == 0 {
                        scratch.terminal_statuses.insert(
                            ev.event_id,
                            EventTerminalStatus::Rejected {
                                code: "OUT_OF_AMMO_OR_DUD".into(),
                                detail: "no rounds fired".into(),
                            },
                        );
                        continue;
                    }
                    if outcome.frag_radius_m > 0.0 {
                        let _ = scratch.blast_workspace.splat_explosion(
                            &world.terrain.geom,
                            *target_cell,
                            outcome.frag_radius_m.min(32.0),
                            outcome.blast_kpa,
                            outcome.rounds_fired * 20,
                            ev.event_id,
                        );
                    }
                    mutations.push(PendingMutation::FireDamage {
                        attacker_id: attacker.id,
                        target_id: *target_id,
                        target_cell: *target_cell,
                        rounds_fired: outcome.rounds_fired,
                        casualties: outcome.casualties,
                        hp_loss: outcome.hp_loss,
                        suppression_add: outcome.suppression_add,
                        blast_kpa: outcome.blast_kpa,
                        crater_depth_m: outcome.crater_depth_m,
                        dist_m: los.dist_m,
                        message: format!(
                            "{} -> {} rounds={} cas={} hp_loss={:.1} supp+={:.2}",
                            attacker.callsign,
                            tgt.callsign,
                            outcome.rounds_fired,
                            outcome.casualties,
                            outcome.hp_loss,
                            outcome.suppression_add
                        ),
                    });
                    scratch.terminal_statuses.insert(
                        ev.event_id,
                        EventTerminalStatus::Resolved {
                            code: "FIRE_RESOLVED".into(),
                            detail: format!("hit_prob={:.2}", outcome.hit_prob),
                        },
                    );
                }
                None => {}
            }
        }
        stages_executed.push(TickStage::ResolveEvents);

        // Stage 10: CommitPersistent (SINGLE COMBAT / TERRAIN / WORK WRITE POINT)
        let mut fire_events = 0usize;
        let mut engineer_events = 0usize;
        let mut smoke_events = 0usize;
        let mut mine_events = 0usize;
        let mut casualties_total = 0u16;
        let mut tick_logs = Vec::new();
        let mut state_mutated = false;

        for m in mutations {
            match m {
                PendingMutation::FireDamage {
                    attacker_id,
                    target_id,
                    target_cell,
                    rounds_fired,
                    casualties,
                    hp_loss,
                    suppression_add,
                    blast_kpa,
                    crater_depth_m,
                    dist_m,
                    message,
                } => {
                    fire_events += 1;
                    state_mutated = true;
                    if world.ruleset.enforce_ammo_consumption {
                        if let Some(att) = world.units.get_mut(&attacker_id.0) {
                            att.ammo_rounds = att.ammo_rounds.saturating_sub(rounds_fired);
                        }
                    }
                    if let Ok(cidx) = world.terrain.idx(target_cell) {
                        if cidx < world.cell_s_supp.len() {
                            world.cell_s_supp[cidx] += suppression_add;
                        }
                        if blast_kpa > 0.0 && cidx < world.cell_h_blast.len() {
                            world.cell_h_blast[cidx] += blast_kpa;
                        }
                        if crater_depth_m > 0.0 {
                            let _ = world.terrain.mutate_cell(
                                target_cell,
                                Some(-crater_depth_m),
                                None,
                                None,
                                None,
                            );
                        }
                    }
                    if let Some(tid) = target_id {
                        if let Some(tgt) = world.units.get_mut(&tid.0) {
                            apply_unit_damage(
                                tgt,
                                hp_loss,
                                casualties,
                                suppression_add,
                                world.ruleset.enforce_suppression_posture,
                            );
                        }
                    }
                    casualties_total = casualties_total.saturating_add(casualties);
                    let entry = CombatLogEntry {
                        tick: world.tick,
                        actor_id: attacker_id,
                        target_id,
                        kind: "fire".into(),
                        dist_m,
                        casualties,
                        hp_loss,
                        suppression_add,
                        ammo_spent: rounds_fired,
                        message,
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
                                }
                                EngineerWorkKind::Minefield => {
                                    let _ = world.terrain.add_mine_density(cell, 0.80);
                                }
                                EngineerWorkKind::Demolition => {
                                    let _ = world.terrain.mutate_cell(
                                        cell,
                                        Some(-2.0),
                                        None,
                                        None,
                                        None,
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
                    if let Ok(cidx) = world.terrain.idx(cell) {
                        if cidx < world.cell_h_blast.len() {
                            world.cell_h_blast[cidx] += 90.0;
                            world.cell_s_supp[cidx] += 0.9;
                        }
                    }
                    if let Some(u) = world.units.get_mut(&unit_id.0) {
                        apply_unit_damage(
                            u,
                            18.0,
                            1,
                            0.85,
                            world.ruleset.enforce_suppression_posture,
                        );
                    }
                    casualties_total = casualties_total.saturating_add(1);
                }
            }
        }
        if state_mutated {
            world.unit_revision = world.unit_revision.wrapping_add(1);
        }
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
        world.terrain.clear_dirty_halo();
        derived.rebuild_if_needed(world);
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
            tick_logs,
            authoritative_sha256: world.authoritative_sha256(),
        }
    }
}

fn build_motion_proposal(
    world: &WorldState,
    unit: &UnitRecord,
    order: &UnitOrder,
) -> Option<MotionProposal> {
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
                return Some(MotionProposal {
                    unit_id: unit.id,
                    from: unit.pos,
                    to,
                    speed_mps: unit.base_speed_mps,
                });
            }
        }
        _ => return None,
    };

    let next_wp = if segment_has_obstacle(&world.terrain, unit.mobility, unit.pos, goal) {
        let path = find_path_world(&world.terrain, unit.mobility, unit.pos, goal, 12_000)?;
        path.into_iter()
            .find(|wp| unit.pos.distance_to(*wp) > 0.5)
            .unwrap_or(goal)
    } else {
        goal
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
    let speed_mps = (unit.base_speed_mps * stance_mult * supp_mult) / terrain_cost.max(0.5);
    let step_dist = speed_mps * TICK_SECONDS_F32;
    let dist = unit.pos.distance_to(next_wp);
    let to = if dist <= step_dist || dist <= 1e-4 {
        next_wp
    } else {
        let f = step_dist / dist;
        WorldPos::new(
            unit.pos.x + (next_wp.x - unit.pos.x) * f,
            unit.pos.y + (next_wp.y - unit.pos.y) * f,
        )
    };

    Some(MotionProposal {
        unit_id: unit.id,
        from: unit.pos,
        to,
        speed_mps,
    })
}

fn select_fire_target(
    world: &WorldState,
    derived: &DerivedState,
    unit: &UnitRecord,
    active_order: &UnitOrder,
) -> Option<(Option<UnitId>, CellCoord, LosResult)> {
    if let UnitOrder::AttackUnit { target, .. } = active_order {
        if let Some(enemy) = world.units.get(&target.0).filter(|e| e.alive && e.team.is_hostile_to(unit.team)) {
            let los = check_los(&world.terrain, unit.pos, enemy.pos, unit.vision_m, world.night);
            if los.visible {
                if let Ok(cell) = world.terrain.geom.world_to_cell(enemy.pos) {
                    return Some((Some(enemy.id), cell, los));
                }
            }
        }
        return None;
    }

    let nearby = derived.query_units_in_radius(world, unit.pos, unit.vision_m);
    let mut best: Option<(f32, u32, UnitId, CellCoord, LosResult)> = None;
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
            best = Some((los.dist_m, uid.0, enemy.id, cell, los));
        }
    }
    best.map(|(_, _, id, cell, los)| (Some(id), cell, los))
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

fn apply_unit_damage(
    unit: &mut UnitRecord,
    hp_loss: f32,
    casualties: u16,
    suppression_add: f32,
    enforce_posture: bool,
) {
    unit.hp = (unit.hp - hp_loss).max(0.0);
    unit.men = unit.men.saturating_sub(casualties);
    unit.suppression += suppression_add;
    if enforce_posture {
        unit.posture = unit.posture.transition(unit.suppression);
    }
    if unit.hp <= 0.0 || unit.men == 0 {
        unit.alive = false;
        unit.hp = 0.0;
        unit.men = 0;
    }
}
