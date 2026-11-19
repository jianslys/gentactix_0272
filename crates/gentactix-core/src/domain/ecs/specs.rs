use super::*;
use crate::world_grid::MobilityClass;

#[derive(Debug, Clone)]
pub struct EquipmentSpec {
    pub model: &'static str,
    pub name: &'static str,
    pub range_m: f32,
    pub min_range_m: f32,
    pub indirect: bool,
    pub deployable: bool,
    pub towable: bool,
    pub crew: u32,
    pub deploy_hours: f32,
    pub mass_t: f32,
    pub frag_radius_m: f32,
    pub ballistic_group: BallisticGroup,
    pub is_heavy: bool,
    pub is_vehicle: bool,
    pub radio_km: f32,
    pub ammo_type: &'static str,
    pub fire: [f32; 6], // indexed by DamageType as usize
}

pub fn equipment_spec(model: &str) -> &'static EquipmentSpec {
    static RIFLE: EquipmentSpec = EquipmentSpec {
        model: "rifle", name: "步枪", range_m: 400.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 0.004,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [1.0, 0.8, 0.0, 0.0, 0.0, 0.0],
    };
    static SMG: EquipmentSpec = EquipmentSpec {
        model: "smg", name: "冲锋枪", range_m: 200.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 0.004,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [1.6, 1.2, 0.0, 0.0, 0.0, 0.0],
    };
    static LMG: EquipmentSpec = EquipmentSpec {
        model: "lmg", name: "轻机枪", range_m: 800.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.01,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [2.4, 3.0, 0.3, 0.0, 0.0, 0.0],
    };
    static HMG: EquipmentSpec = EquipmentSpec {
        model: "hmg", name: "重机枪", range_m: 1500.0, min_range_m: 0.0, indirect: false,
        deployable: true, towable: false, crew: 3, deploy_hours: 0.5, mass_t: 0.045,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectHeavyMg, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "small_arms",
        fire: [4.2, 6.0, 1.0, 0.2, 0.0, 0.0],
    };
    static ATR: EquipmentSpec = EquipmentSpec {
        model: "atr", name: "反坦克枪", range_m: 600.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.018,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectAntiTank, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "at",
        fire: [0.2, 0.3, 1.2, 2.5, 0.2, 0.0],
    };
    static BAZOOKA: EquipmentSpec = EquipmentSpec {
        model: "bazooka", name: "反坦克火箭筒", range_m: 250.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.01,
        frag_radius_m: 5.0, ballistic_group: BallisticGroup::HeatShaped, is_heavy: false,
        is_vehicle: false, radio_km: 0.0, ammo_type: "at",
        fire: [0.5, 0.5, 2.0, 8.0, 1.5, 0.5],
    };
    static MORTAR_60: EquipmentSpec = EquipmentSpec {
        model: "mortar_60", name: "60mm迫击炮", range_m: 1800.0, min_range_m: 100.0, indirect: true,
        deployable: true, towable: false, crew: 2, deploy_hours: 0.25, mass_t: 0.02,
        frag_radius_m: 18.0, ballistic_group: BallisticGroup::IndirectMortar, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "mortar",
        fire: [3.2, 4.0, 1.2, 0.3, 0.6, 0.0],
    };
    static MORTAR_81: EquipmentSpec = EquipmentSpec {
        model: "mortar_81", name: "81mm迫击炮", range_m: 3000.0, min_range_m: 150.0, indirect: true,
        deployable: true, towable: false, crew: 3, deploy_hours: 0.5, mass_t: 0.06,
        frag_radius_m: 28.0, ballistic_group: BallisticGroup::IndirectMortar, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "mortar",
        fire: [6.0, 7.5, 2.2, 0.5, 1.5, 0.0],
    };
    static MORTAR_120: EquipmentSpec = EquipmentSpec {
        model: "mortar_120", name: "120mm重迫击炮", range_m: 5700.0, min_range_m: 300.0, indirect: true,
        deployable: true, towable: true, crew: 4, deploy_hours: 0.5, mass_t: 0.28,
        frag_radius_m: 42.0, ballistic_group: BallisticGroup::IndirectMortar, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "mortar",
        fire: [11.0, 12.0, 4.0, 1.2, 4.0, 0.0],
    };
    static AT_GUN_50: EquipmentSpec = EquipmentSpec {
        model: "at_gun_50", name: "50mm反坦克炮", range_m: 2000.0, min_range_m: 0.0, indirect: false,
        deployable: true, towable: true, crew: 4, deploy_hours: 0.5, mass_t: 0.9,
        frag_radius_m: 12.0, ballistic_group: BallisticGroup::DirectAntiTank, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "at",
        fire: [1.5, 1.5, 4.0, 10.0, 2.0, 0.0],
    };
    static FLAK_88: EquipmentSpec = EquipmentSpec {
        model: "flak_88", name: "88mm高平两用炮", range_m: 8000.0, min_range_m: 0.0, indirect: false,
        deployable: true, towable: true, crew: 5, deploy_hours: 0.75, mass_t: 5.0,
        frag_radius_m: 25.0, ballistic_group: BallisticGroup::DirectCannon, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [4.0, 4.0, 8.0, 18.0, 5.0, 0.0],
    };
    static HOWITZER_105: EquipmentSpec = EquipmentSpec {
        model: "howitzer_105", name: "105mm榴弹炮", range_m: 11200.0, min_range_m: 500.0, indirect: true,
        deployable: true, towable: true, crew: 6, deploy_hours: 0.75, mass_t: 2.0,
        frag_radius_m: 38.0, ballistic_group: BallisticGroup::IndirectHowitzer, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [12.0, 12.0, 6.0, 2.5, 8.0, 0.0],
    };
    static HOWITZER_152: EquipmentSpec = EquipmentSpec {
        model: "howitzer_152", name: "152mm重榴弹炮", range_m: 17200.0, min_range_m: 800.0, indirect: true,
        deployable: true, towable: true, crew: 8, deploy_hours: 1.0, mass_t: 5.5,
        frag_radius_m: 55.0, ballistic_group: BallisticGroup::IndirectHowitzer, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [20.0, 18.0, 10.0, 5.0, 16.0, 0.0],
    };
    static HOWITZER_203: EquipmentSpec = EquipmentSpec {
        model: "howitzer_203", name: "203mm超重型榴弹炮", range_m: 18000.0, min_range_m: 1000.0, indirect: true,
        deployable: true, towable: true, crew: 8, deploy_hours: 1.5, mass_t: 17.5,
        frag_radius_m: 80.0, ballistic_group: BallisticGroup::IndirectHowitzer, is_heavy: true,
        is_vehicle: false, radio_km: 0.0, ammo_type: "heavy",
        fire: [35.0, 30.0, 18.0, 12.0, 28.0, 0.0],
    };
    static RADIO: EquipmentSpec = EquipmentSpec {
        model: "radio", name: "野战电台", range_m: 0.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 0.015,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 8.0, ammo_type: "small_arms",
        fire: [0.0; 6],
    };
    static HQ_RADIO: EquipmentSpec = EquipmentSpec {
        model: "hq_radio", name: "指挥部大功率电台", range_m: 0.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 0.12,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: false, radio_km: 25.0, ammo_type: "small_arms",
        fire: [0.0; 6],
    };
    static TRUCK: EquipmentSpec = EquipmentSpec {
        model: "truck", name: "卡车", range_m: 0.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 1, deploy_hours: 0.0, mass_t: 3.5,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectSmallArms, is_heavy: false,
        is_vehicle: true, radio_km: 0.0, ammo_type: "small_arms",
        fire: [0.0; 6],
    };
    static HALFTRACK: EquipmentSpec = EquipmentSpec {
        model: "halftrack", name: "半履带装甲车", range_m: 1500.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 2, deploy_hours: 0.0, mass_t: 7.5,
        frag_radius_m: 0.0, ballistic_group: BallisticGroup::DirectHeavyMg, is_heavy: false,
        is_vehicle: true, radio_km: 8.0, ammo_type: "small_arms",
        fire: [4.0, 5.0, 1.5, 0.3, 0.2, 0.0],
    };
    static TANK_MEDIUM: EquipmentSpec = EquipmentSpec {
        model: "tank_medium", name: "中型坦克", range_m: 2500.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 4, deploy_hours: 0.0, mass_t: 28.0,
        frag_radius_m: 15.0, ballistic_group: BallisticGroup::DirectCannon, is_heavy: true,
        is_vehicle: true, radio_km: 8.0, ammo_type: "tank",
        fire: [5.0, 5.0, 8.0, 14.0, 5.0, 0.0],
    };
    static TANK_HEAVY: EquipmentSpec = EquipmentSpec {
        model: "tank_heavy", name: "重型坦克", range_m: 3200.0, min_range_m: 0.0, indirect: false,
        deployable: false, towable: false, crew: 5, deploy_hours: 0.0, mass_t: 46.0,
        frag_radius_m: 20.0, ballistic_group: BallisticGroup::DirectCannon, is_heavy: true,
        is_vehicle: true, radio_km: 10.0, ammo_type: "tank",
        fire: [6.0, 6.0, 10.0, 22.0, 8.0, 0.0],
    };

    match model {
        "rifle" => return &RIFLE,
        "smg" => return &SMG,
        "lmg" => return &LMG,
        "hmg" => return &HMG,
        "atr" | "at_rifle" => return &ATR,
        "bazooka" => return &BAZOOKA,
        "mortar_60" => return &MORTAR_60,
        "mortar_81" => return &MORTAR_81,
        "mortar_120" => return &MORTAR_120,
        "at_gun_50" => return &AT_GUN_50,
        "flak_88" => return &FLAK_88,
        "howitzer_105" => return &HOWITZER_105,
        "howitzer_152" => return &HOWITZER_152,
        "howitzer_203" | "203mm" | "wpn_203" => return &HOWITZER_203,
        "radio" => return &RADIO,
        "hq_radio" => return &HQ_RADIO,
        "truck" => return &TRUCK,
        "halftrack" => return &HALFTRACK,
        "tank_medium" => return &TANK_MEDIUM,
        "tank_heavy" => return &TANK_HEAVY,
        _ => {}
    }

    // 德国历史装备库 (items_catalog) 映射
    if matches!(
        model,
        "wpn_luger_p08"
            | "wpn_walther_p38"
            | "wpn_walther_ppk"
            | "wpn_leuchtpistole_34"
            | "wpn_kar98k"
            | "wpn_kar98k_zf39"
            | "wpn_g41w"
            | "wpn_g43"
            | "wpn_fg42"
            | "wpn_g33_40"
            | "wpn_stg44"
            | "wpn_vg1_5"
    ) {
        return &RIFLE;
    }

    if matches!(
        model,
        "wpn_mp38_40"
            | "wpn_mp41"
            | "wpn_flammenwerfer_35"
            | "wpn_flammenwerfer_41"
    ) {
        return &SMG;
    }

    if matches!(
        model,
        "wpn_mg34" | "wpn_mg42" | "wpn_mg13" | "wpn_mg15"
    ) {
        return &LMG;
    }

    if matches!(
        model,
        "wpn_mg08"
            | "wpn_flak30_20"
            | "wpn_flak38_20"
            | "wpn_gebirgsflak38_20"
            | "wpn_flakvierling38_20"
            | "wpn_flak18_37"
            | "wpn_flak36_37"
            | "wpn_flak37_37"
            | "wpn_flak43_37"
    ) {
        return &HMG;
    }

    if matches!(model, "wpn_pzb39" | "wpn_grb39") {
        return &ATR;
    }

    if matches!(
        model,
        "wpn_panzerfaust_30_klein"
            | "wpn_panzerfaust_30"
            | "wpn_panzerfaust_60"
            | "wpn_panzerfaust_100"
            | "wpn_panzerschreck_rpzb54"
    ) {
        return &BAZOOKA;
    }

    if matches!(model, "wpn_legrw36") {
        return &MORTAR_60;
    }

    if matches!(model, "wpn_grw34" | "wpn_kzgrw42" | "wpn_nbw35_10cm") {
        return &MORTAR_81;
    }

    if matches!(model, "wpn_nbw40_10cm" | "wpn_grw42_120") {
        return &MORTAR_120;
    }

    if matches!(
        model,
        "wpn_spzb41"
            | "wpn_pak36"
            | "wpn_lepak41"
            | "wpn_pak38"
            | "wpn_pak40"
            | "wpn_pak41_75"
            | "wpn_pak43_cruciform"
            | "wpn_pak43_41"
            | "wpn_pak44_128"
    ) {
        return &AT_GUN_50;
    }

    if matches!(
        model,
        "wpn_flak18_88"
            | "wpn_flak36_88"
            | "wpn_flak37_88"
            | "wpn_flak41_88"
            | "wpn_flak38_105"
            | "wpn_flak39_105"
            | "wpn_flak40_128"
            | "wpn_flakzwilling40_128"
    ) {
        return &FLAK_88;
    }

    if matches!(
        model,
        "wpn_leig18"
            | "wpn_legebig18"
            | "wpn_ig37"
            | "wpn_ig42"
            | "wpn_gebg36_75"
            | "wpn_lefh18"
            | "wpn_lefh18m"
            | "wpn_lefh18_40"
            | "wpn_gebh40_105"
            | "wpn_sig33"
    ) {
        return &HOWITZER_105;
    }

    if matches!(
        model,
        "wpn_sk18_10cm"
            | "wpn_sfh18_15cm"
            | "wpn_sfh36_15cm"
            | "wpn_k18_15cm"
            | "wpn_k18_17cm"
            | "wpn_mrs18_21cm"
            | "wpn_nbw41_15cm"
            | "wpn_nbw42_21cm"
            | "wpn_nbw42_30cm"
            | "wpn_wurfrahmen40"
    ) {
        return &HOWITZER_152;
    }

    if matches!(
        model,
        "comm_feldfu_b" | "comm_torn_fu_d2" | "comm_ff33" | "comm_feldkabel_reel"
    ) {
        return &RADIO;
    }
    if matches!(model, "comm_torn_fu_b1") {
        return &HQ_RADIO;
    }

    if let Some(item) = crate::catalog::global_catalog().get(model) {
        if item.kind == "comm" {
            return &RADIO;
        }
        if let Some(ref w) = item.weapon {
            if w.indirect {
                if w.towable {
                    return &HOWITZER_105;
                } else if w.is_heavy_weapon {
                    return &MORTAR_81;
                } else {
                    return &MORTAR_60;
                }
            }
            if let Some(subcat) = item.attr_str("subcategory") {
                match subcat {
                    "at_gun" | "heavy_at_gun" | "superheavy_at_gun" | "squeeze_bore_at" => return &AT_GUN_50,
                    "heavy_aa_at" | "super_heavy_aa_at" | "heavy_aa_gun" | "super_heavy_aa_gun" | "heavy_cannon" | "super_heavy_cannon" => return &FLAK_88,
                    "field_howitzer" | "heavy_howitzer" | "siege_howitzer" | "heavy_rocket_launcher" | "heavy_rocket_crate" => return &HOWITZER_152,
                    "infantry_gun" | "heavy_infantry_gun" | "mountain_howitzer" | "mountain_infantry_gun" => return &HOWITZER_105,
                    "mortar" => return &MORTAR_81,
                    "heavy_mortar" => return &MORTAR_120,
                    "at_rifle" | "grenade_rifle" => return &ATR,
                    "at_rocket_launcher" | "disposable_at" | "rocket_launcher" => return &BAZOOKA,
                    "gpmg" | "lmg" => return &LMG,
                    "hmg_tripod" | "fortress_twin_aa" | "autocannon_aa" | "quad_autocannon_aa" | "med_autocannon_aa" => return &HMG,
                    "smg" | "flamethrower" => return &SMG,
                    _ => {}
                }
            }
            if w.is_heavy_weapon {
                return &HMG;
            }
        }
    }

    &RIFLE
}

/// 武器基准装填冷却时间（秒）
pub fn base_reload_sec_for_weapon(model: &str) -> f32 {
    let m = model.trim().to_ascii_lowercase();
    match m.as_str() {
        "howitzer_203" | "203mm" | "wpn_203" | "wpn_mrs18_21cm" => 92.0, // 超重型火炮 (80~96s，规格基准92s)
        "howitzer_152" | "152mm" | "wpn_sfh18_15cm" | "wpn_k18_15cm" | "ml20" | "sfh18" => 32.0, // 重加榴炮 (32s / 恰好2步)
        "howitzer_105" | "105mm" | "wpn_lefh18" | "lefh18" | "wpn_sig33" => 16.0, // 轻型榴弹炮 (16s / 恰好1步)
        "mortar_120" | "wpn_granatwerfer42_120" => 8.0,
        "flak_88" | "at_gun_50" => 6.0,
        "mortar_81" | "wpn_grw34_81" => 4.0, // 中迫击炮 (4s / 单步4发)
        "mortar_60" => 3.0,
        "lmg" | "hmg" | "wpn_mg34" | "wpn_mg42" => 2.0, // 机枪点射
        "smg" => 2.0,
        _ => 3.0, // 栓动/半自动步枪单兵基准
    }
}


#[derive(Debug, Clone, Copy)]
pub struct VehicleSpec {
    pub model: &'static str,
    pub mobility: MobilityClass,
    /// Base speed in km/h on cost=1.0 terrain (e.g. truck=24 -> ~12 offroad / ~55 highway).
    pub base_speed_kmh: f32,
    pub road_max_kmh: f32,
    pub min_crew: u32,
    pub is_armored: bool,
}

pub fn vehicle_spec(model: &str) -> Option<VehicleSpec> {
    let m = model.trim().to_ascii_lowercase();
    match m.as_str() {
        "truck" | "opel_blitz" => Some(VehicleSpec {
            model: "truck",
            mobility: MobilityClass::Wheeled,
            base_speed_kmh: 24.0,
            road_max_kmh: 55.0,
            min_crew: 1,
            is_armored: false,
        }),
        "armor_car" | "sdkfz_222" => Some(VehicleSpec {
            model: "armor_car",
            mobility: MobilityClass::Wheeled,
            base_speed_kmh: 26.0,
            road_max_kmh: 65.0,
            min_crew: 2,
            is_armored: true,
        }),
        "halftrack" | "sdkfz_251" => Some(VehicleSpec {
            model: "halftrack",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 22.0,
            road_max_kmh: 48.0,
            min_crew: 1,
            is_armored: true,
        }),
        "tank_light" | "tank_lt" | "pz2" => Some(VehicleSpec {
            model: "tank_light",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 21.0,
            road_max_kmh: 44.0,
            min_crew: 3,
            is_armored: true,
        }),
        "tank_medium" | "tank" | "pz4" => Some(VehicleSpec {
            model: "tank_medium",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 19.0,
            road_max_kmh: 38.0,
            min_crew: 4,
            is_armored: true,
        }),
        "tank_heavy" | "tiger" => Some(VehicleSpec {
            model: "tank_heavy",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 14.0,
            road_max_kmh: 26.0,
            min_crew: 4,
            is_armored: true,
        }),
        _ if equipment_spec(model).is_vehicle => Some(VehicleSpec {
            model: "vehicle_generic",
            mobility: MobilityClass::Tracked,
            base_speed_kmh: 18.0,
            road_max_kmh: 40.0,
            min_crew: 1,
            is_armored: true,
        }),
        _ => None,
    }
}

/// Multi-crew heavy weapon bonus: 1 operator suffers solo penalty; 3-4 operators grant sustained suppression/reload buff.
#[inline]
pub fn heavy_crew_buff_mult(crew_count: u32, weapon: &str) -> f32 {
    if crew_count == 0 {
        return 0.0;
    }
    let spec = equipment_spec(weapon);
    if !spec.is_heavy && !spec.deployable && spec.crew <= 1 {
        return 1.0;
    }
    match crew_count {
        1 => 0.70,
        2 => 1.00,
        3 => 1.18,
        _ => 1.32,
    }
}

/// Shared ballistic accuracy decay (no hard range cutoff).
#[inline]
pub fn weapon_accuracy_decay(dist_m: f32, range_m: f32) -> f32 {
    let r = range_m.max(50.0);
    1.0 / (1.0 + (dist_m / r).powi(2))
}

/// Shared ballistic damage decay with CQB small-arms surge & AT close-range penalty.
#[inline]
pub fn weapon_damage_decay(model: &str, dist_m: f32, range_m: f32) -> f32 {
    let spec = equipment_spec(model);
    if dist_m <= 50.0 {
        return match model {
            "rifle" | "smg" | "lmg" => 2.0,
            "at_gun_50" | "flak_88" => 0.15,
            _ => 1.0,
        };
    }
    match spec.ballistic_group {
        BallisticGroup::IndirectMortar | BallisticGroup::IndirectHowitzer | BallisticGroup::HeatShaped => 1.0,
        _ => (-0.65 * (dist_m - 50.0).max(0.0) / range_m.max(50.0)).exp(),
    }
}

/// 3-anchor power-law fire registration curve (50m->0.60, 300m->0.20, 3000m->0.01/240).
pub fn compute_registration_delta(weapon: &str, dist_m: f32, duration_min: f32) -> (f32, f32) {
    let range_m = equipment_spec(weapon).range_m.max(50.0);
    let d_eq = (dist_m * 400.0 / range_m).max(1.0);
    let rate_per_min = if d_eq <= 50.0 {
        0.60 / 15.0
    } else if d_eq <= 300.0 {
        let t = (d_eq - 50.0) / 250.0;
        let p50 = 0.60_f32;
        let p300 = 0.20_f32;
        (p50 + t * (p300 - p50)) / 15.0
    } else {
        let ratio = (300.0 / d_eq).clamp(0.0, 1.0);
        let p300 = 0.20 / 15.0;
        let p3000 = 0.01 / 240.0;
        let k = (ratio.powf(1.6)).max(p3000 / p300);
        (p300 * k).max(p3000 * (3000.0 / d_eq).min(1.0))
    };
    let delta = rate_per_min * duration_min.max(1.0);
    let cap = if d_eq <= 300.0 {
        0.60
    } else {
        (0.60 * (300.0 / d_eq).powf(1.6)).min(0.60)
    };
    (delta, cap)
}

