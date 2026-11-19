use crate::math_ops::{CellCoord, WorldPos, TACTICAL_CELL_SIZE_M};
use crate::terrain::{TerrainGrid, TerrainKind};
use crate::types::MobilityKind;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

const COST_SCALE: i64 = 1000;
const STEP_SAMPLE_M: f32 = 3.5; // strictly < 4.0m (half of 8.0m tactical cell)

#[inline]
pub fn is_cell_passable(terrain: &TerrainGrid, mob: MobilityKind, cell: CellCoord) -> bool {
    let Ok(idx) = terrain.idx(cell) else {
        return false;
    };
    if terrain.kind[idx].move_cost(mob).is_none() {
        return false;
    }
    let slope = terrain.slope_deg[idx];
    match mob {
        MobilityKind::Foot => slope <= 50.0,
        MobilityKind::Wheeled | MobilityKind::Tracked => slope <= 35.0,
    }
}

#[inline]
pub fn is_point_passable(terrain: &TerrainGrid, mob: MobilityKind, pos: WorldPos) -> bool {
    let Ok(cell) = terrain.geom.world_to_cell(pos) else {
        return false;
    };
    is_cell_passable(terrain, mob, cell)
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct AStarNode {
    f_cost: i64,
    g_cost: i64,
    r: i32,
    c: i32,
}

impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f_cost
            .cmp(&self.f_cost)
            .then_with(|| other.g_cost.cmp(&self.g_cost))
            .then_with(|| other.r.cmp(&self.r))
            .then_with(|| other.c.cmp(&self.c))
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[inline]
fn octile_heuristic(a: CellCoord, b: CellCoord) -> i64 {
    let dr = (a.r - b.r).abs() as i64;
    let dc = (a.c - b.c).abs() as i64;
    let (mn, mx) = if dr < dc { (dr, dc) } else { (dc, dr) };
    // Admissible floor: minimum cost multiplier is 0.65 (wheeled road) => 650 per 1000
    (mn * 919) + (mx - mn) * 650
}

#[inline]
fn cell_traversal_factor(terrain: &TerrainGrid, mob: MobilityKind, idx: usize) -> i64 {
    let base = terrain.kind[idx].move_cost(mob).unwrap_or(1.0);
    let slope = terrain.slope_deg[idx];
    let slope_mult = match mob {
        MobilityKind::Foot if slope > 20.0 => 1.0 + (slope - 20.0) * 0.05,
        MobilityKind::Wheeled | MobilityKind::Tracked if slope > 15.0 => 1.0 + (slope - 15.0) * 0.06,
        _ => 1.0,
    };
    ((base * slope_mult) * (COST_SCALE as f32)).round().max(650.0) as i64
}

pub fn find_path_world(
    terrain: &TerrainGrid,
    mob: MobilityKind,
    start: WorldPos,
    goal: WorldPos,
    max_expansions: usize,
) -> Option<Vec<WorldPos>> {
    let start_cell = terrain.geom.world_to_cell(start).ok()?;
    let goal_cell = terrain.geom.world_to_cell(goal).ok()?;
    if !is_cell_passable(terrain, mob, goal_cell) {
        return None;
    }
    if start_cell == goal_cell {
        return Some(vec![goal]);
    }

    let mut open = BinaryHeap::new();
    let mut g_scores: FxHashMap<(i32, i32), i64> = FxHashMap::default();
    let mut parents: FxHashMap<(i32, i32), (i32, i32)> = FxHashMap::default();

    let s_key = (start_cell.r, start_cell.c);
    let g_key = (goal_cell.r, goal_cell.c);
    g_scores.insert(s_key, 0);
    open.push(AStarNode {
        f_cost: octile_heuristic(start_cell, goal_cell),
        g_cost: 0,
        r: start_cell.r,
        c: start_cell.c,
    });

    const DIRS: [(i32, i32, i64); 8] = [
        (-1, 0, 1000),
        (1, 0, 1000),
        (0, -1, 1000),
        (0, 1, 1000),
        (-1, -1, 1414),
        (-1, 1, 1414),
        (1, -1, 1414),
        (1, 1, 1414),
    ];

    let mut expansions = 0usize;
    let mut reached = false;
    let limit = max_expansions.max(64);

    while let Some(cur) = open.pop() {
        let c_key = (cur.r, cur.c);
        if c_key == g_key {
            reached = true;
            break;
        }
        if g_scores.get(&c_key).copied().unwrap_or(i64::MAX) < cur.g_cost {
            continue;
        }
        expansions += 1;
        if expansions > limit {
            break;
        }

        for &(dr, dc, step_scale) in &DIRS {
            let nr = cur.r + dr;
            let nc = cur.c + dc;
            let next_cell = CellCoord::new(nr, nc);
            if !is_cell_passable(terrain, mob, next_cell) {
                continue;
            }
            if dr != 0
                && dc != 0
                && (!is_cell_passable(terrain, mob, CellCoord::new(cur.r + dr, cur.c))
                    || !is_cell_passable(terrain, mob, CellCoord::new(cur.r, cur.c + dc)))
            {
                continue;
            }
            let Ok(idx) = terrain.idx(next_cell) else {
                continue;
            };
            let cell_factor = cell_traversal_factor(terrain, mob, idx);
            let step_cost = (step_scale * cell_factor) / COST_SCALE;
            let tentative_g = cur.g_cost.saturating_add(step_cost);

            let n_key = (nr, nc);
            if tentative_g < g_scores.get(&n_key).copied().unwrap_or(i64::MAX) {
                g_scores.insert(n_key, tentative_g);
                parents.insert(n_key, c_key);
                let h = octile_heuristic(next_cell, goal_cell);
                open.push(AStarNode {
                    f_cost: tentative_g.saturating_add(h),
                    g_cost: tentative_g,
                    r: nr,
                    c: nc,
                });
            }
        }
    }

    if !reached {
        return None;
    }

    let mut raw_cells = Vec::new();
    let mut trace = g_key;
    raw_cells.push(CellCoord::new(trace.0, trace.1));
    while let Some(&prev) = parents.get(&trace) {
        if prev == s_key {
            break;
        }
        raw_cells.push(CellCoord::new(prev.0, prev.1));
        trace = prev;
    }
    raw_cells.reverse();

    let mut waypoints = Vec::with_capacity(raw_cells.len());
    for i in 0..raw_cells.len() {
        let is_last = i + 1 == raw_cells.len();
        if is_last {
            waypoints.push(goal);
            continue;
        }
        let is_bridge = terrain
            .idx(raw_cells[i])
            .is_ok_and(|idx| terrain.kind[idx] == TerrainKind::Bridge);
        if !is_bridge && i > 0 {
            let prev = raw_cells[i - 1];
            let cur = raw_cells[i];
            let next = raw_cells[i + 1];
            if (cur.r - prev.r, cur.c - prev.c) == (next.r - cur.r, next.c - cur.c) {
                continue;
            }
        }
        if let Ok(center) = terrain.geom.cell_center_m(raw_cells[i]) {
            waypoints.push(center);
        }
    }
    if waypoints.is_empty() {
        waypoints.push(goal);
    }
    Some(waypoints)
}

/// Continuous anti-tunneling sweep at <= 3.5m spacing (strictly < 4.0m half-cell limit).
pub fn constrain_segment_motion(
    terrain: &TerrainGrid,
    mob: MobilityKind,
    from: WorldPos,
    to: WorldPos,
) -> WorldPos {
    if !is_point_passable(terrain, mob, from) {
        return from;
    }
    let dist = from.distance_to(to);
    if dist <= 1e-4 {
        return from;
    }
    let steps = ((dist / STEP_SAMPLE_M).ceil() as usize).max(1);
    let mut last_valid = from;
    for s in 1..=steps {
        let t = (s as f32) / (steps as f32);
        let probe = WorldPos::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        let Ok(cell) = terrain.geom.world_to_cell(probe) else {
            break;
        };
        let Ok(idx) = terrain.idx(cell) else {
            break;
        };
        if terrain.kind[idx].move_cost(mob).is_none() {
            break;
        }
        if mob != MobilityKind::Foot && terrain.slope_deg[idx] > 42.0 {
            break;
        }
        last_valid = probe;
    }
    last_valid
}

#[inline]
pub fn segment_has_obstacle(terrain: &TerrainGrid, mob: MobilityKind, from: WorldPos, to: WorldPos) -> bool {
    let clamped = constrain_segment_motion(terrain, mob, from, to);
    clamped.distance_to(to) > TACTICAL_CELL_SIZE_M * 0.25
}
