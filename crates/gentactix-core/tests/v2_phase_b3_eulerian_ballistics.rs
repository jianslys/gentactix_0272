use gentactix_core::clock::ballistics_field::{
    build_direct_cone_field, resolve_direct_cone_batch_into, DirectConeField,
};
use gentactix_core::commands::TacticalCommand;
use gentactix_core::damage_model::ArmorProfileRha;
use gentactix_core::engine::PureEngine;
use gentactix_core::math_ops::{CellCoord, SchmidtPostureState, WorldPos};
use gentactix_core::ruleset::{SimRuleset, UnitSpawnSpec};
use gentactix_core::terrain::TerrainKind;
use gentactix_core::types::{
    FireDiscipline, MobilityKind, Team, UnitId, UnitRole,
};

fn unit_spec(
    callsign: &str,
    team: Team,
    role: UnitRole,
    mobility: MobilityKind,
    pos: WorldPos,
    men: u16,
    hp: f32,
    armor_mm: f32,
    wpn: &str,
    ammo: &str,
    mounts: Vec<&str>,
) -> UnitSpawnSpec {
    UnitSpawnSpec {
        callsign: callsign.into(),
        team,
        platoon_id: None,
        role,
        mobility,
        pos,
        facing_deg: 180.0,
        men,
        hp,
        armor_mm,
        weapon_id: wpn.into(),
        ammo_id: ammo.into(),
        fuze_id: None,
        mount_ids: mounts.into_iter().map(str::to_string).collect(),
        ammo_rounds: 2000,
        smoke_grenades: 0,
        base_speed_mps: 2.0,
        vision_m: 600.0,
        base_radio_range_m: 800.0,
    }
}

#[test]
fn test_b3_attack_pos_empty_ground_hits_corridor_units_and_suppresses() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enforce_suppression_posture = true;
    rs.enforce_ammo_consumption = true;

    let mut eng = PureEngine::new_blank(64, 64, 10.0, rs).unwrap();
    let red_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Red-MG",
                    Team::Red,
                    UnitRole::HeavyWeaponTeam,
                    MobilityKind::Foot,
                    WorldPos::new(40.0, 200.0),
                    5,
                    100.0,
                    0.0,
                    "wpn_mg42",
                    "ammo_792x57_ss",
                    vec!["mount_lafette_42"],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Target empty ground at x=220 while Blue stands halfway down the corridor at x=140.
    let blue_mid = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-Mid",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(140.0, 200.0),
                    9,
                    100.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    eng.submit_command(
        TacticalCommand::SetFireDiscipline {
            unit_id: UnitId(blue_mid),
            discipline: FireDiscipline::HoldFire,
        },
        1,
    );

    let atk = eng.submit_command(
        TacticalCommand::AttackPos {
            unit_id: UnitId(red_id),
            target: WorldPos::new(220.0, 200.0),
            fire_mode: FireDiscipline::FireAtWill,
        },
        1,
    );
    assert!(atk.accepted);

    let step = eng.step_16s(Team::Red);
    assert_eq!(step.report.fire_events, 1, "AttackPos on empty ground MUST fire cleanly");
    assert!(eng.world.units[&blue_mid].hp < 100.0, "unit standing in beaten corridor must take hits");
    assert!(eng.world.units[&blue_mid].suppression > 0.2);

    // Directional I_threat corridor must light up for Blue along x=100..220!
    let c_mid = eng
        .world
        .terrain
        .idx(eng.world.terrain.geom.world_to_cell(WorldPos::new(100.0, 200.0)).unwrap())
        .unwrap();
    assert!(
        eng.derived.team_i_field_threat[1][c_mid] > 0.05,
        "corridor I_threat must propagate along beaten zone"
    );
}

#[test]
fn test_b3_front_prone_unit_passes_overhead_bullets_to_rear_standing_and_tank() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enforce_suppression_posture = true;

    let mut eng = PureEngine::new_blank(64, 64, 10.0, rs).unwrap();
    let red_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Red-MG",
                    Team::Red,
                    UnitRole::HeavyWeaponTeam,
                    MobilityKind::Foot,
                    WorldPos::new(20.0, 200.0),
                    5,
                    100.0,
                    0.0,
                    "wpn_mg42",
                    "ammo_792x57_ss",
                    vec!["mount_lafette_42"],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Front Blue squad at r=40m (x=60) lying PRONE (h=0.35m).
    let front_prone = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-Prone",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(60.0, 200.0),
                    9,
                    100.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    eng.world.units.get_mut(&front_prone).unwrap().suppression = 2.0;
    eng.world.units.get_mut(&front_prone).unwrap().posture = SchmidtPostureState::Prone;

    // Rear standing Blue squad at r=120m (x=140, h=1.7m) directly behind prone squad.
    let rear_stand = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-Stand",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(140.0, 200.0),
                    9,
                    100.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    for uid in [front_prone, rear_stand] {
        eng.submit_command(
            TacticalCommand::SetFireDiscipline {
                unit_id: UnitId(uid),
                discipline: FireDiscipline::HoldFire,
            },
            1,
        );
    }

    eng.submit_command(
        TacticalCommand::AttackUnit {
            unit_id: UnitId(red_id),
            target_id: UnitId(rear_stand),
            chase: false,
            fire_mode: FireDiscipline::FireAtWill,
        },
        1,
    );

    eng.step_n_ticks(2, Team::Red);
    assert!(
        eng.world.units[&rear_stand].hp < 90.0,
        "rear taller standing unit must receive overhead bullets passing above prone front unit, got hp={}",
        eng.world.units[&rear_stand].hp
    );
}

#[test]
fn test_b3_front_tank_100pct_solid_shields_rear_infantry_even_when_penetrated() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enforce_ammo_consumption = true;

    let mut eng = PureEngine::new_blank(64, 64, 10.0, rs).unwrap();
    let red_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Red-AT-MG",
                    Team::Red,
                    UnitRole::HeavyWeaponTeam,
                    MobilityKind::Foot,
                    WorldPos::new(20.0, 200.0),
                    5,
                    100.0,
                    0.0,
                    "wpn_mg42",
                    "ammo_792x57_pmk", // steel-core AP (~11.5mm pen)
                    vec!["mount_lafette_42"],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Front Blue armored vehicle at x=60 (r=40m) with thin 5mm front plate so AP GUARANTEES penetration!
    let front_tank = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-LightTank",
                    Team::Blue,
                    UnitRole::ArmoredVehicle,
                    MobilityKind::Tracked,
                    WorldPos::new(60.0, 200.0),
                    4,
                    300.0,
                    5.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    eng.world.set_unit_armor_rha(
        UnitId(front_tank),
        ArmorProfileRha {
            front_mm: 4.0,
            side_mm: 4.0,
            rear_mm: 4.0,
            top_mm: 4.0,
        },
    );

    // Rear Blue infantry standing right behind the tank at x=120 (r=100m).
    let rear_inf = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-BehindTank",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(120.0, 200.0),
                    9,
                    100.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    for uid in [front_tank, rear_inf] {
        eng.submit_command(
            TacticalCommand::SetFireDiscipline {
                unit_id: UnitId(uid),
                discipline: FireDiscipline::HoldFire,
            },
            1,
        );
    }

    eng.submit_command(
        TacticalCommand::AttackPos {
            unit_id: UnitId(red_id),
            target: WorldPos::new(120.0, 200.0),
            fire_mode: FireDiscipline::FireAtWill,
        },
        1,
    );

    eng.step_n_ticks(2, Team::Red);
    let tank = &eng.world.units[&front_tank];
    let vd = tank.vehicle_damage.as_ref().unwrap();
    assert!(
        tank.hp < 300.0 && vd.subsystems.iter().any(|&h| h < 1.0),
        "tank hull & internal subsystems must absorb penetrating rounds as internal spall"
    );

    // Zero overpenetration: rear infantry shielded behind tank MUST stay 100% unharmed!
    let rear = &eng.world.units[&rear_inf];
    assert_eq!(rear.hp, 100.0, "tank MUST NEVER overpenetrate onto rear infantry");
    assert_eq!(rear.men, 9);
    assert_eq!(rear.suppression, 0.0);

    // Shadow cell behind tank stays safe in Blue's directional I_threat!
    let shadow_cell = eng
        .world
        .terrain
        .geom
        .world_to_cell(WorldPos::new(120.0, 200.0))
        .unwrap();
    let shadow_idx = eng.world.terrain.idx(shadow_cell).unwrap();
    assert_eq!(
        eng.derived.team_i_field_threat[1][shadow_idx], 0.0,
        "behind-tank shadow cell must stay threat-free"
    );
}

#[test]
fn test_b3_porous_infantry_gaps_leak_bullets_to_rear_squad_and_account_per_soldier() {
    let mut rs = SimRuleset::arcade_sandbox();
    let mut eng = PureEngine::new_blank(64, 64, 10.0, rs.clone()).unwrap();
    let _ = &mut rs;

    let red_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Red-MG",
                    Team::Red,
                    UnitRole::HeavyWeaponTeam,
                    MobilityKind::Foot,
                    WorldPos::new(20.0, 200.0),
                    5,
                    100.0,
                    0.0,
                    "wpn_mg42",
                    "ammo_792x57_ss",
                    vec!["mount_lafette_42"],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Front 2-man thin picket at x=60 (porous gaps!).
    let front_2man = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-Picket",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(60.0, 200.0),
                    2,
                    30.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Rear 9-man squad right behind at x=110.
    let rear_9man = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-RearSq",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(110.0, 200.0),
                    9,
                    100.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    for uid in [front_2man, rear_9man] {
        eng.submit_command(
            TacticalCommand::SetFireDiscipline {
                unit_id: UnitId(uid),
                discipline: FireDiscipline::HoldFire,
            },
            1,
        );
    }

    eng.submit_command(
        TacticalCommand::AttackPos {
            unit_id: UnitId(red_id),
            target: WorldPos::new(110.0, 200.0),
            fire_mode: FireDiscipline::FireAtWill,
        },
        1,
    );

    eng.step_16s(Team::Red);
    assert!(
        eng.world.units[&front_2man].hp < 30.0,
        "front picket absorbs hits"
    );
    assert!(
        eng.world.units[&rear_9man].hp < 100.0,
        "bullets passing through infantry gaps must strike rear squad"
    );
    // Confirm individual SoldierRecord arrays updated in rear squad.
    assert!(
        eng.world.units[&rear_9man]
            .soldiers
            .iter()
            .any(|s| !s.alive || s.hp < s.max_hp || s.bleed_rate_per_tick > 0.0),
        "per-soldier records must track individual damage"
    );
}

#[test]
fn test_b3_mountain_ridge_and_radial_blast_reverse_slope_block_100pct() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enforce_ammo_consumption = true;

    let mut eng = PureEngine::new_blank(64, 64, 10.0, rs).unwrap();
    // Place tall mountain ridge at col=8 (x=64..72) row=25 (y=200..208) and neighbor rows.
    for r in 23..=27 {
        eng.world
            .terrain
            .mutate_cell(
                CellCoord::new(r, 8),
                Some(60.0),
                Some(TerrainKind::Mountain),
                None,
                None,
            )
            .unwrap();
    }

    let red_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Red-MG",
                    Team::Red,
                    UnitRole::HeavyWeaponTeam,
                    MobilityKind::Foot,
                    WorldPos::new(20.0, 204.0),
                    5,
                    100.0,
                    0.0,
                    "wpn_mg42",
                    "ammo_792x57_ss",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    let blue_behind = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(unit_spec(
                    "Blue-Safe",
                    Team::Blue,
                    UnitRole::InfantrySquad,
                    MobilityKind::Foot,
                    WorldPos::new(120.0, 204.0),
                    9,
                    100.0,
                    0.0,
                    "wpn_luger_p08",
                    "ammo_9x19_m08_ball",
                    vec![],
                )),
            },
            1,
        )
        .created_id
        .unwrap();
    eng.submit_command(
        TacticalCommand::SetFireDiscipline {
            unit_id: UnitId(blue_behind),
            discipline: FireDiscipline::HoldFire,
        },
        1,
    );

    // Direct fire into mountain ridge direction -> ZERO bullets pass through ridge!
    eng.submit_command(
        TacticalCommand::AttackPos {
            unit_id: UnitId(red_id),
            target: WorldPos::new(120.0, 204.0),
            fire_mode: FireDiscipline::FireAtWill,
        },
        1,
    );
    // Also place a timed explosion on west side of ridge (x=52, y=204) with radius 80m.
    eng.submit_command(
        TacticalCommand::ScheduleTimedCharge {
            unit_id: UnitId(red_id),
            pos: WorldPos::new(52.0, 204.0),
            delay_ticks: 1,
            blast_radius_m: 80.0,
            blast_kpa: 180.0,
            crater_depth_m: 2.0,
        },
        1,
    );

    eng.step_n_ticks(2, Team::Red);
    assert_eq!(
        eng.world.units[&blue_behind].hp, 100.0,
        "mountain ridge must 100% block both direct cone fire and reverse-slope blast"
    );
}

#[test]
fn test_b3_benchmark_5000_cone_engagements() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enforce_ammo_consumption = false;

    let mut eng = PureEngine::new_blank(128, 128, 10.0, rs).unwrap();
    let mut unit_ids = Vec::with_capacity(200);
    for i in 0..200u32 {
        let team = if i % 2 == 0 { Team::Red } else { Team::Blue };
        let col_x = 120.0 + ((i % 20) as f32) * 32.0;
        let row_y = 120.0 + ((i / 20) as f32) * 48.0;
        let uid = eng
            .submit_command(
                TacticalCommand::SpawnUnit {
                    spec: Box::new(unit_spec(
                        &format!("U-{i}"),
                        team,
                        if i % 5 == 0 {
                            UnitRole::ArmoredVehicle
                        } else {
                            UnitRole::InfantrySquad
                        },
                        if i % 5 == 0 {
                            MobilityKind::Tracked
                        } else {
                            MobilityKind::Foot
                        },
                        WorldPos::new(col_x, row_y),
                        9,
                        100.0,
                        40.0,
                        "wpn_mg42",
                        "ammo_792x57_ss",
                        vec!["mount_zweibein_34_42"],
                    )),
                },
                1,
            )
            .created_id
            .unwrap();
        unit_ids.push(uid);
    }
    eng.derived.rebuild_if_needed(&eng.world);

    let mut jobs: Vec<(DirectConeField, bool, f32, f32)> = Vec::with_capacity(5000);
    for k in 0..5000usize {
        let uid = unit_ids[k % unit_ids.len()];
        let att = &eng.world.units[&uid];
        let aim = WorldPos::new((att.pos.x + 180.0).min(960.0), att.pos.y);
        if let Some(job) = build_direct_cone_field(
            &eng.world.terrain,
            &eng.catalog,
            att,
            aim,
            eng.world.clock_seconds,
            k as u64 + 1,
            false,
            false,
        ) {
            jobs.push(job);
        }
    }
    assert_eq!(jobs.len(), 5000);

    let mut res_buf = Vec::with_capacity(5000);
    let t_cold = std::time::Instant::now();
    resolve_direct_cone_batch_into(
        &eng.world.terrain,
        &eng.derived,
        &jobs,
        false,
        &mut res_buf,
    );
    let cold_ms = t_cold.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(res_buf.len(), 5000);

    let t_warm = std::time::Instant::now();
    resolve_direct_cone_batch_into(
        &eng.world.terrain,
        &eng.derived,
        &jobs,
        false,
        &mut res_buf,
    );
    let elapsed_ms = t_warm.elapsed().as_secs_f64() * 1000.0;
    println!("BENCH_5000_CONES_COLD_MS={cold_ms:.3} BENCH_5000_CONES_MS={elapsed_ms:.3}");
    assert_eq!(res_buf.len(), 5000);
}
