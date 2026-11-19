use crate::world_grid::{MobilityClass, WorldGrid};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone)]
struct AStarNode {
    cost: f32,
    f: f32,
    r: i32,
    c: i32,
}

impl Eq for AStarNode {}
impl PartialEq for AStarNode {
    fn eq(&self, other: &Self) -> bool {
        self.r == other.r && self.c == other.c
    }
}
impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f)
    }
}
impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn plan_grid_path_f32(grid: &WorldGrid, start: [f32; 2], goal: [f32; 2], mob: MobilityClass) -> Vec<[f32; 2]> {
    let rows = grid.rows as i32;
    let cols = grid.cols as i32;
    let sr = (start[0].round() as i32).clamp(0, rows - 1);
    let sc = (start[1].round() as i32).clamp(0, cols - 1);
    let gr = (goal[0].round() as i32).clamp(0, rows - 1);
    let gc = (goal[1].round() as i32).clamp(0, cols - 1);
    let clamped_goal = [goal[0].clamp(0.0, (rows - 1) as f32), goal[1].clamp(0.0, (cols - 1) as f32)];
    if sr == gr && sc == gc {
        return vec![clamped_goal];
    }

    let n = (rows * cols) as usize;
    let mut g_score = vec![f32::INFINITY; n];
    let mut parent = vec![-1i32; n];
    let mut heap = BinaryHeap::new();
    let s_idx = (sr * cols + sc) as usize;
    let g_idx = (gr * cols + gc) as usize;
    g_score[s_idx] = 0.0;
    heap.push(AStarNode {
        cost: 0.0,
        f: ((gr - sr) as f32).hypot((gc - sc) as f32),
        r: sr,
        c: sc,
    });

    const DIRS: [(i32, i32); 8] = [
        (-1, 0), (1, 0), (0, -1), (0, 1),
        (-1, -1), (-1, 1), (1, -1), (1, 1),
    ];

    let mut found = false;
    while let Some(cur) = heap.pop() {
        let c_idx = (cur.r * cols + cur.c) as usize;
        if c_idx == g_idx {
            found = true;
            break;
        }
        if cur.cost > g_score[c_idx] + 1e-5 {
            continue;
        }
        for &(dr, dc) in &DIRS {
            let nr = cur.r + dr;
            let nc = cur.c + dc;
            if nr < 0 || nr >= rows || nc < 0 || nc >= cols {
                continue;
            }
            let step_cost = grid.move_cost_step(cur.r, cur.c, nr, nc, mob, 1.0);
            if !step_cost.is_finite() {
                continue;
            }
            let dist_mult = if dr != 0 && dc != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            let forest_pen = 1.0 + grid.forest_density_at(nr, nc) * 0.65;
            let next_g = cur.cost + step_cost * dist_mult * forest_pen;
            let n_idx = (nr * cols + nc) as usize;
            if next_g + 1e-5 < g_score[n_idx] {
                g_score[n_idx] = next_g;
                parent[n_idx] = c_idx as i32;
                let h = ((gr - nr) as f32).hypot((gc - nc) as f32) * 0.25 * (grid.cell_m / 1000.0);
                heap.push(AStarNode { cost: next_g, f: next_g + h, r: nr, c: nc });
            }
        }
    }

    if !found {
        return vec![clamped_goal];
    }

    let mut cells = Vec::new();
    let mut cur = g_idx as i32;
    while cur >= 0 && cur != s_idx as i32 {
        let r = cur / cols;
        let c = cur % cols;
        cells.push([r as f32, c as f32]);
        cur = parent[cur as usize];
    }
    cells.reverse();
    if let Some(last) = cells.last_mut() {
        *last = clamped_goal;
    } else {
        cells.push(clamped_goal);
    }
    cells
}

