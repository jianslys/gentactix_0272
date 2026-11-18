//! Gentactix V3.1 Pure-CPU Deterministic Tactical Simulation Core
//! Consolidated 6-module Rust architecture (replaces 18 fragmented Python files).

pub mod baseline_test;
pub mod catalog;
pub mod combat_logistics;
pub mod command_log;
pub mod events;
pub mod fields;
pub mod geometry;
pub mod intent;
pub mod motion;
pub mod operators;
pub mod publication;
pub mod sim_loop;
pub mod spatial_autonomy;
pub mod state;
#[cfg(test)]
pub mod stress_test;
pub mod time_machine;
pub mod transaction;
pub mod units_ecs;
pub mod world_grid;

pub use combat_logistics::{
    filter_and_dispatch_fragments, forward_152mm_he_packet, logistics_tonnage_transfer_sec,
    resolve_projectile_hit, scavenge_ground_body, unit_cross_section_weight, ProjectileHitReport,
};
pub use units_ecs::{
    Echelon, FireIntensityMode, FireTask, TaskLifecyclePhase, TaskTimingScheduler,
};
pub use world_grid::{PixelCellContainer, PriorityTarget, ShotPacket};

// -----------------------------------------------------------------------------
// GTX-CORE-SPEC-FINAL-2.0 Target Architecture Clean Exports (Phase 01..10)
// -----------------------------------------------------------------------------
pub use events::{EventBuffer, EventFinalStatus, EventKind, EventPayload, InstantEvent};
pub use fields::{execute_single_fire_slice, FieldAlgebra, TransientBlastWorkspace};
pub use geometry::{CellCoord, GridGeometry, Meter, OutsideBoundsError, WorldPos, TACTICAL_CELL_AREA_M2, TACTICAL_CELL_SHIFT, TACTICAL_CELL_SIZE_M};
pub use intent::{IntentArbitrator, IntentCandidate, IntentSource, MotionIntent};
pub use motion::{commit_motions, ConstrainedMotion, MotionConstraintSolver, MotionProposal};
pub use operators::{DirtyHaloBox, TerrainMutation, TerrainTransaction};
pub use publication::{compute_authority_fingerprint, load_world_state_from_json, save_world_state_to_json};
pub use state::{ClockState, DerivedState, EntityAuthorityRecord, EntityState, InventoryLedger, PersistentFieldState, StateKind, TerrainState, TickScratch, WorldState};
pub use time_machine::*;
pub use transaction::{DEFAULT_SUBTICK_SECONDS, TickClock, TickOrchestrator, TickSummary, TransactionError, TransactionPhase};

#[cfg(test)]
mod tests {
    use super::combat_logistics::{
        apply_registration_to_cells, cast_los_ray_dda, compute_directional_cover_eff,
        dispatch_logistics_step, execute_artillery_mission, gather_intel, resolve_cell_strike,
        resolve_registration_bonus, Battery, Depot, FireFields, ShellType,
    };
    use super::command_log::{
        can_commander_write_line, collect_commander_uplink_logs, get_commander_readable_scope,
        harvest_nearby_friendly_intel, register_zone, update_war_councils, CommanderDriver,
        DownlinkPipeline, LinkVia, MathRecursiveDriver,
    };
    use super::sim_loop::{build_demo_world, SimWorld};
    use super::units_ecs::{
        aggregate_modifiers, compute_registration_delta, set_modifier_slot,
        tick_overload_faults_5m, AlertMark, CommanderId, Echelon, LockFlags, ModifierSlot,
        ModifierSource, Rank, ReorgTargetSpec, UnitId,
    };
    use super::world_grid::{
        FrontlineRole, MobilityClass, TeamId, WorldGrid, BRIDGE, FOREST, HIGHWAY, LINE_MASK_ALL,
        LINE_MASK_INFANTRY, MOUNTAIN, PLAIN, WATER,
    };
    use rustc_hash::FxHashMap;

    fn eq(pairs: &[(&str, u32)]) -> FxHashMap<String, u32> {
        let mut m = FxHashMap::default();
        for &(k, v) in pairs {
            m.insert(k.to_string(), v);
        }
        m
    }

    #[test]
    fn test_world_grid_r16_slope_literal_ground_and_windowed_astar() {
        let mut raw = vec![0u8; 64 * 64 * 2];
        for r in 0..64 {
            for c in 0..64 {
                let v: u16 = if c == 32 { 65535 } else { (r * 40 + c * 25) as u16 };
                let idx = (r * 64 + c) * 2;
                let le = v.to_le_bytes();
                raw[idx] = le[0];
                raw[idx + 1] = le[1];
            }
        }
        let g = WorldGrid::from_r16_bytes(&raw, None, None, 8.0, 100.0, 500.0, false).unwrap();
        assert_eq!((g.rows, g.cols), (64, 64));
        assert!(g.slope_deg.iter().any(|&s| s > 0.1));

        let syn = WorldGrid::synthetic_grid(64, 64, 8.0);
        // Column 32 is WATER everywhere except row 32 which is BRIDGE
        assert!(!syn.is_literal_ground_px(10, 32));
        assert!(!syn.is_literal_ground_px(32, 32)); // Bridge is NOT literal ground!
        let near = syn.find_nearest_literal_ground_px(32, 32);
        assert!(syn.is_literal_ground_px(near.0, near.1));
        assert_ne!(near, (32, 32));

        // Windowed A* must cross via bridge when crossing river at (32, 32)
        let path = syn
            .shortest_path_windowed((30, 30), (30, 34), MobilityClass::Wheeled, |_, _| false, 1.0)
            .expect("must find route across bridge");
        assert!(path.0.contains(&(32, 32)));
        assert_ne!(syn.terrain[syn.idx(0, 0)], WATER);
        assert_eq!(syn.terrain[syn.idx(32, 32)], BRIDGE);
        let _ = (HIGHWAY, PLAIN, FOREST, MOUNTAIN);
    }

    #[test]
    fn test_toe_locks_churn_war_council_coexistence_and_overload() {
        let mut slots: Vec<ModifierSlot> = Vec::new();
        set_modifier_slot(
            &mut slots,
            ModifierSlot {
                source: ModifierSource::OverloadGearbox,
                tag: "FAULT_GEARBOX",
                locks: LockFlags::FORBID_TOW,
                speed_mult: 0.45,
                ..Default::default()
            },
        );
        // Same-source replace must NOT stack multiplicatively to 0.45^2
        set_modifier_slot(
            &mut slots,
            ModifierSlot {
                source: ModifierSource::OverloadGearbox,
                tag: "FAULT_GEARBOX",
                locks: LockFlags::FORBID_TOW,
                speed_mult: 0.45,
                ..Default::default()
            },
        );
        let agg_same = aggregate_modifiers(&slots, 0.0, false);
        assert!((agg_same.speed_mult - 0.45).abs() < 1e-4);

        // Coexistence: Order Churn >= 2 + War Council Sync Active MUST retain BOTH tags & compensate!
        let churn_only = aggregate_modifiers(&[], 3.0, false);
        assert!(churn_only.tags.contains(&"ORDER_CHURN_DEBUFF"));
        assert!(churn_only.speed_mult < 1.0);

        let sync_coexist = aggregate_modifiers(&[], 3.0, true);
        assert!(sync_coexist.tags.contains(&"ORDER_CHURN_DEBUFF"));
        assert!(sync_coexist.tags.contains(&"WAR_COUNCIL_SYNC_BUFF"));
        assert!((sync_coexist.speed_mult - 1.08).abs() < 1e-3);
        assert!((sync_coexist.combat_power_mult - 1.10).abs() < 1e-3);
        assert!((sync_coexist.kc_mult - 1.15).abs() < 1e-3);
        assert!((sync_coexist.tau_mult - 0.75).abs() < 1e-3);

        // Direct merge <= 15 vs > 15 rejection & split + tamper guard
        let mut sim = build_demo_world();
        let u0 = UnitId(0);
        let child = sim.units.split_unit(u0, 4, 0, "red-sq-1b").unwrap();
        assert_eq!(sim.units.persons(child), 4);
        assert!(sim.units.merge_squads_direct(u0, child).is_ok());

        let big = sim.units.spawn_unit("red-big", "LargeSq", TeamId::Red, (20, 20), Echelon::Platoon, "rifle_squad", 12, 1, eq(&[("rifle", 12)]), 0);
        assert!(sim.units.merge_squads_direct(u0, big).is_err());

        assert!(sim.units.set_counter_skin(u0, Some("#00ff00"), None, Some("精锐一班"), Some("division")).is_err());
        assert!(sim.units.set_counter_skin(u0, Some("#00ff00"), None, Some("精锐一班"), None).is_ok());

        // Overload fault deterministic check
        sim.units.overload_ratio[0] = 2.0;
        sim.units.moved_last_turn[0] = true;
        let mut saw_fault = false;
        for t in 1..40 {
            if !tick_overload_faults_5m(&mut sim.units, 0.25, t).is_empty() {
                saw_fault = true;
                break;
            }
        }
        assert!(saw_fault);
    }

    #[test]
    fn test_command_hierarchy_ao_rumor_and_recursive_frontlines() {
        let mut g = WorldGrid::synthetic_grid(128, 128, 8.0);
        let mut units = super::units_ecs::UnitSoA::new();
        let mut cmds = super::units_ecs::CommanderSoA::new();

        let co = cmds.spawn_commander("co-1", "1Co", TeamId::Red, (20, 20), Echelon::Company, Rank::Captain, 0);
        let pl1 = cmds.spawn_commander("pl-1", "1Pl", TeamId::Red, (20, 21), Echelon::Platoon, Rank::Lieutenant, 0);
        let pl2 = cmds.spawn_commander("pl-2", "2Pl", TeamId::Red, (70, 70), Echelon::Platoon, Rank::Lieutenant, 0);
        let pl_other = cmds.spawn_commander("pl-other", "OtherPl", TeamId::Red, (20, 21), Echelon::Platoon, Rank::Lieutenant, 1); // diff channel but within 400m!
        cmds.link_subordinate_cmd(co, pl1);
        cmds.link_subordinate_cmd(co, pl2);

        let u1 = units.spawn_unit("u1", "S1", TeamId::Red, (20, 21), Echelon::Squad, "rifle_squad", 10, 1, eq(&[("rifle", 10)]), 0);
        let u2 = units.spawn_unit("u2", "S2", TeamId::Red, (125, 125), Echelon::Squad, "rifle_squad", 10, 1, eq(&[("rifle", 10), ("radio", 1)]), 0);
        let u_other = units.spawn_unit("u_other", "SOther", TeamId::Red, (20, 20), Echelon::Squad, "rifle_squad", 8, 1, eq(&[("rifle", 8)]), 2);
        cmds.link_subordinate_unit(co, u1, &mut units);
        cmds.link_subordinate_unit(pl2, u2, &mut units);

        // Single-commander write permission
        assert!(can_commander_write_line(co, co));
        assert!(!can_commander_write_line(co, pl1));

        // Put u2 on radio silence -> disconnected from co & pl2 remotely (> 400m)
        units.radio_silence[u2.0] = true;
        let scope = get_commander_readable_scope(co, &units, &cmds, &g);
        assert!(scope.readable_unit_ids.contains(&u1));
        assert!(scope.proximity_unit_ids.contains(&u_other)); // <= 400m cross-hierarchy read!
        assert!(scope.proximity_cmd_ids.contains(&pl_other));
        assert!(!scope.readable_unit_ids.contains(&u2));

        // Disconnected uplinks produce NO uplink report for u2
        let pl2_logs = collect_commander_uplink_logs(pl2, &units, &cmds, &g);
        assert!(pl2_logs.is_empty());

        // Rumor amplification when nearby friendly disconnected units gossip within 400m
        units.was_disconnected[u1.0] = true;
        units.was_disconnected[u_other.0] = true;
        let (est, amp) = harvest_nearby_friendly_intel(&units, u1, &g, 10.0);
        assert!(amp > 1.29 && est > 12.9);

        // Same-echelon AO overlap rejection
        let mut zones = Vec::new();
        let ao1_cells = vec![(20, 20), (20, 21), (21, 20), (21, 21)];
        assert!(register_zone(&mut zones, "z1".into(), pl1, Echelon::Platoon, ao1_cells.clone(), 0, 0, &cmds, &mut g).is_ok());
        assert!(register_zone(&mut zones, "z2".into(), pl_other, Echelon::Platoon, ao1_cells, 0, 0, &cmds, &mut g).is_err());

        // MathRecursiveDriver partitions parent line among connected subordinates + tau delivery + bridging move line
        cmds.assigned_parent_line_pts[co.0] = vec![[18.0, 22.0], [26.0, 22.0]];
        let mut drv = MathRecursiveDriver;
        let mut pipe = DownlinkPipeline::default();
        if let Some(sub_orders) = drv.evaluate_downlink(co, 1.0, Echelon::Company.cadence_ticks(), &g, &units, &cmds) {
            for (sc, pts, norm, mask) in sub_orders {
                assert!(pipe.queue_downlink_to_connected(co, sc, 1.0, FrontlineRole::Deploy, mask, pts, norm, &cmds, &units, &g));
            }
        }
        assert_eq!(pipe.tick_deliver(1.01, &mut g, &mut cmds), 0); // tau not elapsed yet!
        assert!(pipe.tick_deliver(1.20, &mut g, &mut cmds) >= 1); // tau elapsed!

        // Deadzone gate (< 2px & < 15 deg) suppresses redundant churn
        let assigned_pts = cmds.assigned_parent_line_pts[pl1.0].clone();
        assert!(!pipe.record_order_churn(&mut cmds, pl1, &assigned_pts, 1.25));
        let _ = (LinkVia::Radio, update_war_councils(&mut cmds, &mut units, &mut g, 0.25));
    }

    #[test]
    fn test_autonomy_provenance_stealth_ambush_and_reorg_abort_on_attack() {
        let mut sim = build_demo_world();
        let deploy_id = sim.grid.write_typed_frontline(
            "dep-1",
            TeamId::Red,
            1,
            0,
            FrontlineRole::Deploy,
            0,
            0,
            LINE_MASK_ALL,
            1.0,
            None,
            &[[22.0, 25.0], [26.0, 25.0]],
            4,
        );
        // Squad 0 claims deploy_id
        sim.units.active_deploy_line_id[0] = deploy_id;
        sim.units.active_deploy_line_id[1] = 9999; // Different source deploy line!

        // MOVE_LINE sourced ONLY from `deploy_id`
        sim.grid.write_typed_frontline(
            "mv-1",
            TeamId::Red,
            1,
            0,
            FrontlineRole::Move,
            deploy_id,
            0,
            LINE_MASK_INFANTRY,
            1.5,
            None,
            &[[23.0, 25.0], [23.0, 28.0]],
            6,
        );
        let samp0 = sim.grid.sample_ao_and_frontline_field(sim.units.pos[0], TeamId::Red, LINE_MASK_INFANTRY, deploy_id, Some(1));
        let samp1 = sim.grid.sample_ao_and_frontline_field(sim.units.pos[1], TeamId::Red, LINE_MASK_INFANTRY, 9999, Some(1));
        assert!(samp0.has_move_or_attack);
        assert!(!samp1.has_move_or_attack); // Unit on other deploy line ignores unrelated attack/move line!

        // Bridge reorganization test: anchor on (32, 32) BRIDGE must abort when HQ attacked, and spawn on literal ground when completed
        let pl_cid = CommanderId(1);
        sim.commanders.px[pl_cid.0] = (32, 32);
        sim.units.px[0] = (32, 32);
        sim.units.pos[0] = [32.0, 32.0];
        let ev_abort = sim.events.start_reorganize_echelon(
            &mut sim.units,
            &sim.commanders,
            pl_cid,
            Some(UnitId(0)),
            Echelon::Platoon,
            vec![UnitId(0), UnitId(1)],
            Vec::new(),
            false,
            Some(1.0),
        ).unwrap();
        assert!(sim.units.effective_locks(UnitId(0), &sim.events.events).contains(LockFlags::HOLD_STILL));

        // Enemy fires on HQ unit -> MUST abort reorganization immediately!
        resolve_cell_strike(
            &mut sim.grid,
            &mut sim.units,
            &sim.commanders,
            &mut sim.events,
            (32, 32),
            TeamId::Blue,
            [12.0, 5.0, 0.0, 0.0, 0.0, 0.0],
            "rifle",
            [32.0, 34.0],
            true,
            "abort-test",
        );
        assert!(sim.units.reorganizing_event_id[0].is_none(), "attack on commander unit must abort reorganization");
        let _ = ev_abort;

        // Restart reorganization with cadre=0 allowed -> let time finish -> verify literal ground spawn (NOT bridge 32,32)
        let _ev2 = sim.events.start_reorganize_echelon(
            &mut sim.units,
            &sim.commanders,
            pl_cid,
            Some(UnitId(0)),
            Echelon::Platoon,
            vec![UnitId(0), UnitId(1)],
            vec![ReorgTargetSpec {
                str_id: "reorg-empty-cadre".into(),
                name: "空干部整编排".into(),
                echelon: Echelon::Platoon,
                enlisted: 12,
                cadre: 0,
                equipment: vec![("rifle".into(), 10)],
            }],
            true,
            Some(0.25),
        ).unwrap();
        let before_count = sim.units.id.len();
        while !sim.events.events.is_empty() {
            sim.step_subtick_5m(&mut MathRecursiveDriver);
        }
        assert!(sim.units.id.len() > before_count);
        let spawned_u = UnitId(before_count);
        assert!(sim.grid.is_literal_ground_px(sim.units.px[spawned_u.0].0, sim.units.px[spawned_u.0].1));
        assert!(sim.units.empty_cadre[spawned_u.0]);
        assert!(sim.units.org_cap[spawned_u.0] <= 65.0);
    }

    #[test]
    fn test_combat_registration_recon_logistics_conservation_and_determinism() {
        let mut g = WorldGrid::synthetic_grid(48, 48, 8.0);
        let mut units = super::units_ecs::UnitSoA::new();
        let mut cmds = super::units_ecs::CommanderSoA::new();
        let mut eq_ev = super::units_ecs::EventQueue::default();

        // Shared registration across company recon + ballistic decay check
        let co = cmds.spawn_commander("co", "Co", TeamId::Red, (20, 20), Echelon::Company, Rank::Captain, 0);
        let recon = units.spawn_unit("rec", "Recon", TeamId::Red, (20, 21), Echelon::Squad, "recon_squad", 6, 1, eq(&[("rifle", 6), ("radio", 1)]), 0);
        let gun = units.spawn_unit("gun", "AT", TeamId::Red, (20, 22), Echelon::Squad, "at_squad", 5, 1, eq(&[("at_gun_50", 1)]), 0);
        cmds.link_subordinate_unit(co, recon, &mut units);
        cmds.link_subordinate_unit(co, gun, &mut units);

        apply_registration_to_cells(&mut g, recon, &units, &cmds, (20, 26), 1.0, "rifle", 1000.0, 15.0, true);
        let gun_bonus = resolve_registration_bonus(&g, gun, &units, &cmds, (20, 26), super::units_ecs::BallisticGroup::DirectAntiTank);
        assert!(gun_bonus > 0.0, "recon penetration must share registration company-wide");
        let (d1, cap1) = compute_registration_delta("rifle", 50.0, 15.0);
        assert!(d1 > 0.5 && cap1 >= 0.59);

        // Directional cos(theta_i) front vs rear
        let (c_front, fm_front, _) = compute_directional_cover_eff([20.0, 25.0], 270.0, [20.0, 20.0], 0.6);
        let (c_rear, fm_rear, _) = compute_directional_cover_eff([20.0, 25.0], 90.0, [20.0, 20.0], 0.6);
        assert!(c_front > 0.5 && fm_front < 1.1);
        assert!(c_rear == 0.0 && fm_rear > 1.7);

        // Artillery smoke + strike + recon fog (infantry hides facing)
        let en = units.spawn_unit("en-inf", "EnInf", TeamId::Blue, (20, 26), Echelon::Squad, "rifle_squad", 10, 1, eq(&[("rifle", 10)]), 1);
        units.facing_deg[en.0] = 180.0;
        let intel = gather_intel(TeamId::Red, &g, &units, false);
        assert!(intel.iter().any(|c| c.unit_id == en && c.facing_deg.is_none()));

        let mut bat = Battery { str_id: "bat-1".into(), team: TeamId::Red, px: (20, 15), caliber_mm: 105, tubes: 4, shells: [20.0; 6] };
        let mut ff = FireFields::default();
        execute_artillery_mission(&mut g, &mut units, &cmds, &mut eq_ev, &mut ff, &mut bat, (20, 26), ShellType::Smoke, 2, true, 1, "sm");
        assert!(cast_los_ray_dda(&g, (20, 21), (20, 26), false).visibility < 0.35);
        execute_artillery_mission(&mut g, &mut units, &cmds, &mut eq_ev, &mut ff, &mut bat, (20, 26), ShellType::He, 4, true, 1, "he");

        // Logistics exact mass conservation check
        units.food_t[recon.0] = 0.0;
        units.ammo_t[recon.0] = 0.0;
        let mut depots = vec![Depot { str_id: "dep".into(), team: TeamId::Red, px: (20, 18), food_t: 10.0, ammo_t: 10.0, fuel_t: 10.0, replacements: 5.0 }];
        let logi_rep = dispatch_logistics_step(&g, &mut depots, &mut units, &cmds, &[], &ff, 1.0, 1);
        assert!(logi_rep.delivered_total_t > 0.0);
        assert!(logi_rep.conservation_error < 1e-4);

        // Two independent multi-subtick determinism verification!
        let mut w1: SimWorld = build_demo_world();
        let mut w2: SimWorld = build_demo_world();
        let r1 = w1.run_turn_default(1.0); // 225 x 16s subticks
        let r2 = w2.run_turn_default(1.0);
        assert_eq!(r1.subtick_count, 225);
        assert_eq!(r1.last_fingerprint, r2.last_fingerprint, "100% deterministic replay failed!");
        let _ = AlertMark::None;
    }

    #[test]
    fn test_v32_four_ledger_form_command_squad_threat_no_avoidance_and_segments() {
        use super::command_log::{
            commander_has_officer_or_acting_nco, effective_commander_max_area_px,
            effective_commander_radius_km,
        };
        use super::spatial_autonomy::{step_world_autonomy_5m, SpatialBucketGrid, ORG_ROUT_THRESHOLD};
        use super::units_ecs::{AtomicAction, CommanderSoA, EventQueue, UnitSoA, ORG_CAP_FLOOR};

        // 1. 4-ledger caps & form_command_squad promotion + demotion
        let mut sim = build_demo_world();
        let sq = sim.units.spawn_unit_4ledger(
            "v32-sq-nco",
            "NCO测试班",
            TeamId::Red,
            (18, 18),
            Echelon::Squad,
            "rifle_squad",
            8,
            1,
            0,
            eq(&[("rifle", 8), ("lmg", 1), ("radio", 1)]),
            0,
        );
        assert_eq!(sim.units.nco[sq.0], 1);
        assert_eq!(sim.units.officer[sq.0], 0);
        assert!(sim.units.can_operate_radio[sq.0]);
        assert!(sim.units.org_cap[sq.0] >= 75.0);

        // Loss of all NCO + officer locks org cap to ORG_CAP_FLOOR (25.0), disables radio, degrades heavy_op_cap
        sim.units.set_nco_officer(sq, 0, 0);
        assert!(sim.units.org_cap[sq.0] <= ORG_CAP_FLOOR + 0.01);
        assert!(!sim.units.can_operate_radio[sq.0]);
        assert!(sim.units.heavy_op_cap[sq.0] < 1.0);

        // Restore 1 NCO and form command squad
        sim.units.set_nco_officer(sq, 1, 0);
        assert!(sim.units.can_operate_radio[sq.0]);
        sim.events
            .start_form_command_squad(&sim.units, sq, "临时排部".into(), true, 0.1)
            .unwrap();
        while !sim.events.events.is_empty() {
            sim.step_subtick_5m(&mut MathRecursiveDriver);
        }
        assert_eq!(sim.units.kind[sq.0], "hq_squad");
        let new_cid = (0..sim.commanders.id.len())
            .find(|&i| sim.commanders.hq_piece_id[i] == Some(sq))
            .map(CommanderId)
            .expect("form_command_squad must spawn/link a commander");
        assert_eq!(
            commander_has_officer_or_acting_nco(new_cid, &sim.commanders, &sim.units),
            (false, true)
        );
        let r_acting = effective_commander_radius_km(new_cid, &sim.commanders, &sim.units, &sim.grid);
        let a_acting = effective_commander_max_area_px(new_cid, &sim.commanders);
        assert!(r_acting > 0.0 && r_acting <= Rank::Sergeant.base_radius_km() * 0.65 + 0.05);
        assert!(a_acting > 0 && a_acting <= Rank::Sergeant.ao_max_area_px());

        // Losing remaining NCO demotes hq_squad and locks command authority
        sim.units.set_nco_officer(sq, 0, 0);
        sim.step_subtick_5m(&mut MathRecursiveDriver);
        assert_ne!(sim.units.kind[sq.0], "hq_squad");
        assert_eq!(
            commander_has_officer_or_acting_nco(new_cid, &sim.commanders, &sim.units),
            (false, false)
        );
        assert_eq!(
            effective_commander_radius_km(new_cid, &sim.commanders, &sim.units, &sim.grid),
            0.0
        );

        // 2. "Threats belong to commander judgment": connected squad ignores threat avoidance when ordered,
        // and triggers irreversible automatic retreat once org <= ORG_ROUT_THRESHOLD (12.0)
        let mut g = WorldGrid::synthetic_grid(48, 48, 8.0);
        let mut units = UnitSoA::new();
        let mut cmds = CommanderSoA::new();
        let evs = EventQueue::default();
        let mut buckets = SpatialBucketGrid::new(48, 48);

        let c_id = cmds.spawn_commander("c-v32", "V32-Pl", TeamId::Red, (20, 20), Echelon::Platoon, Rank::Lieutenant, 0);
        let u_adv = units.spawn_unit_4ledger(
            "u-adv",
            "突击班",
            TeamId::Red,
            (20, 20),
            Echelon::Squad,
            "rifle_squad",
            8,
            1,
            0,
            eq(&[("rifle", 8), ("radio", 1)]),
            0,
        );
        cmds.link_subordinate_unit(c_id, u_adv, &mut units);
        // Enemy sitting right ahead (+east at col 24, out of attack range or blocked by forest if needed — put at (20, 24) behind forest/mountain? No, just check facing & non-westward drift!)
        let en = units.spawn_unit_4ledger(
            "u-en",
            "蓝军阻击",
            TeamId::Blue,
            (20, 24),
            Echelon::Squad,
            "rifle_squad",
            10,
            1,
            0,
            eq(&[("rifle", 10)]),
            1,
        );
        let blk_idx = g.idx(20, 22);
        g.terrain[blk_idx] = MOUNTAIN; // block direct LOS so unit chooses MOVE rather than holding to shoot
        g.elevation_m[blk_idx] = g.elev_max_m;
        let mv_id = g.write_typed_frontline(
            "fl-adv-east",
            TeamId::Red,
            c_id.0,
            0,
            FrontlineRole::Move,
            0,
            0,
            LINE_MASK_ALL,
            2.0,
            None,
            &[[20.0, 25.0], [20.0, 27.0]],
            4,
        );
        units.active_deploy_line_id[u_adv.0] = 0;
        let _ = (mv_id, en);

        step_world_autonomy_5m(&mut g, &mut units, &cmds, &mut buckets, &evs.events, &[], 5.0 / 60.0, false);
        assert!(
            units.pos[u_adv.0][1] > 20.0,
            "connected ordered squad must advance along command vector (+east) instead of fleeing west from threat; got col={}",
            units.pos[u_adv.0][1]
        );
        assert_ne!(units.autonomy_state[u_adv.0], AtomicAction::Retreat);

        // Collapse org below ORG_ROUT_THRESHOLD -> triggers irreversible auto-retreat
        units.org[u_adv.0] = ORG_ROUT_THRESHOLD - 1.0;
        step_world_autonomy_5m(&mut g, &mut units, &cmds, &mut buckets, &evs.events, &[], 5.0 / 60.0, false);
        assert_eq!(units.autonomy_state[u_adv.0], AtomicAction::Retreat);
        assert!(units.retreat_recovery_hours[u_adv.0] > 0.0);

        // 3. Direct assault + locked segment[0] vs editable segment[1] + irreversible retreat lock
        let u2 = sim.units.spawn_unit_4ledger(
            "v32-u2",
            "二班",
            TeamId::Red,
            (6, 6),
            Echelon::Squad,
            "rifle_squad",
            9,
            1,
            0,
            eq(&[("rifle", 9)]),
            0,
        );
        sim.try_set_direct_unit_order_ex(
            u2,
            "assault",
            Some([8.0, 8.0]),
            false,
            None,
            "fast",
            "fire_on_the_move",
            true,
        )
        .unwrap();
        sim.try_set_direct_unit_order_ex(
            u2,
            "move",
            Some([10.0, 10.0]),
            true,
            None,
            "normal",
            "stop_to_fire",
            false,
        )
        .unwrap();
        assert!(sim.update_queued_segment(u2, 2, Some("move"), None, None, None, None).is_err());
        assert!(sim.update_queued_segment(u2, 0, Some("move"), None, None, None, None).is_ok());
        assert!(sim
            .update_queued_segment(u2, 1, Some("assault"), Some("slow"), Some("fire_on_the_move"), Some(true), Some([11.0, 11.0]))
            .is_ok());

        sim.try_set_direct_unit_order_ex(
            u2,
            "retreat",
            None,
            false,
            None,
            "fast",
            "fire_on_the_move",
            false,
        )
        .unwrap();
        assert!(sim.units.retreat_recovery_hours[u2.0] > 0.0);
        assert!(sim
            .try_set_direct_unit_order_ex(u2, "move", Some([5.0, 5.0]), false, None, "normal", "stop_to_fire", false)
            .is_err());
    }

    #[test]
    fn test_v32_rust_core_five_bugfixes() {
        use super::units_ecs::{
            equipment_spec, BallisticGroup, Echelon, LockFlags, ModifierSlot, ModifierSource,
            Rank, ReorgTargetSpec,
        };

        // Bug 5 check: catalog items mapped to correct ballistic & equipment specs
        let mg42_spec = equipment_spec("wpn_mg42");
        assert_eq!(mg42_spec.model, "lmg");
        assert_eq!(mg42_spec.range_m, 800.0);

        let pak40_spec = equipment_spec("wpn_pak40");
        assert_eq!(pak40_spec.ballistic_group, BallisticGroup::DirectAntiTank);
        assert!(pak40_spec.is_heavy);

        let flak88_spec = equipment_spec("wpn_flak36_88");
        assert_eq!(flak88_spec.model, "flak_88");
        assert_eq!(flak88_spec.range_m, 8000.0);

        let grw34_spec = equipment_spec("wpn_grw34");
        assert_eq!(grw34_spec.model, "mortar_81");
        assert!(grw34_spec.indirect);

        let radio_spec = equipment_spec("comm_torn_fu_d2");
        assert_eq!(radio_spec.model, "radio");

        // Bug 2 check: heavy weapon preservation on casualty sync
        let mut sim = SimWorld::new(WorldGrid::synthetic_grid(32, 32, 8.0));
        let mut gun_eq = FxHashMap::default();
        gun_eq.insert("rifle".to_string(), 4);
        gun_eq.insert("at_gun_50".to_string(), 1);
        let gun_unit = sim.units.spawn_unit_4ledger(
            "gun-sq",
            "反坦克炮班",
            TeamId::Red,
            (10, 10),
            Echelon::Squad,
            "at_squad",
            3,
            1,
            0,
            gun_eq,
            0,
        );
        assert_eq!(*sim.units.equipment[gun_unit.0].get("at_gun_50").unwrap_or(&0), 1);
        // Simulate a casualty and run sync_totals_from_soldiers
        sim.units.soldiers[gun_unit.0][0].status = "dead".to_string();
        sim.units.sync_totals_from_soldiers(gun_unit);
        assert_eq!(
            *sim.units.equipment[gun_unit.0].get("at_gun_50").unwrap_or(&0),
            1,
            "heavy crew-served weapon at_gun_50 must NOT be erased on casualty sync"
        );
        assert_eq!(sim.units.persons(gun_unit), 3);

        // Bug 3 check: split squad drains soldiers list and prevents phantom revival
        let u_parent = sim.units.spawn_unit_4ledger(
            "parent-sq",
            "步兵母班",
            TeamId::Red,
            (15, 15),
            Echelon::Squad,
            "infantry_squad",
            8,
            2,
            0,
            FxHashMap::default(),
            0,
        );
        assert_eq!(sim.units.soldiers[u_parent.0].len(), 10);
        let u_child = sim.units.split_unit(u_parent, 4, 1, "child-sq").unwrap();
        assert_eq!(sim.units.persons(u_child), 5);
        assert_eq!(sim.units.persons(u_parent), 5);
        assert_eq!(sim.units.soldiers[u_parent.0].len(), 5);
        assert_eq!(sim.units.soldiers[u_child.0].len(), 5);
        // Sync parent again -> must NOT resurrect drained soldiers
        sim.units.sync_totals_from_soldiers(u_parent);
        assert_eq!(sim.units.persons(u_parent), 5);

        // Bug 4 check: reorganization preserves officer and does not demote to 0
        let u_hq = sim.units.spawn_unit_4ledger(
            "hq-src",
            "营部指挥班",
            TeamId::Red,
            (20, 20),
            Echelon::Squad,
            "hq_squad",
            4,
            2,
            1,
            FxHashMap::default(),
            0,
        );
        let u_line = sim.units.spawn_unit_4ledger(
            "line-src",
            "警卫班",
            TeamId::Red,
            (20, 21),
            Echelon::Squad,
            "infantry_squad",
            8,
            1,
            0,
            FxHashMap::default(),
            0,
        );
        let cmdr = sim.commanders.spawn_commander("cmdr-co", "营长", TeamId::Red, (20, 20), Echelon::Battalion, Rank::Major, 0);
        let reorg_targets = vec![ReorgTargetSpec {
            str_id: "reorg-bat-hq".to_string(),
            name: "整编营指挥部".to_string(),
            echelon: Echelon::Company,
            enlisted: 10,
            cadre: 3,
            equipment: Vec::new(),
        }];
        let new_units = super::units_ecs::complete_reorganize_echelon(
            &sim.grid,
            &mut sim.units,
            &mut sim.commanders,
            cmdr,
            None,
            Echelon::Company,
            &[u_hq, u_line],
            &reorg_targets,
            false,
        );
        assert!(!new_units.is_empty());
        let reorg_u = new_units[0];
        assert!(
            sim.units.officer[reorg_u.0] >= 1,
            "reorganized echelon must preserve officer count >= 1 instead of demoting to 0"
        );
        assert_eq!(sim.units.cadre[reorg_u.0], 4);

        // Bug 1 check: step_direct_orders_5m respects LockFlags
        let u_locked = sim.units.spawn_unit_4ledger(
            "u-locked",
            "被锁定班组",
            TeamId::Red,
            (5, 5),
            Echelon::Squad,
            "infantry_squad",
            8,
            1,
            0,
            FxHashMap::default(),
            0,
        );
        sim.try_set_direct_unit_order_ex(
            u_locked,
            "move",
            Some([10.0, 10.0]),
            false,
            None,
            "normal",
            "stop_to_fire",
            false,
        ).unwrap();
        // Insert a HOLD_STILL / FORBID_MOVE lock
        let mut slot = ModifierSlot::default();
        slot.source = ModifierSource::EventWork;
        slot.locks = LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE;
        sim.units.modifier_slots[u_locked.0].push(slot);

        let initial_pos = sim.units.pos[u_locked.0];
        let moved = sim.step_direct_orders_5m();
        assert_eq!(moved, 0, "locked unit must not move in step_direct_orders_5m");
        assert_eq!(sim.units.pos[u_locked.0], initial_pos);
    }

    #[test]
    fn test_v33_rust_contract_and_physics() {
        use super::combat_logistics::cast_los_ray_dda;
        use super::sim_loop::{plan_grid_path_f32, SimWorld};
        use super::units_ecs::{heavy_crew_buff_mult, Echelon, Rank};
        use super::world_grid::{
            CellModEntry, FeatureFlags, MobilityClass, TeamId, WorldGrid, BRIDGE, HIGHWAY, WATER,
        };
        use rustc_hash::FxHashMap;

        // 1. Model-coupled speed + multi-cell 5m step budget (> 2.4km/h cap) + stealth debuff
        let mut g = WorldGrid::new_plain(48, 48, 8.0);
        for c in 2..=30 {
            let idx = g.idx(10, c);
            g.terrain[idx] = HIGHWAY;
        }
        g.sync_feature_flags_from_terrain();
        let mut sim = SimWorld::new(g);

        let mut truck_eq = FxHashMap::default();
        truck_eq.insert("truck".to_string(), 1);
        truck_eq.insert("rifle".to_string(), 2);
        let u_truck = sim.units.spawn_unit_4ledger(
            "v33-truck", "卡车分队", TeamId::Red, (10, 4), Echelon::Squad, "truck", 2, 0, 0, truck_eq, 0,
        );
        sim.units.vehicle_model[u_truck.0] = Some("truck".into());
        sim.units.refresh_caps(u_truck);

        let mut tank_eq = FxHashMap::default();
        tank_eq.insert("tank_heavy".to_string(), 1);
        let u_tank = sim.units.spawn_unit_4ledger(
            "v33-tank", "重坦分队", TeamId::Red, (12, 4), Echelon::Squad, "armor", 3, 1, 0, tank_eq, 0,
        );
        sim.units.vehicle_model[u_tank.0] = Some("tank_heavy".into());
        sim.units.refresh_caps(u_tank);

        let mut inf_eq = FxHashMap::default();
        inf_eq.insert("rifle".to_string(), 8);
        let u_inf = sim.units.spawn_unit_4ledger(
            "v33-inf", "徒步班", TeamId::Red, (14, 4), Echelon::Squad, "infantry_squad", 7, 1, 0, inf_eq, 0,
        );

        let spd_truck = sim.units.effective_speed_kmh(u_truck, &sim.grid, 10, 4, 10, 5, "move", "normal");
        let spd_tank = sim.units.effective_speed_kmh(u_tank, &sim.grid, 12, 4, 12, 5, "move", "normal");
        let spd_inf = sim.units.effective_speed_kmh(u_inf, &sim.grid, 14, 4, 14, 5, "move", "normal");
        assert!(spd_truck > spd_tank && spd_tank > spd_inf, "expected truck({spd_truck}) > tank({spd_tank}) > inf({spd_inf})");

        let spd_stealth = sim.units.effective_speed_kmh(u_inf, &sim.grid, 14, 4, 14, 5, "move", "stealth");
        assert!(spd_stealth < spd_inf, "stealth must apply real speed debuff");

        sim.try_set_direct_unit_order_ex(u_truck, "move", Some([10.0, 28.0]), false, None, "fast", "stop_to_fire", false).unwrap();
        let moved = sim.step_direct_orders_5m();
        assert!(moved >= 1);
        let d_px = sim.units.pos[u_truck.0][1] - 4.0;
        assert!(d_px > 1.0, "truck in 16s subtick must traverse > 1.0 cells, got {d_px}");

        // 2. Fast A* pathfinding crosses BRIDGE to avoid WATER river
        for r in 0..48 {
            let idx = sim.grid.idx(r, 20);
            sim.grid.terrain[idx] = if r == 25 { BRIDGE } else { WATER };
        }
        sim.grid.sync_feature_flags_from_terrain();
        let path = plan_grid_path_f32(&sim.grid, [20.0, 17.0], [20.0, 23.0], MobilityClass::Wheeled);
        assert!(path.iter().any(|p| (p[0] - 25.0).abs() <= 1.2 && (p[1] - 20.0).abs() <= 1.2), "fast path should route via bridge at row 25, got {path:?}");

        // 3. Heavy weapon detach (unmanned) + crew Buff scaling + commander link + pack restore
        let cmdr = sim.commanders.spawn_commander("c-v33", "三连长", TeamId::Red, (30, 8), Echelon::Company, Rank::Captain, 0);
        let mut hvy_eq = FxHashMap::default();
        hvy_eq.insert("rifle".to_string(), 6);
        hvy_eq.insert("hmg".to_string(), 1);
        let u_mother = sim.units.spawn_unit_4ledger(
            "v33-mother", "机枪母班", TeamId::Red, (30, 8), Echelon::Squad, "infantry_squad", 7, 1, 0, hvy_eq, 0,
        );
        sim.commanders.link_subordinate_unit(cmdr, u_mother, &mut sim.units);
        let init_ammo = sim.units.ammo_t[u_mother.0];

        let emp_id = sim.execute_deploy_heavy(u_mother, "hmg", false, &[], 2, 101).unwrap();
        assert_eq!(sim.units.persons(emp_id), 0, "detached emplacement must have persons == 0");
        assert!(sim.commanders.subordinate_unit_ids[cmdr.0].contains(&emp_id), "emplacement must link to commander subordinates");
        let pow_2 = sim.units.unit_power(emp_id, 300.0, true, Some("hmg"), 1.0)[0];
        assert!(pow_2 > 0.0, "operated unmanned emplacement must have > 0 firepower");

        let sids_4: Vec<String> = sim.units.soldiers[u_mother.0].iter().take(4).map(|s| s.id.clone()).collect();
        sim.units.set_heavy_operators(emp_id, Some(u_mother), sids_4, 4).unwrap();
        let pow_4 = sim.units.unit_power(emp_id, 300.0, true, Some("hmg"), 1.0)[0];
        assert!(pow_4 > pow_2, "4-man crew buff must boost hmg power: pow_4={pow_4} vs pow_2={pow_2}");
        assert!((heavy_crew_buff_mult(4, "hmg") - 1.32).abs() < 1e-4);

        sim.execute_pack_heavy(u_mother, emp_id, false).unwrap();
        assert!(!sim.units.alive[emp_id.0]);
        assert!(!sim.commanders.subordinate_unit_ids[cmdr.0].contains(&emp_id));
        assert_eq!(*sim.units.equipment[u_mother.0].get("hmg").unwrap_or(&0), 1);
        assert!(sim.units.ammo_t[u_mother.0] >= init_ammo - 1e-4);

        // 4. Engineer capability & adjacency gate + clear_mines
        assert!(sim.events.start_engineering(&sim.units, u_inf, "lay_mines", (14, 15), 0.25).is_err(), "non-engineer must not lay mines");
        assert!(sim.events.start_demolition(&sim.units, u_inf, (14, 5), "bridge", 0.25).is_err(), "non-engineer must not perform demolition");
        let mut eng_eq = FxHashMap::default();
        eng_eq.insert("demo_charge".to_string(), 2);
        eng_eq.insert("mine_detector".to_string(), 1);
        let u_eng = sim.units.spawn_unit_4ledger(
            "v33-eng", "工兵班", TeamId::Red, (34, 8), Echelon::Squad, "engineer_squad", 5, 1, 0, eng_eq, 0,
        );
        assert!(sim.events.start_engineering(&sim.units, u_eng, "clear_mines", (34, 12), 0.25).is_err(), "out-of-reach engineering must be rejected");
        let mine_gi = sim.grid.idx(34, 9);
        let mine_bits = (FeatureFlags::MINEFIELD_RED | FeatureFlags::MINEFIELD_BLUE).bits();
        sim.grid.feature_flags[mine_gi] |= mine_bits;
        sim.events.start_engineering(&sim.units, u_eng, "clear_mines", (34, 9), 0.05).unwrap();
        sim.run_turn_default(0.25);
        assert_eq!(sim.grid.feature_flags[mine_gi] & mine_bits, 0, "clear_mines must clear minefield flags");

        // 5. Unified LOS block on CellModEntry::los_block_m + follow_target stops GPS wallhack
        let mut blk_grid = WorldGrid::new_plain(16, 16, 8.0);
        blk_grid.set_cell_modifier(8, 8, CellModEntry { los_block_m: 120.0, ..CellModEntry::neutral("los_blk") });
        let los = cast_los_ray_dda(&blk_grid, (8, 5), (8, 11), false);
        assert!(los.blocked, "los_block_m obstacle must block cast_los_ray_dda");

        let mut fog_sim = SimWorld::new(blk_grid);
        let r_hunt = fog_sim.units.spawn_unit_4ledger("v33-hunt", "追击班", TeamId::Red, (8, 5), Echelon::Squad, "rifle_squad", 6, 1, 0, FxHashMap::default(), 0);
        let b_targ = fog_sim.units.spawn_unit_4ledger("v33-prey", "隐蔽蓝军", TeamId::Blue, (8, 11), Echelon::Squad, "rifle_squad", 6, 1, 0, FxHashMap::default(), 1);
        fog_sim.try_set_direct_unit_order_ex(r_hunt, "move", Some([8.0, 5.0]), false, Some(fog_sim.units.str_id[b_targ.0].clone()), "normal", "stop_to_fire", true).unwrap();
        fog_sim.step_direct_orders_5m();
        let plan = fog_sim.direct_unit_plans.get("v33-hunt").unwrap();
        assert_eq!(plan.target_pos, Some([8.0, 5.0]), "follow_target must NOT GPS-update target_pos when LOS is blocked");
    }

    #[test]
    fn test_v33_shot_packet_and_task_hourglass() {
        use super::combat_logistics::{
            filter_and_dispatch_fragments, forward_152mm_he_packet,
            logistics_tonnage_transfer_sec, resolve_projectile_hit, scavenge_ground_body,
            unit_cross_section_weight,
        };
        use super::command_log::{
            effective_commander_max_area_px, effective_commander_radius_km,
        };
        use super::units_ecs::{
            CommanderSoA, Echelon, FireIntensityMode, FireTask, GroundBody, Rank, SoldierItem,
            SoldierState, TaskLifecyclePhase, TaskTimingScheduler, UnitSoA,
        };
        use super::world_grid::{PriorityTarget, ShotPacket, TeamId, WorldGrid};
        use rustc_hash::FxHashMap;

        // ========================================================
        // 1. 射弹容器与 ShotPacket 像素寄存结构体落地
        // ========================================================
        let mut grid = WorldGrid::new_plain(32, 32, 8.0);
        assert!(grid.pixel_containers.is_empty());

        // shots < 1 强制丢弃
        let dud_packet = ShotPacket {
            target_px: (10, 10),
            origin_px: (5, 5),
            priority_target: PriorityTarget::Any,
            ammo_spec_id: "wpn_mg42".into(),
            shots: 0,
            shooter_accuracy: 0.8,
            incoming_dir_deg: 45.0,
            remaining_radius_m: 0.0,
        };
        grid.inject_shot_packet(dud_packet);
        assert!(grid.get_pixel_container(10, 10).is_none());

        // 有效射弹包寄存
        let valid_packet = ShotPacket {
            target_px: (10, 10),
            origin_px: (5, 5),
            priority_target: PriorityTarget::Infantry,
            ammo_spec_id: "wpn_mg42".into(),
            shots: 120,
            shooter_accuracy: 0.85,
            incoming_dir_deg: 90.0,
            remaining_radius_m: 0.0,
        };
        grid.inject_shot_packet(valid_packet);
        let container = grid.get_pixel_container(10, 10).expect("container must exist");
        assert_eq!(container.incoming_packets.len(), 1);
        assert_eq!(container.incoming_packets[0].shots, 120);

        // ========================================================
        // 2. 三段式破片整数 floor 过滤与 152mm 跨格衰减
        // ========================================================
        let mut units = UnitSoA::new();
        let empty_hits = filter_and_dispatch_fragments(1000, (10, 10), &grid, &units, &[]);
        assert!(empty_hits.is_empty(), "empty cell must yield 0 fragment hits");

        // 单兵极小截面测试：1 人 (W_i ≈ 0.008)，10 发破片 -> 10 * 0.008 = 0.08 < 1 -> 向下取整为 0
        let mut rifle_eq = FxHashMap::default();
        rifle_eq.insert("rifle".into(), 1);
        let u_lone = units.spawn_unit_4ledger(
            "lone", "落单散兵", TeamId::Red, (10, 10), Echelon::Team, "infantry", 1, 0, 0, rifle_eq, 0,
        );
        let w_lone = unit_cross_section_weight(u_lone, &units);
        assert!(w_lone > 0.0 && w_lone <= 0.015);
        let small_burst_hits = filter_and_dispatch_fragments(10, (10, 10), &grid, &units, &[u_lone]);
        assert!(small_burst_hits.is_empty(), "10 frags on 1 man must floor to 0 effective hits");

        // 10人标准班组在 1000 发 152mm 破片面覆盖下的三段式结算
        let mut sq_eq = FxHashMap::default();
        sq_eq.insert("rifle".into(), 10);
        let u_squad = units.spawn_unit_4ledger(
            "sq10", "标准步兵班", TeamId::Red, (10, 10), Echelon::Squad, "rifle_squad", 9, 1, 0, sq_eq, 0,
        );
        let hits_squad = filter_and_dispatch_fragments(1000, (10, 10), &grid, &units, &[u_squad]);
        assert!(!hits_squad.is_empty());
        assert_eq!(hits_squad[0].0, u_squad);
        assert!(hits_squad[0].1 >= 40 && hits_squad[0].1 <= 80);

        // 152mm HE 跨格转发：向 8 个邻格派发向下取整新 ShotPacket，半径扣减 200m
        let he152_packet = ShotPacket {
            target_px: (15, 15),
            origin_px: (10, 15),
            priority_target: PriorityTarget::Any,
            ammo_spec_id: "152mm".into(),
            shots: 1000,
            shooter_accuracy: 0.9,
            incoming_dir_deg: 0.0,
            remaining_radius_m: 250.0,
        };
        let forwarded = forward_152mm_he_packet(&mut grid, &he152_packet);
        assert_eq!(forwarded.len(), 8, "152mm must forward to 4 orthogonal + 4 diagonal neighbors");
        for fp in &forwarded {
            assert_eq!(fp.remaining_radius_m, 50.0);
            assert!(fp.shots > 0);
        }
        let second_fwd = forward_152mm_he_packet(&mut grid, &forwarded[0]);
        assert!(second_fwd.is_empty(), "packets with remaining_radius <= 100m must not forward further");

        // ========================================================
        // 3. 任务与事件统一时序沙漏状态机 (TaskTimingScheduler)
        // ========================================================
        let mut sched = TaskTimingScheduler::new_for_echelon(Echelon::Squad);
        assert_eq!(sched.base_prep_sec, 2);
        assert_eq!(sched.base_transition_sec, 2);
        assert_eq!(sched.base_tail_sec, 2);
        assert_eq!(sched.turn_budget_remaining_sec, 16);

        sched.init_queue(2);
        assert_eq!(sched.phase, TaskLifecyclePhase::QueuePrep { remaining_sec: 2 });

        let mut step0_done = false;
        let mut step1_done = false;
        let mut step_completed = Vec::new();
        sched.advance_turn_budget(
            2,
            |idx| if idx == 0 { step0_done } else { step1_done },
            |idx| step_completed.push(idx),
        );
        assert_eq!(sched.phase, TaskLifecyclePhase::Executing { step_idx: 0 });
        assert_eq!(sched.turn_budget_remaining_sec, 14);

        step0_done = true;
        sched.advance_turn_budget(
            2,
            |idx| if idx == 0 { step0_done } else { step1_done },
            |idx| step_completed.push(idx),
        );
        assert_eq!(sched.phase, TaskLifecyclePhase::Executing { step_idx: 1 });
        assert_eq!(sched.turn_budget_remaining_sec, 12);

        step1_done = true;
        sched.advance_turn_budget(
            2,
            |idx| if idx == 0 { step0_done } else { step1_done },
            |idx| step_completed.push(idx),
        );
        assert_eq!(sched.phase, TaskLifecyclePhase::IdleDefend);
        assert_eq!(sched.turn_budget_remaining_sec, 10);

        // Epsilon 临界吸附验证
        assert!(TaskTimingScheduler::is_maneuver_epsilon_absorbed(4.8));
        assert!(!TaskTimingScheduler::is_maneuver_epsilon_absorbed(5.5));
        assert!(TaskTimingScheduler::is_fire_epsilon_absorbed(12, false));
        assert!(TaskTimingScheduler::is_fire_epsilon_absorbed(20, true));
        assert!(!TaskTimingScheduler::is_fire_epsilon_absorbed(20, false));

        // 不兜底指挥哲学：受击绝不主动打断！
        sched.phase = TaskLifecyclePhase::Executing { step_idx: 0 };
        sched.notify_combat_stress(false, false);
        assert_eq!(sched.phase, TaskLifecyclePhase::Executing { step_idx: 0 }, "under fire must NOT auto-abort!");
        sched.notify_combat_stress(true, false);
        assert!(matches!(sched.phase, TaskLifecyclePhase::Aborted { .. }));

        // ========================================================
        // 4. 编制层级与指挥部参谋体系扩展
        // ========================================================
        assert_eq!(Echelon::from_str("team"), Echelon::Team);
        assert_eq!(Echelon::from_str("组"), Echelon::Team);
        assert_eq!(Echelon::from_str("half_platoon"), Echelon::HalfPlatoon);
        assert_eq!(Echelon::from_str("半排"), Echelon::HalfPlatoon);
        assert_eq!(Echelon::Team.as_str(), "team");
        assert_eq!(Echelon::HalfPlatoon.as_str(), "half_platoon");
        assert_eq!(Echelon::Team.cadence_ticks(), 1);
        assert_eq!(Echelon::HalfPlatoon.cadence_ticks(), 2);

        let mut cmds = CommanderSoA::new();
        let c_pl = cmds.spawn_commander("pl_c", "排长", TeamId::Red, (5, 5), Echelon::Platoon, Rank::Lieutenant, 0);
        let c_bat = cmds.spawn_commander("bat_c", "营长", TeamId::Red, (6, 6), Echelon::Battalion, Rank::Major, 0);

        assert_eq!(cmds.staff_full[c_pl.0], 0);
        assert_eq!(cmds.commander_staff_efficiency(c_pl), 1.0);

        assert_eq!(cmds.staff_full[c_bat.0], 4);
        assert_eq!(cmds.staff[c_bat.0], 4);
        assert_eq!(cmds.commander_staff_efficiency(c_bat), 1.0);

        let full_radius = effective_commander_radius_km(c_bat, &cmds, &units, &grid);
        let full_area = effective_commander_max_area_px(c_bat, &cmds);

        cmds.set_staff(c_bat, 2);
        assert_eq!(cmds.commander_staff_efficiency(c_bat), 0.5);
        let half_radius = effective_commander_radius_km(c_bat, &cmds, &units, &grid);
        let half_area = effective_commander_max_area_px(c_bat, &cmds);
        assert!((half_radius - full_radius * 0.5).abs() < 1e-3);
        assert_eq!(half_area, (full_area as f32 * 0.5).round() as usize);

        // ========================================================
        // 5. 载具模块化毁伤与人员高致死失血测试
        // ========================================================
        let mut tank_eq = FxHashMap::default();
        tank_eq.insert("tank_medium".into(), 1);
        let u_tank = units.spawn_unit_4ledger(
            "t1", "中坦", TeamId::Red, (20, 20), Echelon::Squad, "armor", 4, 1, 0, tank_eq, 0,
        );
        units.facing_deg[u_tank.0] = 0.0;

        let rep_rico = resolve_projectile_hit(&mut units, u_tank, "rifle", 0.0, 100.0, 1.0, "rico_test");
        assert!(!rep_rico.penetrated);
        assert_eq!(rep_rico.damage_state, "ricochet");

        let rep_pen = resolve_projectile_hit(&mut units, u_tank, "wpn_pak40", 0.0, 200.0, 1.0, "pen_test");
        assert!(rep_pen.penetrated);
        assert!(rep_pen.hit_location.starts_with("hull_front:"));

        let rep_soldier = resolve_projectile_hit(&mut units, u_squad, "rifle", 90.0, 50.0, 1.0, "wound_test");
        assert!(rep_soldier.penetrated);
        assert!(rep_soldier.is_bleeding, "bullet wound must cause bleeding debuff");

        // ========================================================
        // 6. 战场搜刮与物资装卸时序验证
        // ========================================================
        let dead_soldier = SoldierState {
            id: "dead-1".into(),
            name: "牺牲战士".into(),
            hp: 0.0,
            max_hp: 100.0,
            status: "dead".into(),
            weapon: "rifle".into(),
            ammo_t: 0.05,
            is_cadre: false,
            role: "enlisted".into(),
            items: vec![SoldierItem {
                kind: "weapon".into(),
                model: "wpn_gewehr43".into(),
                name: "G43步枪".into(),
                count: 1,
                ammo_t: 0.02,
                slot: "hand".into(),
                weight_kg: 4.3,
                grid_w: 1,
                grid_h: 1,
                grid_x: 0,
                grid_y: 0,
            }],
            cannot_fire: false,
            cannot_operate_heavy: false,
            max_hand_weight_kg: None,
            max_pack_weight_kg: None,
            pack_cols: 8,
            pack_rows: 6,
        };
        let mut body = GroundBody {
            id: "body-1".into(),
            kind: "kia".into(),
            soldier: dead_soldier,
            origin_unit_id: "sq10".into(),
            origin_unit_name: "标准步兵班".into(),
            team: TeamId::Red,
            pos: [10.0, 10.0],
            looted: false,
        };
        let init_ammo = units.ammo_t[u_squad.0];
        assert!(scavenge_ground_body(&mut units, u_squad, &mut body));
        assert!(body.looted);
        assert!(!scavenge_ground_body(&mut units, u_squad, &mut body));
        assert_eq!(*units.equipment[u_squad.0].get("wpn_gewehr43").unwrap_or(&0), 1);
        assert!(units.ammo_t[u_squad.0] > init_ammo);

        assert_eq!(logistics_tonnage_transfer_sec(0.5), 60);
        assert_eq!(logistics_tonnage_transfer_sec(2.5), 300);

        let _ = (FireIntensityMode::RapidFire, FireTask {
            target: (10.0, 10.0),
            duration_sec: 30,
            priority_target: PriorityTarget::Any,
            fire_mode: FireIntensityMode::NormalFire,
        });
    }

    #[test]
    fn test_v40_micro_tactix_step_16s_and_artillery_reload_cooldown() {
        use super::sim_loop::{build_art_16_world, SUBTICK_HOURS, SUBTICK_SECONDS};
        use super::units_ecs::{base_reload_sec_for_weapon, equipment_spec};

        // 1. Time step constants
        assert_eq!(SUBTICK_SECONDS, 16.0);
        assert!((SUBTICK_HOURS - 16.0 / 3600.0).abs() < 1e-7);

        // 2. 203mm heavy howitzer catalog & reload cycle
        let spec203 = equipment_spec("howitzer_203");
        assert_eq!(spec203.name, "203mm超重型榴弹炮");
        assert_eq!(spec203.crew, 8);
        assert_eq!(spec203.frag_radius_m, 80.0);
        assert!(spec203.indirect);
        assert_eq!(base_reload_sec_for_weapon("howitzer_203"), 92.0);
        assert_eq!(base_reload_sec_for_weapon("howitzer_152"), 32.0);
        assert_eq!(base_reload_sec_for_weapon("howitzer_105"), 16.0);

        // 3. Mathematical zero-drift continuous cooldown countdown simulation (92s reload over 16s ticks)
        let effective_cycle = 92.0f32;
        let mut cur_cd = 0.0f32;
        let time_spent = 16.0f32;

        // Tick 1: fires shot 1 at t=0, cd resets to 92s, 16s leftover -> 76s remaining
        let mut shots1 = 0u32;
        let mut t_cur1 = 0.0f32;
        while t_cur1 + cur_cd <= time_spent {
            t_cur1 += cur_cd;
            shots1 += 1;
            cur_cd = effective_cycle;
        }
        let leftover1 = time_spent - t_cur1;
        cur_cd = (cur_cd - leftover1).max(0.0);
        assert_eq!(shots1, 1);
        assert!((cur_cd - 76.0).abs() < 1e-5);

        // Ticks 2-5: reloading (76 -> 60 -> 44 -> 28 -> 12)
        for expected in [60.0f32, 44.0, 28.0, 12.0] {
            let mut shots = 0u32;
            let mut t_cur = 0.0f32;
            while t_cur + cur_cd <= time_spent {
                t_cur += cur_cd;
                shots += 1;
                cur_cd = effective_cycle;
            }
            let leftover = time_spent - t_cur;
            cur_cd = (cur_cd - leftover).max(0.0);
            assert_eq!(shots, 0);
            assert!((cur_cd - expected).abs() < 1e-5);
        }

        // Tick 6: cooldown (12s) reaches 0 at t=12s! Fires shot 2, resets to 92s, leftover (16-12=4s) -> 88s remaining
        let mut shots6 = 0u32;
        let mut t_cur6 = 0.0f32;
        while t_cur6 + cur_cd <= time_spent {
            t_cur6 += cur_cd;
            shots6 += 1;
            cur_cd = effective_cycle;
        }
        let leftover6 = time_spent - t_cur6;
        cur_cd = (cur_cd - leftover6).max(0.0);
        assert_eq!(shots6, 1);
        assert!((cur_cd - 88.0).abs() < 1e-5);

        // 4. Live SimWorld Art16 verification
        let mut w = build_art_16_world();
        let u_arty = w.units.str_to_id.get("red-arty-1").copied().expect("red-arty-1 should be spawned");
        let u_tgt = w.units.str_to_id.get("blue-sq-1").copied().expect("blue-sq-1 target should be spawned");
        let tgt_pos = w.units.pos[u_tgt.0];

        // Assign 96-second attack mission
        w.try_set_direct_unit_order_v33(
            u_arty,
            "attack",
            Some(tgt_pos),
            false,
            Some("blue-sq-1".into()),
            "normal",
            "normal",
            false,
            Some(96),
            Some("any"),
        ).unwrap();

        // Tick 1 (16s): 1st shot fires, remaining duration becomes 80s
        w.step_direct_orders_5m();
        let plan = w.direct_unit_plans.get("red-arty-1").expect("plan should be present");
        assert_eq!(plan.duration_sec, 80);
        let cd_key = "red-arty-1:howitzer_203";
        let live_cd = w.weapon_cooldowns.get(cd_key).copied().unwrap_or(0.0);
        assert!((live_cd - 76.0).abs() < 1e-4, "cooldown after 16s tick should be 76s, got {live_cd}");

        // Tick 2 (16s): reloading in progress, duration becomes 64s, cd becomes 60s
        w.step_direct_orders_5m();
        let plan2 = w.direct_unit_plans.get("red-arty-1").expect("plan should be present");
        assert_eq!(plan2.duration_sec, 64);
        let live_cd2 = w.weapon_cooldowns.get(cd_key).copied().unwrap_or(0.0);
        assert!((live_cd2 - 60.0).abs() < 1e-4, "cooldown after 32s should be 60s, got {live_cd2}");
    }

    #[test]
    fn test_phase_01_clock_16s_lossless_integer_progression() {
        use crate::command_log::MathRecursiveDriver;
        use crate::sim_loop::build_demo_world;

        let mut w = build_demo_world();
        assert_eq!(w.epoch_seconds, 23400);
        assert_eq!(w.clock_seconds, 23400);
        assert_eq!(w.clock_hms_string(), "06:30:00");

        let mut drv = MathRecursiveDriver;
        // 连续运行 225 个 16s 步长
        for _ in 0..225 {
            w.step_subtick_16s(&mut drv);
        }

        assert_eq!(w.subtick, 225);
        assert_eq!(w.clock_seconds, 23400 + 225 * 16);
        assert_eq!(w.clock_seconds, 27000);
        assert_eq!(w.clock_hms_string(), "07:30:00");
        assert!((w.clock_hours - 7.5).abs() < 1e-6);
    }

    #[test]
    fn test_phase_02_p2g_field_g2p_registration_and_consumption() {
        use crate::combat_logistics::resolve_cell_strike_ex;
        use crate::sim_loop::build_demo_world;
        use crate::world_grid::TeamId;

        let mut w = build_demo_world();
        let target_px = (24, 30); // 蓝方警戒班所在坐标
        let target_gi = w.grid.idx(target_px.0, target_px.1) as u32;

        // 验证初始状态：该格没有未处理射弹包，压制密度为 0
        assert_eq!(
            w.grid.pixel_containers.get(&target_gi).map_or(0, |c| c.incoming_packets.len()),
            0
        );
        let init_suppr = w.grid.pixel_containers.get(&target_gi).map_or(0.0, |c| c.suppression_density);
        assert_eq!(init_suppr, 0.0);

        // 执行一发 105mm 榴弹炮打击
        let rep = resolve_cell_strike_ex(
            &mut w.grid,
            &mut w.units,
            &w.commanders,
            &mut w.events,
            target_px,
            TeamId::Red,
            [12.0, 8.0, 5.0, 2.0, 0.0, 0.0],
            "howitzer_105",
            [24.0, 20.0],
            false,
            "seed-test-p2g",
            false,
            None,
        );

        assert!(rep.person_losses > 0 || rep.org_loss > 0.0);

        // 验证第二拍与第三拍契约：
        // 1. suppression_density 成功更新
        let post_suppr = w.grid.pixel_containers.get(&target_gi).expect("container should exist").suppression_density;
        assert!(post_suppr > 0.0, "suppression_density must be updated by strike, got {post_suppr}");

        // 2. incoming_packets 已在第三拍完成消费并清空
        let remaining_packets = w.grid.pixel_containers.get(&target_gi).unwrap().incoming_packets.len();
        assert_eq!(remaining_packets, 0, "incoming_packets must be consumed and cleared after G2P settlement");
    }
}
