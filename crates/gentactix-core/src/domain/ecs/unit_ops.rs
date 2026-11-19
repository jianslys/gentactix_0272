use super::*;
use crate::world_grid::{WorldGrid, LINE_MASK_ARMOR, LINE_MASK_ARTY_LOG, LINE_MASK_HEAVY_AT, LINE_MASK_INFANTRY, LINE_MASK_RECON};
use rustc_hash::FxHashMap;

impl UnitSoA {
    #[inline]
    pub fn persons(&self, id: UnitId) -> u32 {
        let i = id.0;
        self.enlisted[i] + self.cadre[i]
    }

    #[inline]
    pub fn is_directional_hull(&self, id: UnitId) -> bool {
        let i = id.0;
        self.vehicle_model[i].is_some()
            || self.equipment[i].iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle)
    }

    pub fn refresh_caps(&mut self, id: UnitId) {
        let i = id.0;
        let ledger_cadre = self.nco[i] + self.officer[i];
        if ledger_cadre != self.cadre[i] {
            if self.cadre[i] < ledger_cadre {
                let mut drop = ledger_cadre - self.cadre[i];
                let d_nco = drop.min(self.nco[i]);
                self.nco[i] -= d_nco;
                drop -= d_nco;
                self.officer[i] = self.officer[i].saturating_sub(drop);
            } else {
                self.nco[i] += self.cadre[i] - ledger_cadre;
            }
        }
        self.cadre[i] = self.nco[i] + self.officer[i];
        if self.kind[i] == "hq_squad" && self.cadre[i] == 0 {
            self.kind[i] = "infantry_squad".to_string();
            self.unit_mask[i] = classify_unit_mask(&self.kind[i], &self.equipment[i]);
        }
        let eval = evaluate_counter_and_caps_4ledger(
            self.enlisted[i],
            self.nco[i],
            self.officer[i],
            &self.equipment[i],
            self.echelon[i],
        );
        self.restricted_echelon[i] = eval.restricted_echelon;
        self.org_cap[i] = eval.org_cap_limit;
        self.org[i] = self.org[i].min(eval.org_cap_limit);
        self.empty_cadre[i] = eval.empty_cadre;
        self.recovery_mult[i] = eval.recovery_rate_mult;
        self.display_tier_bonus[i] = eval.display_tier_bonus;
        self.mobility[i] = self
            .vehicle_model[i]
            .as_deref()
            .and_then(vehicle_spec)
            .map(|vs| vs.mobility)
            .unwrap_or(eval.mobility);
        self.can_operate_radio[i] = self.cadre[i] > 0;
        let (eff_p, eff_nco, eff_off, _) = self.effective_operator_stats(id);
        self.heavy_op_cap[i] = heavy_operator_efficiency_4ledger(
            eff_nco,
            eff_off,
            eff_p,
            self.deployed_weapon[i].as_deref().unwrap_or("rifle"),
        );
        self.is_acting_nco_cmd[i] = self.officer[i] == 0 && self.nco[i] > 0;
    }

    /// Returns `(eff_persons, eff_nco, eff_officer, crew_buff_mult)`.
    /// Supports manned units as well as unmanned heavy emplacements/vehicles linked via `operated_by`.
    pub fn effective_operator_stats(&self, id: UnitId) -> (u32, u32, u32, f32) {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return (0, 0, 0, 0.0);
        }
        let own_p = self.persons(id);
        let main_w = self
            .deployed_weapon[i]
            .as_deref()
            .or_else(|| {
                self.equipment[i]
                    .iter()
                    .find(|(m, &c)| c > 0 && (equipment_spec(m).is_heavy || equipment_spec(m).deployable || equipment_spec(m).is_vehicle))
                    .map(|(m, _)| m.as_str())
            })
            .unwrap_or("rifle");
        if own_p > 0 {
            let buff = heavy_crew_buff_mult(own_p.min(4), main_w);
            return (own_p, self.nco[i], self.officer[i], buff);
        }
        if let Some(op) = self.operated_by[i] {
            if op.0 < self.alive.len() && self.alive[op.0] && self.persons(op) > 0 {
                let d_px = (self.pos[i][0] - self.pos[op.0][0]).hypot(self.pos[i][1] - self.pos[op.0][1]);
                if d_px <= 2.6 {
                    let mut crew = 0u32;
                    let mut nco_c = 0u32;
                    let mut off_c = 0u32;
                    if !self.assigned_operator_soldiers[i].is_empty() && !self.soldiers[op.0].is_empty() {
                        for s in &self.soldiers[op.0] {
                            if s.status != "dead"
                                && !s.cannot_fire
                                && !s.cannot_operate_heavy
                                && self.assigned_operator_soldiers[i].iter().any(|sid| sid == &s.id)
                            {
                                crew += 1;
                                if s.role == "officer" {
                                    off_c += 1;
                                } else if s.is_cadre {
                                    nco_c += 1;
                                }
                            }
                        }
                    }
                    if crew == 0 {
                        let req = if self.assigned_operator_count[i] > 0 {
                            self.assigned_operator_count[i]
                        } else {
                            equipment_spec(main_w).crew.max(2)
                        };
                        let avail = if self.soldiers[op.0].is_empty() {
                            self.persons(op)
                        } else {
                            self.soldiers[op.0]
                                .iter()
                                .filter(|s| s.status != "dead" && !s.cannot_fire && !s.cannot_operate_heavy)
                                .count() as u32
                        };
                        crew = req.min(avail);
                        nco_c = self.nco[op.0].min(crew);
                        off_c = self.officer[op.0].min(crew.saturating_sub(nco_c));
                    }
                    let buff = heavy_crew_buff_mult(crew, main_w);
                    return (crew, nco_c, off_c, buff);
                }
            }
        }
        (0, 0, 0, 0.0)
    }

    pub fn set_heavy_operators(&mut self, id: UnitId, op_uid: Option<UnitId>, soldier_ids: Vec<String>, op_count: u32) -> Result<(), String> {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return Err("target_unit_dead".into());
        }
        if let Some(op) = op_uid {
            if op.0 >= self.alive.len() || !self.alive[op.0] || self.team[op.0] != self.team[i] || self.persons(op) == 0 {
                return Err("invalid_operator_unit".into());
            }
            let d_px = (self.pos[i][0] - self.pos[op.0][0]).hypot(self.pos[i][1] - self.pos[op.0][1]);
            if d_px > 2.6 {
                return Err("operator_unit_too_far".into());
            }
        }
        self.operated_by[i] = op_uid;
        self.assigned_operator_soldiers[i] = soldier_ids;
        self.assigned_operator_count[i] = op_count;
        self.refresh_caps(id);
        Ok(())
    }

    pub fn unit_has_engineer_cap(&self, id: UnitId) -> bool {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return false;
        }
        let kl = self.kind[i].to_ascii_lowercase();
        let sl = self.str_id[i].to_ascii_lowercase();
        if kl.contains("eng")
            || kl.contains("sapper")
            || kl.contains("pion")
            || sl.contains("eng")
            || sl.contains("pion")
            || self.name[i].contains("工兵")
            || self.name[i].contains("爆破")
        {
            return true;
        }
        for (m, &cnt) in &self.equipment[i] {
            if cnt == 0 {
                continue;
            }
            let ml = m.to_ascii_lowercase();
            if matches!(
                ml.as_str(),
                "explosives" | "satchel" | "mine_detector" | "bridge_kit" | "flamethrower" | "wpn_flammenwerfer_35"
            ) || ml.contains("engineer")
                || ml.contains("mine")
                || ml.contains("explosive")
            {
                return true;
            }
        }
        self.soldiers[i].iter().any(|s| {
            if s.status == "dead" {
                return false;
            }
            let rl = s.role.to_ascii_lowercase();
            rl.contains("eng")
                || rl.contains("sapper")
                || rl.contains("pion")
                || s.items.iter().any(|it| {
                    it.count > 0
                        && matches!(
                            it.model.as_str(),
                            "explosives" | "satchel" | "mine_detector" | "bridge_kit" | "flamethrower" | "wpn_flammenwerfer_35"
                        )
                })
        })
    }

    pub fn effective_speed_kmh(
        &self,
        id: UnitId,
        grid: &WorldGrid,
        r0: i32,
        c0: i32,
        r1: i32,
        c1: i32,
        action: &str,
        move_mode: &str,
    ) -> f32 {
        let i = id.0;
        if i >= self.alive.len() || !self.alive[i] {
            return 0.0;
        }
        if self.unit_state[i] == UnitState::Deployed {
            return 0.0;
        }
        let (eff_persons, _, _, _) = self.effective_operator_stats(id);
        let mob = self.mobility[i];
        let vspec = self
            .vehicle_model[i]
            .as_deref()
            .and_then(vehicle_spec)
            .or_else(|| {
                self.equipment[i]
                    .iter()
                    .filter(|(_, &c)| c > 0)
                    .filter_map(|(m, _)| vehicle_spec(m))
                    .min_by(|a, b| a.base_speed_kmh.total_cmp(&b.base_speed_kmh))
            });
        let mut base_kmh = if let Some(vs) = vspec {
            if eff_persons == 0 {
                return 0.0;
            }
            let crew_mult = (eff_persons as f32 / vs.min_crew.max(1) as f32).clamp(0.45, 1.0);
            vs.base_speed_kmh * crew_mult
        } else {
            if eff_persons == 0 {
                return 0.0;
            }
            let mut foot_spd = if self.kind[i].contains("recon") { 4.3 } else { 3.8 };
            let heavy_drag_cnt: u32 = self.equipment[i]
                .iter()
                .filter(|(m, &c)| c > 0 && {
                    let s = equipment_spec(m);
                    s.is_heavy || s.towable
                })
                .map(|(_, &c)| c)
                .sum();
            if heavy_drag_cnt > 0 {
                foot_spd *= 0.80f32.powi(heavy_drag_cnt.min(3) as i32).max(0.52);
            }
            foot_spd
        };

        let i1 = grid.idx(r1.clamp(0, (grid.rows as i32) - 1), c1.clamp(0, (grid.cols as i32) - 1));
        let t1 = grid.terrain[i1];
        let tc = crate::world_grid::terrain_cost(mob, t1);
        if !tc.is_finite() {
            return 0.0;
        }
        let cm1 = grid.get_cell_modifiers(r1, c1, mob);
        if cm1.impassable {
            return 0.0;
        }
        if !cm1.ignore_slope_limit && grid.slope_deg[i1] > mob.slope_limit_deg() {
            return 0.0;
        }
        let w1 = (grid.weather[i1] as usize).min(5);
        let w_mult = if cm1.is_road {
            crate::world_grid::WEATHER_ROAD[w1]
        } else {
            crate::world_grid::WEATHER_OFFROAD[w1]
        };
        let horiz_m = (((r1 - r0) as f32).hypot((c1 - c0) as f32) * grid.cell_m).max(1.0);
        let i0 = grid.idx(r0.clamp(0, (grid.rows as i32) - 1), c0.clamp(0, (grid.cols as i32) - 1));
        let dz = grid.elevation_m[i1] - grid.elevation_m[i0];
        let grade = dz / horiz_m;
        let slope_mult = if grade >= 0.0 {
            1.0 + mob.uphill_k() * grade
        } else {
            (1.0 + 0.45 * grade).max(0.85)
        };
        let overload_mult = (1.0 - self.carry_loss[i] * 0.45).clamp(0.45, 1.0);
        let mode_mult = match move_mode {
            "fast" => 1.25,
            "slow" | "stealth" => 0.55,
            _ => 1.0,
        };
        let act_mult = match action {
            "retreat" => 1.30,
            "assault" => 0.85,
            _ => 1.0,
        };
        base_kmh = (base_kmh * cm1.speed_mult * overload_mult * mode_mult * act_mult)
            / (tc.max(0.25) * w_mult * slope_mult);
        if let Some(vs) = vspec {
            let cap = if move_mode == "fast" || action == "retreat" {
                vs.road_max_kmh * 1.15
            } else {
                vs.road_max_kmh
            };
            base_kmh = base_kmh.min(cap);
        }
        base_kmh.max(0.2)
    }

    pub fn unit_power(&self, id: UnitId, dist_m: f32, has_los: bool, only_weapon: Option<&str>, acc_mult: f32) -> [f32; 6] {
        let mut totals = [0.0f32; 6];
        let i = id.0;
        let (eff_persons, eff_nco, eff_officer, _) = self.effective_operator_stats(id);
        if eff_persons == 0 || !self.alive[i] {
            return totals;
        }
        let base_org = if self.persons(id) > 0 {
            self.org[i]
        } else if let Some(op) = self.operated_by[i] {
            self.org[op.0]
        } else {
            self.org[i]
        };
        let org_mult = (0.3 + 0.7 * (base_org / 100.0)).clamp(0.15, 1.3);
        let mut needed_small_crew = 0u32;
        let mut has_small_arms = false;
        for (model, &cnt) in &self.equipment[i] {
            if cnt == 0 {
                continue;
            }
            if let Some(ow) = only_weapon {
                if model != ow {
                    continue;
                }
            }
            let spec = equipment_spec(model);
            if !spec.indirect && !has_los {
                continue;
            }
            let acc = (weapon_accuracy_decay(dist_m, spec.range_m) * acc_mult).min(1.5);
            let dmg_dec = weapon_damage_decay(model, dist_m, spec.range_m);
            let is_crew_wpn = spec.is_heavy || spec.deployable || spec.crew >= 2;
            let crew_scale = if is_crew_wpn {
                let op_eff = heavy_operator_efficiency_4ledger(eff_nco, eff_officer, eff_persons, model);
                let buff = heavy_crew_buff_mult(eff_persons.min(4), model);
                op_eff * buff
            } else {
                has_small_arms = true;
                needed_small_crew += spec.crew.max(1) * cnt;
                1.0
            };
            let scale = cnt as f32 * acc * dmg_dec * org_mult * crew_scale;
            for d in 0..6 {
                totals[d] += spec.fire[d] * scale;
            }
        }
        if has_small_arms && needed_small_crew > 0 && eff_persons < needed_small_crew {
            let small_manning = (eff_persons as f32 / needed_small_crew as f32).clamp(0.1, 1.0);
            // If unit has only small arms, scale whole total; if mixed, scaling conservatively preserves crew buff on heavy weapons.
            let has_only_small = !self.equipment[i].iter().any(|(m, &c)| c > 0 && {
                let sp = equipment_spec(m);
                sp.is_heavy || sp.deployable || sp.crew >= 2
            });
            if has_only_small {
                for d in 0..6 {
                    totals[d] *= small_manning;
                }
            }
        }
        totals
    }

    pub fn set_counter_skin(
        &mut self,
        id: UnitId,
        color: Option<&str>,
        badge: Option<&str>,
        custom_label: Option<&str>,
        attempt_level: Option<&str>,
    ) -> Result<(), String> {
        let i = id.0;
        if let Some(lvl) = attempt_level {
            if Echelon::from_str(lvl) != self.restricted_echelon[i] {
                return Err("forbidden_counter_echelon_tamper".into());
            }
        }
        if let Some(c) = color {
            self.skin_color[i] = c.to_string();
        }
        if let Some(b) = badge {
            self.skin_badge[i] = b.to_string();
        }
        if let Some(lbl) = custom_label {
            self.skin_label[i] = if lbl.is_empty() { None } else { Some(lbl.to_string()) };
        }
        Ok(())
    }

    pub fn split_unit(&mut self, src: UnitId, take_enlisted: u32, take_cadre: u32, new_str_id: &str) -> Result<UnitId, String> {
        let i = src.0;
        if !self.alive[i] {
            return Err("source_dead".into());
        }
        let te = take_enlisted.min(self.enlisted[i]);
        let tc = take_cadre.min(self.cadre[i]);
        if te + tc == 0 {
            return Err("empty_split".into());
        }
        if self.enlisted[i] - te + self.cadre[i] - tc == 0 {
            return Err("cannot_empty_source".into());
        }
        let total_before = self.persons(src).max(1) as f32;
        self.enlisted[i] -= te;
        let take_nco = tc.min(self.nco[i]);
        let take_off = (tc - take_nco).min(self.officer[i]);
        self.nco[i] -= take_nco;
        self.officer[i] -= take_off;
        self.cadre[i] = self.nco[i] + self.officer[i];

        let mut split_eq = FxHashMap::default();
        if let Some(&r_cnt) = self.equipment[i].get("rifle") {
            let take_r = (r_cnt / 2).min(te + tc);
            if take_r > 0 {
                self.equipment[i].insert("rifle".to_string(), r_cnt - take_r);
                split_eq.insert("rifle".to_string(), take_r);
            }
        }

        let frac = (te + tc) as f32 / total_before;
        let f_take = self.food_t[i] * frac;
        let a_take = self.ammo_t[i] * frac;
        let g_take = self.fuel_t[i] * frac;
        self.food_t[i] -= f_take;
        self.ammo_t[i] -= a_take;
        self.fuel_t[i] -= g_take;
        self.refresh_caps(src);

        let mut moved_soldiers: Vec<SoldierState> = Vec::with_capacity((te + tc) as usize);
        if !self.soldiers[i].is_empty() {
            let mut rem_off = take_off;
            let mut rem_nco = take_nco;
            let mut rem_enl = te;
            let mut kept = Vec::with_capacity(self.soldiers[i].len());
            for mut s in self.soldiers[i].drain(..) {
                if s.status != "dead" {
                    if s.role == "officer" && rem_off > 0 {
                        rem_off -= 1;
                        s.id = format!("{}-o{}", new_str_id, take_off - rem_off);
                        moved_soldiers.push(s);
                        continue;
                    }
                    if s.is_cadre && s.role != "officer" && rem_nco > 0 {
                        rem_nco -= 1;
                        s.id = format!("{}-c{}", new_str_id, take_nco - rem_nco);
                        moved_soldiers.push(s);
                        continue;
                    }
                    if !s.is_cadre && rem_enl > 0 {
                        rem_enl -= 1;
                        s.id = format!("{}-e{}", new_str_id, te - rem_enl);
                        moved_soldiers.push(s);
                        continue;
                    }
                }
                kept.push(s);
            }
            self.soldiers[i] = kept;
        }

        let team = self.team[i];
        let px = self.px[i];
        let pos = [self.pos[i][0] + 0.08, self.pos[i][1] + 0.08];
        let kind = self.kind[i].clone();
        let ch = self.radio_channel[i];
        let cmdr = self.direct_commander_id[i];
        let org_val = self.org[i];
        let name = format!("{}-分", self.name[i]);
        let child = self.spawn_unit_4ledger(new_str_id, name, team, px, Echelon::Squad, &kind, te, take_nco, take_off, split_eq, ch);
        if !moved_soldiers.is_empty() {
            self.soldiers[child.0] = moved_soldiers;
        }
        self.sync_totals_from_soldiers(src);
        self.sync_totals_from_soldiers(child);
        self.pos[child.0] = pos;
        self.direct_commander_id[child.0] = cmdr;
        self.org[child.0] = org_val.min(self.org_cap[child.0]);
        self.food_t[child.0] = f_take;
        self.ammo_t[child.0] = a_take;
        self.fuel_t[child.0] = g_take;
        Ok(child)
    }

    pub fn merge_squads_direct(&mut self, dst: UnitId, src: UnitId) -> Result<u32, String> {
        let di = dst.0;
        let si = src.0;
        if di == si || !self.alive[di] || !self.alive[si] {
            return Err("invalid_merge_units".into());
        }
        if self.team[di] != self.team[si] {
            return Err("cross_team_merge_forbidden".into());
        }
        if (self.echelon[di] as u8) > (Echelon::Squad as u8) || (self.echelon[si] as u8) > (Echelon::Squad as u8) {
            return Err("platoon_merge_requires_reorganize_event".into());
        }
        let total = self.persons(dst) + self.persons(src);
        if total > DIRECT_MERGE_MAX_PERSONS {
            return Err(format!("exceeds_squad_direct_merge_cap:{total}>15"));
        }
        let p_dst = self.persons(dst) as f32;
        let p_src = self.persons(src) as f32;
        let w_org = (self.org[di] * p_dst + self.org[si] * p_src) / (p_dst + p_src).max(1.0);

        self.enlisted[di] += self.enlisted[si];
        self.nco[di] += self.nco[si];
        self.officer[di] += self.officer[si];
        self.cadre[di] = self.nco[di] + self.officer[di];
        self.enlisted_full[di] += self.enlisted_full[si];
        self.nco_full[di] += self.nco_full[si];
        self.officer_full[di] += self.officer_full[si];
        self.cadre_full[di] = self.nco_full[di] + self.officer_full[di];
        let src_eq = std::mem::take(&mut self.equipment[si]);
        for (k, v) in src_eq {
            *self.equipment[di].entry(k.clone()).or_insert(0) += v;
            *self.equipment_full[di].entry(k).or_insert(0) += v;
        }
        self.food_t[di] += self.food_t[si];
        self.ammo_t[di] += self.ammo_t[si];
        self.fuel_t[di] += self.fuel_t[si];
        self.replacements[di] += self.replacements[si];

        if !self.soldiers[si].is_empty() {
            let mut moved = std::mem::take(&mut self.soldiers[si]);
            self.soldiers[di].append(&mut moved);
            self.sync_totals_from_soldiers(dst);
        }

        self.enlisted[si] = 0;
        self.nco[si] = 0;
        self.officer[si] = 0;
        self.cadre[si] = 0;
        self.alive[si] = false;
        self.refresh_caps(dst);
        self.org[di] = w_org.min(self.org_cap[di]);
        Ok(total)
    }

    pub fn effective_locks(&self, id: UnitId, events: &[ActiveEvent]) -> LockFlags {
        let mut locks = LockFlags::empty();
        for s in &self.modifier_slots[id.0] {
            locks |= s.locks;
        }
        if self.unit_state[id.0] == UnitState::Deployed {
            locks.insert(LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE);
        }
        if self.reorganizing_event_id[id.0].is_some() {
            locks |= LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE | LockFlags::FORBID_REST | LockFlags::FORBID_DEPLOY;
        }
        for ev in events {
            if ev.actor_id == id {
                locks |= ev.kind.locks();
            } else if let EventKind::ReorganizeEchelon {
                commander_piece_id,
                source_unit_ids,
                ..
            } = &ev.kind
            {
                if *commander_piece_id == Some(id) || source_unit_ids.contains(&id) {
                    locks |= ev.kind.locks();
                }
            }
        }
        locks.normalized()
    }
}


pub fn classify_unit_mask(kind: &str, equipment: &FxHashMap<String, u32>) -> u8 {
    let kl = kind.to_ascii_lowercase();
    if kl.contains("tank") || kl.contains("armor") || equipment.get("tank_medium").copied().unwrap_or(0) > 0 || equipment.get("tank_heavy").copied().unwrap_or(0) > 0 {
        return LINE_MASK_ARMOR;
    }
    if kl.contains("recon") {
        return LINE_MASK_RECON;
    }
    if kl.contains("howitzer") || kl.contains("arty") || kl.contains("porter") || kl.contains("transport") || kl.contains("hq") {
        return LINE_MASK_ARTY_LOG;
    }
    if kl.contains("mg") || kl.contains("at") || kl.contains("mortar") || equipment.get("hmg").copied().unwrap_or(0) > 0 || equipment.get("at_gun_50").copied().unwrap_or(0) > 0 {
        return LINE_MASK_HEAVY_AT;
    }
    LINE_MASK_INFANTRY
}

pub fn heavy_operator_efficiency_4ledger(nco: u32, officer: u32, persons: u32, weapon: &str) -> f32 {
    if persons == 0 {
        return 0.0;
    }
    let req = equipment_spec(weapon).crew.max(1);
    let man = (persons as f32 / req as f32).clamp(0.1, 1.0);
    // 士官(NCO)专职管重武器与现场操炮效能；无士官有军官则有轻微折损，全无骨干大幅折损
    let cadre_bonus = if nco >= 2 {
        1.0
    } else if nco == 1 {
        0.90
    } else if officer > 0 {
        0.75
    } else {
        0.55
    };
    man * cadre_bonus
}

pub fn heavy_operator_efficiency(cadre: u32, persons: u32, weapon: &str) -> f32 {
    heavy_operator_efficiency_4ledger(cadre, 0, persons, weapon)
}
