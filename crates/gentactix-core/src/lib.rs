//! Gentactix V3.1 Pure-CPU Deterministic Tactical Simulation Core
//! Consolidated 6-module Rust architecture (replaces 18 fragmented Python files).

pub mod baseline_test;
pub mod catalog;
pub mod combat_logistics;
pub mod command_log;
pub mod events;
pub mod fields;
pub mod geometry;
pub mod intent;
pub mod motion;
pub mod operators;
pub mod publication;
pub mod sim_loop;
pub mod spatial_autonomy;
pub mod state;
#[cfg(test)]
pub mod stress_test;
pub mod time_machine;
pub mod transaction;
pub mod units_ecs;
pub mod world_grid;

pub use combat_logistics::{
    filter_and_dispatch_fragments, forward_152mm_he_packet, logistics_tonnage_transfer_sec,
    resolve_projectile_hit, scavenge_ground_body, unit_cross_section_weight, ProjectileHitReport,
};
pub use units_ecs::{
    Echelon, FireIntensityMode, FireTask, TaskLifecyclePhase, TaskTimingScheduler,
};
pub use world_grid::{PixelCellContainer, PriorityTarget, ShotPacket};

// -----------------------------------------------------------------------------
// GTX-CORE-SPEC-FINAL-2.0 Target Architecture Clean Exports (Phase 01..10)
// -----------------------------------------------------------------------------
pub use events::{EventBuffer, EventFinalStatus, EventKind, EventPayload, InstantEvent};
pub use fields::{execute_single_fire_slice, FieldAlgebra, TransientBlastWorkspace};
pub use geometry::{CellCoord, GridGeometry, Meter, OutsideBoundsError, WorldPos, TACTICAL_CELL_AREA_M2, TACTICAL_CELL_SHIFT, TACTICAL_CELL_SIZE_M};
pub use intent::{IntentArbitrator, IntentCandidate, IntentSource, MotionIntent};
pub use motion::{commit_motions, ConstrainedMotion, MotionConstraintSolver, MotionProposal};
pub use operators::{DirtyHaloBox, TerrainMutation, TerrainTransaction};
pub use publication::{compute_authority_fingerprint, load_world_state_from_json, save_world_state_to_json};
pub use state::{ClockState, DerivedState, EntityAuthorityRecord, EntityState, InventoryLedger, PersistentFieldState, StateKind, TerrainState, TickScratch, WorldState};
pub use time_machine::*;
pub use transaction::{DEFAULT_SUBTICK_SECONDS, TickClock, TickOrchestrator, TickSummary, TransactionError, TransactionPhase};
