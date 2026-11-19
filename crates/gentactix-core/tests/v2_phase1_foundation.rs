use gentactix_core::catalog::ItemCatalog;
use gentactix_core::math_ops::{CellCoord, OutsideBoundsError, WorldPos};
use gentactix_core::ruleset::{
    ObjectiveSpec, PlatoonSpec, ScenarioSpec, SimRuleset, TerrainSourceSpec, UnitSpawnSpec,
};
use gentactix_core::state::{
    DerivedState, EventRecord, EventTerminalStatus, IntentCandidate, IntentSource, MotionProposal,
    TickScratch, WorldState,
};
use gentactix_core::terrain::{TerrainGrid, TerrainKind, BINARY_TERRAIN_MAGIC};
use gentactix_core::types::{
    MobilityKind, MoveStance, Team, UnitId, UnitOrder, UnitRole, TICK_SECONDS,
};

fn build_test_scenario() -> ScenarioSpec {
    ScenarioSpec {
        name: "V2 Phase 1 Platoon Skirmish".into(),
        epoch_seconds: 23400,
        night: false,
        ruleset: SimRuleset::hardcore_platoon(),
        terrain: TerrainSourceSpec::Flat {
            rows: 64,
            cols: 64,
            base_elevation_m: 50.0,
        },
        platoons: vec![
            PlatoonSpec {
                id: 1,
                name: "1st Rifle Platoon".into(),
                team: Team::Red,
                hq_unit_id: None,
            },
            PlatoonSpec {
                id: 2,
                name: "Blue Recon Platoon".into(),
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
                pos: WorldPos::new(40.0, 120.0),
                facing_deg: 90.0,
                men: 5,
                hp: 60.0,
                armor_mm: 0.0,
                weapon_id: "wpn_luger_p08".into(),
                ammo_id: "ammo_9x19_m08_ball".into(),
                fuze_id: None,
                mount_ids: vec![],
                ammo_rounds: 64,
                smoke_grenades: 2,
                base_speed_mps: 1.5,
                vision_m: 600.0,
                base_radio_range_m: 900.0,
            },
            UnitSpawnSpec {
                callsign: "Red-Sq1".into(),
                team: Team::Red,
                platoon_id: Some(1),
                role: UnitRole::InfantrySquad,
                mobility: MobilityKind::Foot,
                pos: WorldPos::new(120.0, 120.0),
                facing_deg: 90.0,
                men: 9,
                hp: 100.0,
                armor_mm: 0.0,
                weapon_id: "wpn_mg42".into(),
                ammo_id: "ammo_792x57_ss".into(),
                fuze_id: None,
                mount_ids: vec!["mount_zweibein_34_42".into()],
                ammo_rounds: 600,
                smoke_grenades: 2,
                base_speed_mps: 1.6,
                vision_m: 600.0,
                base_radio_range_m: 800.0,
            },
            UnitSpawnSpec {
                callsign: "Blue-HQ".into(),
                team: Team::Blue,
                platoon_id: Some(2),
                role: UnitRole::PlatoonHq,
                mobility: MobilityKind::Foot,
                pos: WorldPos::new(440.0, 120.0),
                facing_deg: 270.0,
                men: 5,
                hp: 60.0,
                armor_mm: 0.0,
                weapon_id: "wpn_luger_p08".into(),
                ammo_id: "ammo_9x19_m08_ball".into(),
                fuze_id: None,
                mount_ids: vec![],
                ammo_rounds: 64,
                smoke_grenades: 2,
                base_speed_mps: 1.5,
                vision_m: 600.0,
                base_radio_range_m: 900.0,
            },
            UnitSpawnSpec {
                callsign: "Blue-HMG".into(),
                team: Team::Blue,
                platoon_id: Some(2),
                role: UnitRole::HeavyWeaponTeam,
                mobility: MobilityKind::Foot,
                pos: WorldPos::new(380.0, 140.0),
                facing_deg: 270.0,
                men: 4,
                hp: 80.0,
                armor_mm: 0.0,
                weapon_id: "wpn_mg42".into(),
                ammo_id: "ammo_792x57_pmk".into(),
                fuze_id: None,
                mount_ids: vec!["mount_lafette_42".into()],
                ammo_rounds: 1000,
                smoke_grenades: 1,
                base_speed_mps: 1.1,
                vision_m: 750.0,
                base_radio_range_m: 800.0,
            },
        ],
        objectives: vec![ObjectiveSpec {
            id: "obj_crossroads".into(),
            name: "Crossroads".into(),
            center: WorldPos::new(256.0, 256.0),
            radius_m: 40.0,
            owner: None,
        }],
    }
}

#[test]
fn test_v2_p1_8m_terrain_mutation_r16_and_binary_stream() {
    let mut grid = TerrainGrid::new_flat(32, 32, 100.0);
    assert_eq!(grid.geom.cell_size_m(), 8.0);
    assert_eq!(grid.geom.world_extent_m(), [256.0, 256.0]);
    assert_eq!(grid.revision, 1);

    let cell = CellCoord::new(10, 12);
    grid.mutate_cell(cell, Some(-4.0), Some(TerrainKind::Forest), Some(0.25), Some(0.5))
        .expect("in-bounds mutation must succeed");
    let idx = grid.idx(cell).unwrap();
    assert_eq!(grid.revision, 2);
    assert_eq!(grid.elevation_m[idx], 96.0);
    assert_eq!(grid.crater_depth_m[idx], 4.0);
    assert_eq!(grid.kind[idx], TerrainKind::Forest);
    assert_eq!(grid.cover_bonus[idx], 0.25);
    assert_eq!(grid.smoke_density[idx], 0.5);
    let halo = grid.dirty_halo.expect("mutation must expand dirty_halo");
    assert_eq!((halo.min_r, halo.max_r, halo.min_c, halo.max_c), (8, 12, 10, 14));
    let neighbor_idx = grid.idx(CellCoord::new(10, 11)).unwrap();
    assert!(grid.slope_deg[neighbor_idx] > 0.0, "Horn slope must update on neighbor");

    assert!(matches!(
        grid.mutate_cell(CellCoord::new(32, 0), Some(1.0), None, None, None),
        Err(OutsideBoundsError::InvalidCellIndex { .. })
    ));

    // Verify 4x4 synthetic 16-bit LE .r16 loader stays locked to 8.0m cells.
    let mut raw_r16 = Vec::with_capacity(32);
    for i in 0u16..16 {
        raw_r16.extend_from_slice(&(i * 4000).to_le_bytes());
    }
    let r16_grid = TerrainGrid::from_r16_bytes(&raw_r16, Some(4), Some(4), 10.0, 110.0, false)
        .expect("r16 bytes must decode cleanly");
    assert_eq!(r16_grid.geom.cell_size_m(), 8.0);
    assert!((r16_grid.elevation_m[0] - 10.0).abs() < 1e-4);
    assert!((r16_grid.elevation_m[15] - 110.0).abs() < 1e-4);

    // Verify binary terrain stream layout.
    let bin = grid.to_binary_stream();
    assert_eq!(&bin[0..8], BINARY_TERRAIN_MAGIC);
    assert_eq!(u32::from_le_bytes(bin[12..16].try_into().unwrap()), 32);
    assert_eq!(u32::from_le_bytes(bin[16..20].try_into().unwrap()), 32);
    assert_eq!(f32::from_le_bytes(bin[20..24].try_into().unwrap()), 8.0);
    assert_eq!(bin.len(), 24 + 32 * 32 * 7);
}

#[test]
fn test_v2_p1_ruleset_presets_and_scenario_json_roundtrip() {
    let hardcore = SimRuleset::hardcore_platoon();
    let standard = SimRuleset::tactical_standard();
    let sandbox = SimRuleset::arcade_sandbox();
    assert!(hardcore.enforce_radio_deygout && !hardcore.allow_runtime_map_edit);
    assert!(standard.enforce_command_radius && !standard.enforce_radio_deygout);
    assert!(!sandbox.enforce_command_radius && !sandbox.enforce_fog_of_war && sandbox.allow_runtime_map_edit);

    let catalog = ItemCatalog::embedded().expect("embedded catalog must load");
    let spec = build_test_scenario();
    let json = spec.to_json_pretty().expect("serialize scenario");
    let reparsed = ScenarioSpec::from_json(&json).expect("reparse scenario");
    assert_eq!(spec, reparsed);

    let world = reparsed.build_world(&catalog).expect("build scenario world");
    assert_eq!(world.platoons.len(), 2);
    assert_eq!(world.units.len(), 4);
    assert_eq!(world.platoons[&1].hq_unit, Some(UnitId(1)));
    assert_eq!(world.platoons[&2].hq_unit, Some(UnitId(3)));
    assert_eq!(world.objectives.len(), 1);

    // Verify invalid weapon or out-of-bounds spawn is rejected cleanly.
    let mut bad = spec.clone();
    bad.units[0].pos = WorldPos::new(-5.0, 20.0);
    assert!(bad.build_world(&catalog).is_err());

    let mut bad_wpn = spec;
    bad_wpn.units[0].weapon_id = "wpn_nonexistent".into();
    assert!(bad_wpn.build_world(&catalog).is_err());
}

#[test]
fn test_v2_p1_tripartite_derived_destroy_and_rebuild_identical() {
    let catalog = ItemCatalog::embedded().unwrap();
    let mut world = build_test_scenario().build_world(&catalog).unwrap();
    // Place a steep mountain ridge midpoint between Red-HQ (40, 120) and Red-Sq1 (120, 120) -> (80, 120) = cell (15, 10)
    world
        .terrain
        .mutate_cell(
            CellCoord::new(15, 10),
            Some(90.0),
            Some(TerrainKind::Mountain),
            None,
            None,
        )
        .unwrap();

    let mut derived = DerivedState::new(world.terrain.rows, world.terrain.cols);
    assert!(derived.rebuild_if_needed(&world));
    assert!(!derived.needs_rebuild(&world));
    assert!(!derived.rebuild_if_needed(&world));

    let t_diff_sq1 = derived.unit_radio_t_diff[&2];
    assert!(
        t_diff_sq1 < 0.85,
        "knife-edge ridge must attenuate Deygout transmission factor, got {t_diff_sq1}"
    );

    let within_100m = derived.query_units_in_radius(&world, WorldPos::new(40.0, 120.0), 100.0);
    assert_eq!(within_100m, vec![UnitId(1), UnitId(2)]);
    let within_15m = derived.query_units_in_radius(&world, WorldPos::new(40.0, 120.0), 15.0);
    assert_eq!(within_15m, vec![UnitId(1)]);

    let snapshot = derived.clone();
    derived.invalidate();
    assert!(derived.needs_rebuild(&world));
    assert!(derived.g_field_cells.is_empty());

    assert!(derived.rebuild_if_needed(&world));
    assert_eq!(derived, snapshot, "DerivedState must rebuild bit-for-bit identically");
}

#[test]
fn test_v2_p1_tick_scratch_arbitration_and_force_clear() {
    let mut scratch = TickScratch::default();
    assert!(scratch.is_completely_empty());

    // Autonomy proposes Hold, then DirectOrder proposes Move -> DirectOrder wins even at lower numeric priority.
    scratch.submit_intent(IntentCandidate {
        unit_id: UnitId(1),
        source: IntentSource::Autonomy,
        priority: 999,
        order: UnitOrder::Hold,
    });
    let direct_move = UnitOrder::Move {
        waypoints: vec![WorldPos::new(200.0, 120.0)],
        stance: MoveStance::Fast,
    };
    scratch.submit_intent(IntentCandidate {
        unit_id: UnitId(1),
        source: IntentSource::DirectOrder,
        priority: 1,
        order: direct_move.clone(),
    });
    // Subsequent higher-priority Autonomy MUST NOT override DirectOrder.
    scratch.submit_intent(IntentCandidate {
        unit_id: UnitId(1),
        source: IntentSource::Autonomy,
        priority: 5000,
        order: UnitOrder::PopSmoke {
            target: WorldPos::new(50.0, 50.0),
        },
    });
    assert_eq!(scratch.intents[&1].source, IntentSource::DirectOrder);
    assert_eq!(scratch.intents[&1].order, direct_move);

    let mut tmp = WorldState::new_blank(16, 16, 0.0, SimRuleset::arcade_sandbox());
    scratch.motion_proposals.push(MotionProposal {
        unit_id: UnitId(1),
        from: WorldPos::new(16.0, 16.0),
        to: WorldPos::new(24.0, 16.0),
        speed_mps: 2.0,
    });
    scratch
        .blast_workspace
        .splat_explosion(&tmp.terrain.geom, CellCoord::new(5, 5), 12.0, 100.0, 50, 7)
        .unwrap();
    scratch.events.push(EventRecord {
        event_id: 7,
        tick: 1,
        source: UnitId(1),
        kind: "blast".into(),
        target_cell: Some(CellCoord::new(5, 5)),
    });
    scratch.terminal_statuses.insert(
        7,
        EventTerminalStatus::Resolved {
            code: "OK".into(),
            detail: "done".into(),
        },
    );
    scratch.prior_positions.insert(1, WorldPos::new(16.0, 16.0));
    tmp.advance_clock_one_tick();

    assert!(!scratch.is_completely_empty());
    scratch.clear_and_assert_empty();
    assert!(scratch.is_completely_empty());
}

#[test]
fn test_v2_p1_save_load_clock_and_sha256_zero_drift() {
    let catalog = ItemCatalog::embedded().unwrap();
    let mut world = build_test_scenario().build_world(&catalog).unwrap();
    world
        .terrain
        .mutate_cell(CellCoord::new(20, 20), Some(-2.5), Some(TerrainKind::Urban), Some(0.3), None)
        .unwrap();
    world.cell_h_blast[0] = 42.5;
    world.cell_s_supp[0] = 1.75;

    for _ in 0..5 {
        world.advance_clock_one_tick();
    }
    assert_eq!(world.tick, 5);
    assert_eq!(world.clock_seconds, world.epoch_seconds + 5 * TICK_SECONDS);

    let saved = world.save_json().expect("save_json must succeed");
    let mut reloaded = WorldState::load_json(&saved).expect("load_json must succeed");
    assert_eq!(world, reloaded);
    assert_eq!(world.authoritative_sha256(), reloaded.authoritative_sha256());

    world.advance_clock_one_tick();
    reloaded.advance_clock_one_tick();
    assert_eq!(world.authoritative_sha256(), reloaded.authoritative_sha256());
}
