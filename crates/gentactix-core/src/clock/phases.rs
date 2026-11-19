use serde::{Deserialize, Serialize};

/// 13 deterministic simulation stages executed strictly in sequence per 16-second sub-tick.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TickStage {
    BeginTick = 0,
    ClockAndSchedule = 1,
    PersistentFieldEvolve = 2,
    DerivedRefresh = 3,
    SenseAndArbitrate = 4,
    ProposeMotion = 5,
    ConstrainMotion = 6,
    CommitMotion = 7,
    EmitEvents = 8,
    ResolveEvents = 9,
    CommitPersistent = 10,
    Validate = 11,
    PublishAndDrop = 12,
}

impl TickStage {
    pub const ALL: [TickStage; 13] = [
        Self::BeginTick,
        Self::ClockAndSchedule,
        Self::PersistentFieldEvolve,
        Self::DerivedRefresh,
        Self::SenseAndArbitrate,
        Self::ProposeMotion,
        Self::ConstrainMotion,
        Self::CommitMotion,
        Self::EmitEvents,
        Self::ResolveEvents,
        Self::CommitPersistent,
        Self::Validate,
        Self::PublishAndDrop,
    ];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BeginTick => "begin_tick",
            Self::ClockAndSchedule => "clock_and_schedule",
            Self::PersistentFieldEvolve => "persistent_field_evolve",
            Self::DerivedRefresh => "derived_refresh",
            Self::SenseAndArbitrate => "sense_and_arbitrate",
            Self::ProposeMotion => "propose_motion",
            Self::ConstrainMotion => "constrain_motion",
            Self::CommitMotion => "commit_motion",
            Self::EmitEvents => "emit_events",
            Self::ResolveEvents => "resolve_events",
            Self::CommitPersistent => "commit_persistent",
            Self::Validate => "validate",
            Self::PublishAndDrop => "publish_and_drop",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v2_p2_thirteen_stages_strict_order() {
        assert_eq!(TickStage::ALL.len(), 13);
        for (expected_idx, &stage) in TickStage::ALL.iter().enumerate() {
            assert_eq!(stage.index(), expected_idx);
            if expected_idx > 0 {
                assert!(TickStage::ALL[expected_idx - 1] < stage);
            }
        }
        assert_eq!(TickStage::CommitMotion.index(), 7);
        assert_eq!(TickStage::CommitPersistent.index(), 10);
        assert_eq!(TickStage::PublishAndDrop.index(), 12);
    }
}
