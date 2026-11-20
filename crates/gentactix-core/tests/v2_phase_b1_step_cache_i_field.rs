use gentactix_core::commands::TacticalCommand;
use gentactix_core::engine::PureEngine;
use gentactix_core::math_ops::{
    fuse_intel_log_survival, transition_vis_tier_hysteresis, CellCoord, WorldPos, ALPHA_INTEL,
};
use gentactix_core::ruleset::{SimRuleset, UnitSpawnSpec};
use gentactix_core::step_cache::{WaveZoneKind, WaveZoneSpec, WorldInstanceKind, MAX_STEP_CACHE_HORIZON_TICKS};
use gentactix_core::terrain::TerrainKind;
use gentactix_core::types::{
    FireDiscipline, MobilityKind, MoveStance, Team, UnitId, UnitOrder, UnitRole,
};
use gentactix_core::visibility::VisTier;

fn spawn_spec(
    callsign: &str,
    team: Team,
    platoon_id: Option<u32>,
    role: UnitRole,
    pos: WorldPos,
    vision_m: f32,
    radio_m: f32,
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
        weapon_id: "wpn_luger_p08".into(),
        ammo_id: "ammo_9x19_m08_ball".into(),
        fuze_id: None,
        mount_ids: vec![],
        ammo_rounds: 120,
        smoke_grenades: 2,
        base_speed_mps: 2.0,
        vision_m,
        base_radio_range_m: radio_m,
    }
}

#[test]
fn test_b1_step_cache_60_day_horizon_and_save_load_persistence() {
    let mut rs = SimRuleset::hardcore_platoon();
    rs.enable_g_field_autonomy = false;
    rs.enable_time_machine_deferral = false;

    let mut eng = PureEngine::new_blank(64, 64, 20.0, rs).unwrap();
    let eid = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(spawn_spec(
                    "Eng-Sap",
                    Team::Red,
                    None,
                    UnitRole::EngineerSquad,
                    WorldPos::new(100.0, 100.0),
                    200.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // 1. Exact 60-day horizon (324,000 ticks) accepted; 324,001 rejected.
    let ok_60d = eng.submit_command(
        TacticalCommand::ScheduleTimedCharge {
            unit_id: UnitId(eid),
            pos: WorldPos::new(140.0, 100.0),
            delay_ticks: MAX_STEP_CACHE_HORIZON_TICKS,
            blast_radius_m: 24.0,
            blast_kpa: 120.0,
            crater_depth_m: 1.5,
        },
        1,
    );
    assert!(ok_60d.accepted);
    assert_eq!(ok_60d.deferred_until_tick, Some(MAX_STEP_CACHE_HORIZON_TICKS));

    let bad_60d = eng.submit_command(
        TacticalCommand::ScheduleTimedCharge {
            unit_id: UnitId(eid),
            pos: WorldPos::new(140.0, 100.0),
            delay_ticks: MAX_STEP_CACHE_HORIZON_TICKS + 1,
            blast_radius_m: 24.0,
            blast_kpa: 120.0,
            crater_depth_m: 1.5,
        },
        1,
    );
    assert!(!bad_60d.accepted && bad_60d.code == "INVALID_DELAY_TICKS");

    // 2. Schedule a charge at tick #4, save/reload JSON, step #1..#3 (unexploded), step #4 -> detonates bit-identically!
    let chg = eng.submit_command(
        TacticalCommand::ScheduleTimedCharge {
            unit_id: UnitId(eid),
            pos: WorldPos::new(200.0, 100.0),
            delay_ticks: 4,
            blast_radius_m: 24.0,
            blast_kpa: 150.0,
            crater_depth_m: 2.2,
        },
        1,
    );
    assert!(chg.accepted);
    let charge_inst_id = chg.created_id.unwrap() as u64;
    assert!(eng.world.world_instances[&charge_inst_id].active);

    let saved = eng.save_json().unwrap();
    let mut reloaded = PureEngine::new_blank(16, 16, 0.0, SimRuleset::arcade_sandbox()).unwrap();
    reloaded.load_json(&saved).unwrap();
    assert_eq!(
        eng.world.authoritative_sha256(),
        reloaded.world.authoritative_sha256()
    );

    let ccell = eng
        .world
        .terrain
        .geom
        .world_to_cell(WorldPos::new(200.0, 100.0))
        .unwrap();
    let cidx = eng.world.terrain.idx(ccell).unwrap();

    for _ in 0..3 {
        let s1 = eng.step_16s(Team::Red);
        let s2 = reloaded.step_16s(Team::Red);
        assert_eq!(s1.report.authoritative_sha256, s2.report.authoritative_sha256);
        assert!(eng.world.world_instances[&charge_inst_id].active);
        assert_eq!(eng.world.terrain.crater_depth_m[cidx], 0.0);
    }

    let s4_a = eng.step_16s(Team::Red);
    let s4_b = reloaded.step_16s(Team::Red);
    assert_eq!(s4_a.report.authoritative_sha256, s4_b.report.authoritative_sha256);
    assert!(s4_a
        .activated_commands
        .iter()
        .any(|c| c.code == "TIMED_DETONATION"));
    assert!(!eng.world.world_instances[&charge_inst_id].active);
    assert!(eng.world.terrain.crater_depth_m[cidx] >= 2.0);
    assert!(eng.world.cell_h_blast[cidx] > 10.0);
}

#[test]
fn test_b1_cancel_step_event_via_derived_index_and_wave_activation() {
    let mut rs = SimRuleset::hardcore_platoon();
    rs.enable_time_machine_deferral = false;

    let mut eng = PureEngine::new_blank(64, 64, 20.0, rs).unwrap();
    let eid = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(spawn_spec(
                    "Eng-Cancel",
                    Team::Red,
                    None,
                    UnitRole::EngineerSquad,
                    WorldPos::new(100.0, 100.0),
                    200.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    let chg = eng.submit_command(
        TacticalCommand::ScheduleTimedCharge {
            unit_id: UnitId(eid),
            pos: WorldPos::new(180.0, 100.0),
            delay_ticks: 5,
            blast_radius_m: 20.0,
            blast_kpa: 100.0,
            crater_depth_m: 1.5,
        },
        1,
    );
    let ticket_id = chg.ticket_id.unwrap();
    let inst_id = chg.created_id.unwrap() as u64;
    assert_eq!(eng.derived.step_event_index.get(&ticket_id), Some(&5));

    let cancel = eng.submit_command(TacticalCommand::CancelStepEvent { event_id: ticket_id }, 1);
    assert!(cancel.accepted && cancel.code == "STEP_EVENT_CANCELLED");
    assert!(!eng.world.world_instances[&inst_id].active);
    assert!(!eng.derived.step_event_index.contains_key(&ticket_id));

    // Commander wave enters G-field on its scheduled tick.
    let wave = eng.submit_command(
        TacticalCommand::IssueCommanderWave {
            team: Team::Red,
            hq_unit_id: None,
            delay_ticks: 2,
            zones: vec![WaveZoneSpec {
                kind: WaveZoneKind::Assault,
                center: WorldPos::new(260.0, 100.0),
                radius_m: 24.0,
                waypoints: vec![WorldPos::new(260.0, 100.0)],
                g0_weight: 2.0,
            }],
        },
        1,
    );
    assert!(wave.accepted && wave.deferred_until_tick == Some(2));
    eng.step_16s(Team::Red);
    assert!(eng.world.active_wave_directives.is_empty());
    let out2 = eng.step_16s(Team::Red);
    assert!(out2
        .activated_commands
        .iter()
        .any(|c| c.code == "COMMANDER_WAVE_ACTIVATED"));
    assert_eq!(eng.world.active_wave_directives.len(), 1);
}

#[test]
fn test_b1_remote_command_radio_delay_and_blackout_runner_fallback() {
    let mut rs = SimRuleset::hardcore_platoon();
    rs.enable_g_field_autonomy = false;
    rs.enable_time_machine_deferral = false;

    let mut eng = PureEngine::new_blank(64, 64, 20.0, rs).unwrap();
    let pid = eng
        .submit_command(
            TacticalCommand::SpawnPlatoon {
                name: "Red-1".into(),
                team: Team::Red,
            },
            1,
        )
        .created_id
        .unwrap();
    let _hq = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(spawn_spec(
                    "Red-HQ",
                    Team::Red,
                    Some(pid),
                    UnitRole::PlatoonHq,
                    WorldPos::new(40.0, 120.0),
                    400.0,
                    100.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    let far_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(spawn_spec(
                    "Red-Far",
                    Team::Red,
                    Some(pid),
                    UnitRole::InfantrySquad,
                    WorldPos::new(300.0, 120.0),
                    400.0,
                    100.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Inject steep knife-edge mountain at midpoint (170, 120) -> radio eta collapses -> runner fallback!
    eng.world
        .terrain
        .mutate_cell(
            CellCoord::new(15, 21),
            Some(250.0),
            Some(TerrainKind::Mountain),
            None,
            None,
        )
        .unwrap();
    eng.step_16s(Team::Red);

    let far_eta = eng.world.units[&far_id].effective_radio_range_m
        / eng.world.units[&far_id].base_radio_range_m.max(1.0);
    assert!(far_eta <= 0.05, "ridge must collapse radio eta, got {far_eta}");

    let mv = eng.submit_command(
        TacticalCommand::MoveUnit {
            unit_id: UnitId(far_id),
            waypoints: vec![WorldPos::new(340.0, 120.0)],
            stance: MoveStance::Normal,
        },
        1,
    );
    assert!(mv.accepted && mv.code == "DEFERRED_REMOTE_COMMAND");
    assert_eq!(mv.via_runner, Some(true));
    let delay = mv.remote_delay_ticks.unwrap();
    let target_tick = mv.deferred_until_tick.unwrap();
    assert_eq!(delay, 5); // ceil(260m / 64m/tick) = 5 ticks

    while eng.world.tick < target_tick - 1 {
        eng.step_16s(Team::Red);
        assert_eq!(eng.world.units[&far_id].order, UnitOrder::Hold);
    }
    let act = eng.step_16s(Team::Red);
    assert!(act
        .activated_commands
        .iter()
        .any(|c| c.accepted && c.code == "ORDER_ACCEPTED"));
    assert!(eng.world.units[&far_id].pos.x > 300.0);
}

#[test]
fn test_b1_instance_exists_and_visibility_strictly_follows_i_intel() {
    let mut rs = SimRuleset::hardcore_platoon();
    rs.enforce_fog_of_war = true;
    rs.enable_g_field_autonomy = false;
    rs.enable_time_machine_deferral = false;

    let mut eng = PureEngine::new_blank(80, 80, 10.0, rs).unwrap();
    let red_scout = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(spawn_spec(
                    "Red-Scout",
                    Team::Red,
                    None,
                    UnitRole::ReconTeam,
                    WorldPos::new(100.0, 200.0),
                    200.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    let blue_eng = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(spawn_spec(
                    "Blue-Eng",
                    Team::Blue,
                    None,
                    UnitRole::EngineerSquad,
                    WorldPos::new(380.0, 200.0),
                    150.0,
                    500.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    for uid in [red_scout, blue_eng] {
        eng.submit_command(
            TacticalCommand::SetFireDiscipline {
                unit_id: UnitId(uid),
                discipline: FireDiscipline::HoldFire,
            },
            1,
        );
    }

    // Blue plants timed charge outside Red's 200m vision (distance = 260m).
    let charge_pos = WorldPos::new(360.0, 200.0);
    let chg = eng.submit_command(
        TacticalCommand::ScheduleTimedCharge {
            unit_id: UnitId(blue_eng),
            pos: charge_pos,
            delay_ticks: 10,
            blast_radius_m: 20.0,
            blast_kpa: 120.0,
            crater_depth_m: 1.5,
        },
        1,
    );
    let chg_id = chg.created_id.unwrap() as u64;
    assert_eq!(
        eng.world.world_instances[&chg_id].kind,
        WorldInstanceKind::TimedCharge
    );

    // Step 1 tick: Blue sees its own created charge immediately; Red's I_intel is 0 -> Hidden!
    eng.step_16s(Team::Red);
    let obs_blue = eng.observe(Team::Blue);
    assert!(obs_blue
        .visible_instances
        .iter()
        .any(|v| v.instance_id == chg_id));
    let obs_red_before = eng.observe(Team::Red);
    assert!(obs_red_before
        .visible_instances
        .iter()
        .all(|v| v.instance_id != chg_id));

    // Red scout advances to within 60m (< 200 * 0.55) of Blue's already-created charge -> I_intel rises -> Red sees it!
    eng.submit_command(
        TacticalCommand::MoveUnit {
            unit_id: UnitId(red_scout),
            waypoints: vec![WorldPos::new(300.0, 200.0)],
            stance: MoveStance::Fast,
        },
        1,
    );
    eng.step_n_ticks(5, Team::Red);
    let obs_red_after = eng.observe(Team::Red);
    let seen_chg = obs_red_after
        .visible_instances
        .iter()
        .find(|v| v.instance_id == chg_id)
        .expect("already-created enemy charge must appear purely from Red I_intel once observed");
    assert_eq!(seen_chg.vis_tier, VisTier::Full);

    // Verify Report Three 0.05 hysteresis deadband directly.
    let mut tier = VisTier::Hidden;
    let intel = fuse_intel_log_survival(0.0, 1.45); // ~0.765 >= 0.75
    tier = transition_vis_tier_hysteresis(tier, intel);
    assert_eq!(tier, VisTier::Full);
    // Single partial decay step that lands inside [0.70, 0.75) remains Full (deadband).
    let intel_deadband = 0.72;
    assert!(intel_deadband > 0.0 * ALPHA_INTEL);
    tier = transition_vis_tier_hysteresis(tier, intel_deadband);
    assert_eq!(tier, VisTier::Full);
    // Dropping below 0.70 degrades cleanly to Spotted.
    tier = transition_vis_tier_hysteresis(tier, 0.69);
    assert_eq!(tier, VisTier::Spotted);
}
