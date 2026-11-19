use crate::world_grid::TeamId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoldierItem {
    pub kind: String,  // "weapon" | "radio" | "ammo" | "ration" | "gear"
    pub model: String, // e.g. "rifle", "lmg", "radio", "ammo", "ration"
    pub name: String,
    pub count: u32,
    pub ammo_t: f32,
    pub slot: String,  // "hand" | "pack"
    // Reserved Men-of-War style weight & 2D grid dimensions (not enforced yet)
    #[serde(default)]
    pub weight_kg: f32,
    #[serde(default = "default_grid_dim")]
    pub grid_w: u8,
    #[serde(default = "default_grid_dim")]
    pub grid_h: u8,
    #[serde(default)]
    pub grid_x: u8,
    #[serde(default)]
    pub grid_y: u8,
}

fn default_grid_dim() -> u8 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoldierState {
    pub id: String,
    pub name: String,
    pub hp: f32,
    pub max_hp: f32,
    pub status: String, // "healthy" | "light" | "heavy" | "dead"
    pub weapon: String,
    pub ammo_t: f32,
    pub is_cadre: bool,
    #[serde(default = "default_soldier_role")]
    pub role: String, // "enlisted" | "nco" | "officer"
    pub items: Vec<SoldierItem>,
    // Hand occupancy flags: >= 2 items in hand forbids firing & operating heavy weapons
    #[serde(default)]
    pub cannot_fire: bool,
    #[serde(default)]
    pub cannot_operate_heavy: bool,
    // Reserved separate limits: hand weight does NOT count toward pack weight
    #[serde(default)]
    pub max_hand_weight_kg: Option<f32>,
    #[serde(default)]
    pub max_pack_weight_kg: Option<f32>,
    #[serde(default = "default_pack_cols")]
    pub pack_cols: u8,
    #[serde(default = "default_pack_rows")]
    pub pack_rows: u8,
}

fn default_soldier_role() -> String { "enlisted".to_string() }
fn default_pack_cols() -> u8 { 8 }
fn default_pack_rows() -> u8 { 6 }

impl SoldierState {
    pub fn hand_weight_kg(&self) -> f32 {
        self.items.iter().filter(|it| it.slot == "hand").map(|it| it.weight_kg * (it.count.max(1) as f32)).sum()
    }

    pub fn pack_weight_kg(&self) -> f32 {
        self.items.iter().filter(|it| it.slot != "hand").map(|it| it.weight_kg * (it.count.max(1) as f32)).sum()
    }

    pub fn can_place_in_pack(&self, gw: u8, gh: u8, gx: u8, gy: u8) -> bool {
        if gx + gw > self.pack_cols || gy + gh > self.pack_rows {
            return false;
        }
        for it in &self.items {
            if it.slot == "hand" { continue; }
            let overlap_x = gx < it.grid_x + it.grid_w && gx + gw > it.grid_x;
            let overlap_y = gy < it.grid_y + it.grid_h && gy + gh > it.grid_y;
            if overlap_x && overlap_y {
                return false;
            }
        }
        true
    }

    pub fn refresh_hand_occupancy_flags(&mut self) {
        let hand_n = self.items.iter().filter(|it| it.slot == "hand" && it.count > 0).count();
        self.cannot_fire = hand_n >= 2;
        self.cannot_operate_heavy = hand_n >= 2;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundBody {
    pub id: String,
    pub kind: String, // "wounded" (red skull) | "kia" (black skull)
    pub soldier: SoldierState,
    pub origin_unit_id: String,
    pub origin_unit_name: String,
    pub team: TeamId,
    pub pos: [f32; 2],
    pub looted: bool,
}
