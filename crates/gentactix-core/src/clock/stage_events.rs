use crate::combat_logistics::{
    apply_registration_to_cells, cast_los_ray_dda, resolve_cell_strike, resolve_registration_bonus,
};
use crate::state::world_state::{DirectCombatLogEntry, SimWorld, SUBTICK_HOURS};
use crate::units_ecs::{
    classify_unit_mask, complete_reorganize_echelon, equipment_spec, ActiveEvent, CommanderId,
    Echelon, EventKind, Rank, UnitId, UnitState,
};
use crate::world_grid::{CellModEntry, FeatureFlags, TeamId};
use rustc_hash::FxHashMap;

impl SimWorld {
    pub fn execute_deploy_heavy(
        &mut self,
        u: UnitId,
        weapon: &str,
        keep_manned: bool,
        operator_soldier_ids: &[String],
        operator_count: u32,
        tag_id: u64,
    ) -> Result<UnitId, String> {
        if u.0 >= self.units.alive.len() || !self.units.alive[u.0] {
            return Err("actor_dead".into());
        }
        let has_w = self.units.equipment[u.0].get(weapon).copied().unwrap_or(0) > 0;
        if !has_w {
            return Err(format!("weapon_not_owned:{weapon}"));
        }
        if let Some(c) = self.units.equipment[u.0].get_mut(weapon) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                self.units.equipment[u.0].remove(weapon);
            }
        }
        if let Some(cf) = self.units.equipment_full[u.0].get_mut(weapon) {
            *cf = cf.saturating_sub(1);
        }
        let mut eq = FxHashMap::default();
        eq.insert(weapon.to_string(), 1);
        let emp_str = format!("{}-{}-{}", self.units.str_id[u.0], weapon, tag_id);
        let emp_name = format!("{}阵地", equipment_spec(weapon).name);
        let px = self.units.px[u.0];
        let pos = [self.units.pos[u.0][0] + 0.08, self.units.pos[u.0][1] + 0.08];
        let team = self.units.team[u.0];
        let ch = self.units.radio_channel[u.0];
        let cmdr_opt = self.units.direct_commander_id[u.0];

        let emp_id = if keep_manned && self.units.persons(u) > 1 {
            let max_take = self.units.persons(u).saturating_sub(1);
            let target_crew = if !operator_soldier_ids.is_empty() {
                (operator_soldier_ids.len() as u32).min(max_take)
            } else {
                operator_count.max(equipment_spec(weapon).crew).min(max_take)
            };
            let mut moved_sols = Vec::new();
            if !self.units.soldiers[u.0].is_empty() {
                let mut rem = target_crew;
                let mut kept = Vec::with_capacity(self.units.soldiers[u.0].len());
                for s in self.units.soldiers[u.0].drain(..) {
                    let wanted = if !operator_soldier_ids.is_empty() {
                        operator_soldier_ids.iter().any(|id| id == &s.id)
                    } else {
                        true
                    };
                    if rem > 0 && s.status != "dead" && !s.cannot_fire && wanted {
                        rem -= 1;
                        moved_sols.push(s);
                    } else {
                        kept.push(s);
                    }
                }
                if rem > 0 {
                    // Top up from remaining healthy soldiers if requested count > matched ids
                    let mut final_kept = Vec::with_capacity(kept.len());
                    for s in kept {
                        if rem > 0 && s.status != "dead" && !s.cannot_fire && final_kept.len() + rem as usize > 0 {
                            rem -= 1;
                            moved_sols.push(s);
                        } else {
                            final_kept.push(s);
                        }
                    }
                    kept = final_kept;
                }
                self.units.soldiers[u.0] = kept;
                self.units.sync_totals_from_soldiers(u);
            }
            let tc_nco = moved_sols.iter().filter(|s| s.is_cadre && s.role != "officer").count() as u32;
            let tc_off = moved_sols.iter().filter(|s| s.role == "officer").count() as u32;
            let te = moved_sols.len() as u32 - tc_nco - tc_off;
            let id = self.units.spawn_unit_4ledger(
                emp_str, emp_name, team, px, Echelon::Squad, "heavy_emplacement",
                te.max(if moved_sols.is_empty() { target_crew } else { 0 }),
                tc_nco, tc_off, eq, ch,
            );
            if !moved_sols.is_empty() {
                self.units.soldiers[id.0] = moved_sols;
                self.units.sync_totals_from_soldiers(id);
            }
            self.units.operated_by[id.0] = Some(u);
            id
        } else {
            // Detached unmanned heavy emplacement operated by mother squad
            let id = self.units.spawn_unit_4ledger(
                emp_str, emp_name, team, px, Echelon::Squad, "heavy_emplacement",
                0, 0, 0, eq, ch,
            );
            self.units.soldiers[id.0].clear();
            let ammo_share = (self.units.ammo_t[u.0] * 0.18).min(0.25).max(0.02).min(self.units.ammo_t[u.0]);
            self.units.ammo_t[id.0] = ammo_share;
            self.units.operated_by[id.0] = Some(u);
            self.units.assigned_operator_soldiers[id.0] = operator_soldier_ids.to_vec();
            self.units.assigned_operator_count[id.0] = operator_count
                .max(operator_soldier_ids.len() as u32)
                .max(equipment_spec(weapon).crew)
                .max(1);
            id
        };

        self.units.pos[emp_id.0] = pos;
        self.units.unit_state[emp_id.0] = UnitState::Deployed;
        self.units.deployed_weapon[emp_id.0] = Some(weapon.to_string());
        self.units.direct_commander_id[emp_id.0] = cmdr_opt;
        if let Some(cid) = cmdr_opt {
            if cid.0 < self.commanders.subordinate_unit_ids.len()
                && !self.commanders.subordinate_unit_ids[cid.0].contains(&emp_id)
            {
                self.commanders.subordinate_unit_ids[cid.0].push(emp_id);
            }
        }
        self.units.refresh_caps(u);
        self.units.refresh_caps(emp_id);
        Ok(emp_id)
    }

    pub fn execute_pack_heavy(&mut self, receiver: UnitId, target_uid: UnitId, attach: bool) -> Result<(), String> {
        let ti = target_uid.0;
        if ti >= self.units.alive.len() || !self.units.alive[ti] {
            return Err("target_emplacement_dead".into());
        }
        if receiver.0 >= self.units.alive.len() || !self.units.alive[receiver.0] {
            return Err("receiver_dead".into());
        }
        if self.units.team[receiver.0] != self.units.team[ti] {
            return Err("cross_team_pack_forbidden".into());
        }
        if attach || receiver == target_uid {
            self.units.operated_by[ti] = None;
            if attach {
                self.units.unit_state[ti] = UnitState::Carried;
                self.units.attached_to[ti] = Some(receiver);
            } else {
                self.units.unit_state[ti] = UnitState::Mobile;
            }
            self.units.refresh_caps(target_uid);
            return Ok(());
        }

        // Merge heavy weapon, stock, soldiers & weighted org back into receiver
        let ri = receiver.0;
        let p_dst = self.units.persons(receiver) as f32;
        let p_src = self.units.persons(target_uid) as f32;
        let w_org = if (p_dst + p_src) > 0.0 {
            (self.units.org[ri] * p_dst + self.units.org[ti] * p_src) / (p_dst + p_src)
        } else {
            self.units.org[ri]
        };

        let eq_src = std::mem::take(&mut self.units.equipment[ti]);
        for (m, cnt) in eq_src {
            if cnt > 0 {
                *self.units.equipment[ri].entry(m.clone()).or_insert(0) += cnt;
                *self.units.equipment_full[ri].entry(m).or_insert(0) += cnt;
            }
        }
        if let Some(dw) = self.units.deployed_weapon[ti].take() {
            if self.units.equipment[ri].get(&dw).copied().unwrap_or(0) == 0 {
                *self.units.equipment[ri].entry(dw.clone()).or_insert(0) += 1;
                *self.units.equipment_full[ri].entry(dw).or_insert(0) += 1;
            }
        }

        let df = self.units.food_t[ti];
        let da = self.units.ammo_t[ti];
        let dg = self.units.fuel_t[ti];
        self.units.food_t[ti] = 0.0;
        self.units.ammo_t[ti] = 0.0;
        self.units.fuel_t[ti] = 0.0;
        self.units.add_supply_reserving_soldiers(receiver, df, da, dg);

        if !self.units.soldiers[ti].is_empty() {
            let mut sols = std::mem::take(&mut self.units.soldiers[ti]);
            self.units.soldiers[ri].append(&mut sols);
            self.units.sync_totals_from_soldiers(receiver);
        } else {
            self.units.enlisted[ri] += self.units.enlisted[ti];
            self.units.nco[ri] += self.units.nco[ti];
            self.units.officer[ri] += self.units.officer[ti];
            self.units.cadre[ri] = self.units.nco[ri] + self.units.officer[ri];
        }

        self.units.enlisted[ti] = 0;
        self.units.nco[ti] = 0;
        self.units.officer[ti] = 0;
        self.units.cadre[ti] = 0;
        self.units.operated_by[ti] = None;
        self.units.alive[ti] = false;
        self.units.unit_state[ti] = UnitState::Mobile;
        for subs in &mut self.commanders.subordinate_unit_ids {
            subs.retain(|&id| id != target_uid);
        }
        self.units.refresh_caps(receiver);
        self.units.org[ri] = w_org.min(self.units.org_cap[ri]);
        Ok(())
    }

    pub(crate) fn complete_event(&mut self, ev: ActiveEvent) {
        match ev.kind {
            EventKind::ReorganizeEchelon {
                commander_id,
                commander_piece_id,
                target_echelon,
                source_unit_ids,
                target_specs,
                strict_cadre_allocation,
            } => {
                complete_reorganize_echelon(
                    &self.grid,
                    &mut self.units,
                    &mut self.commanders,
                    commander_id,
                    commander_piece_id,
                    target_echelon,
                    &source_unit_ids,
                    &target_specs,
                    strict_cadre_allocation,
                );
            }
            EventKind::DeployHeavyUnit {
                weapon,
                keep_manned,
                operator_soldier_ids,
                operator_count,
            } => {
                let _ = self.execute_deploy_heavy(
                    ev.actor_id,
                    &weapon,
                    keep_manned,
                    &operator_soldier_ids,
                    operator_count,
                    ev.id,
                );
            }
            EventKind::PackHeavyUnit { target_unit_id, attach } => {
                let _ = self.execute_pack_heavy(ev.actor_id, target_unit_id, attach);
            }
            EventKind::Demolition { target_px, target_type } => {
                self.grid.add_exposure(target_px.0, target_px.1, 12.0, 2.0);
                self.grid.cut_wire_at(target_px.0, target_px.1, 1.0, None);
                if target_type == "bridge" {
                    self.grid.destroy_bridge(target_px.0, target_px.1);
                } else if target_type == "terrain" {
                    self.grid.destroy_terrain(target_px.0, target_px.1);
                } else if target_type == "stash" {
                    let k = self.grid.idx(target_px.0, target_px.1) as u32;
                    self.grid.cell_stash.remove(&k);
                }
            }
            EventKind::Engineering { op, target_px } => {
                let i_px = self.grid.idx(
                    target_px.0.clamp(0, (self.grid.rows as i32) - 1),
                    target_px.1.clamp(0, (self.grid.cols as i32) - 1),
                );
                if op == "build_bridge" {
                    self.grid.repair_bridge(target_px.0, target_px.1);
                } else if op == "repair_terrain" || op == "repair_airfield" {
                    self.grid.repair_terrain(target_px.0, target_px.1);
                } else if op == "lay_mines" {
                    let mut entry = CellModEntry::neutral("mine:field");
                    entry.mine_density = 0.70;
                    entry.speed_mult = 0.45;
                    self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                    let mf = if self.units.team[ev.actor_id.0] == TeamId::Red {
                        FeatureFlags::MINEFIELD_RED
                    } else {
                        FeatureFlags::MINEFIELD_BLUE
                    };
                    self.grid.feature_flags[i_px] |= mf.bits();
                    self.grid.terrain_revision += 1;
                } else if op == "clear_mines" {
                    self.grid.remove_cell_modifier(target_px.0, target_px.1, "mine:field");
                    self.grid.feature_flags[i_px] &= !(FeatureFlags::MINEFIELD_RED | FeatureFlags::MINEFIELD_BLUE).bits();
                    self.grid.terrain_revision += 1;
                } else if op == "fortify" {
                    let mut entry = CellModEntry::neutral("fort:foxhole");
                    entry.fortification_bonus = 0.22;
                    entry.cover_bonus = 0.14;
                    self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                } else {
                    let mut entry = CellModEntry::neutral("fort:trench");
                    entry.fortification_bonus = 0.35;
                    entry.cover_bonus = 0.20;
                    self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                    self.grid.feature_flags[i_px] |= FeatureFlags::TRENCH.bits();
                    self.grid.terrain_revision += 1;
                }
            }
            EventKind::ResupplyPickup { food_t, ammo_t, fuel_t } => {
                let px = self.units.px[ev.actor_id.0];
                let rows = self.grid.rows as i32;
                let cols = self.grid.cols as i32;
                let mut picked_k = None;
                'outer: for dr in -1..=1 {
                    for dc in -1..=1 {
                        let rr = px.0 + dr;
                        let cc = px.1 + dc;
                        if rr >= 0 && rr < rows && cc >= 0 && cc < cols {
                            let k = self.grid.idx(rr, cc) as u32;
                            if self.grid.cell_stash.contains_key(&k) {
                                picked_k = Some(k);
                                break 'outer;
                            }
                        }
                    }
                }
                if let Some(k) = picked_k {
                    let mut remove_empty = false;
                    if let Some(st) = self.grid.cell_stash.get_mut(&k) {
                        let f_req = if food_t > 0.0 { food_t } else { st.food_t };
                        let a_req = if ammo_t > 0.0 { ammo_t } else { st.ammo_t };
                        let g_req = if fuel_t > 0.0 { fuel_t } else { st.fuel_t };
                        let df = f_req.min(st.food_t).min((self.units.cap_food_t[ev.actor_id.0] - self.units.food_t[ev.actor_id.0]).max(0.0));
                        let da = a_req.min(st.ammo_t).min((self.units.cap_ammo_t[ev.actor_id.0] - self.units.ammo_t[ev.actor_id.0]).max(0.0));
                        let dg = g_req.min(st.fuel_t).min((self.units.cap_fuel_t[ev.actor_id.0] - self.units.fuel_t[ev.actor_id.0]).max(0.0));
                        st.food_t -= df;
                        st.ammo_t -= da;
                        st.fuel_t -= dg;
                        self.units.add_supply_reserving_soldiers(ev.actor_id, df, da, dg);
                        if st.food_t <= 1e-4 && st.ammo_t <= 1e-4 && st.fuel_t <= 1e-4 && st.equipment.is_empty() {
                            remove_empty = true;
                        }
                    }
                    if remove_empty {
                        self.grid.cell_stash.remove(&k);
                    }
                }
            }
            EventKind::FormCommandSquad {
                preferred_name,
                dissolve_if_empty,
            } => {
                let u = ev.actor_id;
                let has_cadre = self.units.alive[u.0] && (self.units.nco[u.0] + self.units.officer[u.0]) > 0;
                if has_cadre || !dissolve_if_empty {
                    self.units.kind[u.0] = "hq_squad".to_string();
                    self.units.unit_mask[u.0] = classify_unit_mask(&self.units.kind[u.0], &self.units.equipment[u.0]);
                    if !preferred_name.trim().is_empty() {
                        self.units.name[u.0] = preferred_name;
                    }
                    let existing_cid = self
                        .commanders
                        .hq_piece_id
                        .iter()
                        .position(|&hp| hp == Some(u))
                        .map(CommanderId);
                    let has_officer = self.units.officer[u.0] > 0;
                    let target_rank = if has_officer { Rank::Lieutenant } else { Rank::Sergeant };
                    let cid = match existing_cid {
                        Some(ec) => {
                            self.commanders.alive[ec.0] = true;
                            self.commanders.rank[ec.0] = target_rank;
                            self.commanders.px[ec.0] = self.units.px[u.0];
                            ec
                        }
                        None => {
                            let new_sid = format!("cmd-hq-{}", self.units.str_id[u.0]);
                            let cname = format!("{} · 指挥部", self.units.name[u.0]);
                            let nc = self.commanders.spawn_commander(
                                new_sid,
                                cname,
                                self.units.team[u.0],
                                self.units.px[u.0],
                                Echelon::Platoon,
                                target_rank,
                                self.units.radio_channel[u.0],
                            );
                            self.commanders.hq_piece_id[nc.0] = Some(u);
                            if let Some(parent_cid) = self.units.direct_commander_id[u.0] {
                                self.commanders.link_subordinate_cmd(parent_cid, nc);
                            } else {
                                self.units.direct_commander_id[u.0] = Some(nc);
                            }
                            nc
                        }
                    };
                    self.units.refresh_caps(u);
                    self.commanders.sync_hq_ledger_authority(cid, &self.units);
                    self.push_combat_log(DirectCombatLogEntry {
                        turn: self.turn,
                        clock_hours: self.clock_hours,
                        actor_id: self.units.str_id[u.0].clone(),
                        actor_name: self.units.name[u.0].clone(),
                        target_id: None,
                        target_name: None,
                        kind: "order".into(),
                        weapon_model: "radio".into(),
                        dist_m: 0.0,
                        dispersion_m: 0.0,
                        hit_prob: 0.0,
                        casualties: 0,
                        org_loss: 0.0,
                        carry_loss_after: 0.0,
                        ammo_used_t: 0.0,
                        message: format!(
                            "【组建指挥班】完成（{}），正式赋予指挥班身份并解锁「任务」指挥权限。",
                            if has_officer { "军官正职" } else { "士官临时代理·排级上限" }
                        ),
                    });
                }
            }
            EventKind::LayWire { target_px } => {
                let a_px = self.units.px[ev.actor_id.0];
                let team = self.units.team[ev.actor_id.0];
                self.grid.lay_wire_line(a_px, target_px, team);
            }
            EventKind::CargoTransfer {
                target_unit_id,
                food_t,
                ammo_t,
                fuel_t,
                equipment,
            } => {
                let s_idx = ev.actor_id.0;
                let t_idx = target_unit_id.0;
                if s_idx < self.units.id.len() && t_idx < self.units.id.len() && self.units.alive[s_idx] && self.units.alive[t_idx] {
                    let df = food_t.max(0.0).min(self.units.food_t[s_idx]);
                    let da = ammo_t.max(0.0).min(self.units.ammo_t[s_idx]);
                    let dg = fuel_t.max(0.0).min(self.units.fuel_t[s_idx]);
                    self.units.food_t[s_idx] -= df;
                    self.units.food_t[t_idx] = (self.units.food_t[t_idx] + df).min(self.units.cap_food_t[t_idx].max(self.units.food_t[t_idx] + df));
                    self.units.ammo_t[s_idx] -= da;
                    self.units.ammo_t[t_idx] += da;
                    self.units.fuel_t[s_idx] -= dg;
                    self.units.fuel_t[t_idx] += dg;
                    for (m, cnt) in equipment {
                        let avail = self.units.equipment[s_idx].get(&m).copied().unwrap_or(0);
                        let mv = cnt.min(avail);
                        if mv > 0 {
                            if let Some(c) = self.units.equipment[s_idx].get_mut(&m) {
                                *c -= mv;
                            }
                            *self.units.equipment[t_idx].entry(m).or_insert(0) += mv;
                        }
                    }
                    self.units.refresh_caps(ev.actor_id);
                    self.units.refresh_caps(target_unit_id);
                }
            }
            EventKind::RadioDecoy { target_px, ttl_h } => {
                let mut entry = CellModEntry::neutral("decoy:radio");
                entry.decoy_signal = 1.0;
                entry.ttl_hours = Some(ttl_h.max(0.5));
                self.grid.set_cell_modifier(target_px.0, target_px.1, entry);
                self.grid.add_exposure(target_px.0, target_px.1, 8.0, 2.0);
            }
            EventKind::FireWeapon { weapon, target_px, radius_px } => {
                let aid = ev.actor_id;
                if self.units.alive[aid.0] {
                    let d_m = self.grid.distance_km(self.units.pos[aid.0], [target_px.0 as f32, target_px.1 as f32]) * 1000.0;
                    let los = cast_los_ray_dda(&self.grid, self.units.px[aid.0], target_px, self.night);
                    let spec = equipment_spec(&weapon);
                    let reg_b = resolve_registration_bonus(&self.grid, aid, &self.units, &self.commanders, target_px, spec.ballistic_group);
                    let pwr = self.units.unit_power(aid, d_m, !los.blocked, Some(&weapon), 1.0 + reg_b);
                    let mut fp = [0.0f32; 6];
                    for d in 0..6 {
                        fp[d] = pwr[d] * SUBTICK_HOURS;
                    }
                    let tag = format!("ev{}|u{}", ev.id, aid.0);
                    let a_team = self.units.team[aid.0];
                    let a_pos = self.units.pos[aid.0];
                    resolve_cell_strike(
                        &mut self.grid,
                        &mut self.units,
                        &self.commanders,
                        &mut self.events,
                        target_px,
                        a_team,
                        fp,
                        &weapon,
                        a_pos,
                        !spec.indirect,
                        &tag,
                    );
                    apply_registration_to_cells(&mut self.grid, aid, &self.units, &self.commanders, target_px, radius_px, &weapon, d_m, 5.0, false);
                }
            }
            EventKind::RegisterFire { weapon, target_px, radius_px } => {
                let aid = ev.actor_id;
                let d_m = self.grid.distance_km(self.units.pos[aid.0], [target_px.0 as f32, target_px.1 as f32]) * 1000.0;
                apply_registration_to_cells(&mut self.grid, aid, &self.units, &self.commanders, target_px, radius_px, &weapon, d_m, 15.0, true);
            }
            _ => {}
        }
    }

}
