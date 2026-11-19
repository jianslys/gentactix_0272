use crate::clock::pathfinding::plan_grid_path_f32;
use crate::state::world_state::{DirectCombatLogEntry, DirectOrderPlan, SimWorld, WaypointSegmentConfig};
use crate::units_ecs::{equipment_spec, AtomicAction, UnitId};
use crate::world_grid::{MobilityClass, WorldGrid};

impl SimWorld {
    pub(crate) fn advance_pos_along_waypoints(
        grid: &WorldGrid,
        cur: [f32; 2],
        waypoints: &mut Vec<[f32; 2]>,
        base_cells_per_5m: f32,
        forest_drag: f32,
    ) -> ([f32; 2], bool, f32) {
        let mut pos = cur;
        let r_i = (pos[0].round() as i32).clamp(0, grid.rows as i32 - 1);
        let c_i = (pos[1].round() as i32).clamp(0, grid.cols as i32 - 1);
        let fd = grid.forest_density_at(r_i, c_i);
        let mut budget = base_cells_per_5m / (1.0 + fd * forest_drag);
        let mut moved = false;
        let mut last_deg = 0.0f32;
        while budget > 1e-4 && !waypoints.is_empty() {
            let tgt = waypoints[0];
            let dr = tgt[0] - pos[0];
            let dc = tgt[1] - pos[1];
            let dist = dr.hypot(dc);
            if dist <= 1e-4 {
                waypoints.remove(0);
                continue;
            }
            last_deg = dc.atan2(-dr).to_degrees();
            if dist <= budget {
                pos = tgt;
                budget -= dist;
                waypoints.remove(0);
                moved = true;
            } else {
                let s = budget / dist;
                pos = [pos[0] + dr * s, pos[1] + dc * s];
                budget = 0.0;
                moved = true;
            }
        }
        (pos, moved, last_deg)
    }

    pub fn advance_unit_along_waypoints_dt(
        &self,
        uid: UnitId,
        mut pos: [f32; 2],
        waypoints: &mut Vec<[f32; 2]>,
        action: &str,
        move_mode: &str,
        dt_hours: f32,
    ) -> ([f32; 2], bool, f32, f32) {
        let mut rem_h = dt_hours;
        let mut moved = false;
        let mut last_deg = self.units.facing_deg[uid.0];
        let rows = self.grid.rows as i32;
        let cols = self.grid.cols as i32;
        while rem_h > 1e-6 && !waypoints.is_empty() {
            let tgt = waypoints[0];
            let dr = tgt[0] - pos[0];
            let dc = tgt[1] - pos[1];
            let dist_px = dr.hypot(dc);
            if dist_px <= 1e-4 {
                waypoints.remove(0);
                continue;
            }
            let r0 = (pos[0].round() as i32).clamp(0, rows - 1);
            let c0 = (pos[1].round() as i32).clamp(0, cols - 1);
            let probe = 1.0f32.min(dist_px) / dist_px;
            let r1 = ((pos[0] + dr * probe).round() as i32).clamp(0, rows - 1);
            let c1 = ((pos[1] + dc * probe).round() as i32).clamp(0, cols - 1);
            let spd_kmh = self
                .units
                .effective_speed_kmh(uid, &self.grid, r0, c0, r1, c1, action, move_mode);
            if spd_kmh <= 0.0 {
                break;
            }
            let px_per_h = spd_kmh * 1000.0 / self.grid.cell_m;
            let budget_px = px_per_h * rem_h;
            let step_px = dist_px.min(budget_px).min(0.95);
            if step_px <= 1e-5 {
                break;
            }
            last_deg = dc.atan2(-dr).to_degrees();
            if step_px >= dist_px - 1e-4 {
                pos = tgt;
                waypoints.remove(0);
            } else {
                let s = step_px / dist_px;
                pos = [pos[0] + dr * s, pos[1] + dc * s];
            }
            rem_h -= step_px / px_per_h;
            moved = true;
        }
        (pos, moved, last_deg, rem_h.max(0.0))
    }

    pub fn advance_unit_along_waypoints_5m(
        &self,
        uid: UnitId,
        pos: [f32; 2],
        waypoints: &mut Vec<[f32; 2]>,
        action: &str,
        move_mode: &str,
        dt_hours: f32,
    ) -> ([f32; 2], bool, f32) {
        let (p, m, d, _) = self.advance_unit_along_waypoints_dt(uid, pos, waypoints, action, move_mode, dt_hours);
        (p, m, d)
    }

    pub fn estimate_path_time_minutes(
        &self,
        uid_opt: Option<UnitId>,
        mob: MobilityClass,
        mut pos: [f32; 2],
        waypoints: &[[f32; 2]],
        action: &str,
        move_mode: &str,
    ) -> (f32, f32, f32) {
        let mut total_h = 0.0f32;
        let mut total_km = 0.0f32;
        let rows = self.grid.rows as i32;
        let cols = self.grid.cols as i32;
        for &tgt in waypoints {
            loop {
                let dr = tgt[0] - pos[0];
                let dc = tgt[1] - pos[1];
                let dist_px = dr.hypot(dc);
                if dist_px <= 1e-4 {
                    pos = tgt;
                    break;
                }
                let step_px = dist_px.min(0.95);
                let s = step_px / dist_px;
                let nxt = [pos[0] + dr * s, pos[1] + dc * s];
                let r0 = (pos[0].round() as i32).clamp(0, rows - 1);
                let c0 = (pos[1].round() as i32).clamp(0, cols - 1);
                let r1 = (nxt[0].round() as i32).clamp(0, rows - 1);
                let c1 = (nxt[1].round() as i32).clamp(0, cols - 1);
                let spd = if let Some(u) = uid_opt {
                    let s_u = self
                        .units
                        .effective_speed_kmh(u, &self.grid, r0, c0, r1, c1, action, move_mode);
                    if s_u <= 0.0 { 0.5 } else { s_u }
                } else {
                    let tc = crate::world_grid::terrain_cost(mob, self.grid.terrain[self.grid.idx(r1, c1)]);
                    let mode_m = match move_mode {
                        "fast" => 1.25,
                        "slow" | "stealth" => 0.55,
                        _ => 1.0,
                    };
                    (18.0 * mode_m / tc.max(0.35)).clamp(2.0, 48.0)
                };
                let slice_km = step_px * self.grid.cell_m / 1000.0;
                total_km += slice_km;
                total_h += slice_km / spd.max(0.2);
                pos = nxt;
                if step_px >= dist_px - 1e-4 {
                    pos = tgt;
                    break;
                }
            }
        }
        let avg_spd = if total_h > 1e-5 { total_km / total_h } else { 0.0 };
        (total_h * 60.0, total_km, avg_spd)
    }

    pub fn resolve_target_pos_f32(&self, target_id: &str) -> Option<[f32; 2]> {
        if let Some(&cid) = self.commanders.str_to_id.get(target_id) {
            if self.commanders.alive[cid.0] {
                return Some(self.commander_pos_f32(cid));
            }
        }
        if let Some(&uid) = self.units.str_to_id.get(target_id) {
            if self.units.alive[uid.0] && self.units.persons(uid) > 0 {
                return Some(self.units.pos[uid.0]);
            }
        }
        None
    }

    pub(crate) fn push_combat_log(&mut self, entry: DirectCombatLogEntry) {
        self.direct_combat_logs.push(entry);
        if self.direct_combat_logs.len() > 100 {
            let drain_n = self.direct_combat_logs.len() - 100;
            self.direct_combat_logs.drain(0..drain_n);
        }
    }

    pub fn primary_weapon_for_unit(&self, uid: UnitId) -> String {
        self.units.equipment[uid.0]
            .iter()
            .filter(|(_, &cnt)| cnt > 0)
            .max_by(|a, b| equipment_spec(a.0).fire[0].total_cmp(&equipment_spec(b.0).fire[0]))
            .map(|(m, _)| m.clone())
            .unwrap_or_else(|| "rifle".to_string())
    }

    pub fn compute_fire_ballistics(weapon_model: &str, dist_m: f32, los_blocked: bool) -> (f32, f32) {
        let spec = equipment_spec(weapon_model);
        let disp_m = (12.0 + dist_m * 0.14).clamp(15.0, 120.0);
        if los_blocked {
            return (disp_m, 0.0);
        }
        let prob = (1.0 - dist_m / (spec.range_m * 1.12)).clamp(0.08, 0.95);
        (disp_m, prob)
    }

    pub fn set_direct_unit_order(
        &mut self,
        uid: UnitId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
    ) {
        let _ = self.try_set_direct_unit_order_ex(
            uid,
            action,
            goal,
            append,
            target_unit_id,
            "normal",
            "stop_to_fire",
            true,
        );
    }

    pub fn try_set_direct_unit_order_ex(
        &mut self,
        uid: UnitId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
        move_mode: &str,
        fire_mode: &str,
        follow_target: bool,
    ) -> Result<(), String> {
        self.try_set_direct_unit_order_v33(
            uid,
            action,
            goal,
            append,
            target_unit_id,
            move_mode,
            fire_mode,
            follow_target,
            None,
            None,
        )
    }

    pub fn try_set_direct_unit_order_v33(
        &mut self,
        uid: UnitId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
        move_mode: &str,
        fire_mode: &str,
        follow_target: bool,
        duration_sec: Option<u32>,
        priority_target: Option<&str>,
    ) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 && action != "retreat" {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let sname = self.units.name[uid.0].clone();
        let pw = self.primary_weapon_for_unit(uid);
        let mm = if matches!(move_mode, "fast" | "slow" | "stealth") { move_mode } else { "normal" };
        let mob = self.units.mobility[uid.0];
        let fm = fire_mode;
        let dur_sec = duration_sec.unwrap_or(300);
        let prio_tgt = priority_target.unwrap_or("any").to_string();

        if action == "stop" {
            let seg = WaypointSegmentConfig {
                start_idx: 0,
                goal: self.units.pos[uid.0],
                action: "stop".into(),
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: false,
                target_unit_id: None,
                duration_sec: dur_sec,
                priority_target: prio_tgt.clone(),
            };
            if append {
                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                    plan.segments.push(seg);
                    return Ok(());
                }
            }
            self.direct_unit_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "stop".into(),
                    waypoints: Vec::new(),
                    target_pos: None,
                    target_unit_id: None,
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    segments: vec![seg],
                    duration_sec: dur_sec,
                    priority_target: prio_tgt,
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: String::new(),
                    weapon_model: pw.clone(),
                    dispersion_m: 0.0,
                    hit_prob: 0.0,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: None, target_name: None,
                kind: "order".into(), weapon_model: pw, dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: "下达【停止】指令，原地驻留待命。".into(),
            });
            return Ok(());
        }

        if action == "retreat" {
            // 不可逆撤退：设置 0.5 小时（30 分钟）不可控狂奔恢复期
            self.units.retreat_recovery_hours[uid.0] = 0.5;
            let cur = self.units.pos[uid.0];
            let team = self.units.team[uid.0];
            // Compute safest rearward direction away from nearest hostile or toward HQ/rear
            let mut threat = [0.0f32; 2];
            for j in 0..self.units.id.len() {
                if !self.units.alive[j] || self.units.team[j] == team || self.units.persons(UnitId(j)) == 0 {
                    continue;
                }
                let d_km = self.grid.distance_km(cur, self.units.pos[j]);
                if d_km <= 3.0 {
                    let w = (-d_km / 0.8).exp();
                    let dr = self.units.pos[j][0] - cur[0];
                    let dc = self.units.pos[j][1] - cur[1];
                    let dm = dr.hypot(dc).max(1e-4);
                    threat[0] += (dr / dm) * w;
                    threat[1] += (dc / dm) * w;
                }
            }
            let (rx, ry) = if threat[0].hypot(threat[1]) > 1e-3 {
                let m = threat[0].hypot(threat[1]);
                (-threat[0] / m, -threat[1] / m)
            } else {
                match team {
                    crate::world_grid::TeamId::Red => (0.0, -1.0),
                    crate::world_grid::TeamId::Blue => (0.0, 1.0),
                }
            };
            let fallback_goal = goal.unwrap_or_else(|| {
                self.grid.clamp_pos([cur[0] + rx * 8.0, cur[1] + ry * 8.0])
            });
            let seg = plan_grid_path_f32(&self.grid, cur, fallback_goal, mob);
            self.direct_unit_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "retreat".into(),
                    waypoints: seg,
                    target_pos: Some(fallback_goal),
                    target_unit_id: None,
                    move_mode: "fast".into(),
                    fire_mode: "fire_on_the_move".into(),
                    follow_target: false,
                    segments: vec![WaypointSegmentConfig {
                        start_idx: 0,
                        goal: fallback_goal,
                        action: "retreat".into(),
                        move_mode: "fast".into(),
                        fire_mode: "fire_on_the_move".into(),
                        follow_target: false,
                        target_unit_id: None,
                        duration_sec: dur_sec,
                        priority_target: prio_tgt.clone(),
                    }],
                    duration_sec: dur_sec,
                    priority_target: prio_tgt,
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: "溃退撤离中（不可逆恢复期）".into(),
                    weapon_model: pw.clone(),
                    dispersion_m: 0.0,
                    hit_prob: 0.0,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.units.autonomy_state[uid.0] = AtomicAction::Retreat;
            self.units.exposed[uid.0] = true;
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: None, target_name: None,
                kind: "order".into(), weapon_model: pw, dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: format!("执行【不可逆撤退】-> 脱离接触向后狂奔 ({:.1}, {:.1})，恢复期内不可控。", fallback_goal[0], fallback_goal[1]),
            });
            return Ok(());
        }

        if action == "attack" {
            let tgt_pos = target_unit_id
                .as_deref()
                .and_then(|tid| self.resolve_target_pos_f32(tid))
                .or(goal);
            let Some(tp) = tgt_pos else { return Err("missing_attack_target".into()); };
            let dist_m = self.grid.distance_km(self.units.pos[uid.0], tp) * 1000.0;
            let (disp_m, hit_prob) = Self::compute_fire_ballistics(&pw, dist_m, false);
            let tname = target_unit_id.as_deref().map(|tid| self.resolve_target_name(tid));
            let seg = WaypointSegmentConfig {
                start_idx: 0,
                goal: tp,
                action: "attack".into(),
                move_mode: "normal".into(),
                fire_mode: fm.into(),
                follow_target,
                target_unit_id: target_unit_id.clone(),
                duration_sec: dur_sec,
                priority_target: prio_tgt.clone(),
            };
            if append {
                if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                    plan.segments.push(seg);
                    return Ok(());
                }
            }
            self.direct_unit_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "attack".into(),
                    waypoints: Vec::new(),
                    target_pos: Some(tp),
                    target_unit_id: target_unit_id.clone(),
                    move_mode: "normal".into(),
                    fire_mode: fm.into(),
                    follow_target,
                    segments: vec![seg],
                    duration_sec: dur_sec,
                    priority_target: prio_tgt,
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: format!("准备射击 · 散布{:.0}m · 命中{:.0}%", disp_m, hit_prob * 100.0),
                    weapon_model: pw.clone(),
                    dispersion_m: disp_m,
                    hit_prob,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.units.autonomy_state[uid.0] = AtomicAction::Hold;
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: target_unit_id, target_name: tname.clone(),
                kind: "order".into(), weapon_model: pw, dist_m, dispersion_m: disp_m, hit_prob,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: format!(
                    "瞄准 {} ({:.0}m, {}, 散布 {:.0}m, 预计命中 {:.0}%)。",
                    tname.as_deref().unwrap_or("指定坐标"),
                    dist_m,
                    if follow_target { "跟随目标" } else { "固定坐标" },
                    disp_m,
                    hit_prob * 100.0
                ),
            });
            return Ok(());
        }

        let Some(goal_pos) = goal else { return Err("missing_goal".into()); };
        let act_norm = if action == "assault" { "assault" } else { "move" };
        let start = if append {
            self.direct_unit_plans
                .get(&sid)
                .and_then(|p| p.waypoints.last().copied())
                .unwrap_or(self.units.pos[uid.0])
        } else {
            self.units.pos[uid.0]
        };
        let mut seg = plan_grid_path_f32(&self.grid, start, goal_pos, mob);
        if seg.is_empty() && (start[0] - goal_pos[0]).hypot(start[1] - goal_pos[1]) > 0.05 {
            seg.push(goal_pos);
        }
        if append {
            if let Some(plan) = self.direct_unit_plans.get_mut(&sid) {
                let start_idx = plan.waypoints.len();
                if plan.waypoints.is_empty() && plan.segments.is_empty() {
                    plan.waypoints = seg;
                    plan.action = act_norm.into();
                    plan.target_pos = Some(goal_pos);
                }
                plan.segments.push(WaypointSegmentConfig {
                    start_idx,
                    goal: goal_pos,
                    action: act_norm.into(),
                    move_mode: mm.into(),
                    fire_mode: fm.into(),
                    follow_target,
                    target_unit_id: target_unit_id.clone(),
                    duration_sec: dur_sec,
                    priority_target: prio_tgt.clone(),
                });
                return Ok(());
            }
        }
        self.direct_unit_plans.insert(
            sid.clone(),
            DirectOrderPlan {
                action: act_norm.into(),
                waypoints: seg,
                target_pos: Some(goal_pos),
                target_unit_id: target_unit_id.clone(),
                move_mode: mm.into(),
                fire_mode: fm.into(),
                follow_target,
                segments: vec![WaypointSegmentConfig {
                    start_idx: 0,
                    goal: goal_pos,
                    action: act_norm.into(),
                    move_mode: mm.into(),
                    fire_mode: fm.into(),
                    follow_target,
                    target_unit_id: target_unit_id.clone(),
                    duration_sec: dur_sec,
                    priority_target: prio_tgt.clone(),
                }],
                duration_sec: dur_sec,
                priority_target: prio_tgt,
                last_los_blocked: false,
                out_of_ammo: false,
                last_fire_summary: String::new(),
                weapon_model: pw.clone(),
                dispersion_m: 0.0,
                hit_prob: 0.0,
                last_casualties: 0,
                last_org_loss: 0.0,
                last_ammo_used_t: 0.0,
            },
        );
        self.units.autonomy_state[uid.0] = AtomicAction::Move;
        let mode_cn = match mm { "fast" => "快速", "slow" | "stealth" => "隐蔽渗透", _ => "正常" };
        let label_cn = if act_norm == "assault" {
            format!("进攻/{}/{}", mode_cn, if fm == "fire_on_the_move" { "移动射击" } else { "遇敌停下射击" })
        } else {
            format!("移动/{}", mode_cn)
        };
        self.push_combat_log(DirectCombatLogEntry {
            turn: self.turn, clock_hours: self.clock_hours,
            actor_id: sid, actor_name: sname, target_id: None, target_name: None,
            kind: "order".into(), weapon_model: pw, dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
            casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
            message: format!("下达【{}】指令 -> 目标 ({:.1}, {:.1})。", label_cn, goal_pos[0], goal_pos[1]),
        });
        Ok(())
    }

}
