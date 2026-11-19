use crate::catalog::ItemCatalog;
use crate::math_ops::WorldPos;
use crate::state::WorldState;
use crate::terrain::{TerrainGrid, TerrainKind};
use crate::types::{
    MobilityKind, Team, UnitRole, DEFAULT_EPOCH_SECONDS,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimRuleset {
    pub enforce_command_radius: bool,
    pub enforce_platoon_cohesion: bool,
    pub enforce_radio_deygout: bool,
    pub enforce_suppression_posture: bool,
    pub enforce_ammo_consumption: bool,
    pub enforce_fog_of_war: bool,
    pub enable_g_field_autonomy: bool,
    pub enable_time_machine_deferral: bool,
    pub allow_runtime_map_edit: bool,
}

impl SimRuleset {
    pub const fn hardcore_platoon() -> Self {
        Self {
            enforce_command_radius: true,
            enforce_platoon_cohesion: true,
            enforce_radio_deygout: true,
            enforce_suppression_posture: true,
            enforce_ammo_consumption: true,
            enforce_fog_of_war: true,
            enable_g_field_autonomy: true,
            enable_time_machine_deferral: true,
            allow_runtime_map_edit: false,
        }
    }

    pub const fn tactical_standard() -> Self {
        Self {
            enforce_command_radius: true,
            enforce_platoon_cohesion: true,
            enforce_radio_deygout: false,
            enforce_suppression_posture: true,
            enforce_ammo_consumption: true,
            enforce_fog_of_war: true,
            enable_g_field_autonomy: true,
            enable_time_machine_deferral: true,
            allow_runtime_map_edit: false,
        }
    }

    pub const fn arcade_sandbox() -> Self {
        Self {
            enforce_command_radius: false,
            enforce_platoon_cohesion: false,
            enforce_radio_deygout: false,
            enforce_suppression_posture: false,
            enforce_ammo_consumption: false,
            enforce_fog_of_war: false,
            enable_g_field_autonomy: false,
            enable_time_machine_deferral: false,
            allow_runtime_map_edit: true,
        }
    }
}

impl Default for SimRuleset {
    fn default() -> Self {
        Self::hardcore_platoon()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerrainSourceSpec {
    Flat {
        rows: usize,
        cols: usize,
        #[serde(default)]
        base_elevation_m: f32,
    },
    R16File {
        r16_path: String,
        #[serde(default)]
        diffuse_path: Option<String>,
        #[serde(default)]
        river_path: Option<String>,
        #[serde(default = "default_flow_threshold")]
        flow_threshold: u16,
        #[serde(default = "default_low_m")]
        low_m: f32,
        #[serde(default = "default_high_m")]
        high_m: f32,
    },
    InlineGrid {
        rows: usize,
        cols: usize,
        elevation_m: Vec<f32>,
        #[serde(default)]
        cells: Vec<TerrainKind>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlatoonSpec {
    pub id: u32,
    pub name: String,
    pub team: Team,
    #[serde(default)]
    pub hq_unit_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitSpawnSpec {
    pub callsign: String,
    pub team: Team,
    #[serde(default)]
    pub platoon_id: Option<u32>,
    #[serde(default = "default_unit_role")]
    pub role: UnitRole,
    #[serde(default = "default_mobility")]
    pub mobility: MobilityKind,
    pub pos: WorldPos,
    #[serde(default)]
    pub facing_deg: f32,
    #[serde(default = "default_men")]
    pub men: u16,
    #[serde(default = "default_hp")]
    pub hp: f32,
    #[serde(default)]
    pub armor_mm: f32,
    #[serde(default)]
    pub weapon_id: String,
    #[serde(default)]
    pub ammo_id: String,
    #[serde(default)]
    pub fuze_id: Option<String>,
    #[serde(default)]
    pub mount_ids: Vec<String>,
    #[serde(default = "default_ammo_rounds")]
    pub ammo_rounds: u32,
    #[serde(default = "default_smoke_grenades")]
    pub smoke_grenades: u16,
    #[serde(default = "default_speed_mps")]
    pub base_speed_mps: f32,
    #[serde(default = "default_vision_m")]
    pub vision_m: f32,
    #[serde(default = "default_radio_range_m")]
    pub base_radio_range_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveSpec {
    pub id: String,
    pub name: String,
    pub center: WorldPos,
    pub radius_m: f32,
    #[serde(default)]
    pub owner: Option<Team>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioSpec {
    pub name: String,
    #[serde(default = "default_epoch")]
    pub epoch_seconds: u64,
    #[serde(default)]
    pub night: bool,
    #[serde(default)]
    pub ruleset: SimRuleset,
    pub terrain: TerrainSourceSpec,
    #[serde(default)]
    pub platoons: Vec<PlatoonSpec>,
    #[serde(default)]
    pub units: Vec<UnitSpawnSpec>,
    #[serde(default)]
    pub objectives: Vec<ObjectiveSpec>,
}

impl ScenarioSpec {
    pub fn build_world(&self, catalog: &ItemCatalog) -> Result<WorldState, String> {
        let terrain = match &self.terrain {
            TerrainSourceSpec::Flat {
                rows,
                cols,
                base_elevation_m,
            } => {
                if *rows == 0 || *cols == 0 {
                    return Err("Flat terrain dimensions must be non-zero".into());
                }
                TerrainGrid::new_flat(*rows, *cols, *base_elevation_m)
            }
            TerrainSourceSpec::R16File {
                r16_path,
                diffuse_path,
                river_path,
                flow_threshold,
                low_m,
                high_m,
            } => TerrainGrid::from_r16_files(
                Path::new(r16_path),
                diffuse_path.as_deref().map(Path::new),
                river_path.as_deref().map(Path::new),
                *flow_threshold,
                *low_m,
                *high_m,
            )?,
            TerrainSourceSpec::InlineGrid {
                rows,
                cols,
                elevation_m,
                cells,
            } => TerrainGrid::from_parts(*rows, *cols, elevation_m.clone(), cells.clone())?,
        };

        let mut world = WorldState::from_terrain(
            terrain,
            self.ruleset.clone(),
            self.epoch_seconds,
            self.night,
        );
        world.objectives = self.objectives.clone();

        for pl in &self.platoons {
            world.insert_platoon_with_id(pl.id, pl.name.clone(), pl.team, None)?;
        }

        for unit_spec in &self.units {
            world.spawn_unit(unit_spec, catalog)?;
        }

        for pl in &self.platoons {
            if let Some(hq_uid) = pl.hq_unit_id {
                if !world.units.contains_key(&hq_uid) {
                    return Err(format!(
                        "platoon {} references unknown hq_unit_id {}",
                        pl.id, hq_uid
                    ));
                }
                if let Some(rec) = world.platoons.get_mut(&pl.id) {
                    rec.hq_unit = Some(crate::types::UnitId(hq_uid));
                }
            }
        }

        Ok(world)
    }

    pub fn to_json_pretty(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("scenario serialize error: {e}"))
    }

    pub fn from_json(s: &str) -> Result<Self, String> {
        let cleaned = s.strip_prefix('\u{feff}').unwrap_or(s);
        serde_json::from_str(cleaned).map_err(|e| format!("scenario parse error: {e}"))
    }
}

const fn default_flow_threshold() -> u16 {
    100
}
const fn default_low_m() -> f32 {
    120.0
}
const fn default_high_m() -> f32 {
    1480.0
}
const fn default_epoch() -> u64 {
    DEFAULT_EPOCH_SECONDS
}
const fn default_unit_role() -> UnitRole {
    UnitRole::InfantrySquad
}
const fn default_mobility() -> MobilityKind {
    MobilityKind::Foot
}
const fn default_men() -> u16 {
    9
}
const fn default_hp() -> f32 {
    100.0
}
const fn default_ammo_rounds() -> u32 {
    300
}
const fn default_smoke_grenades() -> u16 {
    2
}
const fn default_speed_mps() -> f32 {
    1.6
}
const fn default_vision_m() -> f32 {
    600.0
}
const fn default_radio_range_m() -> f32 {
    800.0
}
