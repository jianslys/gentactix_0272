use super::*;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

impl WorldGrid {

    pub fn move_cost_step(&self, r0: i32, c0: i32, r1: i32, c1: i32, mob: MobilityClass, extra_mult: f32) -> f32 {
        if !self.in_bounds(r1, c1) {
            return f32::INFINITY;
        }
        let i1 = self.idx(r1, c1);
        let t1 = self.terrain[i1];
        if t1 == WATER {
            return f32::INFINITY;
        }
        let cm1 = self.get_cell_modifiers(r1, c1, mob);
        if cm1.impassable {
            return f32::INFINITY;
        }
        if !cm1.ignore_slope_limit && self.slope_deg[i1] > mob.slope_limit_deg() {
            return f32::INFINITY;
        }
        let tc = terrain_cost(mob, t1);
        if !tc.is_finite() {
            return f32::INFINITY;
        }
        let w1 = (self.weather[i1] as usize).min(5);
        let w_mult = if cm1.is_road { WEATHER_ROAD[w1] } else { WEATHER_OFFROAD[w1] };

        let horiz_m = (((r1 - r0) as f32).hypot((c1 - c0) as f32)) * self.cell_m;
        let dz = self.elevation_m[i1] - self.elevation_m[self.idx(r0, c0)];
        let grade = dz / horiz_m.max(1.0);
        let slope_mult = if grade >= 0.0 {
            1.0 + mob.uphill_k() * grade
        } else {
            (1.0 + 0.5 * grade).max(0.85)
        };
        let horiz_km = horiz_m / 1000.0;
        let speed_div = cm1.speed_mult.max(0.05);
        horiz_km * tc * w_mult * slope_mult * extra_mult / speed_div
    }

    pub fn shortest_path_windowed<F>(
        &self,
        start: (i32, i32),
        target: (i32, i32),
        mob: MobilityClass,
        blocked: F,
        extra_mult: f32,
    ) -> Option<(Vec<(i32, i32)>, Vec<f32>)>
    where
        F: Fn(i32, i32) -> bool,
    {
        let s = self.clamp_rc(start.0, start.1);
        let t = self.clamp_rc(target.0, target.1);
        if s == t {
            return Some((vec![s], vec![0.0]));
        }
        if !self.in_bounds(t.0, t.1) || self.terrain[self.idx(t.0, t.1)] == WATER {
            return None;
        }
        let span = (t.0 - s.0).abs().max((t.1 - s.1).abs());
        let pad = (span + 24).min(96);
        let r_min = (s.0.min(t.0) - pad).max(0);
        let r_max = (s.0.max(t.0) + pad).min((self.rows as i32) - 1);
        let c_min = (s.1.min(t.1) - pad).max(0);
        let c_max = (s.1.max(t.1) + pad).min((self.cols as i32) - 1);

        let min_step_factor = 0.35 * (self.cell_m / 1000.0);

        #[derive(Copy, Clone, PartialEq)]
        struct QNode {
            f: f32,
            g: f32,
            r: i32,
            c: i32,
        }
        impl Eq for QNode {}
        impl Ord for QNode {
            fn cmp(&self, o: &Self) -> Ordering {
                o.f.total_cmp(&self.f).then_with(|| o.r.cmp(&self.r)).then_with(|| o.c.cmp(&self.c))
            }
        }
        impl PartialOrd for QNode {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }

        let mut g_score: FxHashMap<(i32, i32), f32> = FxHashMap::default();
        let mut came_from: FxHashMap<(i32, i32), (i32, i32)> = FxHashMap::default();
        let mut pq = BinaryHeap::new();
        g_score.insert(s, 0.0);
        let h0 = ((t.0 - s.0) as f32).hypot((t.1 - s.1) as f32) * min_step_factor;
        pq.push(QNode { f: h0, g: 0.0, r: s.0, c: s.1 });

        const DIRS: [(i32, i32); 8] = [
            (-1, 0), (1, 0), (0, -1), (0, 1),
            (-1, -1), (-1, 1), (1, -1), (1, 1),
        ];

        while let Some(cur) = pq.pop() {
            if (cur.r, cur.c) == t {
                let mut rev_path = Vec::new();
                let mut p = t;
                rev_path.push(p);
                while let Some(&prev) = came_from.get(&p) {
                    rev_path.push(prev);
                    p = prev;
                }
                rev_path.reverse();
                let mut costs = Vec::with_capacity(rev_path.len());
                for pt in &rev_path {
                    costs.push(*g_score.get(pt).unwrap_or(&0.0));
                }
                return Some((rev_path, costs));
            }
            if cur.g > *g_score.get(&(cur.r, cur.c)).unwrap_or(&f32::INFINITY) + 1e-6 {
                continue;
            }
            for &(dr, dc) in &DIRS {
                let nr = cur.r + dr;
                let nc = cur.c + dc;
                if nr < r_min || nr > r_max || nc < c_min || nc > c_max {
                    continue;
                }
                if (nr, nc) != t && blocked(nr, nc) {
                    continue;
                }
                let step = self.move_cost_step(cur.r, cur.c, nr, nc, mob, extra_mult);
                if !step.is_finite() {
                    continue;
                }
                let ng = cur.g + step;
                if ng + 1e-6 < *g_score.get(&(nr, nc)).unwrap_or(&f32::INFINITY) {
                    g_score.insert((nr, nc), ng);
                    came_from.insert((nr, nc), (cur.r, cur.c));
                    let h = ((t.0 - nr) as f32).hypot((t.1 - nc) as f32) * min_step_factor;
                    pq.push(QNode { f: ng + h, g: ng, r: nr, c: nc });
                }
            }
        }
        None
    }

    pub fn advance_along(path: &[(i32, i32)], costs: &[f32], budget_km: f32) -> ((i32, i32), f32, bool) {
        if path.is_empty() {
            return ((0, 0), 0.0, false);
        }
        let mut best_i = 0usize;
        for (i, &c) in costs.iter().enumerate() {
            if c <= budget_km + 1e-6 {
                best_i = i;
            } else {
                break;
            }
        }
        let arrived = best_i + 1 == path.len();
        (path[best_i], costs[best_i], arrived)
    }

    pub fn write_ao_cells(&mut self, zone_id: u32, team: TeamId, echelon_rank: u8, cells: &[(i32, i32)]) {
        let e_bit = 1u8 << (echelon_rank.min(7));
        let (owners, masks) = match team {
            TeamId::Red => (&mut self.ao_owner_red, &mut self.ao_echelon_mask_red),
            TeamId::Blue => (&mut self.ao_owner_blue, &mut self.ao_echelon_mask_blue),
        };
        for &(r, c) in cells {
            if r >= 0 && (r as usize) < self.rows && c >= 0 && (c as usize) < self.cols {
                let i = (r as usize) * self.cols + (c as usize);
                owners[i] = zone_id;
                masks[i] |= e_bit;
            }
        }
    }

    pub fn write_typed_frontline(
        &mut self,
        name_id: &str,
        team: TeamId,
        commander_id: usize,
        zone_id: u32,
        role: FrontlineRole,
        source_deploy_line_id: u32,
        target_deploy_line_id: u32,
        unit_type_mask: u8,
        attraction_weight: f32,
        front_normal: Option<[f32; 2]>,
        points_px: &[[f32; 2]],
        spread_radius_px: i32,
    ) -> u32 {
        let (cells, def_norm) = rasterize_polyline(points_px, self.rows, self.cols);
        let normal = front_normal.unwrap_or(def_norm);
        let n_len = normal[0].hypot(normal[1]).max(1e-6);
        let normal = [normal[0] / n_len, normal[1] / n_len];

        let mut tangents = Vec::with_capacity(cells.len());
        for i in 0..cells.len() {
            let p_prev = cells[i.saturating_sub(1)];
            let p_next = cells[(i + 1).min(cells.len() - 1)];
            let dr = (p_next.0 - p_prev.0) as f32;
            let dc = (p_next.1 - p_prev.1) as f32;
            let l = dr.hypot(dc);
            if l > 1e-6 {
                tangents.push([dr / l, dc / l]);
            } else {
                tangents.push([-normal[1], normal[0]]);
            }
        }

        let line_idx = if let Some(&existing_id) = self.frontline_id_map.get(name_id) {
            let idx = (existing_id - 1) as usize;
            self.frontlines[idx] = TypedFrontline {
                line_id: existing_id,
                name_id: name_id.to_string(),
                team,
                commander_id,
                zone_id,
                role,
                source_deploy_line_id,
                target_deploy_line_id,
                unit_type_mask,
                attraction_weight,
                front_normal: normal,
                points_px: points_px.to_vec(),
                cells: cells.clone(),
                tangents,
                spread_radius_px,
                active: true,
            };
            existing_id
        } else {
            let new_id = (self.frontlines.len() + 1) as u32;
            self.frontlines.push(TypedFrontline {
                line_id: new_id,
                name_id: name_id.to_string(),
                team,
                commander_id,
                zone_id,
                role,
                source_deploy_line_id,
                target_deploy_line_id,
                unit_type_mask,
                attraction_weight,
                front_normal: normal,
                points_px: points_px.to_vec(),
                cells: cells.clone(),
                tangents,
                spread_radius_px,
                active: true,
            });
            self.frontline_id_map.insert(name_id.to_string(), new_id);
            new_id
        };

        let presence = match team {
            TeamId::Red => &mut self.fl_presence_red,
            TeamId::Blue => &mut self.fl_presence_blue,
        };
        let fl_vec_idx = (line_idx - 1) as usize;
        let rad = spread_radius_px.clamp(0, 12);
        let r2 = rad * rad;
        let bit_mask = if unit_type_mask == 0 { LINE_MASK_ALL } else { unit_type_mask };
        for &(cr, cc) in &cells {
            for dr in -rad..=rad {
                for dc in -rad..=rad {
                    if dr * dr + dc * dc <= r2 {
                        let nr = cr + dr;
                        let nc = cc + dc;
                        if nr >= 0 && (nr as usize) < self.rows && nc >= 0 && (nc as usize) < self.cols {
                            let flat = (nr as usize) * self.cols + (nc as usize);
                            presence[flat] |= bit_mask | 0x80; // high bit = any frontline nearby
                            let list = self.fl_cell_entries.entry(flat as u32).or_default();
                            if !list.contains(&fl_vec_idx) {
                                list.push(fl_vec_idx);
                            }
                        }
                    }
                }
            }
        }
        line_idx
    }

    pub fn sample_ao_and_frontline_field(
        &self,
        pos: [f32; 2],
        team: TeamId,
        unit_type_mask: u8,
        active_deploy_line_id: u32,
        direct_commander_id: Option<usize>,
    ) -> FieldSample {
        let mut out = FieldSample::default();
        let (cr, cc) = self.clamp_rc(pos[0].round() as i32, pos[1].round() as i32);
        let flat = self.idx(cr, cc);
        let (owners, presence) = match team {
            TeamId::Red => (&self.ao_owner_red, &self.fl_presence_red),
            TeamId::Blue => (&self.ao_owner_blue, &self.fl_presence_blue),
        };

        let cur_ao = owners[flat];
        out.in_any_ao = cur_ao != 0;
        out.in_own_ao = cur_ao != 0;

        // If outside own AO, find local AO boundary pull in a small ring
        if !out.in_any_ao {
            let mut best_d2 = f32::INFINITY;
            for dr in -6..=6 {
                for dc in -6..=6 {
                    let nr = cr + dr;
                    let nc = cc + dc;
                    if self.in_bounds(nr, nc) && owners[self.idx(nr, nc)] != 0 {
                        let d2 = (dr * dr + dc * dc) as f32;
                        if d2 < best_d2 && d2 > 0.0 {
                            best_d2 = d2;
                            let inv = 1.0 / d2.sqrt();
                            out.ao_pull = [dr as f32 * inv, dc as f32 * inv];
                        }
                    }
                }
            }
        }

        // Fast check: only inspect cell frontlines OR active lines of this commander/team
        let mut candidates: Vec<usize> = Vec::new();
        if presence[flat] != 0 {
            if let Some(list) = self.fl_cell_entries.get(&(flat as u32)) {
                candidates.extend_from_slice(list);
            }
        }
        // Also scan commander's active lines in case unit is outside spread_radius
        for (idx, fl) in self.frontlines.iter().enumerate() {
            if !fl.active || fl.team != team {
                continue;
            }
            if let Some(dcid) = direct_commander_id {
                if fl.commander_id != dcid && !candidates.contains(&idx) {
                    continue;
                }
            }
            if !candidates.contains(&idx) {
                candidates.push(idx);
            }
        }

        let mut best_score = -1.0f32;
        let mut best_move_score = -1.0f32;

        for idx in candidates {
            let Some(fl) = self.frontlines.get(idx) else {
                continue;
            };
            if !fl.active || fl.team != team || fl.cells.is_empty() {
                continue;
            }
            if let Some(dcid) = direct_commander_id {
                if fl.commander_id != dcid {
                    continue;
                }
            }
            let aff = unit_line_affinity(unit_type_mask, fl.unit_type_mask);
            // Nearest point along frontline cells
            let mut min_d2 = f32::INFINITY;
            let mut near_i = 0usize;
            for (ci, &(lr, lc)) in fl.cells.iter().enumerate() {
                let d2 = (lr as f32 - pos[0]).powi(2) + (lc as f32 - pos[1]).powi(2);
                if d2 < min_d2 {
                    min_d2 = d2;
                    near_i = ci;
                }
            }
            let dist = min_d2.sqrt();
            let near_rc = fl.cells[near_i];
            let end_rc = *fl.cells.last().unwrap();
            let pull_target = match fl.role {
                FrontlineRole::Move | FrontlineRole::Attack => {
                    let end_dist = (end_rc.0 as f32 - pos[0]).hypot(end_rc.1 as f32 - pos[1]);
                    if end_dist > 0.75 {
                        // Guide along polyline ahead of nearest cell or towards endpoint
                        let next_i = (near_i + 2).min(fl.cells.len() - 1);
                        fl.cells[next_i]
                    } else {
                        end_rc
                    }
                }
                FrontlineRole::GlobalAction => {
                    let next_i = (near_i + 2).min(fl.cells.len() - 1);
                    fl.cells[next_i]
                }
                FrontlineRole::Deploy => near_rc,
            };
            let pdr = pull_target.0 as f32 - pos[0];
            let pdc = pull_target.1 as f32 - pos[1];
            let plen = pdr.hypot(pdc);
            let pdir = if plen > 1e-5 { [pdr / plen, pdc / plen] } else { [0.0, 0.0] };

            match fl.role {
                FrontlineRole::GlobalAction => {
                    if plen > 1e-5 {
                        out.global_action_vec = pdir;
                        out.has_global_action = true;
                    }
                }
                FrontlineRole::Move | FrontlineRole::Attack => {
                    // Check source_deploy_line_id provenance!
                    let provenance_ok = fl.source_deploy_line_id == 0
                        || active_deploy_line_id == 0
                        || fl.source_deploy_line_id == active_deploy_line_id;
                    if provenance_ok {
                        let sc = fl.attraction_weight * aff / (1.0 + dist * 0.25);
                        if sc > best_move_score {
                            best_move_score = sc;
                            out.has_move_or_attack = true;
                            out.move_or_attack_vec = pdir;
                            out.move_or_attack_dist_px = (end_rc.0 as f32 - pos[0]).hypot(end_rc.1 as f32 - pos[1]);
                            out.best_line_id = fl.line_id;
                            out.best_line_role = Some(fl.role);
                            out.best_line_source_deploy_id = fl.source_deploy_line_id;
                            out.fl_normal = fl.front_normal;
                        }
                    }
                }
                FrontlineRole::Deploy => {
                    if dist <= (fl.spread_radius_px as f32 + 1.5) {
                        out.has_deploy_line = true;
                        if out.deploy_line_id == 0 {
                            out.deploy_line_id = fl.line_id;
                        }
                    }
                    let sc = fl.attraction_weight * aff / (1.0 + dist * 0.5);
                    if sc > best_score {
                        best_score = sc;
                        out.has_matching_line = true;
                        out.fl_pull = pdir;
                        out.fl_tangent = fl.tangents[near_i];
                        out.fl_normal = fl.front_normal;
                        if !out.has_move_or_attack {
                            out.best_line_id = fl.line_id;
                            out.best_line_role = Some(FrontlineRole::Deploy);
                        }
                    }
                }
            }
        }
        out
    }
}
