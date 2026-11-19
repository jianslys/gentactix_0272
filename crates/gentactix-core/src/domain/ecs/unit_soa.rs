use super::*;
use crate::world_grid::{MobilityClass, TeamId};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct UnitSoA {
    pub id: Vec<UnitId>,
    pub str_id: Vec<String>,
    pub name: Vec<String>,
    pub team: Vec<TeamId>,
    pub alive: Vec<bool>,
    pub echelon: Vec<Echelon>,
    pub restricted_echelon: Vec<Echelon>,
    pub kind: Vec<String>,
    pub unit_mask: Vec<u8>,
    pub mobility: Vec<MobilityClass>,

    pub pos: Vec<[f32; 2]>,
    pub px: Vec<(i32, i32)>,
    pub facing_deg: Vec<f32>,
    pub tactical_face_vec: Vec<[f32; 2]>,
    pub moved_last_turn: Vec<bool>,
    pub footprint_px: Vec<u8>,

    // Four-ledger TOE: enlisted / nco / officer / equipment (cadre = nco + officer kept synced)
    pub enlisted: Vec<u32>,
    pub nco: Vec<u32>,
    pub officer: Vec<u32>,
    pub cadre: Vec<u32>,
    pub enlisted_full: Vec<u32>,
    pub nco_full: Vec<u32>,
    pub officer_full: Vec<u32>,
    pub cadre_full: Vec<u32>,
    pub org: Vec<f32>,
    pub org_cap: Vec<f32>,
    pub recovery_mult: Vec<f32>,
    pub empty_cadre: Vec<bool>,
    pub can_operate_radio: Vec<bool>,
    pub heavy_op_cap: Vec<f32>,
    pub is_acting_nco_cmd: Vec<bool>,
    pub retreat_recovery_hours: Vec<f32>,
    pub carry_loss: Vec<f32>,
    pub carry_attr: Vec<f32>,
    pub equipment: Vec<FxHashMap<String, u32>>,
    pub equipment_full: Vec<FxHashMap<String, u32>>,

    // Heavy weapon / vehicle state
    pub vehicle_model: Vec<Option<String>>,
    pub deployed_weapon: Vec<Option<String>>,
    pub unit_state: Vec<UnitState>,
    pub attached_to: Vec<Option<UnitId>>,
    pub operated_by: Vec<Option<UnitId>>,
    pub assigned_operator_count: Vec<u32>,
    pub assigned_operator_soldiers: Vec<Vec<String>>,
    pub overload_ratio: Vec<f32>,
    pub display_tier_bonus: Vec<i8>,

    // Comms & ROE
    pub radio_channel: Vec<u8>,
    pub radio_silence: Vec<bool>,
    pub control_radius_km: Vec<f32>,
    pub roe: Vec<RoeMode>,

    // Logistics stocks
    pub food_t: Vec<f32>,
    pub ammo_t: Vec<f32>,
    pub fuel_t: Vec<f32>,
    pub replacements: Vec<f32>,
    pub cap_food_t: Vec<f32>,
    pub cap_ammo_t: Vec<f32>,
    pub cap_fuel_t: Vec<f32>,

    // Strong-typed autonomy & hierarchy fields (replaced Python piece.meta!)
    pub direct_commander_id: Vec<Option<CommanderId>>,
    pub active_deploy_line_id: Vec<u32>,
    pub autonomy_state: Vec<AtomicAction>,
    pub autonomy_dir: Vec<[f32; 2]>,
    pub autonomy_deployed: Vec<bool>,
    pub exclamation_buff: Vec<AlertMark>,
    pub k_c: Vec<f32>,
    pub was_disconnected: Vec<bool>,
    pub alpha_stealth: Vec<f32>,
    pub exposed: Vec<bool>,
    pub delta_persons_5m: Vec<i32>,
    pub delta_org_5m: Vec<f32>,
    pub planned_route: Vec<Vec<(i32, i32)>>,
    pub reorganizing_event_id: Vec<Option<u64>>,
    pub modifier_slots: Vec<Vec<ModifierSlot>>,

    // Skin customization
    pub skin_color: Vec<String>,
    pub skin_badge: Vec<String>,
    pub skin_label: Vec<Option<String>>,

    // Individual soldier HP & carried ground bodies (wounded/KIA)
    pub soldiers: Vec<Vec<SoldierState>>,
    pub carried_bodies: Vec<Vec<GroundBody>>,
    pub task_scheduler: Vec<TaskTimingScheduler>,

    pub str_to_id: FxHashMap<String, UnitId>,
}

impl Default for UnitSoA {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitSoA {
    pub fn new() -> Self {
        Self {
            id: Vec::new(),
            str_id: Vec::new(),
            name: Vec::new(),
            team: Vec::new(),
            alive: Vec::new(),
            echelon: Vec::new(),
            restricted_echelon: Vec::new(),
            kind: Vec::new(),
            unit_mask: Vec::new(),
            mobility: Vec::new(),
            pos: Vec::new(),
            px: Vec::new(),
            facing_deg: Vec::new(),
            tactical_face_vec: Vec::new(),
            moved_last_turn: Vec::new(),
            footprint_px: Vec::new(),
            enlisted: Vec::new(),
            nco: Vec::new(),
            officer: Vec::new(),
            cadre: Vec::new(),
            enlisted_full: Vec::new(),
            nco_full: Vec::new(),
            officer_full: Vec::new(),
            cadre_full: Vec::new(),
            org: Vec::new(),
            org_cap: Vec::new(),
            recovery_mult: Vec::new(),
            empty_cadre: Vec::new(),
            can_operate_radio: Vec::new(),
            heavy_op_cap: Vec::new(),
            is_acting_nco_cmd: Vec::new(),
            retreat_recovery_hours: Vec::new(),
            carry_loss: Vec::new(),
            carry_attr: Vec::new(),
            equipment: Vec::new(),
            equipment_full: Vec::new(),
            vehicle_model: Vec::new(),
            deployed_weapon: Vec::new(),
            unit_state: Vec::new(),
            attached_to: Vec::new(),
            operated_by: Vec::new(),
            assigned_operator_count: Vec::new(),
            assigned_operator_soldiers: Vec::new(),
            overload_ratio: Vec::new(),
            display_tier_bonus: Vec::new(),
            radio_channel: Vec::new(),
            radio_silence: Vec::new(),
            control_radius_km: Vec::new(),
            roe: Vec::new(),
            food_t: Vec::new(),
            ammo_t: Vec::new(),
            fuel_t: Vec::new(),
            replacements: Vec::new(),
            cap_food_t: Vec::new(),
            cap_ammo_t: Vec::new(),
            cap_fuel_t: Vec::new(),
            direct_commander_id: Vec::new(),
            active_deploy_line_id: Vec::new(),
            autonomy_state: Vec::new(),
            autonomy_dir: Vec::new(),
            autonomy_deployed: Vec::new(),
            exclamation_buff: Vec::new(),
            k_c: Vec::new(),
            was_disconnected: Vec::new(),
            alpha_stealth: Vec::new(),
            exposed: Vec::new(),
            delta_persons_5m: Vec::new(),
            delta_org_5m: Vec::new(),
            planned_route: Vec::new(),
            reorganizing_event_id: Vec::new(),
            modifier_slots: Vec::new(),
            skin_color: Vec::new(),
            skin_badge: Vec::new(),
            skin_label: Vec::new(),
            soldiers: Vec::new(),
            carried_bodies: Vec::new(),
            task_scheduler: Vec::new(),
            str_to_id: FxHashMap::default(),
        }
    }

    pub fn spawn_unit(
        &mut self,
        str_id: impl Into<String>,
        name: impl Into<String>,
        team: TeamId,
        px: (i32, i32),
        echelon: Echelon,
        kind: &str,
        enlisted: u32,
        cadre: u32,
        equipment: FxHashMap<String, u32>,
        radio_channel: u8,
    ) -> UnitId {
        let (nco, officer) = infer_nco_officer(kind, echelon, cadre);
        self.spawn_unit_4ledger(str_id, name, team, px, echelon, kind, enlisted, nco, officer, equipment, radio_channel)
    }

    pub fn spawn_unit_4ledger(
        &mut self,
        str_id: impl Into<String>,
        name: impl Into<String>,
        team: TeamId,
        px: (i32, i32),
        echelon: Echelon,
        kind: &str,
        enlisted: u32,
        nco: u32,
        officer: u32,
        equipment: FxHashMap<String, u32>,
        radio_channel: u8,
    ) -> UnitId {
        let uid = UnitId(self.id.len());
        let s_id = str_id.into();
        let cadre = nco + officer;
        let eval = evaluate_counter_and_caps_4ledger(enlisted, nco, officer, &equipment, echelon);
        let mask = classify_unit_mask(kind, &equipment);
        let persons = (enlisted + cadre) as f32;
        let has_veh = equipment.iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle);
        let cap_f = (persons * 0.015).max(0.15);
        let cap_a = (persons * 0.03 + equipment.values().sum::<u32>() as f32 * 0.05).max(0.25);
        let cap_g = if has_veh { 1.2 } else { 0.05 };

        self.str_to_id.insert(s_id.clone(), uid);
        self.id.push(uid);
        self.str_id.push(s_id);
        self.name.push(name.into());
        self.team.push(team);
        self.alive.push(true);
        self.echelon.push(echelon);
        self.restricted_echelon.push(eval.restricted_echelon);
        self.kind.push(kind.to_string());
        self.unit_mask.push(mask);
        self.mobility.push(eval.mobility);
        self.pos.push([px.0 as f32, px.1 as f32]);
        self.px.push(px);
        self.facing_deg.push(match team {
            TeamId::Red => 90.0,
            TeamId::Blue => 270.0,
        });
        self.tactical_face_vec.push(match team {
            TeamId::Red => [0.0, 1.0],
            TeamId::Blue => [0.0, -1.0],
        });
        self.moved_last_turn.push(false);
        self.footprint_px.push(1);
        self.enlisted.push(enlisted);
        self.nco.push(nco);
        self.officer.push(officer);
        self.cadre.push(cadre);
        self.enlisted_full.push(enlisted);
        self.nco_full.push(nco);
        self.officer_full.push(officer);
        self.cadre_full.push(cadre);
        self.org.push(eval.org_cap_limit);
        self.org_cap.push(eval.org_cap_limit);
        self.recovery_mult.push(eval.recovery_rate_mult);
        self.empty_cadre.push(eval.empty_cadre);
        self.can_operate_radio.push(cadre > 0);
        let veh = equipment.iter().find(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle).map(|(m, _)| m.clone());
        let heavy_w = if equipment.len() == 1 {
            equipment.iter().find(|(m, &n)| n > 0 && equipment_spec(m).deployable).map(|(m, _)| m.clone())
        } else {
            None
        };
        self.heavy_op_cap.push(heavy_operator_efficiency_4ledger(
            nco,
            officer,
            enlisted + cadre,
            heavy_w.as_deref().unwrap_or("rifle"),
        ));
        self.is_acting_nco_cmd.push(officer == 0 && nco > 0);
        self.retreat_recovery_hours.push(0.0);
        self.carry_loss.push(0.0);
        self.carry_attr.push(0.0);
        self.equipment_full.push(equipment.clone());
        self.equipment.push(equipment);
        self.vehicle_model.push(veh);
        self.deployed_weapon.push(heavy_w);
        self.unit_state.push(UnitState::Mobile);
        self.attached_to.push(None);
        self.operated_by.push(None);
        self.assigned_operator_count.push(0);
        self.assigned_operator_soldiers.push(Vec::new());
        self.overload_ratio.push(1.0);
        self.display_tier_bonus.push(eval.display_tier_bonus);
        self.radio_channel.push(radio_channel);
        self.radio_silence.push(false);
        self.control_radius_km.push(1.5);
        self.roe.push(RoeMode::ReturnFire);
        self.food_t.push(cap_f);
        self.ammo_t.push(cap_a);
        self.fuel_t.push(cap_g);
        self.replacements.push(0.0);
        self.cap_food_t.push(cap_f);
        self.cap_ammo_t.push(cap_a);
        self.cap_fuel_t.push(cap_g);
        self.direct_commander_id.push(None);
        self.active_deploy_line_id.push(0);
        self.autonomy_state.push(AtomicAction::Hold);
        self.autonomy_dir.push([0.0, 0.0]);
        self.autonomy_deployed.push(false);
        self.exclamation_buff.push(AlertMark::None);
        self.k_c.push(1.20);
        self.was_disconnected.push(false);
        self.alpha_stealth.push(if kind == "recon_squad" { 0.85 } else { 0.35 });
        self.exposed.push(false);
        self.delta_persons_5m.push(0);
        self.delta_org_5m.push(0.0);
        self.planned_route.push(Vec::new());
        self.reorganizing_event_id.push(None);
        self.modifier_slots.push(Vec::new());
        self.skin_color.push(match team {
            TeamId::Red => "#ef4444".to_string(),
            TeamId::Blue => "#3b82f6".to_string(),
        });
        self.skin_badge.push("default".to_string());
        self.skin_label.push(None);
        let mut w_pool: Vec<String> = Vec::new();
        let mut eq_keys: Vec<String> = self.equipment[uid.0].keys().cloned().collect();
        eq_keys.sort();
        for m in eq_keys {
            let cnt = *self.equipment[uid.0].get(&m).unwrap_or(&0);
            if !equipment_spec(&m).is_vehicle {
                for _ in 0..cnt {
                    w_pool.push(m.clone());
                }
            }
        }
        let total_p = (enlisted + cadre).max(1) as f32;
        let per_sol_ammo = cap_a / total_p;
        let per_sol_food = cap_f / total_p;
        let mut sol_vec = Vec::with_capacity((enlisted + cadre) as usize);
        let mut idx_seq = 0usize;
        let build_items = |w: &str, ammo_q: f32, food_q: f32| -> Vec<SoldierItem> {
            let mut list = Vec::new();
            if w == "radio" {
                list.push(SoldierItem {
                    kind: "radio".to_string(),
                    model: "radio".to_string(),
                    name: "背负电台".to_string(),
                    count: 1,
                    ammo_t: 0.0,
                    slot: "pack".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 0, grid_y: 0,
                });
            } else if !w.is_empty() {
                let wname = match w {
                    "lmg" => "轻机枪",
                    "rifle" => "自动步枪",
                    "mortar_60" => "60mm迫击炮",
                    _ => w,
                };
                list.push(SoldierItem {
                    kind: "weapon".to_string(),
                    model: w.to_string(),
                    name: wname.to_string(),
                    count: 1,
                    ammo_t: 0.0,
                    slot: "hand".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 0, grid_y: 0,
                });
            }
            if ammo_q > 0.0 {
                list.push(SoldierItem {
                    kind: "ammo".to_string(),
                    model: "ammo".to_string(),
                    name: "随身弹药".to_string(),
                    count: 1,
                    ammo_t: ammo_q,
                    slot: "pack".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 1, grid_y: 0,
                });
            }
            if food_q > 0.0 {
                list.push(SoldierItem {
                    kind: "ration".to_string(),
                    model: "ration".to_string(),
                    name: "单兵口粮".to_string(),
                    count: 1,
                    ammo_t: food_q,
                    slot: "pack".to_string(),
                    weight_kg: 0.0, grid_w: 1, grid_h: 1, grid_x: 2, grid_y: 0,
                });
            }
            list
        };
        for o_i in 0..officer {
            let w = w_pool.get(idx_seq).cloned().unwrap_or_else(|| "rifle".to_string());
            idx_seq += 1;
            sol_vec.push(SoldierState {
                id: format!("{}-o{}", self.str_id[uid.0], o_i + 1),
                name: format!("军官 #{}", o_i + 1),
                hp: 100.0,
                max_hp: 100.0,
                status: "healthy".to_string(),
                weapon: w.clone(),
                ammo_t: per_sol_ammo,
                is_cadre: true,
                role: "officer".to_string(),
                items: build_items(&w, per_sol_ammo, per_sol_food),
                cannot_fire: false,
                cannot_operate_heavy: false,
                max_hand_weight_kg: None,
                max_pack_weight_kg: None,
                pack_cols: 8,
                pack_rows: 6,
            });
        }
        for c_i in 0..nco {
            let w = w_pool.get(idx_seq).cloned().unwrap_or_else(|| "rifle".to_string());
            idx_seq += 1;
            sol_vec.push(SoldierState {
                id: format!("{}-c{}", self.str_id[uid.0], c_i + 1),
                name: format!("士官 #{}", c_i + 1),
                hp: 100.0,
                max_hp: 100.0,
                status: "healthy".to_string(),
                weapon: w.clone(),
                ammo_t: per_sol_ammo,
                is_cadre: true,
                role: "nco".to_string(),
                items: build_items(&w, per_sol_ammo, per_sol_food),
                cannot_fire: false,
                cannot_operate_heavy: false,
                max_hand_weight_kg: None,
                max_pack_weight_kg: None,
                pack_cols: 8,
                pack_rows: 6,
            });
        }
        for e_i in 0..enlisted {
            let w = w_pool.get(idx_seq).cloned().unwrap_or_else(|| "rifle".to_string());
            idx_seq += 1;
            sol_vec.push(SoldierState {
                id: format!("{}-s{}", self.str_id[uid.0], e_i + 1),
                name: format!("士兵 #{}", e_i + 1),
                hp: 100.0,
                max_hp: 100.0,
                status: "healthy".to_string(),
                weapon: w.clone(),
                ammo_t: per_sol_ammo,
                is_cadre: false,
                role: "enlisted".to_string(),
                items: build_items(&w, per_sol_ammo, per_sol_food),
                cannot_fire: false,
                cannot_operate_heavy: false,
                max_hand_weight_kg: None,
                max_pack_weight_kg: None,
                pack_cols: 8,
                pack_rows: 6,
            });
        }
        self.soldiers.push(sol_vec);
        self.carried_bodies.push(Vec::new());
        self.task_scheduler.push(TaskTimingScheduler::new_for_echelon(echelon));
        self.sync_totals_from_soldiers(uid);
        self.refresh_caps(uid);
        uid
    }

    pub fn set_nco_officer(&mut self, id: UnitId, nco: u32, officer: u32) {
        let i = id.0;
        self.nco[i] = nco;
        self.officer[i] = officer;
        self.cadre[i] = nco + officer;
        // Synchronize soldier roles to match explicit 4-ledger edit
        let mut off_rem = officer;
        let mut nco_rem = nco;
        for s in &mut self.soldiers[i] {
            if s.status == "dead" {
                continue;
            }
            if off_rem > 0 {
                s.is_cadre = true;
                s.role = "officer".to_string();
                off_rem -= 1;
            } else if nco_rem > 0 {
                s.is_cadre = true;
                s.role = "nco".to_string();
                nco_rem -= 1;
            } else {
                s.is_cadre = false;
                s.role = "enlisted".to_string();
            }
        }
        self.refresh_caps(id);
    }

    pub fn sync_totals_from_soldiers(&mut self, id: UnitId) {
        let i = id.0;
        let mut new_eq: FxHashMap<String, u32> = FxHashMap::default();
        // 保留车辆、班组重武器、牵引炮与多人操作装备，防止单兵减员/随身装备覆盖时被意外清除
        for (m, &cnt) in &self.equipment[i] {
            let spec = equipment_spec(m);
            let is_crew_served = spec.is_vehicle || spec.is_heavy || spec.crew > 1 || spec.towable;
            if is_crew_served && cnt > 0 {
                new_eq.insert(m.clone(), cnt);
            }
        }
        let mut sum_ammo = 0.0f32;
        let mut sum_food = 0.0f32;
        for s in &mut self.soldiers[i] {
            if s.status == "dead" {
                continue;
            }
            let mut s_ammo = 0.0f32;
            let mut primary_w = String::new();
            for it in &s.items {
                if (it.kind == "weapon" || it.kind == "radio") && it.count > 0 && !it.model.is_empty() {
                    let spec = equipment_spec(&it.model);
                    let is_crew_served = spec.is_vehicle || spec.is_heavy || spec.crew > 1 || spec.towable;
                    if !is_crew_served {
                        *new_eq.entry(it.model.clone()).or_insert(0) += it.count;
                    }
                    if it.slot == "hand" {
                        primary_w = it.model.clone();
                    }
                } else if it.kind == "ammo" {
                    s_ammo += it.ammo_t.max(0.0);
                } else if it.kind == "ration" {
                    sum_food += it.ammo_t.max(0.0);
                }
            }
            if primary_w.is_empty() {
                if let Some(it) = s.items.iter().find(|it| it.kind == "weapon" && it.count > 0) {
                    primary_w = it.model.clone();
                } else if let Some(it) = s.items.iter().find(|it| it.kind == "radio" && it.count > 0) {
                    primary_w = it.model.clone();
                }
            }
            s.weapon = primary_w;
            s.ammo_t = s_ammo;
            s.refresh_hand_occupancy_flags();
            sum_ammo += s_ammo;
        }
        self.equipment[i] = new_eq;
        if !self.soldiers[i].is_empty() {
            self.ammo_t[i] = sum_ammo;
            self.food_t[i] = sum_food;
            self.officer[i] = self.soldiers[i].iter().filter(|s| s.role == "officer" && s.status != "dead").count() as u32;
            self.nco[i] = self.soldiers[i].iter().filter(|s| s.is_cadre && s.role != "officer" && s.status != "dead").count() as u32;
            self.cadre[i] = self.nco[i] + self.officer[i];
            self.enlisted[i] = self.soldiers[i].iter().filter(|s| !s.is_cadre && s.status != "dead").count() as u32;
        }
    }

    pub fn add_supply_reserving_soldiers(&mut self, id: UnitId, food_add: f32, ammo_add: f32, fuel_add: f32) {
        let i = id.0;
        let f_room = (self.cap_food_t[i] - self.food_t[i]).max(0.0);
        let a_room = (self.cap_ammo_t[i] - self.ammo_t[i]).max(0.0);
        let g_room = (self.cap_fuel_t[i] - self.fuel_t[i]).max(0.0);
        let df = food_add.max(0.0).min(f_room);
        let da = ammo_add.max(0.0).min(a_room);
        let dg = fuel_add.max(0.0).min(g_room);
        self.food_t[i] += df;
        self.ammo_t[i] += da;
        self.fuel_t[i] += dg;
        if da > 1e-6 && !self.soldiers[i].is_empty() {
            let live_n = self.soldiers[i].iter().filter(|s| s.status != "dead").count().max(1) as f32;
            let each_a = da / live_n;
            for s in &mut self.soldiers[i] {
                if s.status == "dead" {
                    continue;
                }
                s.ammo_t += each_a;
                if let Some(it) = s.items.iter_mut().find(|it| it.kind == "ammo") {
                    it.ammo_t += each_a;
                } else {
                    s.items.push(SoldierItem {
                        kind: "ammo".to_string(),
                        model: "ammo".to_string(),
                        name: "随身弹药".to_string(),
                        count: 1,
                        ammo_t: each_a,
                        slot: "pack".to_string(),
                        weight_kg: 0.0,
                        grid_w: 1,
                        grid_h: 1,
                        grid_x: 1,
                        grid_y: 0,
                    });
                }
            }
        }
    }

    pub fn fire_eligible_ratio(&self, id: UnitId) -> f32 {
        let sols = &self.soldiers[id.0];
        if sols.is_empty() {
            if let Some(op) = self.operated_by[id.0] {
                if op.0 < self.alive.len() && self.alive[op.0] && !self.soldiers[op.0].is_empty() {
                    let op_sols = &self.soldiers[op.0];
                    let ok = op_sols.iter().filter(|s| !s.cannot_fire && !s.cannot_operate_heavy).count();
                    return (ok as f32) / (op_sols.len() as f32);
                }
            }
            return if self.effective_operator_stats(id).0 > 0 { 1.0 } else { 0.0 };
        }
        let ok = sols.iter().filter(|s| !s.cannot_fire).count();
        (ok as f32) / (sols.len() as f32)
    }

    pub fn consume_soldier_ammo(&mut self, id: UnitId, amount: f32) -> f32 {
        let i = id.0;
        if amount <= 0.0 {
            return 0.0;
        }
        if self.soldiers[i].is_empty() {
            let avail = self.ammo_t[i].max(0.0);
            let take = amount.min(avail);
            self.ammo_t[i] = (avail - take).max(0.0);
            if take < amount && self.ammo_t[i] <= 1e-6 {
                if let Some(op) = self.operated_by[i] {
                    if op.0 < self.alive.len() && self.alive[op.0] && op != id {
                        return take + self.consume_soldier_ammo(op, amount - take);
                    }
                }
            }
            return take;
        }
        let tot: f32 = self.soldiers[i].iter().map(|s| s.ammo_t.max(0.0)).sum();
        if tot <= 1e-6 {
            self.ammo_t[i] = 0.0;
            return 0.0;
        }
        let take = amount.min(tot);
        let ratio = ((tot - take) / tot).clamp(0.0, 1.0);
        for s in &mut self.soldiers[i] {
            s.ammo_t = (s.ammo_t * ratio).max(0.0);
            for it in &mut s.items {
                if it.kind == "ammo" {
                    it.ammo_t = (it.ammo_t * ratio).max(0.0);
                }
            }
        }
        self.ammo_t[i] = self.soldiers[i].iter().map(|s| s.ammo_t).sum();
        take
    }

}
