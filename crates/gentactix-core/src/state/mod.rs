//! `state` module: Tripartite World State Boundaries
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 2 节与 plan.txt Phase 02 要求：
//! 1. WorldState (权威状态：可存档、可回放、可生成指纹)
//! 2. DerivedState (派生状态：带版本依赖、可从权威状态无损重建)
//! 3. TickScratch (单Tick工作区：瞬态生命周期，Tick结束全量重置)

use crate::geometry::{CellCoord, GridGeometry, WorldPos};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StateKind {
    Clock,
    Terrain,
    Entity,
    Inventory,
    Command,
    PersistentField,
    ScheduledEvent,
    Random,
}

/// 权威状态一：逻辑时钟与调度状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockState {
    pub tick: u64,
    pub logic_time_sec: f64,
    pub subtick_step_sec: f32, // Doctrine 默认 16.0s
    pub revision: u64,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            tick: 0,
            logic_time_sec: 0.0,
            subtick_step_sec: 16.0,
            revision: 0,
        }
    }
}

/// 权威状态二：地形权威状态与修订号
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainState {
    pub rows: u32,
    pub cols: u32,
    pub elevation_m: Vec<f32>,
    pub terrain_type: Vec<u8>,
    pub revision: u64,
}

impl TerrainState {
    pub fn new(rows: u32, cols: u32, default_type: u8, default_elev: f32) -> Self {
        let n = (rows * cols) as usize;
        Self {
            rows,
            cols,
            elevation_m: vec![default_elev; n],
            terrain_type: vec![default_type; n],
            revision: 1,
        }
    }

    #[inline]
    pub fn idx(&self, r: i32, c: i32) -> usize {
        (r as usize) * (self.cols as usize) + (c as usize)
    }

    /// 应用地形变异 (TerrainMutation)，权威版本号递增
    pub fn mutate_cell(&mut self, r: i32, c: i32, new_elev: f32, new_type: Option<u8>) {
        let i = self.idx(r, c);
        self.elevation_m[i] = new_elev;
        if let Some(t) = new_type {
            self.terrain_type[i] = t;
        }
        self.revision = self.revision.wrapping_add(1);
    }
}

/// 权威状态三：实体权威状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityAuthorityRecord {
    pub id: u32,
    pub str_id: String,
    pub team_id: u8,
    pub pos: WorldPos,
    pub facing_deg: f32,
    pub alive: bool,
    pub persons: u32,
    pub org: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityState {
    pub entities: FxHashMap<u32, EntityAuthorityRecord>,
    pub revision: u64,
}

impl EntityState {
    pub fn insert(&mut self, record: EntityAuthorityRecord) {
        self.entities.insert(record.id, record);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn commit_position(&mut self, entity_id: u32, new_pos: WorldPos) -> bool {
        if let Some(e) = self.entities.get_mut(&entity_id) {
            e.pos = new_pos;
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

/// 权威状态四：物资与弹药守恒账本
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InventoryLedger {
    /// 实体持有的装备/弹药: entity_id -> item_key -> count
    pub entity_stocks: FxHashMap<u32, FxHashMap<String, u32>>,
    pub ground_caches: FxHashMap<CellCoord, FxHashMap<String, u32>>,
    pub revision: u64,
}

impl InventoryLedger {
    pub fn deduct_ammo(&mut self, entity_id: u32, item: &str, count: u32) -> Result<(), &'static str> {
        let stocks = self.entity_stocks.get_mut(&entity_id).ok_or("Entity not in ledger")?;
        let entry = stocks.get_mut(item).ok_or("Item not in inventory")?;
        if *entry < count {
            return Err("Insufficient inventory stock");
        }
        *entry -= count;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
}

/// 权威状态五：持久环境场 (跨Tick存活)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistentFieldState {
    /// 烟雾场：cell_idx -> (optical_density, remaining_ttl_sec)
    pub smoke: FxHashMap<usize, (f32, f32)>,
    pub revision: u64,
}

impl PersistentFieldState {
    pub fn evolve(&mut self, dt_sec: f32) {
        let mut expired = Vec::new();
        for (&idx, (density, ttl)) in self.smoke.iter_mut() {
            *ttl -= dt_sec;
            if *ttl <= 0.0 {
                expired.push(idx);
            } else {
                *density *= (1.0 - 0.02 * (dt_sec / 16.0)).max(0.1);
            }
        }
        for idx in expired {
            self.smoke.remove(&idx);
        }
        self.revision = self.revision.wrapping_add(1);
    }
}

/// 权威状态六：计划中事件与装填任务 (跨Tick存活)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub task_id: u64,
    pub entity_id: u32,
    pub due_logic_sec: f64,
    pub action_kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduledEventState {
    pub tasks: Vec<ScheduledTask>,
    pub revision: u64,
}

/// 权威状态七：确定性随机状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RandomState {
    pub seed: u64,
    pub counter: u64,
}

impl Default for RandomState {
    fn default() -> Self {
        Self { seed: 42, counter: 0 }
    }
}

/// 容器一：WorldState 权威状态容器
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub geometry: GridGeometry,
    pub clock: ClockState,
    pub terrain: TerrainState,
    pub entities: EntityState,
    pub inventory: InventoryLedger,
    pub persistent_fields: PersistentFieldState,
    pub scheduled_events: ScheduledEventState,
    pub random: RandomState,
}

impl WorldState {
    pub fn new(geometry: GridGeometry, default_terrain: u8, default_elev: f32) -> Self {
        let rows = geometry.rows;
        let cols = geometry.cols;
        Self {
            geometry,
            clock: ClockState::default(),
            terrain: TerrainState::new(rows, cols, default_terrain, default_elev),
            entities: EntityState::default(),
            inventory: InventoryLedger::default(),
            persistent_fields: PersistentFieldState::default(),
            scheduled_events: ScheduledEventState::default(),
            random: RandomState::default(),
        }
    }
}

/// 派生缓存元数据 (依赖、版本与有效域)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheMeta {
    pub key: String,
    pub dependencies: Vec<(StateKind, u64)>,
    pub builder_version: u32,
    pub valid: bool,
}

impl CacheMeta {
    pub fn new(key: &str, builder_version: u32) -> Self {
        Self {
            key: key.to_string(),
            dependencies: Vec::new(),
            builder_version,
            valid: false,
        }
    }

    pub fn is_up_to_date(&self, world: &WorldState) -> bool {
        if !self.valid {
            return false;
        }
        for (kind, rev) in &self.dependencies {
            let cur_rev = match kind {
                StateKind::Terrain => world.terrain.revision,
                StateKind::Entity => world.entities.revision,
                StateKind::Clock => world.clock.revision,
                StateKind::Inventory => world.inventory.revision,
                StateKind::PersistentField => world.persistent_fields.revision,
                StateKind::ScheduledEvent => world.scheduled_events.revision,
                StateKind::Random => world.random.counter,
                StateKind::Command => 0,
            };
            if *rev != cur_rev {
                return false;
            }
        }
        true
    }
}

/// 派生状态缓存：SDF (符号距离场) 缓存
#[derive(Debug, Clone, Default)]
pub struct SdfCache {
    pub meta: Option<CacheMeta>,
    pub sdf_distance_m: Vec<f32>,
    pub normal_vec: Vec<[f32; 2]>,
}

/// 第一类修改派生缓存：班组/指挥班/重武器任务队列与路径/LOS预览缓存
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedActionItem {
    pub unit_id: String,
    pub action_kind: String, // "move" | "fire" | "conceal" | "stop" | "retreat"
    pub waypoints: Vec<[f32; 2]>,
    pub eta_sec: f32,
    pub fire_live_segments: Vec<[f32; 4]>,
    pub fire_blocked_segments: Vec<[f32; 4]>,
    pub dispersion_m: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlannedActionCache {
    pub actions: FxHashMap<String, Vec<PlannedActionItem>>,
    pub revision: u64,
}

/// 第二类修改派生缓存：布雷、挖坑、建造、爆破、拆除、无线电、测距读秒事件队列预览
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedEventItem {
    pub event_id: u64,
    pub unit_id: String,
    pub kind: String, // "mine" | "trench" | "build" | "demolish" | "clear" | "radio" | "rangefind"
    pub remaining_sec: f32,
    pub target_cell: Option<CellCoord>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlannedEventCache {
    pub events: Vec<PlannedEventItem>,
    pub revision: u64,
}

/// 第三类修改派生缓存：非队列直接调整草案（拆分/整编编制、士兵装备、使用物品、架设/收拢重武器）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImmediateStagingOp {
    pub op_id: u64,
    pub op_type: String, // "reorg" | "equip" | "use_item" | "deploy_weapon" | "pack_weapon"
    pub unit_id: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImmediateStagingCache {
    pub pending_ops: Vec<ImmediateStagingOp>,
    pub revision: u64,
}

/// 第四类修改派生缓存：指挥AO、进攻线、移动线、部署线及欧拉连续场（I, G, SDF）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandFieldCache {
    pub ao_zones: Vec<String>,
    pub attack_lines: Vec<Vec<[f32; 2]>>,
    pub move_lines: Vec<Vec<[f32; 2]>>,
    pub deploy_lines: Vec<Vec<[f32; 2]>>,
    pub gravity_field_g: Vec<f32>,
    pub repulse_field_i: Vec<f32>,
    pub revision: u64,
}

/// 容器二：DerivedState 派生状态容器 (可随时销毁并由 WorldState 重建)
#[derive(Debug, Clone, Default)]
pub struct DerivedState {
    pub sdf: SdfCache,
    pub threat_field_revision: u64,
    pub planned_actions: PlannedActionCache,
    pub planned_events: PlannedEventCache,
    pub immediate_staging: ImmediateStagingCache,
    pub command_fields: CommandFieldCache,
}

impl DerivedState {
    pub fn invalidate_by_mutation(&mut self, kind: StateKind) {
        if kind == StateKind::Terrain {
            if let Some(m) = self.sdf.meta.as_mut() {
                m.valid = false;
            }
        }
    }

    pub fn rebuild_sdf_if_needed(&mut self, world: &WorldState) {
        let needs_rebuild = self.sdf.meta.as_ref().map_or(true, |m| !m.is_up_to_date(world));
        if needs_rebuild {
            let n = (world.geometry.rows * world.geometry.cols) as usize;
            self.sdf.sdf_distance_m = vec![1000.0; n];
            self.sdf.normal_vec = vec![[0.0, 0.0]; n];
            let mut meta = CacheMeta::new("sdf_global", 1);
            meta.dependencies.push((StateKind::Terrain, world.terrain.revision));
            meta.valid = true;
            self.sdf.meta = Some(meta);
        }
    }
}

/// 8m 网格杀伤寄存桶瞬态草稿条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridFireRecord {
    pub cell_idx: usize,
    pub packets: Vec<crate::world_grid::ShotPacket>,
    pub total_shrapnel_flux: f32,
    pub max_overpressure_kpa: f32,
    pub suppression_density: f32,
}

/// 计划事件到期变异提案
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMutation {
    pub event_id: u64,
    pub unit_id: String,
    pub target_cell: Option<CellCoord>,
    pub mutation_kind: String,
    pub description: String,
}

/// 容器三：TickScratch 单 Tick 瞬态工作区
#[derive(Debug, Clone, Default)]
pub struct TickScratch {
    pub tick_id: u64,
    pub intent_candidates: Vec<u32>,
    pub motion_proposals: Vec<(u32, WorldPos)>,
    pub transient_blast_cell_indices: Vec<usize>,
    pub grid_fire_scratch: Vec<GridFireRecord>,
    pub completed_event_mutations: Vec<EventMutation>,
    pub diagnostics: Vec<String>,
}

impl TickScratch {
    pub fn begin_tick(&mut self, tick_id: u64) {
        self.tick_id = tick_id;
        self.intent_candidates.clear();
        self.motion_proposals.clear();
        self.transient_blast_cell_indices.clear();
        self.grid_fire_scratch.clear();
        self.completed_event_mutations.clear();
        self.diagnostics.clear();
    }

    pub fn reset_scratch(&mut self) {
        self.intent_candidates.clear();
        self.motion_proposals.clear();
        self.transient_blast_cell_indices.clear();
        self.grid_fire_scratch.clear();
        self.completed_event_mutations.clear();
        self.diagnostics.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_02_tripartite_separation_and_rebuild() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);
        let mut derived = DerivedState::default();
        let mut scratch = TickScratch::default();

        // 1. 初始化派生 SDF 缓存
        derived.rebuild_sdf_if_needed(&world);
        assert!(derived.sdf.meta.as_ref().unwrap().is_up_to_date(&world));

        // 2. 地形修改产生变异，SDF 缓存必须判定失效
        world.terrain.mutate_cell(10, 10, 80.0, Some(2));
        assert!(!derived.sdf.meta.as_ref().unwrap().is_up_to_date(&world));

        // 3. 再次重建后恢复有效
        derived.rebuild_sdf_if_needed(&world);
        assert!(derived.sdf.meta.as_ref().unwrap().is_up_to_date(&world));

        // 4. 单 Tick 瞬态工作区写入与清空，不应影响持久状态
        scratch.begin_tick(1);
        scratch.motion_proposals.push((101, WorldPos::new(16.0, 16.0)));
        assert_eq!(scratch.motion_proposals.len(), 1);

        world.persistent_fields.smoke.insert(100, (0.8, 100.0));
        scratch.reset_scratch();
        assert!(scratch.motion_proposals.is_empty(), "Scratch must be empty after reset");
        assert!(
            world.persistent_fields.smoke.contains_key(&100),
            "Persistent smoke must survive scratch reset!"
        );
    }
}
