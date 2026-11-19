use crate::clock::pathfinding::plan_grid_path_f32;
use crate::combat_logistics::{cast_los_ray_dda, resolve_cell_strike_bucketed};
use crate::state::world_state::{DirectCombatLogEntry, SimWorld, SUBTICK_HOURS, SUBTICK_SECONDS};
use crate::units_ecs::{base_reload_sec_for_weapon, equipment_spec, AtomicAction, LockFlags};
use crate::world_grid::MobilityClass;
use rustc_hash::FxHashSet;

impl SimWorld {
    pub fn step_direct_orders_5m(&mut self) -> usize {
        let mut moved_cnt = 0usize;
        let rows = self.grid.rows as i32;
        let cols = self.grid.cols as i32;
        let mut touched_cd_keys: FxHashSet<String> = FxHashSet::default();

        // 1. Units with direct orders
        let unit_keys: Vec<String> = self.direct_unit_plans.keys().cloned().collect();
        for sid in unit_keys {
            let Some(&uid) = self.units.str_to_id.get(&sid) else { continue; };
            if !self.units.alive[uid.0] || self.units.effective_operator_stats(uid).0 == 0 {
                continue;
            }
            let locks = self.units.effective_locks(uid, &self.events.events);

            let mut time_budget = SUBTICK_SECONDS;
            while time_budget > 0.05 {
                let Some(seg) = self.direct_unit_plans.get(&sid).and_then(|p| p.segments.first().cloned()) else {
                    self.direct_unit_plans.remove(&sid);
                    self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                    break;
                };

                let seg_action = seg.action;
                let seg_mm = seg.move_mode;
                let seg_fm = seg.fire_mode;
                let seg_tgt_uid = seg.target_unit_id;
                let seg_follow = seg.follow_target;
                let seg_goal = seg.goal;
                let seg_dur = seg.duration_sec;
                let seg_priority = seg.priority_target;

                // Sync top-level plan metadata with the active segment
                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                    plan.action = seg_action.clone();
                    plan.move_mode = seg_mm.clone();
                    plan.fire_mode = seg_fm.clone();
                    plan.target_unit_id = seg_tgt_uid.clone();
                    plan.follow_target = seg_follow;
                    plan.duration_sec = seg_dur;
                    plan.priority_target = seg_priority.clone();
                    plan.target_pos = Some(seg_goal);
                }

                if seg_action == "stop" {
                    let time_spent = time_budget.min(seg_dur as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;
                    let should_remove = if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    self.units.autonomy_state[uid.0] = AtomicAction::Hold;

                    if new_dur == 0 {
                        if should_remove {
                            self.direct_unit_plans.remove(&sid);
                            break;
                        }
                        self.rebuild_plan_waypoints(uid);
                        continue;
                    } else {
                        break;
                    }
                }

                if seg_action == "attack" {
                    if locks.contains(LockFlags::FORBID_FIRE) {
                        if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.last_fire_summary = "开火受限(状态锁定)".to_string();
                        }
                        self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                        break;
                    }

                    let time_spent = time_budget.min(seg_dur as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;

                    let tgt_uid_opt = seg_tgt_uid.clone();
                    let resolved = if seg_follow {
                        tgt_uid_opt.as_deref().and_then(|tid| {
                            let cand = self.resolve_target_pos_f32(tid)?;
                            let cand_px = (
                                (cand[0].round() as i32).clamp(0, rows - 1),
                                (cand[1].round() as i32).clamp(0, cols - 1),
                            );
                            let los = cast_los_ray_dda(&self.grid, self.units.px[uid.0], cand_px, self.night);
                            let d_km = self.grid.distance_km(self.units.pos[uid.0], cand);
                            if !los.blocked && d_km <= 18.0 { Some(cand) } else { None }
                        })
                    } else {
                        None
                    };
                    let tgt_pos_opt = resolved.or(Some(seg_goal));
                    if let Some(tp) = tgt_pos_opt {
                        let a_pos = self.units.pos[uid.0];
                        let dr = tp[0] - a_pos[0];
                        let dc = tp[1] - a_pos[1];
                        self.units.facing_deg[uid.0] = dc.atan2(-dr).to_degrees();
                        let a_px = self.units.px[uid.0];
                        let t_px = (
                            (tp[0].round() as i32).clamp(0, rows - 1),
                            (tp[1].round() as i32).clamp(0, cols - 1),
                        );
                        let dist_m = self.grid.distance_km(a_pos, tp) * 1000.0;
                        let los = cast_los_ray_dda(&self.grid, a_px, t_px, self.night);
                        let primary_w = self.primary_weapon_for_unit(uid);
                        let spec = equipment_spec(&primary_w);
                        let (disp_m, hit_prob) = Self::compute_fire_ballistics(&primary_w, dist_m, los.blocked);
                        let mut cas_total = 0u32;
                        let mut org_loss_val = 0.0f32;
                        let mut ammo_used = 0.0f32;
                        let mut carry_after = 0.0f32;
                        let mut is_out_of_ammo = self.units.ammo_t[uid.0] <= 0.0005;

                        // Weapon cooldown math
                        let cd_key = format!("{}:{}", sid, primary_w);
                        touched_cd_keys.insert(cd_key.clone());
                        let cur_cd = self.weapon_cooldowns.entry(cd_key).or_insert(0.0);
                        let base_cycle = base_reload_sec_for_weapon(&primary_w);
                        let (eff_crew, _, _, _) = self.units.effective_operator_stats(uid);
                        let eff_crew_ratio = (eff_crew as f32 / spec.crew.max(1) as f32).clamp(0.2, 1.0);
                        let effective_cycle = base_cycle / eff_crew_ratio;

                        let mut shots = 0u32;
                        let mut time_cursor = 0.0f32;
                        while time_cursor + *cur_cd <= time_spent {
                            time_cursor += *cur_cd;
                            shots += 1;
                            *cur_cd = effective_cycle;
                        }
                        let leftover = time_spent - time_cursor;
                        *cur_cd = (*cur_cd - leftover).max(0.0);

                        let summary = if los.blocked && !spec.indirect {
                            "视线被地形/林地遮挡".to_string()
                        } else if is_out_of_ammo {
                            "弹药耗尽".to_string()
                        } else if shots == 0 {
                            format!("火炮装填中 (冷却还剩 {:.0}s)", *cur_cd)
                        } else {
                            let fire_ratio = self.units.fire_eligible_ratio(uid);
                            if fire_ratio > 0.0 {
                                let pwr = self.units.unit_power(uid, dist_m, true, None, 1.0);
                                let cqb_lethality = (280.0 / dist_m.max(15.0)).powf(1.4).clamp(1.5, 110.0) * 2.6;
                                let mut fire_dt = [0.0f32; 6];
                                let mut base_pts = 0.0f32;
                                let shot_time_h = (time_spent / 3600.0) * (shots as f32).min(4.0);
                                for d in 0..6 {
                                    let step_p = pwr[d] * shot_time_h * fire_ratio;
                                    base_pts += step_p;
                                    fire_dt[d] = step_p * cqb_lethality;
                                }
                                if base_pts > 1e-4 {
                                    ammo_used = self.units.consume_soldier_ammo(uid, base_pts * 0.002);
                                    if self.units.ammo_t[uid.0] <= 0.0005 {
                                        is_out_of_ammo = true;
                                    }
                                    let tag = format!("direct{}|u{}", self.subtick, uid.0);
                                    let a_team = self.units.team[uid.0];
                                    let rep = resolve_cell_strike_bucketed(
                                        &mut self.grid,
                                        &mut self.units,
                                        &self.commanders,
                                        &mut self.events,
                                        Some(&self.derived.buckets),
                                        t_px,
                                        a_team,
                                        fire_dt,
                                        &primary_w,
                                        a_pos,
                                        !spec.indirect,
                                        &tag,
                                        self.debug_full_friendly_fire,
                                        Some(uid),
                                    );
                                    cas_total = rep.person_losses + rep.friendly_fire_losses;
                                    org_loss_val = rep.org_loss;
                                    if let Some(ref tid) = tgt_uid_opt {
                                        if let Some(&tcid) = self.commanders.str_to_id.get(tid) {
                                            if let Some(hq_u) = self.commanders.hq_piece_id[tcid.0] {
                                                carry_after = self.units.carry_loss[hq_u.0];
                                            }
                                        } else if let Some(&tuid) = self.units.str_to_id.get(tid) {
                                            carry_after = self.units.carry_loss[tuid.0];
                                        }
                                    }
                                    let tname = tgt_uid_opt.as_deref().map(|t| self.resolve_target_name(t));
                                    let actor_name = self.units.name[uid.0].clone();
                                    let msg = format!(
                                        "{} [{}] 射击 {} ({:.0}m, 散布{:.0}m, 命中{:.0}%, 开火{}发) -> 伤亡 {}人 (累积{:.2}), 组织度 -{:.1}",
                                        actor_name, primary_w, tname.as_deref().unwrap_or("目标格"),
                                        dist_m, disp_m, hit_prob * 100.0, shots, cas_total, carry_after, org_loss_val
                                    );
                                    self.push_combat_log(DirectCombatLogEntry {
                                        turn: self.turn, clock_hours: self.clock_hours,
                                        actor_id: sid.clone(), actor_name,
                                        target_id: tgt_uid_opt.clone(), target_name: tname,
                                        kind: "fire".into(), weapon_model: primary_w.clone(),
                                        dist_m, dispersion_m: disp_m, hit_prob,
                                        casualties: cas_total, org_loss: org_loss_val,
                                        carry_loss_after: carry_after, ammo_used_t: ammo_used,
                                        message: msg,
                                    });
                                    format!("开火{}发 杀伤{}人(累积{:.2}) 组织-{:.1}", shots, cas_total, carry_after, org_loss_val)
                                } else {
                                    "火力准备中".to_string()
                                }
                            } else {
                                "无开火人员".to_string()
                            }
                        };

                        if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.target_pos = Some(tp);
                            plan.last_los_blocked = los.blocked;
                            plan.out_of_ammo = is_out_of_ammo;
                            plan.last_fire_summary = if is_out_of_ammo { "弹药耗尽".to_string() } else { summary };
                            plan.weapon_model = primary_w;
                            plan.dispersion_m = disp_m;
                            plan.hit_prob = hit_prob;
                            plan.last_casualties = cas_total;
                            plan.last_org_loss = org_loss_val;
                            plan.last_ammo_used_t = ammo_used;
                        }
                    }

                    self.units.autonomy_state[uid.0] = AtomicAction::Attack;

                    let should_remove = if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };

                    if new_dur == 0 {
                        if should_remove {
                            self.direct_unit_plans.remove(&sid);
                            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                            break;
                        }
                        self.rebuild_plan_waypoints(uid);
                        continue;
                    } else {
                        break;
                    }
                }

                // Movement actions (move / retreat / assault)
                if locks.intersects(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE) {
                    self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                    break;
                }

                if seg_action == "retreat" || seg_mm == "fast" {
                    self.units.exposed[uid.0] = true;
                } else if matches!(seg_mm.as_str(), "slow" | "stealth") {
                    self.units.exposed[uid.0] = false;
                }

                let cur = self.units.pos[uid.0];
                let mut follow_goal = seg_goal;
                if seg_follow {
                    if let Some(ref tid) = seg_tgt_uid {
                        if let Some(cand) = self.resolve_target_pos_f32(tid) {
                            let cand_px = (
                                (cand[0].round() as i32).clamp(0, rows - 1),
                                (cand[1].round() as i32).clamp(0, cols - 1),
                            );
                            let los = cast_los_ray_dda(&self.grid, self.units.px[uid.0], cand_px, self.night);
                            let d_km = self.grid.distance_km(cur, cand);
                            if !los.blocked && d_km <= 18.0 {
                                follow_goal = cand;
                                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                                    plan.target_pos = Some(cand);
                                }
                            }
                        }
                    }
                }

                let has_wps = self.direct_unit_plans.get(&sid).map_or(false, |p| !p.waypoints.is_empty());
                if !has_wps {
                    let path = plan_grid_path_f32(&self.grid, cur, follow_goal, self.units.mobility[uid.0]);
                    if path.is_empty() {
                        let is_persistent_follow = seg_follow && seg_tgt_uid.is_some() && self.direct_unit_plans.get(&sid).map_or(0, |p| p.segments.len()) <= 1;
                        if is_persistent_follow {
                            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                            break;
                        }
                        let should_remove = if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            true
                        };
                        if should_remove {
                            self.direct_unit_plans.remove(&sid);
                            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                            break;
                        }
                        self.rebuild_plan_waypoints(uid);
                        continue;
                    } else {
                        if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                            plan.waypoints = path;
                        }
                    }
                }

                let mut wps_tmp = self.direct_unit_plans.get_mut(&sid).map(|p| std::mem::take(&mut p.waypoints)).unwrap_or_default();
                let (new_pos, moved, deg, rem_h) = self.advance_unit_along_waypoints_dt(
                    uid,
                    cur,
                    &mut wps_tmp,
                    &seg_action,
                    &seg_mm,
                    time_budget / 3600.0,
                );
                let Some(plan) = self.direct_unit_plans.get_mut(&sid) else { break; };
                plan.waypoints = wps_tmp;
                if moved {
                    self.units.pos[uid.0] = new_pos;
                    self.units.px[uid.0] = (
                        (new_pos[0].round() as i32).clamp(0, rows - 1),
                        (new_pos[1].round() as i32).clamp(0, cols - 1),
                    );
                    self.units.facing_deg[uid.0] = deg;
                    self.units.moved_last_turn[uid.0] = true;
                    self.units.autonomy_state[uid.0] = if seg_action == "retreat" { AtomicAction::Retreat } else { AtomicAction::Move };
                    moved_cnt += 1;
                }
                time_budget = rem_h * 3600.0;
                if plan.waypoints.is_empty() {
                    let is_persistent_follow = seg_follow && seg_tgt_uid.is_some() && plan.segments.len() <= 1;
                    if is_persistent_follow {
                        self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                        break;
                    }
                    plan.segments.remove(0);
                    if plan.segments.is_empty() {
                        self.direct_unit_plans.remove(&sid);
                        self.units.autonomy_state[uid.0] = AtomicAction::Hold;
                        break;
                    }
                    self.rebuild_plan_waypoints(uid);
                    continue;
                } else {
                    break;
                }
            }
        }

        // Background cooldown decay for units not actively attacking in this tick
        for (k, cd) in self.weapon_cooldowns.iter_mut() {
            if !touched_cd_keys.contains(k) && *cd > 0.0 {
                *cd = (*cd - SUBTICK_SECONDS).max(0.0);
            }
        }

        // 2. Commanders with direct orders
        let cmd_keys: Vec<String> = self.direct_cmd_plans.keys().cloned().collect();
        for sid in cmd_keys {
            let Some(&cid) = self.commanders.str_to_id.get(&sid) else { continue; };
            let hq_alive = self.commanders.alive[cid.0]
                && self.commanders.hq_piece_id[cid.0].map_or(true, |u| self.units.alive[u.0] && self.units.persons(u) > 0);
            if !hq_alive {
                self.direct_cmd_plans.remove(&sid);
                continue;
            }
            let action = self.direct_cmd_plans.get(&sid).map(|p| p.action.clone()).unwrap_or_default();
            if action == "stop" {
                continue;
            }
            let cmdr_locks = self.commanders.hq_piece_id[cid.0]
                .map(|u| self.units.effective_locks(u, &self.events.events))
                .unwrap_or_default();

            let mut time_budget = SUBTICK_SECONDS;
            while time_budget > 0.05 {
                let Some(seg) = self.direct_cmd_plans.get(&sid).and_then(|p| p.segments.first().cloned()) else {
                    self.direct_cmd_plans.remove(&sid);
                    break;
                };

                let seg_action = seg.action;
                let seg_goal = seg.goal;
                let seg_dur = seg.duration_sec;
                let seg_tgt_uid = seg.target_unit_id;

                if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                    plan.action = seg_action.clone();
                    plan.target_pos = Some(seg_goal);
                    plan.target_unit_id = seg_tgt_uid.clone();
                    plan.duration_sec = seg_dur;
                }

                if seg_action == "stop" {
                    let time_spent = time_budget.min(seg_dur as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;
                    let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    if should_remove {
                        self.direct_cmd_plans.remove(&sid);
                        break;
                    }
                    if new_dur == 0 {
                        continue;
                    } else {
                        break;
                    }
                }

                if seg_action == "attack" {
                    if cmdr_locks.contains(LockFlags::FORBID_FIRE) {
                        if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                            plan.last_fire_summary = "开火受限(状态锁定)".to_string();
                        }
                        break;
                    }
                    let time_spent = time_budget.min(seg_dur.max(16) as f32);
                    time_budget -= time_spent;
                    let new_dur = (seg_dur as f32 - time_spent).max(0.0).round() as u32;

                    let tgt_uid_opt = seg_tgt_uid.clone();
                    let resolved = tgt_uid_opt.as_deref().and_then(|tid| self.resolve_target_pos_f32(tid));
                    let tgt_pos_opt = resolved.or(Some(seg_goal));
                    if let Some(tp) = tgt_pos_opt {
                        let a_pos = self.commander_pos_f32(cid);
                        let a_px = self.commanders.px[cid.0];
                        let t_px = (
                            (tp[0].round() as i32).clamp(0, rows - 1),
                            (tp[1].round() as i32).clamp(0, cols - 1),
                        );
                        let dist_m = self.grid.distance_km(a_pos, tp) * 1000.0;
                        let los = cast_los_ray_dda(&self.grid, a_px, t_px, self.night);
                        let (disp_m, hit_prob) = Self::compute_fire_ballistics("rifle", dist_m, los.blocked);
                        let mut cas_total = 0u32;
                        let mut org_loss_val = 0.0f32;
                        let mut ammo_used = 0.0f32;
                        let mut is_out_of_ammo = self.commanders.hq_piece_id[cid.0].map_or(false, |u| self.units.ammo_t[u.0] <= 0.0005);
                        let mut summary = if los.blocked { "视线受阻".to_string() } else if is_out_of_ammo { String::new() } else { "警戒瞄准".to_string() };
                        if let Some(hq_uid) = self.commanders.hq_piece_id[cid.0] {
                            let fire_ratio = self.units.fire_eligible_ratio(hq_uid);
                            if self.units.alive[hq_uid.0] && self.units.persons(hq_uid) > 0 && !los.blocked && !is_out_of_ammo && fire_ratio > 0.0 {
                                let pwr = self.units.unit_power(hq_uid, dist_m, true, None, 1.0);
                                let cqb_lethality = (280.0 / dist_m.max(15.0)).powf(1.4).clamp(1.5, 110.0) * 2.6;
                                let mut fire_dt = [0.0f32; 6];
                                let mut base_pts = 0.0f32;
                                for d in 0..6 {
                                    let step_p = pwr[d] * SUBTICK_HOURS * fire_ratio;
                                    base_pts += step_p;
                                    fire_dt[d] = step_p * cqb_lethality;
                                }
                                ammo_used = self.units.consume_soldier_ammo(hq_uid, base_pts * 0.002);
                                if self.units.ammo_t[hq_uid.0] <= 0.0005 { is_out_of_ammo = true; }
                                let rep = resolve_cell_strike_bucketed(
                                    &mut self.grid, &mut self.units, &self.commanders, &mut self.events,
                                    Some(&self.derived.buckets),
                                    t_px, self.commanders.team[cid.0], fire_dt, "rifle", a_pos, true,
                                    &format!("directcmd{}|c{}", self.subtick, cid.0), self.debug_full_friendly_fire, Some(hq_uid),
                                );
                                cas_total = rep.person_losses + rep.friendly_fire_losses;
                                org_loss_val = rep.org_loss;
                                summary = format!("伤亡{}人 组织度-{:.1}", cas_total, org_loss_val);
                                let tname = tgt_uid_opt.as_deref().map(|t| self.resolve_target_name(t));
                                let actor_name = self.commanders.name[cid.0].clone();
                                let msg = format!(
                                    "{} [rifle] 射击 {} ({:.0}m, 散布{:.0}m) -> 伤亡 {}人, 组织度 -{:.1}",
                                    actor_name, tname.as_deref().unwrap_or("目标格"), dist_m, disp_m, cas_total, org_loss_val
                                );
                                self.push_combat_log(DirectCombatLogEntry {
                                    turn: self.turn, clock_hours: self.clock_hours,
                                    actor_id: sid.clone(), actor_name,
                                    target_id: tgt_uid_opt.clone(), target_name: tname,
                                    kind: "fire".into(), weapon_model: "rifle".into(),
                                    dist_m, dispersion_m: disp_m, hit_prob,
                                    casualties: cas_total, org_loss: org_loss_val,
                                    carry_loss_after: 0.0, ammo_used_t: ammo_used,
                                    message: msg,
                                });
                            }
                        }
                        if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                            plan.target_pos = Some(tp);
                            plan.last_los_blocked = los.blocked;
                            plan.out_of_ammo = is_out_of_ammo;
                            plan.last_fire_summary = if is_out_of_ammo { String::new() } else { summary };
                            plan.weapon_model = "rifle".into();
                            plan.dispersion_m = disp_m;
                            plan.hit_prob = hit_prob;
                            plan.last_casualties = cas_total;
                            plan.last_org_loss = org_loss_val;
                            plan.last_ammo_used_t = ammo_used;
                        }
                    }

                    let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments[0].duration_sec = new_dur;
                        }
                        plan.duration_sec = new_dur;
                        if new_dur == 0 {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    if should_remove {
                        self.direct_cmd_plans.remove(&sid);
                        break;
                    }
                    if new_dur == 0 {
                        continue;
                    } else {
                        break;
                    }
                }

                // Movement actions (move)
                if cmdr_locks.intersects(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE) {
                    break;
                }

                let cur = self.commander_pos_f32(cid);
                let has_wps = self.direct_cmd_plans.get(&sid).map_or(false, |p| !p.waypoints.is_empty());
                if !has_wps {
                    let path = plan_grid_path_f32(&self.grid, cur, seg_goal, MobilityClass::Wheeled);
                    if path.is_empty() {
                        let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                            plan.segments.remove(0);
                            plan.segments.is_empty()
                        } else {
                            true
                        };
                        if should_remove {
                            self.direct_cmd_plans.remove(&sid);
                            break;
                        }
                        continue;
                    } else if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        plan.waypoints = path;
                    }
                }

                let mut wps_tmp = self.direct_cmd_plans.get_mut(&sid).map(|p| std::mem::take(&mut p.waypoints)).unwrap_or_default();
                let (new_pos, moved, _deg) = Self::advance_pos_along_waypoints(&self.grid, cur, &mut wps_tmp, 0.65, 0.85);
                if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                    plan.waypoints = wps_tmp;
                }
                if moved {
                    let new_px = (
                        (new_pos[0].round() as i32).clamp(0, rows - 1),
                        (new_pos[1].round() as i32).clamp(0, cols - 1),
                    );
                    self.direct_cmd_pos.insert(sid.clone(), new_pos);
                    self.commanders.px[cid.0] = new_px;
                    if let Some(hq_uid) = self.commanders.hq_piece_id[cid.0] {
                        self.units.pos[hq_uid.0] = new_pos;
                        self.units.px[hq_uid.0] = new_px;
                    }
                    moved_cnt += 1;
                }

                let is_wps_empty = self.direct_cmd_plans.get(&sid).map_or(true, |p| p.waypoints.is_empty());
                if is_wps_empty {
                    let should_remove = if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                        if !plan.segments.is_empty() {
                            plan.segments.remove(0);
                        }
                        plan.segments.is_empty()
                    } else {
                        true
                    };
                    if should_remove {
                        self.direct_cmd_plans.remove(&sid);
                        break;
                    }
                    continue;
                } else {
                    break;
                }
            }
        }

        moved_cnt
    }

}
