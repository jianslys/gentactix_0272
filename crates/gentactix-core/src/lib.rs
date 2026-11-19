//! Gentactix V3.1 Pure-CPU Deterministic Tactical Simulation Core
//! Modular 8-pillar Rust architecture: `state` (tripartite boundaries), `clock` (13-stage 16s),
//! `domain`, `math_ops`, and runtime pillars.

pub mod clock;
pub mod domain;
pub mod math_ops;
pub mod state;

pub mod baseline_test;
pub mod events;
pub mod intent;
pub mod motion;
pub mod publication;
#[cfg(test)]
pub mod stress_test;
pub mod time_machine;

// Thin compatibility shims preserving 100% existing module import paths during Phase 2 -> Phase 5 transition.
pub use clock::phases as transaction;
pub use domain::catalog;
pub use domain::combat as combat_logistics;
pub use domain::command as command_log;
pub use domain::command::autonomy as spatial_autonomy;
pub use domain::ecs as units_ecs;
pub use domain::terrain as world_grid;
pub use domain::terrain::mutation as operators;

pub mod sim_loop {
    pub use crate::clock::pathfinding::plan_grid_path_f32;
    pub use crate::state::presets::*;
    pub use crate::state::world_state::*;
}

pub mod geometry {
    pub use crate::math_ops::geometry_8m::*;
    pub use crate::math_ops::mod_b_engineer as tactical_instance;
}

pub mod fields {
    pub use crate::math_ops::mod_a_g_field::*;
    pub use crate::math_ops::mod_c_i_field::*;
    pub use crate::math_ops::mod_d_radio_zoh::*;
}

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
pub use geometry::{
    CellCoord, GridGeometry, Meter, OutsideBoundsError, WorldPos, TACTICAL_CELL_AREA_M2,
    TACTICAL_CELL_SHIFT, TACTICAL_CELL_SIZE_M,
};
pub use intent::{IntentArbitrator, IntentCandidate, IntentSource, MotionIntent};
pub use motion::{commit_motions, ConstrainedMotion, MotionConstraintSolver, MotionProposal};
pub use operators::{DirtyHaloBox, TerrainMutation, TerrainTransaction};
pub use publication::{
    compute_authority_fingerprint, load_world_state_from_json, save_world_state_to_json,
};
pub use state::{
    ClockState, DerivedState, EntityAuthorityRecord, EntityState, InventoryLedger,
    PersistentFieldState, SimDerivedState, SimTickScratch, StateKind, TerrainState, TickScratch,
    WorldState,
};
pub use time_machine::*;
pub use transaction::{
    DEFAULT_SUBTICK_SECONDS, TickClock, TickOrchestrator, TickSummary, TransactionError,
    TransactionPhase,
};
