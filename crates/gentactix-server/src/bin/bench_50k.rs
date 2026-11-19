//! High-scale 50,000+ squad determinism & performance benchmark on 1:1 4096x4096 `.r16` terrain.

use clap::Parser;
use gentactix_core::combat_logistics::Depot;
use gentactix_core::command_log::{register_zone, MathRecursiveDriver};
use gentactix_core::sim_loop::SimWorld;
use gentactix_core::units_ecs::{Echelon, Rank};
use gentactix_core::world_grid::{FrontlineRole, TeamId, WorldGrid, LINE_MASK_ALL, WATER};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "bench_50k", about = "Benchmark 1,000 battalions / 50,000 squads on 4096x4096 .r16")]
struct Args {
    /// Directory containing 1100_x1_y1.r16, diffuse_x0_y0_x1_y1.png, river_x1_y1.png
    #[arg(long, default_value = r"E:\商单1100\New folder")]
    r16_dir: PathBuf,
    /// Direct path to .r16 file (overrides --r16-dir tile auto-discovery if provided)
    #[arg(long)]
    r16: Option<PathBuf>,
    #[arg(long, default_value_t = 100)]
    flow_threshold: u16,
    #[arg(long, default_value_t = 1000)]
    battalions: usize,
    #[arg(long, default_value_t = 6)]
    ticks: u32,
}

#[repr(C)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
}

fn current_rss_mb() -> (f64, f64) {
    let mut pmc = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) };
    if ok != 0 {
        (
            pmc.working_set_size as f64 / (1024.0 * 1024.0),
            pmc.peak_working_set_size as f64 / (1024.0 * 1024.0),
        )
    } else {
        (0.0, 0.0)
    }
}

fn populate_large_force(w: &mut SimWorld, n_battalions: usize) {
    let rows = w.grid.rows as i32;
    let cols = w.grid.cols as i32;

    // Collect passable anchor coordinates spaced across the map
    let mut anchors = Vec::with_capacity(n_battalions);
    let step = 48i32;
    let mut r = 120i32;
    while r < rows - 120 && anchors.len() < n_battalions {
        let mut c = 120i32;
        while c < cols - 120 && anchors.len() < n_battalions {
            if w.grid.terrain[w.grid.idx(r, c)] != WATER {
                anchors.push((r, c));
            }
            c += step;
        }
        r += step;
    }
    while anchors.len() < n_battalions {
        let k = anchors.len() as i32;
        anchors.push((500 + (k * 7) % (rows - 600), 500 + (k * 13) % (cols - 600)));
    }

    let mut eq_rifle = FxHashMap::default();
    eq_rifle.insert("rifle".to_string(), 9);
    eq_rifle.insert("lmg".to_string(), 1);
    eq_rifle.insert("radio".to_string(), 1);

    let mut eq_heavy = FxHashMap::default();
    eq_heavy.insert("hmg".to_string(), 1);
    eq_heavy.insert("rifle".to_string(), 5);
    eq_heavy.insert("radio".to_string(), 1);

    // Top-level division/regiment commanders for Red and Blue
    let red_div = w.commanders.spawn_commander("red-div-1", "Red Div", TeamId::Red, (700, 1800), Echelon::Division, Rank::General, 0);
    let blue_div = w.commanders.spawn_commander("blue-div-1", "Blue Div", TeamId::Blue, (700, 2600), Echelon::Division, Rank::General, 1);

    for (bi, &(ar, ac)) in anchors.iter().enumerate() {
        let team = if bi % 2 == 0 { TeamId::Red } else { TeamId::Blue };
        let ch = if team == TeamId::Red { 0 } else { 1 };
        let bn_cmd = w.commanders.spawn_commander(
            format!("bn-{}", bi),
            format!("Bn {}", bi),
            team,
            (ar, ac),
            Echelon::Battalion,
            Rank::Major,
            ch,
        );
        w.commanders.link_subordinate_cmd(if team == TeamId::Red { red_div } else { blue_div }, bn_cmd);

        let mut co_cmds = Vec::with_capacity(3);
        for ci in 0..3 {
            let co_px = ((ar + (ci as i32 - 1) * 4).clamp(1, rows - 2), (ac + 2).clamp(1, cols - 2));
            let co_cmd = w.commanders.spawn_commander(
                format!("co-{}-{}", bi, ci),
                format!("Co {}-{}", bi, ci),
                team,
                co_px,
                Echelon::Company,
                Rank::Captain,
                ch,
            );
            w.commanders.link_subordinate_cmd(bn_cmd, co_cmd);
            // Every active maneuver battalion (20%) gets an active AO + DEPLOY_LINE / MOVE_LINE
            let fl_id = if bi % 5 == 0 {
                let p0 = [co_px.0 as f32 - 2.0, co_px.1 as f32];
                let p1 = [co_px.0 as f32 + 2.0, co_px.1 as f32];
                let lid = w.grid.write_typed_frontline(
                    &format!("dep-{}-{}", bi, ci),
                    team,
                    co_cmd.0,
                    0,
                    FrontlineRole::Deploy,
                    0,
                    0,
                    LINE_MASK_ALL,
                    1.0,
                    None,
                    &[p0, p1],
                    4,
                );
                if ci == 0 {
                    // Add active MOVE_LINE on first 10% for realistic dynamic movement + route evaluation
                    w.grid.write_typed_frontline(
                        &format!("mv-{}-{}", bi, ci),
                        team,
                        co_cmd.0,
                        0,
                        FrontlineRole::Move,
                        lid,
                        0,
                        LINE_MASK_ALL,
                        1.4,
                        None,
                        &[p0, [p0[0], p0[1] + 5.0]],
                        6,
                    );
                }
                let mut ao_cells = Vec::with_capacity(25);
                for dr in -2..=2 {
                    for dc in -2..=2 {
                        ao_cells.push((co_px.0 + dr, co_px.1 + dc));
                    }
                }
                let _ = register_zone(
                    &mut w.zones,
                    format!("ao-{}-{}", bi, ci),
                    co_cmd,
                    Echelon::Company,
                    ao_cells,
                    0,
                    0,
                    &w.commanders,
                    &mut w.grid,
                );
                lid
            } else {
                // Static garrison battalion also claims a baseline deploy line and local AO box
                let mut ao_cells = Vec::with_capacity(9);
                for dr in -1..=1 {
                    for dc in -1..=1 {
                        ao_cells.push((co_px.0 + dr, co_px.1 + dc));
                    }
                }
                w.grid.write_ao_cells((bi * 4 + ci + 1) as u32, team, Echelon::Company as u8, &ao_cells);
                1
            };
            co_cmds.push((co_cmd, co_px, fl_id));
        }

        // 50 squads per battalion (16 + 17 + 17 distributed across 3 companies -> 50,000 total for 1,000 bns!)
        for si in 0..50 {
            let co_slot = si % 3;
            let (co_cmd, co_px, fl_id) = co_cmds[co_slot];
            let s_r = (co_px.0 + (si as i32 / 10) - 2).clamp(1, rows - 2);
            let s_c = (co_px.1 + (si as i32 % 5) - 2).clamp(1, cols - 2);
            let is_heavy = si % 12 == 0;
            let u = w.units.spawn_unit(
                format!("sq-{}-{}", bi, si),
                format!("Sq {}-{}", bi, si),
                team,
                (s_r, s_c),
                Echelon::Squad,
                if is_heavy { "mg_squad" } else { "rifle_squad" },
                if is_heavy { 7 } else { 10 },
                1,
                if is_heavy { eq_heavy.clone() } else { eq_rifle.clone() },
                ch,
            );
            w.units.active_deploy_line_id[u.0] = fl_id;
            w.commanders.link_subordinate_unit(co_cmd, u, &mut w.units);
        }
    }

    w.depots.push(Depot {
        str_id: "depot-red-main".into(),
        team: TeamId::Red,
        px: (700, 1790),
        food_t: 5000.0,
        ammo_t: 5000.0,
        fuel_t: 2000.0,
        replacements: 5000.0,
    });
    w.depots.push(Depot {
        str_id: "depot-blue-main".into(),
        team: TeamId::Blue,
        px: (700, 2610),
        food_t: 5000.0,
        ammo_t: 5000.0,
        fuel_t: 2000.0,
        replacements: 5000.0,
    });
}

fn clone_world_for_replay(r16: &PathBuf, diff: Option<&PathBuf>, riv: Option<&PathBuf>, flow_thr: u16, n_bns: usize) -> SimWorld {
    let grid = WorldGrid::from_r16_diffuse_river(
        r16.as_path(),
        diff.map(|p| p.as_path()),
        riv.map(|p| p.as_path()),
        flow_thr,
    )
    .expect("failed loading 1:1 .r16 terrain");
    let mut w = SimWorld::new(grid);
    populate_large_force(&mut w, n_bns);
    w
}

fn main() {
    let args = Args::parse();
    let r16_path = args.r16.clone().unwrap_or_else(|| args.r16_dir.join("1100_x1_y1.r16"));
    let diff_path = args.r16_dir.join("diffuse_x0_y0_x1_y1.png");
    let riv_path = args.r16_dir.join("river_x1_y1.png");
    let diff_opt = if diff_path.exists() { Some(diff_path) } else { None };
    let riv_opt = if riv_path.exists() { Some(riv_path) } else { None };

    println!("=== Gentactix V3.1 Rust Engine 50,000-Squad Benchmark ===");
    println!("Rayon threads: {}", rayon::current_num_threads());
    println!("Loading 1:1 terrain: {}", r16_path.display());

    let t_load0 = Instant::now();
    let mut w1 = clone_world_for_replay(&r16_path, diff_opt.as_ref(), riv_opt.as_ref(), args.flow_threshold, args.battalions);
    let load_ms = t_load0.elapsed().as_secs_f64() * 1000.0;
    println!(
        "Loaded Grid: {}x{} ({:.0} m/px) | Squads: {} | Commanders: {} | Init time: {:.1} ms",
        w1.grid.rows,
        w1.grid.cols,
        w1.grid.cell_m,
        w1.units.id.len(),
        w1.commanders.id.len(),
        load_ms
    );

    // Measure pure SpatialBucketGrid rebuild time
    let t_b0 = Instant::now();
    w1.derived.buckets.rebuild(&w1.units, w1.grid.rows, w1.grid.cols);
    let bucket_ms = t_b0.elapsed().as_secs_f64() * 1000.0;
    println!("CSR SpatialBucketGrid rebuild (50k units): {:.3} ms", bucket_ms);

    // Run simulation batch 1
    let mut drv1 = MathRecursiveDriver;
    let mut subtick_ms_list = Vec::new();
    let mut last_moved = 0usize;
    let mut last_fp1 = String::new();
    for t in 0..args.ticks {
        let ts = Instant::now();
        let rep = w1.step_subtick_5m(&mut drv1);
        let dt_ms = ts.elapsed().as_secs_f64() * 1000.0;
        subtick_ms_list.push(dt_ms);
        last_moved = rep.moved_units;
        last_fp1 = rep.fingerprint;
        println!(
            "  Run1 subtick {}: {:.2} ms | moved={} | attacking={} | fp={}",
            t + 1,
            dt_ms,
            rep.moved_units,
            rep.attacking_units,
            &last_fp1[..16]
        );
    }

    let (rss_mb, peak_rss_mb) = current_rss_mb();
    let avg_ms = subtick_ms_list.iter().sum::<f64>() / subtick_ms_list.len().max(1) as f64;
    println!(
        "Run1 Summary: avg_5m_subtick={:.2} ms | steady_last_moved={} | RSS={:.1} MiB | PeakRSS={:.1} MiB",
        avg_ms, last_moved, rss_mb, peak_rss_mb
    );

    // Run simulation batch 2 independently to prove 100% deterministic multi-core replay
    let mut w2 = clone_world_for_replay(&r16_path, diff_opt.as_ref(), riv_opt.as_ref(), args.flow_threshold, args.battalions);
    let mut drv2 = MathRecursiveDriver;
    let mut last_fp2 = String::new();
    for _ in 0..args.ticks {
        last_fp2 = w2.step_subtick_5m(&mut drv2).fingerprint;
    }
    assert_eq!(
        last_fp1, last_fp2,
        "DETERMINISM FAILURE: fp1={} != fp2={}",
        last_fp1, last_fp2
    );
    println!("Determinism Verification: PASS (fp1 == fp2 == {})", last_fp1);
    let _ = par_iter_dummy();
}

fn par_iter_dummy() -> usize {
    (0..4usize).into_par_iter().sum()
}
