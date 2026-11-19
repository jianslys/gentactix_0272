use super::*;
use crate::events::{EventBuffer, EventFinalStatus};
use crate::geometry::CellCoord;
use crate::state::WorldState;
use rustc_hash::FxHashMap;

    use crate::geometry::{GridGeometry, WorldPos};
    use crate::state::EntityAuthorityRecord;

    #[test]
    fn test_phase_06_fire_slice_ammo_deduct_damage_and_workspace_cleanup() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);
        let mut events = EventBuffer::default();
        let mut workspace = TransientBlastWorkspace::default();

        // 设定炮手与初始弹药
        let shooter_id = 1;
        let mut stocks = FxHashMap::default();
        stocks.insert("105mm_he".to_string(), 1); // 仅有 1 发弹药
        world.inventory.entity_stocks.insert(shooter_id, stocks);

        // 设定目标实体 (位于 80m, 80m 处，即 cell (10, 10))
        let target_entity_id = 2;
        world.entities.insert(EntityAuthorityRecord {
            id: target_entity_id,
            str_id: "target_inf".into(),
            team_id: 2,
            pos: WorldPos::new(84.0, 84.0), // center of (10, 10)
            facing_deg: 0.0,
            alive: true,
            persons: 10,
            org: 100.0,
        });

        let target_cell = CellCoord::new(10, 10);

        // 第一次开火：弹药充裕，成功命中并破坏地形
        let res1 = execute_single_fire_slice(
            &mut world,
            &mut events,
            &mut workspace,
            shooter_id,
            target_cell,
            "105mm_he",
            1001,
        ).unwrap();

        assert!(matches!(res1, EventFinalStatus::Resolved { .. }));
        assert_eq!(world.inventory.entity_stocks[&shooter_id]["105mm_he"], 0, "Ammo must be decremented to 0");
        assert!(world.entities.entities[&target_entity_id].org < 100.0, "Target org must take blast damage");
        assert_eq!(world.terrain.elevation_m[world.terrain.idx(10, 10)], 98.5, "Crater must drop elevation by 1.5m");
        assert_eq!(workspace.peak_overpressure_kpa.len(), 0, "Transient workspace must be thoroughly cleaned!");

        // 第二次开火：弹药已空，必须拒绝，绝不造成二次伤害
        let org_before = world.entities.entities[&target_entity_id].org;
        let res2 = execute_single_fire_slice(
            &mut world,
            &mut events,
            &mut workspace,
            shooter_id,
            target_cell,
            "105mm_he",
            1002,
        ).unwrap();

        assert!(matches!(res2, EventFinalStatus::Rejected { .. }));
        assert_eq!(world.entities.entities[&target_entity_id].org, org_before, "No damage on rejected out-of-ammo fire");
    }

    #[test]
    fn test_phase_01_dual_field_four_antifraud_iron_rules() {
        // 铁律一测试：在途预扣 vs 到场实扣
        let g0 = 100.0;
        let mut on_site = 20.0;
        let mut en_route = 80.0;
        let alloc1 = compute_g_alloc(g0, on_site, en_route);
        assert_eq!(alloc1, 0.0, "100% 预扣后待分配残差必须为 0，防止友军在途超卖");

        // 1 班取消队列，释放在途预扣 40.0
        en_route -= 40.0;
        let alloc2 = compute_g_alloc(g0, on_site, en_route);
        assert_eq!(alloc2, 40.0, "释放预扣后残差场立即回弹 40");

        // 铁律二测试：状态占坑 vs 积分做功
        // 状态占坑型：人在账消，人撤账回
        on_site += 40.0;
        assert_eq!(compute_g_alloc(g0, on_site, en_route), 0.0);
        on_site = 0.0; // 撤离
        en_route = 0.0;
        assert_eq!(compute_g_alloc(g0, on_site, en_route), 100.0, "状态占坑人走后账目必须弹回 100");

        // 积分做功型：工兵排雷，永久销账
        let mut minefield_g0 = 50.0;
        apply_g_work_consumption(&mut minefield_g0, 30.0);
        assert_eq!(minefield_g0, 20.0);
        apply_g_work_consumption(&mut minefield_g0, 20.0);
        assert_eq!(minefield_g0, 0.0, "排雷完毕 G0 归零");
        apply_g_work_consumption(&mut minefield_g0, 10.0); // 即使有额外工兵撤离
        assert_eq!(minefield_g0, 0.0, "积分做功人走绝不反弹地雷复活");

        // 铁律三测试：落点瞬态爆炸危险与辐射源项压制
        let mut threat_op = ThreatRadiationOperator::default();
        threat_op.enemy_sources.insert(42, 100.0); // 敌方阵地发射源 S_k = 100
        let threat_before = threat_op.compute_threat_at(42, true, 0.8);
        assert_eq!(threat_before, 80.0);

        // 炮火落点格自身 H_blast 必为正累加
        threat_op.add_blast_danger(100, 250.0);
        threat_op.add_blast_danger(100, 50.0);
        assert_eq!(threat_op.h_blast[&100], 300.0, "炮火落点自身 H_blast 必须正累加");

        // 压制敌方阵地源项：拉高压制密度使 S_k -> 0
        threat_op.apply_enemy_suppression(42, 0.9); // 90% 压制
        let threat_after = threat_op.compute_threat_at(42, true, 0.8);
        assert!((threat_after - 8.0).abs() < 1e-4, "敌发射源被压制后前线封锁威胁自然消散");

        // 铁律四测试：电台乘数滤波与 30% 迟滞死区
        let g_current = 100.0;
        // 杂音轻微扰动：滤波后为 110.0 (偏差 10% <= 30%)
        let n_rethink_none = check_hysteresis_and_rethink(110.0, g_current, 0.8, 10, false);
        assert!(n_rethink_none.is_none(), "偏差 <= 30% 死区内严禁打断下属队列");

        // 战局剧变：滤波后为 145.0 (偏差 45% > 30%)
        let n_rethink_some = check_hysteresis_and_rethink(145.0, g_current, 0.8, 10, false);
        assert_eq!(n_rethink_some, Some(2), "偏差 > 30% 触发重寻思，eta=0.8 时 10*(1-0.8)=2 步长");

        // 原目标失效强制触发重寻思
        let n_rethink_invalid = check_hysteresis_and_rethink(105.0, g_current, 0.5, 10, true);
        assert_eq!(n_rethink_invalid, Some(5), "目标失效必须触发重寻思，eta=0.5 时 10*(1-0.5)=5 步长");
    }

    #[test]
    fn test_v5_g_field_auction_and_hysteresis() {
        // 1. 确定性 Tier-B 多容量最小堆拍卖 (无 exp/log 浮点漂移)
        let bids = vec![
            AuctionBid { unit_id: 1, cell_idx: 10, cost_fixed: 50 },
            AuctionBid { unit_id: 2, cell_idx: 10, cost_fixed: 30 }, // 更优报价
            AuctionBid { unit_id: 3, cell_idx: 10, cost_fixed: 70 },
            AuctionBid { unit_id: 4, cell_idx: 20, cost_fixed: 40 },
        ];
        let mut capacities = rustc_hash::FxHashMap::default();
        capacities.insert(10, 1); // cell 10 仅允许 1 个名额
        capacities.insert(20, 2); // cell 20 允许 2 个名额

        let win = solve_deterministic_auction(bids, &capacities);
        // unit 2 (cost 30) 胜出 cell 10，unit 4 胜出 cell 20
        assert_eq!(win.len(), 2);
        assert!(win.contains(&(2, 10)));
        assert!(win.contains(&(4, 20)));

        // 预扣防超卖计算
        let alloc = compute_g_alloc(100.0, 30.0, 70.0);
        assert_eq!(alloc, 0.0);
    }

    #[test]
    fn test_v5_work_mode_never_rebounds() {
        // 积分做功型 (Work): 工兵排雷削减 WorldState 基底 G0，人走绝不反弹
        let mut g0_work = 80.0f32;
        apply_g_work_consumption(&mut g0_work, 50.0);
        assert_eq!(g0_work, 30.0);

        // 工兵排雷完毕并撤离战场，G0 依然是 30.0，绝不恢复为 80.0
        let on_site = 0.0;
        let en_route = 0.0;
        let remaining_alloc = compute_g_alloc(g0_work, on_site, en_route);
        assert_eq!(remaining_alloc, 30.0, "工兵撤离后积分做功剩余基底不得弹回初始值");
    }

    #[test]
    fn test_v5_suppression_and_assault_window() {
        // 双时间尺度：tau_blast = 9s, tau_supp = 36s (16s 步长: alpha_blast = 0.1690, alpha_supp = 0.6412)
        // 重炮轰击：目标格 H_blast 骤升，发射源进入 Prone 态，S_k -> 0
        let initial_h_blast = 100.0f32;
        let initial_supp = 5.0f32; // 高压制

        // 轰击中姿态判定 (从 Active 过渡到 Pinned 再到 Prone)
        let posture_during = SchmidtPostureState::Active.transition(initial_supp).transition(initial_supp);
        assert_eq!(posture_during, SchmidtPostureState::Prone);
        let zeta_during = posture_during.source_factor();
        assert_eq!(zeta_during, 0.02, "Prone 态发射辐射极大坍塌");

        // 炮火停歇 2 个 16s 步长 (32s 后):
        // H_blast 经历 2 次快衰减: 100.0 * 0.1690^2 ~ 2.85 (几乎清零，爆炸危险消除)
        let h_blast_after_32s = initial_h_blast * 0.1690f32 * 0.1690f32;
        assert!(h_blast_after_32s < 3.0, "32s 后爆炸危险清空，适合步兵突入");

        // 压制 S_supp 慢恢复: 5.0 * 0.6412^2 ~ 2.05 (依然 > 1.20 上界，依然被压制无法还击)
        let supp_after_32s = initial_supp * 0.6412f32 * 0.6412f32;
        let posture_after_32s = posture_during.transition(supp_after_32s);
        assert!(
            posture_after_32s == SchmidtPostureState::Prone || posture_after_32s == SchmidtPostureState::Pinned,
            "敌方在 32s 后依然受压制无法组织有效火力，自然形成步兵突击黄金窗口"
        );
    }

    #[test]
    fn test_v5_radio_schmidt_hysteresis() {
        let mut zoh = UnitZohDirectiveFilter::default();
        let targets = vec![(101, 5.0f32)];

        // 前 3 步：驻留步数未满 4 步，即使扰动很大也绝不打断
        for _ in 0..3 {
            zoh.step_zoh(&targets, 1.0, 16.0);
            assert!(zoh.evaluate_schmidt_rethink(&[(101, 10.0)], 1.0).is_none());
        }

        // 第 4 步到达驻留
        zoh.step_zoh(&targets, 1.0, 16.0);
        assert!(zoh.dwell_ticks >= 4);

        // 死区测试：失配 < Theta_cancel (3.5)
        let minor_mismatch = vec![(101, 6.0f32)]; // mismatch = 1.0 < 3.5
        assert!(zoh.evaluate_schmidt_rethink(&minor_mismatch, 1.0).is_none(), "死区内防帕金森晃动");

        // 剧烈变化测试：失配 >= 3.5
        let major_mismatch = vec![(101, 12.0f32)]; // mismatch > 3.5
        let rethink = zoh.evaluate_schmidt_rethink(&major_mismatch, 0.8);
        assert!(rethink.is_some(), "越过死区门限必须触发重寻思");
    }

    #[test]
    fn test_v5_sparse_zoh_memory_and_determinism() {
        // 稀疏向量内存约束：单单位 entries <= 16，内存 <= 128 字节
        let mut filter = UnitZohDirectiveFilter::default();
        for i in 0..20 {
            filter.step_zoh(&[(i, i as f32)], 1.0, 16.0);
        }
        assert!(filter.entries.len() <= 16, "稀疏滤波向量条目数必须 <= 16");
        let mem_size = std::mem::size_of_val(&filter) + filter.entries.capacity() * std::mem::size_of::<(u32, f32, f32)>();
        assert!(mem_size <= 512, "单单位稀疏滤波内存极为轻量（堆栈总和 <= 512B，条目存储 <= 128B）");

        // 确定性回放一致性：相同输入必须产出完全相同位移状态
        let mut f1 = UnitZohDirectiveFilter::default();
        let mut f2 = UnitZohDirectiveFilter::default();
        for i in 0..10 {
            f1.step_zoh(&[(1, (i * 2) as f32), (2, (i * 3) as f32)], 0.9, 16.0);
            f2.step_zoh(&[(1, (i * 2) as f32), (2, (i * 3) as f32)], 0.9, 16.0);
        }
        assert_eq!(f1, f2, "跨实例 Exact ZOH 滤波状态必须逐位完全一致");
    }
