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
