# GenTactix V2 Pure Engine 核心模块与 HTTP 接口速查手册

> **定位**：面向后续前端开发与内核维护的 GenTactix V2.0.0（排/班尺度 `8.0m` / `16s` 步长）真实代码与 HTTP API 对照手册。

---

## 一 · `gentactix-core` 内核 12 模块总览（`crates/gentactix-core/src/`）

| 模块 | 文件 | 核心类型与职责 |
| :--- | :--- | :--- |
| **`types`** | `types.rs` | `TICK_SECONDS = 16`、`UnitId`、`PlatoonId`、`Team`、`UnitRole`、`MobilityKind`、`MoveStance`、`FireDiscipline`、`EngineerWorkKind`、强类型 `UnitOrder` |
| **`math_ops`** | `math_ops/{geometry_8m, mod_a_g_field, mod_b_engineer, mod_c_i_field, mod_d_radio_zoh}.rs` | `GridGeometry`（`8.0m` / `OutsideBoundsError`）、`run_deterministic_multi_capacity_auction`、`compute_g_alloc`、`apply_g_work_consumption`、工兵三模态与 `AugmentedArcIntegrator` / `DetonatorSwitchEntity`、`ALPHA_BLAST` / `ALPHA_SUPP` / `SchmidtPostureState` / `project_orthogonal_cover`、`DeygoutRadioOperator` / `UnitZohDirectiveFilter` |
| **`catalog`** | `catalog.rs` | `ItemCatalog::embedded()` 读取 `data/items_catalog.json`，验证无 `range_m` 硬截断、解析 `resolve_attack_profile` |
| **`terrain`** | `terrain.rs` | `TerrainKind`（8 种地貌）、`DirtyHaloBox`、`TerrainGrid`（`new_flat`、`mutate_cell`、`add_mine_density`、`decay_smoke`、`recompute_slope_horn`、`from_r16_bytes`、`from_r16_files`、`to_binary_stream`） |
| **`ruleset`** | `ruleset.rs` | `SimRuleset` 9 项布尔规则开关 + `hardcore_platoon()` / `tactical_standard()` / `arcade_sandbox()`；`ScenarioSpec`（`Flat` / `R16File` / `InlineGrid`）与 `build_world` |
| **`state`** | `state/{world, derived, scratch}.rs` | `WorldState`（权威状态、`authoritative_sha256`、`save_json`/`load_json`）、`DerivedState`（可重建空间桶、G/I 双场、Deygout 衰减系数）、`TickScratch`（意图仲裁、`blast_workspace`、Stage 12 强清） |
| **`clock`** | `clock/{phases, pathfinding, los_combat, autonomy, orchestrator}.rs` | 13 阶段 `TickStage`、`TickOrchestrator::step_16s` / `step_n_ticks`、整数 8 向 A* `find_path_world`、`constrain_segment_motion`（$\le 3.5\text{m}$ 防穿隧）、8m 全尺寸 DDA `check_los`、`resolve_fire_exchange`、`propose_autonomous_intents` |
| **`time_machine`** | `time_machine.rs` | `mix_time_unit_hash`、`query_time_lut`、5×16 品类常量表、`TimeMachinePendingQueue`、`TimeMachineFramePayload` |
| **`commands`** | `commands.rs` | `TacticalCommand`（15 个 `snake_case` `kind` 变体）、`CommandOutcome`、`validate_tactical_command_access`、`execute_command_immediate` |
| **`preview`** | `preview.rs` | 纯只读 `&WorldState`：`preview_move_path`、`preview_fire_arc`、`preview_los_between`、`preview_engineer_work` |
| **`visibility`** | `visibility.rs` | `PRERENDER_SKIRT_MARGIN_M = 64.0`、`VisTier`、`build_observed_view`、`build_egress_playback_frame` |
| **`engine`** | `engine.rs` | 顶层 `PureEngine` 装配：`submit_command`、`step_16s`、`step_n_ticks`、`observe`、`preview_*`、`save_json`/`load_json` |

---

## 二 · `TacticalCommand` 统一指令格式（`POST /api/command`）

JSON 使用内部标签 `"kind"`；当规则集开启 `enable_time_machine_deferral` 时，战术动作指令会按 `TimeCategory` 挂入时间机并于未来拍交界激活（可附带 `"cmd_sec_in_tick": 1..=16`），建造/规则类指令立即生效：

| `kind` | 关键字段 | 类别 |
| :--- | :--- | :--- |
| `move_unit` | `unit_id`, `waypoints: [{x,y}...]`, `stance?` | Movement |
| `attack_pos` | `unit_id`, `target: {x,y}`, `fire_mode?` | Bullet |
| `attack_unit` | `unit_id`, `target_id`, `chase?`, `fire_mode?` | Bullet |
| `hold_unit` | `unit_id` | Command |
| `set_fire_discipline` | `unit_id`, `discipline` | Command |
| `pop_smoke` | `unit_id`, `target: {x,y}` | Command |
| `build_work` | `unit_id`, `work: minefield\|foxhole_line\|zigzag_trench\|demolition`, `waypoints` | Event |
| `arm_detonator` | `unit_id`, `switch_pos`, `charge_positions`, `wire_polyline` | Event |
| `trigger_detonator` | `unit_id`, `switch_id`（必须撤离所有炸点 $\ge 40\text{m}$） | Event |
| `spawn_platoon` | `name`, `team: red\|blue\|neutral` | 即时 |
| `spawn_unit` | `spec: UnitSpawnSpec` | 即时 |
| `despawn_unit` | `unit_id` | 即时 |
| `edit_terrain` | `cell: {r,c}`, `elev_delta?`, `terrain_kind?`, `cover_add?`, `smoke_add?`（受 `allow_runtime_map_edit` 门禁） | 即时 |
| `set_ruleset` | `ruleset: SimRuleset` | 即时 |
| `set_night` | `night: bool` | 即时 |

---

## 三 · 无头 HTTP 接口清单（`crates/gentactix-server/src/routes.rs`）

默认监听 `127.0.0.1:8767`（可通过 `--listen`、`--rows`、`--cols` 启动参数指定）：

| 类别 | 方法与路径 | 入参 / 查询参数 | 返回说明 |
| :--- | :--- | :--- | :--- |
| **健康** | `GET /health` | — | `{ status: "ok", version: "2.0.0", cell_m: 8.0, tick_s: 16 }` |
| **建图** | `POST /api/world/create` | `{ rows, cols, base_elevation_m?, ruleset? }` | 新建空白 8m 世界并返回 `authoritative_sha256` |
| **剧本** | `POST /api/world/scenario` | `ScenarioSpec` JSON | 加载 `Flat`/`R16File`/`InlineGrid` 剧本 |
| **存档** | `GET /api/world/save` | — | 导出完整确定性 `WorldState` JSON |
| **读档** | `POST /api/world/load` | 原始存档 JSON 字符串 | 还原世界与派生缓存并返回 `tick` 与 SHA-256 |
| **查规则** | `GET /api/ruleset` | — | 返回当前 `SimRuleset` 9 项开关 |
| **切规则** | `PUT /api/ruleset` | `SimRuleset` JSON | 热更新规则开关 |
| **自描述** | `GET /api/schema` | — | 导出支持指令集、预设规则与端点清单 |
| **射表** | `GET /api/catalog` | — | 返回 `items_catalog.json` 全部武器弹药引信枪架 |
| **下令** | `POST /api/command` | `TacticalCommand` + 可选 `cmd_sec_in_tick` | 统一返回 `CommandOutcome`（含是否延期及 `created_id`） |
| **快照** | `GET /api/state` | `?team=red\|blue\|neutral` | 返回经迷雾与字段裁剪后的 `ObservedStateView` |
| **查单兵** | `GET /api/units/:id` | `?team=red\|blue\|neutral` | 返回单个可见单位详情；对 `Hidden`/缓冲环敌军回 `404` |
| **地形流** | `GET /api/terrain.bin` | — | 字节流：24B 头（`GTXV2TER`+`ver`+`rows`+`cols`+`8.0`）+ `elev`/`kind`/`cover`/`smoke` |
| **双场** | `GET /api/fields/:kind` | `:kind` 为 `i` 或 `g` | 返回当前 `i_field_threat` 全图数组或 `g_field_cells` 分配态 |
| **路径预览** | `POST /api/query/plan` | `{ unit_id, goal, stance? }` | 只读 A* 路径与耗时预估 |
| **通视预览** | `POST /api/query/los` | `{ from, to, max_vision_m? }` | 只读两点间 DDA 视距与山脊/烟雾阻断详情 |
| **射界预览** | `POST /api/query/arc` | `{ unit_id, center_angle_deg, half_arc_deg, ray_count?, max_dist_m? }` | 只读扇形多射线遮挡边界采样 |
| **工事预览** | `POST /api/query/work` | `{ unit_id, work, control_points }` | 只读多边形/折线工兵实例点位与工时预估 |
| **推进时钟** | `POST /api/clock/step` | `{ ticks?, observer_team? }` | 推进 $1..=64$ 拍（每拍 16s），返回每拍 `report` 与 `[0..16000ms]` 时间机 `playback`（含 `is_prerender_skirt` 缓冲环） |
