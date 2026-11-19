use crate::catalog::ItemCatalog;
use crate::math_ops::{
    AugmentedArcIntegrator, DetonatorSwitchEntity, SchmidtPostureState, UnitZohDirectiveFilter,
    WorldPos,
};
use crate::ruleset::{ObjectiveSpec, SimRuleset, UnitSpawnSpec};
use crate::terrain::TerrainGrid;
use crate::types::{
    FireDiscipline, MobilityKind, PlatoonId, Team, UnitId, UnitOrder, UnitRole,
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
        }
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
        };

        self.units.insert(uid, rec);
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
        let world: Self =
            serde_json::from_str(cleaned).map_err(|e| format!("world load_json error: {e}"))?;
        let n_cells = world.terrain.rows * world.terrain.cols;
        if world.cell_h_blast.len() != n_cells || world.cell_s_supp.len() != n_cells {
            return Err("corrupted world save: cell field dimensions mismatch terrain".into());
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
