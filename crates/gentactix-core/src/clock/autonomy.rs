use crate::clock::pathfinding::is_cell_passable;
use crate::math_ops::{
    project_orthogonal_cover, run_deterministic_multi_capacity_auction, AuctionBid, CellCoord,
    GFieldKind, UnitZohDirectiveFilter, WorldPos,
};
use crate::state::{DerivedState, IntentCandidate, IntentSource, TickScratch, WorldState};
use crate::types::{
    EngineerWorkKind, MoveStance, UnitId, UnitOrder, UnitRole, TICK_SECONDS_F32,
};
use rustc_hash::FxHashMap;

/// Runs G-field multi-capacity auction and I-field orthogonal cover seek for units
/// without an overriding DirectOrder in `scratch.intents`.
pub fn propose_autonomous_intents(
    world: &mut WorldState,
    derived: &mut DerivedState,
    scratch: &mut TickScratch,
) {
    if !world.ruleset.enable_g_field_autonomy {
        return;
    }

    run_g_field_auction(world, derived, scratch);
    run_i_field_cover_seek(world, derived, scratch);
}

fn run_g_field_auction(
    world: &mut WorldState,
    derived: &mut DerivedState,
    scratch: &mut TickScratch,
) {
    let cols = world.terrain.cols.max(1);
    let mut bids = Vec::new();
    let mut capacities: FxHashMap<usize, usize> = FxHashMap::default();
    let mut candidate_cells: Vec<(usize, WorldPos, GFieldKind, f32)> = Vec::new();

    for (&cidx, g_cell) in &derived.g_field_cells {
        let alloc = g_cell.residual_alloc();
        if alloc <= 0.01 {
            continue;
        }
        let r = (cidx / cols) as i32;
        let c = (cidx % cols) as i32;
        if let Ok(center) = world.terrain.geom.cell_center_m(CellCoord::new(r, c)) {
            candidate_cells.push((cidx, center, g_cell.kind, g_cell.g0));
            capacities.insert(cidx, (alloc.ceil() as usize).max(1));
        }
    }

    if candidate_cells.is_empty() {
        return;
    }

    for (&uid, unit) in &world.units {
        if !unit.alive || scratch.intents.contains_key(&uid) {
            continue;
        }
        for &(cidx, center, kind, _) in &candidate_cells {
            if kind == GFieldKind::Work && unit.role != UnitRole::EngineerSquad {
                continue;
            }
            if kind == GFieldKind::State && unit.role == UnitRole::PlatoonHq {
                continue;
            }
            let r = (cidx / cols) as i32;
            let c = (cidx % cols) as i32;
            if !is_cell_passable(&world.terrain, unit.mobility, CellCoord::new(r, c)) {
                continue;
            }
            let dist_m = unit.pos.distance_to(center);
            let threat = unit
                .team
                .index()
                .and_then(|ti| derived.team_i_field_threat[ti].get(cidx).copied())
                .unwrap_or_else(|| derived.i_field_threat.get(cidx).copied().unwrap_or(0.0));
            let work_bonus = if kind == GFieldKind::Work && unit.role == UnitRole::EngineerSquad {
                -500
            } else {
                0
            };
            let cost_fixed = ((dist_m * 10.0) as i64 + (threat * 500.0) as i64 + work_bonus).max(1);
            bids.push(AuctionBid {
                cost_fixed,
                unit_id: uid,
                cell_idx: cidx,
            });
        }
    }

    if bids.is_empty() {
        return;
    }

    let winners = run_deterministic_multi_capacity_auction(bids, &capacities);
    for (uid, cidx) in winners {
        let Some(unit) = world.units.get(&uid) else {
            continue;
        };
        let r = (cidx / cols) as i32;
        let c = (cidx % cols) as i32;
        let Ok(center) = world.terrain.geom.cell_center_m(CellCoord::new(r, c)) else {
            continue;
        };

        let mut is_work_on_site = false;
        let mut target_g0 = 1.0f32;
        if let Some(g_cell) = derived.g_field_cells.get_mut(&cidx) {
            target_g0 = g_cell.g0;
            g_cell.committed_en_route += 1.0;
            if g_cell.kind == GFieldKind::Work
                && unit.role == UnitRole::EngineerSquad
                && unit.pos.distance_to(center) <= 4.0
            {
                is_work_on_site = true;
            }
        }

        let eta = derived.unit_radio_t_diff.get(&uid).copied().unwrap_or(1.0);
        world
            .radio_zoh_filters
            .entry(uid)
            .or_insert_with(UnitZohDirectiveFilter::default)
            .step_zoh(&[(cidx as u32, target_g0)], eta, TICK_SECONDS_F32);

        let order = if is_work_on_site {
            UnitOrder::BuildWork {
                work: EngineerWorkKind::FoxholeLine,
                waypoints: vec![center],
            }
        } else if unit.pos.distance_to(center) > 4.0 {
            UnitOrder::Move {
                waypoints: vec![center],
                stance: MoveStance::Normal,
            }
        } else {
            continue;
        };

        scratch.submit_intent(IntentCandidate {
            unit_id: UnitId(uid),
            source: IntentSource::Autonomy,
            priority: 10,
            order,
        });
    }
}

fn run_i_field_cover_seek(
    world: &WorldState,
    derived: &DerivedState,
    scratch: &mut TickScratch,
) {
    for (&uid, unit) in &world.units {
        if !unit.alive || scratch.intents.contains_key(&uid) {
            continue;
        }
        let Ok(cur_cell) = world.terrain.geom.world_to_cell(unit.pos) else {
            continue;
        };
        let Ok(cur_idx) = world.terrain.idx(cur_cell) else {
            continue;
        };
        let cur_threat = unit
            .team
            .index()
            .and_then(|ti| derived.team_i_field_threat[ti].get(cur_idx).copied())
            .unwrap_or_else(|| derived.i_field_threat.get(cur_idx).copied().unwrap_or(0.0));
        if cur_threat <= 1.0 {
            continue;
        }

        let cur_cover = world.terrain.kind[cur_idx].cover_factor() + world.terrain.cover_bonus[cur_idx];
        let mut grad_safe = [0.0f32; 2];
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let n = CellCoord::new(cur_cell.r + dr, cur_cell.c + dc);
                if let Ok(nidx) = world.terrain.idx(n) {
                    let t = unit
                        .team
                        .index()
                        .and_then(|ti| derived.team_i_field_threat[ti].get(nidx).copied())
                        .unwrap_or_else(|| derived.i_field_threat.get(nidx).copied().unwrap_or(0.0));
                    let delta = cur_threat - t;
                    grad_safe[0] += (dc as f32) * delta;
                    grad_safe[1] += (dr as f32) * delta;
                }
            }
        }

        let rad = unit.facing_deg.to_radians();
        let ortho = project_orthogonal_cover(grad_safe, [rad.cos(), rad.sin()]);
        let mut best_score = cur_cover * 10.0 - cur_threat * 5.0;
        let mut best_target: Option<WorldPos> = None;

        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let n = CellCoord::new(cur_cell.r + dr, cur_cell.c + dc);
                if !is_cell_passable(&world.terrain, unit.mobility, n) {
                    continue;
                }
                let Ok(nidx) = world.terrain.idx(n) else {
                    continue;
                };
                let n_threat = unit
                    .team
                    .index()
                    .and_then(|ti| derived.team_i_field_threat[ti].get(nidx).copied())
                    .unwrap_or_else(|| derived.i_field_threat.get(nidx).copied().unwrap_or(0.0));
                let n_cover = world.terrain.kind[nidx].cover_factor() + world.terrain.cover_bonus[nidx];
                let align = (dc as f32) * ortho[0] + (dr as f32) * ortho[1];
                let score = n_cover * 10.0 - n_threat * 5.0 + align * 0.25;
                if score > best_score {
                    if let Ok(center) = world.terrain.geom.cell_center_m(n) {
                        best_score = score;
                        best_target = Some(center);
                    }
                }
            }
        }

        if let Some(target_pos) = best_target {
            scratch.submit_intent(IntentCandidate {
                unit_id: UnitId(uid),
                source: IntentSource::Autonomy,
                priority: 5,
                order: UnitOrder::Move {
                    waypoints: vec![target_pos],
                    stance: MoveStance::Cautious,
                },
            });
        }
    }
}
