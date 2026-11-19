pub mod routes;

pub use routes::build_router;

use gentactix_core::engine::PureEngine;
use gentactix_core::ruleset::SimRuleset;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<RwLock<PureEngine>>,
}

impl AppState {
    pub fn new_blank(
        rows: usize,
        cols: usize,
        base_elevation_m: f32,
        ruleset: SimRuleset,
    ) -> Result<Self, String> {
        let engine = PureEngine::new_blank(rows, cols, base_elevation_m, ruleset)?;
        Ok(Self {
            engine: Arc::new(RwLock::new(engine)),
        })
    }
}
