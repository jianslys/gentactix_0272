use super::*;

impl WorldGrid {

    pub fn encode_tile_rgba(&self, tile_r: usize, tile_c: usize, tile_size: usize) -> (usize, usize, Vec<u8>) {
        let r0 = tile_r * tile_size;
        let c0 = tile_c * tile_size;
        if r0 >= self.rows || c0 >= self.cols {
            return (0, 0, Vec::new());
        }
        let h = (self.rows - r0).min(tile_size);
        let w = (self.cols - c0).min(tile_size);
        let mut rgba = vec![0u8; h * w * 4];

        let mut min_u = u16::MAX;
        let mut max_u = 0u16;
        // Use fast sample or fixed range mapped from stored elevation_m bounds
        let min_m = self.elev_min_m;
        let inv_span = 1.0 / (self.elev_max_m - min_m).max(1.0);
        let _ = (&mut min_u, &mut max_u);

        for dr in 0..h {
            let gr = r0 + dr;
            let row_off = gr * self.cols + c0;
            let out_row = dr * w * 4;
            for dc in 0..w {
                let gi = row_off + dc;
                let tc = self.terrain[gi];
                let norm_e = ((self.elevation_m[gi] - min_m) * inv_span).clamp(0.0, 1.0);
                let (br, bg, bb): (f32, f32, f32) = match tc {
                    PLAIN => (38.0, 48.0, 38.0),
                    FOREST => (24.0, 62.0, 34.0),
                    MOUNTAIN => (7.0, 8.0, 12.0),
                    SWAMP => (36.0, 56.0, 52.0),
                    WATER => (16.0, 58.0, 108.0),
                    URBAN => (72.0, 68.0, 76.0),
                    DIRT_ROAD => (92.0, 74.0, 52.0),
                    HIGHWAY => (130.0, 105.0, 70.0),
                    RAIL => (90.0, 90.0, 105.0),
                    BRIDGE => (190.0, 145.0, 70.0),
                    AIRFIELD => (95.0, 95.0, 110.0),
                    _ => (42.0, 48.0, 42.0),
                };
                // Directional hillshade from NW (-grad_x - grad_y)
                let hillshade = (-self.grad_x[gi] * 0.7 - self.grad_y[gi] * 0.7).clamp(-0.22, 0.22);
                let (mut rr, gg, mut b_out) = if tc == MOUNTAIN || norm_e > 0.78 {
                    let k = (1.0 - (norm_e - 0.65) * 2.2).max(0.05);
                    ((br * k) as i32, (bg * k) as i32, (bb * k) as i32)
                } else {
                    let shade = 0.75 + norm_e * 0.45 + hillshade;
                    ((br * shade) as i32, (bg * shade) as i32, (bb * shade) as i32)
                };
                if self.ao_owner_red[gi] != 0 {
                    rr = (rr + 12).min(255);
                }
                if self.ao_owner_blue[gi] != 0 {
                    b_out = (b_out + 14).min(255);
                }
                let p = out_row + dc * 4;
                rgba[p] = rr.clamp(0, 255) as u8;
                rgba[p + 1] = gg.clamp(0, 255) as u8;
                rgba[p + 2] = b_out.clamp(0, 255) as u8;
                rgba[p + 3] = 255;
            }
        }
        (h, w, rgba)
    }

    pub fn encode_terrain_raw_bin(&self) -> Vec<u8> {
        let n = self.rows * self.cols;
        let mut out = Vec::with_capacity(8 + n + n * 2 + n);
        out.extend_from_slice(&(self.rows as u32).to_le_bytes());
        out.extend_from_slice(&(self.cols as u32).to_le_bytes());
        out.extend_from_slice(&self.terrain);
        for &v in &self.raw_u16 {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for &d in &self.forest_density {
            out.push((d.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out
    }
}
