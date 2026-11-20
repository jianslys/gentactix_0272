use crate::catalog::{AttackProfileIdx, ItemCatalog};
use crate::damage_model::{
    ArmorProfileRha, SoldierRecord, VehicleDamageState, VehicleSubsystemKind,
};
use crate::math_ops::{
    fuse_intel_log_survival, transition_vis_tier_hysteresis, AugmentedArcIntegrator,
    DetonatorSwitchEntity, SchmidtPostureState, UnitZohDirectiveFilter, WorldPos,
};
use crate::ruleset::{ObjectiveSpec, SimRuleset, UnitSpawnSpec};
use crate::step_cache::{
    ActiveWaveDirective, ScheduledStepEvent, WorldInstanceKind, WorldInstanceRecord,
    MAX_STEP_CACHE_HORIZON_TICKS,
};
use crate::terrain::TerrainGrid;
use crate::types::{
    FireDiscipline, MobilityKind, PlatoonId, Team, UnitId, UnitOrder, UnitRole, VisTier,
    DEFAULT_EPOCH_SECONDS, TICK_SECONDS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlatoonRecord {
    pub id: PlatoonId,
    pub name: String,
    pub team: Team,
    pub hq_unit: Option<UnitId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitRecord {
    pub id: UnitId,
    pub callsign: String,
    pub team: Team,
    pub platoon_id: Option<PlatoonId>,
    pub role: UnitRole,
    pub mobility: MobilityKind,
    pub pos: WorldPos,
    pub facing_deg: f32,
    pub alive: bool,
    pub men: u16,
    pub max_men: u16,
    pub hp: f32,
    pub max_hp: f32,
    pub armor_mm: f32,
    pub morale: f32,
    pub suppression: f32,
    pub posture: SchmidtPostureState,
    pub weapon_id: String,
    pub ammo_id: String,
    pub fuze_id: Option<String>,
    pub mount_ids: Vec<String>,
    pub ammo_rounds: u32,
    pub max_ammo_rounds: u32,
    pub smoke_grenades: u16,
    pub base_speed_mps: f32,
    pub vision_m: f32,
    pub base_radio_range_m: f32,
    pub effective_radio_range_m: f32,
    pub order: UnitOrder,
    pub fire_discipline: FireDiscipline,
    pub cooldown_s: f32,
    #[serde(default)]
    pub is_vehicle: bool,
    #[serde(default)]
    pub profile_idx: Option<AttackProfileIdx>,
    #[serde(default)]
    pub soldiers: Vec<SoldierRecord>,
    #[serde(default)]
    pub vehicle_damage: Option<VehicleDamageState>,
}

impl UnitRecord {
    pub fn recompute_damage_aggregates(&mut self) {
        if self.is_vehicle {
            if let Some(vd) = &self.vehicle_damage {
                self.men = vd.crew_alive;
                if vd.ammo_cooked_off || vd.crew_alive == 0 || self.hp <= 0.0 {
                    self.alive = false;
                    self.hp = 0.0;
                }
            }
            return;
        }
        if !self.soldiers.is_empty() {
            let alive_cnt = self.soldiers.iter().filter(|s| s.alive).count() as u16;
            let total_hp: f32 = self.soldiers.iter().filter(|s| s.alive).map(|s| s.hp).sum();
            self.men = alive_cnt;
            self.hp = total_hp.clamp(0.0, self.max_hp);
            if alive_cnt == 0 || self.hp <= 0.0 {
                self.alive = false;
                self.hp = 0.0;
                self.men = 0;
            }
        }
    }

    pub fn effective_speed_scale(&self) -> f32 {
        if let Some(vd) = &self.vehicle_damage {
            let eng = vd.subsystems[VehicleSubsystemKind::Engine as usize];
            let trk = vd.subsystems[VehicleSubsystemKind::Tracks as usize];
            if eng <= 0.15 || trk <= 0.15 || vd.crew_alive == 0 || vd.ammo_cooked_off {
                return 0.0;
            }
            return (0.5 * eng + 0.5 * trk).clamp(0.25, 1.0);
        }
        if !self.soldiers.is_empty() {
            let mut count = 0usize;
            let mut sum_legs = 0.0f32;
            for s in &self.soldiers {
                if s.alive {
                    count += 1;
                    sum_legs += s.regions[3].integrity;
                }
            }
            if count > 0 {
                return (sum_legs / count as f32).clamp(0.30, 1.0);
            }
        }
        1.0
    }

    pub fn effective_radio_scale(&self) -> f32 {
        if let Some(vd) = &self.vehicle_damage {
            let r = vd.subsystems[VehicleSubsystemKind::Radio as usize];
            if r <= 0.15 || vd.ammo_cooked_off || vd.crew_alive == 0 {
                0.0
            } else {
                r.clamp(0.0, 1.0)
            }
        } else {
            1.0
        }
    }

    pub fn can_fire_main_weapon(&self) -> bool {
        if !self.alive || self.men == 0 {
            return false;
        }
        if let Some(vd) = &self.vehicle_damage {
            return !vd.ammo_cooked_off
                && vd.crew_alive > 0
                && vd.subsystems[VehicleSubsystemKind::MainGun as usize] > 0.15;
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombatLogEntry {
    pub tick: u64,
    pub actor_id: UnitId,
    pub target_id: Option<UnitId>,
    pub kind: String,
    pub dist_m: f32,
    pub casualties: u16,
    pub hp_loss: f32,
    pub suppression_add: f32,
    pub ammo_spent: u32,
    pub message: String,
}

/// Authoritative simulation state: the single source of truth for saves, replays, and hashes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub tick: u64,
    pub epoch_seconds: u64,
    pub clock_seconds: u64,
    pub night: bool,
    pub ruleset: SimRuleset,
    pub terrain: TerrainGrid,
    pub platoons: BTreeMap<u32, PlatoonRecord>,
    pub units: BTreeMap<u32, UnitRecord>,
    pub next_unit_id: u32,
    pub next_platoon_id: u32,
    pub objectives: Vec<ObjectiveSpec>,
    pub cell_h_blast: Vec<f32>,
    pub cell_s_supp: Vec<f32>,
    pub engineer_integrators: BTreeMap<u32, AugmentedArcIntegrator>,
    pub detonators: Vec<DetonatorSwitchEntity>,
    pub radio_zoh_filters: BTreeMap<u32, UnitZohDirectiveFilter>,
    pub combat_logs: Vec<CombatLogEntry>,
    pub unit_revision: u64,
    #[serde(default)]
    pub g_work_remaining: BTreeMap<usize, f32>,
    #[serde(default = "default_next_event_id")]
    pub next_event_id: u64,
    #[serde(default)]
    pub world_instances: BTreeMap<u64, WorldInstanceRecord>,
    #[serde(default = "default_next_instance_id")]
    pub next_instance_id: u64,
    #[serde(default)]
    pub step_cache: BTreeMap<u64, Vec<ScheduledStepEvent>>,
    #[serde(default)]
    pub active_wave_directives: Vec<ActiveWaveDirective>,
    #[serde(default)]
    pub team_i_intel: [Vec<f32>; 2],
    #[serde(default)]
    pub team_i_vis_tier: [Vec<VisTier>; 2],
    #[serde(default)]
    pub team_s_threat: [Vec<f32>; 2],
}

const fn default_next_event_id() -> u64 {
    1
}

const fn default_next_instance_id() -> u64 {
    1
}

impl WorldState {
    pub fn new_blank(rows: usize, cols: usize, base_elevation_m: f32, ruleset: SimRuleset) -> Self {
        let terrain = TerrainGrid::new_flat(rows, cols, base_elevation_m);
        Self::from_terrain(terrain, ruleset, DEFAULT_EPOCH_SECONDS, false)
    }

    pub fn from_terrain(
        terrain: TerrainGrid,
        ruleset: SimRuleset,
        epoch_seconds: u64,
        night: bool,
    ) -> Self {
        let n_cells = terrain.rows * terrain.cols;
        Self {
            tick: 0,
            epoch_seconds,
            clock_seconds: epoch_seconds,
            night,
            ruleset,
            terrain,
            platoons: BTreeMap::new(),
            units: BTreeMap::new(),
            next_unit_id: 1,
            next_platoon_id: 1,
            objectives: Vec::new(),
            cell_h_blast: vec![0.0; n_cells],
            cell_s_supp: vec![0.0; n_cells],
            engineer_integrators: BTreeMap::new(),
            detonators: Vec::new(),
            radio_zoh_filters: BTreeMap::new(),
            combat_logs: Vec::new(),
            unit_revision: 1,
            g_work_remaining: BTreeMap::new(),
            next_event_id: 1,
            world_instances: BTreeMap::new(),
            next_instance_id: 1,
            step_cache: BTreeMap::new(),
            active_wave_directives: Vec::new(),
            team_i_intel: [vec![0.0; n_cells], vec![0.0; n_cells]],
            team_i_vis_tier: [vec![VisTier::Hidden; n_cells], vec![VisTier::Hidden; n_cells]],
            team_s_threat: [vec![0.0; n_cells], vec![0.0; n_cells]],
        }
    }

    #[inline]
    pub fn alloc_event_id(&mut self) -> u64 {
        let id = self.next_event_id.max(1);
        self.next_event_id = id.saturating_add(1);
        id
    }

    #[inline]
    pub fn alloc_instance_id(&mut self) -> u64 {
        let id = self.next_instance_id.max(1);
        self.next_instance_id = id.saturating_add(1);
        id
    }

    pub fn inject_team_intel(&mut self, team: Team, cell_idx: usize, j_evidence: f32) {
        if j_evidence <= 0.0 {
            return;
        }
        let targets: &[usize] = match team.index() {
            Some(0) => &[0],
            Some(1) => &[1],
            _ => &[0, 1],
        };
        for &t_idx in targets {
            if cell_idx < self.team_i_intel[t_idx].len() {
                let fused = fuse_intel_log_survival(self.team_i_intel[t_idx][cell_idx], j_evidence);
                self.team_i_intel[t_idx][cell_idx] = fused;
                if cell_idx < self.team_i_vis_tier[t_idx].len() {
                    self.team_i_vis_tier[t_idx][cell_idx] =
                        transition_vis_tier_hysteresis(self.team_i_vis_tier[t_idx][cell_idx], fused);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_world_instance(
        &mut self,
        kind: WorldInstanceKind,
        team: Team,
        owner_unit_id: Option<UnitId>,
        pos: WorldPos,
        label: impl Into<String>,
        blast_radius_m: f32,
        blast_kpa: f32,
        crater_depth_m: f32,
    ) -> Result<u64, String> {
        let pos_clean = sanitize_pos(pos);
        let cell = self
            .terrain
            .geom
            .world_to_cell(pos_clean)
            .map_err(|e| format!("world instance out of bounds: {e}"))?;
        let id = self.alloc_instance_id();
        self.world_instances.insert(
            id,
            WorldInstanceRecord {
                instance_id: id,
                kind,
                team,
                owner_unit_id,
                pos: pos_clean,
                created_tick: self.tick,
                active: true,
                label: label.into(),
                blast_radius_m: sanitize_f32(blast_radius_m.max(0.0)),
                blast_kpa: sanitize_f32(blast_kpa.max(0.0)),
                crater_depth_m: sanitize_f32(crater_depth_m.max(0.0)),
            },
        );
        if let Ok(cidx) = self.terrain.idx(cell) {
            // Creator/owning side has immediate awareness (I_intel >= 0.75) on its own created instance.
            if team != Team::Neutral {
                self.inject_team_intel(team, cidx, 2.5);
            }
        }
        self.unit_revision = self.unit_revision.wrapping_add(1);
        Ok(id)
    }

    pub fn schedule_step_event(&mut self, mut event: ScheduledStepEvent) -> Result<u64, String> {
        let max_tick = self.tick.saturating_add(MAX_STEP_CACHE_HORIZON_TICKS);
        if event.target_tick < self.tick || event.target_tick > max_tick {
            return Err(format!(
                "step_cache target_tick {} out of 60-day range [{}..={}]",
                event.target_tick, self.tick, max_tick
            ));
        }
        if event.event_id == 0 {
            event.event_id = self.alloc_event_id();
        }
        event.exact_ms = event.exact_ms.clamp(1, 16_000);
        let eid = event.event_id;
        self.step_cache.entry(event.target_tick).or_default().push(event);
        Ok(eid)
    }

    pub fn drain_due_step_events(&mut self, up_to_tick: u64) -> Vec<ScheduledStepEvent> {
        let due_ticks: Vec<u64> = self
            .step_cache
            .keys()
            .copied()
            .take_while(|&t| t <= up_to_tick)
            .collect();
        if due_ticks.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for t in due_ticks {
            if let Some(mut bucket) = self.step_cache.remove(&t) {
                out.append(&mut bucket);
            }
        }
        out.sort_by_key(|ev| (ev.target_tick, ev.exact_ms, ev.event_id));
        out
    }

    pub fn cancel_step_event(
        &mut self,
        event_id: u64,
        tick_hint: Option<u64>,
    ) -> Option<ScheduledStepEvent> {
        if let Some(t) = tick_hint {
            if let Some(bucket) = self.step_cache.get_mut(&t) {
                if let Some(pos) = bucket.iter().position(|e| e.event_id == event_id) {
                    let removed = bucket.remove(pos);
                    if bucket.is_empty() {
                        self.step_cache.remove(&t);
                    }
                    return Some(removed);
                }
            }
        }
        let found_tick = self
            .step_cache
            .iter()
            .find_map(|(&t, bucket)| bucket.iter().any(|e| e.event_id == event_id).then_some(t))?;
        let bucket = self.step_cache.get_mut(&found_tick)?;
        let pos = bucket.iter().position(|e| e.event_id == event_id)?;
        let removed = bucket.remove(pos);
        if bucket.is_empty() {
            self.step_cache.remove(&found_tick);
        }
        Some(removed)
    }

    pub fn spawn_platoon(&mut self, name: String, team: Team) -> PlatoonId {
        let id = self.next_platoon_id;
        self.next_platoon_id = self.next_platoon_id.saturating_add(1);
        self.platoons.insert(
            id,
            PlatoonRecord {
                id: PlatoonId(id),
                name,
                team,
                hq_unit: None,
            },
        );
        self.unit_revision = self.unit_revision.wrapping_add(1);
        PlatoonId(id)
    }

    pub fn insert_platoon_with_id(
        &mut self,
        id: u32,
        name: String,
        team: Team,
        hq_unit: Option<UnitId>,
    ) -> Result<PlatoonId, String> {
        if id == 0 {
            return Err("platoon id must be >= 1".into());
        }
        if self.platoons.contains_key(&id) {
            return Err(format!("duplicate platoon id {}", id));
        }
        self.platoons.insert(
            id,
            PlatoonRecord {
                id: PlatoonId(id),
                name,
                team,
                hq_unit,
            },
        );
        if id >= self.next_platoon_id {
            self.next_platoon_id = id.saturating_add(1);
        }
        self.unit_revision = self.unit_revision.wrapping_add(1);
        Ok(PlatoonId(id))
    }

    pub fn spawn_unit(
        &mut self,
        spec: &UnitSpawnSpec,
        catalog: &ItemCatalog,
    ) -> Result<UnitId, String> {
        self.terrain
            .geom
            .world_to_cell(spec.pos)
            .map_err(|e| format!("unit '{}' spawn position invalid: {e}", spec.callsign))?;

        let platoon_ref = match spec.platoon_id {
            Some(pid) => {
                if !self.platoons.contains_key(&pid) {
                    return Err(format!(
                        "unit '{}' references unknown platoon_id {}",
                        spec.callsign, pid
                    ));
                }
                Some(PlatoonId(pid))
            }
            None => None,
        };

        if !spec.weapon_id.is_empty() || !spec.ammo_id.is_empty() {
            let mount_refs: Vec<&str> = spec.mount_ids.iter().map(String::as_str).collect();
            catalog.resolve_attack_profile(
                &spec.weapon_id,
                &spec.ammo_id,
                spec.fuze_id.as_deref(),
                &mount_refs,
            )?;
        }

        let uid = self.next_unit_id;
        self.next_unit_id = self.next_unit_id.saturating_add(1);
        let id = UnitId(uid);

        let is_vehicle = spec.role == UnitRole::ArmoredVehicle
            || spec.mobility == MobilityKind::Tracked;
        let profile_idx = if !spec.weapon_id.is_empty() && !spec.ammo_id.is_empty() {
            catalog.find_profile_idx(
                &spec.weapon_id,
                &spec.ammo_id,
                spec.fuze_id.as_deref(),
                &spec.mount_ids,
            )
        } else {
            None
        };
        let (soldiers, vehicle_damage) = if is_vehicle {
            (
                Vec::new(),
                Some(VehicleDamageState::new(
                    ArmorProfileRha::from_base_mm(spec.armor_mm),
                    spec.men,
                )),
            )
        } else {
            let per_hp = (spec.hp.max(1.0) / spec.men.max(1) as f32).max(1.0);
            let list = (1..=spec.men)
                .map(|sid| SoldierRecord::new_healthy(sid, per_hp))
                .collect();
            (list, None)
        };

        let rec = UnitRecord {
            id,
            callsign: spec.callsign.clone(),
            team: spec.team,
            platoon_id: platoon_ref,
            role: spec.role,
            mobility: spec.mobility,
            pos: sanitize_pos(spec.pos),
            facing_deg: sanitize_f32(spec.facing_deg),
            alive: true,
            men: spec.men,
            max_men: spec.men,
            hp: spec.hp.max(1.0),
            max_hp: spec.hp.max(1.0),
            armor_mm: sanitize_f32(spec.armor_mm.max(0.0)),
            morale: 1.0,
            suppression: 0.0,
            posture: SchmidtPostureState::Active,
            weapon_id: spec.weapon_id.clone(),
            ammo_id: spec.ammo_id.clone(),
            fuze_id: spec.fuze_id.clone(),
            mount_ids: spec.mount_ids.clone(),
            ammo_rounds: spec.ammo_rounds,
            max_ammo_rounds: spec.ammo_rounds,
            smoke_grenades: spec.smoke_grenades,
            base_speed_mps: spec.base_speed_mps.max(0.1),
            vision_m: spec.vision_m.max(8.0),
            base_radio_range_m: spec.base_radio_range_m.max(0.0),
            effective_radio_range_m: spec.base_radio_range_m.max(0.0),
            order: UnitOrder::Hold,
            fire_discipline: FireDiscipline::FireAtWill,
            cooldown_s: 0.0,
            is_vehicle,
            profile_idx,
            soldiers,
            vehicle_damage,
        };

        self.units.insert(uid, rec);
        if let Ok(cell) = self.terrain.geom.world_to_cell(spec.pos) {
            if let Ok(cidx) = self.terrain.idx(cell) {
                if spec.team != Team::Neutral {
                    self.inject_team_intel(spec.team, cidx, 2.5);
                }
            }
        }
        if spec.role == UnitRole::PlatoonHq {
            if let Some(pid) = spec.platoon_id {
                if let Some(pl) = self.platoons.get_mut(&pid) {
                    if pl.hq_unit.is_none() {
                        pl.hq_unit = Some(id);
                    }
                }
            }
        }
        self.unit_revision = self.unit_revision.wrapping_add(1);
        Ok(id)
    }

    pub fn set_unit_armor_rha(&mut self, id: UnitId, rha: ArmorProfileRha) -> bool {
        let Some(u) = self.units.get_mut(&id.0) else {
            return false;
        };
        u.armor_mm = sanitize_f32(rha.front_mm.max(0.0));
        u.is_vehicle = true;
        if let Some(vd) = &mut u.vehicle_damage {
            vd.armor_rha = rha;
        } else {
            u.vehicle_damage = Some(VehicleDamageState::new(rha, u.men.max(1)));
        }
        self.unit_revision = self.unit_revision.wrapping_add(1);
        true
    }

    pub fn despawn_unit(&mut self, id: UnitId) -> bool {
        let removed = self.units.remove(&id.0).is_some();
        if removed {
            for pl in self.platoons.values_mut() {
                if pl.hq_unit == Some(id) {
                    pl.hq_unit = None;
                }
            }
            self.engineer_integrators.remove(&id.0);
            self.radio_zoh_filters.remove(&id.0);
            self.unit_revision = self.unit_revision.wrapping_add(1);
        }
        removed
    }

    #[inline]
    pub fn advance_clock_one_tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
        self.clock_seconds = self.epoch_seconds.saturating_add(self.tick << 4);
        debug_assert_eq!(self.clock_seconds, self.epoch_seconds + self.tick * TICK_SECONDS);
    }

    pub fn save_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("world save_json error: {e}"))
    }

    pub fn load_json(s: &str) -> Result<Self, String> {
        let cleaned = s.strip_prefix('\u{feff}').unwrap_or(s);
        let mut world: Self =
            serde_json::from_str(cleaned).map_err(|e| format!("world load_json error: {e}"))?;
        let n_cells = world.terrain.rows * world.terrain.cols;
        if world.cell_h_blast.len() != n_cells || world.cell_s_supp.len() != n_cells {
            return Err("corrupted world save: cell field dimensions mismatch terrain".into());
        }
        if world.terrain.mine_density.len() != n_cells {
            world.terrain.mine_density.resize(n_cells, 0.0);
        }
        for t in 0..2 {
            if world.team_i_intel[t].len() != n_cells {
                world.team_i_intel[t].resize(n_cells, 0.0);
            }
            if world.team_i_vis_tier[t].len() != n_cells {
                world.team_i_vis_tier[t].resize(n_cells, VisTier::Hidden);
            }
            if world.team_s_threat[t].len() != n_cells {
                world.team_s_threat[t].resize(n_cells, 0.0);
            }
        }
        if let Some(&max_inst) = world.world_instances.keys().max() {
            if world.next_instance_id <= max_inst {
                world.next_instance_id = max_inst.saturating_add(1);
            }
        }
        for u in world.units.values_mut() {
            let inferred_veh =
                u.is_vehicle || u.role == UnitRole::ArmoredVehicle || u.mobility == MobilityKind::Tracked;
            u.is_vehicle = inferred_veh;
            if inferred_veh {
                if u.vehicle_damage.is_none() {
                    u.vehicle_damage = Some(VehicleDamageState::new(
                        ArmorProfileRha::from_base_mm(u.armor_mm),
                        u.men.max(1),
                    ));
                }
            } else if u.soldiers.is_empty() && u.max_men > 0 {
                let per_hp = (u.max_hp.max(1.0) / u.max_men.max(1) as f32).max(1.0);
                for sid in 1..=u.max_men {
                    let mut s = SoldierRecord::new_healthy(sid, per_hp);
                    if sid > u.men || !u.alive {
                        s.alive = false;
                        s.hp = 0.0;
                    }
                    u.soldiers.push(s);
                }
            }
        }
        Ok(world)
    }

    pub fn authoritative_sha256(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("WorldState serialization must succeed");
        let digest = Sha256::digest(&bytes);
        let mut hex = String::with_capacity(64);
        for b in digest {
            use std::fmt::Write;
            let _ = write!(&mut hex, "{b:02x}");
        }
        hex
    }
}

#[inline]
fn sanitize_f32(v: f32) -> f32 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

#[inline]
fn sanitize_pos(p: WorldPos) -> WorldPos {
    WorldPos::new(sanitize_f32(p.x), sanitize_f32(p.y))
}
