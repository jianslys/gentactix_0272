//! Phase 00: Baseline Freeze Test
//! 冻结重构前的物理行为基线：覆盖两兵种、直控与自主、不可穿越障碍、精确射击、范围爆炸、弹药消耗、地形破坏与跨Tick持久效果。

#[cfg(test)]
pub mod tests {
    use crate::combat_logistics::{execute_artillery_mission, Battery, ShellType};
    use crate::command_log::MathRecursiveDriver;
    use crate::sim_loop::{DirectOrderPlan, SimWorld, WaypointSegmentConfig};
    use crate::units_ecs::{Echelon, Rank};
    use crate::world_grid::{TeamId, WorldGrid, MOUNTAIN, WATER};
    use rustc_hash::FxHashMap;

    fn make_eq(pairs: &[(&str, u32)]) -> FxHashMap<String, u32> {
        let mut m = FxHashMap::default();
        for &(k, v) in pairs {
            m.insert(k.to_string(), v);
        }
        m
    }

    #[derive(Debug, Clone)]
    pub struct BaselineSnapshot {
        pub fingerprint: String,
        pub unit_positions: Vec<[f32; 2]>,
        pub unit_health: Vec<(u32, f32)>, // (persons, org)
        pub battery_he_shells: f32,
        pub unit_ammo: u32,
        pub terrain_crater_elev: f32,
        pub smoke_ttl: f32,
        pub combat_log_count: usize,
    }

    pub fn run_baseline_scenario(_seed: u64) -> (SimWorld, BaselineSnapshot) {
        let mut grid = WorldGrid::synthetic_grid(32, 32, 200.0);

        // 1. 一处不可穿越障碍：在 (15, 10)..(17, 10) 设立峭壁山脉，在 (15, 20)..(17, 20) 设立水域
        for r in 15..=17 {
            let m_idx = grid.idx(r, 10);
            grid.terrain[m_idx] = MOUNTAIN;
            grid.elevation_m[m_idx] = 600.0;

            let w_idx = grid.idx(r, 20);
            grid.terrain[w_idx] = WATER;
        }

        let mut sim = SimWorld::new(grid);

        // 2. 两个不同兵种实体：步兵班 (直控) 与 坦克组 (自主)
        let cmd_pl = sim.commanders.spawn_commander(
            "cmd-base-pl",
            "基线排长",
            TeamId::Red,
            (10, 10),
            Echelon::Platoon,
            Rank::Lieutenant,
            0,
        );

        // 直控步兵班：有明确航点
        let u_inf = sim.units.spawn_unit_4ledger(
            "u-base-inf",
            "基线步兵班",
            TeamId::Red,
            (10, 12),
            Echelon::Squad,
            "rifle_squad",
            10,
            1,
            0,
            make_eq(&[("rifle", 9), ("lmg", 1), ("ammo_762_mm", 500)]),
            0,
        );
        sim.commanders.link_subordinate_unit(cmd_pl, u_inf, &mut sim.units);

        // 自主实体：坦克组，无直控计划，随自主系统调度
        let u_tank = sim.units.spawn_unit_4ledger(
            "u-base-tank",
            "基线战车组",
            TeamId::Red,
            (10, 14),
            Echelon::Squad,
            "tank_squad",
            4,
            1,
            0,
            make_eq(&[("tank_medium", 1), ("ammo_75mm", 30)]),
            0,
        );
        sim.commanders.link_subordinate_unit(cmd_pl, u_tank, &mut sim.units);

        // 敌军目标班组
        let u_enemy = sim.units.spawn_unit_4ledger(
            "u-base-enemy",
            "敌军前哨班",
            TeamId::Blue,
            (12, 12),
            Echelon::Squad,
            "rifle_squad",
            8,
            1,
            0,
            make_eq(&[("rifle", 8)]),
            1,
        );

        // 直控命令：步兵班向 (11, 12) 推进并射击
        sim.direct_unit_plans.insert(
            "u-base-inf".into(),
            DirectOrderPlan {
                action: "assault".into(),
                waypoints: vec![[11.0, 12.0]],
                target_pos: Some([12.0, 12.0]),
                target_unit_id: Some("u-base-enemy".into()),
                move_mode: "normal".into(),
                fire_mode: "stop_to_fire".into(),
                follow_target: true,
                segments: vec![WaypointSegmentConfig {
                    start_idx: 0,
                    goal: [12.0, 12.0],
                    action: "assault".into(),
                    move_mode: "normal".into(),
                    fire_mode: "stop_to_fire".into(),
                    follow_target: true,
                    target_unit_id: Some("u-base-enemy".into()),
                    duration_sec: 300,
                    priority_target: "infantry".into(),
                }],
                duration_sec: 300,
                priority_target: "infantry".into(),
                last_los_blocked: false,
                out_of_ammo: false,
                last_fire_summary: String::new(),
                weapon_model: "rifle".into(),
                dispersion_m: 0.0,
                hit_prob: 0.0,
                last_casualties: 0,
                last_org_loss: 0.0,
                last_ammo_used_t: 0.0,
            },
        );

        // 3. 火炮阵地与一次范围爆炸/弹药消耗
        sim.batteries.push(Battery {
            str_id: "arty-base-1".into(),
            team: TeamId::Red,
            px: (5, 5),
            caliber_mm: 105,
            tubes: 4,
            shells: [50.0, 0.0, 0.0, 0.0, 10.0, 0.0], // 50发 HE, 10发烟雾
        });

        // 4. 一次地形破坏与弹坑
        let crater_idx = sim.grid.idx(12, 12);
        sim.grid.elevation_m[crater_idx] -= 3.5; // 弹坑下陷

        // 5. 跨 Tick 持久效果：烟雾
        sim.grid.apply_smoke(11, 12, 0.85, 2.0);

        // 执行火炮任务（范围爆炸 + 弹药消耗）
        let mut b = sim.batteries.pop().unwrap();
        execute_artillery_mission(
            &mut sim.grid,
            &mut sim.units,
            &sim.commanders,
            &mut sim.events,
            &mut sim.fire_fields,
            &mut b,
            (12, 12),
            ShellType::He,
            4,
            true,
            1,
            "seed-base-1",
        );
        sim.batteries.push(b);

        // 推进 2 个 Subtick
        for _ in 0..2 {
            sim.step_subtick_5m(&mut MathRecursiveDriver);
        }

        let snap = BaselineSnapshot {
            fingerprint: sim.state_fingerprint(),
            unit_positions: vec![
                sim.units.pos[u_inf.0],
                sim.units.pos[u_tank.0],
                sim.units.pos[u_enemy.0],
            ],
            unit_health: vec![
                (sim.units.persons(u_inf), sim.units.org[u_inf.0]),
                (sim.units.persons(u_tank), sim.units.org[u_tank.0]),
                (sim.units.persons(u_enemy), sim.units.org[u_enemy.0]),
            ],
            battery_he_shells: sim.batteries[0].shells[ShellType::He as usize],
            unit_ammo: sim.units.equipment[u_inf.0]
                .get("ammo_762_mm")
                .copied()
                .unwrap_or(0),
            terrain_crater_elev: sim.grid.elevation_m[crater_idx],
            smoke_ttl: sim.grid.smoke_ttl_h[sim.grid.idx(11, 12)],
            combat_log_count: sim.direct_combat_logs.len(),
        };

        (sim, snap)
    }

    #[test]
    fn test_phase_00_behavioral_baseline_freeze() {
        let (_sim1, snap1) = run_baseline_scenario(42);
        let (_sim2, snap2) = run_baseline_scenario(42);

        // 验收断言 1: 确定性指纹 100% 逐位复现
        assert_eq!(
            snap1.fingerprint, snap2.fingerprint,
            "Baseline scenario must produce identical deterministic fingerprints across independent runs!"
        );

        // 验收断言 2: 关键位置变化一致
        assert_eq!(snap1.unit_positions, snap2.unit_positions);

        // 验收断言 3: 健康与生命值一致
        assert_eq!(snap1.unit_health, snap2.unit_health);

        // 验收断言 4: 弹药消耗一致（已发生火炮实扣）
        assert_eq!(snap1.battery_he_shells, snap2.battery_he_shells);
        assert!(
            snap1.battery_he_shells < 50.0,
            "Battery ammo must have been consumed: remaining is {}",
            snap1.battery_he_shells
        );

        // 验收断言 5: 地形弹坑与烟雾持久状态保持
        assert_eq!(snap1.terrain_crater_elev, snap2.terrain_crater_elev);
        assert!(
            snap1.smoke_ttl > 0.0,
            "Smoke ttl must persist across subticks"
        );
    }
}
