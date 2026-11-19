use crate::command_log::autonomy::SpatialBucketGrid;
use crate::domain::ecs::UnitSoA;
use crate::domain::terrain::WorldGrid;
use crate::math_ops::GFieldKind;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GFieldCellState {
    pub g0: f32,
    pub committed_en_route: f32,
    pub actual_on_site: f32,
    pub kind: GFieldKind,
}

/// 引擎级 DerivedState：可随时销毁并由 WorldState 无损重建的派生计算视图
#[derive(Debug, Clone)]
pub struct SimDerivedState {
    pub buckets: SpatialBucketGrid,
    pub synced_terrain_rev: u64,
    pub g_field_cells: FxHashMap<usize, GFieldCellState>,
    pub g_auction_assignments: FxHashMap<usize, usize>,
    pub unit_radio_t_diff: Vec<f32>,
}

impl SimDerivedState {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            buckets: SpatialBucketGrid::new(rows, cols),
            synced_terrain_rev: u64::MAX,
            g_field_cells: FxHashMap::default(),
            g_auction_assignments: FxHashMap::default(),
            unit_radio_t_diff: Vec::new(),
        }
    }

    pub fn invalidate(&mut self) {
        self.synced_terrain_rev = u64::MAX;
        self.g_field_cells.clear();
        self.g_auction_assignments.clear();
        self.unit_radio_t_diff.clear();
        self.buckets = SpatialBucketGrid::new(self.buckets.b_rows << self.buckets.bucket_shift, self.buckets.b_cols << self.buckets.bucket_shift);
    }

    pub fn rebuild_if_needed(&mut self, grid: &WorldGrid, units: &UnitSoA) {
        self.buckets.rebuild(units, grid.rows, grid.cols);
        if self.unit_radio_t_diff.len() < units.id.len() {
            self.unit_radio_t_diff.resize(units.id.len(), 1.0);
        }
        self.synced_terrain_rev = grid.terrain_revision;
    }
}
