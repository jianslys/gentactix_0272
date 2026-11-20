use gentactix_core::commands::TacticalCommand;
use gentactix_core::damage_model::{
    select_armor_facing, ArmorFacing, ArmorProfileRha, BodyPartKind, VehicleSubsystemKind,
};
use gentactix_core::engine::PureEngine;
use gentactix_core::math_ops::WorldPos;
use gentactix_core::ruleset::{SimRuleset, UnitSpawnSpec};
use gentactix_core::step_cache::WorldInstanceKind;
use gentactix_core::types::{MobilityKind, MoveStance, Team, UnitId, UnitRole};

fn make_infantry_spec(callsign: &str, team: Team, pos: WorldPos) -> UnitSpawnSpec {
    UnitSpawnSpec {
        callsign: callsign.into(),
        team,
        platoon_id: None,
        role: UnitRole::InfantrySquad,
        mobility: MobilityKind::Foot,
        pos,
        facing_deg: 0.0,
        men: 9,
        hp: 90.0,
        armor_mm: 0.0,
        weapon_id: "wpn_mg42".into(),
        ammo_id: "ammo_792x57_ss".into(),
        fuze_id: None,
        mount_ids: vec!["mount_zweibein_34_42".into()],
        ammo_rounds: 600,
        smoke_grenades: 2,
        base_speed_mps: 2.0,
        vision_m: 300.0,
        base_radio_range_m: 500.0,
    }
}

fn make_tank_spec(callsign: &str, team: Team, pos: WorldPos, armor_mm: f32) -> UnitSpawnSpec {
    UnitSpawnSpec {
        callsign: callsign.into(),
        team,
        platoon_id: None,
        role: UnitRole::ArmoredVehicle,
        mobility: MobilityKind::Tracked,
        pos,
        facing_deg: 0.0,
        men: 4,
        hp: 300.0,
        armor_mm,
        weapon_id: "wpn_mg42".into(),
        ammo_id: "ammo_792x57_pmk".into(),
        fuze_id: None,
        mount_ids: vec!["mount_lafette_42".into()],
        ammo_rounds: 400,
        smoke_grenades: 2,
        base_speed_mps: 6.0,
        vision_m: 350.0,
        base_radio_range_m: 800.0,
    }
}

#[test]
fn test_b2_precompiled_catalog_profile_idx_matches_string_resolve() {
    let rs = SimRuleset::arcade_sandbox();
    let mut eng = PureEngine::new_blank(32, 32, 10.0, rs).unwrap();
    let uid = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_infantry_spec(
                    "Red-MG",
                    Team::Red,
                    WorldPos::new(64.0, 64.0),
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    let u = &eng.world.units[&uid];
    let pidx = u.profile_idx.expect("spawned unit must have precompiled AttackProfileIdx");
    let compact = eng.catalog.compact_profile(pidx).unwrap();
    let mounts: Vec<&str> = u.mount_ids.iter().map(String::as_str).collect();
    let resolved = eng
        .catalog
        .resolve_attack_profile(&u.weapon_id, &u.ammo_id, u.fuze_id.as_deref(), &mounts)
        .unwrap();

    assert_eq!(compact.combat_rpm, resolved.combat_rpm);
    assert_eq!(compact.base_accuracy, resolved.base_accuracy);
    assert_eq!(compact.armor_pen_mm, resolved.armor_pen_mm);
    assert_eq!(compact.damage, resolved.damage);
    assert_eq!(compact.frag_radius_m, resolved.frag_radius_m);
    assert_eq!(compact.is_dud, resolved.is_dud);
}

#[test]
fn test_b2_infantry_per_soldier_4_regions_bleeding_and_corpse_spawn() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enable_g_field_autonomy = false;
    rs.enable_time_machine_deferral = false;

    let mut eng = PureEngine::new_blank(32, 32, 10.0, rs).unwrap();
    let uid = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_infantry_spec(
                    "Red-Sq1",
                    Team::Red,
                    WorldPos::new(64.0, 64.0),
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    {
        let u = eng.world.units.get_mut(&uid).unwrap();
        assert!(!u.is_vehicle && u.soldiers.len() == 9);
        // Wound soldier 0 in legs (no lethal bleed, affects squad leg scale slightly).
        let out_leg = u.soldiers[0].apply_hit(BodyPartKind::LowerLimbs, 3.0, 0.2);
        assert!(!out_leg.killed);
        // Wound soldier 1 severely in torso -> fatal bleed trajectory across upcoming ticks!
        let out_torso = u.soldiers[1].apply_hit(BodyPartKind::Torso, 6.2, 0.5);
        assert!(!out_torso.killed);
        assert!(u.soldiers[1].bleed_rate_per_tick > 1.5);
        // Untouched soldier 2 stays at full integrity.
        assert_eq!(u.soldiers[2].regions[BodyPartKind::Head as usize].integrity, 1.0);
        assert_eq!(u.soldiers[2].regions[BodyPartKind::Torso as usize].integrity, 1.0);
        u.recompute_damage_aggregates();
    }

    // Save/load preserves exact per-soldier anatomy & bleeding rates.
    let saved = eng.save_json().unwrap();
    let mut eng_reloaded = PureEngine::new_blank(8, 8, 0.0, SimRuleset::arcade_sandbox()).unwrap();
    eng_reloaded.load_json(&saved).unwrap();
    assert_eq!(eng.world.authoritative_sha256(), eng_reloaded.world.authoritative_sha256());

    // Advance 6 ticks (96s): wounded soldier #1 bleeds out, men drops 9 -> 8, Corpse created!
    for _ in 0..8 {
        eng.step_16s(Team::Red);
        eng_reloaded.step_16s(Team::Red);
    }
    assert_eq!(eng.world.authoritative_sha256(), eng_reloaded.world.authoritative_sha256());
    assert!(!eng.world.units[&uid].soldiers[1].alive);
    assert_eq!(eng.world.units[&uid].men, 8);
    assert!(eng
        .world
        .world_instances
        .values()
        .any(|inst| inst.kind == WorldInstanceKind::Corpse && inst.owner_unit_id == Some(UnitId(uid))));
    assert!(eng
        .world
        .combat_logs
        .iter()
        .any(|l| l.kind == "bleed_out" && l.actor_id == UnitId(uid) && l.casualties == 1));
}

#[test]
fn test_b2_tank_4_facing_rha_8_subsystems_and_zero_overpenetration_spalling() {
    let mut rs = SimRuleset::arcade_sandbox();
    rs.enable_g_field_autonomy = false;
    rs.enable_time_machine_deferral = false;

    let mut eng = PureEngine::new_blank(32, 32, 10.0, rs).unwrap();
    let tank_id = eng
        .submit_command(
            TacticalCommand::SpawnUnit {
                spec: Box::new(make_tank_spec(
                    "Panzer-IV",
                    Team::Red,
                    WorldPos::new(100.0, 100.0),
                    80.0,
                )),
            },
            1,
        )
        .created_id
        .unwrap();

    // Verify default 4-facing RHA mapping & facing selector angles (tank facing East = +X).
    assert_eq!(select_armor_facing([-1.0, 0.0], 0.0, false), ArmorFacing::Front);
    assert_eq!(select_armor_facing([0.0, -1.0], 0.0, false), ArmorFacing::Side);
    assert_eq!(select_armor_facing([1.0, 0.0], 0.0, false), ArmorFacing::Rear);
    assert_eq!(select_armor_facing([-1.0, 0.0], 0.0, true), ArmorFacing::Top);

    assert!(eng.world.set_unit_armor_rha(
        UnitId(tank_id),
        ArmorProfileRha {
            front_mm: 80.0,
            side_mm: 30.0,
            rear_mm: 20.0,
            top_mm: 12.0,
        },
    ));

    // 1. Sub-caliber / insufficient penetration on front (70mm < 80mm): bounce! Zero internal spall, zero exit flux.
    {
        let vd = eng.world.units.get_mut(&tank_id).unwrap().vehicle_damage.as_mut().unwrap();
        let bounce = vd.apply_ballistic_impact(ArmorFacing::Front, 70.0, 15.0, 42);
        assert!(!bounce.penetrated);
        assert_eq!(bounce.internal_spall_energy, 0.0);
        assert_eq!(bounce.transmitted_flux, 0.0);
        assert_eq!(vd.crew_alive, 4);
    }

    // 2. Massive AP hit on side (150mm >> 30mm): penetrates, 100% energy stays INSIDE hull as spall, 0.0 exits!
    {
        let u = eng.world.units.get_mut(&tank_id).unwrap();
        let vd = u.vehicle_damage.as_mut().unwrap();
        let pen = vd.apply_ballistic_impact(ArmorFacing::Side, 150.0, 60.0, 777);
        assert!(pen.penetrated);
        assert!(pen.internal_spall_energy > 60.0);
        assert_eq!(pen.transmitted_flux, 0.0, "tank MUST NEVER overpenetrate!");
        assert!(vd.subsystems.iter().any(|&h| h < 1.0));
        u.hp = (u.hp - pen.hull_hp_loss).max(0.0);
        u.recompute_damage_aggregates();
    }

    // 3. Damage Tracks, Radio, and MainGun -> verify mobility immobilization, radio blackout, and fire suppression!
    {
        let u = eng.world.units.get_mut(&tank_id).unwrap();
        let vd = u.vehicle_damage.as_mut().unwrap();
        vd.crew_alive = vd.crew_alive.max(1);
        vd.ammo_cooked_off = false;
        vd.subsystems[VehicleSubsystemKind::Tracks as usize] = 0.05;
        vd.subsystems[VehicleSubsystemKind::Radio as usize] = 0.05;
        vd.subsystems[VehicleSubsystemKind::MainGun as usize] = 0.05;
        u.alive = true;
        u.hp = u.hp.max(50.0);
        u.recompute_damage_aggregates();
    }

    let pos_before = eng.world.units[&tank_id].pos;
    eng.submit_command(
        TacticalCommand::MoveUnit {
            unit_id: UnitId(tank_id),
            waypoints: vec![WorldPos::new(160.0, 100.0)],
            stance: MoveStance::Normal,
        },
        1,
    );
    eng.step_16s(Team::Red);
    assert_eq!(
        eng.world.units[&tank_id].pos, pos_before,
        "destroyed tracks must immobilize tank"
    );
    assert_eq!(
        eng.world.units[&tank_id].effective_radio_range_m, 0.0,
        "destroyed radio must collapse effective radio range"
    );
    assert!(
        !eng.world.units[&tank_id].can_fire_main_weapon(),
        "destroyed main gun must block weapon firing"
    );
}
