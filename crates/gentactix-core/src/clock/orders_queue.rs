use crate::clock::pathfinding::plan_grid_path_f32;
use crate::state::world_state::{DirectCombatLogEntry, DirectOrderPlan, SimWorld, WaypointSegmentConfig};
use crate::units_ecs::{CommanderId, UnitId};
use crate::world_grid::MobilityClass;

impl SimWorld {
    pub fn rebuild_plan_waypoints(&mut self, uid: UnitId) {
        let sid = self.units.str_id[uid.0].clone();
        let cur_pos = self.units.pos[uid.0];
        let mob = self.units.mobility[uid.0];
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else { return; };
        if plan.segments.is_empty() {
            plan.action = "stop".into();
            plan.waypoints.clear();
            plan.target_pos = None;
            return;
        }
        let seg = &plan.segments[0];
        plan.action = seg.action.clone();
        plan.move_mode = seg.move_mode.clone();
        plan.fire_mode = seg.fire_mode.clone();
        plan.follow_target = seg.follow_target;
        plan.target_unit_id = seg.target_unit_id.clone();
        plan.duration_sec = seg.duration_sec;
        plan.priority_target = seg.priority_target.clone();
        plan.target_pos = Some(seg.goal);
        if seg.action == "move" || seg.action == "assault" || seg.action == "retreat" {
            plan.waypoints = plan_grid_path_f32(&self.grid, cur_pos, seg.goal, mob);
        } else {
            plan.waypoints.clear();
        }
    }

    pub fn delete_task_step(&mut self, uid: UnitId, step_idx: usize) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else {
            return Err("no_active_plan".into());
        };
        if plan.segments.is_empty() {
            self.direct_unit_plans.remove(&sid);
            return Ok(());
        }
        if step_idx >= plan.segments.len() {
            if plan.segments.len() <= 1 {
                self.direct_unit_plans.remove(&sid);
                return Ok(());
            }
            return Err("step_index_out_of_range".into());
        }
        plan.segments.remove(step_idx);
        if plan.segments.is_empty() {
            self.direct_unit_plans.remove(&sid);
            return Ok(());
        }
        self.rebuild_plan_waypoints(uid);
        Ok(())
    }

    pub fn reorder_task_queue(&mut self, uid: UnitId, from_idx: usize, to_idx: usize) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else {
            return Err("no_active_plan".into());
        };
        if from_idx >= plan.segments.len() || to_idx >= plan.segments.len() {
            return Err("step_index_out_of_range".into());
        }
        plan.segments.swap(from_idx, to_idx);
        let _ = plan;
        self.rebuild_plan_waypoints(uid);
        Ok(())
    }

    pub fn update_queued_segment(
        &mut self,
        uid: UnitId,
        seg_idx: usize,
        action: Option<&str>,
        move_mode: Option<&str>,
        fire_mode: Option<&str>,
        follow_target: Option<bool>,
        goal: Option<[f32; 2]>,
    ) -> Result<(), String> {
        self.update_queued_segment_ex(
            uid,
            seg_idx,
            action,
            move_mode,
            fire_mode,
            follow_target,
            goal,
            None,
            None,
        )
    }

    pub fn update_queued_segment_ex(
        &mut self,
        uid: UnitId,
        seg_idx: usize,
        action: Option<&str>,
        move_mode: Option<&str>,
        fire_mode: Option<&str>,
        follow_target: Option<bool>,
        goal: Option<[f32; 2]>,
        duration_sec: Option<u32>,
        priority_target: Option<&str>,
    ) -> Result<(), String> {
        if self.units.retreat_recovery_hours[uid.0] > 0.0 {
            return Err("retreat_recovery_uncontrollable".into());
        }
        let sid = self.units.str_id[uid.0].clone();
        let Some(plan) = self.direct_unit_plans.get_mut(&sid) else {
            return Err("no_active_plan".into());
        };
        let Some(seg) = plan.segments.get_mut(seg_idx) else {
            return Err("segment_index_out_of_range".into());
        };
        if let Some(a) = action {
            if matches!(a, "move" | "assault" | "attack" | "stop") {
                seg.action = a.to_string();
                if seg_idx == 0 { plan.action = a.to_string(); }
            }
        }
        if let Some(m) = move_mode {
            if matches!(m, "normal" | "fast" | "slow" | "stealth") {
                seg.move_mode = m.to_string();
                if seg_idx == 0 { plan.move_mode = m.to_string(); }
            }
        }
        if let Some(f) = fire_mode {
            seg.fire_mode = f.to_string();
            if seg_idx == 0 { plan.fire_mode = f.to_string(); }
        }
        if let Some(ft) = follow_target {
            seg.follow_target = ft;
            if seg_idx == 0 { plan.follow_target = ft; }
        }
        if let Some(d) = duration_sec {
            seg.duration_sec = d;
            if seg_idx == 0 { plan.duration_sec = d; }
        }
        if let Some(pt) = priority_target {
            seg.priority_target = pt.to_string();
            if seg_idx == 0 { plan.priority_target = pt.to_string(); }
        }
        let mut goal_changed = false;
        if let Some(g) = goal {
            seg.goal = g;
            goal_changed = true;
        }
        let _ = plan;
        if goal_changed {
            self.rebuild_plan_waypoints(uid);
        }
        Ok(())
    }

    pub fn format_unit_active_order_text(&self, uid: UnitId) -> String {
        let i = uid.0;
        let sid = &self.units.str_id[i];
        let name = &self.units.name[i];
        let cmdr_txt = match self.units.direct_commander_id[i] {
            Some(cid) => format!("上级指挥：{}", self.commanders.name[cid.0]),
            None => "无上级直属指挥官（独立行动）".to_string(),
        };
        let deploy_txt = if self.units.active_deploy_line_id[i] != 0 {
            format!("部署防线 ID=#{}", self.units.active_deploy_line_id[i])
        } else {
            "暂无展开战线".to_string()
        };
        if self.units.retreat_recovery_hours[i] > 0.0 {
            return format!(
                "【现行作战命令 · 紧急状态】\n单位：{name} ({sid})\n{cmdr_txt}\n状态：不可逆撤退恢复期（剩余 {:.0} 分钟），单位暂时脱离人工接管并全速向安全区脱离接触。",
                self.units.retreat_recovery_hours[i] * 60.0
            );
        }
        if let Some(p) = self.direct_unit_plans.get(sid) {
            let mm_cn = match p.move_mode.as_str() { "fast" => "快速机动（高暴露）", "slow" => "慢速渗透（低暴露）", _ => "正常行军" };
            let fm_cn = if p.fire_mode == "fire_on_the_move" { "移动射击" } else { "遇敌停下射击" };
            let act_cn = match p.action.as_str() {
                "move" => format!("机动推进 ({mm_cn})"),
                "assault" => format!("战术进攻 ({mm_cn} · {fm_cn})"),
                "attack" => format!("定点火力压制 ({})", if p.follow_target { "持续跟随目标" } else { "锁定坐标点" }),
                "retreat" => "全力撤退脱离接触".to_string(),
                _ => "原地待命戒备".to_string(),
            };
            let goal_txt = p.target_pos.map_or("当前位置".into(), |g| format!("坐标 ({:.1}, {:.1})", g[0], g[1]));
            let q_cnt = p.segments.len().saturating_sub(1);
            format!(
                "【现行作战命令 · 军令摘要】\n受令单位：{name} ({sid})\n{cmdr_txt} | {deploy_txt}\n主力任务：{act_cn}\n当前目标：{goal_txt}（当前段已锁定，后续队列可改航段：{q_cnt} 段）"
            )
        } else {
            format!(
                "【现行作战命令 · 自主态势】\n受令单位：{name} ({sid})\n{cmdr_txt} | {deploy_txt}\n当前动作：{}（随所属作战区 AO 与战线场自动维持警戒，未下达直接覆盖命令）",
                self.units.autonomy_state[i].as_str()
            )
        }
    }

    pub fn resolve_target_name(&self, target_id: &str) -> String {
        if let Some(&cid) = self.commanders.str_to_id.get(target_id) {
            return self.commanders.name[cid.0].clone();
        }
        if let Some(&uid) = self.units.str_to_id.get(target_id) {
            return self.units.name[uid.0].clone();
        }
        target_id.to_string()
    }

    pub fn set_direct_cmd_order(
        &mut self,
        cid: CommanderId,
        action: &str,
        goal: Option<[f32; 2]>,
        append: bool,
        target_unit_id: Option<String>,
    ) {
        let sid = self.commanders.str_id[cid.0].clone();
        let sname = self.commanders.name[cid.0].clone();
        if action == "stop" {
            self.direct_cmd_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "stop".into(),
                    waypoints: Vec::new(),
                    target_pos: None,
                    target_unit_id: None,
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    segments: Vec::new(),
                    duration_sec: 300,
                    priority_target: "any".into(),
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: String::new(),
                    weapon_model: "rifle".into(),
                    dispersion_m: 0.0,
                    hit_prob: 0.0,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            self.push_combat_log(DirectCombatLogEntry {
                turn: self.turn, clock_hours: self.clock_hours,
                actor_id: sid, actor_name: sname, target_id: None, target_name: None,
                kind: "order".into(), weapon_model: "rifle".into(), dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
                casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
                message: "下达【停止】指令，原地驻留。".into(),
            });
            return;
        }
        if action == "attack" {
            let tgt_pos = target_unit_id
                .as_deref()
                .and_then(|tid| self.resolve_target_pos_f32(tid))
                .or(goal);
            let Some(tp) = tgt_pos else { return; };
            let dist_m = self.grid.distance_km(self.commander_pos_f32(cid), tp) * 1000.0;
            let (disp_m, hit_prob) = Self::compute_fire_ballistics("rifle", dist_m, false);
            let seg = WaypointSegmentConfig {
                start_idx: 0,
                goal: tp,
                action: "attack".into(),
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: true,
                target_unit_id: target_unit_id.clone(),
                duration_sec: 16,
                priority_target: "any".into(),
            };
            if append {
                if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                    plan.segments.push(seg);
                    return;
                }
            }
            self.direct_cmd_plans.insert(
                sid.clone(),
                DirectOrderPlan {
                    action: "attack".into(),
                    waypoints: Vec::new(),
                    target_pos: Some(tp),
                    target_unit_id: target_unit_id.clone(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: true,
                    segments: vec![seg],
                    duration_sec: 16,
                    priority_target: "any".into(),
                    last_los_blocked: false,
                    out_of_ammo: false,
                    last_fire_summary: format!("准备射击 · 散布{:.0}m · 命中{:.0}%", disp_m, hit_prob * 100.0),
                    weapon_model: "rifle".into(),
                    dispersion_m: disp_m,
                    hit_prob,
                    last_casualties: 0,
                    last_org_loss: 0.0,
                    last_ammo_used_t: 0.0,
                },
            );
            return;
        }
        let Some(goal_pos) = goal else { return; };
        let cur_pos = self.commander_pos_f32(cid);
        let start = if append {
            self.direct_cmd_plans
                .get(&sid)
                .and_then(|p| p.waypoints.last().copied())
                .unwrap_or(cur_pos)
        } else {
            cur_pos
        };
        let mut seg = plan_grid_path_f32(&self.grid, start, goal_pos, MobilityClass::Wheeled);
        if seg.is_empty() && (start[0] - goal_pos[0]).hypot(start[1] - goal_pos[1]) > 0.05 {
            seg.push(goal_pos);
        }
        if append {
            if let Some(plan) = self.direct_cmd_plans.get_mut(&sid) {
                let start_idx = plan.waypoints.len();
                if plan.waypoints.is_empty() && plan.segments.is_empty() {
                    plan.action = action.to_string();
                    plan.waypoints = seg;
                    plan.target_pos = Some(goal_pos);
                }
                plan.segments.push(WaypointSegmentConfig {
                    start_idx,
                    goal: goal_pos,
                    action: action.into(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    target_unit_id: None,
                    duration_sec: 300,
                    priority_target: "any".into(),
                });
                return;
            }
        }
        self.direct_cmd_plans.insert(
            sid.clone(),
            DirectOrderPlan {
                action: action.to_string(),
                waypoints: seg,
                target_pos: Some(goal_pos),
                target_unit_id: None,
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: false,
                segments: vec![WaypointSegmentConfig {
                    start_idx: 0,
                    goal: goal_pos,
                    action: action.into(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: false,
                    target_unit_id: None,
                    duration_sec: 300,
                    priority_target: "any".into(),
                }],
                duration_sec: 300,
                priority_target: "any".into(),
                last_los_blocked: false,
                out_of_ammo: false,
                last_fire_summary: String::new(),
                weapon_model: "rifle".into(),
                dispersion_m: 0.0,
                hit_prob: 0.0,
                last_casualties: 0,
                last_org_loss: 0.0,
                last_ammo_used_t: 0.0,
            },
        );
        self.push_combat_log(DirectCombatLogEntry {
            turn: self.turn, clock_hours: self.clock_hours,
            actor_id: sid, actor_name: sname, target_id: None, target_name: None,
            kind: "order".into(), weapon_model: "rifle".into(), dist_m: 0.0, dispersion_m: 0.0, hit_prob: 0.0,
            casualties: 0, org_loss: 0.0, carry_loss_after: 0.0, ammo_used_t: 0.0,
            message: format!("下达【移动】指令 -> 目标 ({:.1}, {:.1})。", goal_pos[0], goal_pos[1]),
        });
    }

}
