//! `world_grid` module: 1:1 no-downsampling 200 m/px terrain tensor, Horn gradients,
//! dynamic sparse pixel modifiers, CellStash, field wires, AO bitmasks, and typed frontlines.
//! Replaces Python `terrain_field.py` + `viz_v2.py` grid encoding + `command_v2.py` AO channels.

use bitflags::bitflags;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

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

