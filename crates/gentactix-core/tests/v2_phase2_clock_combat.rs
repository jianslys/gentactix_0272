use gentactix_core::catalog::ItemCatalog;
use gentactix_core::clock::{
    constrain_segment_motion, find_path_world, check_los, TickOrchestrator, TickStage,
};
use gentactix_core::math_ops::{CellCoord, ConstructionFsmState, SchmidtPostureState, WorldPos};
use gentactix_core::ruleset::{
    ObjectiveSpec, PlatoonSpec, ScenarioSpec, SimRuleset, TerrainSourceSpec, UnitSpawnSpec,
};
use gentactix_core::state::{DerivedState, TickScratch, WorldState};
use gentactix_core::terrain::TerrainKind;
use gentactix_core::types::{
    EngineerWorkKind, FireDiscipline, MobilityKind, MoveStance, Team, UnitId, UnitOrder, UnitRole,
};

fn spawn_basic_unit(
    world: &mut WorldState,
    catalog: &ItemCatalog,
    callsign: &str,
    team: Team,
    platoon_id: Option<u32>,
    role: UnitRole,
    pos: WorldPos,
    weapon_id: &str,
    ammo_id: &str,
    mount_ids: Vec<String>,
) -> UnitId {
    world
        .spawn_unit(
            &UnitSpawnSpec {
                callsign: callsign.into(),
                team,
                platoon_id,
                role,
                mobility: MobilityKind::Foot,
                pos,
                facing_deg: 90.0,
                men: 9,
                hp: 100.0,
                armor_mm: 0.0,
                weapon_id: weapon_id.into(),
                ammo_id: ammo_id.into(),
                fuze_id: None,
                mount_ids,
                ammo_rounds: 600,
                smoke_grenades: 2,
                base_speed_mps: 1.6,
                vision_m: 800.0,
                base_radio_range_m: 900.0,
            },
            catalog,
        )
        .unwrap()
}

fn build_platoon_battle_world() -> (WorldState, ItemCatalog) {
    let catalog = ItemCatalog::embedded().unwrap();
    let spec = ScenarioSpec {
        name: "V2 Phase 2 Skirmish".into(),
        epoch_seconds: 23400,
        night: false,
        ruleset: SimRuleset::hardcore_platoon(),
        terrain: TerrainSourceSpec::Flat {
            rows: 64,
            cols: 64,
            base_elevation_m: 40.0,
        },
        platoons: vec![
            PlatoonSpec {
                id: 1,
                name: "Red 1st".into(),
                team: Team::Red,
                hq_unit_id: None,
            },
            PlatoonSpec {
                id: 2,
                name: "Blue 1st".into(),
                team: Team::Blue,
                hq_unit_id: None,
            },
        ],
        units: vec![
            UnitSpawnSpec {
                callsign: "Red-HQ".into(),
                team: Team::Red,
                platoon_id: Some(1),
                role: UnitRole::PlatoonHq,
                mobility: MobilityKind::Foot,
                pos: WorldPos::new(60.0, 200.0),
                facing_deg: 90.0,
                men: 5,
                hp: 70.0,
                armor_mm: 0.0,
                weapon_id: "wpn_luger_p08".into(),
                ammo_id: "ammo_9x19_m08_ball".into(),
                fuze_id: None,
                mount_ids: vec![],
                ammo_rounds: 64,
                smoke_grenades: 2,
                base_speed_mps: 1.6,
                vision_m: 600.0,
                base_radio_range_m: 900.0,
            },
            UnitSpawnSpec {
                callsign: "Red-MG1".into(),
                team: Team::Red,
                platoon_id: Some(1),
                role: UnitRole::HeavyWeaponTeam,
                mobility: MobilityKind::Foot,
                pos: WorldPos::new(140.0, 200.0),
                facing_deg: 90.0,
                men: 6,
                hp: 100.0,
                armor_mm: 0.0,
                weapon_id: "wpn_mg42".into(),
                ammo_id: "ammo_792x57_ss".into(),
                fuze_id: None,
                mount_ids: vec!["mount_lafette_42".into()],
                ammo_rounds: 1200,
                smoke_grenades: 2,
                base_speed_mps: 1.4,
                vision_m: 800.0,
                base_radio_range_m: 900.0,
            },
            UnitSpawnSpec {
                callsign: "Blue-Sq1".into(),
                team: Team::Blue,
                platoon_id: Some(2),
                role: UnitRole::InfantrySquad,
                mobility: MobilityKind::Foot,
                pos: WorldPos::new(340.0, 200.0),
                facing_deg: 270.0,
                men: 9,
                hp: 100.0,
                armor_mm: 0.0,
                weapon_id: "wpn_mg42".into(),
                ammo_id: "ammo_792x57_ss".into(),
                fuze_id: None,
                mount_ids: vec!["mount_zweibein_34_42".into()],
                ammo_rounds: 800,
                smoke_grenades: 2,
                base_speed_mps: 1.6,
                vision_m: 800.0,
                base_radio_range_m: 900.0,
            },
        ],
        objectives: vec![ObjectiveSpec {
            id: "obj_1".into(),
            name: "Hamlet".into(),
            center: WorldPos::new(244.0, 204.0),
            radius_m: 24.0,
            owner: None,
        }],
    };
    let world = spec.build_world(&catalog).unwrap();
    (world, catalog)
}

#[test]
fn test_v2_p2_pathfinding_detours_river_and_blocks_tunneling() {
    let mut world = WorldState::new_blank(32, 32, 10.0, SimRuleset::arcade_sandbox());
    // River barrier along col = 16 except a bridge ford at row = 20.
    for r in 0..32 {
        let k = if r == 20 { TerrainKind::Bridge } else { TerrainKind::Water };
        world.terrain.mutate_cell(CellCoord::new(r, 16), None, Some(k), None, None).unwrap();
    }

    let start = WorldPos::new(68.0, 100.0); // (12, 8)
    let goal = WorldPos::new(196.0, 100.0); // (12, 24)
    let clamped = constrain_segment_motion(&world.terrain, MobilityKind::Foot, start, goal);
    assert!(
        clamped.x < 128.0,
        "anti-tunneling sweep must stop west of Water col 16 (x=128..136), got {clamped:?}"
    );

    let path = find_path_world(&world.terrain, MobilityKind::Foot, start, goal, 10_000)
        .expect("A* must find path across the bridge at row 20");
    assert!(
        path.iter().any(|p| p.y >= 160.0 && p.x >= 128.0 && p.x <= 136.0),
        "path must cross the bridge around row 20 (y=164): {path:?}"
    );
}

#[test]
fn test_v2_p2_full_scale_8m_los_and_firefight_with_suppression() {
    let (mut world, catalog) = build_platoon_battle_world();
    let mut derived = DerivedState::new(world.terrain.rows, world.terrain.cols);
    let mut scratch = TickScratch::default();

    // 140 -> 340 = 200m = 25 cells on 8m grid (strictly > legacy 15-cell cutoff!).
    let los_clear = check_los(
        &world.terrain,
        WorldPos::new(140.0, 200.0),
        WorldPos::new(340.0, 200.0),
        800.0,
        false,
    );
    assert!(los_clear.visible && los_clear.dist_m >= 199.0);

    // Execute 3 ticks -> all 13 stages run, ammo drops, HP/men drop, suppression rises to Pinned/Prone!
    let reports = TickOrchestrator::step_n_ticks(&mut world, &mut derived, &mut scratch, &catalog, 3);
    assert_eq!(reports[0].stages_executed, TickStage::ALL.to_vec());
    assert!(reports.iter().any(|r| r.fire_events > 0));
    assert!(world.units[&2].ammo_rounds < 1200, "MG42 must consume ammo");
    assert!(world.units[&3].hp < 100.0, "Blue squad must take damage at 200m (25 cells)");
    assert!(
        world.units[&3].suppression > 0.35 || world.units[&3].posture != SchmidtPostureState::Active,
        "sustained MG42 fire must suppress Blue squad"
    );

    // Popping smoke mid-screen blocks LOS completely.
    world.terrain.mutate_cell(CellCoord::new(25, 30), None, None, None, Some(0.9)).unwrap();
    let los_smoked = check_los(
        &world.terrain,
        WorldPos::new(140.0, 200.0),
        WorldPos::new(340.0, 200.0),
        800.0,
        false,
    );
    assert!(!los_smoked.visible && los_smoked.blocked_by_smoke);
}

#[test]
fn test_v2_p2_deygout_radio_attenuates_behind_ridge_and_recovers() {
    let catalog = ItemCatalog::embedded().unwrap();
    let mut world = WorldState::new_blank(32, 32, 20.0, SimRuleset::hardcore_platoon());
    let pl = world.spawn_platoon("Red".into(), Team::Red);
    let _hq = spawn_basic_unit(
        &mut world,
        &catalog,
        "Red-HQ",
        Team::Red,
        Some(pl.0),
        UnitRole::PlatoonHq,
        WorldPos::new(20.0, 100.0),
        "wpn_luger_p08",
        "ammo_9x19_m08_ball",
        vec![],
    );
    let sq = spawn_basic_unit(
        &mut world,
        &catalog,
        "Red-Sq",
        Team::Red,
        Some(pl.0),
        UnitRole::InfantrySquad,
        WorldPos::new(100.0, 100.0),
        "wpn_mg42",
        "ammo_792x57_ss",
        vec!["mount_zweibein_34_42".into()],
    );

    // Put a 95m ridge halfway between HQ (20,100) and Squad (100,100) at x=60 -> cell (12, 7).
    world
        .terrain
        .mutate_cell(CellCoord::new(12, 7), Some(95.0), Some(TerrainKind::Mountain), None, None)
        .unwrap();

    let mut derived = DerivedState::new(32, 32);
    let mut scratch = TickScratch::default();

    // Run 8 ticks behind ridge: effective radius drops below base 900m, and MUST NOT decay further on subsequent ticks!
    TickOrchestrator::step_n_ticks(&mut world, &mut derived, &mut scratch, &catalog, 2);
    let r_tick2 = world.units[&sq.0].effective_radio_range_m;
    TickOrchestrator::step_n_ticks(&mut world, &mut derived, &mut scratch, &catalog, 6);
    let r_tick8 = world.units[&sq.0].effective_radio_range_m;
    assert!(r_tick2 < 800.0 && (r_tick2 - r_tick8).abs() < 1e-3, "must attenuate without exponential decay");

    // Now level the ridge (or move clear): radio range immediately recovers toward base 900m!
    world
        .terrain
        .mutate_cell(CellCoord::new(12, 7), Some(-95.0), Some(TerrainKind::Plain), None, None)
        .unwrap();
    TickOrchestrator::step_16s(&mut world, &mut derived, &mut scratch, &catalog);
    assert!(
        (world.units[&sq.0].effective_radio_range_m - 900.0).abs() < 1e-3,
        "radio radius must recover statelessly to 900m once line clears, got {}",
        world.units[&sq.0].effective_radio_range_m
    );
}

#[test]
fn test_v2_p2_engineer_augmented_arc_and_irreversible_g_work() {
    let catalog = ItemCatalog::embedded().unwrap();
    let mut world = WorldState::new_blank(32, 32, 10.0, SimRuleset::hardcore_platoon());
    let eng = spawn_basic_unit(
        &mut world,
        &catalog,
        "Eng-1",
        Team::Red,
        None,
        UnitRole::EngineerSquad,
        WorldPos::new(20.0, 20.0),
        "wpn_luger_p08",
        "ammo_9x19_m08_ball",
        vec![],
    );

    let wp0 = WorldPos::new(28.0, 20.0); // cell (2, 3)
    let wp1 = WorldPos::new(36.0, 20.0); // cell (2, 4)
    let cidx0 = world.terrain.idx(world.terrain.geom.world_to_cell(wp0).unwrap()).unwrap();
    world.g_work_remaining.insert(cidx0, 80.0);

    world.units.get_mut(&eng.0).unwrap().order = UnitOrder::BuildWork {
        work: EngineerWorkKind::FoxholeLine,
        waypoints: vec![wp0, wp1],
    };

    let mut derived = DerivedState::new(32, 32);
    let mut scratch = TickScratch::default();

    // Tick 1: reaches wp0 and finishes work there (9 men consume 40+ work).
    let rep1 = TickOrchestrator::step_16s(&mut world, &mut derived, &mut scratch, &catalog);
    assert_eq!(rep1.engineer_events, 1);
    assert!(world.g_work_remaining[&cidx0] < 80.0, "G^work must be irreversibly decremented");
    assert!(world.terrain.cover_bonus[cidx0] > 0.0, "completed foxhole must add cover bonus");

    // Tick 2 under heavy suppression (> 0.60): FSM enters PinnedPause cleanly!
    world.units.get_mut(&eng.0).unwrap().suppression = 2.0; // decays by 0.6412 -> ~1.28 > 0.60
    let rem_before_pin = world.g_work_remaining[&cidx0];
    TickOrchestrator::step_16s(&mut world, &mut derived, &mut scratch, &catalog);
    assert_eq!(world.engineer_integrators[&eng.0].fsm_state, ConstructionFsmState::PinnedPause);
    assert_eq!(world.g_work_remaining[&cidx0], rem_before_pin, "pinned pause freezes work");

    // Clear suppression -> completes work and walks away -> G^work remains irreversible!
    world.units.get_mut(&eng.0).unwrap().suppression = 0.0;
    TickOrchestrator::step_16s(&mut world, &mut derived, &mut scratch, &catalog);
    let rem_after = world.g_work_remaining[&cidx0];
    world.units.get_mut(&eng.0).unwrap().order = UnitOrder::Move {
        waypoints: vec![WorldPos::new(100.0, 100.0)],
        stance: MoveStance::Normal,
    };
    TickOrchestrator::step_n_ticks(&mut world, &mut derived, &mut scratch, &catalog, 3);
    assert_eq!(world.g_work_remaining[&cidx0], rem_after, "leaving site never rebounds G^work");
    derived.invalidate();
    derived.rebuild_if_needed(&world);
    assert_eq!(derived.g_field_cells[&cidx0].g0, rem_after);
}

#[test]
fn test_v2_p2_direct_order_overrides_g_field_autonomy() {
    let catalog = ItemCatalog::embedded().unwrap();
    let mut world = WorldState::new_blank(32, 32, 10.0, SimRuleset::hardcore_platoon());
    world.objectives.push(ObjectiveSpec {
        id: "obj_east".into(),
        name: "East Hill".into(),
        center: WorldPos::new(180.0, 100.0),
        radius_m: 16.0,
        owner: None,
    });
    let sq_direct = spawn_basic_unit(
        &mut world,
        &catalog,
        "Direct-Sq",
        Team::Red,
        None,
        UnitRole::InfantrySquad,
        WorldPos::new(40.0, 100.0),
        "wpn_mg42",
        "ammo_792x57_ss",
        vec![],
    );
    let sq_auto = spawn_basic_unit(
        &mut world,
        &catalog,
        "Auto-Sq",
        Team::Red,
        None,
        UnitRole::InfantrySquad,
        WorldPos::new(60.0, 100.0),
        "wpn_mg42",
        "ammo_792x57_ss",
        vec![],
    );

    // Direct order tells Direct-Sq to move NORTH (y=20), opposite the east objective (x=180).
    world.units.get_mut(&sq_direct.0).unwrap().order = UnitOrder::Move {
        waypoints: vec![WorldPos::new(40.0, 20.0)],
        stance: MoveStance::Fast,
    };
    world.units.get_mut(&sq_auto.0).unwrap().fire_discipline = FireDiscipline::HoldFire;

    let mut derived = DerivedState::new(32, 32);
    let mut scratch = TickScratch::default();
    TickOrchestrator::step_16s(&mut world, &mut derived, &mut scratch, &catalog);

    assert!(world.units[&sq_direct.0].pos.y < 100.0, "DirectOrder must move North and never be hijacked by G-field");
    assert!(world.units[&sq_auto.0].pos.x > 60.0, "idle Auto-Sq must win G-field auction and advance East toward objective");
}

#[test]
fn test_v2_p2_20_tick_battle_determinism_and_save_load_bit_exact() {
    let (mut world_a, catalog) = build_platoon_battle_world();
    world_a.units.get_mut(&2).unwrap().order = UnitOrder::Move {
        waypoints: vec![WorldPos::new(220.0, 200.0)],
        stance: MoveStance::Normal,
    };
    let mut derived_a = DerivedState::new(64, 64);
    let mut scratch_a = TickScratch::default();

    TickOrchestrator::step_n_ticks(&mut world_a, &mut derived_a, &mut scratch_a, &catalog, 10);
    let saved = world_a.save_json().unwrap();
    let mut world_b = WorldState::load_json(&saved).unwrap();
    let mut derived_b = DerivedState::new(64, 64);
    let mut scratch_b = TickScratch::default();

    let reps_a = TickOrchestrator::step_n_ticks(&mut world_a, &mut derived_a, &mut scratch_a, &catalog, 10);
    let reps_b = TickOrchestrator::step_n_ticks(&mut world_b, &mut derived_b, &mut scratch_b, &catalog, 10);

    assert_eq!(world_a.tick, 20);
    assert_eq!(world_a.authoritative_sha256(), world_b.authoritative_sha256());
    for i in 0..10 {
        assert_eq!(reps_a[i].authoritative_sha256, reps_b[i].authoritative_sha256);
    }
    assert!(scratch_a.is_completely_empty() && scratch_b.is_completely_empty());
}
