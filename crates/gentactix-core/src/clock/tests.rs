    use crate::state::{build_demo_world, SimDerivedState};
    use crate::units_ecs::UnitId;
    use crate::command_log::MathRecursiveDriver;
    use crate::fields::{SchmidtPostureState, ALPHA_BLAST, ALPHA_SUPP};
    use crate::geometry::tactical_instance::{AugmentedArcIntegrator, ConstructionFsmState, DetonatorSwitchEntity};

    #[test]
    fn test_v6_phase01_sim_loop_golden_operators_integration() {
        let mut sim = build_demo_world();
        let mut driver = MathRecursiveDriver::default();

        // 1. 验证主循环推进与电台 ZOH 稀疏滤波器接入
        assert!((ALPHA_BLAST - 0.1690).abs() < 1e-4, "ALPHA_BLAST 必须精确锁死 0.1690");
        assert!((ALPHA_SUPP - 0.6412).abs() < 1e-4, "ALPHA_SUPP 必须精确锁死 0.6412");

        sim.step_subtick_16s(&mut driver);
        assert!(!sim.unit_zoh_filters.is_empty(), "主循环必须自动维护单位 ZOH 滤波器");
        for (_, filter) in &sim.unit_zoh_filters {
            assert!(filter.entries.len() <= 16, "稀疏 ZOH 模态项不超过 16");
            assert!(std::mem::size_of_val(filter) <= 128, "单个单位 ZOH 滤波器内存占用严格 <= 128 字节");
            assert!(filter.dwell_ticks >= 1, "ZOH 滤波器随 16s 步长累加 dwell_ticks");
        }

        // 2. 验证受击格双时间尺度衰减与单位姿态坍塌机
        let target_px = (20, 20);
        let target_uid = UnitId(0);
        sim.record_blast_and_suppression(target_px, 10.0, Some(target_uid));
        let cidx = sim.grid.idx(target_px.0, target_px.1);
        let h_blast_init = sim.cell_h_blast[cidx];
        let s_supp_init = sim.cell_s_supp[cidx];
        assert!(h_blast_init >= 100.0, "受到重击后目标格 H_blast 骤升");
        assert!(s_supp_init >= 10.0, "受到重击后目标格 S_supp 骤升");
        assert_eq!(
            sim.unit_postures[target_uid.0],
            SchmidtPostureState::Prone,
            "高压制直接击穿施密特死区，单位进入 Prone 伏卧态"
        );
        assert_eq!(
            sim.unit_postures[target_uid.0].source_factor(),
            0.02,
            "Prone 态发射辐射极大坍塌至 2%"
        );

        // 推进 2 个 16s 步长 (32 秒后)
        sim.step_subtick_16s(&mut driver);
        sim.step_subtick_16s(&mut driver);

        let h_blast_32s = sim.cell_h_blast[cidx];
        let s_supp_32s = sim.cell_s_supp[cidx];
        // H_blast 经历快衰减 (ALPHA_BLAST^2)
        assert!(
            h_blast_32s < h_blast_init * 0.05,
            "32s 后快衰减爆炸危险已基本消除 (剩余 < 5%)"
        );
        // S_supp 经历慢恢复 (ALPHA_SUPP^2 ~ 0.41)
        assert!(
            s_supp_32s > s_supp_init * 0.35,
            "32s 后敌方依然处于显著压制恢复期，突击黄金窗口确立"
        );

        // 3. 验证工兵增广弧长 s_tilde 边走边干施工实体与半截保留
        let u_eng = 0u32;
        let waypoints = vec![[15.0, 15.0], [15.0, 18.0]];
        let mut integrator = AugmentedArcIntegrator::new(waypoints);
        integrator.cur_pos = [15.0, 15.0];
        sim.engineer_integrators.insert(u_eng, integrator);

        // 重压制下暂停测试
        sim.unit_suppression[u_eng as usize] = 2.0;
        sim.step_subtick_16s(&mut driver);
        assert_eq!(
            sim.engineer_integrators[&u_eng].fsm_state,
            ConstructionFsmState::PinnedPause,
            "严重压制下工兵进入 PinnedPause 暂停"
        );

        // 恢复作业并推进
        sim.unit_suppression[u_eng as usize] = 0.0;
        sim.unit_postures[u_eng as usize] = SchmidtPostureState::Active;
        sim.step_subtick_16s(&mut driver);
        assert!(
            sim.engineer_integrators[&u_eng].s_tilde > 0.0,
            "工兵恢复作业后增广弧长 s_tilde 推进大于 0"
        );

        // 4. 验证起爆开关实体门禁与触发
        let mut switch = DetonatorSwitchEntity::new(
            101,
            sim.units.team[0] as u8,
            [10.0, 60.0],
            vec![[10.0, 15.0]],
            vec![[10.0, 60.0], [10.0, 15.0]],
        );
        switch.charges_placed = true;
        switch.wire_laid = true;
        // 操作员在安全距离外 (距离 45m >= 40m)
        assert!(switch.can_detonate([10.0, 60.0]).is_ok(), "安全距离外允许起爆");
        sim.detonator_switches.push(switch);

        // 推进触发起爆
        sim.step_subtick_16s(&mut driver);
        assert!(sim.detonator_switches[0].detonated, "起爆开关成功引爆");
    }

#[test]
fn test_phase_2a_thirteen_stage_execution_and_scratch_clean() {
    let mut sim = build_demo_world();
    let mut driver = MathRecursiveDriver::default();
    for _ in 0..5 {
        sim.step_subtick_16s(&mut driver);
        assert_eq!(sim.last_tick_phases, 13, "Every 16s subtick must run all 13 phases cleanly");
        assert!(sim.scratch.is_clean(), "SimTickScratch must be empty after Stage 12 PublishAndDrop");
    }
}

#[test]
fn test_phase_2a_derived_destroy_and_rebuild_zero_drift() {
    let mut w1 = build_demo_world();
    let mut w2 = build_demo_world();
    let mut d1 = MathRecursiveDriver::default();
    let mut d2 = MathRecursiveDriver::default();

    for step in 0..10 {
        w1.step_subtick_16s(&mut d1);
        if step == 4 {
            // Destroy all derived state containers and rebuild solely from WorldState
            w2.derived = SimDerivedState::new(w2.grid.rows, w2.grid.cols);
            w2.rebuild_derived_state();
        }
        w2.step_subtick_16s(&mut d2);
    }

    assert_eq!(
        w1.state_fingerprint(),
        w2.state_fingerprint(),
        "Destroying and rebuilding DerivedState midway must cause zero drift"
    );
}

#[test]
fn test_phase_2b_deygout_radio_non_decaying_and_recovery() {
    use crate::math_ops::DeygoutRadioOperator;
    use crate::state::SimWorld;
    use crate::units_ecs::{Echelon, Rank};
    use crate::world_grid::{TeamId, WorldGrid};
    use rustc_hash::FxHashMap;

    let mut grid = WorldGrid::new_plain(32, 32, 8.0);
    // Raise a high ridge halfway at col 15 -> triggers severe Deygout diffraction (t_diff < 0.5)
    for r in 0..32 {
        let gi = grid.idx(r, 15);
        grid.elevation_m[gi] = 400.0;
    }
    let mut sim = SimWorld::new(grid);
    let cmd = sim.commanders.spawn_commander("cmd-r", "Cmd", TeamId::Red, (10, 10), Echelon::Platoon, Rank::Lieutenant, 0);
    let uid = sim.units.spawn_unit("u-r", "U", TeamId::Red, (10, 20), Echelon::Squad, "rifle_squad", 8, 1, FxHashMap::default(), 0);
    sim.commanders.link_subordinate_unit(cmd, uid, &mut sim.units);
    sim.units.control_radius_km[uid.0] = 5.0;

    let mut driver = MathRecursiveDriver::default();
    sim.step_subtick_16s(&mut driver);
    let damped_after_1 = sim.units.control_radius_km[uid.0];
    assert!(damped_after_1 < 5.0, "Mountain ridge must damp radio control_radius_km, got {damped_after_1}");

    for _ in 0..20 {
        sim.step_subtick_16s(&mut driver);
    }
    let damped_after_21 = sim.units.control_radius_km[uid.0];
    assert!(
        (damped_after_21 - damped_after_1).abs() < 1e-4,
        "Blocked radio radius must remain steady instead of exponentially decaying to 0.1km: t1={damped_after_1}, t21={damped_after_21}"
    );

    // Flatten the ridge -> radius must fully recover back to base 5.0km
    for r in 0..32 {
        let gi = sim.grid.idx(r, 15);
        sim.grid.elevation_m[gi] = 0.0;
    }
    sim.step_subtick_16s(&mut driver);
    assert!(
        (sim.units.control_radius_km[uid.0] - 5.0).abs() < 1e-4,
        "Clearing obstacle must restore control_radius_km back to 5.0km, got {}",
        sim.units.control_radius_km[uid.0]
    );
    let _ = DeygoutRadioOperator::lorentz_resonance(0.0, 1.0);
}

#[test]
fn test_phase_2b_g_field_auction_en_route_and_work_irreversible() {
    use crate::math_ops::{AugmentedArcIntegrator, GFieldKind};
    use crate::state::{GFieldCellState, SimWorld};
    use crate::units_ecs::Echelon;
    use crate::world_grid::{FrontlineRole, TeamId, WorldGrid, LINE_MASK_ALL};
    use rustc_hash::FxHashMap;

    let grid = WorldGrid::new_plain(32, 32, 8.0);
    let mut sim = SimWorld::new(grid);
    let uid = sim.units.spawn_unit("u-g", "G-Squad", TeamId::Red, (15, 10), Echelon::Squad, "rifle_squad", 8, 1, FxHashMap::default(), 0);
    let _fl = sim.grid.write_typed_frontline(
        "dep-g",
        TeamId::Red,
        0,
        0,
        FrontlineRole::Deploy,
        0,
        0,
        LINE_MASK_ALL,
        2.0,
        None,
        &[[15.0, 14.0], [16.0, 14.0]],
        4,
    );

    // Mount a GFieldKind::Work demand at (25, 25) and an engineer finishing that waypoint
    let u_work = sim.units.spawn_unit("u-w", "W-Squad", TeamId::Red, (25, 25), Echelon::Squad, "engineer_squad", 6, 1, FxHashMap::default(), 0);
    let work_gi = sim.grid.idx(25, 25);
    sim.derived.g_field_cells.insert(
        work_gi,
        GFieldCellState {
            g0: 3.0,
            committed_en_route: 0.0,
            actual_on_site: 0.0,
            kind: GFieldKind::Work,
        },
    );
    let mut integrator = AugmentedArcIntegrator::new(vec![[25.0, 25.0]]);
    integrator.cur_pos = [25.0, 25.0];
    sim.engineer_integrators.insert(u_work.0 as u32, integrator);

    let mut driver = MathRecursiveDriver::default();
    sim.step_subtick_16s(&mut driver);

    assert!(
        sim.derived.g_auction_assignments.contains_key(&uid.0),
        "Auction must assign unit to active G-field cell"
    );
    let any_en_route = sim
        .derived
        .g_field_cells
        .values()
        .any(|c| c.kind == GFieldKind::State && c.committed_en_route >= 1.0);
    assert!(any_en_route, "Remote auction winner must count toward committed_en_route");

    // Step until engineer completes waypoint at (25, 25) -> work G0 must irreversibly drop
    for _ in 0..3 {
        sim.step_subtick_16s(&mut driver);
    }
    let work_g0 = sim.derived.g_field_cells[&work_gi].g0;
    assert!(work_g0 < 3.0, "Completed GFieldKind::Work must irreversibly decrement g0 from 3.0, got {work_g0}");
}

#[test]
fn test_phase_2b_8m_grid_scan_and_fire_arc_scale() {
    use crate::combat_logistics::compute_fire_arc;
    use crate::state::SimWorld;
    use crate::units_ecs::{AtomicAction, Echelon};
    use crate::world_grid::{TeamId, WorldGrid};
    use rustc_hash::FxHashMap;

    // 1. Verify compute_fire_arc on 8m grid lifts old 72-cell (576m) truncation cap
    let grid = WorldGrid::new_plain(300, 300, 8.0);
    let arc = compute_fire_arc(&grid, [150.0, 150.0], 1000.0, 0.0, false, false, 1000.0);
    let max_dist_m = arc.iter().map(|c| c.dist_m).fold(0.0f32, f32::max);
    assert!(
        max_dist_m > 700.0,
        "8m compute_fire_arc must reach > 700m instead of capping at 72*8=576m, got {max_dist_m}"
    );

    // 2. Verify Stage 8 Attack scan on 8m grid detects & fires at 320m (40 cells > old 15-cell limit)
    let mut sim = SimWorld::new(grid);
    let mut red_eq = FxHashMap::default();
    red_eq.insert("hmg".to_string(), 1);
    red_eq.insert("ammo_762_mm".to_string(), 500);
    let u_red = sim.units.spawn_unit_4ledger("r8", "RedHMG", TeamId::Red, (50, 50), Echelon::Squad, "mg_squad", 6, 1, 0, red_eq, 0);
    sim.units.ammo_t[u_red.0] = 5.0;
    let u_blue = sim.units.spawn_unit_4ledger("b8", "BlueTgt", TeamId::Blue, (50, 90), Echelon::Squad, "rifle_squad", 8, 1, 0, FxHashMap::default(), 1);
    sim.units.autonomy_state[u_red.0] = AtomicAction::Attack;
    sim.rebuild_derived_state();

    // Call Stage 8 + Stage 9 indirectly via direct attack state preserved on attack hold
    let mut driver = MathRecursiveDriver::default();
    sim.step_subtick_16s(&mut driver);
    assert!(
        sim.units.org[u_blue.0] < 100.0 || sim.unit_suppression[u_blue.0] > 0.0,
        "8m Stage 8 target scan must engage enemy at 40 cells (320m), got org={} supp={}",
        sim.units.org[u_blue.0],
        sim.unit_suppression[u_blue.0]
    );
}

#[test]
fn test_phase_2b_intent_arbitrator_direct_overrides_autonomy() {
    use crate::state::SimWorld;
    use crate::units_ecs::Echelon;
    use crate::world_grid::{TeamId, WorldGrid};
    use rustc_hash::FxHashMap;

    let grid = WorldGrid::new_plain(64, 64, 8.0);
    let mut sim = SimWorld::new(grid);
    let mut eq = FxHashMap::default();
    eq.insert("rifle".to_string(), 8);
    let u_red = sim.units.spawn_unit_4ledger("u-dir", "直控班", TeamId::Red, (20, 20), Echelon::Squad, "rifle_squad", 8, 1, 0, eq, 0);
    // Enemy located South-West to pull any unconstrained autonomous unit away
    let _u_en = sim.units.spawn_unit_4ledger("u-lure", "诱饵", TeamId::Blue, (30, 15), Echelon::Squad, "rifle_squad", 8, 1, 0, FxHashMap::default(), 1);

    sim.try_set_direct_unit_order_ex(
        u_red,
        "move",
        Some([20.0, 28.0]),
        false,
        None,
        "fast",
        "stop_to_fire",
        false,
    )
    .unwrap();

    let mut driver = MathRecursiveDriver::default();
    for _ in 0..3 {
        sim.step_subtick_16s(&mut driver);
    }
    assert!(
        sim.units.pos[u_red.0][1] > 21.0 && (sim.units.pos[u_red.0][0] - 20.0).abs() < 1.0,
        "IntentArbitrator must enforce DirectOrder eastward instead of drifting south toward lure: pos={:?}",
        sim.units.pos[u_red.0]
    );
}
