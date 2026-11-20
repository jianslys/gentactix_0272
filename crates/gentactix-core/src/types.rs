use crate::math_ops::WorldPos;
use serde::{Deserialize, Serialize};

/// Canonical simulation clock cadence: 16 seconds per deterministic sub-tick.
pub const TICK_SECONDS: u64 = 16;
pub const TICK_SECONDS_F32: f32 = 16.0;
/// Default scenario start wall clock: 06:30:00 (23400s).
pub const DEFAULT_EPOCH_SECONDS: u64 = 23400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisTier {
    #[default]
    Hidden = 0,
    Suspected = 1,
    Spotted = 2,
    Full = 3,
}

impl Team {
    #[inline]
    pub const fn index(self) -> Option<usize> {
        match self {
            Self::Red => Some(0),
            Self::Blue => Some(1),
            Self::Neutral => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct UnitId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlatoonId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    Red,
    Blue,
    Neutral,
}

impl Team {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Blue => "blue",
            Self::Neutral => "neutral",
        }
    }

    #[inline]
    pub const fn is_hostile_to(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Red, Self::Blue) | (Self::Blue, Self::Red)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitRole {
    PlatoonHq,
    InfantrySquad,
    HeavyWeaponTeam,
    EngineerSquad,
    ArmoredVehicle,
    ReconTeam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobilityKind {
    Foot,
    Wheeled,
    Tracked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStance {
    #[default]
    Normal,
    Fast,
    Cautious,
}

impl MoveStance {
    #[inline]
    pub const fn speed_mult(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Fast => 1.35,
            Self::Cautious => 0.65,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FireDiscipline {
    HoldFire,
    ReturnFire,
    #[default]
    FireAtWill,
    Suppressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineerWorkKind {
    Minefield,
    FoxholeLine,
    ZigzagTrench,
    Demolition,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnitOrder {
    #[default]
    Hold,
    Move {
        waypoints: Vec<WorldPos>,
        #[serde(default)]
        stance: MoveStance,
    },
    AttackPos {
        target: WorldPos,
        #[serde(default)]
        fire_mode: FireDiscipline,
    },
    AttackUnit {
        target: UnitId,
        #[serde(default)]
        chase: bool,
        #[serde(default)]
        fire_mode: FireDiscipline,
    },
    BuildWork {
        work: EngineerWorkKind,
        waypoints: Vec<WorldPos>,
    },
    PopSmoke {
        target: WorldPos,
    },
}
