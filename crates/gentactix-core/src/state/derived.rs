use crate::math_ops::{
    compute_g_alloc, DeygoutRadioOperator, GFieldKind, WorldPos, TACTICAL_CELL_SIZE_M,
};
use crate::state::WorldState;
use crate::types::UnitId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BUCKET_CELL_SPAN: usize = 8; // 8 cells * 8.0m = 64.0m spatial bucket

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GFieldCell {
    pub g0: f32,
    pub committed_en_route: f32,
    pub actual_on_site: f32,
    pub kind: GFieldKind,
}

impl GFieldCell {
    #[inline]
    pub fn residual_alloc(&self) -> f32 {
        compute_g_alloc(self.g0, self.actual_on_site, self.committed_en_route)
    }
}

/// Rebuildable derived views & spatial indexes (NOT serialized into saves).
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedState {
    pub synced_terrain_rev: u64,
    pub synced_unit_rev: u64,
    pub bucket_cells: usize,
    pub bucket_rows: usize,
    pub bucket_cols: usize,
    pub spatial_buckets: Vec<Vec<UnitId>>,
    pub g_field_cells: BTreeMap<usize, GFieldCell>,
    pub i_field_threat: Vec<f32>,
    pub unit_radio_t_diff: BTreeMap<u32, f32>,
}

impl DerivedState {
    pub fn new(rows: usize, cols: usize) -> Self {
        let bucket_cells = BUCKET_CELL_SPAN;
        let bucket_rows = rows.div_ceil(bucket_cells).max(1);
        let bucket_cols = cols.div_ceil(bucket_cells).max(1);
        Self {
            synced_terrain_rev: u64::MAX,
            synced_unit_rev: u64::MAX,
            bucket_cells,
            bucket_rows,
            bucket_cols,
            spatial_buckets: vec![Vec::new(); bucket_rows * bucket_cols],
            g_field_cells: BTreeMap::new(),
            i_field_threat: vec![0.0; rows * cols],
            unit_radio_t_diff: BTreeMap::new(),
        }
    }

    pub fn invalidate(&mut self) {
        self.synced_terrain_rev = u64::MAX;
        self.synced_unit_rev = u64::MAX;
        for bucket in &mut self.spatial_buckets {
            bucket.clear();
        }
        self.g_field_cells.clear();
        self.i_field_threat.clear();
        self.unit_radio_t_diff.clear();
    }

    #[inline]
    pub fn needs_rebuild(&self, world: &WorldState) -> bool {
        let expected_cells = world.terrain.rows * world.terrain.cols;
        self.synced_terrain_rev != world.terrain.revision
            || self.synced_unit_rev != world.unit_revision
            || self.i_field_threat.len() != expected_cells
    }

    /// Rebuilds all derived spatial buckets, G-field baseline, I-field baseline, and Deygout factors
    /// solely from `WorldState`. Returns `true` if a rebuild ran.
    pub fn rebuild_if_needed(&mut self, world: &WorldState) -> bool {
        if !self.needs_rebuild(world) {
            return false;
        }

        let rows = world.terrain.rows;
        let cols = world.terrain.cols;
        self.bucket_cells = BUCKET_CELL_SPAN;
        self.bucket_rows = rows.div_ceil(self.bucket_cells).max(1);
        self.bucket_cols = cols.div_ceil(self.bucket_cells).max(1);

        let b_total = self.bucket_rows * self.bucket_cols;
        self.spatial_buckets.clear();
        self.spatial_buckets.resize_with(b_total, Vec::new);

        self.g_field_cells.clear();
        self.unit_radio_t_diff.clear();

        let n_cells = rows * cols;
        self.i_field_threat.resize(n_cells, 0.0);
        for i in 0..n_cells {
            let h = world.cell_h_blast.get(i).copied().unwrap_or(0.0);
            let s = world.cell_s_supp.get(i).copied().unwrap_or(0.0);
            self.i_field_threat[i] = h + s;
        }

        for obj in &world.objectives {
            if let Ok(cell) = world.terrain.geom.world_to_cell(obj.center) {
                if let Ok(idx) = world.terrain.idx(cell) {
                    self.g_field_cells.insert(
                        idx,
                        GFieldCell {
                            g0: 1.0,
                            committed_en_route: 0.0,
                            actual_on_site: 0.0,
                            kind: GFieldKind::State,
                        },
                    );
                }
            }
        }

        for (&uid, unit) in &world.units {
            if !unit.alive {
                continue;
            }
            if let Ok(cell) = world.terrain.geom.world_to_cell(unit.pos) {
                let br = (cell.r as usize / self.bucket_cells).min(self.bucket_rows - 1);
                let bc = (cell.c as usize / self.bucket_cells).min(self.bucket_cols - 1);
                self.spatial_buckets[br * self.bucket_cols + bc].push(unit.id);

                if let Ok(cidx) = world.terrain.idx(cell) {
                    let g_cell = self.g_field_cells.entry(cidx).or_insert(GFieldCell {
                        g0: 1.0,
                        committed_en_route: 0.0,
                        actual_on_site: 0.0,
                        kind: GFieldKind::State,
                    });
                    g_cell.actual_on_site += 1.0;
                }
            }

            let t_diff = compute_unit_deygout_factor(world, unit.id);
            self.unit_radio_t_diff.insert(uid, t_diff);
        }

        for (&cidx, &rem) in &world.g_work_remaining {
            if cidx < n_cells {
                self.g_field_cells.insert(
                    cidx,
                    GFieldCell {
                        g0: rem,
                        committed_en_route: 0.0,
                        actual_on_site: 0.0,
                        kind: GFieldKind::Work,
                    },
                );
            }
        }

        self.synced_terrain_rev = world.terrain.revision;
        self.synced_unit_rev = world.unit_revision;
        true
    }

    pub fn query_units_in_radius(
        &self,
        world: &WorldState,
        center: WorldPos,
        radius_m: f32,
    ) -> Vec<UnitId> {
        if radius_m < 0.0 || self.spatial_buckets.is_empty() {
            return Vec::new();
        }
        let bucket_m = (self.bucket_cells as f32) * TACTICAL_CELL_SIZE_M;
        let ox = world.terrain.geom.origin_m[0];
        let oy = world.terrain.geom.origin_m[1];

        let min_bc = (((center.x - radius_m - ox) / bucket_m).floor() as isize)
            .clamp(0, self.bucket_cols as isize - 1) as usize;
        let max_bc = (((center.x + radius_m - ox) / bucket_m).floor() as isize)
            .clamp(0, self.bucket_cols as isize - 1) as usize;
        let min_br = (((center.y - radius_m - oy) / bucket_m).floor() as isize)
            .clamp(0, self.bucket_rows as isize - 1) as usize;
        let max_br = (((center.y + radius_m - oy) / bucket_m).floor() as isize)
            .clamp(0, self.bucket_rows as isize - 1) as usize;

        let mut found = Vec::new();
        for br in min_br..=max_br {
            for bc in min_bc..=max_bc {
                for &uid in &self.spatial_buckets[br * self.bucket_cols + bc] {
                    if let Some(u) = world.units.get(&uid.0) {
                        if u.alive && u.pos.distance_to(center) <= radius_m {
                            found.push(uid);
                        }
                    }
                }
            }
        }
        found.sort();
        found.dedup();
        found
    }
}

fn compute_unit_deygout_factor(world: &WorldState, uid: UnitId) -> f32 {
    if !world.ruleset.enforce_radio_deygout {
        return 1.0;
    }
    let Some(unit) = world.units.get(&uid.0) else {
        return 1.0;
    };
    let Some(pid) = unit.platoon_id else {
        return 1.0;
    };
    let Some(hq_uid) = world.platoons.get(&pid.0).and_then(|p| p.hq_unit) else {
        return 1.0;
    };
    if hq_uid == uid {
        return 1.0;
    }
    let Some(hq) = world.units.get(&hq_uid.0).filter(|h| h.alive) else {
        return 1.0;
    };
    let dist_m = hq.pos.distance_to(unit.pos);
    if dist_m <= TACTICAL_CELL_SIZE_M {
        return 1.0;
    }

    let mut profile = [(0.0f32, 0.0f32); 5];
    for (step, slot) in profile.iter_mut().enumerate() {
        let t = (step as f32) * 0.25;
        let p = WorldPos::new(
            hq.pos.x + (unit.pos.x - hq.pos.x) * t,
            hq.pos.y + (unit.pos.y - hq.pos.y) * t,
        );
        let elev = world.terrain.elevation_at(p).unwrap_or(0.0);
        *slot = (dist_m * t, elev);
    }

    let Some(k_idx) = DeygoutRadioOperator::find_dominant_knife_edge(&profile) else {
        return 1.0;
    };
    let (s_k, z_k) = profile[k_idx];
    let (s_a, h_a) = profile[0];
    let (s_b, h_b) = profile[4];
    let chord_h = h_a + (h_b - h_a) * (s_k / (s_b - s_a).max(1.0));
    let h_diff = z_k - chord_h;
    if h_diff <= 0.0 {
        return 1.0;
    }
    // Scale diffraction parameter nu smoothly by ridge clearance over Fresnel zone (~25m scale)
    let nu = h_diff / 25.0;
    DeygoutRadioOperator::rational_diffraction_transmission(nu)
}
