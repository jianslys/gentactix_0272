use super::*;
use crate::world_grid::{TeamId, WorldGrid};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct CommanderSoA {
    pub id: Vec<CommanderId>,
    pub str_id: Vec<String>,
    pub name: Vec<String>,
    pub team: Vec<TeamId>,
    pub echelon: Vec<Echelon>,
    pub rank: Vec<Rank>,
    pub px: Vec<(i32, i32)>,
    pub alive: Vec<bool>,
    pub radio_channel: Vec<u8>,
    pub radio_silence: Vec<bool>,
    pub hq_piece_id: Vec<Option<UnitId>>,
    pub attached_to: Vec<Option<UnitId>>,
    pub cadre_piece_id: Vec<Option<UnitId>>,
    pub parent_commander_id: Vec<Option<CommanderId>>,
    pub subordinate_cmd_ids: Vec<Vec<CommanderId>>,
    pub subordinate_unit_ids: Vec<Vec<UnitId>>,

    pub last_px: Vec<(i32, i32)>,
    pub still_hours: Vec<f32>,
    pub council_streak: Vec<u32>,
    pub council_cooldown_hours: Vec<f32>,
    pub briefing_buff_hours: Vec<f32>,
    pub council_ao_hours: Vec<f32>,
    pub war_council_sync_hours: Vec<f32>,
    pub council_partner_id: Vec<Option<CommanderId>>,
    pub council_radius_km: Vec<f32>,
    pub council_completed: Vec<bool>,
    pub post_council_exposure_hours: Vec<f32>,
    pub post_council_exposure_val: Vec<f32>,

    pub order_churn_score: Vec<f32>,
    pub last_order_sig: Vec<u64>,
    pub last_auto_order_clock_hours: Vec<f32>,
    pub parent_order_seq: Vec<u64>,
    pub assigned_parent_line_pts: Vec<Vec<[f32; 2]>>,
    pub modifier_slots: Vec<Vec<ModifierSlot>>,

    pub boldness: Vec<f32>,
    pub patience: Vec<f32>,
    pub nco_deputy_scale: Vec<f32>,
    pub staff: Vec<u32>,
    pub staff_full: Vec<u32>,
    pub str_to_id: FxHashMap<String, CommanderId>,
}

impl Default for CommanderSoA {
    fn default() -> Self {
        Self::new()
    }
}

impl CommanderSoA {
    pub fn new() -> Self {
        Self {
            id: Vec::new(),
            str_id: Vec::new(),
            name: Vec::new(),
            team: Vec::new(),
            echelon: Vec::new(),
            rank: Vec::new(),
            px: Vec::new(),
            alive: Vec::new(),
            radio_channel: Vec::new(),
            radio_silence: Vec::new(),
            hq_piece_id: Vec::new(),
            attached_to: Vec::new(),
            cadre_piece_id: Vec::new(),
            parent_commander_id: Vec::new(),
            subordinate_cmd_ids: Vec::new(),
            subordinate_unit_ids: Vec::new(),
            last_px: Vec::new(),
            still_hours: Vec::new(),
            council_streak: Vec::new(),
            council_cooldown_hours: Vec::new(),
            briefing_buff_hours: Vec::new(),
            council_ao_hours: Vec::new(),
            war_council_sync_hours: Vec::new(),
            council_partner_id: Vec::new(),
            council_radius_km: Vec::new(),
            council_completed: Vec::new(),
            post_council_exposure_hours: Vec::new(),
            post_council_exposure_val: Vec::new(),
            order_churn_score: Vec::new(),
            last_order_sig: Vec::new(),
            last_auto_order_clock_hours: Vec::new(),
            parent_order_seq: Vec::new(),
            assigned_parent_line_pts: Vec::new(),
            modifier_slots: Vec::new(),
            boldness: Vec::new(),
            patience: Vec::new(),
            nco_deputy_scale: Vec::new(),
            staff: Vec::new(),
            staff_full: Vec::new(),
            str_to_id: FxHashMap::default(),
        }
    }

    pub fn spawn_commander(
        &mut self,
        str_id: impl Into<String>,
        name: impl Into<String>,
        team: TeamId,
        px: (i32, i32),
        echelon: Echelon,
        rank: Rank,
        radio_channel: u8,
    ) -> CommanderId {
        let cid = CommanderId(self.id.len());
        let s_id = str_id.into();
        self.str_to_id.insert(s_id.clone(), cid);
        self.id.push(cid);
        self.str_id.push(s_id);
        self.name.push(name.into());
        self.team.push(team);
        self.echelon.push(echelon);
        self.rank.push(rank);
        self.px.push(px);
        self.alive.push(true);
        self.radio_channel.push(radio_channel);
        self.radio_silence.push(false);
        self.hq_piece_id.push(None);
        self.attached_to.push(None);
        self.cadre_piece_id.push(None);
        self.parent_commander_id.push(None);
        self.subordinate_cmd_ids.push(Vec::new());
        self.subordinate_unit_ids.push(Vec::new());
        self.last_px.push(px);
        self.still_hours.push(0.0);
        self.council_streak.push(0);
        self.council_cooldown_hours.push(0.0);
        self.briefing_buff_hours.push(0.0);
        self.council_ao_hours.push(0.0);
        self.war_council_sync_hours.push(0.0);
        self.council_partner_id.push(None);
        self.council_radius_km.push(1.0);
        self.council_completed.push(false);
        self.post_council_exposure_hours.push(0.0);
        self.post_council_exposure_val.push(0.0);
        self.order_churn_score.push(0.0);
        self.last_order_sig.push(0);
        self.last_auto_order_clock_hours.push(-9999.0);
        self.parent_order_seq.push(0);
        self.assigned_parent_line_pts.push(Vec::new());
        self.modifier_slots.push(Vec::new());
        self.boldness.push(0.5);
        self.patience.push(0.5);
        self.nco_deputy_scale.push(1.0);
        let sf = echelon.default_staff_full();
        self.staff_full.push(sf);
        self.staff.push(sf);
        cid
    }

    pub fn set_staff(&mut self, cid: CommanderId, staff: u32) {
        if cid.0 < self.staff.len() {
            self.staff[cid.0] = staff;
        }
    }

    pub fn set_staff_full(&mut self, cid: CommanderId, staff_full: u32) {
        if cid.0 < self.staff_full.len() {
            self.staff_full[cid.0] = staff_full;
        }
    }

    pub fn commander_staff_efficiency(&self, cid: CommanderId) -> f32 {
        let sf = self.staff_full.get(cid.0).copied().unwrap_or(0);
        if sf == 0 {
            1.0
        } else {
            let cur = self.staff.get(cid.0).copied().unwrap_or(sf);
            (cur as f32 / sf as f32).clamp(0.0, 1.0)
        }
    }

    pub fn sync_hq_ledger_authority(&mut self, cid: CommanderId, units: &UnitSoA) {
        let i = cid.0;
        if i >= self.id.len() || !self.alive[i] {
            return;
        }
        if let Some(uid) = self.hq_piece_id[i] {
            if units.alive[uid.0] && units.kind[uid.0] == "hq_squad" {
                let off = units.officer[uid.0];
                let nco = units.nco[uid.0];
                self.nco_deputy_scale[i] = if off > 0 {
                    1.0
                } else if nco > 0 {
                    0.65
                } else {
                    0.0
                };
            } else {
                self.nco_deputy_scale[i] = 0.0;
            }
        }
    }

    pub fn link_subordinate_unit(&mut self, parent: CommanderId, unit: UnitId, units: &mut UnitSoA) {
        if let Some(old_p) = units.direct_commander_id[unit.0] {
            self.subordinate_unit_ids[old_p.0].retain(|&u| u != unit);
        }
        units.direct_commander_id[unit.0] = Some(parent);
        if !self.subordinate_unit_ids[parent.0].contains(&unit) {
            self.subordinate_unit_ids[parent.0].push(unit);
        }
    }

    pub fn link_subordinate_cmd(&mut self, parent: CommanderId, child: CommanderId) {
        if parent == child {
            return;
        }
        if let Some(old_p) = self.parent_commander_id[child.0] {
            self.subordinate_cmd_ids[old_p.0].retain(|&c| c != child);
        }
        self.parent_commander_id[child.0] = Some(parent);
        if !self.subordinate_cmd_ids[parent.0].contains(&child) {
            self.subordinate_cmd_ids[parent.0].push(child);
        }
    }

    pub fn resolve_succession(&mut self, cid: CommanderId, units: &UnitSoA) -> Option<String> {
        let i = cid.0;
        if !self.alive[i] {
            return None;
        }
        let hq_ok = self.hq_piece_id[i].map_or(true, |uid| units.alive[uid.0] && units.persons(uid) > 0);
        if hq_ok {
            return None;
        }
        if let Some(next_rank) = self.rank[i].demotion_next() {
            self.rank[i] = next_rank;
            self.order_churn_score[i] += 1.5;
            return Some(format!("{} decapitated -> demoted to {}", self.str_id[i], next_rank.as_str()));
        }
        self.alive[i] = false;
        Some(format!("{} destroyed with no succession", self.str_id[i]))
    }

    pub fn is_war_council_sync_active(&self, cid: CommanderId, grid: &WorldGrid) -> bool {
        let i = cid.0;
        if !self.alive[i] || self.war_council_sync_hours[i] <= 0.0 {
            return false;
        }
        if let Some(pid) = self.council_partner_id[i] {
            if !self.alive[pid.0] {
                return false;
            }
            let dist = grid.distance_km(
                [self.px[i].0 as f32, self.px[i].1 as f32],
                [self.px[pid.0].0 as f32, self.px[pid.0].1 as f32],
            );
            return dist <= self.council_radius_km[i] + 1e-6;
        }
        true
    }
}

#[inline]
pub fn deterministic_unit_float(seed_tag: &str, unit_id: UnitId, tick: u32) -> f32 {
    let mut h = Sha256::new();
    h.update(seed_tag.as_bytes());
    h.update(unit_id.0.to_le_bytes());
    h.update(tick.to_le_bytes());
    let dig = h.finalize();
    let v = u32::from_le_bytes([dig[0], dig[1], dig[2], dig[3]]);
    (v as f64 / u32::MAX as f64) as f32
}

pub fn tick_overload_faults_5m(units: &mut UnitSoA, dt_hours: f32, tick: u32) -> Vec<(UnitId, &'static str)> {
    let mut faults = Vec::new();
    for i in 0..units.id.len() {
        if !units.alive[i] || units.overload_ratio[i] <= 1.0 + 1e-6 || !units.moved_last_turn[i] {
            continue;
        }
        let ratio = units.overload_ratio[i];
        let base_p = (0.18 * (ratio - 1.0)).min(0.85);
        let p_step = 1.0 - (1.0 - base_p).powf(dt_hours / 0.25);
        let roll = deterministic_unit_float("overload", UnitId(i), tick);
        if roll < p_step {
            let fault_idx = ((roll * 997.0) as usize) % 3;
            let (src, tag, locks, spd) = match fault_idx {
                0 => (ModifierSource::OverloadAxleSnap, "FAULT_AXLE_SNAP", LockFlags::FORBID_MOVE | LockFlags::FORBID_TOW, 0.0),
                1 => (ModifierSource::OverloadGearbox, "FAULT_GEARBOX", LockFlags::FORBID_TOW, 0.45),
                _ => (ModifierSource::OverloadEngineOverheat, "FAULT_ENGINE_OVERHEAT", LockFlags::empty(), 0.65),
            };
            set_modifier_slot(
                &mut units.modifier_slots[i],
                ModifierSlot {
                    source: src,
                    tag,
                    locks,
                    speed_mult: spd,
                    ttl_hours: Some(2.0),
                    ..Default::default()
                },
            );
            faults.push((UnitId(i), tag));
        }
    }
    faults
}
