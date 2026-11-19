use super::*;
use crate::command_log::validate_command_link;
use crate::units_ecs::{CommanderId, CommanderSoA, UnitId, UnitSoA};
use crate::world_grid::{TeamId, WorldGrid, AIRFIELD, FOG, ICE, SNOW, WATER};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelContact {
    pub unit_id: UnitId,
    pub px: (i32, i32),
    pub fog_level: u8,
    pub facing_deg: Option<f32>, // Hidden for infantry, exposed only for vehicles at level >= 2
    pub est_persons: u32,
}

pub fn gather_intel(
    observer_team: TeamId,
    grid: &WorldGrid,
    units: &UnitSoA,
    night: bool,
) -> Vec<IntelContact> {
    let mut contacts = Vec::new();
    let n = units.id.len();
    for eid in 0..n {
        if !units.alive[eid] || units.team[eid] == observer_team || units.persons(UnitId(eid)) == 0 {
            continue;
        }
        let e_pos = units.pos[eid];
        let e_px = units.px[eid];
        let e_idx = grid.idx(e_px.0, e_px.1);
        let mut best_lvl = 0u8;

        if grid.exposure[e_idx] >= SKY_EYE_EXPOSURE_THRESHOLD {
            best_lvl = best_lvl.max(2);
        }

        for fid in 0..n {
            if !units.alive[fid] || units.team[fid] != observer_team || units.persons(UnitId(fid)) == 0 {
                continue;
            }
            let d_km = grid.distance_km(units.pos[fid], e_pos);
            let obs_range = BASE_OBSERVE_KM + if units.kind[fid] == "recon_squad" { RECON_BONUS_KM } else { 0.0 };
            if d_km <= obs_range {
                let los = cast_los_ray_dda(grid, units.px[fid], e_px, night);
                if !los.blocked && los.visibility > 0.15 {
                    best_lvl = best_lvl.max(if d_km <= 0.6 { 3 } else { 2 });
                }
            } else if !units.radio_silence[eid] && d_km <= DF_RANGE_KM {
                best_lvl = best_lvl.max(1);
            }
        }

        if best_lvl > 0 {
            let blur_km: i32 = match best_lvl {
                1 => 4,
                2 => 1,
                _ => 0,
            };
            let _ = blur_km;
            let facing_deg = if best_lvl >= 2 && units.is_directional_hull(UnitId(eid)) {
                Some(units.facing_deg[eid])
            } else {
                None
            };
            contacts.push(IntelContact {
                unit_id: UnitId(eid),
                px: e_px,
                fog_level: best_lvl,
                facing_deg,
                est_persons: units.persons(UnitId(eid)),
            });
        }
    }
    contacts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellType {
    He,
    Ap,
    Heat,
    Wp,
    Smoke,
    Illum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub str_id: String,
    pub team: TeamId,
    pub px: (i32, i32),
    pub caliber_mm: u32,
    pub tubes: u32,
    pub shells: [f32; 6],
}

#[derive(Debug, Clone, Default)]
pub struct FireFields {
    pub hazard_by_cell_red: FxHashMap<u32, f32>,
    pub hazard_by_cell_blue: FxHashMap<u32, f32>,
}

impl FireFields {
    pub fn add_barrage(&mut self, grid: &WorldGrid, center: (i32, i32), radius_px: i32, attacker: TeamId, intensity: f32) {
        let victim = attacker.enemy();
        let target_map = match victim {
            TeamId::Red => &mut self.hazard_by_cell_red,
            TeamId::Blue => &mut self.hazard_by_cell_blue,
        };
        let r2 = radius_px * radius_px;
        for dr in -radius_px..=radius_px {
            for dc in -radius_px..=radius_px {
                if dr * dr + dc * dc <= r2 && grid.in_bounds(center.0 + dr, center.1 + dc) {
                    let idx = grid.idx(center.0 + dr, center.1 + dc) as u32;
                    *target_map.entry(idx).or_insert(0.0) += intensity;
                }
            }
        }
    }

    pub fn hazard_at(&self, team: TeamId, idx: u32) -> f32 {
        match team {
            TeamId::Red => self.hazard_by_cell_red.get(&idx).copied().unwrap_or(0.0),
            TeamId::Blue => self.hazard_by_cell_blue.get(&idx).copied().unwrap_or(0.0),
        }
    }
}

pub fn execute_artillery_mission(
    grid: &mut WorldGrid,
    units: &mut UnitSoA,
    commanders: &CommanderSoA,
    events: &mut crate::units_ecs::EventQueue,
    fire_fields: &mut FireFields,
    battery: &mut Battery,
    target_px: (i32, i32),
    shell: ShellType,
    rounds: u32,
    observed: bool,
    radius_px: i32,
    seed_tag: &str,
) -> Option<StrikeReport> {
    let si = shell as usize;
    let avail = battery.shells[si].floor() as u32;
    let fired = rounds.min(avail);
    if fired == 0 {
        return None;
    }
    battery.shells[si] -= fired as f32;
    grid.add_exposure(battery.px.0, battery.px.1, (fired as f32) * 0.4, 1.0);

    match shell {
        ShellType::Smoke => {
            for dr in -radius_px..=radius_px {
                for dc in -radius_px..=radius_px {
                    if dr * dr + dc * dc <= radius_px * radius_px {
                        grid.apply_smoke(target_px.0 + dr, target_px.1 + dc, 0.75, 0.5);
                    }
                }
            }
            None
        }
        ShellType::Illum => {
            for dr in -radius_px..=radius_px {
                for dc in -radius_px..=radius_px {
                    if dr * dr + dc * dc <= radius_px * radius_px {
                        grid.apply_illumination(target_px.0 + dr, target_px.1 + dc, 0.5);
                    }
                }
            }
            None
        }
        _ => {
            let obs_mult = if observed { 1.0 } else { UNOBSERVED_FACTOR };
            let cal_scale = (battery.caliber_mm as f32 / 105.0).max(0.5);
            let pts = fired as f32 * cal_scale * obs_mult * 1.4;
            let fire = match shell {
                ShellType::He => [pts, pts * 1.2, pts * 0.5, pts * 0.2, pts * 0.7, 0.0],
                ShellType::Wp => [pts * 0.6, pts * 1.1, pts * 0.3, 0.0, pts * 0.2, pts * 1.3],
                ShellType::Heat | ShellType::Ap => [pts * 0.2, pts * 0.3, pts * 0.8, pts * 1.6, pts * 0.3, 0.0],
                _ => [0.0; 6],
            };
            fire_fields.add_barrage(grid, target_px, radius_px.max(1), battery.team, pts * 0.08);
            let w_model = if battery.caliber_mm >= 140 { "howitzer_152" } else { "howitzer_105" };
            let rep = resolve_cell_strike(
                grid,
                units,
                commanders,
                events,
                target_px,
                battery.team,
                fire,
                w_model,
                [battery.px.0 as f32, battery.px.1 as f32],
                false,
                seed_tag,
            );
            Some(rep)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirRole {
    Fighter,
    DiveBomber,
    TacticalBomber,
    Recon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirWing {
    pub str_id: String,
    pub team: TeamId,
    pub role: AirRole,
    pub airfield_px: (i32, i32),
    pub radio_channel: u8,
    pub air_radio_radius_km: f32,
    pub readiness: f32,
    pub aircraft: u32,
}

pub fn can_receive_air_order(
    wing: &AirWing,
    commander: &CommanderSoA,
    cmd_id: CommanderId,
    units: &UnitSoA,
    grid: &WorldGrid,
) -> bool {
    if !commander.alive[cmd_id.0] || commander.team[cmd_id.0] != wing.team {
        return false;
    }
    let c_px = commander.px[cmd_id.0];
    let d_km = grid.distance_km([c_px.0 as f32, c_px.1 as f32], [wing.airfield_px.0 as f32, wing.airfield_px.1 as f32]);
    if d_km <= 0.4001 || d_km <= wing.air_radio_radius_km {
        if !commander.radio_silence[cmd_id.0] && commander.radio_channel[cmd_id.0] == wing.radio_channel {
            return true;
        }
    }
    // Check air-ground liaison team (`fac_radio` or `hq_radio` matching wing channel)
    for uid in 0..units.id.len() {
        if units.alive[uid]
            && units.team[uid] == wing.team
            && !units.radio_silence[uid]
            && units.radio_channel[uid] == wing.radio_channel
            && units.equipment[uid].get("hq_radio").copied().unwrap_or(0) > 0
            && validate_command_link(cmd_id, UnitId(uid), commander, units, grid).is_connected()
        {
            return true;
        }
    }
    false
}

pub fn run_air_subtick_1h(wings: &mut [AirWing], grid: &WorldGrid) -> usize {
    let mut sorties = 0usize;
    for w in wings.iter_mut() {
        let ai = grid.idx(w.airfield_px.0, w.airfield_px.1);
        let wx = grid.weather[ai];
        if matches!(wx, FOG | SNOW | ICE) || grid.terrain[ai] == WATER {
            continue; // Grounded in severe weather or destroyed airfield!
        }
        if grid.terrain[ai] == AIRFIELD && w.aircraft > 0 && w.readiness > 0.25 {
            sorties += w.aircraft as usize;
        }
    }
    sorties
}
