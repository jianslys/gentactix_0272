pub mod autonomy;
pub mod los_combat;
pub mod orchestrator;
pub mod pathfinding;
pub mod phases;

pub use los_combat::{check_los, resolve_fire_exchange, FireOutcome, LosResult};
pub use orchestrator::{generate_work_points, TickOrchestrator, TickReport};
pub use pathfinding::{constrain_segment_motion, find_path_world, is_cell_passable, is_point_passable};
pub use phases::TickStage;
