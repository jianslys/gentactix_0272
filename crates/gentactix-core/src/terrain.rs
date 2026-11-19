use crate::math_ops::{
    CellCoord, GeometryId, GridGeometry, OutsideBoundsError, WorldPos, TACTICAL_CELL_SIZE_M,
};
use crate::types::MobilityKind;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub const BINARY_TERRAIN_MAGIC: &[u8; 8] = b"GTXV2TER";
pub const BINARY_TERRAIN_VERSION: u32 = 1;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainKind {
    #[default]
    Plain = 0,
    Forest = 1,
    Urban = 2,
    Water = 3,
    Swamp = 4,
    Mountain = 5,
    Road = 6,
    Bridge = 7,
}

impl TerrainKind {
    #[inline]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Plain,
            1 => Self::Forest,
            2 => Self::Urban,
            3 => Self::Water,
            4 => Self::Swamp,
            5 => Self::Mountain,
            6 => Self::Road,
            7 => Self::Bridge,
            _ => Self::Plain,
        }
    }

    #[inline]
    pub const fn cover_factor(self) -> f32 {
        match self {
            Self::Plain => 0.10,
            Self::Forest => 0.45,
            Self::Urban => 0.65,
            Self::Water => 0.00,
            Self::Swamp => 0.15,
            Self::Mountain => 0.35,
            Self::Road => 0.05,
            Self::Bridge => 0.05,
        }
    }

    #[inline]
    pub const fn los_opacity(self) -> f32 {
        match self {
            Self::Plain | Self::Water | Self::Road | Self::Bridge => 0.00,
            Self::Swamp => 0.05,
            Self::Forest => 0.35,
            Self::Mountain => 0.45,
            Self::Urban => 0.65,
        }
    }

    #[inline]
    pub const fn move_cost(self, mob: MobilityKind) -> Option<f32> {
        match self {
            Self::Water => None,
            Self::Bridge => Some(1.0),
            Self::Road => match mob {
                MobilityKind::Wheeled => Some(0.65),
                MobilityKind::Tracked => Some(0.75),
                MobilityKind::Foot => Some(0.80),
            },
            Self::Plain => Some(1.0),
            Self::Forest => match mob {
                MobilityKind::Foot => Some(1.25),
                MobilityKind::Tracked => Some(1.60),
                MobilityKind::Wheeled => Some(2.40),
            },
            Self::Urban => match mob {
                MobilityKind::Foot => Some(1.15),
                MobilityKind::Wheeled | MobilityKind::Tracked => Some(1.30),
            },
            Self::Swamp => match mob {
                MobilityKind::Foot => Some(2.0),
                MobilityKind::Tracked => Some(2.60),
                MobilityKind::Wheeled => None,
            },
            Self::Mountain => match mob {
                MobilityKind::Foot => Some(2.20),
                MobilityKind::Tracked => Some(3.20),
                MobilityKind::Wheeled => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyHaloBox {
    pub min_r: i32,
    pub max_r: i32,
    pub min_c: i32,
    pub max_c: i32,
}

impl DirtyHaloBox {
    #[inline]
    pub fn from_cell(r: i32, c: i32) -> Self {
        Self {
            min_r: r,
            max_r: r,
            min_c: c,
            max_c: c,
        }
    }

    pub fn expand_with_halo(&mut self, r: i32, c: i32, halo: i32, rows: usize, cols: usize) {
        let max_row = (rows as i32).saturating_sub(1);
        let max_col = (cols as i32).saturating_sub(1);
        self.min_r = self.min_r.min((r - halo).clamp(0, max_row));
        self.max_r = self.max_r.max((r + halo).clamp(0, max_row));
        self.min_c = self.min_c.min((c - halo).clamp(0, max_col));
        self.max_c = self.max_c.max((c + halo).clamp(0, max_col));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainGrid {
    pub geom: GridGeometry,
    pub rows: usize,
    pub cols: usize,
    pub elevation_m: Vec<f32>,
    pub slope_deg: Vec<f32>,
    pub kind: Vec<TerrainKind>,
    pub cover_bonus: Vec<f32>,
    pub crater_depth_m: Vec<f32>,
    pub smoke_density: Vec<f32>,
    pub revision: u64,
    pub dirty_halo: Option<DirtyHaloBox>,
}

impl TerrainGrid {
    pub fn new_flat(rows: usize, cols: usize, base_elevation_m: f32) -> Self {
        assert!(rows > 0 && cols > 0, "TerrainGrid dimensions must be non-zero");
        let geom = GridGeometry::new(GeometryId(1), [0.0, 0.0], rows as u32, cols as u32)
            .expect("valid tactical 8m grid geometry");
        debug_assert!((geom.cell_size_m() - TACTICAL_CELL_SIZE_M).abs() < f32::EPSILON);
        let total = rows * cols;
        Self {
            geom,
            rows,
            cols,
            elevation_m: vec![base_elevation_m; total],
            slope_deg: vec![0.0; total],
            kind: vec![TerrainKind::Plain; total],
            cover_bonus: vec![0.0; total],
            crater_depth_m: vec![0.0; total],
            smoke_density: vec![0.0; total],
            revision: 1,
            dirty_halo: None,
        }
    }

    pub fn from_parts(
        rows: usize,
        cols: usize,
        elevation_m: Vec<f32>,
        kind: Vec<TerrainKind>,
    ) -> Result<Self, String> {
        if rows == 0 || cols == 0 {
            return Err("terrain dimensions must be non-zero".into());
        }
        let total = rows * cols;
        if elevation_m.len() != total {
            return Err(format!(
                "elevation_m length {} does not match {}x{} ({})",
                elevation_m.len(),
                rows,
                cols,
                total
            ));
        }
        if !kind.is_empty() && kind.len() != total {
            return Err(format!(
                "kind length {} does not match {}x{} ({})",
                kind.len(),
                rows,
                cols,
                total
            ));
        }
        let mut grid = Self::new_flat(rows, cols, 0.0);
        grid.elevation_m = elevation_m;
        if !kind.is_empty() {
            grid.kind = kind;
        }
        grid.recompute_slope_horn();
        Ok(grid)
    }

    #[inline]
    pub fn idx(&self, coord: CellCoord) -> Result<usize, OutsideBoundsError> {
        if !self.geom.in_bounds(coord.r, coord.c) {
            return Err(OutsideBoundsError::InvalidCellIndex {
                r: coord.r,
                c: coord.c,
                rows: self.geom.rows,
                cols: self.geom.cols,
            });
        }
        Ok((coord.r as usize) * self.cols + (coord.c as usize))
    }

    #[inline]
    pub fn elevation_at(&self, pos: WorldPos) -> Result<f32, OutsideBoundsError> {
        let cell = self.geom.world_to_cell(pos)?;
        let idx = self.idx(cell)?;
        Ok(self.elevation_m[idx])
    }

    pub fn mutate_cell(
        &mut self,
        coord: CellCoord,
        elev_delta: Option<f32>,
        kind: Option<TerrainKind>,
        cover_add: Option<f32>,
        smoke_add: Option<f32>,
    ) -> Result<(), OutsideBoundsError> {
        let i = self.idx(coord)?;
        if let Some(d) = elev_delta {
            self.elevation_m[i] += d;
            if d < 0.0 {
                self.crater_depth_m[i] += -d;
            }
        }
        if let Some(k) = kind {
            self.kind[i] = k;
        }
        if let Some(cov) = cover_add {
            self.cover_bonus[i] = (self.cover_bonus[i] + cov).clamp(0.0, 1.0);
        }
        if let Some(smk) = smoke_add {
            self.smoke_density[i] = (self.smoke_density[i] + smk).clamp(0.0, 1.0);
        }

        let mut halo_box = self
            .dirty_halo
            .unwrap_or_else(|| DirtyHaloBox::from_cell(coord.r, coord.c));
        halo_box.expand_with_halo(coord.r, coord.c, 2, self.rows, self.cols);
        self.dirty_halo = Some(halo_box);

        let local_box = DirtyHaloBox {
            min_r: (coord.r - 1).max(0),
            max_r: (coord.r + 1).min(self.rows as i32 - 1),
            min_c: (coord.c - 1).max(0),
            max_c: (coord.c + 1).min(self.cols as i32 - 1),
        };
        self.recompute_slope_box(local_box);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    #[inline]
    pub fn clear_dirty_halo(&mut self) {
        self.dirty_halo = None;
    }

    #[inline]
    fn horn_slope_at(&self, r: usize, c: usize, inv8: f32) -> f32 {
        let rm = r.saturating_sub(1);
        let rp = (r + 1).min(self.rows - 1);
        let cm = c.saturating_sub(1);
        let cp = (c + 1).min(self.cols - 1);
        let elev = &self.elevation_m;
        let cols = self.cols;
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
        dx.hypot(dy).atan().to_degrees()
    }

    pub fn recompute_slope_box(&mut self, b: DirtyHaloBox) {
        let inv8 = 1.0 / (8.0 * TACTICAL_CELL_SIZE_M);
        let r0 = b.min_r.clamp(0, self.rows as i32 - 1) as usize;
        let r1 = b.max_r.clamp(0, self.rows as i32 - 1) as usize;
        let c0 = b.min_c.clamp(0, self.cols as i32 - 1) as usize;
        let c1 = b.max_c.clamp(0, self.cols as i32 - 1) as usize;
        for r in r0..=r1 {
            for c in c0..=c1 {
                let s = self.horn_slope_at(r, c, inv8);
                self.slope_deg[r * self.cols + c] = s;
            }
        }
    }

    pub fn recompute_slope_horn(&mut self) {
        let rows = self.rows;
        let cols = self.cols;
        let inv8 = 1.0 / (8.0 * TACTICAL_CELL_SIZE_M);
        let elev = &self.elevation_m;
        let mut sd = vec![0.0f32; rows * cols];
        sd.par_chunks_mut(cols).enumerate().for_each(|(r, row)| {
            let rm = r.saturating_sub(1);
            let rp = (r + 1).min(rows - 1);
            for c in 0..cols {
                let cm = c.saturating_sub(1);
                let cp = (c + 1).min(cols - 1);
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
                row[c] = dx.hypot(dy).atan().to_degrees();
            }
        });
        self.slope_deg = sd;
    }

    pub fn from_r16_bytes(
        raw_bytes: &[u8],
        width: Option<usize>,
        height: Option<usize>,
        low_m: f32,
        high_m: f32,
        flip_y: bool,
    ) -> Result<Self, String> {
        if raw_bytes.len() % 2 != 0 {
            return Err("raw .r16 byte length must be even".into());
        }
        let total_px = raw_bytes.len() / 2;
        if total_px == 0 {
            return Err("raw .r16 buffer cannot be empty".into());
        }
        let (rows, cols) = match (width, height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (h, w),
            (Some(w), None) if w > 0 => (total_px / w, w),
            (None, Some(h)) if h > 0 => (h, total_px / h),
            (None, None) => {
                let side = (total_px as f64).sqrt().round() as usize;
                if side * side != total_px {
                    return Err(format!("non-square raw r16 pixel count {total_px}"));
                }
                (side, side)
            }
            _ => return Err("width and height must be positive".into()),
        };
        if rows * cols != total_px {
            return Err(format!("r16 dimensions {rows}x{cols} != {total_px} pixels"));
        }

        let mut raw_u16 = vec![0u16; total_px];
        for r in 0..rows {
            let dst_r = if flip_y { rows - 1 - r } else { r };
            let src_off = r * cols * 2;
            let dst_off = dst_r * cols;
            for c in 0..cols {
                raw_u16[dst_off + c] = u16::from_le_bytes([raw_bytes[src_off + c * 2], raw_bytes[src_off + c * 2 + 1]]);
            }
        }

        let (min_u, max_u) = raw_u16.iter().copied().fold((u16::MAX, 0u16), |(mn, mx), v| {
            if v < 65535 {
                (mn.min(v), mx.max(v))
            } else {
                (mn, mx)
            }
        });
        let (min_f, span_f) = if min_u == u16::MAX {
            (65535.0f32, 1.0f32)
        } else {
            (min_u as f32, max_u.saturating_sub(min_u).max(1) as f32)
        };
        let m_span = high_m - low_m;

        let elevation_m: Vec<f32> = raw_u16
            .par_iter()
            .map(|&v| low_m + ((v as f32 - min_f) / span_f).clamp(0.0, 1.0) * m_span)
            .collect();

        let mut grid = Self::new_flat(rows, cols, low_m);
        grid.elevation_m = elevation_m;
        grid.recompute_slope_horn();
        Ok(grid)
    }

    pub fn from_r16_files(
        r16_path: &Path,
        diff_path: Option<&Path>,
        riv_path: Option<&Path>,
        flow_threshold: u16,
        low_m: f32,
        high_m: f32,
    ) -> Result<Self, String> {
        let raw_bytes = std::fs::read(r16_path).map_err(|e| format!("read r16 error: {e}"))?;
        let mut grid = Self::from_r16_bytes(&raw_bytes, None, None, low_m, high_m, false)?;
        let (rows, cols) = (grid.rows, grid.cols);
        let diff_rgb = diff_path
            .filter(|p| p.exists())
            .map(|p| decode_rgb_png(p, rows, cols))
            .transpose()?;
        let riv_u16 = riv_path
            .filter(|p| p.exists())
            .map(|p| decode_river_png(p, rows, cols))
            .transpose()?;

        let m_span = (high_m - low_m).abs().max(1.0);
        grid.kind.par_iter_mut().enumerate().for_each(|(i, slot)| {
            let norm_h = ((grid.elevation_m[i] - low_m) / m_span).clamp(0.0, 1.0);
            let riv = riv_u16.as_ref().map_or(0, |v| v[i]);
            let (r_c, g_c, b_c) = diff_rgb
                .as_ref()
                .map_or((100i16, 100i16, 100i16), |d| (d[i * 3] as i16, d[i * 3 + 1] as i16, d[i * 3 + 2] as i16));

            let mut tk = TerrainKind::Plain;
            if diff_rgb.is_some() {
                if (g_c - r_c >= 7) && (g_c >= 95) && (b_c < 85) {
                    tk = TerrainKind::Forest;
                }
                if (b_c >= 68) && (g_c >= r_c) && (norm_h < 0.55) && (riv <= flow_threshold) {
                    tk = TerrainKind::Swamp;
                }
                if norm_h >= 0.72 || (r_c + g_c + b_c) < 255 {
                    tk = TerrainKind::Mountain;
                }
                if riv > flow_threshold || (b_c > r_c + 8 && b_c > g_c - 10) {
                    tk = TerrainKind::Water;
                }
            } else {
                if norm_h >= 0.72 {
                    tk = TerrainKind::Mountain;
                }
                if riv > flow_threshold {
                    tk = TerrainKind::Water;
                }
            }
            *slot = tk;
        });
        grid.revision = grid.revision.wrapping_add(1);
        Ok(grid)
    }

    pub fn to_binary_stream(&self) -> Vec<u8> {
        let n = self.rows * self.cols;
        let mut out = Vec::with_capacity(24 + n * 7);
        out.extend_from_slice(BINARY_TERRAIN_MAGIC);
        out.extend_from_slice(&BINARY_TERRAIN_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.rows as u32).to_le_bytes());
        out.extend_from_slice(&(self.cols as u32).to_le_bytes());
        out.extend_from_slice(&TACTICAL_CELL_SIZE_M.to_le_bytes());
        for &e in &self.elevation_m {
            out.extend_from_slice(&e.to_le_bytes());
        }
        for &k in &self.kind {
            out.push(k as u8);
        }
        for &cov in &self.cover_bonus {
            out.push((cov.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        for &smk in &self.smoke_density {
            out.push((smk.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }
}

fn decode_rgb_png(path: &Path, rows: usize, cols: usize) -> Result<Vec<u8>, String> {
    let dec = png::Decoder::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?));
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.width as usize != cols || info.height as usize != rows {
        return Err(format!("diffuse PNG {}x{} != {}x{}", info.height, info.width, rows, cols));
    }
    let bpp = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        _ => return Err("unsupported diffuse PNG color type".into()),
    };
    let total = rows * cols;
    let mut out = vec![0u8; total * 3];
    for i in 0..total {
        let s = i * bpp;
        if bpp == 1 {
            out[i * 3] = buf[s];
            out[i * 3 + 1] = buf[s];
            out[i * 3 + 2] = buf[s];
        } else {
            out[i * 3] = buf[s];
            out[i * 3 + 1] = buf[s + 1];
            out[i * 3 + 2] = buf[s + 2];
        }
    }
    Ok(out)
}

fn decode_river_png(path: &Path, rows: usize, cols: usize) -> Result<Vec<u16>, String> {
    let dec = png::Decoder::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?));
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.width as usize != cols || info.height as usize != rows {
        return Err(format!("river PNG {}x{} != {}x{}", info.height, info.width, rows, cols));
    }
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return Err("unsupported river PNG color type".into()),
    };
    let total = rows * cols;
    let mut out = vec![0u16; total];
    if info.bit_depth == png::BitDepth::Sixteen {
        for i in 0..total {
            let s = i * ch * 2;
            out[i] = u16::from_be_bytes([buf[s], buf[s + 1]]);
        }
    } else {
        for i in 0..total {
            out[i] = buf[i * ch] as u16;
        }
    }
    Ok(out)
}
