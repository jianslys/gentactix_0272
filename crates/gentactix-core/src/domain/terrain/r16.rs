use super::*;
use rayon::prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

impl WorldGrid {
    pub fn synthetic_grid(rows: usize, cols: usize, cell_m: f32) -> Self {
        let mut g = Self::new_plain(rows, cols, cell_m);
        let river_col = cols / 2;
        let bridge_row = rows / 2;
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                let rf = r as f32;
                let cf = c as f32;
                let hill = 180.0 * ((-((rf - rows as f32 * 0.3) / 6.0).powi(2) - ((cf - cols as f32 * 0.25) / 6.0).powi(2)).exp())
                    + 220.0 * ((-((rf - rows as f32 * 0.72) / 5.0).powi(2) - ((cf - cols as f32 * 0.75) / 5.0).powi(2)).exp());
                g.elevation_m[i] = hill;
                g.raw_u16[i] = (hill * 20.0).clamp(0.0, 65000.0) as u16;

                if c == river_col {
                    g.terrain[i] = if r == bridge_row { BRIDGE } else { WATER };
                } else if r == bridge_row && c >= river_col.saturating_sub(8) && c <= (river_col + 8).min(cols - 1) {
                    g.terrain[i] = HIGHWAY;
                } else if r > rows / 6 && r < rows / 2 && c > cols / 8 && c < cols / 3 {
                    g.terrain[i] = FOREST;
                } else if hill >= 160.0 {
                    g.terrain[i] = MOUNTAIN;
                }
            }
        }
        g.base_terrain.copy_from_slice(&g.terrain);
        g.sync_feature_flags_from_terrain();
        g.recompute_slope_horn();
        g.refresh_attrition();
        g
    }

    pub fn from_r16_bytes(
        raw_bytes: &[u8],
        width: Option<usize>,
        height: Option<usize>,
        cell_m: f32,
        low_m: f32,
        high_m: f32,
        flip_y: bool,
    ) -> Result<Self, String> {
        if raw_bytes.len() % 2 != 0 {
            return Err("raw .r16 length must be even".into());
        }
        let total_px = raw_bytes.len() / 2;
        let (rows, cols) = match (width, height) {
            (Some(w), Some(h)) => (h, w),
            (Some(w), None) => (total_px / w, w),
            (None, Some(h)) => (h, total_px / h),
            (None, None) => {
                let side = (total_px as f64).sqrt().round() as usize;
                if side * side != total_px {
                    return Err(format!("non-square raw size {total_px}"));
                }
                (side, side)
            }
        };
        if rows * cols != total_px {
            return Err(format!("dimensions {rows}x{cols} != {total_px}"));
        }

        let mut raw_u16 = vec![0u16; total_px];
        for r in 0..rows {
            let dst_r = if flip_y { rows - 1 - r } else { r };
            let src_row_off = r * cols * 2;
            let dst_row_off = dst_r * cols;
            for c in 0..cols {
                let lo = raw_bytes[src_row_off + c * 2];
                let hi = raw_bytes[src_row_off + c * 2 + 1];
                raw_u16[dst_row_off + c] = u16::from_le_bytes([lo, hi]);
            }
        }

        let mut actual_min = u16::MAX;
        let mut actual_max = 0u16;
        for &v in &raw_u16 {
            if v < 65535 {
                if v < actual_min {
                    actual_min = v;
                }
                if v > actual_max {
                    actual_max = v;
                }
            }
        }
        if actual_min == u16::MAX {
            actual_min = 65535;
            actual_max = 65535;
        }
        let min_f = actual_min as f32;
        let span_f = ((actual_max.saturating_sub(actual_min)).max(1)) as f32;
        let m_span = high_m - low_m;

        let elevation_m: Vec<f32> = raw_u16
            .par_iter()
            .map(|&v| {
                let norm = ((v as f32 - min_f) / span_f).clamp(0.0, 1.0);
                low_m + norm * m_span
            })
            .collect();

        let mut g = Self::new_plain(rows, cols, cell_m);
        g.elev_min_m = low_m;
        g.elev_max_m = high_m;
        g.raw_u16 = raw_u16;
        g.elevation_m = elevation_m;
        g.recompute_slope_horn();
        g.refresh_attrition();
        Ok(g)
    }

    /// Directly loads .r16 + optional diffuse PNG + river PNG natively at 1:1 (no downsampling).
    pub fn from_r16_diffuse_river(
        r16_path: &Path,
        diff_path: Option<&Path>,
        riv_path: Option<&Path>,
        flow_threshold: u16,
    ) -> Result<Self, String> {
        let raw_bytes = std::fs::read(r16_path).map_err(|e| format!("read r16 error: {e}"))?;
        let mut g = Self::from_r16_bytes(&raw_bytes, None, None, 200.0, 120.0, 1480.0, false)?;
        let rows = g.rows;
        let cols = g.cols;
        let total = rows * cols;

        let diff_rgb_opt: Option<Vec<u8>> = if let Some(dp) = diff_path.filter(|p| p.exists()) {
            let dec = png::Decoder::new(BufReader::new(File::open(dp).map_err(|e| e.to_string())?));
            let mut reader = dec.read_info().map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; reader.output_buffer_size()];
            let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
            if info.width as usize != cols || info.height as usize != rows {
                return Err(format!("diffuse dimensions {}x{} != {}x{}", info.height, info.width, rows, cols));
            }
            let bpp = match info.color_type {
                png::ColorType::Rgb => 3,
                png::ColorType::Rgba => 4,
                png::ColorType::Grayscale => 1,
                _ => return Err("unsupported diffuse color type".into()),
            };
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
            Some(out)
        } else {
            None
        };

        let riv_u16_opt: Option<Vec<u16>> = if let Some(rp) = riv_path.filter(|p| p.exists()) {
            let dec = png::Decoder::new(BufReader::new(File::open(rp).map_err(|e| e.to_string())?));
            let mut reader = dec.read_info().map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; reader.output_buffer_size()];
            let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
            if info.width as usize != cols || info.height as usize != rows {
                return Err(format!("river dimensions {}x{} != {}x{}", info.height, info.width, rows, cols));
            }
            let ch = match info.color_type {
                png::ColorType::Grayscale => 1,
                png::ColorType::GrayscaleAlpha => 2,
                png::ColorType::Rgb => 3,
                png::ColorType::Rgba => 4,
                _ => return Err("unsupported river color type".into()),
            };
            let mut out = vec![0u16; total];
            if info.bit_depth == png::BitDepth::Sixteen {
                for i in 0..total {
                    let s = i * ch * 2;
                    out[i] = u16::from_be_bytes([buf[s], buf[s + 1]]);
                }
            } else {
                for i in 0..total {
                    let s = i * ch;
                    out[i] = buf[s] as u16;
                }
            }
            Some(out)
        } else {
            None
        };

        g.terrain
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, t_slot)| {
                let r16 = g.raw_u16[i];
                let riv = riv_u16_opt.as_ref().map_or(0, |v| v[i]);
                let (r_c, g_c, b_c) = if let Some(ref d) = diff_rgb_opt {
                    (d[i * 3] as i16, d[i * 3 + 1] as i16, d[i * 3 + 2] as i16)
                } else {
                    (100, 100, 100)
                };

                let mut tc = PLAIN;
                if diff_rgb_opt.is_some() {
                    if (g_c - r_c >= 7) && (g_c >= 95) && (b_c < 85) {
                        tc = FOREST;
                    }
                    if (b_c >= 68) && (g_c >= r_c) && (r16 < 4300) && (riv <= flow_threshold) {
                        tc = SWAMP;
                    }
                    if r16 >= 5200 || (r_c + g_c + b_c) < 255 {
                        tc = MOUNTAIN;
                    }
                    if riv > flow_threshold || (b_c > r_c + 8 && b_c > g_c - 10) {
                        tc = WATER;
                    }
                } else {
                    if r16 >= 5200 {
                        tc = MOUNTAIN;
                    }
                    if riv > flow_threshold {
                        tc = WATER;
                    }
                }
                *t_slot = tc;
            });

        g.base_terrain.copy_from_slice(&g.terrain);
        g.sync_feature_flags_from_terrain();
        g.refresh_attrition();
        Ok(g)
    }
}
