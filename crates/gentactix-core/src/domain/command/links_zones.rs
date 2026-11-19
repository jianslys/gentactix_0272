//! `command_log` module: Single-parent command hierarchy, communication links (400m / wire / radio),
//! non-overlapping AO validation, bidirectional combat logs, disconnected rumor amplification,
//! pure-math self-similar hierarchical frontline generator + propagation delay tau, and pluggable `CommanderDriver`.

use crate::units_ecs::{
    aggregate_modifiers, equipment_spec, CommanderId, CommanderSoA,
    Echelon, LockFlags, Rank, UnitId, UnitSoA,
};
use crate::world_grid::{TeamId, WorldGrid};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkVia {
    Proximity400m,
    Wire,
    Radio,
    FixedStashRadio,
    Disconnected,
}

impl LinkVia {
    #[inline]
    pub fn is_connected(self) -> bool {
        !matches!(self, LinkVia::Disconnected)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LinkVia::Proximity400m => "proximity_400m",
            LinkVia::Wire => "wire",
            LinkVia::Radio => "radio",
            LinkVia::FixedStashRadio => "fixed_stash_radio",
            LinkVia::Disconnected => "disconnected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Doctrine {
    Attack,
    Assault,
    Defend,
    Hold,
    Withdraw,
}

impl Doctrine {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "attack" => Doctrine::Attack,
            "assault" => Doctrine::Assault,
            "hold" => Doctrine::Hold,
            "withdraw" | "retreat" => Doctrine::Withdraw,
            _ => Doctrine::Defend,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Doctrine::Attack => "attack",
            Doctrine::Assault => "assault",
            Doctrine::Defend => "defend",
            Doctrine::Hold => "hold",
            Doctrine::Withdraw => "withdraw",
        }
    }
}

pub fn max_radio_range_on_unit(units: &UnitSoA, uid: UnitId) -> f32 {
    let mut best = 0.0f32;
    for (m, &n) in &units.equipment[uid.0] {
        if n > 0 {
            best = best.max(equipment_spec(m).radio_km);
        }
    }
    best
}

pub fn commander_radio_range_km(cmdr_id: CommanderId, commanders: &CommanderSoA, units: &UnitSoA, grid: &WorldGrid) -> f32 {
    if commanders.radio_silence[cmdr_id.0] {
        return 0.0;
    }
    let mut best: f32 = match commanders.rank[cmdr_id.0] {
        Rank::General => 35.0,
        Rank::Colonel => 22.0,
        Rank::LtCol => 15.0,
        Rank::Major => 10.0,
        Rank::Captain => 6.0,
        _ => 3.5,
    };
    if let Some(hq) = commanders.hq_piece_id[cmdr_id.0] {
        if units.alive[hq.0] {
            best = best.max(max_radio_range_on_unit(units, hq));
        }
    }
    let px = commanders.px[cmdr_id.0];
    let idx = grid.idx(px.0, px.1) as u32;
    if let Some(st) = grid.cell_stash.get(&idx) {
        if st.owner_team == Some(commanders.team[cmdr_id.0]) && st.fixed_radio_units > 0 {
            best = best.max(20.0);
        }
    }
    best
}

pub fn commander_has_officer_or_acting_nco(cmdr_id: CommanderId, commanders: &CommanderSoA, units: &UnitSoA) -> (bool, bool) {
    let i = cmdr_id.0;
    if i >= commanders.id.len() || !commanders.alive[i] {
        return (false, false);
    }
    if let Some(hq) = commanders.hq_piece_id[i] {
        if !units.alive[hq.0] {
            return (false, false);
        }
        let off = units.officer[hq.0];
        let nco = units.nco[hq.0];
        return (off > 0, off == 0 && nco > 0);
    }
    let s = commanders.nco_deputy_scale.get(i).copied().unwrap_or(1.0);
    (s >= 0.99, s > 0.0 && s < 0.99)
}

pub fn effective_commander_radius_km(cmdr_id: CommanderId, commanders: &CommanderSoA, units: &UnitSoA, _grid: &WorldGrid) -> f32 {
    let (has_off, is_acting_nco) = commander_has_officer_or_acting_nco(cmdr_id, commanders, units);
    if !has_off && !is_acting_nco {
        return 0.0;
    }
    let base = if is_acting_nco {
        commanders.rank[cmdr_id.0].base_radius_km().min(crate::units_ecs::Rank::Sergeant.base_radius_km())
    } else {
        commanders.rank[cmdr_id.0].base_radius_km()
    };
    let deputy_scale = if is_acting_nco { 0.65 } else { 1.0 };
    let silent_scale = if commanders.radio_silence[cmdr_id.0] { 0.35 } else { 1.0 };
    let council_scale = if commanders.council_ao_hours[cmdr_id.0] > 0.0 { 1.5 } else { 1.0 };
    let staff_scale = commanders.commander_staff_efficiency(cmdr_id);
    base * deputy_scale * silent_scale * council_scale * staff_scale
}

pub fn effective_commander_max_area_px(cmdr_id: CommanderId, commanders: &CommanderSoA) -> usize {
    let deputy_scale = commanders.nco_deputy_scale.get(cmdr_id.0).copied().unwrap_or(1.0);
    if deputy_scale <= 0.0 {
        return 0;
    }
    let raw_base = commanders.rank[cmdr_id.0].ao_max_area_px();
    let base = if deputy_scale < 0.99 {
        raw_base.min(crate::units_ecs::Rank::Sergeant.ao_max_area_px())
    } else {
        raw_base
    };
    let silent_scale = if commanders.radio_silence[cmdr_id.0] { 0.5 } else { 1.0 };
    let council_scale = if commanders.council_ao_hours[cmdr_id.0] > 0.0 { 1.5 } else { 1.0 };
    let staff_scale = commanders.commander_staff_efficiency(cmdr_id);
    ((base as f32) * deputy_scale * silent_scale * council_scale * staff_scale).round().max(1.0) as usize
}

pub fn validate_command_link(
    cmdr_id: CommanderId,
    unit_id: UnitId,
    commanders: &CommanderSoA,
    units: &UnitSoA,
    grid: &WorldGrid,
) -> LinkVia {
    if !commanders.alive[cmdr_id.0] || !units.alive[unit_id.0] || units.persons(unit_id) == 0 {
        return LinkVia::Disconnected;
    }
    if commanders.team[cmdr_id.0] != units.team[unit_id.0] {
        return LinkVia::Disconnected;
    }
    let c_px = commanders.px[cmdr_id.0];
    let u_px = units.px[unit_id.0];
    let dist_km = grid.distance_km([c_px.0 as f32, c_px.1 as f32], [u_px.0 as f32, u_px.1 as f32]);
    if dist_km <= 0.4001 {
        return LinkVia::Proximity400m;
    }
    if grid.has_wire_connection(c_px, u_px, commanders.team[cmdr_id.0]) {
        return LinkVia::Wire;
    }
    // 无线电操作权限由士官/军官(can_operate_radio = nco + officer > 0)控制
    if !units.can_operate_radio[unit_id.0] {
        return LinkVia::Disconnected;
    }
    if let Some(hq) = commanders.hq_piece_id[cmdr_id.0] {
        if !units.alive[hq.0] || !units.can_operate_radio[hq.0] {
            return LinkVia::Disconnected;
        }
    }
    if commanders.radio_silence[cmdr_id.0] || units.radio_silence[unit_id.0] {
        return LinkVia::Disconnected;
    }
    if commanders.radio_channel[cmdr_id.0] != units.radio_channel[unit_id.0] {
        return LinkVia::Disconnected;
    }
    let c_mods = aggregate_modifiers(&commanders.modifier_slots[cmdr_id.0], commanders.order_churn_score[cmdr_id.0], false);
    let u_mods = aggregate_modifiers(&units.modifier_slots[unit_id.0], 0.0, false);
    if c_mods.locks.contains(LockFlags::FORBID_RADIO) || u_mods.locks.contains(LockFlags::FORBID_RADIO) {
        return LinkVia::Disconnected;
    }
    let c_rng = commander_radio_range_km(cmdr_id, commanders, units, grid);
    let u_rng = max_radio_range_on_unit(units, unit_id).max(5.0);
    if dist_km <= c_rng.min(u_rng) {
        LinkVia::Radio
    } else {
        LinkVia::Disconnected
    }
}

pub fn validate_commander_link(
    sup_id: CommanderId,
    sub_id: CommanderId,
    commanders: &CommanderSoA,
    units: &UnitSoA,
    grid: &WorldGrid,
) -> LinkVia {
    if !commanders.alive[sup_id.0] || !commanders.alive[sub_id.0] {
        return LinkVia::Disconnected;
    }
    if commanders.team[sup_id.0] != commanders.team[sub_id.0] {
        return LinkVia::Disconnected;
    }
    let a = commanders.px[sup_id.0];
    let b = commanders.px[sub_id.0];
    let dist_km = grid.distance_km([a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]);
    if dist_km <= 0.4001 {
        return LinkVia::Proximity400m;
    }
    if grid.has_wire_connection(a, b, commanders.team[sup_id.0]) {
        return LinkVia::Wire;
    }
    if commanders.radio_silence[sup_id.0] || commanders.radio_silence[sub_id.0] {
        return LinkVia::Disconnected;
    }
    if commanders.radio_channel[sup_id.0] != commanders.radio_channel[sub_id.0] {
        return LinkVia::Disconnected;
    }
    let r_a = commander_radio_range_km(sup_id, commanders, units, grid);
    let r_b = commander_radio_range_km(sub_id, commanders, units, grid);
    if dist_km <= r_a.min(r_b) {
        LinkVia::Radio
    } else {
        LinkVia::Disconnected
    }
}

#[inline]
pub fn can_commander_write_line(player_cid: CommanderId, target_cid: CommanderId) -> bool {
    player_cid == target_cid
}

#[derive(Debug, Clone, Default)]
pub struct ReadableScope {
    pub readable_unit_ids: FxHashSet<UnitId>,
    pub readable_cmd_ids: FxHashSet<CommanderId>,
    pub proximity_unit_ids: FxHashSet<UnitId>,
    pub proximity_cmd_ids: FxHashSet<CommanderId>,
}

/// Returns strictly what `viewer_cid` can read: connected direct subordinates PLUS
/// ANY friendly units/commanders physically within <= 400m direct sight (cross-battalion allowed).
/// Disconnected remote units are OMITTED completely.
pub fn get_commander_readable_scope(
    viewer_cid: CommanderId,
    units: &UnitSoA,
    commanders: &CommanderSoA,
    grid: &WorldGrid,
) -> ReadableScope {
    let mut scope = ReadableScope::default();
    if !commanders.alive[viewer_cid.0] {
        return scope;
    }
    scope.readable_cmd_ids.insert(viewer_cid);
    let team = commanders.team[viewer_cid.0];
    let v_px = commanders.px[viewer_cid.0];
    let v_pos = [v_px.0 as f32, v_px.1 as f32];

    for &uid in &commanders.subordinate_unit_ids[viewer_cid.0] {
        if validate_command_link(viewer_cid, uid, commanders, units, grid).is_connected() {
            scope.readable_unit_ids.insert(uid);
        }
    }
    for &cid in &commanders.subordinate_cmd_ids[viewer_cid.0] {
        if validate_commander_link(viewer_cid, cid, commanders, units, grid).is_connected() {
            scope.readable_cmd_ids.insert(cid);
        }
    }

    // 400m physical direct observation of cross-hierarchy friendlies
    for uid in 0..units.id.len() {
        let u = UnitId(uid);
        if !units.alive[uid] || units.team[uid] != team || units.persons(u) == 0 {
            continue;
        }
        let d = grid.distance_km(v_pos, [units.px[uid].0 as f32, units.px[uid].1 as f32]);
        if d <= 0.4001 {
            scope.readable_unit_ids.insert(u);
            if !commanders.subordinate_unit_ids[viewer_cid.0].contains(&u) {
                scope.proximity_unit_ids.insert(u);
            }
        }
    }
    for cid in 0..commanders.id.len() {
        let c = CommanderId(cid);
        if c == viewer_cid || !commanders.alive[cid] || commanders.team[cid] != team {
            continue;
        }
        let d = grid.distance_km(v_pos, [commanders.px[cid].0 as f32, commanders.px[cid].1 as f32]);
        if d <= 0.4001 {
            scope.readable_cmd_ids.insert(c);
            if !commanders.subordinate_cmd_ids[viewer_cid.0].contains(&c) {
                scope.proximity_cmd_ids.insert(c);
            }
        }
    }
    scope
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalZone {
    pub zone_id: u32,
    pub name_id: String,
    pub commander_id: CommanderId,
    pub team: TeamId,
    pub echelon: Echelon,
    pub cells: Vec<(i32, i32)>,
    pub cell_set: FxHashSet<(i32, i32)>,
    pub activate_turn: u32,
    pub active: bool,
}

impl OperationalZone {
    pub fn area_px(&self) -> usize {
        self.cells.len()
    }
}

pub fn register_zone(
    zones: &mut Vec<OperationalZone>,
    zone_name: String,
    commander_id: CommanderId,
    echelon: Echelon,
    cells: Vec<(i32, i32)>,
    current_turn: u32,
    activate_delay_turns: u32,
    commanders: &CommanderSoA,
    grid: &mut WorldGrid,
) -> Result<u32, String> {
    if !commanders.alive[commander_id.0] {
        return Err("commander_dead".into());
    }
    let team = commanders.team[commander_id.0];
    let deputy_scale = commanders.nco_deputy_scale.get(commander_id.0).copied().unwrap_or(1.0);
    if deputy_scale <= 0.0 {
        return Err("requires_officer_or_nco_deputy_to_draw_ao".into());
    }
    if deputy_scale < 0.99 && (echelon as u8) > (Echelon::Platoon as u8) {
        return Err("nco_acting_deputy_capped_at_platoon_echelon".into());
    }
    let max_area = effective_commander_max_area_px(commander_id, commanders);
    let max_cnt = commanders.rank[commander_id.0].ao_max_count();

    let cell_set: FxHashSet<(i32, i32)> = cells
        .into_iter()
        .filter(|&(r, c)| grid.in_bounds(r, c))
        .collect();
    if cell_set.is_empty() {
        return Err("empty_ao".into());
    }
    if cell_set.len() > max_area {
        return Err(format!("ao_area_exceeds_quota:{}>{}", cell_set.len(), max_area));
    }

    let own_count = zones.iter().filter(|z| z.active && z.commander_id == commander_id).count();
    if own_count >= max_cnt {
        return Err(format!("ao_count_exceeds_quota:{}>={}", own_count, max_cnt));
    }

    for z in zones.iter().filter(|z| z.active && z.team == team && z.echelon == echelon && z.commander_id != commander_id) {
        if z.cell_set.iter().any(|rc| cell_set.contains(rc)) {
            return Err(format!("same_echelon_ao_overlap:{}", z.name_id));
        }
    }

    let zone_id = (zones.len() + 1) as u32;
    let cell_vec: Vec<(i32, i32)> = cell_set.iter().copied().collect();
    let act_turn = current_turn + activate_delay_turns;
    if activate_delay_turns == 0 {
        grid.write_ao_cells(zone_id, team, echelon as u8, &cell_vec);
    }
    zones.push(OperationalZone {
        zone_id,
        name_id: zone_name,
        commander_id,
        team,
        echelon,
        cells: cell_vec,
        cell_set,
        activate_turn: act_turn,
        active: true,
    });
    Ok(zone_id)
}

#[derive(Debug, Clone, Copy)]
pub struct AoBuff {
    pub connected: bool,
    pub logistics_mult: f32,
    pub combat_mult: f32,
}

pub fn compute_operational_ao_buff(
    px: (i32, i32),
    team: TeamId,
    unit_cmdr: Option<CommanderId>,
    zones: &[OperationalZone],
    commanders: &CommanderSoA,
    units: &UnitSoA,
    grid: &WorldGrid,
    current_turn: u32,
) -> AoBuff {
    let mut best_logi = 1.0f32;
    let mut best_combat = 1.0f32;
    let mut any_connected = false;
    for z in zones {
        if !z.active || z.team != team || z.activate_turn > current_turn || !z.cell_set.contains(&px) {
            continue;
        }
        let linked = match unit_cmdr {
            Some(c) if c == z.commander_id => true,
            Some(c) => validate_commander_link(z.commander_id, c, commanders, units, grid).is_connected(),
            None => true,
        };
        if !linked {
            continue;
        }
        any_connected = true;
        if (z.echelon as u8) >= (Echelon::Regiment as u8) {
            best_logi = best_logi.min(0.65);
        } else if z.echelon == Echelon::Battalion {
            best_logi = best_logi.min(0.82);
        }
        best_combat = best_combat.max(1.06);
    }
    if !any_connected {
        AoBuff { connected: false, logistics_mult: 1.0, combat_mult: 1.0 }
    } else {
        AoBuff { connected: true, logistics_mult: best_logi, combat_mult: best_combat }
    }
}

