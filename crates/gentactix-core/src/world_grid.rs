//! `world_grid` module: 1:1 no-downsampling 200 m/px terrain tensor, Horn gradients,
//! dynamic sparse pixel modifiers, CellStash, field wires, AO bitmasks, and typed frontlines.
//! Replaces Python `terrain_field.py` + `viz_v2.py` grid encoding + `command_v2.py` AO channels.

use bitflags::bitflags;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub const PLAIN: u8 = 0;
pub const FOREST: u8 = 1;
pub const MOUNTAIN: u8 = 2;
pub const SWAMP: u8 = 3;
pub const WATER: u8 = 4;
pub const URBAN: u8 = 5;
pub const DIRT_ROAD: u8 = 10;
pub const HIGHWAY: u8 = 11;
pub const RAIL: u8 = 12;
pub const BRIDGE: u8 = 13;
pub const AIRFIELD: u8 = 14;

pub const CLEAR: u8 = 0;
pub const RAIN: u8 = 1;
pub const FOG: u8 = 2;
pub const SNOW: u8 = 3;
pub const MUD: u8 = 4;
pub const ICE: u8 = 5;

pub const WEATHER_OFFROAD: [f32; 6] = [1.0, 1.3, 1.1, 1.6, 2.2, 1.8];
pub const WEATHER_ROAD: [f32; 6] = [1.0, 1.1, 1.1, 1.3, 1.4, 1.5];
pub const WEATHER_VIS: [f32; 6] = [1.0, 0.7, 0.35, 0.5, 0.9, 0.9];
pub const NIGHT_BASE_VIS: f32 = 0.4;

pub const LINE_MASK_INFANTRY: u8 = 1 << 0;
pub const LINE_MASK_ARMOR: u8 = 1 << 1;
pub const LINE_MASK_HEAVY_AT: u8 = 1 << 2;
pub const LINE_MASK_RECON: u8 = 1 << 3;
pub const LINE_MASK_ARTY_LOG: u8 = 1 << 4;
pub const LINE_MASK_ALL: u8 = (1 << 5) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TeamId {
    Red = 0,
    Blue = 1,
}

impl TeamId {
    #[inline]
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "blue" => TeamId::Blue,
            _ => TeamId::Red,
        }
    }

    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            TeamId::Red => "red",
            TeamId::Blue => "blue",
        }
    }

    #[inline]
    pub fn enemy(self) -> Self {
        match self {
            TeamId::Red => TeamId::Blue,
            TeamId::Blue => TeamId::Red,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MobilityClass {
    Foot = 0,
    Wheeled = 1,
    Tracked = 2,
}

impl MobilityClass {
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            MobilityClass::Foot => "foot",
            MobilityClass::Wheeled => "wheeled",
            MobilityClass::Tracked => "tracked",
        }
    }

    #[inline]
    pub fn slope_limit_deg(self) -> f32 {
        match self {
            MobilityClass::Foot => 40.0,
            MobilityClass::Wheeled => 20.0,
            MobilityClass::Tracked => 28.0,
        }
    }

    #[inline]
    pub fn uphill_k(self) -> f32 {
        match self {
            MobilityClass::Foot => 4.0,
            MobilityClass::Wheeled => 9.0,
            MobilityClass::Tracked => 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum FrontlineRole {
    Deploy = 1,
    Move = 2,
    Attack = 3,
    GlobalAction = 4,
}

impl FrontlineRole {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "MOVE_LINE" | "MOVE" => FrontlineRole::Move,
            "ATTACK_LINE" | "ATTACK" => FrontlineRole::Attack,
            "GLOBAL_ACTION_LINE" | "GLOBAL" => FrontlineRole::GlobalAction,
            _ => FrontlineRole::Deploy,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FrontlineRole::Deploy => "DEPLOY_LINE",
            FrontlineRole::Move => "MOVE_LINE",
            FrontlineRole::Attack => "ATTACK_LINE",
            FrontlineRole::GlobalAction => "GLOBAL_ACTION_LINE",
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    pub struct FeatureFlags: u32 {
        const FOREST           = 1 << 0;
        const MOUNTAIN         = 1 << 1;
        const SWAMP            = 1 << 2;
        const WATER            = 1 << 3;
        const URBAN            = 1 << 4;
        const DIRT_ROAD        = 1 << 5;
        const HIGHWAY          = 1 << 6;
        const RAIL             = 1 << 7;
        const BRIDGE           = 1 << 8;
        const AIRFIELD         = 1 << 9;
        const TRENCH           = 1 << 10;
        const BUNKER           = 1 << 11;
        const MINEFIELD_RED    = 1 << 12;
        const MINEFIELD_BLUE   = 1 << 13;
        const FORT_DAMAGED     = 1 << 14;
        const BRIDGE_DESTROYED = 1 << 15;
        const CQB_AMBUSH       = 1 << 16;
    }
}

#[inline]
pub fn is_road_terrain(t: u8) -> bool {
    matches!(t, DIRT_ROAD | HIGHWAY | RAIL | BRIDGE)
}

#[inline]
pub fn terrain_name(t: u8) -> &'static str {
    match t {
        PLAIN => "平原",
        FOREST => "森林",
        MOUNTAIN => "山地",
        SWAMP => "沼泽",
        WATER => "水域",
        URBAN => "城市",
        DIRT_ROAD => "土路",
        HIGHWAY => "干线公路",
        RAIL => "铁路",
        BRIDGE => "桥梁",
        AIRFIELD => "野战机场",
        _ => "未知",
    }
}

#[inline]
pub fn terrain_cost(mob: MobilityClass, t: u8) -> f32 {
    match (mob, t) {
        (_, WATER) => f32::INFINITY,
        (MobilityClass::Foot, PLAIN) => 1.0,
        (MobilityClass::Foot, FOREST) => 1.4,
        (MobilityClass::Foot, MOUNTAIN) => 2.0,
        (MobilityClass::Foot, SWAMP) => 2.5,
        (MobilityClass::Foot, URBAN) => 1.3,
        (MobilityClass::Foot, DIRT_ROAD) => 0.85,
        (MobilityClass::Foot, HIGHWAY) => 0.8,
        (MobilityClass::Foot, RAIL) => 0.9,
        (MobilityClass::Foot, BRIDGE) => 0.85,
        (MobilityClass::Foot, AIRFIELD) => 0.9,
        (MobilityClass::Wheeled, PLAIN) => 2.0,
        (MobilityClass::Wheeled, FOREST) => 4.0,
        (MobilityClass::Wheeled, MOUNTAIN) => 6.0,
        (MobilityClass::Wheeled, SWAMP) => 8.0,
        (MobilityClass::Wheeled, URBAN) => 2.0,
        (MobilityClass::Wheeled, DIRT_ROAD) => 0.7,
        (MobilityClass::Wheeled, HIGHWAY) => 0.35,
        (MobilityClass::Wheeled, RAIL) => 4.0,
        (MobilityClass::Wheeled, BRIDGE) => 0.45,
        (MobilityClass::Wheeled, AIRFIELD) => 0.8,
        (MobilityClass::Tracked, PLAIN) => 1.2,
        (MobilityClass::Tracked, FOREST) => 2.2,
        (MobilityClass::Tracked, MOUNTAIN) => 3.5,
        (MobilityClass::Tracked, SWAMP) => 5.0,
        (MobilityClass::Tracked, URBAN) => 1.8,
        (MobilityClass::Tracked, DIRT_ROAD) => 0.8,
        (MobilityClass::Tracked, HIGHWAY) => 0.5,
        (MobilityClass::Tracked, RAIL) => 3.0,
        (MobilityClass::Tracked, BRIDGE) => 0.6,
        (MobilityClass::Tracked, AIRFIELD) => 0.8,
        _ => 1.0,
    }
}

#[inline]
pub fn base_cover(t: u8) -> f32 {
    match t {
        FOREST => 0.35,
        MOUNTAIN => 0.40,
        SWAMP => 0.10,
        URBAN => 0.50,
        _ => 0.0,
    }
}

#[inline]
pub fn base_concealment(t: u8) -> f32 {
    match t {
        FOREST => 0.50,
        MOUNTAIN => 0.30,
        SWAMP => 0.15,
        URBAN => 0.60,
        _ => 0.0,
    }
}

#[inline]
pub fn base_los_block_m(t: u8) -> f32 {
    match t {
        FOREST => 8.0,
        URBAN => 12.0,
        _ => 0.0,
    }
}

#[inline]
pub fn base_attrition_day(t: u8, w: u8) -> f32 {
    let ta = match t {
        SWAMP => 0.004,
        MOUNTAIN => 0.002,
        FOREST => 0.001,
        _ => 0.0,
    };
    let wa = match w {
        SNOW => 0.006,
        ICE => 0.008,
        MUD => 0.003,
        RAIN => 0.001,
        _ => 0.0,
    };
    ta + wa
}

#[inline]
pub fn unit_line_affinity(u_mask: u8, l_mask: u8) -> f32 {
    let um = if u_mask == 0 { LINE_MASK_INFANTRY } else { u_mask };
    let lm = if l_mask == 0 { LINE_MASK_ALL } else { l_mask };
    if (um & lm) != 0 {
        return 1.0;
    }
    if (um & LINE_MASK_ARMOR) != 0 && (lm & LINE_MASK_INFANTRY) != 0 {
        return 0.40;
    }
    if (um & LINE_MASK_INFANTRY) != 0 && (lm & (LINE_MASK_ARMOR | LINE_MASK_HEAVY_AT)) != 0 {
        return 0.30;
    }
    if (um & LINE_MASK_RECON) != 0 && (lm & LINE_MASK_INFANTRY) != 0 {
        return 0.35;
    }
    0.05
}

pub fn rasterize_polyline(points_px: &[[f32; 2]], rows: usize, cols: usize) -> (Vec<(i32, i32)>, [f32; 2]) {
    if points_px.is_empty() {
        return (Vec::new(), [0.0, 1.0]);
    }
    let pts: Vec<(i32, i32)> = points_px
        .iter()
        .map(|p| (p[0].round() as i32, p[1].round() as i32))
        .collect();
    if pts.len() == 1 {
        let (r0, c0) = pts[0];
        if r0 >= 0 && (r0 as usize) < rows && c0 >= 0 && (c0 as usize) < cols {
            return (vec![(r0, c0)], [0.0, 1.0]);
        }
        return (Vec::new(), [0.0, 1.0]);
    }

    let mut cells = Vec::new();
    let mut seen: FxHashSet<(i32, i32)> = FxHashSet::default();
    for pair in pts.windows(2) {
        let (r0, c0) = pair[0];
        let (r1, c1) = pair[1];
        let steps = (r1 - r0).abs().max((c1 - c0).abs());
        if steps == 0 {
            if r0 >= 0 && (r0 as usize) < rows && c0 >= 0 && (c0 as usize) < cols && seen.insert((r0, c0)) {
                cells.push((r0, c0));
            }
        } else {
            let dr = (r1 - r0) as f32;
            let dc = (c1 - c0) as f32;
            let sf = steps as f32;
            for i in 0..=steps {
                let rr = (r0 as f32 + dr * (i as f32 / sf)).round() as i32;
                let cc = (c0 as f32 + dc * (i as f32 / sf)).round() as i32;
                if rr >= 0 && (rr as usize) < rows && cc >= 0 && (cc as usize) < cols && seen.insert((rr, cc)) {
                    cells.push((rr, cc));
                }
            }
        }
    }
    let dr = (pts[pts.len() - 1].0 - pts[0].0) as f32;
    let dc = (pts[pts.len() - 1].1 - pts[0].1) as f32;
    let norm = dr.hypot(dc);
    let default_normal = if norm <= 1e-6 { [0.0, 1.0] } else { [dc / norm, -dr / norm] };
    (cells, default_normal)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellModEntry {
    pub key: String,
    pub speed_mult: f32,
    pub combat_power_mult: f32,
    pub accuracy_mult: f32,
    pub fortification_bonus: f32,
    pub cover_bonus: f32,
    pub suppressed: f32,
    pub mine_density: f32,
    pub decoy_signal: f32,
    pub los_block_m: f32,
    pub ttl_hours: Option<f32>,
}

impl CellModEntry {
    pub fn neutral(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            speed_mult: 1.0,
            combat_power_mult: 1.0,
            accuracy_mult: 1.0,
            fortification_bonus: 0.0,
            cover_bonus: 0.0,
            suppressed: 0.0,
            mine_density: 0.0,
            decoy_signal: 0.0,
            los_block_m: 0.0,
            ttl_hours: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellModifiers {
    pub speed_mult: f32,
    pub non_feature_speed_mult: f32,
    pub combat_power_mult: f32,
    pub accuracy_mult: f32,
    pub cover_bonus: f32,
    pub fortification_bonus: f32,
    pub concealment_bonus: f32,
    pub los_block_m: f32,
    pub suppressed: bool,
    pub mine_hazard: f32,
    pub decoy_signal: f32,
    pub is_road: bool,
    pub is_airfield: bool,
    pub ignore_slope_limit: bool,
    pub bridge_over_water: bool,
    pub impassable: bool,
    pub cqb_ambush: bool,
}

impl Default for CellModifiers {
    fn default() -> Self {
        Self {
            speed_mult: 1.0,
            non_feature_speed_mult: 1.0,
            combat_power_mult: 1.0,
            accuracy_mult: 1.0,
            cover_bonus: 0.0,
            fortification_bonus: 0.0,
            concealment_bonus: 0.0,
            los_block_m: 0.0,
            suppressed: false,
            mine_hazard: 0.0,
            decoy_signal: 0.0,
            is_road: false,
            is_airfield: false,
            ignore_slope_limit: false,
            bridge_over_water: false,
            impassable: false,
            cqb_ambush: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellStash {
    pub owner_team: Option<TeamId>,
    pub radio_channel: Option<u8>,
    pub food_t: f32,
    pub ammo_t: f32,
    pub fuel_t: f32,
    pub replacements: f32,
    pub fixed_radio_units: u32,
    pub equipment: FxHashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedFrontline {
    pub line_id: u32,
    pub name_id: String,
    pub team: TeamId,
    pub commander_id: usize,
    pub zone_id: u32,
    pub role: FrontlineRole,
    pub source_deploy_line_id: u32,
    pub target_deploy_line_id: u32,
    pub unit_type_mask: u8,
    pub attraction_weight: f32,
    pub front_normal: [f32; 2],
    pub points_px: Vec<[f32; 2]>,
    pub cells: Vec<(i32, i32)>,
    pub tangents: Vec<[f32; 2]>,
    pub spread_radius_px: i32,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FieldSample {
    pub in_any_ao: bool,
    pub in_own_ao: bool,
    pub ao_pull: [f32; 2],
    pub fl_pull: [f32; 2],
    pub fl_tangent: [f32; 2],
    pub fl_normal: [f32; 2],
    pub has_matching_line: bool,
    pub best_line_id: u32,
    pub best_line_role: Option<FrontlineRole>,
    pub best_line_source_deploy_id: u32,
    pub has_deploy_line: bool,
    pub deploy_line_id: u32,
    pub has_move_or_attack: bool,
    pub move_or_attack_vec: [f32; 2],
    pub move_or_attack_dist_px: f32,
    pub global_action_vec: [f32; 2],
    pub has_global_action: bool,
}

/// 优先瞄准目标标签
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriorityTarget {
    /// 点名锁定具体单位
    Unit(String),
    /// 优先打装甲/载具
    Armor,
    /// 优先打露天步兵
    Infantry,
    /// 优先打重武器阵地 (机枪/迫击炮/战防炮)
    HeavyWeapon,
    /// 无差别面覆盖火力 / 爆炸破片强制标签
    Any,
}

impl Default for PriorityTarget {
    fn default() -> Self {
        PriorityTarget::Any
    }
}

/// 注入像素容器的射弹流包
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotPacket {
    /// 目标像素格坐标
    pub target_px: (i32, i32),
    /// 开火炸点/射击源物理坐标 (直射为射手坐标，HE破片为炸点坐标)
    pub origin_px: (i32, i32),
    /// 优先瞄准标签
    pub priority_target: PriorityTarget,
    /// 适配弹药规格 ID (来自 items_catalog.json)
    pub ammo_spec_id: String,
    /// 本次注入的弹药/破片发数 (强制整数，向下取整，< 1 直接销毁)
    pub shots: u32,
    /// 射手命中精度 / 散布因子 (0.0 ~ 1.0)
    pub shooter_accuracy: f32,
    /// 入射角 (0 ~ 360 deg)
    pub incoming_dir_deg: f32,
    /// 剩余飞散杀伤半径 (仅用于大口径破片跨格扩散转发，米)
    pub remaining_radius_m: f32,
}

/// 像素网格伤害寄存容器 (挂载于 WorldGrid 每个 200m/px 格点)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PixelCellContainer {
    /// 待结算的射弹包队列
    pub incoming_packets: Vec<ShotPacket>,
    /// 弹着密度统计 (用于持续震荡目标 org，未击穿亦施加压制)
    pub suppression_density: f32,
}

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

    pub fn move_cost_step(&self, r0: i32, c0: i32, r1: i32, c1: i32, mob: MobilityClass, extra_mult: f32) -> f32 {
        if !self.in_bounds(r1, c1) {
            return f32::INFINITY;
        }
        let i1 = self.idx(r1, c1);
        let t1 = self.terrain[i1];
        if t1 == WATER {
            return f32::INFINITY;
        }
        let cm1 = self.get_cell_modifiers(r1, c1, mob);
        if cm1.impassable {
            return f32::INFINITY;
        }
        if !cm1.ignore_slope_limit && self.slope_deg[i1] > mob.slope_limit_deg() {
            return f32::INFINITY;
        }
        let tc = terrain_cost(mob, t1);
        if !tc.is_finite() {
            return f32::INFINITY;
        }
        let w1 = (self.weather[i1] as usize).min(5);
        let w_mult = if cm1.is_road { WEATHER_ROAD[w1] } else { WEATHER_OFFROAD[w1] };

        let horiz_m = (((r1 - r0) as f32).hypot((c1 - c0) as f32)) * self.cell_m;
        let dz = self.elevation_m[i1] - self.elevation_m[self.idx(r0, c0)];
        let grade = dz / horiz_m.max(1.0);
        let slope_mult = if grade >= 0.0 {
            1.0 + mob.uphill_k() * grade
        } else {
            (1.0 + 0.5 * grade).max(0.85)
        };
        let horiz_km = horiz_m / 1000.0;
        let speed_div = cm1.speed_mult.max(0.05);
        horiz_km * tc * w_mult * slope_mult * extra_mult / speed_div
    }

    pub fn shortest_path_windowed<F>(
        &self,
        start: (i32, i32),
        target: (i32, i32),
        mob: MobilityClass,
        blocked: F,
        extra_mult: f32,
    ) -> Option<(Vec<(i32, i32)>, Vec<f32>)>
    where
        F: Fn(i32, i32) -> bool,
    {
        let s = self.clamp_rc(start.0, start.1);
        let t = self.clamp_rc(target.0, target.1);
        if s == t {
            return Some((vec![s], vec![0.0]));
        }
        if !self.in_bounds(t.0, t.1) || self.terrain[self.idx(t.0, t.1)] == WATER {
            return None;
        }
        let span = (t.0 - s.0).abs().max((t.1 - s.1).abs());
        let pad = (span + 24).min(96);
        let r_min = (s.0.min(t.0) - pad).max(0);
        let r_max = (s.0.max(t.0) + pad).min((self.rows as i32) - 1);
        let c_min = (s.1.min(t.1) - pad).max(0);
        let c_max = (s.1.max(t.1) + pad).min((self.cols as i32) - 1);

        let min_step_factor = 0.35 * (self.cell_m / 1000.0);

        #[derive(Copy, Clone, PartialEq)]
        struct QNode {
            f: f32,
            g: f32,
            r: i32,
            c: i32,
        }
        impl Eq for QNode {}
        impl Ord for QNode {
            fn cmp(&self, o: &Self) -> Ordering {
                o.f.total_cmp(&self.f).then_with(|| o.r.cmp(&self.r)).then_with(|| o.c.cmp(&self.c))
            }
        }
        impl PartialOrd for QNode {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }

        let mut g_score: FxHashMap<(i32, i32), f32> = FxHashMap::default();
        let mut came_from: FxHashMap<(i32, i32), (i32, i32)> = FxHashMap::default();
        let mut pq = BinaryHeap::new();
        g_score.insert(s, 0.0);
        let h0 = ((t.0 - s.0) as f32).hypot((t.1 - s.1) as f32) * min_step_factor;
        pq.push(QNode { f: h0, g: 0.0, r: s.0, c: s.1 });

        const DIRS: [(i32, i32); 8] = [
            (-1, 0), (1, 0), (0, -1), (0, 1),
            (-1, -1), (-1, 1), (1, -1), (1, 1),
        ];

        while let Some(cur) = pq.pop() {
            if (cur.r, cur.c) == t {
                let mut rev_path = Vec::new();
                let mut p = t;
                rev_path.push(p);
                while let Some(&prev) = came_from.get(&p) {
                    rev_path.push(prev);
                    p = prev;
                }
                rev_path.reverse();
                let mut costs = Vec::with_capacity(rev_path.len());
                for pt in &rev_path {
                    costs.push(*g_score.get(pt).unwrap_or(&0.0));
                }
                return Some((rev_path, costs));
            }
            if cur.g > *g_score.get(&(cur.r, cur.c)).unwrap_or(&f32::INFINITY) + 1e-6 {
                continue;
            }
            for &(dr, dc) in &DIRS {
                let nr = cur.r + dr;
                let nc = cur.c + dc;
                if nr < r_min || nr > r_max || nc < c_min || nc > c_max {
                    continue;
                }
                if (nr, nc) != t && blocked(nr, nc) {
                    continue;
                }
                let step = self.move_cost_step(cur.r, cur.c, nr, nc, mob, extra_mult);
                if !step.is_finite() {
                    continue;
                }
                let ng = cur.g + step;
                if ng + 1e-6 < *g_score.get(&(nr, nc)).unwrap_or(&f32::INFINITY) {
                    g_score.insert((nr, nc), ng);
                    came_from.insert((nr, nc), (cur.r, cur.c));
                    let h = ((t.0 - nr) as f32).hypot((t.1 - nc) as f32) * min_step_factor;
                    pq.push(QNode { f: ng + h, g: ng, r: nr, c: nc });
                }
            }
        }
        None
    }

    pub fn advance_along(path: &[(i32, i32)], costs: &[f32], budget_km: f32) -> ((i32, i32), f32, bool) {
        if path.is_empty() {
            return ((0, 0), 0.0, false);
        }
        let mut best_i = 0usize;
        for (i, &c) in costs.iter().enumerate() {
            if c <= budget_km + 1e-6 {
                best_i = i;
            } else {
                break;
            }
        }
        let arrived = best_i + 1 == path.len();
        (path[best_i], costs[best_i], arrived)
    }

    pub fn write_ao_cells(&mut self, zone_id: u32, team: TeamId, echelon_rank: u8, cells: &[(i32, i32)]) {
        let e_bit = 1u8 << (echelon_rank.min(7));
        let (owners, masks) = match team {
            TeamId::Red => (&mut self.ao_owner_red, &mut self.ao_echelon_mask_red),
            TeamId::Blue => (&mut self.ao_owner_blue, &mut self.ao_echelon_mask_blue),
        };
        for &(r, c) in cells {
            if r >= 0 && (r as usize) < self.rows && c >= 0 && (c as usize) < self.cols {
                let i = (r as usize) * self.cols + (c as usize);
                owners[i] = zone_id;
                masks[i] |= e_bit;
            }
        }
    }

    pub fn write_typed_frontline(
        &mut self,
        name_id: &str,
        team: TeamId,
        commander_id: usize,
        zone_id: u32,
        role: FrontlineRole,
        source_deploy_line_id: u32,
        target_deploy_line_id: u32,
        unit_type_mask: u8,
        attraction_weight: f32,
        front_normal: Option<[f32; 2]>,
        points_px: &[[f32; 2]],
        spread_radius_px: i32,
    ) -> u32 {
        let (cells, def_norm) = rasterize_polyline(points_px, self.rows, self.cols);
        let normal = front_normal.unwrap_or(def_norm);
        let n_len = normal[0].hypot(normal[1]).max(1e-6);
        let normal = [normal[0] / n_len, normal[1] / n_len];

        let mut tangents = Vec::with_capacity(cells.len());
        for i in 0..cells.len() {
            let p_prev = cells[i.saturating_sub(1)];
            let p_next = cells[(i + 1).min(cells.len() - 1)];
            let dr = (p_next.0 - p_prev.0) as f32;
            let dc = (p_next.1 - p_prev.1) as f32;
            let l = dr.hypot(dc);
            if l > 1e-6 {
                tangents.push([dr / l, dc / l]);
            } else {
                tangents.push([-normal[1], normal[0]]);
            }
        }

        let line_idx = if let Some(&existing_id) = self.frontline_id_map.get(name_id) {
            let idx = (existing_id - 1) as usize;
            self.frontlines[idx] = TypedFrontline {
                line_id: existing_id,
                name_id: name_id.to_string(),
                team,
                commander_id,
                zone_id,
                role,
                source_deploy_line_id,
                target_deploy_line_id,
                unit_type_mask,
                attraction_weight,
                front_normal: normal,
                points_px: points_px.to_vec(),
                cells: cells.clone(),
                tangents,
                spread_radius_px,
                active: true,
            };
            existing_id
        } else {
            let new_id = (self.frontlines.len() + 1) as u32;
            self.frontlines.push(TypedFrontline {
                line_id: new_id,
                name_id: name_id.to_string(),
                team,
                commander_id,
                zone_id,
                role,
                source_deploy_line_id,
                target_deploy_line_id,
                unit_type_mask,
                attraction_weight,
                front_normal: normal,
                points_px: points_px.to_vec(),
                cells: cells.clone(),
                tangents,
                spread_radius_px,
                active: true,
            });
            self.frontline_id_map.insert(name_id.to_string(), new_id);
            new_id
        };

        let presence = match team {
            TeamId::Red => &mut self.fl_presence_red,
            TeamId::Blue => &mut self.fl_presence_blue,
        };
        let fl_vec_idx = (line_idx - 1) as usize;
        let rad = spread_radius_px.clamp(0, 12);
        let r2 = rad * rad;
        let bit_mask = if unit_type_mask == 0 { LINE_MASK_ALL } else { unit_type_mask };
        for &(cr, cc) in &cells {
            for dr in -rad..=rad {
                for dc in -rad..=rad {
                    if dr * dr + dc * dc <= r2 {
                        let nr = cr + dr;
                        let nc = cc + dc;
                        if nr >= 0 && (nr as usize) < self.rows && nc >= 0 && (nc as usize) < self.cols {
                            let flat = (nr as usize) * self.cols + (nc as usize);
                            presence[flat] |= bit_mask | 0x80; // high bit = any frontline nearby
                            let list = self.fl_cell_entries.entry(flat as u32).or_default();
                            if !list.contains(&fl_vec_idx) {
                                list.push(fl_vec_idx);
                            }
                        }
                    }
                }
            }
        }
        line_idx
    }

    pub fn sample_ao_and_frontline_field(
        &self,
        pos: [f32; 2],
        team: TeamId,
        unit_type_mask: u8,
        active_deploy_line_id: u32,
        direct_commander_id: Option<usize>,
    ) -> FieldSample {
        let mut out = FieldSample::default();
        let (cr, cc) = self.clamp_rc(pos[0].round() as i32, pos[1].round() as i32);
        let flat = self.idx(cr, cc);
        let (owners, presence) = match team {
            TeamId::Red => (&self.ao_owner_red, &self.fl_presence_red),
            TeamId::Blue => (&self.ao_owner_blue, &self.fl_presence_blue),
        };

        let cur_ao = owners[flat];
        out.in_any_ao = cur_ao != 0;
        out.in_own_ao = cur_ao != 0;

        // If outside own AO, find local AO boundary pull in a small ring
        if !out.in_any_ao {
            let mut best_d2 = f32::INFINITY;
            for dr in -6..=6 {
                for dc in -6..=6 {
                    let nr = cr + dr;
                    let nc = cc + dc;
                    if self.in_bounds(nr, nc) && owners[self.idx(nr, nc)] != 0 {
                        let d2 = (dr * dr + dc * dc) as f32;
                        if d2 < best_d2 && d2 > 0.0 {
                            best_d2 = d2;
                            let inv = 1.0 / d2.sqrt();
                            out.ao_pull = [dr as f32 * inv, dc as f32 * inv];
                        }
                    }
                }
            }
        }

        // Fast check: only inspect cell frontlines OR active lines of this commander/team
        let mut candidates: Vec<usize> = Vec::new();
        if presence[flat] != 0 {
            if let Some(list) = self.fl_cell_entries.get(&(flat as u32)) {
                candidates.extend_from_slice(list);
            }
        }
        // Also scan commander's active lines in case unit is outside spread_radius
        for (idx, fl) in self.frontlines.iter().enumerate() {
            if !fl.active || fl.team != team {
                continue;
            }
            if let Some(dcid) = direct_commander_id {
                if fl.commander_id != dcid && !candidates.contains(&idx) {
                    continue;
                }
            }
            if !candidates.contains(&idx) {
                candidates.push(idx);
            }
        }

        let mut best_score = -1.0f32;
        let mut best_move_score = -1.0f32;

        for idx in candidates {
            let Some(fl) = self.frontlines.get(idx) else {
                continue;
            };
            if !fl.active || fl.team != team || fl.cells.is_empty() {
                continue;
            }
            if let Some(dcid) = direct_commander_id {
                if fl.commander_id != dcid {
                    continue;
                }
            }
            let aff = unit_line_affinity(unit_type_mask, fl.unit_type_mask);
            // Nearest point along frontline cells
            let mut min_d2 = f32::INFINITY;
            let mut near_i = 0usize;
            for (ci, &(lr, lc)) in fl.cells.iter().enumerate() {
                let d2 = (lr as f32 - pos[0]).powi(2) + (lc as f32 - pos[1]).powi(2);
                if d2 < min_d2 {
                    min_d2 = d2;
                    near_i = ci;
                }
            }
            let dist = min_d2.sqrt();
            let near_rc = fl.cells[near_i];
            let end_rc = *fl.cells.last().unwrap();
            let pull_target = match fl.role {
                FrontlineRole::Move | FrontlineRole::Attack => {
                    let end_dist = (end_rc.0 as f32 - pos[0]).hypot(end_rc.1 as f32 - pos[1]);
                    if end_dist > 0.75 {
                        // Guide along polyline ahead of nearest cell or towards endpoint
                        let next_i = (near_i + 2).min(fl.cells.len() - 1);
                        fl.cells[next_i]
                    } else {
                        end_rc
                    }
                }
                FrontlineRole::GlobalAction => {
                    let next_i = (near_i + 2).min(fl.cells.len() - 1);
                    fl.cells[next_i]
                }
                FrontlineRole::Deploy => near_rc,
            };
            let pdr = pull_target.0 as f32 - pos[0];
            let pdc = pull_target.1 as f32 - pos[1];
            let plen = pdr.hypot(pdc);
            let pdir = if plen > 1e-5 { [pdr / plen, pdc / plen] } else { [0.0, 0.0] };

            match fl.role {
                FrontlineRole::GlobalAction => {
                    if plen > 1e-5 {
                        out.global_action_vec = pdir;
                        out.has_global_action = true;
                    }
                }
                FrontlineRole::Move | FrontlineRole::Attack => {
                    // Check source_deploy_line_id provenance!
                    let provenance_ok = fl.source_deploy_line_id == 0
                        || active_deploy_line_id == 0
                        || fl.source_deploy_line_id == active_deploy_line_id;
                    if provenance_ok {
                        let sc = fl.attraction_weight * aff / (1.0 + dist * 0.25);
                        if sc > best_move_score {
                            best_move_score = sc;
                            out.has_move_or_attack = true;
                            out.move_or_attack_vec = pdir;
                            out.move_or_attack_dist_px = (end_rc.0 as f32 - pos[0]).hypot(end_rc.1 as f32 - pos[1]);
                            out.best_line_id = fl.line_id;
                            out.best_line_role = Some(fl.role);
                            out.best_line_source_deploy_id = fl.source_deploy_line_id;
                            out.fl_normal = fl.front_normal;
                        }
                    }
                }
                FrontlineRole::Deploy => {
                    if dist <= (fl.spread_radius_px as f32 + 1.5) {
                        out.has_deploy_line = true;
                        if out.deploy_line_id == 0 {
                            out.deploy_line_id = fl.line_id;
                        }
                    }
                    let sc = fl.attraction_weight * aff / (1.0 + dist * 0.5);
                    if sc > best_score {
                        best_score = sc;
                        out.has_matching_line = true;
                        out.fl_pull = pdir;
                        out.fl_tangent = fl.tangents[near_i];
                        out.fl_normal = fl.front_normal;
                        if !out.has_move_or_attack {
                            out.best_line_id = fl.line_id;
                            out.best_line_role = Some(FrontlineRole::Deploy);
                        }
                    }
                }
            }
        }
        out
    }

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
