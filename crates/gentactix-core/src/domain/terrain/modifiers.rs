use super::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

impl WorldGrid {

    pub fn set_cell_modifier(&mut self, r: i32, c: i32, entry: CellModEntry) {
        if !self.in_bounds(r, c) {
            return;
        }
        let i = self.idx(r, c) as u32;
        if entry.key == "fort:trench" {
            self.feature_flags[i as usize] |= FeatureFlags::TRENCH.bits();
        }
        let bucket = self.cell_mods.entry(i).or_default();
        if let Some(existing) = bucket.iter_mut().find(|e| e.key == entry.key) {
            *existing = entry;
        } else {
            bucket.push(entry);
        }
    }

    pub fn remove_cell_modifier(&mut self, r: i32, c: i32, key: &str) -> bool {
        if !self.in_bounds(r, c) {
            return false;
        }
        let i = self.idx(r, c) as u32;
        let mut removed = false;
        if let Some(bucket) = self.cell_mods.get_mut(&i) {
            let before = bucket.len();
            bucket.retain(|e| e.key != key);
            removed = bucket.len() < before;
            if bucket.is_empty() {
                self.cell_mods.remove(&i);
            }
        }
        removed
    }

    pub fn get_cell_modifiers(&self, r: i32, c: i32, mob: MobilityClass) -> CellModifiers {
        if !self.in_bounds(r, c) {
            return CellModifiers { impassable: true, speed_mult: 0.0, ..Default::default() };
        }
        let i = self.idx(r, c);
        let t = self.terrain[i];
        let fl = FeatureFlags::from_bits_truncate(self.feature_flags[i]);
        let mut cm = CellModifiers {
            cover_bonus: base_cover(t),
            concealment_bonus: base_concealment(t),
            los_block_m: base_los_block_m(t),
            is_road: is_road_terrain(t),
            is_airfield: t == AIRFIELD,
            ignore_slope_limit: is_road_terrain(t),
            bridge_over_water: t == BRIDGE,
            impassable: t == WATER,
            cqb_ambush: matches!(t, FOREST | URBAN),
            speed_mult: if t == WATER { 0.0 } else { 1.0 },
            non_feature_speed_mult: 1.0,
            ..Default::default()
        };

        if fl.contains(FeatureFlags::TRENCH) {
            cm.speed_mult *= 0.85;
            cm.cover_bonus += 0.30;
            cm.fortification_bonus += 0.45;
            cm.concealment_bonus += 0.25;
        }
        if fl.contains(FeatureFlags::BUNKER) {
            cm.speed_mult *= 0.80;
            cm.cover_bonus += 0.45;
            cm.fortification_bonus += 0.70;
            cm.concealment_bonus += 0.35;
        }
        if fl.intersects(FeatureFlags::MINEFIELD_RED | FeatureFlags::MINEFIELD_BLUE) {
            cm.speed_mult *= 0.65;
            cm.mine_hazard = cm.mine_hazard.max(1.0);
        }

        if let Some(bucket) = self.cell_mods.get(&(i as u32)) {
            for e in bucket {
                cm.speed_mult *= e.speed_mult;
                cm.non_feature_speed_mult *= e.speed_mult;
                cm.combat_power_mult *= e.combat_power_mult;
                cm.accuracy_mult *= e.accuracy_mult;
                cm.fortification_bonus += e.fortification_bonus;
                cm.cover_bonus += e.cover_bonus;
                if e.suppressed > 0.0 {
                    cm.suppressed = true;
                }
                cm.mine_hazard = cm.mine_hazard.max(e.mine_density);
                cm.decoy_signal += e.decoy_signal;
                cm.los_block_m += e.los_block_m;
            }
        }
        if mob != MobilityClass::Foot && t == RAIL {
            cm.speed_mult *= 0.85;
        }
        cm
    }

    pub fn effective_cover_at(&self, r: i32, c: i32) -> (f32, f32) {
        let cm = self.get_cell_modifiers(r, c, MobilityClass::Foot);
        let cov = (cm.cover_bonus + cm.fortification_bonus * 0.5).clamp(0.0, 0.85);
        (cov, cm.concealment_bonus.clamp(0.0, 0.90))
    }

    #[inline]
    pub fn apply_smoke(&mut self, r: i32, c: i32, intensity: f32, ttl_h: f32) {
        if !self.in_bounds(r, c) {
            return;
        }
        let i = self.idx(r, c);
        if self.smoke_ttl_h[i] <= 0.0 {
            self.smoke_active.push(i as u32);
        }
        self.smoke_ttl_h[i] = self.smoke_ttl_h[i].max(ttl_h);
        self.visibility_mod[i] = self.visibility_mod[i].min((1.0 - intensity).clamp(0.05, 1.0));
    }

    #[inline]
    pub fn apply_illumination(&mut self, r: i32, c: i32, ttl_h: f32) {
        if !self.in_bounds(r, c) {
            return;
        }
        let i = self.idx(r, c);
        if self.illum_ttl_h[i] <= 0.0 {
            self.illum_active.push(i as u32);
        }
        self.illum_ttl_h[i] = self.illum_ttl_h[i].max(ttl_h);
    }

    pub fn add_exposure(&mut self, center_r: i32, center_c: i32, amount: f32, radius_px: f32) {
        if amount <= 0.0 {
            return;
        }
        let lim = radius_px.ceil() as i32;
        let r2 = radius_px * radius_px + 1e-6;
        for dr in -lim..=lim {
            for dc in -lim..=lim {
                if (dr * dr + dc * dc) as f32 <= r2 {
                    let nr = center_r + dr;
                    let nc = center_c + dc;
                    if self.in_bounds(nr, nc) {
                        let i = self.idx(nr, nc);
                        if self.exposure[i] <= 1e-6 {
                            self.exposure_active.push(i as u32);
                        }
                        self.exposure[i] += amount;
                    }
                }
            }
        }
    }

    /// O(|active|) sparse decay — never scans untouched pixels on 4096x4096.
    pub fn tick_environment(&mut self, dt_hours: f32) {
        if dt_hours <= 0.0 {
            return;
        }
        let mut next_smoke = Vec::with_capacity(self.smoke_active.len());
        for &idx_u in &self.smoke_active {
            let i = idx_u as usize;
            let rem = self.smoke_ttl_h[i] - dt_hours;
            if rem > 1e-6 {
                self.smoke_ttl_h[i] = rem;
                next_smoke.push(idx_u);
            } else {
                self.smoke_ttl_h[i] = 0.0;
                self.visibility_mod[i] = 1.0;
            }
        }
        self.smoke_active = next_smoke;

        let mut next_illum = Vec::with_capacity(self.illum_active.len());
        for &idx_u in &self.illum_active {
            let i = idx_u as usize;
            let rem = self.illum_ttl_h[i] - dt_hours;
            if rem > 1e-6 {
                self.illum_ttl_h[i] = rem;
                next_illum.push(idx_u);
            } else {
                self.illum_ttl_h[i] = 0.0;
            }
        }
        self.illum_active = next_illum;

        // Transient exposure & impact flush: zero out immediately per subtick
        for &idx_u in &self.exposure_active {
            let i = idx_u as usize;
            self.exposure[i] = 0.0;
        }
        self.exposure_active.clear();

        let mut empty_buckets = Vec::new();
        for (&px_idx, bucket) in &mut self.cell_mods {
            bucket.retain_mut(|e| {
                if let Some(ttl) = &mut e.ttl_hours {
                    *ttl -= dt_hours;
                    *ttl > 1e-6
                } else {
                    true
                }
            });
            if bucket.is_empty() {
                empty_buckets.push(px_idx);
            }
        }
        for k in empty_buckets {
            self.cell_mods.remove(&k);
        }
    }

    pub fn stash_put_resource(&mut self, r: i32, c: i32, team: Option<TeamId>, food: f32, ammo: f32, fuel: f32, rep: f32, ch: Option<u8>, fixed_radio: u32) {
        if !self.in_bounds(r, c) {
            return;
        }
        let key = self.idx(r, c) as u32;
        let st = self.cell_stash.entry(key).or_default();
        if team.is_some() {
            st.owner_team = team;
        }
        if ch.is_some() {
            st.radio_channel = ch;
        }
        st.food_t += food;
        st.ammo_t += ammo;
        st.fuel_t += fuel;
        st.replacements += rep;
        st.fixed_radio_units += fixed_radio;
    }

    pub fn stash_put_equipment(&mut self, r: i32, c: i32, team: Option<TeamId>, ch: Option<u8>, eq: &[(String, u32)]) {
        if !self.in_bounds(r, c) {
            return;
        }
        let key = self.idx(r, c) as u32;
        let st = self.cell_stash.entry(key).or_default();
        if team.is_some() {
            st.owner_team = team;
        }
        if ch.is_some() {
            st.radio_channel = ch;
        }
        for (m, n) in eq {
            if *n > 0 {
                *st.equipment.entry(m.clone()).or_insert(0) += *n;
            }
        }
    }

    pub fn add_wire_edge(&mut self, a: (i32, i32), b: (i32, i32), team: TeamId) {
        if !self.in_bounds(a.0, a.1) || !self.in_bounds(b.0, b.1) {
            return;
        }
        let ia = self.idx(a.0, a.1) as u32;
        let ib = self.idx(b.0, b.1) as u32;
        let edge = if ia <= ib { (ia, ib) } else { (ib, ia) };
        let bit = 1u8 << (team as u8);
        *self.wire_edges.entry(edge).or_insert(0) |= bit;
    }

    pub fn lay_wire_line(&mut self, a: (i32, i32), b: (i32, i32), team: TeamId) {
        let (cells, _) = rasterize_polyline(&[[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]], self.rows, self.cols);
        for w in cells.windows(2) {
            self.add_wire_edge(w[0], w[1], team);
        }
    }

    pub fn cut_wire_at(&mut self, r: i32, c: i32, radius_px: f32, only_team: Option<TeamId>) {
        let lim = radius_px.ceil() as i32;
        let r2 = radius_px * radius_px + 1e-6;
        let mut victim_px: FxHashSet<u32> = FxHashSet::default();
        for dr in -lim..=lim {
            for dc in -lim..=lim {
                if (dr * dr + dc * dc) as f32 <= r2 && self.in_bounds(r + dr, c + dc) {
                    victim_px.insert(self.idx(r + dr, c + dc) as u32);
                }
            }
        }
        if victim_px.is_empty() {
            return;
        }
        let clear_mask = match only_team {
            Some(t) => !(1u8 << (t as u8)),
            None => 0u8,
        };
        self.wire_edges.retain(|&(a, b), mask| {
            if victim_px.contains(&a) || victim_px.contains(&b) {
                *mask &= clear_mask;
                *mask != 0
            } else {
                true
            }
        });
    }

    pub fn has_wire_connection(&self, a: (i32, i32), b: (i32, i32), team: TeamId) -> bool {
        if a == b {
            return true;
        }
        if !self.in_bounds(a.0, a.1) || !self.in_bounds(b.0, b.1) || self.wire_edges.is_empty() {
            return false;
        }
        let start = self.idx(a.0, a.1) as u32;
        let goal = self.idx(b.0, b.1) as u32;
        let bit = 1u8 << (team as u8);
        let mut adj: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        for (&(u, v), &m) in &self.wire_edges {
            if (m & bit) != 0 {
                adj.entry(u).or_default().push(v);
                adj.entry(v).or_default().push(u);
            }
        }
        let mut q = VecDeque::new();
        let mut vis: FxHashSet<u32> = FxHashSet::default();
        q.push_back(start);
        vis.insert(start);
        while let Some(cur) = q.pop_front() {
            if cur == goal {
                return true;
            }
            if let Some(nbs) = adj.get(&cur) {
                for &nx in nbs {
                    if vis.insert(nx) {
                        q.push_back(nx);
                    }
                }
            }
        }
        false
    }

    pub fn destroy_bridge(&mut self, r: i32, c: i32) -> bool {
        if !self.in_bounds(r, c) {
            return false;
        }
        let i = self.idx(r, c);
        if self.terrain[i] != BRIDGE {
            return false;
        }
        self.terrain[i] = WATER;
        self.feature_flags[i] |= FeatureFlags::BRIDGE_DESTROYED.bits() | FeatureFlags::WATER.bits();
        self.feature_flags[i] &= !FeatureFlags::BRIDGE.bits();
        self.destroyed_bridges.insert(i as u32);
        self.terrain_revision += 1;
        true
    }

    pub fn repair_bridge(&mut self, r: i32, c: i32) -> bool {
        if !self.in_bounds(r, c) {
            return false;
        }
        let i = self.idx(r, c);
        if self.terrain[i] != WATER && !self.destroyed_bridges.contains(&(i as u32)) {
            return false;
        }
        self.terrain[i] = BRIDGE;
        self.feature_flags[i] |= FeatureFlags::BRIDGE.bits();
        self.feature_flags[i] &= !(FeatureFlags::BRIDGE_DESTROYED.bits() | FeatureFlags::WATER.bits());
        self.destroyed_bridges.remove(&(i as u32));
        self.terrain_revision += 1;
        true
    }

    pub fn destroy_terrain(&mut self, r: i32, c: i32) -> bool {
        if !self.in_bounds(r, c) {
            return false;
        }
        let i = self.idx(r, c);
        if is_road_terrain(self.terrain[i]) || self.terrain[i] == AIRFIELD {
            self.terrain[i] = PLAIN;
            self.damaged_terrain.insert(i as u32);
            self.sync_feature_flags_from_terrain();
            self.terrain_revision += 1;
            return true;
        }
        false
    }

    pub fn repair_terrain(&mut self, r: i32, c: i32) -> bool {
        if !self.in_bounds(r, c) {
            return false;
        }
        let i = self.idx(r, c);
        if self.damaged_terrain.remove(&(i as u32)) {
            self.terrain[i] = self.base_terrain[i];
            self.sync_feature_flags_from_terrain();
            self.terrain_revision += 1;
            return true;
        }
        false
    }
}
