use super::*;
use crate::world_grid::MobilityClass;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModifierSource {
    OrderChurn,
    WarCouncilSync,
    OverloadAxleSnap,
    OverloadGearbox,
    OverloadEngineOverheat,
    MineDamaged,
    RapidMove,
    ResupplyPickup,
    EventWork,
    ReorganizeEchelon,
    CellSuppression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierSlot {
    pub source: ModifierSource,
    pub tag: &'static str,
    pub locks: LockFlags,
    pub speed_mult: f32,
    pub fuel_mult: f32,
    pub combat_power_mult: f32,
    pub recon_mult: f32,
    pub kc_mult: f32,
    pub tau_mult: f32,
    pub shock_m_add: f32,
    pub org_delta_per_h: f32,
    pub exposure_delta: f32,
    pub ttl_hours: Option<f32>,
}

impl Default for ModifierSlot {
    fn default() -> Self {
        Self {
            source: ModifierSource::EventWork,
            tag: "NEUTRAL",
            locks: LockFlags::empty(),
            speed_mult: 1.0,
            fuel_mult: 1.0,
            combat_power_mult: 1.0,
            recon_mult: 1.0,
            kc_mult: 1.0,
            tau_mult: 1.0,
            shock_m_add: 0.0,
            org_delta_per_h: 0.0,
            exposure_delta: 0.0,
            ttl_hours: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedModifiers {
    pub locks: LockFlags,
    pub speed_mult: f32,
    pub fuel_mult: f32,
    pub combat_power_mult: f32,
    pub recon_mult: f32,
    pub kc_mult: f32,
    pub tau_mult: f32,
    pub shock_m_add: f32,
    pub org_delta_per_h: f32,
    pub exposure_delta: f32,
    pub tags: Vec<&'static str>,
}

impl AggregatedModifiers {
    pub fn neutral() -> Self {
        Self {
            locks: LockFlags::empty(),
            speed_mult: 1.0,
            fuel_mult: 1.0,
            combat_power_mult: 1.0,
            recon_mult: 1.0,
            kc_mult: 1.0,
            tau_mult: 1.0,
            shock_m_add: 0.0,
            org_delta_per_h: 0.0,
            exposure_delta: 0.0,
            tags: Vec::new(),
        }
    }
}

pub fn set_modifier_slot(slots: &mut Vec<ModifierSlot>, slot: ModifierSlot) {
    if let Some(ex) = slots.iter_mut().find(|s| s.source == slot.source) {
        *ex = slot;
    } else {
        slots.push(slot);
    }
}

pub fn aggregate_modifiers(slots: &[ModifierSlot], churn_score: f32, war_council_sync_active: bool) -> AggregatedModifiers {
    let mut agg = AggregatedModifiers::neutral();
    for s in slots {
        agg.locks |= s.locks;
        agg.speed_mult *= s.speed_mult;
        agg.fuel_mult *= s.fuel_mult;
        agg.combat_power_mult *= s.combat_power_mult;
        agg.recon_mult *= s.recon_mult;
        agg.kc_mult *= s.kc_mult;
        agg.tau_mult *= s.tau_mult;
        agg.shock_m_add += s.shock_m_add;
        agg.org_delta_per_h += s.org_delta_per_h;
        agg.exposure_delta += s.exposure_delta;
        agg.tags.push(s.tag);
    }

    // Order Churn Debuff + War Council Sync coexistence (never deletes ORDER_CHURN_DEBUFF!)
    let mut churn_speed = 1.0f32;
    let mut churn_power = 1.0f32;
    let mut churn_org_dh = 0.0f32;
    if churn_score > 1.0 {
        let s = churn_score - 1.0;
        churn_speed = (1.0 - 0.10 * s).max(0.55);
        churn_power = (1.0 - 0.08 * s).max(0.60);
        churn_org_dh = -1.5 * s;
        agg.speed_mult *= churn_speed;
        agg.combat_power_mult *= churn_power;
        agg.org_delta_per_h += churn_org_dh;
        agg.kc_mult *= (-0.25 * s).exp().max(0.30);
        agg.tau_mult *= 1.0 + 0.45 * s;
        agg.shock_m_add += (0.08 * s).min(0.45);
        agg.tags.push("ORDER_CHURN_DEBUFF");
        agg.tags.push("ORDER_CONFUSION");
    }

    if war_council_sync_active {
        let comp_speed = (1.0 / churn_speed.max(0.1)) * 1.08;
        let comp_power = (1.0 / churn_power.max(0.1)) * 1.10;
        let comp_org_dh = if churn_org_dh < 0.0 { -churn_org_dh + 1.0 } else { 1.0 };
        agg.speed_mult *= comp_speed;
        agg.combat_power_mult *= comp_power;
        agg.org_delta_per_h += comp_org_dh;
        agg.kc_mult = 1.15;
        agg.tau_mult = 0.75;
        agg.shock_m_add = 0.0;
        agg.tags.push("WAR_COUNCIL_SYNC_BUFF");
    }

    agg.locks = agg.locks.normalized();
    agg
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgTargetSpec {
    pub str_id: String,
    pub name: String,
    pub echelon: Echelon,
    pub enlisted: u32,
    pub cadre: u32,
    pub equipment: Vec<(String, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    RapidMove,
    ResupplyPickup {
        food_t: f32,
        ammo_t: f32,
        fuel_t: f32,
    },
    Demolition {
        target_px: (i32, i32),
        target_type: String,
    },
    DeployHeavyUnit {
        weapon: String,
        keep_manned: bool,
        operator_soldier_ids: Vec<String>,
        operator_count: u32,
    },
    PackHeavyUnit {
        target_unit_id: UnitId,
        attach: bool,
    },
    OperateHeavyUnit {
        target_unit_id: UnitId,
    },
    RepairDebuff {
        target_unit_id: UnitId,
    },
    FireWeapon {
        weapon: String,
        target_px: (i32, i32),
        radius_px: f32,
    },
    RegisterFire {
        weapon: String,
        target_px: (i32, i32),
        radius_px: f32,
    },
    Engineering {
        op: String,
        target_px: (i32, i32),
    },
    RadioDecoy {
        target_px: (i32, i32),
        ttl_h: f32,
    },
    FormCommandSquad {
        preferred_name: String,
        dissolve_if_empty: bool,
    },
    LayWire {
        target_px: (i32, i32),
    },
    CargoTransfer {
        target_unit_id: UnitId,
        food_t: f32,
        ammo_t: f32,
        fuel_t: f32,
        equipment: Vec<(String, u32)>,
    },
    ReorganizeEchelon {
        commander_id: CommanderId,
        commander_piece_id: Option<UnitId>,
        target_echelon: Echelon,
        source_unit_ids: Vec<UnitId>,
        target_specs: Vec<ReorgTargetSpec>,
        strict_cadre_allocation: bool,
    },
}

impl EventKind {
    pub fn locks(&self) -> LockFlags {
        match self {
            EventKind::RapidMove => LockFlags::FORBID_REST | LockFlags::FORBID_DEPLOY,
            EventKind::ResupplyPickup { .. } => LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE | LockFlags::FORBID_REST,
            EventKind::Demolition { .. }
            | EventKind::DeployHeavyUnit { .. }
            | EventKind::PackHeavyUnit { .. }
            | EventKind::OperateHeavyUnit { .. }
            | EventKind::RepairDebuff { .. }
            | EventKind::FireWeapon { .. }
            | EventKind::RegisterFire { .. }
            | EventKind::Engineering { .. }
            | EventKind::FormCommandSquad { .. }
            | EventKind::LayWire { .. }
            | EventKind::CargoTransfer { .. }
            | EventKind::ReorganizeEchelon { .. } => {
                LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE | LockFlags::FORBID_REST | LockFlags::FORBID_DEPLOY
            }
            EventKind::RadioDecoy { .. } => LockFlags::HOLD_STILL | LockFlags::FORBID_MOVE,
        }
    }

    pub fn combat_power_mult(&self) -> f32 {
        match self {
            EventKind::RapidMove => 0.75,
            EventKind::RepairDebuff { .. } | EventKind::Engineering { .. } => 0.5,
            EventKind::LayWire { .. } | EventKind::CargoTransfer { .. } | EventKind::FormCommandSquad { .. } => 0.6,
            EventKind::ReorganizeEchelon { .. } => 0.8,
            EventKind::RegisterFire { .. } => 0.85,
            _ => 1.0,
        }
    }

    pub fn speed_mult(&self) -> f32 {
        match self {
            EventKind::RapidMove => 1.4,
            EventKind::ReorganizeEchelon { .. } => 0.0,
            _ => 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveEvent {
    pub id: u64,
    pub actor_id: UnitId,
    pub remaining_hours: f32,
    pub labor_persons: u32,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CounterEvaluation {
    pub restricted_echelon: Echelon,
    pub org_cap_limit: f32,
    pub empty_cadre: bool,
    pub recovery_rate_mult: f32,
    pub display_tier_bonus: i8,
    pub mobility: MobilityClass,
}

#[inline]
pub fn infer_nco_officer(kind: &str, echelon: Echelon, cadre: u32) -> (u32, u32) {
    if cadre == 0 {
        return (0, 0);
    }
    let kl = kind.to_ascii_lowercase();
    let is_hq = kl.contains("hq") || kl.contains("cmd") || kl.contains("command");
    if is_hq || (echelon as u8) >= (Echelon::Platoon as u8) {
        if cadre == 1 {
            (0, 1)
        } else {
            let off = if is_hq { (cadre / 2).max(1) } else { (cadre / 3).max(1) };
            (cadre - off, off)
        }
    } else {
        (cadre, 0)
    }
}

pub fn evaluate_counter_and_caps(
    enlisted: u32,
    cadre: u32,
    equipment: &FxHashMap<String, u32>,
    declared_echelon: Echelon,
) -> CounterEvaluation {
    let (nco, officer) = infer_nco_officer("", declared_echelon, cadre);
    evaluate_counter_and_caps_4ledger(enlisted, nco, officer, equipment, declared_echelon)
}

pub fn evaluate_counter_and_caps_4ledger(
    enlisted: u32,
    nco: u32,
    officer: u32,
    equipment: &FxHashMap<String, u32>,
    declared_echelon: Echelon,
) -> CounterEvaluation {
    let cadre = nco + officer;
    let total = enlisted + cadre;
    let empty_cadre = cadre == 0 && total > 0;
    let has_vehicle = equipment.iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_vehicle);
    let has_armor = equipment.get("tank_medium").copied().unwrap_or(0) > 0
        || equipment.get("tank_heavy").copied().unwrap_or(0) > 0;
    let has_heavy = equipment.iter().any(|(m, &n)| n > 0 && equipment_spec(m).is_heavy);

    let max_by_manpower = if total <= 15 {
        Echelon::Squad
    } else if total <= 55 {
        Echelon::Platoon
    } else if total <= 220 {
        Echelon::Company
    } else if total <= 900 {
        Echelon::Battalion
    } else if total <= 3000 {
        Echelon::Regiment
    } else if total <= 8000 {
        Echelon::Brigade
    } else {
        Echelon::Division
    };

    let max_by_cadre = if cadre == 0 {
        Echelon::Squad
    } else if officer == 0 {
        // 士官代理上限：最高仅允许排级临时指挥
        Echelon::Platoon
    } else if cadre <= 2 {
        Echelon::Platoon
    } else if cadre <= 8 {
        Echelon::Company
    } else {
        Echelon::Division
    };

    let restricted = if (declared_echelon as u8) <= (max_by_manpower as u8) {
        declared_echelon
    } else {
        max_by_manpower
    };

    let mut org_cap_limit: f32 = if empty_cadre { ORG_CAP_FLOOR } else { 100.0 };
    if total > 0 && (restricted as u8) > (max_by_cadre as u8) {
        org_cap_limit = org_cap_limit.min(65.0);
    }

    let recovery_rate_mult = if empty_cadre {
        0.20
    } else if cadre >= 2 {
        1.15
    } else {
        1.0
    };

    let mobility = if has_armor || equipment.get("halftrack").copied().unwrap_or(0) > 0 {
        MobilityClass::Tracked
    } else if has_vehicle {
        MobilityClass::Wheeled
    } else {
        MobilityClass::Foot
    };

    let display_tier_bonus = if has_heavy || has_armor { 1 } else { 0 };

    CounterEvaluation {
        restricted_echelon: restricted,
        org_cap_limit,
        empty_cadre,
        recovery_rate_mult,
        display_tier_bonus,
        mobility,
    }
}
