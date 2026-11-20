# GenTactix V2 Pure Engine 核心模块与 HTTP 接口速查手册

> **定位**：面向后续前端开发与内核维护的 GenTactix V2.0.0（排/班尺度 `8.0m` / `16s` 步长）真实代码与 HTTP API 速查对照手册（含 Phase B1~B4 完整契约）。  
> **完整前端联调 TypeScript 契约、差异对照表与九层美术规范**：见 [docs/FRONTEND_DEV_AND_ART_MANUAL.md](docs/FRONTEND_DEV_AND_ART_MANUAL.md)。

---

## 一 · `gentactix-core` 内核 15 模块总览（`crates/gentactix-core/src/`）

| 模块 | 文件 | 核心类型与职责 |
| :--- | :--- | :--- |
| **`types`** | `types.rs` | `TICK_SECONDS = 16`、`UnitId`、`PlatoonId`、`Team`、`UnitRole`、`MobilityKind`、`MoveStance`、`FireDiscipline`、`EngineerWorkKind`、`VisTier`、强类型 `UnitOrder` |
| **`math_ops`** | `math_ops/{geometry_8m, mod_a_g_field, mod_b_engineer, mod_c_i_field, mod_d_radio_zoh}.rs` | `GridGeometry`（`8.0m` / `OutsideBoundsError`）、`run_deterministic_multi_capacity_auction`、`compute_g_alloc`、`apply_g_work_consumption`、工兵三模态与 `AugmentedArcIntegrator` / `DetonatorSwitchEntity`、`ALPHA_BLAST` / `ALPHA_SUPP` / `ALPHA_INTEL` / `fuse_intel_log_survival` / `transition_vis_tier_hysteresis` / `SchmidtPostureState` / `project_orthogonal_cover`、`DeygoutRadioOperator` / `UnitZohDirectiveFilter` |
| **`catalog`** | `catalog.rs` | `ItemCatalog::embedded()` 读取 `data/items_catalog.json`，验证无 `range_m` 硬截断、构建预编译射表句柄 `AttackProfileIdx(u16)` 与 `compact_profile` 零堆解析 |
| **`damage_model`** | `damage_model.rs` | `SoldierRecord`（头/躯干/上肢/下肢 4 部位独立损伤与 `tick_bleeding_16s`）、`ArmorProfileRha`（前/侧/后/顶 4 向等效 RHA）、`VehicleDamageState`（8 核心子系统、`transmitted_flux: 0.0` 绝不过穿内崩落） |
| **`step_cache`** | `step_cache.rs` | `MAX_STEP_CACHE_HORIZON_TICKS = 324_000`（60 天）、`WorldInstanceRecord`（`timed_charge` / `mine` / `fortification` / `dropped_*` / `corpse`，创建即落盘）、`ScheduledStepEvent` / `ScheduledEventKind`、`WaveZoneSpec` / `ActiveWaveDirective` |
| **`terrain`** | `terrain.rs` | `TerrainKind`（8 种地貌）、`DirtyHaloBox`、`TerrainGrid`（`new_flat`、`mutate_cell`、`add_mine_density`、`decay_smoke`、`recompute_slope_horn`、`from_r16_bytes`、`from_r16_files`、`to_binary_stream`） |
| **`ruleset`** | `ruleset.rs` | `SimRuleset` 9 项布尔规则开关 + `hardcore_platoon()` / `tactical_standard()` / `arcade_sandbox()`；`ScenarioSpec`（`Flat` / `R16File` / `InlineGrid`）与 `build_world` |
| **`state`** | `state/{world, derived, scratch}.rs` | `WorldState`（权威状态、`world_instances`、`step_cache`、`team_i_intel`/`team_i_vis_tier`、`team_s_threat`、`authoritative_sha256`、`save_json`/`load_json`）、`DerivedState`（可重建空间桶代理、`bucket_max_crest_z`、`step_event_index`、G/I 双场、Deygout 衰减系数）、`TickScratch`（意图仲裁、`unit_start_baselines`、`motion_waypoints`、`blast_workspace`、Stage 12 强清） |
| **`clock`** | `clock/{phases, pathfinding, los_combat, ballistics_field, autonomy, orchestrator}.rs` | 13 阶段 `TickStage`、`TickOrchestrator::step_16s` / `step_n_ticks`、整数 8 向 A* `find_path_world`、`constrain_segment_motion`（$\le 3.5\text{m}$ 防穿隧）、8m DDA `check_los`、零 P2P 三维欧拉锥场 `DirectConeField` + 爆心场 `RadialBlastField`（8×4 微束二维角窗遮挡、`rayon` 批量并行）、`propose_autonomous_intents` |
| **`time_machine`** | `time_machine.rs` | `mix_time_unit_hash`、`query_time_lut`、5×16 品类常量表、`TimeMachinePendingQueue`、`TimeMachineFramePayload`（含 `unit_baselines`、`keyframes`、`stop_exact_ms`） |
| **`commands`** | `commands.rs` | `TacticalCommand`（18 个 `snake_case` `kind` 变体）、`CommandOutcome`（含 `remote_delay_ticks` / `via_runner`）、`validate_tactical_command_access`（远程电台/传令兵偏移代替一刀切拒单）、`execute_command_immediate` |
| **`preview`** | `preview.rs` | 纯只读 `&WorldState`：`preview_move_path`、`preview_fire_arc`、`preview_los_between`、`preview_engineer_work` |
| **`visibility`** | `visibility.rs` | `PRERENDER_SKIRT_MARGIN_M = 64.0`、`VisTier`（完全查表 `team_i_vis_tier`）、`build_observed_view`（含 `visible_instances` 与本方 `step_cache_queue`）、`build_observed_unit_detail`、`build_egress_playback_frame` |
| **`engine`** | `engine.rs` | 顶层 `PureEngine` 装配：`submit_command`（唯一入 `WorldState.step_cache`）、`step_16s`、`step_n_ticks`、`observe`、`observe_unit_detail`、`preview_*`、`save_json`/`load_json` |

---

## 二 · `TacticalCommand` 统一指令格式（`POST /api/command`）

JSON 使用内部标签 `"kind"`；当规则集开启 `enable_time_machine_deferral` 时，战术动作指令先经 `validate_tactical_command_access` 判定直接指挥半径或远程电台/黑障传令兵延迟（绝不粗暴拒单），再结合 `TimeCategory` 唯一一次计算目标拍 `target_tick = tick + delay_ticks + 1 + turn_skip` 落入 `WorldState.step_cache`（可附带 `"cmd_sec_in_tick": 1..=16`）；`StepCache` 最大支持 `60 天`（`324,000` 拍）。

| `kind` | 关键字段 | 类别 / 行为 |
| :--- | :--- | :--- |
| `move_unit` | `unit_id`, `waypoints: [{x,y}...]`, `stance?` | Movement（含远程延迟偏移） |
| `attack_pos` | `unit_id`, `target: {x,y}`, `fire_mode?` | Bullet（放行空地火力压制与三维走廊弹道） |
| `attack_unit` | `unit_id`, `target_id`, `chase?`, `fire_mode?` | Bullet（仅取瞄准参考点，由连续场结算遮挡与命中） |
| `hold_unit` | `unit_id` | Command |
| `set_fire_discipline` | `unit_id`, `discipline` | Command |
| `pop_smoke` | `unit_id`, `target: {x,y}` | Command |
| `build_work` | `unit_id`, `work: minefield\|foxhole_line\|zigzag_trench\|demolition`, `waypoints` | Event |
| `arm_detonator` | `unit_id`, `switch_pos`, `charge_positions`, `wire_polyline` | Event |
| `trigger_detonator` | `unit_id`, `switch_id`（必须撤离所有炸点 $\ge 40\text{m}$，经 `RadialBlastField` 引爆） | Event |
| `schedule_timed_charge` | `unit_id`, `pos: {x,y}`, `delay_ticks`, `blast_radius_m?`, `blast_kpa?`, `crater_depth_m?` | Event（当场建 `WorldInstance` 并入 `step_cache[tick + delay_ticks + ...]`） |
| `cancel_step_event` | `event_id` | 即时（按 `DerivedState::step_event_index` 定点注销） |
| `issue_commander_wave` | `team`, `hq_unit_id?`, `delay_ticks`, `zones: [WaveZoneSpec...]` | Command（激活炮击 / 友军冲锋 / 许可 AO 波次） |
| `spawn_platoon` | `name`, `team: red\|blue\|neutral` | 即时 |
| `spawn_unit` | `spec: UnitSpawnSpec`（可选 `armor_profile: ArmorProfileRha`） | 即时（自动建档单兵 4 部位 / 车辆 8 部件） |
| `despawn_unit` | `unit_id` | 即时 |
| `edit_terrain` | `cell: {r,c}`, `elev_delta?`, `terrain_kind?`, `cover_add?`, `smoke_add?` | 即时（受 `allow_runtime_map_edit` 门禁） |
| `set_ruleset` | `ruleset: SimRuleset` | 即时 |
| `set_night` | `night: bool` | 即时 |

---

## 三 · 无头 HTTP 接口清单（`crates/gentactix-server/src/routes.rs`，代码严格 `< 400` 行）

默认监听 `127.0.0.1:8767`（可通过 `--listen`、`--rows`、`--cols` 启动参数指定）：

| 类别 | 方法与路径 | 入参 / 查询参数 | 返回说明 |
| :--- | :--- | :--- | :--- |
| **健康** | `GET /health` | — | `{ status: "ok", version: "2.0.0", cell_m: 8.0, tick_s: 16 }` |
| **建图** | `POST /api/world/create` | `{ rows, cols, base_elevation_m?, ruleset? }` | 新建空白 8m 世界并返回 `authoritative_sha256` |
| **剧本** | `POST /api/world/scenario` | `ScenarioSpec` JSON | 加载 `Flat`/`R16File`/`InlineGrid` 剧本 |
| **存档** | `GET /api/world/save` | — | 导出完整确定性 `WorldState` JSON（含 `step_cache`、`world_instances`、`team_i_intel`、`soldiers`/`vehicle_damage`） |
| **读档** | `POST /api/world/load` | 原始存档 JSON 字符串 | 还原世界与派生缓存并返回 `tick` 与 SHA-256 |
| **查规则** | `GET /api/ruleset` | — | 返回当前 `SimRuleset` 9 项开关 |
| **切规则** | `PUT /api/ruleset` | `SimRuleset` JSON | 热更新规则开关 |
| **自描述** | `GET /api/schema` | — | 导出全部 18 种指令集、预设规则与端点清单 |
| **射表** | `GET /api/catalog` | — | 返回 `items_catalog.json` 全部武器弹药引信枪架 |
| **下令** | `POST /api/command` | `TacticalCommand` + 可选 `cmd_sec_in_tick` | 返回 `CommandOutcome`（含 `deferred_until_tick`、`remote_delay_ticks`、`via_runner`、`created_id`） |
| **快照** | `GET /api/state` | `?team=red\|blue\|neutral` | 返回经迷雾裁剪的 `ObservedStateView`（含 `units`、`visible_instances` 与本方可见 `step_cache_queue` 摘要） |
| **查单兵/车** | `GET /api/units/:id` | `?team=red\|blue\|neutral` | 返回可见单位及按迷雾裁剪后的单兵 4 部位 `soldiers` / 车辆 8 部件 `vehicle_damage`；`Hidden` 回 `404` |
| **地形流** | `GET /api/terrain.bin` | — | 字节流：24B 头（`GTXV2TER`+`ver`+`rows`+`cols`+`8.0`）+ `elev`/`kind`/`cover`/`smoke` |
| **双场** | `GET /api/fields/:kind` | `:kind` 为 `i` 或 `g`（`i` 支持 `?team=red\|blue`） | `i` 返回正交三元组 `{ observer_team, values, i_intel, i_vis_tier, h_blast, i_threat }`；`g` 返回 `g_field_cells` |
| **路径预览** | `POST /api/query/plan` | `{ unit_id, goal, stance? }` | 只读 A* 路径与耗时预估 |
| **通视预览** | `POST /api/query/los` | `{ from, to, max_vision_m? }` | 只读两点间 DDA 视距与山脊/烟雾阻断详情 |
| **射界预览** | `POST /api/query/arc` | `{ unit_id, center_angle_deg, half_arc_deg, ray_count?, max_dist_m? }` | 只读扇形多射线遮挡边界采样 |
| **工事预览** | `POST /api/query/work` | `{ unit_id, work, control_points }` | 只读多边形/折线工兵实例点位与工时预估 |
| **推进时钟** | `POST /api/clock/step` | `{ ticks?, observer_team? }` | 推进 $1..=64$ 拍（每拍 16s），返回每拍 `report` 与 `[0..16000ms]` `playback`（含 `unit_baselines`、分段 `keyframes`、阵亡/失能 `stop_exact_ms` 截断、`playback_events` 及 `is_prerender_skirt`） |
