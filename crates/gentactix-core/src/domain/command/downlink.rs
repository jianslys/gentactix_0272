use super::*;
use crate::units_ecs::{aggregate_modifiers, AlertMark, AtomicAction, CommanderId, CommanderSoA, Echelon, UnitId, UnitSoA, RADIO_CHANNELS};
use crate::world_grid::{FrontlineRole, WorldGrid, FOREST, LINE_MASK_ALL, WATER};
use serde::{Deserialize, Serialize};

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
