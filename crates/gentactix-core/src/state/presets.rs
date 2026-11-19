use crate::combat_logistics::Depot;
use crate::command_log::radio_channel_idx;
use crate::state::world_state::SimWorld;
use crate::units_ecs::{Echelon, Rank};
use crate::world_grid::{WorldGrid, TeamId, BRIDGE, DIRT_ROAD, FOREST, HIGHWAY, MOUNTAIN, PLAIN, WATER};
use rustc_hash::FxHashMap;
use std::path::Path;

pub fn find_passable_cell(grid: &WorldGrid, pref_r: i32, pref_c: i32) -> (i32, i32) {
    let (cr, cc) = grid.clamp_rc(pref_r, pref_c);
    let ok = |r: i32, c: i32| -> bool {
        grid.in_bounds(r, c) && matches!(grid.terrain[grid.idx(r, c)], PLAIN | FOREST)
    };
    if ok(cr, cc) {
        return (cr, cc);
    }
    for rad in 1..25 {
        let mut best: Option<(i32, i32, i32)> = None;
        for r in (cr - rad)..=(cr + rad) {
            for c in (cc - rad)..=(cc + rad) {
                if ok(r, c) {
                    let d2 = (r - cr).pow(2) + (c - cc).pow(2);
                    if best.map_or(true, |b| d2 < b.2) {
                        best = Some((r, c, d2));
                    }
                }
            }
        }
        if let Some((r, c, _)) = best {
            return (r, c);
        }
    }
    (cr, cc)
}

fn eq_map(items: &[(&str, u32)]) -> FxHashMap<String, u32> {
    let mut m = FxHashMap::default();
    for &(k, v) in items {
        m.insert(k.to_string(), v);
    }
    m
}

pub fn build_r16_platoon_world(folder: &Path, tile_xy: (u32, u32), flow_threshold: u16) -> Result<SimWorld, String> {
    let (tx, ty) = tile_xy;
    let r16_p = folder.join(format!("1100_x{tx}_y{ty}.r16"));
    let diff_p = folder.join(format!("diffuse_x0_y0_x{tx}_y{ty}.png"));
    let riv_p = folder.join(format!("river_x{tx}_y{ty}.png"));
    let mut grid = WorldGrid::from_r16_diffuse_river(
        &r16_p,
        if diff_p.exists() { Some(diff_p.as_path()) } else { None },
        if riv_p.exists() { Some(riv_p.as_path()) } else { None },
        flow_threshold,
    )?;

    let (ar, ac) = find_passable_cell(&grid, 696, 2700);
    let pos_sq1 = find_passable_cell(&grid, ar, ac - 1);
    let pos_sq2 = find_passable_cell(&grid, ar, ac + 1);
    let pos_sq3 = find_passable_cell(&grid, ar + 1, ac - 1);
    let pos_stash = find_passable_cell(&grid, ar - 1, ac - 2);
    let pos_blue = find_passable_cell(&grid, ar - 1, ac + 4);

    grid.stash_put_resource(pos_stash.0, pos_stash.1, Some(TeamId::Red), 1.8, 2.5, 0.2, 0.0, Some(radio_channel_idx("A")), 1);
    grid.stash_put_equipment(pos_stash.0, pos_stash.1, Some(TeamId::Red), Some(radio_channel_idx("A")), &[("radio".to_string(), 1)]);

    let mut w = SimWorld::new(grid);
    w.role_echelon = Echelon::Platoon;
    w.r16_source = Some(r16_p.to_string_lossy().to_string());

    let u1 = w.units.spawn_unit("red-sq-1", "红1排1班(步兵)", TeamId::Red, pos_sq1, Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u2 = w.units.spawn_unit("red-sq-2", "红1排2班(步兵)", TeamId::Red, pos_sq2, Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u3 = w.units.spawn_unit("red-sq-3", "红1排3班(步兵)", TeamId::Red, pos_sq3, Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let umg = w.units.spawn_unit("red-sq-mg", "红1排机炮火力班", TeamId::Red, (ar, ac), Echelon::Squad, "mg_squad", 7, 1, eq_map(&[("hmg", 1), ("mortar_81", 1), ("rifle", 4), ("radio", 1)]), 0);
    let _ub = w.units.spawn_unit("blue-recon-1", "蓝方前出警戒班", TeamId::Blue, pos_blue, Echelon::Squad, "recon_squad", 6, 1, eq_map(&[("rifle", 5), ("radio", 1)]), 1);

    let cmd = w.commanders.spawn_commander("cmd-pl-1", "红1排排长", TeamId::Red, (ar, ac), Echelon::Platoon, Rank::Lieutenant, 0);
    for u in [u1, u2, u3, umg] {
        w.commanders.link_subordinate_unit(cmd, u, &mut w.units);
    }

    w.depots.push(Depot {
        str_id: "depot-red-1".into(),
        team: TeamId::Red,
        px: pos_stash,
        food_t: 15.0,
        ammo_t: 25.0,
        fuel_t: 10.0,
        replacements: 20.0,
    });

    Ok(w)
}

pub fn build_demo_world() -> SimWorld {
    let grid = WorldGrid::synthetic_grid(64, 64, 200.0);
    let mut w = SimWorld::new(grid);
    let cmd_co = w.commanders.spawn_commander("cmd-co-1", "红1连连长", TeamId::Red, (24, 20), Echelon::Company, Rank::Captain, 0);
    let cmd_pl = w.commanders.spawn_commander("cmd-pl-1", "红1排排长", TeamId::Red, (24, 24), Echelon::Platoon, Rank::Lieutenant, 0);
    w.commanders.link_subordinate_cmd(cmd_co, cmd_pl);

    let u1 = w.units.spawn_unit("red-sq-1", "红1排1班", TeamId::Red, (23, 25), Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u2 = w.units.spawn_unit("red-sq-2", "红1排2班", TeamId::Red, (25, 25), Echelon::Squad, "rifle_squad", 10, 1, eq_map(&[("rifle", 9), ("lmg", 1)]), 0);
    let u_tk = w.units.spawn_unit("red-tk-1", "红坦支援组", TeamId::Red, (24, 24), Echelon::Squad, "tank_squad", 4, 1, eq_map(&[("tank_medium", 1), ("radio", 1)]), 0);
    let _ub = w.units.spawn_unit("blue-sq-1", "蓝方警戒班", TeamId::Blue, (24, 30), Echelon::Squad, "rifle_squad", 8, 1, eq_map(&[("rifle", 8), ("radio", 1)]), 1);
    let _ub2 = w.units.spawn_unit("blue-tk-2", "蓝方掩护组", TeamId::Blue, (18, 50), Echelon::Squad, "tank_squad", 4, 1, eq_map(&[("tank_medium", 1)]), 1);

    for u in [u1, u2, u_tk] {
        w.commanders.link_subordinate_unit(cmd_pl, u, &mut w.units);
    }
    w.depots.push(Depot {
        str_id: "depot-red-1".into(),
        team: TeamId::Red,
        px: (24, 18),
        food_t: 20.0,
        ammo_t: 30.0,
        fuel_t: 20.0,
        replacements: 30.0,
    });
    w
}

fn raster_road(grid: &mut WorldGrid, p1: (i32, i32), p2: (i32, i32), half_w: i32, t: u8) {
    let (mut r0, mut c0) = p1;
    let (r1, c1) = p2;
    let dr = (r1 - r0).abs();
    let dc = (c1 - c0).abs();
    let sr = if r0 < r1 { 1 } else { -1 };
    let sc = if c0 < c1 { 1 } else { -1 };
    let mut err = dr - dc;

    loop {
        for dr_off in -half_w..=half_w {
            for dc_off in -half_w..=half_w {
                let nr = r0 + dr_off;
                let nc = c0 + dc_off;
                if nr >= 0 && nr < grid.rows as i32 && nc >= 0 && nc < grid.cols as i32 {
                    let gi = grid.idx(nr, nc);
                    if grid.terrain[gi] != WATER && grid.terrain[gi] != BRIDGE {
                        grid.terrain[gi] = t;
                        grid.base_terrain[gi] = t;
                    }
                }
            }
        }
        if r0 == r1 && c0 == c1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dc {
            err -= dc;
            r0 += sr;
        }
        if e2 < dr {
            err += dr;
            c0 += sc;
        }
    }
}

/// 512x512 Birch River MVP World (4096m x 4096m, dx = 8.0m)
/// Deterministic scenario: Birch River meander, central highway bridge (OBJ. A),
/// north shallow ford, Hill 184 & Hill 216, continuous forest coverage,
/// Red 1st Reinforced Company (10+ units) vs Blue Bridgehead Defense Company.
pub fn build_birch_river_512_world(_seed: u64) -> SimWorld {
    let mut grid = WorldGrid::new_plain(512, 512, 8.0);

    // 1. Multi-frequency continuous elevation & water/bridge/mountain terrain
    for r in 0..512i32 {
        let r_f = r as f32;
        let river_c_center = 276.0 + 28.0 * (r_f / 56.0).sin() + 12.0 * (r_f / 29.0).cos();
        let river_c_mid = river_c_center.round() as i32;

        for c in 0..512i32 {
            let c_f = c as f32;
            let gi = grid.idx(r, c);

            // Base smooth elevation
            let mut elev = 135.0 + 3.2 * ((r_f * 0.035).sin() + (c_f * 0.028).cos());

            // Hill 184 at (195, 155), peak ~ 184m
            let d184_sq = (r_f - 195.0) * (r_f - 195.0) + (c_f - 155.0) * (c_f - 155.0);
            let h184 = 49.0 * (-d184_sq / (2.0 * 36.0 * 36.0)).exp();

            // Hill 216 at (310, 355), peak ~ 216m
            let d216_sq = (r_f - 310.0) * (r_f - 310.0) + (c_f - 355.0) * (c_f - 355.0);
            let h216 = 81.0 * (-d216_sq / (2.0 * 42.0 * 42.0)).exp();

            // North ridge at (45..80, 290..340), elevation > 220m
            let ridge_dr = (r_f - 62.0) / 22.0;
            let ridge_dc = (c_f - 315.0) / 30.0;
            let ridge_d_sq = ridge_dr * ridge_dr + ridge_dc * ridge_dc;
            let h_ridge = if ridge_d_sq < 4.0 {
                105.0 * (-ridge_d_sq * 0.5).exp()
            } else {
                0.0
            };

            elev += h184 + h216 + h_ridge;

            // River morphology
            let dist_to_river = (c - river_c_mid).abs();
            let is_river = dist_to_river <= 6;

            // Central Highway Bridge ("OBJ. A / 白桦河大桥") at r in 246..=250 across river
            let is_bridge = r >= 246 && r <= 250 && dist_to_river <= 8;

            // North shallow ford at r in 118..=121 across river
            let is_ford = r >= 118 && r <= 121 && dist_to_river <= 7;

            if is_bridge {
                grid.terrain[gi] = BRIDGE;
                grid.base_terrain[gi] = BRIDGE;
                elev = 125.0;
            } else if is_ford {
                grid.terrain[gi] = DIRT_ROAD;
                grid.base_terrain[gi] = DIRT_ROAD;
                elev = 114.0;
            } else if is_river {
                grid.terrain[gi] = WATER;
                grid.base_terrain[gi] = WATER;
                elev = 112.0 + (dist_to_river as f32) * 0.7;
            } else if elev >= 220.0 {
                grid.terrain[gi] = MOUNTAIN;
                grid.base_terrain[gi] = MOUNTAIN;
            } else {
                grid.terrain[gi] = PLAIN;
                grid.base_terrain[gi] = PLAIN;
            }

            grid.elevation_m[gi] = elev;
            grid.raw_u16[gi] = elev.round().clamp(0.0, 65535.0) as u16;
        }
    }

    // 2. Continuous Forest coverage
    for r in 0..512i32 {
        let r_f = r as f32;
        for c in 0..512i32 {
            let c_f = c as f32;
            let gi = grid.idx(r, c);
            if grid.terrain[gi] == WATER || grid.terrain[gi] == BRIDGE || grid.terrain[gi] == MOUNTAIN {
                continue;
            }

            // Northern forest zone: r in [20, 140], c in [50, 220]
            let in_north_forest = r >= 20 && r <= 140 && c >= 50 && c <= 220;
            // Hill 216 reverse slope forest zone: r in [260, 400], c in [380, 480]
            let in_hill_forest = r >= 260 && r <= 400 && c >= 380 && c <= 480;

            if in_north_forest {
                let dr = (r_f - 80.0) / 45.0;
                let dc = (c_f - 135.0) / 65.0;
                let d_norm = (1.0 - (dr * dr + dc * dc)).clamp(0.0, 1.0);
                let density = 0.45 + 0.45 * d_norm;
                if density >= 0.35 {
                    grid.set_forest_density(r, c, density);
                }
            } else if in_hill_forest {
                let dr = (r_f - 330.0) / 55.0;
                let dc = (c_f - 430.0) / 40.0;
                let d_norm = (1.0 - (dr * dr + dc * dc)).clamp(0.0, 1.0);
                let density = 0.40 + 0.50 * d_norm;
                if density >= 0.35 {
                    grid.set_forest_density(r, c, density);
                }
            }
        }
    }

    // 3. Road Network
    // East-West Highway
    raster_road(&mut grid, (260, 40), (248, 175), 1, HIGHWAY);
    raster_road(&mut grid, (248, 175), (248, 267), 1, HIGHWAY);
    raster_road(&mut grid, (248, 285), (236, 385), 1, HIGHWAY);
    raster_road(&mut grid, (236, 385), (230, 480), 1, HIGHWAY);

    // North-South Dirt Roads (connecting through shallow ford)
    raster_road(&mut grid, (60, 180), (120, 190), 0, DIRT_ROAD);
    raster_road(&mut grid, (120, 190), (120, 260), 0, DIRT_ROAD);
    raster_road(&mut grid, (120, 280), (140, 350), 0, DIRT_ROAD);
    raster_road(&mut grid, (120, 190), (248, 175), 0, DIRT_ROAD);
    raster_road(&mut grid, (248, 175), (380, 210), 0, DIRT_ROAD);

    // Synchronize feature flags & attrition
    grid.sync_feature_flags_from_terrain();
    grid.refresh_attrition();

    let mut w = SimWorld::new(grid);
    w.role_echelon = Echelon::Company;

    // 4. Red Force (1v1 Infantry Squad Sandbox: strictly 1 Red Infantry Squad)
    let cmd_co = w.commanders.spawn_commander(
        "cmd-co-1",
        "红1连连部",
        TeamId::Red,
        (246, 210),
        Echelon::Company,
        Rank::Captain,
        0,
    );
    let cmd_pl1 = w.commanders.spawn_commander(
        "cmd-pl-1",
        "红1排排部",
        TeamId::Red,
        (246, 230),
        Echelon::Platoon,
        Rank::Lieutenant,
        0,
    );
    w.commanders.link_subordinate_cmd(cmd_co, cmd_pl1);

    let u_1pl_1 = w.units.spawn_unit(
        "red-1pl-sq1",
        "红1排1班(步兵)",
        TeamId::Red,
        (248, 224),
        Echelon::Squad,
        "rifle_squad",
        9,
        1,
        eq_map(&[("rifle", 9), ("lmg", 1), ("radio", 1)]),
        0,
    );
    w.commanders.link_subordinate_unit(cmd_pl1, u_1pl_1, &mut w.units);

    // Red Depot
    w.depots.push(Depot {
        str_id: "depot-red-1".into(),
        team: TeamId::Red,
        px: (248, 160),
        food_t: 50.0,
        ammo_t: 80.0,
        fuel_t: 40.0,
        replacements: 50.0,
    });

    // 5. Blue Force (1v1 Infantry Squad Sandbox: strictly 1 Blue Infantry Squad)
    let cmd_bco = w.commanders.spawn_commander(
        "cmd-blue-co",
        "蓝桥头防御连部",
        TeamId::Blue,
        (248, 320),
        Echelon::Company,
        Rank::Captain,
        1,
    );
    let cmd_bpl1 = w.commanders.spawn_commander(
        "cmd-blue-pl1",
        "蓝桥东前沿排部",
        TeamId::Blue,
        (248, 280),
        Echelon::Platoon,
        Rank::Lieutenant,
        1,
    );
    w.commanders.link_subordinate_cmd(cmd_bco, cmd_bpl1);

    let ub_outpost = w.units.spawn_unit(
        "blue-outpost-1",
        "蓝桥头1班(步兵)",
        TeamId::Blue,
        (248, 260),
        Echelon::Squad,
        "rifle_squad",
        9,
        1,
        eq_map(&[("rifle", 9), ("lmg", 1), ("radio", 1)]),
        1,
    );
    w.commanders.link_subordinate_unit(cmd_bpl1, ub_outpost, &mut w.units);

    w
}

/// 16x16 Art Test World: 8m pixel container PLAIN base with MOUNTAIN, WATER & scattered FOREST tree-line placements.
pub fn build_art_16_world() -> SimWorld {
    let mut grid = WorldGrid::new_plain(16, 16, 8.0);
    // 1. Mountain showcase (deep cold iron black #07080c):
    let mtn_cells = [(0, 0), (0, 1), (1, 0), (1, 1)];
    for (r, c) in mtn_cells {
        let gi = grid.idx(r, c);
        grid.terrain[gi] = MOUNTAIN;
        grid.base_terrain[gi] = MOUNTAIN;
        grid.elevation_m[gi] = 320.0;
        grid.raw_u16[gi] = 320;
    }
    // 2. Water showcase (deep navy #163a6c):
    let water_cells = [(14, 14), (14, 15), (15, 14), (15, 15)];
    for (r, c) in water_cells {
        let gi = grid.idx(r, c);
        grid.terrain[gi] = WATER;
        grid.base_terrain[gi] = WATER;
    }
    // 3. Continuous forest density samples (0.0..=1.0): sparse -> medium -> dense
    let tree_cells: [(i32, i32, f32); 16] = [
        (2, 3, 0.95), (2, 4, 0.85), (3, 3, 0.78), (3, 4, 0.65),
        (4, 11, 0.30), (4, 12, 0.25), (5, 11, 0.45), (6, 6, 0.55),
        (8, 2, 0.90), (9, 2, 1.00), (9, 3, 0.72), (10, 9, 0.35),
        (10, 10, 0.50), (11, 9, 0.28), (12, 13, 0.60), (13, 4, 0.40),
    ];
    for (r, c, d) in tree_cells {
        grid.set_forest_density(r, c, d);
    }
    grid.sync_feature_flags_from_terrain();
    grid.refresh_attrition();

    let mut w = SimWorld::new(grid);
    let cmd_pl = w.commanders.spawn_commander(
        "cmd-pl-1",
        "红1排排部",
        TeamId::Red,
        (8, 8),
        Echelon::Platoon,
        Rank::Lieutenant,
        0,
    );
    let hq_uid = w.units.spawn_unit_4ledger(
        "cmd-pl-1-hq",
        "红1排排部 · 指挥班",
        TeamId::Red,
        (8, 8),
        Echelon::Squad,
        "hq_squad",
        2,
        1,
        1,
        eq_map(&[("rifle", 2), ("radio", 1)]),
        0,
    );
    w.commanders.hq_piece_id[cmd_pl.0] = Some(hq_uid);
    w.units.direct_commander_id[hq_uid.0] = Some(cmd_pl);
    w.commanders.sync_hq_ledger_authority(cmd_pl, &w.units);
    let u1 = w.units.spawn_unit_4ledger(
        "red-sq-1",
        "红1排1班",
        TeamId::Red,
        (8, 6),
        Echelon::Squad,
        "rifle_squad",
        10,
        1,
        0,
        eq_map(&[("rifle", 9), ("lmg", 1), ("radio", 1)]),
        0,
    );
    w.commanders.link_subordinate_unit(cmd_pl, u1, &mut w.units);

    let u_arty = w.units.spawn_unit_4ledger(
        "red-arty-1",
        "红重炮1班 (203mm)",
        TeamId::Red,
        (7, 5),
        Echelon::Squad,
        "howitzer_squad",
        8,
        1,
        0,
        eq_map(&[("howitzer_203", 1), ("rifle", 7), ("radio", 1)]),
        0,
    );
    w.commanders.link_subordinate_unit(cmd_pl, u_arty, &mut w.units);

    let _ub = w.units.spawn_unit_4ledger(
        "blue-sq-1",
        "蓝方前哨班",
        TeamId::Blue,
        (8, 12),
        Echelon::Squad,
        "rifle_squad",
        8,
        1,
        0,
        eq_map(&[("rifle", 8), ("radio", 1)]),
        1,
    );
    w
}

