use crate::clock::TransactionPhase;
use crate::combat_logistics::{
    apply_registration_to_cells, cast_los_ray_dda, dispatch_logistics_step,
    resolve_cell_strike, resolve_registration_bonus, run_air_subtick_1h,
};
use crate::command_log::{update_war_councils, CommanderDriver, MathRecursiveDriver};
use crate::geometry::WorldPos;
use crate::intent::{IntentCandidate, IntentSource};
use crate::math_ops::{
    apply_g_work_consumption, compute_g_alloc, run_deterministic_multi_capacity_auction, AuctionBid,
    DeygoutRadioOperator, GFieldKind, SchmidtPostureState, ALPHA_BLAST, ALPHA_SUPP,
};
use crate::spatial_autonomy::{step_world_autonomy_5m, ORG_ROUT_THRESHOLD};
use crate::state::{
    GFieldCellState, SimWorld, SubtickReport, TurnReport, SUBTICK_HOURS, SUBTICK_SECONDS,
};
use crate::units_ecs::{
    aggregate_modifiers, equipment_spec, tick_overload_faults_5m, AtomicAction, CommanderId,
    LockFlags, UnitId,
};
use crate::world_grid::FrontlineRole;
use sha2::{Digest, Sha256};

impl SimWorld {
    pub fn state_fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.turn.to_le_bytes());
        h.update(self.subtick.to_le_bytes());
        h.update(self.grid.terrain_revision.to_le_bytes());
        h.update((self.units.id.len() as u64).to_le_bytes());
        for i in 0..self.units.id.len() {
            h.update((self.units.alive[i] as u8).to_le_bytes());
            h.update(self.units.px[i].0.to_le_bytes());
            h.update(self.units.px[i].1.to_le_bytes());
            h.update(((self.units.pos[i][0] * 1000.0).round() as i64).to_le_bytes());
            h.update(((self.units.pos[i][1] * 1000.0).round() as i64).to_le_bytes());
            h.update(self.units.enlisted[i].to_le_bytes());
            h.update(self.units.cadre[i].to_le_bytes());
            h.update(((self.units.org[i] * 100.0).round() as i32).to_le_bytes());
            h.update((self.units.autonomy_state[i] as u8).to_le_bytes());
            h.update(self.units.active_deploy_line_id[i].to_le_bytes());
        }
        for ci in 0..self.commanders.id.len() {
            h.update((self.commanders.alive[ci] as u8).to_le_bytes());
            h.update(self.commanders.px[ci].0.to_le_bytes());
            h.update(self.commanders.px[ci].1.to_le_bytes());
            h.update((self.commanders.rank[ci] as u8).to_le_bytes());
            h.update(((self.commanders.order_churn_score[ci] * 100.0).round() as i32).to_le_bytes());
        }
        let d = h.finalize();
        let mut s = String::with_capacity(64);
        for b in d {
            use std::fmt::Write;
            let _ = write!(&mut s, "{:02x}", b);
        }
        s
    }

    pub fn clock_hms_string(&self) -> String {
        let s = self.clock_seconds % 86400;
        let h = s / 3600;
        let m = (s % 3600) / 60;
        let sec = s % 60;
        format!("{:02}:{:02}:{:02}", h, m, sec)
    }

    #[inline]
    pub fn step_subtick_16s(&mut self, driver: &mut dyn CommanderDriver) -> SubtickReport {
        self.step_subtick_5m(driver)
    }

    pub fn step_subtick_5m(&mut self, driver: &mut dyn CommanderDriver) -> SubtickReport {
        let mut final_report: Option<SubtickReport> = None;
        for phase in TransactionPhase::ALL {
            if let Some(rep) = self.execute_stage(phase, driver) {
                final_report = Some(rep);
            }
        }
        final_report.expect("Stage 12 PublishAndDrop must emit SubtickReport")
    }

    fn execute_stage(
        &mut self,
        phase: TransactionPhase,
        driver: &mut dyn CommanderDriver,
    ) -> Option<SubtickReport> {
        let dt = SUBTICK_HOURS;
        match phase {
            TransactionPhase::BeginTick => {
                self.scratch.begin_tick(self.subtick as u64 + 1);
                self.ensure_aux_states();
                self.units.delta_persons_5m.fill(0);
                self.units.delta_org_5m.fill(0.0);
                self.units.carry_loss.fill(0.0);
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::ClockAndSchedule => {
                self.subtick += 1;
                self.clock_seconds = self.epoch_seconds + ((self.subtick as u64) << 4);
                self.clock_hours = (self.clock_seconds as f64 / 3600.0) as f32;
                for z in &self.zones {
                    if z.active && z.activate_turn == self.turn {
                        self.grid.write_ao_cells(z.zone_id, z.team, z.echelon as u8, &z.cells);
                    }
                }
                let delivered = self.downlink.tick_deliver(self.clock_hours, &mut self.grid, &mut self.commanders);
                let n_cmd = self.commanders.id.len();
                for ci in 0..n_cmd {
                    let cid = CommanderId(ci);
                    if let Some(sub_orders) = driver.evaluate_downlink(
                        cid,
                        self.clock_hours,
                        self.subtick,
                        &self.grid,
                        &self.units,
                        &self.commanders,
                    ) {
                        for (sub_cid, pts, normal, mask) in sub_orders {
                            if self.downlink.record_order_churn(&mut self.commanders, sub_cid, &pts, self.clock_hours) {
                                self.downlink.queue_downlink_to_connected(
                                    cid,
                                    sub_cid,
                                    self.clock_hours,
                                    crate::world_grid::FrontlineRole::Deploy,
                                    mask,
                                    pts,
                                    normal,
                                    &self.commanders,
                                    &self.units,
                                    &self.grid,
                                );
                            }
                        }
                    }
                }
                self.scratch.delivered_orders = delivered;
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::PersistentFieldEvolve => {
                self.ensure_aux_states();
                // Fix 1: 无衰减 Deygout 5 点电台地形剖面重算 (可随掩蔽物消散而完全恢复)
                for ci in 0..self.commanders.id.len() {
                    let cid = CommanderId(ci);
                    let c_pos = self.commander_pos_f32(cid);
                    for uid in 0..self.units.id.len() {
                        if !self.units.alive[uid] || self.units.direct_commander_id[uid] != Some(cid) {
                            continue;
                        }
                        let u_pos = self.units.pos[uid];
                        let dist_tot = self.grid.distance_km(c_pos, u_pos) * 1000.0;
                        let mut t_diff = 1.0f32;
                        if dist_tot > 1.0 {
                            let mut profile = Vec::with_capacity(5);
                            for step in 0..=4 {
                                let frac = (step as f32) * 0.25;
                                let p = [
                                    c_pos[0] + (u_pos[0] - c_pos[0]) * frac,
                                    c_pos[1] + (u_pos[1] - c_pos[1]) * frac,
                                ];
                                profile.push((dist_tot * frac, self.elevation_at_pos(p)));
                            }
                            if let Some(edge_idx) = DeygoutRadioOperator::find_dominant_knife_edge(&profile) {
                                let (s_a, h_a) = profile[0];
                                let (s_b, h_b) = *profile.last().unwrap();
                                let (s_k, z_k) = profile[edge_idx];
                                let chord_h = h_a + (h_b - h_a) * ((s_k - s_a) / (s_b - s_a).max(1e-3));
                                let clearance_diff = (z_k - chord_h).max(0.0);
                                if clearance_diff > 1e-3 {
                                    let nu = clearance_diff / 10.0;
                                    t_diff = DeygoutRadioOperator::rational_diffraction_transmission(nu);
                                }
                            }
                        }
                        if uid < self.derived.unit_radio_t_diff.len() {
                            self.derived.unit_radio_t_diff[uid] = t_diff;
                        }
                        let base_r = self
                            .base_control_radius_km
                            .get(uid)
                            .copied()
                            .unwrap_or(self.units.control_radius_km[uid]);
                        self.units.control_radius_km[uid] = if t_diff < 0.5 {
                            (base_r * (0.5 + 0.5 * t_diff)).max(0.1)
                        } else {
                            base_r
                        };

                        let target_w = if self.units.autonomy_state[uid] == AtomicAction::Attack {
                            1.0f32
                        } else if self.units.autonomy_state[uid] == AtomicAction::Hold {
                            0.5f32
                        } else {
                            0.0f32
                        };
                        let filter = self.unit_zoh_filters.entry(uid as u32).or_default();
                        filter.step_zoh(&[(1, target_w)], 1.0, SUBTICK_SECONDS);
                        let rethink = filter.evaluate_schmidt_rethink(&[(1, target_w)], 1.0);
                        if let Some(rethink_ticks) = rethink {
                            filter.dwell_ticks = filter.dwell_ticks.saturating_sub(rethink_ticks.min(filter.dwell_ticks.saturating_sub(1)));
                        }
                    }
                }

                // Fix 2: G 场占坑 / 做功销账刷新 + compute_g_alloc 在途预扣与多容量确定性拍卖真接线
                self.derived.g_field_cells.retain(|_, v| v.kind == GFieldKind::Work);
                for v in self.derived.g_field_cells.values_mut() {
                    v.committed_en_route = 0.0;
                    v.actual_on_site = 0.0;
                }
                for fl in &self.grid.frontlines {
                    if !fl.active
                        || !matches!(fl.role, FrontlineRole::Deploy | FrontlineRole::Move | FrontlineRole::Attack)
                    {
                        continue;
                    }
                    for &(fr, fc) in &fl.cells {
                        if self.grid.in_bounds(fr, fc) {
                            let cidx = self.grid.idx(fr, fc);
                            let entry = self.derived.g_field_cells.entry(cidx).or_insert(GFieldCellState {
                                g0: 0.0,
                                committed_en_route: 0.0,
                                actual_on_site: 0.0,
                                kind: GFieldKind::State,
                            });
                            if entry.kind == GFieldKind::State {
                                entry.g0 = entry.g0.max(fl.attraction_weight.max(1.0));
                            }
                        }
                    }
                }

                let mut active_g_cells: Vec<usize> = self.derived.g_field_cells.keys().copied().collect();
                active_g_cells.sort_unstable();
                self.derived.g_auction_assignments.clear();

                for uid in 0..self.units.id.len() {
                    if !self.units.alive[uid] || self.units.persons(UnitId(uid)) == 0 {
                        continue;
                    }
                    let has_direct = self.direct_unit_plans.contains_key(&self.units.str_id[uid]);
                    if !has_direct && !active_g_cells.is_empty() && active_g_cells.len() <= 256 {
                        let mut bid_added = false;
                        for &cidx in &active_g_cells {
                            let gcell = &self.derived.g_field_cells[&cidx];
                            let g_alloc = compute_g_alloc(gcell.g0, gcell.actual_on_site, gcell.committed_en_route);
                            if g_alloc <= 0.0 {
                                continue;
                            }
                            let gr = (cidx / self.grid.cols) as i32;
                            let gc = (cidx % self.grid.cols) as i32;
                            let center = [gr as f32 + 0.5, gc as f32 + 0.5];
                            let dist_m = self.grid.distance_km(self.units.pos[uid], center) * 1000.0;
                            self.scratch.auction_bids.push(AuctionBid {
                                cost_fixed: (dist_m * 1000.0) as i64,
                                unit_id: uid as u32,
                                cell_idx: cidx,
                            });
                            let cap = g_alloc.ceil().max(1.0) as usize;
                            self.scratch.auction_capacities.insert(cidx, cap);
                            bid_added = true;
                        }
                        if bid_added {
                            continue;
                        }
                    }
                    let target_px = if let Some(plan) = self.direct_unit_plans.get(&self.units.str_id[uid]) {
                        if let Some(wp) = plan.waypoints.first() {
                            self.pos_to_px(*wp)
                        } else {
                            self.units.px[uid]
                        }
                    } else {
                        self.units.px[uid]
                    };
                    if self.grid.in_bounds(target_px.0, target_px.1) {
                        let cell_idx = self.grid.idx(target_px.0, target_px.1);
                        let center = [target_px.0 as f32 + 0.5, target_px.1 as f32 + 0.5];
                        let dist_m = self.grid.distance_km(self.units.pos[uid], center) * 1000.0;
                        let cost_fixed = (dist_m * 1000.0) as i64;
                        self.scratch.auction_bids.push(AuctionBid {
                            cost_fixed,
                            unit_id: uid as u32,
                            cell_idx,
                        });
                        self.scratch.auction_capacities.entry(cell_idx).or_insert(1);
                    }
                }
                if !self.scratch.auction_bids.is_empty() {
                    let bids = std::mem::take(&mut self.scratch.auction_bids);
                    let allocations = run_deterministic_multi_capacity_auction(bids, &self.scratch.auction_capacities);
                    for (winner_uid, cell_idx) in allocations {
                        let u_idx = winner_uid as usize;
                        if u_idx < self.units.id.len() && self.units.alive[u_idx] {
                            self.derived.g_auction_assignments.insert(u_idx, cell_idx);
                            let ar = (cell_idx / self.grid.cols) as i32;
                            let ac = (cell_idx % self.grid.cols) as i32;
                            if let Some(gcell) = self.derived.g_field_cells.get_mut(&cell_idx) {
                                if self.units.px[u_idx] == (ar, ac) {
                                    gcell.actual_on_site += 1.0;
                                } else {
                                    gcell.committed_en_route += 1.0;
                                }
                            }
                            if !self.direct_unit_plans.contains_key(&self.units.str_id[u_idx]) {
                                if self.grid.in_bounds(ar, ac) && self.units.px[u_idx] == (ar, ac) {
                                    self.units.moved_last_turn[u_idx] = false;
                                }
                            }
                        }
                    }
                }
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::DerivedRefresh => {
                self.derived.synced_terrain_rev = self.grid.terrain_revision;
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::SenseAndArbitrate => {
                // Fix 4: 所有直控与自主需求统一通过 IntentArbitrator 仲裁，杜绝旁路互相踩踏
                for uid in 0..self.units.id.len() {
                    if !self.units.alive[uid] {
                        continue;
                    }
                    let u_pos = self.units.pos[uid];
                    let facing = self.units.facing_deg[uid];
                    self.scratch.arbitrator.push_candidate(IntentCandidate {
                        entity_id: uid as u32,
                        source: IntentSource::AutonomousAi,
                        desired_speed_mps: 1.0,
                        target_pos: None,
                        heading_deg: facing,
                        timestamp_sec: self.clock_seconds as f64,
                    });
                    if let Some(plan) = self.direct_unit_plans.get(&self.units.str_id[uid]) {
                        let tgt = plan.target_pos.map(|p| WorldPos::new(p[0], p[1]));
                        self.scratch.arbitrator.push_candidate(IntentCandidate {
                            entity_id: uid as u32,
                            source: IntentSource::DirectOrder,
                            desired_speed_mps: 3.5,
                            target_pos: tgt.or(Some(WorldPos::new(u_pos[0], u_pos[1]))),
                            heading_deg: facing,
                            timestamp_sec: self.clock_seconds as f64,
                        });
                    }
                }
                let winners = self.scratch.arbitrator.arbitrate();
                for w in winners {
                    if w.winning_source == IntentSource::DirectOrder {
                        let u = w.entity_id as usize;
                        self.scratch.direct_controlled_uids.insert(u);
                        self.scratch.saved_direct_positions.insert(
                            u,
                            (self.units.pos[u], self.units.px[u], self.units.facing_deg[u]),
                        );
                    }
                }

                let auto_stats = step_world_autonomy_5m(
                    &mut self.grid,
                    &mut self.units,
                    &self.commanders,
                    &mut self.derived.buckets,
                    &self.events.events,
                    &self.zones,
                    dt,
                    self.night,
                );
                self.scratch.auto_moved_count = auto_stats.moved_count;
                self.scratch.auto_attack_count = auto_stats.attack_count;
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::ProposeMotion => {
                for (&u, &(pos, px, facing)) in &self.scratch.saved_direct_positions {
                    self.units.pos[u] = pos;
                    self.units.px[u] = px;
                    self.units.facing_deg[u] = facing;
                }
                for ci in 0..self.commanders.id.len() {
                    if let Some(hq_u) = self.commanders.hq_piece_id[ci] {
                        let cpos = self.commander_pos_f32(CommanderId(ci));
                        self.units.pos[hq_u.0] = cpos;
                        self.units.px[hq_u.0] = self.commanders.px[ci];
                    }
                }
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::ConstrainMotion => {
                let direct_moved = self.step_direct_orders_5m();
                self.scratch.direct_moved_count = direct_moved;
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::CommitMotion => {
                self.ensure_aux_states();
                let (grid_rows_i, grid_cols_i) = (self.grid.rows as i32, self.grid.cols as i32);
                let pos_to_px_local = |pos: [f32; 2]| {
                    ((pos[0].round() as i32).clamp(0, grid_rows_i - 1), (pos[1].round() as i32).clamp(0, grid_cols_i - 1))
                };

                let engineer_uids: Vec<u32> = self.engineer_integrators.keys().copied().collect();
                for uid in engineer_uids {
                    let u_idx = uid as usize;
                    if u_idx >= self.units.alive.len() || !self.units.alive[u_idx] {
                        continue;
                    }
                    let supp = self.unit_suppression.get(u_idx).copied().unwrap_or(0.0);
                    let persons = self.units.persons(UnitId(u_idx));
                    let (cur_pos, completed_wps) = if let Some(integrator) = self.engineer_integrators.get_mut(&uid) {
                        integrator.step_16s(3.5, persons, supp, 40.0);
                        let completed: Vec<[f32; 2]> = integrator.completed_instances.drain(..)
                            .filter_map(|idx| integrator.waypoints.get(idx).copied())
                            .collect();
                        (integrator.cur_pos, completed)
                    } else {
                        continue;
                    };

                    self.units.pos[u_idx] = cur_pos;
                    let new_px = pos_to_px_local(cur_pos);
                    if self.grid.in_bounds(new_px.0, new_px.1) {
                        self.units.px[u_idx] = new_px;
                    }
                    for pt in completed_wps {
                        let px = pos_to_px_local(pt);
                        if self.grid.in_bounds(px.0, px.1) {
                            let gi = self.grid.idx(px.0, px.1);
                            self.grid.terrain_attrition_day[gi] = (self.grid.terrain_attrition_day[gi] * 0.5).max(0.0);
                            if let Some(gcell) = self.derived.g_field_cells.get_mut(&gi) {
                                if gcell.kind == GFieldKind::Work {
                                    apply_g_work_consumption(&mut gcell.g0, 1.0);
                                }
                            }
                        }
                    }
                }
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::EmitEvents => {
                for detonator in &mut self.detonator_switches {
                    if detonator.wire_laid && detonator.charges_placed && !detonator.detonated {
                        let op_pos = detonator.switch_pos;
                        if let Ok(pts) = detonator.trigger_detonation(op_pos) {
                            self.scratch.exploded_points.extend(pts);
                        }
                    }
                }
                let exploded = std::mem::take(&mut self.scratch.exploded_points);
                for pt in exploded {
                    let px = self.pos_to_px(pt);
                    if self.grid.in_bounds(px.0, px.1) {
                        let gi = self.grid.idx(px.0, px.1);
                        self.cell_h_blast[gi] += 50.0;
                        self.cell_s_supp[gi] += 10.0;
                    }
                }

                let n_units = self.units.id.len();
                for uid in 0..n_units {
                    if !self.units.alive[uid]
                        || self.units.autonomy_state[uid] != AtomicAction::Attack
                        || self.units.persons(UnitId(uid)) == 0
                        || self.units.ammo_t[uid] <= 0.001
                    {
                        continue;
                    }
                    let pos = self.units.pos[uid];
                    let px = self.units.px[uid];
                    let team = self.units.team[uid];
                    let scan_px = (3000.0 / self.grid.cell_m.max(1.0)).ceil().min(512.0) as i32;
                    let mut best_target: Option<(UnitId, f32)> = None;
                    self.derived.buckets.query_neighbors_within_px(px.0, px.1, scan_px, |eid| {
                        let j = eid.0;
                        if !self.units.alive[j] || self.units.team[j] == team || self.units.persons(eid) == 0 {
                            return;
                        }
                        let d_m = self.grid.distance_km(pos, self.units.pos[j]) * 1000.0;
                        if d_m <= 3000.0 && best_target.map_or(true, |(_, bd)| d_m < bd) {
                            best_target = Some((eid, d_m));
                        }
                    });
                    if let Some((tid, d_m)) = best_target {
                        let primary_w = self.units.equipment[uid]
                            .iter()
                            .max_by(|a, b| equipment_spec(a.0).fire[0].total_cmp(&equipment_spec(b.0).fire[0]))
                            .map(|(m, _)| m.clone())
                            .unwrap_or_else(|| "rifle".to_string());
                        self.scratch.combat_pairs.push((UnitId(uid), tid, primary_w, d_m));
                    }
                }

                let mut keep_events = Vec::with_capacity(self.events.events.len());
                for mut ev in self.events.events.drain(..) {
                    ev.remaining_hours -= dt;
                    if ev.remaining_hours <= 1e-6 {
                        self.scratch.due_events.push(ev);
                    } else {
                        keep_events.push(ev);
                    }
                }
                self.events.events = keep_events;
                self.scratch.completed_events = self.scratch.due_events.len();
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::ResolveEvents => {
                let pairs = std::mem::take(&mut self.scratch.combat_pairs);
                for (aid, tid, w_model, dist_m) in pairs {
                    if !self.units.alive[aid.0] || !self.units.alive[tid.0] {
                        continue;
                    }
                    let a_px = self.units.px[aid.0];
                    let t_px = self.units.px[tid.0];
                    let a_pos = self.units.pos[aid.0];
                    let los = cast_los_ray_dda(&self.grid, a_px, t_px, self.night);
                    let spec = equipment_spec(&w_model);
                    let reg_b = resolve_registration_bonus(
                        &self.grid,
                        aid,
                        &self.units,
                        &self.commanders,
                        t_px,
                        spec.ballistic_group,
                    );
                    let attacker_posture = self.unit_postures.get(aid.0).copied().unwrap_or(SchmidtPostureState::Active);
                    let zeta = attacker_posture.source_factor();
                    let pwr = self.units.unit_power(aid, dist_m, !los.blocked, None, (1.0 + reg_b) * zeta);
                    let mut fire_dt = [0.0f32; 6];
                    let mut total_pts = 0.0f32;
                    for d in 0..6 {
                        fire_dt[d] = pwr[d] * dt;
                        total_pts += fire_dt[d];
                    }
                    if total_pts > 1e-4 {
                        self.units.ammo_t[aid.0] = (self.units.ammo_t[aid.0] - total_pts * 0.01).max(0.0);
                        let tag = format!("subtick{}|u{}->u{}", self.subtick, aid.0, tid.0);
                        let a_team = self.units.team[aid.0];
                        resolve_cell_strike(
                            &mut self.grid,
                            &mut self.units,
                            &self.commanders,
                            &mut self.events,
                            t_px,
                            a_team,
                            fire_dt,
                            &w_model,
                            a_pos,
                            !spec.indirect,
                            &tag,
                        );
                        self.record_blast_and_suppression(t_px, total_pts, Some(tid));
                        apply_registration_to_cells(
                            &mut self.grid,
                            aid,
                            &self.units,
                            &self.commanders,
                            t_px,
                            1.0,
                            &w_model,
                            dist_m,
                            5.0,
                            false,
                        );
                    }
                }

                let due = std::mem::take(&mut self.scratch.due_events);
                for ev in due {
                    self.complete_event(ev);
                }
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::CommitPersistent => {
                tick_overload_faults_5m(&mut self.units, dt, self.subtick);
                update_war_councils(&mut self.commanders, &mut self.units, &mut self.grid, dt);
                self.scratch.air_sorties = if self.subtick % 12 == 0 {
                    run_air_subtick_1h(&mut self.wings, &self.grid)
                } else {
                    0
                };
                let logi_rep = dispatch_logistics_step(
                    &self.grid,
                    &mut self.depots,
                    &mut self.units,
                    &self.commanders,
                    &self.zones,
                    &self.fire_fields,
                    dt,
                    self.turn,
                );
                self.scratch.logi_conservation_error = logi_rep.conservation_error;

                for uid in 0..self.units.id.len() {
                    let u = UnitId(uid);
                    if !self.units.alive[uid] || self.units.persons(u) == 0 {
                        continue;
                    }
                    let (r, c) = self.units.px[uid];
                    if self.grid.in_bounds(r, c) {
                        let gi = self.grid.idx(r, c);
                        let rate = self.grid.terrain_attrition_day[gi] * dt / 24.0;
                        if rate > 0.0 && self.units.enlisted[uid] > 0 {
                            let cur = self.units.carry_attr[uid] + rate * self.units.enlisted[uid] as f32;
                            let lost = (cur.floor() as u32).min(self.units.enlisted[uid]);
                            self.units.carry_attr[uid] = cur - lost as f32;
                            self.units.enlisted[uid] -= lost;
                            self.units.delta_persons_5m[uid] -= lost as i32;
                        }
                    }
                    let locks = self.units.effective_locks(u, &self.events.events);
                    let mods = aggregate_modifiers(&self.units.modifier_slots[uid], 0.0, false);
                    if !locks.contains(LockFlags::FORBID_REST)
                        && self.units.autonomy_state[uid] == AtomicAction::Hold
                        && self.units.delta_org_5m[uid] >= -1e-4
                    {
                        let rec = 4.0 * dt * self.units.recovery_mult[uid];
                        if self.units.org[uid] < self.units.org_cap[uid] {
                            self.units.org[uid] = (self.units.org[uid] + rec).min(self.units.org_cap[uid]);
                        }
                    }
                    if self.units.org[uid] > self.units.org_cap[uid] {
                        let excess = self.units.org[uid] - self.units.org_cap[uid];
                        self.units.org[uid] = self.units.org_cap[uid] + excess * (-dt / 6.0).exp();
                    }
                    if mods.org_delta_per_h.abs() > 1e-6 {
                        self.units.org[uid] = (self.units.org[uid] + mods.org_delta_per_h * dt).clamp(0.0, 120.0);
                    }
                    self.units.modifier_slots[uid].retain_mut(|slot| {
                        if let Some(ttl) = &mut slot.ttl_hours {
                            *ttl -= dt;
                            *ttl > 1e-6
                        } else {
                            true
                        }
                    });
                    if self.units.persons(u) == 0 {
                        self.units.alive[uid] = false;
                    } else {
                        self.units.retreat_recovery_hours[uid] = (self.units.retreat_recovery_hours[uid] - dt).max(0.0);
                        if self.units.org[uid] <= ORG_ROUT_THRESHOLD && self.units.retreat_recovery_hours[uid] <= 0.0 {
                            let _ = self.try_set_direct_unit_order_ex(
                                u,
                                "retreat",
                                None,
                                false,
                                None,
                                "fast",
                                "fire_on_the_move",
                                false,
                            );
                        }
                    }
                }

                for ci in 0..self.commanders.id.len() {
                    self.commanders.resolve_succession(CommanderId(ci), &self.units);
                    self.commanders.sync_hq_ledger_authority(CommanderId(ci), &self.units);
                }

                self.ensure_aux_states();
                for h in &mut self.cell_h_blast {
                    *h *= ALPHA_BLAST;
                    if *h < 1e-4 {
                        *h = 0.0;
                    }
                }
                for s in &mut self.cell_s_supp {
                    *s *= ALPHA_SUPP;
                    if *s < 1e-4 {
                        *s = 0.0;
                    }
                }
                for uid in 0..self.units.id.len() {
                    if uid < self.unit_suppression.len() {
                        self.unit_suppression[uid] *= ALPHA_SUPP;
                        self.unit_postures[uid] = self.unit_postures[uid].transition(self.unit_suppression[uid]);
                    }
                }

                self.grid.tick_environment(dt);
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::Validate => {
                debug_assert_eq!(self.clock_seconds, self.epoch_seconds + ((self.subtick as u64) << 4));
                debug_assert_eq!(self.scratch.executed_phases.len(), 11);
                self.scratch.executed_phases.push(phase);
                None
            }
            TransactionPhase::PublishAndDrop => {
                let fp = self.state_fingerprint();
                self.scratch.executed_phases.push(phase);
                self.last_tick_phases = self.scratch.executed_phases.len();
                let rep = SubtickReport {
                    subtick: self.subtick,
                    clock_hours: self.clock_hours,
                    moved_units: self.scratch.auto_moved_count + self.scratch.direct_moved_count,
                    attacking_units: self.scratch.auto_attack_count,
                    delivered_orders: self.scratch.delivered_orders,
                    completed_events: self.scratch.completed_events,
                    air_sorties: self.scratch.air_sorties,
                    logi_conservation_error: self.scratch.logi_conservation_error,
                    fingerprint: fp,
                };
                self.scratch.clear();
                debug_assert!(self.scratch.is_clean());
                Some(rep)
            }
        }
    }

    pub fn run_turn(&mut self, hours: f32, driver: &mut dyn CommanderDriver) -> TurnReport {
        self.pre_game_editable = false;
        self.turn += 1;
        let n_ticks = (hours * (3600.0 / SUBTICK_SECONDS)).round().max(1.0) as u32;
        let mut last_fp = String::new();
        for _ in 0..n_ticks {
            let sr = self.step_subtick_5m(driver);
            last_fp = sr.fingerprint;
        }
        TurnReport {
            turn: self.turn,
            clock_hours: self.clock_hours,
            subtick_count: n_ticks,
            total_hours: n_ticks as f32 * SUBTICK_HOURS,
            last_fingerprint: last_fp,
        }
    }

    pub fn run_turn_default(&mut self, hours: f32) -> TurnReport {
        let mut drv = MathRecursiveDriver;
        self.run_turn(hours, &mut drv)
    }
}
