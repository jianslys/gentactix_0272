use super::*;
use crate::world_grid::{TeamId, WorldGrid};
use rustc_hash::FxHashMap;

#[derive(Debug, Default)]
pub struct EventQueue {
    pub next_id: u64,
    pub events: Vec<ActiveEvent>,
}

impl EventQueue {
    pub fn push(&mut self, actor_id: UnitId, duration_h: f32, labor_persons: u32, kind: EventKind) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.events.push(ActiveEvent {
            id,
            actor_id,
            remaining_hours: duration_h,
            labor_persons,
            kind,
        });
        id
    }

    pub fn start_reorganize_echelon(
        &mut self,
        units: &mut UnitSoA,
        commanders: &CommanderSoA,
        commander_id: CommanderId,
        commander_piece_id: Option<UnitId>,
        target_echelon: Echelon,
        source_unit_ids: Vec<UnitId>,
        target_specs: Vec<ReorgTargetSpec>,
        strict_cadre_allocation: bool,
        duration_h: Option<f32>,
    ) -> Result<u64, String> {
        if !commanders.alive[commander_id.0] {
            return Err("commander_unavailable".into());
        }
        let team = commanders.team[commander_id.0];
        let valid_srcs: Vec<UnitId> = source_unit_ids
            .into_iter()
            .filter(|&u| u.0 < units.id.len() && units.alive[u.0] && units.team[u.0] == team)
            .collect();
        if valid_srcs.is_empty() {
            return Err("no_source_units".into());
        }
        let cmd_piece = commander_piece_id.or(commanders.hq_piece_id[commander_id.0]);
        let actor = cmd_piece.unwrap_or(valid_srcs[0]);
        let labor: u32 = valid_srcs.iter().map(|&u| units.persons(u)).sum();
        let dur = duration_h.unwrap_or_else(|| target_echelon.reorg_duration_h());
        let eid = self.push(
            actor,
            dur,
            labor,
            EventKind::ReorganizeEchelon {
                commander_id,
                commander_piece_id: cmd_piece,
                target_echelon,
                source_unit_ids: valid_srcs.clone(),
                target_specs,
                strict_cadre_allocation,
            },
        );
        for u in valid_srcs {
            units.reorganizing_event_id[u.0] = Some(eid);
        }
        if let Some(cp) = cmd_piece {
            units.reorganizing_event_id[cp.0] = Some(eid);
        }
        Ok(eid)
    }

    /// Any attack on the highest commanding unit aborts reorganization immediately!
    pub fn notify_unit_attacked(
        &mut self,
        units: &mut UnitSoA,
        commanders: &CommanderSoA,
        attacked_unit: Option<UnitId>,
        attacker_team: TeamId,
        cell_rc: Option<(i32, i32)>,
    ) -> Vec<u64> {
        let mut aborted = Vec::new();
        self.events.retain(|ev| {
            if let EventKind::ReorganizeEchelon {
                commander_id,
                commander_piece_id,
                source_unit_ids,
                ..
            } = &ev.kind
            {
                let c_team = commanders.team[commander_id.0];
                if c_team == attacker_team {
                    return true;
                }
                let hit = match attacked_unit {
                    Some(uid) if *commander_piece_id == Some(uid) => true,
                    _ => {
                        if let Some(rc) = cell_rc {
                            commander_piece_id.is_none() && commanders.px[commander_id.0] == rc
                        } else {
                            false
                        }
                    }
                };
                if hit {
                    aborted.push(ev.id);
                    for &su in source_unit_ids {
                        units.reorganizing_event_id[su.0] = None;
                    }
                    if let Some(cp) = commander_piece_id {
                        units.reorganizing_event_id[cp.0] = None;
                    }
                    return false;
                }
            }
            true
        });
        aborted
    }

    pub fn start_form_command_squad(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        preferred_name: String,
        dissolve_if_empty: bool,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        if (units.nco[actor_id.0] + units.officer[actor_id.0]) == 0 {
            return Err("requires_nco_or_officer_cadre".into());
        }
        if units.kind[actor_id.0] == "hq_squad" {
            return Err("already_command_squad".into());
        }
        let labor = units.persons(actor_id);
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            labor,
            EventKind::FormCommandSquad {
                preferred_name,
                dissolve_if_empty,
            },
        ))
    }

    pub fn start_lay_wire(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_px: (i32, i32),
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        Ok(self.push(actor_id, duration_h.max(0.05), units.persons(actor_id), EventKind::LayWire { target_px }))
    }

    pub fn start_cargo_transfer(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_unit_id: UnitId,
        food_t: f32,
        ammo_t: f32,
        fuel_t: f32,
        equipment: Vec<(String, u32)>,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len()
            || target_unit_id.0 >= units.id.len()
            || !units.alive[actor_id.0]
            || !units.alive[target_unit_id.0]
        {
            return Err("invalid_cargo_transfer_units".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::CargoTransfer {
                target_unit_id,
                food_t,
                ammo_t,
                fuel_t,
                equipment,
            },
        ))
    }

    pub fn start_engineering(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        op: impl Into<String>,
        target_px: (i32, i32),
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        let op_s = op.into();
        let (ax, ay) = units.px[actor_id.0];
        let cheb = (target_px.0 - ax).abs().max((target_px.1 - ay).abs());
        if cheb > 1 {
            return Err("target_out_of_engineering_reach".into());
        }
        if op_s != "fortify" && !units.unit_has_engineer_cap(actor_id) {
            return Err("engineer_capability_required".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::Engineering {
                op: op_s,
                target_px,
            },
        ))
    }

    pub fn start_demolition(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_px: (i32, i32),
        target_type: impl Into<String>,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        let (ax, ay) = units.px[actor_id.0];
        let cheb = (target_px.0 - ax).abs().max((target_px.1 - ay).abs());
        if cheb > 1 {
            return Err("target_out_of_demolition_reach".into());
        }
        if !units.unit_has_engineer_cap(actor_id) {
            return Err("engineer_capability_required".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::Demolition {
                target_px,
                target_type: target_type.into(),
            },
        ))
    }

    pub fn cancel_event(
        &mut self,
        units: &mut UnitSoA,
        event_id: u64,
        owner_team: TeamId,
    ) -> Result<(), String> {
        let Some(pos) = self.events.iter().position(|ev| ev.id == event_id) else {
            return Err("event_not_found".into());
        };
        let actor = self.events[pos].actor_id;
        if actor.0 < units.team.len() && units.team[actor.0] != owner_team {
            return Err("forbidden_cross_team_event_cancel".into());
        }
        let ev = self.events.remove(pos);
        if let EventKind::ReorganizeEchelon {
            commander_piece_id,
            source_unit_ids,
            ..
        } = ev.kind
        {
            for su in source_unit_ids {
                if su.0 < units.reorganizing_event_id.len() {
                    units.reorganizing_event_id[su.0] = None;
                }
            }
            if let Some(cp) = commander_piece_id {
                if cp.0 < units.reorganizing_event_id.len() {
                    units.reorganizing_event_id[cp.0] = None;
                }
            }
        }
        Ok(())
    }

    pub fn start_radio_decoy(
        &mut self,
        units: &UnitSoA,
        actor_id: UnitId,
        target_px: (i32, i32),
        ttl_h: f32,
        duration_h: f32,
    ) -> Result<u64, String> {
        if actor_id.0 >= units.id.len() || !units.alive[actor_id.0] {
            return Err("unit_dead".into());
        }
        Ok(self.push(
            actor_id,
            duration_h.max(0.05),
            units.persons(actor_id),
            EventKind::RadioDecoy { target_px, ttl_h },
        ))
    }
}

pub fn complete_reorganize_echelon(
    grid: &WorldGrid,
    units: &mut UnitSoA,
    commanders: &mut CommanderSoA,
    commander_id: CommanderId,
    commander_piece_id: Option<UnitId>,
    target_echelon: Echelon,
    source_unit_ids: &[UnitId],
    target_specs: &[ReorgTargetSpec],
    strict_cadre_allocation: bool,
) -> Vec<UnitId> {
    let team = commanders.team[commander_id.0];
    let anchor = commander_piece_id
        .map(|u| units.px[u.0])
        .unwrap_or(commanders.px[commander_id.0]);
    let spawn_px = grid.find_nearest_literal_ground_px(anchor.0, anchor.1);

    let mut pool_enl = 0u32;
    let mut pool_nco = 0u32;
    let mut pool_off = 0u32;
    let mut pool_eq: FxHashMap<String, u32> = FxHashMap::default();
    let mut pool_food = 0.0f32;
    let mut pool_ammo = 0.0f32;
    let mut pool_fuel = 0.0f32;
    let mut org_num = 0.0f32;
    let mut org_den = 0.0f32;
    let mut ch = commanders.radio_channel[commander_id.0];

    for &uid in source_unit_ids {
        let i = uid.0;
        if !units.alive[i] {
            continue;
        }
        units.reorganizing_event_id[i] = None;
        ch = units.radio_channel[i];
        let p = units.persons(uid).max(1) as f32;
        pool_enl += units.enlisted[i];
        pool_nco += units.nco[i];
        pool_off += units.officer[i];
        org_num += units.org[i] * p;
        org_den += p;
        for (m, &n) in &units.equipment[i] {
            *pool_eq.entry(m.clone()).or_insert(0) += n;
        }
        pool_food += units.food_t[i];
        pool_ammo += units.ammo_t[i];
        pool_fuel += units.fuel_t[i];

        units.enlisted[i] = 0;
        units.nco[i] = 0;
        units.officer[i] = 0;
        units.cadre[i] = 0;
        units.alive[i] = false;
        commanders.subordinate_unit_ids[commander_id.0].retain(|&x| x != uid);
    }
    if let Some(cp) = commander_piece_id {
        units.reorganizing_event_id[cp.0] = None;
    }

    let mean_org = org_num / org_den.max(1.0);
    let pool_cad = pool_nco + pool_off;
    let specs_owned: Vec<ReorgTargetSpec>;
    let specs_ref = if target_specs.is_empty() {
        let total = pool_enl + pool_cad;
        let eq_vec: Vec<(String, u32)> = pool_eq.iter().map(|(k, &v)| (k.clone(), v)).collect();
        specs_owned = vec![ReorgTargetSpec {
            str_id: format!("{}-{}-reorg", commanders.str_id[commander_id.0], target_echelon.as_str()),
            name: format!("整编{}", target_echelon.as_str()),
            echelon: target_echelon,
            enlisted: pool_enl.max(if total == 0 { 0 } else { 0 }),
            cadre: pool_cad,
            equipment: eq_vec,
        }];
        &specs_owned
    } else {
        target_specs
    };

    let mut rem_e = pool_enl;
    let mut rem_n = pool_nco;
    let mut rem_o = pool_off;
    let mut rem_eq = pool_eq;
    let mut spawned = Vec::new();
    for (idx, sp) in specs_ref.iter().enumerate() {
        let te = sp.enlisted.min(rem_e);
        rem_e -= te;
        // 分配骨干：优先分配军官，再分配士官，防止降级为0
        let to = sp.cadre.min(rem_o);
        rem_o -= to;
        let tn = (sp.cadre.saturating_sub(to)).min(rem_n);
        rem_n -= tn;

        let mut eq_map = FxHashMap::default();
        for (m, n) in &sp.equipment {
            let av = (*n).min(*rem_eq.get(m).unwrap_or(&0));
            if av > 0 {
                eq_map.insert(m.clone(), av);
                *rem_eq.entry(m.clone()).or_insert(0) -= av;
            }
        }
        let kind = if to > 0 && sp.echelon >= Echelon::Company {
            "hq_squad"
        } else {
            "infantry_squad"
        };
        let uid = units.spawn_unit_4ledger(
            &sp.str_id, &sp.name, team, spawn_px, sp.echelon, kind, te, tn, to, eq_map, ch,
        );
        units.pos[uid.0] = [
            spawn_px.0 as f32 + ((idx % 3) as f32 - 1.0) * 0.08,
            spawn_px.1 as f32 + ((idx / 3) as f32 - 1.0) * 0.08,
        ];
        units.org[uid.0] = mean_org.min(units.org_cap[uid.0]);
        commanders.link_subordinate_unit(commander_id, uid, units);
        spawned.push(uid);
    }

    if let Some(&last_u) = spawned.last() {
        let rem_cad = rem_n + rem_o;
        if rem_e > 0 || rem_eq.values().sum::<u32>() > 0 || (rem_cad > 0 && !strict_cadre_allocation) {
            let i = last_u.0;
            units.enlisted[i] += rem_e;
            if !strict_cadre_allocation {
                units.nco[i] += rem_n;
                units.officer[i] += rem_o;
                units.cadre[i] = units.nco[i] + units.officer[i];
            }
            for (m, n) in rem_eq {
                if n > 0 {
                    *units.equipment[i].entry(m).or_insert(0) += n;
                }
            }
            units.refresh_caps(last_u);
        }
    }

    let tot_p = spawned.iter().map(|&u| units.persons(u)).sum::<u32>().max(1) as f32;
    for &u in &spawned {
        let frac = units.persons(u) as f32 / tot_p;
        units.food_t[u.0] = pool_food * frac;
        units.ammo_t[u.0] = pool_ammo * frac;
        units.fuel_t[u.0] = pool_fuel * frac;
    }
    spawned
}
