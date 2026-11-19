use crate::math_ops::{CellCoord, TransientBlastWorkspace, WorldPos};
use crate::types::{UnitId, UnitOrder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentSource {
    Autonomy,
    DirectOrder,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentCandidate {
    pub unit_id: UnitId,
    pub source: IntentSource,
    pub priority: u32,
    pub order: UnitOrder,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionProposal {
    pub unit_id: UnitId,
    pub from: WorldPos,
    pub to: WorldPos,
    pub speed_mps: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EventTerminalStatus {
    Resolved { code: String, detail: String },
    Rejected { code: String, detail: String },
    Cancelled { reason: String },
    Superseded { by_event_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub event_id: u64,
    pub tick: u64,
    pub source: UnitId,
    pub kind: String,
    pub target_cell: Option<CellCoord>,
}

/// Ephemeral per-tick scratch workspace (must be completely cleared at Stage 12).
#[derive(Debug, Default, Clone)]
pub struct TickScratch {
    pub intents: BTreeMap<u32, IntentCandidate>,
    pub motion_proposals: Vec<MotionProposal>,
    pub blast_workspace: TransientBlastWorkspace,
    pub events: Vec<EventRecord>,
    pub terminal_statuses: BTreeMap<u64, EventTerminalStatus>,
    pub prior_positions: BTreeMap<u32, WorldPos>,
}

impl TickScratch {
    pub fn submit_intent(&mut self, cand: IntentCandidate) {
        let key = cand.unit_id.0;
        match self.intents.get(&key) {
            None => {
                self.intents.insert(key, cand);
            }
            Some(existing) => {
                let replace = match (existing.source, cand.source) {
                    (IntentSource::DirectOrder, IntentSource::Autonomy) => false,
                    (IntentSource::Autonomy, IntentSource::DirectOrder) => true,
                    _ => cand.priority >= existing.priority,
                };
                if replace {
                    self.intents.insert(key, cand);
                }
            }
        }
    }

    pub fn is_completely_empty(&self) -> bool {
        self.intents.is_empty()
            && self.motion_proposals.is_empty()
            && self.blast_workspace.peak_overpressure_kpa.is_empty()
            && self.blast_workspace.fragment_flux.is_empty()
            && self.blast_workspace.source_event_ids.is_empty()
            && self.events.is_empty()
            && self.terminal_statuses.is_empty()
            && self.prior_positions.is_empty()
    }

    pub fn clear_and_assert_empty(&mut self) {
        self.intents.clear();
        self.motion_proposals.clear();
        self.blast_workspace.clear_all_active();
        self.events.clear();
        self.terminal_statuses.clear();
        self.prior_positions.clear();
        assert!(
            self.is_completely_empty(),
            "TickScratch failed invariant check: non-empty after clear"
        );
    }
}
