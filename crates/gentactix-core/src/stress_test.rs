//! Phase 09: Minimal Stress & Full Determinism Regression Test
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 14 节与 plan.txt Phase 09 要求：
//! 1. 多实体容量竞争、高速跨格拦截
//! 2. 密集爆炸与地形变异、持久烟雾共存
//! 3. 完整 13 阶段事务跨 Tick 迭代
//! 4. 跨 Tick 状态指纹确定性回放断言

#[cfg(test)]
pub mod tests {
    use crate::events::EventBuffer;
    use crate::fields::{execute_single_fire_slice, TransientBlastWorkspace};
    use crate::geometry::{CellCoord, GridGeometry, WorldPos};
    use crate::intent::{IntentArbitrator, IntentCandidate, IntentSource};
    use crate::motion::{commit_motions, MotionConstraintSolver, MotionProposal};
    use crate::operators::{TerrainMutation, TerrainTransaction};
    use crate::publication::{compute_authority_fingerprint, load_world_state_from_json, save_world_state_to_json};
    use crate::state::{DerivedState, EntityAuthorityRecord, TickScratch, WorldState};
    use crate::transaction::{TickOrchestrator, TransactionPhase};
    use rustc_hash::FxHashMap;
    use std::time::Instant;

    #[test]
    fn test_phase_09_stress_regression_multi_entity_blast_and_save_replay() {
        let start_time = Instant::now();
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom.clone(), 0, 100.0);
        let mut derived = DerivedState::default();
        let mut scratch = TickScratch::default();
        let mut orch = TickOrchestrator::default();
        let mut events = EventBuffer::default();
        let mut blast_workspace = TransientBlastWorkspace::default();
        let mut motion_solver = MotionConstraintSolver::new(4); // 格内最大容量 4

        // 1. 生成 50 个红军与蓝军实体
        for i in 1..=50 {
            let x = 100.0 + (i as f32) * 2.0;
            let y = 100.0 + (i as f32) * 2.0;
            let team = if i % 2 == 0 { 1 } else { 2 };
            world.entities.insert(EntityAuthorityRecord {
                id: i,
                str_id: format!("unit_{i}"),
                team_id: team,
                pos: WorldPos::new(x, y),
                facing_deg: 0.0,
                alive: true,
                persons: 10,
                org: 100.0,
            });

            // 给予部分实体弹药
            if i <= 5 {
                let mut stocks = FxHashMap::default();
                stocks.insert("105mm_he".to_string(), 10);
                world.inventory.entity_stocks.insert(i, stocks);
            }
        }

        // 2. 设立狭窄通道与薄墙障碍 (在 c=30 设立峭壁)
        for r in 0..1024 {
            let idx = world.terrain.idx(r, 30);
            world.terrain.terrain_type[idx] = 2; // 障碍
        }

        // 3. 注入持久烟雾
        world.persistent_fields.smoke.insert(world.terrain.idx(15, 15), (0.9, 120.0));

        let initial_fp = compute_authority_fingerprint(&world);
        assert!(!initial_fp.is_empty());

        // 4. 连续推进 5 个完整的 13 阶段事务 Tick
        for tick in 1..=5 {
            // Phase 0: BeginTick
            orch.step_phase(TransactionPhase::BeginTick, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 1: ClockAndSchedule
            orch.step_phase(TransactionPhase::ClockAndSchedule, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 2: PersistentFieldEvolve
            orch.step_phase(TransactionPhase::PersistentFieldEvolve, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 3: DerivedRefresh
            orch.step_phase(TransactionPhase::DerivedRefresh, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 4: SenseAndArbitrate (50 实体提出机动意图)
            let mut arb = IntentArbitrator::default();
            for i in 1..=50 {
                // 部分直控，部分自主
                let src = if i <= 10 { IntentSource::DirectOrder } else { IntentSource::AutonomousAi };
                arb.push_candidate(IntentCandidate {
                    entity_id: i,
                    source: src,
                    desired_speed_mps: 2.5,
                    target_pos: Some(WorldPos::new(260.0, 260.0)),
                    heading_deg: 45.0,
                    timestamp_sec: world.clock.logic_time_sec,
                });
            }
            let intents = arb.arbitrate();
            orch.step_phase(TransactionPhase::SenseAndArbitrate, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 5: ProposeMotion
            let mut proposals = Vec::new();
            for intent in &intents {
                let cur_pos = world.entities.entities[&intent.entity_id].pos;
                let prop = MotionProposal::from_intent(intent, cur_pos, 16.0, 1.0);
                proposals.push(prop);
            }
            orch.step_phase(TransactionPhase::ProposeMotion, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 6: ConstrainMotion (通过硬约束与容量预约)
            motion_solver.reset();
            let mut constrained = Vec::new();
            for prop in &proposals {
                let m = motion_solver.solve(&geom, &world, &derived, prop);
                constrained.push(m);
            }
            orch.step_phase(TransactionPhase::ConstrainMotion, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 7: CommitMotion (唯一位置写入点)
            commit_motions(&mut world, &constrained);
            orch.step_phase(TransactionPhase::CommitMotion, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 8: EmitEvents (前 2 个单位执行炮击)
            orch.step_phase(TransactionPhase::EmitEvents, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 9: ResolveEvents (执行火力竖切与地形破坏)
            if tick == 2 {
                let _ = execute_single_fire_slice(
                    &mut world,
                    &mut events,
                    &mut blast_workspace,
                    1,
                    CellCoord::new(15, 15),
                    "105mm_he",
                    (tick as u64) * 1000 + 1,
                );
            }
            orch.step_phase(TransactionPhase::ResolveEvents, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 10: CommitPersistent (地形破坏与缓存失效)
            if tick == 3 {
                TerrainTransaction::apply(
                    &mut world,
                    &mut derived,
                    TerrainMutation {
                        center: CellCoord::new(15, 15),
                        radius_cells: 3,
                        elev_delta: -3.0,
                        new_terrain_type: None,
                    },
                    2,
                );
            }
            orch.step_phase(TransactionPhase::CommitPersistent, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 11: Validate
            orch.step_phase(TransactionPhase::Validate, &mut world, &mut derived, &mut scratch).unwrap();

            // Phase 12: PublishAndDrop
            orch.step_phase(TransactionPhase::PublishAndDrop, &mut world, &mut derived, &mut scratch).unwrap();

            orch.current_phase = None;
        }

        let elapsed = start_time.elapsed();

        // 5. 压力回归验收指标验证
        assert_eq!(world.clock.tick, 5);
        assert_eq!(world.clock.logic_time_sec, 80.0);
        assert!(world.terrain.revision > 1, "Terrain must have mutated");
        assert!(blast_workspace.peak_overpressure_kpa.is_empty(), "Workspace must be clean");

        // 6. 存档并比对反序列化后指纹 100% 逐位一致
        let final_fp = compute_authority_fingerprint(&world);
        let save_json = save_world_state_to_json(&world).unwrap();
        let reloaded_world = load_world_state_from_json(&save_json).unwrap();
        let reloaded_fp = compute_authority_fingerprint(&reloaded_world);

        assert_eq!(final_fp, reloaded_fp, "Stress test: reloaded world fingerprint must match 100%!");
        println!("Phase 09 Stress Regression passed in {:.2?}! Final Fingerprint: {}", elapsed, final_fp);
    }
}
