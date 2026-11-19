use super::*;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone)]
pub struct WorldGrid {
    pub rows: usize,
    pub cols: usize,
    pub cell_m: f32,
    pub origin_latlon: [f32; 2],
    pub elev_min_m: f32,
    pub elev_max_m: f32,

    pub raw_u16: Vec<u16>,
    pub elevation_m: Vec<f32>,
    pub grad_x: Vec<f32>,
    pub grad_y: Vec<f32>,
    pub slope_deg: Vec<f32>,

    pub terrain: Vec<u8>,
    pub base_terrain: Vec<u8>,
    pub feature_flags: Vec<u32>,
    pub forest_density: Vec<f32>,
    pub weather: Vec<u8>,
    pub terrain_attrition_day: Vec<f32>,

    pub visibility_mod: Vec<f32>,
    pub smoke_ttl_h: Vec<f32>,
    pub illum_ttl_h: Vec<f32>,
    pub exposure: Vec<f32>,
    pub smoke_active: Vec<u32>,
    pub illum_active: Vec<u32>,
    pub exposure_active: Vec<u32>,

    // AO channels stored in pixel container per team
    pub ao_owner_red: Vec<u32>,
    pub ao_echelon_mask_red: Vec<u8>,
    pub ao_owner_blue: Vec<u32>,
    pub ao_echelon_mask_blue: Vec<u8>,

    // Frontline channels: dense bitmask for O(1) skip + sparse index entries
    pub frontlines: Vec<TypedFrontline>,
    pub frontline_id_map: FxHashMap<String, u32>,
    pub fl_presence_red: Vec<u8>,
    pub fl_presence_blue: Vec<u8>,
    pub fl_cell_entries: FxHashMap<u32, Vec<usize>>,

    // Sparse buckets keyed by flat pixel idx
    pub cell_mods: FxHashMap<u32, Vec<CellModEntry>>,
    pub cell_stash: FxHashMap<u32, CellStash>,
    pub ground_bodies: Vec<crate::units_ecs::GroundBody>,
    pub pixel_containers: FxHashMap<u32, PixelCellContainer>,
    // Field wires keyed by ordered pixel pair (u32, u32) -> bitmask (1=Red, 2=Blue)
    pub wire_edges: FxHashMap<(u32, u32), u8>,

    pub terrain_revision: u64,
    pub destroyed_bridges: FxHashSet<u32>,
    pub damaged_terrain: FxHashSet<u32>,
}

impl WorldGrid {
    pub fn new_plain(rows: usize, cols: usize, cell_m: f32) -> Self {
        let total = rows * cols;
        let mut g = Self {
            rows,
            cols,
            cell_m,
            origin_latlon: [34.25, 108.95],
            elev_min_m: 0.0,
            elev_max_m: 100.0,
            raw_u16: vec![0; total],
            elevation_m: vec![0.0; total],
            grad_x: vec![0.0; total],
            grad_y: vec![0.0; total],
            slope_deg: vec![0.0; total],
            terrain: vec![PLAIN; total],
            base_terrain: vec![PLAIN; total],
            feature_flags: vec![0; total],
            forest_density: vec![0.0; total],
            weather: vec![CLEAR; total],
            terrain_attrition_day: vec![0.0; total],
            visibility_mod: vec![1.0; total],
            smoke_ttl_h: vec![0.0; total],
            illum_ttl_h: vec![0.0; total],
            exposure: vec![0.0; total],
            smoke_active: Vec::new(),
            illum_active: Vec::new(),
            exposure_active: Vec::new(),
            ao_owner_red: vec![0; total],
            ao_echelon_mask_red: vec![0; total],
            ao_owner_blue: vec![0; total],
            ao_echelon_mask_blue: vec![0; total],
            frontlines: Vec::new(),
            frontline_id_map: FxHashMap::default(),
            fl_presence_red: vec![0; total],
            fl_presence_blue: vec![0; total],
            fl_cell_entries: FxHashMap::default(),
            cell_mods: FxHashMap::default(),
            cell_stash: FxHashMap::default(),
            ground_bodies: Vec::new(),
            pixel_containers: FxHashMap::default(),
            wire_edges: FxHashMap::default(),
            terrain_revision: 1,
            destroyed_bridges: FxHashSet::default(),
            damaged_terrain: FxHashSet::default(),
        };
        g.refresh_attrition();
        g
    }

    #[inline]
    pub fn inject_shot_packet(&mut self, packet: ShotPacket) {
        if packet.shots < 1 || !self.in_bounds(packet.target_px.0, packet.target_px.1) {
            return;
        }
        let gi = self.idx(packet.target_px.0, packet.target_px.1) as u32;
        self.pixel_containers.entry(gi).or_default().incoming_packets.push(packet);
    }

    #[inline]
    pub fn get_pixel_container(&self, r: i32, c: i32) -> Option<&PixelCellContainer> {
        if !self.in_bounds(r, c) {
            return None;
        }
        let gi = self.idx(r, c) as u32;
        self.pixel_containers.get(&gi)
    }

    #[inline]
    pub fn get_pixel_container_mut(&mut self, r: i32, c: i32) -> Option<&mut PixelCellContainer> {
        if !self.in_bounds(r, c) {
            return None;
        }
        let gi = self.idx(r, c) as u32;
        Some(self.pixel_containers.entry(gi).or_default())
    }

    pub fn drain_pixel_packets(&mut self) -> FxHashMap<u32, Vec<ShotPacket>> {
        let mut drained = FxHashMap::default();
        for (&idx, container) in self.pixel_containers.iter_mut() {
            if !container.incoming_packets.is_empty() {
                drained.insert(idx, std::mem::take(&mut container.incoming_packets));
            }
        }
        drained
    }

    pub fn clear_pixel_packets(&mut self) {
        for container in self.pixel_containers.values_mut() {
            container.incoming_packets.clear();
        }
    }

    #[inline]
    pub fn set_forest_density(&mut self, r: i32, c: i32, density: f32) {
        if !self.in_bounds(r, c) {
            return;
        }
        let gi = self.idx(r, c);
        let d = density.clamp(0.0, 1.0);
        self.forest_density[gi] = d;
        if d > 0.0 && self.terrain[gi] == PLAIN {
            self.terrain[gi] = FOREST;
            self.feature_flags[gi] |= FeatureFlags::FOREST.bits();
        }
    }

    #[inline]
    pub fn forest_density_at(&self, r: i32, c: i32) -> f32 {
        if !self.in_bounds(r, c) {
            return 0.0;
        }
        let gi = self.idx(r, c);
        let d = self.forest_density[gi];
        if d > 0.0 {
            d
        } else if self.terrain[gi] == FOREST {
            0.5
        } else {
            0.0
        }
    }



    pub fn sync_feature_flags_from_terrain(&mut self) {
        let terrain = &self.terrain;
        self.feature_flags
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, fl)| {
                let mut f = FeatureFlags::from_bits_truncate(*fl);
                f.remove(
                    FeatureFlags::FOREST
                        | FeatureFlags::MOUNTAIN
                        | FeatureFlags::SWAMP
                        | FeatureFlags::WATER
                        | FeatureFlags::URBAN
                        | FeatureFlags::DIRT_ROAD
                        | FeatureFlags::HIGHWAY
                        | FeatureFlags::RAIL
                        | FeatureFlags::BRIDGE
                        | FeatureFlags::AIRFIELD
                        | FeatureFlags::CQB_AMBUSH,
                );
                match terrain[i] {
                    FOREST => f.insert(FeatureFlags::FOREST | FeatureFlags::CQB_AMBUSH),
                    MOUNTAIN => f.insert(FeatureFlags::MOUNTAIN),
                    SWAMP => f.insert(FeatureFlags::SWAMP),
                    WATER => f.insert(FeatureFlags::WATER),
                    URBAN => f.insert(FeatureFlags::URBAN | FeatureFlags::CQB_AMBUSH),
                    DIRT_ROAD => f.insert(FeatureFlags::DIRT_ROAD),
                    HIGHWAY => f.insert(FeatureFlags::HIGHWAY),
                    RAIL => f.insert(FeatureFlags::RAIL),
                    BRIDGE => f.insert(FeatureFlags::BRIDGE),
                    AIRFIELD => f.insert(FeatureFlags::AIRFIELD),
                    _ => {}
                }
                *fl = f.bits();
            });
    }

    pub fn recompute_slope_horn(&mut self) {
        let rows = self.rows;
        let cols = self.cols;
        let cell_m = self.cell_m.max(1.0);
        let elev = &self.elevation_m;
        let inv8 = 1.0 / (8.0 * cell_m);

        let mut gx = vec![0.0f32; rows * cols];
        let mut gy = vec![0.0f32; rows * cols];
        let mut sd = vec![0.0f32; rows * cols];

        gx.par_chunks_mut(cols)
            .zip(gy.par_chunks_mut(cols))
            .zip(sd.par_chunks_mut(cols))
            .enumerate()
            .for_each(|(r, ((gx_row, gy_row), sd_row))| {
                let rm = if r == 0 { 0 } else { r - 1 };
                let rp = if r + 1 >= rows { rows - 1 } else { r + 1 };
                for c in 0..cols {
                    let cm = if c == 0 { 0 } else { c - 1 };
                    let cp = if c + 1 >= cols { cols - 1 } else { c + 1 };
                    let z00 = elev[rm * cols + cm];
                    let z01 = elev[rm * cols + c];
                    let z02 = elev[rm * cols + cp];
                    let z10 = elev[r * cols + cm];
                    let z12 = elev[r * cols + cp];
                    let z20 = elev[rp * cols + cm];
                    let z21 = elev[rp * cols + c];
                    let z22 = elev[rp * cols + cp];

                    let dx = ((z02 + 2.0 * z12 + z22) - (z00 + 2.0 * z10 + z20)) * inv8;
                    let dy = ((z20 + 2.0 * z21 + z22) - (z00 + 2.0 * z01 + z02)) * inv8;
                    gx_row[c] = dx;
                    gy_row[c] = dy;
                    sd_row[c] = dx.hypot(dy).atan().to_degrees();
                }
            });

        self.grad_x = gx;
        self.grad_y = gy;
        self.slope_deg = sd;
    }

    pub fn refresh_attrition(&mut self) {
        let terrain = &self.terrain;
        let weather = &self.weather;
        self.terrain_attrition_day
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, a)| {
                *a = base_attrition_day(terrain[i], weather[i]);
            });
    }

    #[inline]
    pub fn in_bounds(&self, r: i32, c: i32) -> bool {
        r >= 0 && (r as usize) < self.rows && c >= 0 && (c as usize) < self.cols
    }

    #[inline]
    pub fn idx(&self, r: i32, c: i32) -> usize {
        (r as usize) * self.cols + (c as usize)
    }

    #[inline]
    pub fn clamp_rc(&self, r: i32, c: i32) -> (i32, i32) {
        (
            r.clamp(0, (self.rows as i32) - 1),
            c.clamp(0, (self.cols as i32) - 1),
        )
    }

    #[inline]
    pub fn clamp_pos(&self, pos: [f32; 2]) -> [f32; 2] {
        [
            pos[0].clamp(0.0, (self.rows as f32 - 1.0).max(0.0)),
            pos[1].clamp(0.0, (self.cols as f32 - 1.0).max(0.0)),
        ]
    }

    #[inline]
    pub fn distance_km(&self, a: [f32; 2], b: [f32; 2]) -> f32 {
        (a[0] - b[0]).hypot(a[1] - b[1]) * self.cell_m / 1000.0
    }

    #[inline]
    pub fn is_literal_ground_px(&self, r: i32, c: i32) -> bool {
        if !self.in_bounds(r, c) {
            return false;
        }
        let i = self.idx(r, c);
        let t = self.terrain[i];
        if t == WATER || t == BRIDGE {
            return false;
        }
        let fl = FeatureFlags::from_bits_truncate(self.feature_flags[i]);
        if fl.intersects(FeatureFlags::WATER | FeatureFlags::BRIDGE) {
            return false;
        }
        true
    }

    pub fn find_nearest_literal_ground_px(&self, r: i32, c: i32) -> (i32, i32) {
        let (cr, cc) = self.clamp_rc(r, c);
        if self.is_literal_ground_px(cr, cc) {
            return (cr, cc);
        }
        #[derive(Copy, Clone, Eq, PartialEq)]
        struct Node {
            d2: i64,
            r: i32,
            c: i32,
        }
        impl Ord for Node {
            fn cmp(&self, o: &Self) -> Ordering {
                o.d2.cmp(&self.d2).then_with(|| o.r.cmp(&self.r)).then_with(|| o.c.cmp(&self.c))
            }
        }
        impl PartialOrd for Node {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        let mut heap = BinaryHeap::new();
        let mut seen: FxHashSet<(i32, i32)> = FxHashSet::default();
        heap.push(Node { d2: 0, r: cr, c: cc });
        seen.insert((cr, cc));
        while let Some(cur) = heap.pop() {
            if self.is_literal_ground_px(cur.r, cur.c) {
                return (cur.r, cur.c);
            }
            if cur.d2 > 64 * 64 {
                break;
            }
            for dr in -1..=1 {
                for dc in -1..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = cur.r + dr;
                    let nc = cur.c + dc;
                    if self.in_bounds(nr, nc) && seen.insert((nr, nc)) {
                        let d2 = ((nr - cr) as i64).pow(2) + ((nc - cc) as i64).pow(2);
                        heap.push(Node { d2, r: nr, c: nc });
                    }
                }
            }
        }
        (cr, cc)
    }
}
