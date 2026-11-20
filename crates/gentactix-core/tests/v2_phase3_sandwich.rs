use gentactix_core::commands::TacticalCommand;
use gentactix_core::engine::PureEngine;
use gentactix_core::math_ops::{CellCoord, WorldPos};
use gentactix_core::ruleset::{ScenarioSpec, SimRuleset, TerrainSourceSpec, UnitSpawnSpec};
use gentactix_core::terrain::TerrainKind;
use gentactix_core::types::{
    EngineerWorkKind, FireDiscipline, MobilityKind, MoveStance, Team, UnitId, UnitOrder,
    UnitRole,
};
use gentactix_core::visibility::VisTier;

fn make_unit_spec(
    callsign: &str,
    team: Team,
    platoon_id: Option<u32>,
    role: UnitRole,
    pos: WorldPos,
    weapon_id: &str,
    ammo_id: &str,
    vision_m: f32,
    radio_range_m: f32,
) -> UnitSpawnSpec {
    UnitSpawnSpec {
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
        mount_ids: vec![],
        ammo_rounds: 600,
        smoke_grenades: 2,
        base_speed_mps: 1.6,
        vision_m,
        base_radio_range_m: radio_range_m,
    }
}

#[test]
fn test_v2_p3_unified_commands_ruleset_gates_and_detonator() {
    let mut ruleset = SimRuleset::hardcore_platoon();
    ruleset.allow_runtime_map_edit = false;
    ruleset.enforce_command_radius = true;
    ruleset.enable_time_machine_deferral = false;

    let mut engine = PureEngine::new_blank(64, 64, 20.0, ruleset.clone()).unwrap();

    // EditTerrain gated by allow_runtime_map_edit == false
    let edit_denied = engine.submit_command(
        TacticalCommand::EditTerrain {
            cell: CellCoord::new(10, 10),
            elev_delta: Some(5.0),
            terrain_kind: Some(TerrainKind::Forest),
            cover_add: None,
            smoke_add: None,
        },
        1,
    );
    assert!(!edit_denied.accepted && edit_denied.code == "MAP_EDIT_DISABLED");

    // Flip SetRuleset to enable runtime map edit, then edit succeeds.
    ruleset.allow_runtime_map_edit = true;
    assert!(engine
        .submit_command(TacticalCommand::SetRuleset { ruleset: ruleset.clone() }, 1)
        .accepted);
    assert!(engine
        .submit_command(
            TacticalCommand::EditTerrain {
                cell: CellCoord::new(10, 10),
                elev_delta: Some(5.0),
                terrain_kind: Some(TerrainKind::Forest),
                cover_add: None,
                smoke_add: None,
            },
            1
        )
        .accepted);

    // Spawn platoon + HQ + near/far squads + engineer
    let pl_out = engine.submit_command(
        TacticalCommand::SpawnPlatoon {
            name: "Red-1".into(),
            team: Team::Red,
        },
        1,
    );
    let pid = pl_out.created_id.unwrap();
    let hq = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Red-HQ",
                    Team::Red,
                    Some(pid),
                    UnitRole::PlatoonHq,
                    WorldPos::new(40.0, 40.0),
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    400.0,
                    100.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    let far_sq = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Far-Sq",
                    Team::Red,
                    Some(pid),
                    UnitRole::InfantrySquad,
                    WorldPos::new(300.0, 40.0),
                    "wpn_mg42",
                    "ammo_792x57_ss",
                    400.0,
                    100.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    assert_ne!(hq, far_sq);

    // Outside HQ effective_radio_range_m (260m > 100m): command accepted with computed remote delay into StepCache!
    let far_move = TacticalCommand::MoveUnit {
        unit_id: UnitId(far_sq),
        waypoints: vec![WorldPos::new(320.0, 40.0)],
        stance: MoveStance::Normal,
    };
    let rad_delayed = engine.submit_command(far_move.clone(), 1);
    assert!(rad_delayed.accepted && rad_delayed.code == "DEFERRED_REMOTE_COMMAND");
    let delay_ticks = rad_delayed.remote_delay_ticks.expect("remote delay must be positive");
    assert!(delay_ticks >= 1 && rad_delayed.deferred_until_tick == Some(delay_ticks));
    assert_eq!(engine.world.units[&far_sq].order, UnitOrder::Hold);
    assert!(engine
        .world
        .step_cache
        .get(&delay_ticks)
        .is_some_and(|b| !b.is_empty()));

    // Disable enforce_command_radius -> distant order succeeds immediately!
    ruleset.enforce_command_radius = false;
    engine.submit_command(TacticalCommand::SetRuleset { ruleset }, 1);
    let immediate_far = engine.submit_command(far_move, 1);
    assert!(immediate_far.accepted && immediate_far.code == "ORDER_ACCEPTED");
    assert!(immediate_far.remote_delay_ticks.is_none());

    // Engineer arm & trigger detonator: blocked inside 40m blast corridor, succeeds >= 40m away.
    let eng_id = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Eng-1",
                    Team::Red,
                    None,
                    UnitRole::EngineerSquad,
                    WorldPos::new(100.0, 100.0),
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    300.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    let charge = WorldPos::new(108.0, 100.0); // only 8m from Eng-1!
    let safe_switch = WorldPos::new(20.0, 100.0); // 88m from charge
    let arm_out = engine.submit_command(
        TacticalCommand::ArmDetonator {
            unit_id: UnitId(eng_id),
            switch_pos: safe_switch,
            charge_positions: vec![charge],
            wire_polyline: vec![charge, safe_switch],
        },
        1,
    );
    let switch_id = arm_out.created_id.unwrap() as u64;

    let too_close = engine.submit_command(
        TacticalCommand::TriggerDetonator {
            unit_id: UnitId(eng_id),
            switch_id,
        },
        1,
    );
    assert!(!too_close.accepted && too_close.code == "DETONATION_BLOCKED");

    // Move engineer to >= 40m away (x=20, y=100) and trigger -> detonates!
    engine.world.units.get_mut(&eng_id).unwrap().pos = safe_switch;
    let det_ok = engine.submit_command(
        TacticalCommand::TriggerDetonator {
            unit_id: UnitId(eng_id),
            switch_id,
        },
        1,
    );
    assert!(det_ok.accepted && det_ok.code == "DETONATED");
    let ccell = engine.world.terrain.geom.world_to_cell(charge).unwrap();
    let cidx = engine.world.terrain.idx(ccell).unwrap();
    assert!(engine.world.terrain.crater_depth_m[cidx] >= 1.9);
    assert!(engine.world.cell_h_blast[cidx] >= 100.0);
}

#[test]
fn test_v2_p3_time_machine_ingress_egress_zero_physics_pollution() {
    let mut rules_tm = SimRuleset::hardcore_platoon();
    rules_tm.enable_time_machine_deferral = true;
    rules_tm.enable_g_field_autonomy = false;

    let spec = ScenarioSpec {
        name: "TM Zero Pollution".into(),
        epoch_seconds: 23400,
        night: false,
        ruleset: rules_tm.clone(),
        terrain: TerrainSourceSpec::Flat {
            rows: 64,
            cols: 64,
            base_elevation_m: 10.0,
        },
        platoons: vec![],
        units: vec![
            make_unit_spec(
                "Red-Assault",
                Team::Red,
                None,
                UnitRole::InfantrySquad,
                WorldPos::new(100.0, 120.0),
                "wpn_mg42",
                "ammo_792x57_ss",
                600.0,
                800.0,
            ),
            make_unit_spec(
                "Blue-Def",
                Team::Blue,
                None,
                UnitRole::InfantrySquad,
                WorldPos::new(280.0, 120.0),
                "wpn_mg42",
                "ammo_792x57_ss",
                600.0,
                800.0,
            ),
        ],
        objectives: vec![],
    };

    let mut eng_a = PureEngine::from_scenario(&spec).unwrap();
    let mut spec_b = spec;
    spec_b.ruleset.enable_time_machine_deferral = false;
    let mut eng_b = PureEngine::from_scenario(&spec_b).unwrap();

    let move_cmd = TacticalCommand::MoveUnit {
        unit_id: UnitId(1),
        waypoints: vec![WorldPos::new(180.0, 120.0)],
        stance: MoveStance::Normal,
    };

    // Engine A defers MoveUnit on tick #0: world order MUST remain Hold during #0.
    let ticket_out = eng_a.submit_command(move_cmd.clone(), 5);
    assert!(ticket_out.accepted);
    let act_tick = ticket_out.deferred_until_tick.expect("must defer to future tick");
    assert!(act_tick >= 1);
    assert_eq!(eng_a.world.units[&1].order, UnitOrder::Hold);

    // Advance both identically until act_tick - 1, then inject immediate MoveUnit into Engine B.
    let pre_ticks = (act_tick - 1) as u32;
    if pre_ticks > 0 {
        eng_a.step_n_ticks(pre_ticks, Team::Red);
        eng_b.step_n_ticks(pre_ticks, Team::Red);
    }
    assert!(eng_b.submit_command(move_cmd, 5).accepted);

    // Step through activation + 5 more ticks under full combat exchange.
    let mut found_playback_event = false;
    for _ in 0..6 {
        let out_a = eng_a.step_16s(Team::Red);
        let _out_b = eng_b.step_16s(Team::Red);
        for ev in &out_a.playback.playback_events {
            assert!((1..=16000).contains(&ev.exact_ms));
            if let Some(raw_log) = out_a.report.tick_logs.iter().find(|l| {
                Some(l.actor_id.0.to_string()) == ev.source_piece_id
                    && l.target_id.map(|t| t.0.to_string()) == ev.target_piece_id
            }) {
                assert_eq!(ev.exact_supp_add, Some(raw_log.suppression_add));
                found_playback_event = true;
            }
        }
    }
    assert!(found_playback_event, "must emit unaltered playback combat events");

    // Equalize only ruleset toggle & ticket sequence metadata after simulation so full WorldState & SHA-256 prove zero physics drift.
    let mut norm_b = eng_b.world.clone();
    norm_b.ruleset = rules_tm;
    norm_b.next_event_id = eng_a.world.next_event_id;
    assert_eq!(eng_a.world.units[&1].pos, norm_b.units[&1].pos);
    assert_eq!(eng_a.world.units[&2].hp, norm_b.units[&2].hp);
    assert_eq!(eng_a.world.units[&1].ammo_rounds, norm_b.units[&1].ammo_rounds);
    assert_eq!(eng_a.world, norm_b);
    assert_eq!(eng_a.world.authoritative_sha256(), norm_b.authoritative_sha256());
}

#[test]
fn test_v2_p3_readonly_preview_sandbox_zero_side_effects() {
    let mut engine = PureEngine::new_blank(32, 32, 10.0, SimRuleset::arcade_sandbox()).unwrap();
    // River with bridge at row 20, plus ridge at (12, 14) for fire arc test.
    for r in 0..32 {
        let k = if r == 20 { TerrainKind::Bridge } else { TerrainKind::Water };
        engine.world.terrain.mutate_cell(CellCoord::new(r, 16), None, Some(k), None, None).unwrap();
    }
    engine
        .world
        .terrain
        .mutate_cell(CellCoord::new(12, 14), Some(60.0), Some(TerrainKind::Mountain), None, None)
        .unwrap();

    let u1 = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Scout-1",
                    Team::Red,
                    None,
                    UnitRole::EngineerSquad,
                    WorldPos::new(68.0, 100.0), // cell (12, 8)
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    300.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    let hash_before = engine.world.authoritative_sha256();
    let terr_rev_before = engine.world.terrain.revision;
    let unit_rev_before = engine.world.unit_revision;

    let p_path = engine
        .preview_move(UnitId(u1), WorldPos::new(196.0, 100.0), MoveStance::Normal)
        .unwrap();
    assert!(p_path.reachable && p_path.total_distance_m > 140.0);
    assert!(p_path.waypoints.iter().any(|w| w.y >= 160.0));

    let arc = engine.preview_fire_arc(UnitId(u1), 0.0, 30.0, 9, 250.0).unwrap();
    assert_eq!(arc.samples.len(), 9);
    assert!(arc.samples.iter().any(|s| s.blocked_by_terrain && s.visible_dist_m < 80.0));

    let los = engine.preview_los(WorldPos::new(68.0, 100.0), WorldPos::new(140.0, 100.0), 300.0);
    assert!(!los.visible && los.blocked_by_terrain);

    let w_trench = engine
        .preview_engineer_work(
            UnitId(u1),
            EngineerWorkKind::ZigzagTrench,
            &[WorldPos::new(68.0, 120.0), WorldPos::new(108.0, 120.0)],
        )
        .unwrap();
    let w_mine = engine
        .preview_engineer_work(
            UnitId(u1),
            EngineerWorkKind::Minefield,
            &[
                WorldPos::new(40.0, 40.0),
                WorldPos::new(80.0, 40.0),
                WorldPos::new(80.0, 80.0),
                WorldPos::new(40.0, 80.0),
            ],
        )
        .unwrap();
    assert!(!w_trench.points.is_empty() && w_trench.estimated_work_person_sec > 0.0);
    assert!(!w_mine.points.is_empty());

    assert_eq!(engine.world.terrain.revision, terr_rev_before);
    assert_eq!(engine.world.unit_revision, unit_rev_before);
    assert_eq!(engine.world.authoritative_sha256(), hash_before);
}

#[test]
fn test_v2_p3_fog_of_war_no_leak_and_prerender_skirt() {
    let mut ruleset = SimRuleset::hardcore_platoon();
    ruleset.enforce_fog_of_war = true;
    ruleset.enable_g_field_autonomy = false;
    ruleset.enable_time_machine_deferral = false;

    let mut engine = PureEngine::new_blank(80, 80, 10.0, ruleset.clone()).unwrap();
    // Red observer at x=100, vision_m=200m (so visible <= 200m, skirt <= 264m, far > 264m).
    let red_id = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Red-Obs",
                    Team::Red,
                    None,
                    UnitRole::ReconTeam,
                    WorldPos::new(100.0, 200.0),
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    200.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    engine.submit_command(
        TacticalCommand::SetFireDiscipline {
            unit_id: UnitId(red_id),
            discipline: FireDiscipline::HoldFire,
        },
        1,
    );

    // 1. Near enemy at 80m -> inside 200m*0.55=110m -> VisTier::Full
    let near_id = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Blue-Near",
                    Team::Blue,
                    None,
                    UnitRole::InfantrySquad,
                    WorldPos::new(180.0, 200.0),
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    150.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    // 2. Skirt enemy at 230m -> outside 200m vision, inside 264m skirt -> hidden in observe, visible=0 skirt in playback
    let skirt_id = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Blue-Skirt",
                    Team::Blue,
                    None,
                    UnitRole::InfantrySquad,
                    WorldPos::new(330.0, 200.0),
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    150.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    // 3. Far enemy at 360m -> omitted everywhere!
    let far_id = engine
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_unit_spec(
                    "Blue-Far",
                    Team::Blue,
                    None,
                    UnitRole::InfantrySquad,
                    WorldPos::new(460.0, 200.0),
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    150.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    for bid in [near_id, skirt_id, far_id] {
        engine.submit_command(
            TacticalCommand::SetFireDiscipline {
                unit_id: UnitId(bid),
                discipline: FireDiscipline::HoldFire,
            },
            1,
        );
    }

    let step = engine.step_16s(Team::Red);
    let obs = engine.observe(Team::Red);

    assert!(obs.fog_enforced);
    let obs_near = obs.units.iter().find(|u| u.id == UnitId(near_id)).unwrap();
    assert_eq!(obs_near.vis_tier, VisTier::Full);
    assert!(obs_near.ammo_rounds.is_none() && obs_near.order.is_none(), "hostile order & exact ammo stay redacted");
    assert!(obs.units.iter().all(|u| u.id != UnitId(skirt_id) && u.id != UnitId(far_id)));

    let tr_near = step
        .playback
        .particle_tracks
        .iter()
        .find(|p| p.piece_id == near_id.to_string())
        .unwrap();
    assert_eq!(tr_near.initial_visibility, 1);
    assert!(!tr_near.is_prerender_skirt);

    let tr_skirt = step
        .playback
        .particle_tracks
        .iter()
        .find(|p| p.piece_id == skirt_id.to_string())
        .expect("enemy within 64m outside vision must enter prerender skirt");
    assert_eq!(tr_skirt.initial_visibility, 0);
    assert!(tr_skirt.is_prerender_skirt);

    assert!(
        step.playback
            .particle_tracks
            .iter()
            .all(|p| p.piece_id != far_id.to_string()),
        "distant enemy beyond skirt must not leak into playback"
    );

    // Disable enforce_fog_of_war -> all 3 hostile units become Full!
    ruleset.enforce_fog_of_war = false;
    engine.submit_command(TacticalCommand::SetRuleset { ruleset }, 1);
    let obs_open = engine.observe(Team::Red);
    assert_eq!(obs_open.units.len(), 4);
    assert!(obs_open.units.iter().all(|u| u.vis_tier == VisTier::Full));
}
