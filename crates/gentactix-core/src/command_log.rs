//! `command_log` module: Single-parent command hierarchy, communication links (400m / wire / radio),
//! non-overlapping AO validation, bidirectional combat logs, disconnected rumor amplification,
//! pure-math self-similar hierarchical frontline generator + propagation delay tau, and pluggable `CommanderDriver`.

use crate::units_ecs::{
    aggregate_modifiers, equipment_spec, AlertMark, AtomicAction, CommanderId, CommanderSoA,
    Echelon, LockFlags, Rank, UnitId, UnitSoA, RADIO_CHANNELS,
};
use crate::world_grid::{
    FrontlineRole, TeamId, WorldGrid, FOREST, LINE_MASK_ALL, WATER,
};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatLogUplink {
    pub unit_id: UnitId,
    pub str_id: String,
    pub name: String,
    pub px: (i32, i32),
    pub echelon: Echelon,
    pub persons: u32,
    pub delta_persons_5m: i32,
    pub org: f32,
    pub delta_org_5m: f32,
    pub action: AtomicAction,
    pub alert: AlertMark,
    pub rumor_enemy_persons: f32,
    pub rumor_amplification: f32,
    pub supply_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatLogOrder {
    pub order_id: u64,
    pub seq: u64,
    pub author_commander_id: CommanderId,
    pub target_commander_id: CommanderId,
    pub doctrine: Doctrine,
    pub target_px: (i32, i32),
    pub points_px: Vec<[f32; 2]>,
    pub front_normal: [f32; 2],
    pub unit_type_mask: u8,
    pub role: FrontlineRole,
    pub deliver_at_hours: f32,
}

pub fn harvest_nearby_friendly_intel(units: &UnitSoA, uid: UnitId, grid: &WorldGrid, base_enemy_persons: f32) -> (f32, f32) {
    let i = uid.0;
    let mut amp = 1.0f32;
    if !units.was_disconnected[i] {
        return (base_enemy_persons, amp);
    }
    let team = units.team[i];
    let p = units.pos[i];
    let mut count = 0;
    for j in 0..units.id.len() {
        if j != i && units.alive[j] && units.team[j] == team && units.was_disconnected[j] {
            if grid.distance_km(p, units.pos[j]) <= 0.4001 {
                amp = (amp * 1.30).min(3.0);
                count += 1;
                if count >= 4 {
                    break;
                }
            }
        }
    }
    (base_enemy_persons * amp, amp)
}

pub fn collect_commander_uplink_logs(
    cmdr_id: CommanderId,
    units: &UnitSoA,
    commanders: &CommanderSoA,
    grid: &WorldGrid,
) -> Vec<CombatLogUplink> {
    let mut logs = Vec::new();
    for &uid in &commanders.subordinate_unit_ids[cmdr_id.0] {
        if !validate_command_link(cmdr_id, uid, commanders, units, grid).is_connected() {
            continue; // Disconnected = literally nothing reported!
        }
        let i = uid.0;
        let p = units.persons(uid);
        let sup_pct = if units.cap_ammo_t[i] > 1e-6 {
            ((units.ammo_t[i] / units.cap_ammo_t[i]) * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        };
        let (rumor_p, amp) = harvest_nearby_friendly_intel(units, uid, grid, 0.0);
        logs.push(CombatLogUplink {
            unit_id: uid,
            str_id: units.str_id[i].clone(),
            name: units.name[i].clone(),
            px: units.px[i],
            echelon: units.restricted_echelon[i],
            persons: p,
            delta_persons_5m: units.delta_persons_5m[i],
            org: units.org[i],
            delta_org_5m: units.delta_org_5m[i],
            action: units.autonomy_state[i],
            alert: units.exclamation_buff[i],
            rumor_enemy_persons: rumor_p,
            rumor_amplification: amp,
            supply_pct: sup_pct,
        });
    }
    logs
}

/// Pluggable commander driver trait (pure math / human UI draft / external callback LLM outside engine).
pub trait CommanderDriver: Send + Sync {
    fn evaluate_downlink(
        &mut self,
        cmdr_id: CommanderId,
        clock_hours: f32,
        tick: u32,
        grid: &WorldGrid,
        units: &UnitSoA,
        commanders: &CommanderSoA,
    ) -> Option<Vec<(CommanderId, Vec<[f32; 2]>, [f32; 2], u8)>>;
}

/// Pure mathematical self-similar hierarchical frontline partitioner (Division -> ... -> Platoon).
#[derive(Debug, Default)]
pub struct MathRecursiveDriver;

impl CommanderDriver for MathRecursiveDriver {
    fn evaluate_downlink(
        &mut self,
        cmdr_id: CommanderId,
        clock_hours: f32,
        tick: u32,
        grid: &WorldGrid,
        units: &UnitSoA,
        commanders: &CommanderSoA,
    ) -> Option<Vec<(CommanderId, Vec<[f32; 2]>, [f32; 2], u8)>> {
        let ci = cmdr_id.0;
        if !commanders.alive[ci] {
            return None;
        }
        let ech = commanders.echelon[ci];
        let cadence = ech.cadence_ticks().max(1);
        if tick % cadence != 0 {
            return None;
        }
        let mods = aggregate_modifiers(
            &commanders.modifier_slots[ci],
            commanders.order_churn_score[ci],
            commanders.is_war_council_sync_active(cmdr_id, grid),
        );
        let eff_cooldown_h = (ech.hard_cooldown_min() * mods.tau_mult) / 60.0;
        if clock_hours - commanders.last_auto_order_clock_hours[ci] < eff_cooldown_h - 1e-5 {
            return None;
        }
        if commanders.radio_silence[ci] {
            return None;
        }
        let parent_pts = &commanders.assigned_parent_line_pts[ci];
        if parent_pts.len() < 2 {
            return None;
        }

        // Only partition among CONNECTED subordinate commanders (disconnected gaps left naturally / absorbed)
        let mut conn_subs: Vec<(CommanderId, f32)> = Vec::new();
        for &sub_c in &commanders.subordinate_cmd_ids[ci] {
            if validate_commander_link(cmdr_id, sub_c, commanders, units, grid).is_connected() {
                let mut w = 1.0f32;
                for &u in &commanders.subordinate_unit_ids[sub_c.0] {
                    if units.alive[u.0] {
                        w += units.persons(u) as f32 * (0.5 + 0.5 * units.org[u.0] / 100.0);
                    }
                }
                conn_subs.push((sub_c, w));
            }
        }
        if conn_subs.is_empty() {
            return None;
        }
        let total_w: f32 = conn_subs.iter().map(|x| x.1).sum::<f32>().max(1.0);
        let p0 = parent_pts[0];
        let p1 = parent_pts[parent_pts.len() - 1];
        let dr = p1[0] - p0[0];
        let dc = p1[1] - p0[1];
        let norm = dr.hypot(dc).max(1e-5);
        let def_normal = [dc / norm, -dr / norm];

        let mut acc = 0.0f32;
        let mut out = Vec::with_capacity(conn_subs.len());
        for (sub_c, w) in conn_subs {
            let f0 = acc / total_w;
            acc += w;
            let f1 = acc / total_w;
            let mut a = [p0[0] + dr * f0, p0[1] + dc * f0];
            let mut b = [p0[0] + dr * f1, p0[1] + dc * f1];
            // Local terrain affinity snap: avoid placing line endpoints inside WATER
            let ra = a[0].round() as i32;
            let ca = a[1].round() as i32;
            if !grid.in_bounds(ra, ca) || grid.terrain[grid.idx(ra, ca)] == WATER {
                let g_a = grid.find_nearest_literal_ground_px(ra, ca);
                a = [g_a.0 as f32, g_a.1 as f32];
            } else {
                // Subtle forest preference snap within 1 px
                'snap: for sr in -1..=1 {
                    for sc in -1..=1 {
                        if grid.in_bounds(ra + sr, ca + sc) && grid.terrain[grid.idx(ra + sr, ca + sc)] == FOREST {
                            a = [(ra + sr) as f32, (ca + sc) as f32];
                            break 'snap;
                        }
                    }
                }
            }
            let rb = b[0].round() as i32;
            let cb = b[1].round() as i32;
            if !grid.in_bounds(rb, cb) || grid.terrain[grid.idx(rb, cb)] == WATER {
                let g_b = grid.find_nearest_literal_ground_px(rb, cb);
                b = [g_b.0 as f32, g_b.1 as f32];
            }
            out.push((sub_c, vec![a, b], def_normal, LINE_MASK_ALL));
        }
        Some(out)
    }
}

#[derive(Debug, Clone)]
pub struct PendingDownlink {
    pub deliver_at_hours: f32,
    pub parent_cid: CommanderId,
    pub target_cid: CommanderId,
    pub line_name: String,
    pub role: FrontlineRole,
    pub unit_type_mask: u8,
    pub points_px: Vec<[f32; 2]>,
    pub front_normal: [f32; 2],
    pub source_deploy_line_id: u32,
    pub target_deploy_line_id: u32,
}

#[derive(Debug, Default)]
pub struct DownlinkPipeline {
    pub pending: Vec<PendingDownlink>,
    pub seq_counter: u64,
}

impl DownlinkPipeline {
    pub fn record_order_churn(&mut self, commanders: &mut CommanderSoA, cid: CommanderId, line_pts: &[[f32; 2]], clock_hours: f32) -> bool {
        let i = cid.0;
        let (deadzone_hit, sig) = if let Some(prev) = commanders.assigned_parent_line_pts.get(i).filter(|v| v.len() >= 2) {
            if line_pts.len() >= 2 {
                let prev_mid = [(prev[0][0] + prev.last().unwrap()[0]) * 0.5, (prev[0][1] + prev.last().unwrap()[1]) * 0.5];
                let new_mid = [(line_pts[0][0] + line_pts.last().unwrap()[0]) * 0.5, (line_pts[0][1] + line_pts.last().unwrap()[1]) * 0.5];
                let shift = (prev_mid[0] - new_mid[0]).hypot(prev_mid[1] - new_mid[1]);
                let a_dir = [prev.last().unwrap()[0] - prev[0][0], prev.last().unwrap()[1] - prev[0][1]];
                let b_dir = [line_pts.last().unwrap()[0] - line_pts[0][0], line_pts.last().unwrap()[1] - line_pts[0][1]];
                let la = a_dir[0].hypot(a_dir[1]).max(1e-5);
                let lb = b_dir[0].hypot(b_dir[1]).max(1e-5);
                let cos_a = ((a_dir[0] * b_dir[0] + a_dir[1] * b_dir[1]) / (la * lb)).clamp(-1.0, 1.0);
                let ang_deg = cos_a.acos().to_degrees();
                (shift < 2.0 && ang_deg < 15.0, ((new_mid[0] * 100.0) as u64) ^ (((new_mid[1] * 100.0) as u64) << 16))
            } else {
                (false, 1)
            }
        } else {
            (false, 1)
        };
        if deadzone_hit {
            return false; // Gate 3: deadzone filter suppresses jitter!
        }

        let cooldown_h = commanders.echelon[i].hard_cooldown_min() / 60.0;
        if clock_hours - commanders.last_auto_order_clock_hours[i] < cooldown_h {
            commanders.order_churn_score[i] += 1.0;
        } else {
            commanders.order_churn_score[i] = (commanders.order_churn_score[i] * 0.5).max(0.0);
        }
        commanders.last_auto_order_clock_hours[i] = clock_hours;
        commanders.last_order_sig[i] = sig;
        true
    }

    pub fn queue_downlink_to_connected(
        &mut self,
        parent_cid: CommanderId,
        target_cid: CommanderId,
        clock_hours: f32,
        role: FrontlineRole,
        unit_type_mask: u8,
        points_px: Vec<[f32; 2]>,
        front_normal: [f32; 2],
        commanders: &CommanderSoA,
        units: &UnitSoA,
        grid: &WorldGrid,
    ) -> bool {
        if parent_cid != target_cid
            && !validate_commander_link(parent_cid, target_cid, commanders, units, grid).is_connected()
        {
            return false;
        }
        let mods = aggregate_modifiers(
            &commanders.modifier_slots[parent_cid.0],
            commanders.order_churn_score[parent_cid.0],
            commanders.is_war_council_sync_active(parent_cid, grid),
        );
        let delay_min = commanders.echelon[parent_cid.0].propagation_delay_min() * mods.tau_mult;
        self.seq_counter += 1;
        let seq = self.seq_counter;
        self.pending.push(PendingDownlink {
            deliver_at_hours: clock_hours + delay_min / 60.0,
            parent_cid,
            target_cid,
            line_name: format!("dl-{}-{}", commanders.str_id[target_cid.0], seq),
            role,
            unit_type_mask,
            points_px,
            front_normal,
            source_deploy_line_id: 0,
            target_deploy_line_id: 0,
        });
        true
    }

    pub fn tick_deliver(
        &mut self,
        clock_hours: f32,
        grid: &mut WorldGrid,
        commanders: &mut CommanderSoA,
    ) -> usize {
        let mut delivered = 0usize;
        let mut keep = Vec::with_capacity(self.pending.len());
        let old = std::mem::take(&mut self.pending);
        for item in old {
            if item.deliver_at_hours <= clock_hours + 1e-6 {
                let ci = item.target_cid.0;
                if commanders.alive[ci] {
                    // Check if previous assigned line center is > 3px away -> emit bridging MOVE_LINE!
                    let mut bridge_pts: Option<([f32; 2], [f32; 2])> = None;
                    if item.role == FrontlineRole::Deploy && commanders.assigned_parent_line_pts[ci].len() >= 2 && item.points_px.len() >= 2 {
                        let prev = &commanders.assigned_parent_line_pts[ci];
                        let old_mid = [(prev[0][0] + prev.last().unwrap()[0]) * 0.5, (prev[0][1] + prev.last().unwrap()[1]) * 0.5];
                        let new_mid = [(item.points_px[0][0] + item.points_px.last().unwrap()[0]) * 0.5, (item.points_px[0][1] + item.points_px.last().unwrap()[1]) * 0.5];
                        if (old_mid[0] - new_mid[0]).hypot(old_mid[1] - new_mid[1]) > 3.0 {
                            bridge_pts = Some((old_mid, new_mid));
                        }
                    }

                    let team = commanders.team[ci];
                    let new_deploy_id = grid.write_typed_frontline(
                        &item.line_name,
                        team,
                        ci,
                        0,
                        item.role,
                        item.source_deploy_line_id,
                        item.target_deploy_line_id,
                        item.unit_type_mask,
                        1.0,
                        Some(item.front_normal),
                        &item.points_px,
                        4,
                    );
                    if let Some((a, b)) = bridge_pts {
                        let bname = format!("bridge-{}", item.line_name);
                        grid.write_typed_frontline(
                            &bname,
                            team,
                            ci,
                            0,
                            FrontlineRole::Move,
                            0,
                            new_deploy_id,
                            item.unit_type_mask,
                            1.25,
                            Some(item.front_normal),
                            &[a, b],
                            6,
                        );
                    }
                    commanders.assigned_parent_line_pts[ci] = item.points_px;
                    commanders.parent_order_seq[ci] += 1;
                    delivered += 1;
                }
            } else {
                keep.push(item);
            }
        }
        self.pending = keep;
        delivered
    }
}

pub fn update_war_councils(
    commanders: &mut CommanderSoA,
    units: &mut UnitSoA,
    grid: &mut WorldGrid,
    dt_hours: f32,
) -> (usize, usize) {
    let dt = dt_hours.max(0.0);
    let radius_km = 1.0f32;
    let req_hours = 12.0f32;
    let min_prep_hours = 6.0f32;
    let eff_buff_hours = 18.0f32;

    for i in 0..commanders.id.len() {
        if !commanders.alive[i] {
            commanders.still_hours[i] = 0.0;
            continue;
        }
        if commanders.last_px[i] == commanders.px[i] {
            commanders.still_hours[i] += dt;
        } else {
            commanders.still_hours[i] = 0.0;
            commanders.last_px[i] = commanders.px[i];
        }
        if commanders.council_cooldown_hours[i] > 0.0 {
            commanders.council_cooldown_hours[i] = (commanders.council_cooldown_hours[i] - dt).max(0.0);
        }
        if commanders.briefing_buff_hours[i] > 0.0 {
            commanders.briefing_buff_hours[i] = (commanders.briefing_buff_hours[i] - dt).max(0.0);
        }
        if commanders.council_ao_hours[i] > 0.0 {
            commanders.council_ao_hours[i] = (commanders.council_ao_hours[i] - dt).max(0.0);
        }
        if commanders.war_council_sync_hours[i] > 0.0 {
            commanders.war_council_sync_hours[i] = (commanders.war_council_sync_hours[i] - dt).max(0.0);
        }
        if commanders.post_council_exposure_hours[i] > 0.0 {
            commanders.post_council_exposure_hours[i] = (commanders.post_council_exposure_hours[i] - dt).max(0.0);
        }
    }

    let mut active_pairs = 0usize;
    let mut completed_count = 0usize;
    let n_cmd = commanders.id.len();
    for sup in 0..n_cmd {
        if !commanders.alive[sup] || commanders.still_hours[sup] + 1e-6 < min_prep_hours {
            continue;
        }
        for sub in 0..n_cmd {
            if sup == sub || !commanders.alive[sub] || commanders.team[sub] != commanders.team[sup] {
                continue;
            }
            if commanders.still_hours[sub] + 1e-6 < min_prep_hours {
                continue;
            }
            let is_pair = commanders.parent_commander_id[sub] == Some(CommanderId(sup))
                || ((commanders.rank[sup] as u8) < (commanders.rank[sub] as u8)
                    && commanders.echelon[sup] != commanders.echelon[sub]);
            if !is_pair {
                continue;
            }
            let p_sup = commanders.px[sup];
            let p_sub = commanders.px[sub];
            let d_km = grid.distance_km([p_sup.0 as f32, p_sup.1 as f32], [p_sub.0 as f32, p_sub.1 as f32]);
            if d_km > radius_km {
                continue;
            }
            active_pairs += 1;
            let dist_px = ((p_sup.0 - p_sub.0) as f32).hypot((p_sup.1 - p_sub.1) as f32);
            let cr = ((p_sup.0 + p_sub.0) as f32 * 0.5).round() as i32;
            let cc = ((p_sup.1 + p_sub.1) as f32 * 0.5).round() as i32;
            let exp_rad = 2.0 + 1.5 * (dist_px / 5.0).min(1.0);
            let buff_rad = (dist_px * 0.5).max(0.5);
            let streak = commanders.council_streak[sup].max(commanders.council_streak[sub]);
            let step_exp = 4.0 * 2.5f32.powi(streak as i32) * (dt / 6.0).min(1.0);
            grid.add_exposure(cr, cc, step_exp, exp_rad);

            // Real-time mutual WAR_COUNCIL_SYNC_BUFF (retaining any existing order_churn!)
            commanders.council_partner_id[sup] = Some(CommanderId(sub));
            commanders.council_partner_id[sub] = Some(CommanderId(sup));
            commanders.council_radius_km[sup] = radius_km;
            commanders.council_radius_km[sub] = radius_km;
            commanders.war_council_sync_hours[sup] = commanders.war_council_sync_hours[sup].max(eff_buff_hours);
            commanders.war_council_sync_hours[sub] = commanders.war_council_sync_hours[sub].max(eff_buff_hours);
            grid.add_exposure(p_sup.0, p_sup.1, step_exp * 0.5, 0.0);
            grid.add_exposure(p_sub.0, p_sub.1, step_exp * 0.5, 0.0);

            if commanders.still_hours[sup] + 1e-6 >= req_hours && commanders.still_hours[sub] + 1e-6 >= req_hours {
                commanders.still_hours[sup] = 0.0;
                commanders.still_hours[sub] = 0.0;
                commanders.council_streak[sup] += 1;
                commanders.council_streak[sub] += 1;
                commanders.council_cooldown_hours[sup] = 24.0;
                commanders.council_cooldown_hours[sub] = 24.0;
                commanders.council_completed[sup] = true;
                commanders.council_completed[sub] = true;
                let ns = commanders.council_streak[sup].max(commanders.council_streak[sub]);
                let post_val = 8.0 * 1.8f32.powi((ns - 1) as i32);
                for ci in [sup, sub] {
                    commanders.post_council_exposure_hours[ci] = commanders.post_council_exposure_hours[ci].max(12.0);
                    commanders.post_council_exposure_val[ci] = commanders.post_council_exposure_val[ci].max(post_val);
                    commanders.council_ao_hours[ci] = commanders.council_ao_hours[ci].max(eff_buff_hours);
                }
                commanders.briefing_buff_hours[sub] = commanders.briefing_buff_hours[sub].max(eff_buff_hours);

                let team = commanders.team[sup];
                for uid in 0..units.id.len() {
                    if units.alive[uid] && units.team[uid] == team {
                        let d = ((units.px[uid].0 - cr) as f32).hypot((units.px[uid].1 - cc) as f32);
                        if d <= buff_rad {
                            units.org[uid] = (units.org[uid] + 15.0).min(120.0);
                        }
                    }
                }
                completed_count += 1;
            }
        }
    }
    (active_pairs, completed_count)
}

pub fn radio_channel_idx(ch: &str) -> u8 {
    let up = ch.trim().to_ascii_uppercase();
    RADIO_CHANNELS.iter().position(|&c| c == up).unwrap_or(0) as u8
}
