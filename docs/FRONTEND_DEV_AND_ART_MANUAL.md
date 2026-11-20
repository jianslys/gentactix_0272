# GenTactix V2 前端开发接口与九层美术生产手册

> **定位**：面向 `frontend/`（React 19 + TypeScript + Canvas 2D 九层栈）与后端 `gentactix-server` / `gentactix-core`（Phase B1~B4 完整闭环版）的权威前端联调接口、TypeScript 类型契约、二进制流解析、时间演出机走表算法与美术图层生产规范。  
> **快速后端索引**：[FUNCTION_AND_INTERFACE_MANUAL.md](../FUNCTION_AND_INTERFACE_MANUAL.md) · **业务规格书**：[SPEC_UIO_CLIPBOARD_AND_WAVE_DIRECTIVE_CODEX.md](SPEC/SPEC_UIO_CLIPBOARD_AND_WAVE_DIRECTIVE_CODEX.md)

---

## 一 · 合并后双端联调与启动环境

### 1.1 启动命令与端口配置

1. **启动真后端 HTTP 服务**（瘦薄无头壳，默认监听 `127.0.0.1:8767`）：
   ```powershell
   Set-Location D:\tools\gentactix
   cargo run --release -p gentactix-server -- --listen 127.0.0.1:8767 --rows 64 --cols 64
   ```
   - 当前后端 [crates/gentactix-server/src/](../crates/gentactix-server/src/routes.rs) 总源码仅 `282` 行（硬红线 `< 400` 行），刻意未内置 CORS 头与重型中间件。
   - 因此前端本地开发使用 Vite Dev Server 反向代理 `/api` 与 `/health` 同源转发至 `127.0.0.1:8767`（生产托管亦由同域反代转发）。

2. **前端 [frontend/vite.config.ts](../frontend/vite.config.ts) 联调代理模板**：
   ```ts
   server: {
     proxy: {
       '/api': { target: 'http://127.0.0.1:8767', changeOrigin: true },
       '/health': { target: 'http://127.0.0.1:8767', changeOrigin: true },
     },
   }
   ```
   代理生效后，前端构造 `new HttpEngineAdapter({ baseUrl: '' })`（或同源相对路径），避免浏览器跨域拦截；若以独立 Node/Playwright 或测试进程直连，传 `baseUrl: 'http://127.0.0.1:8767'`。

3. **前端依赖与构建命令**（目录：`frontend/`）：
   ```powershell
   Set-Location D:\tools\gentactix\frontend
   npm install          # 当前合并目录尚未带 node_modules/，首跑前先装包
   npm run dev          # http://127.0.0.1:5173
   npm run typecheck    # tsc --noEmit -p tsconfig.json
   npm test             # vitest run
   npm run build        # tsc && vite build -> dist/
   npm run e2e          # playwright test
   ```

---

### 1.2 已合入前端与 Phase B1~B4 后端的差异对齐清单

合入自远端 `origin/v2/pure-engine` 的前端骨架已在 [frontend/src/engine/localMockSandbox.ts](../frontend/src/engine/localMockSandbox.ts) 跑通纯前端闭环，但 [frontend/src/engine/httpEngineAdapter.ts](../frontend/src/engine/httpEngineAdapter.ts) 与 [frontend/src/engine/types.ts](../frontend/src/engine/types.ts) 尚属早期占位插座。切换接入真后端时，需对齐以下 **8 处真实协议差异**：

| 序号 | 前端现有文件/占位写法 | 真实后端 Phase B1~B4 契约 | 对齐动作 |
| :---: | :--- | :--- | :--- |
| **1** | `vite.config.ts` 无 `server.proxy` | `gentactix-server` 无 CORS 响应头 | 在 `vite.config.ts` 增加 `/api` 与 `/health` 代理 |
| **2** | `httpEngineAdapter.loadInitial` 硬编码 `512x512` 空地形 | `GET /api/terrain.bin`（`24B GTXV2TER` + 全格高程/地貌/掩蔽/烟幕）及 `POST /api/world/scenario` | 初始化拉取 `/api/terrain.bin` 解码真实高程、水域、林地、道路、桥梁与建筑格 |
| **3** | `httpEngineAdapter.getSnapshot` 忽略 `visible_instances` 与 `step_cache_queue` | `GET /api/state?team=...` 直接返回 `visible_instances`（按 `I_intel` 裁切）与本方 `step_cache_queue` | 映射 `visible_instances` 至散落物/尸体/炸药/工事，直接喂 `step_cache_queue` 给 UIO 前向预警 |
| **4** | `httpEngineAdapter.issueQueue` 丢弃 `CommandOutcome` 返回值 | `POST /api/command` 返回真实 `deferred_until_tick`、`ticket_id`、`remote_delay_ticks`、`via_runner` | 解析回包回填 `DirectiveIssueReceipt`，不要写死 `cachedTick + 1` 或 `remote: false` |
| **5** | `httpEngineAdapter.issueWave` 直接 `throw` 未实现 | `POST /api/command` 已原生支持 `{ kind: "issue_commander_wave", team, hq_unit_id?, delay_ticks, zones }` | 按 `WaveGeometry` 转 `WaveZoneSpec[]` 调 `/api/command` |
| **6** | `httpEngineAdapter.advanceTick` 丢弃 `/api/clock/step` 返回的 `playback` | `POST /api/clock/step` 原生返回 `steps[i].playback`（含 `unit_baselines`、`particle_tracks`、`keyframes`、`stop_exact_ms`、`playback_events`） | 直接把 `steps[0].playback` 转成逐秒防剧透 `TickPlayback`，禁止只靠回合末快照补全 |
| **7** | 前端 `UnitRole` / `TacticalCommandJson` 为简化子集（缺 `armor_profile` 展示、`schedule_timed_charge` 等） | 后端 `UnitRole` 含 6 种角色，`TacticalCommand` 含 18 种变体，且 `/api/units/:id` 返回迷雾裁剪后 `soldiers`（4 部位）与 `vehicle_damage`（8 部件） | 扩充前端单位检视器与适配器类型，区分步兵 4 部位与装甲车辆 8 子系统 |
| **8** | 前端缺双场认知热图与物理只读预览对接 | `GET /api/fields/i?team=...`（返回 `i_intel` / `i_vis_tier` / `h_blast` / `i_threat`）、`GET /api/fields/g`、`POST /api/query/{plan,los,arc,work}` | 将 `UnitInspector` / Layer 5 射界与路径预览直连后端只读 `/api/query/*` 端点 |

---

## 二 · 全部 HTTP 端点与 TypeScript 接口契约

### 2.1 无头 HTTP 路由总表（19 个端点）

| 方法 | 路径 | Query / Body 入参 | 成功响应 / 说明 |
| :--- | :--- | :--- | :--- |
| `GET` | `/health` | — | `{ status: "ok", version: "2.0.0", cell_m: 8.0, tick_s: 16 }` |
| `POST` | `/api/world/create` | `{ rows, cols, base_elevation_m?, ruleset? }` | 新建空白世界（`rows/cols in 1..=4096`），返回 `authoritative_sha256` |
| `POST` | `/api/world/scenario` | `ScenarioSpec` JSON | 加载 `Flat` / `R16File` / `InlineGrid` 剧本并建排、建单位、建目标点 |
| `GET` | `/api/world/save` | — | 导出完整权威存档 JSON（含 `step_cache`、`world_instances`、`team_i_intel` 等） |
| `POST` | `/api/world/load` | 原始存档 JSON 字符串 | 加载存档并重建 `DerivedState`，返回 `{ ok, tick, authoritative_sha256 }` |
| `GET` | `/api/ruleset` | — | 读取当前 9 项布尔规则 `SimRuleset` |
| `PUT` | `/api/ruleset` | `SimRuleset` JSON | 提交 `SetRuleset` 热更新 9 项开关并返回 `CommandOutcome` |
| `GET` | `/api/schema` | — | 引擎元信息、18 种 `command_kinds`、3 套预设 `ruleset_presets` 与端点清单 |
| `GET` | `/api/catalog` | — | 返回 `items_catalog.json` 全部武器、弹药、引信、枪架条目 |
| `POST` | `/api/command` | `TacticalCommand` + 可选 `cmd_sec_in_tick: 1..=16` | 执行/入队统一战术指令，返回 `CommandOutcome` |
| `GET` | `/api/state` | `?team=red\|blue\|neutral`（默认 `red`） | 返回 `ObservedStateView`（可见单位、可见实例、`step_cache_queue`） |
| `GET` | `/api/units/:id` | `?team=red\|blue\|neutral` | 返回迷雾裁剪后 `VisibleUnitView`（含 `soldiers` 4 部位 / `vehicle_damage` 8 部件）；`Hidden` 回 `404` |
| `GET` | `/api/terrain.bin` | — | `application/octet-stream` 二进制地形流（见 2.4 节逐字节解析） |
| `GET` | `/api/fields/:kind` | `:kind` 为 `i` 或 `g`；`i` 支持 `?team=red\|blue` | `i` 返回正交 I 场三元组；`g` 返回 `g_field_cells`；其余 `:kind` 回 `400` |
| `POST` | `/api/query/plan` | `{ unit_id, goal, stance? }` | 只读 A* 路径与时间预估（不改 `WorldState` / `SHA-256`） |
| `POST` | `/api/query/los` | `{ from, to, max_vision_m? }`（默认 `600.0`） | 只读两点 DDA 视线遮蔽查询 |
| `POST` | `/api/query/arc` | `{ unit_id, center_angle_deg, half_arc_deg, ray_count?, max_dist_m? }` | 只读扇形射界边界多射线采样（默认 `ray_count=16`, `max_dist_m=600.0`） |
| `POST` | `/api/query/work` | `{ unit_id, work, control_points }` | 只读多边形/折线工兵点位生成与人秒工时预估 |
| `POST` | `/api/clock/step` | `{ ticks?, observer_team? }`（`ticks` 钳位 `1..=64`，默认 `1`） | 推进 `16s` 时钟，返回 `{ tick, clock_seconds, steps: EngineStepOutput[] }` |

---

### 2.2 完整 TypeScript 类型契约（1:1 对齐 Rust `serde`）

> 所有布尔与枚举一律使用 `snake_case`，世界坐标 `WorldPos = { x: number; y: number }` 单位为米（`m`），格尺寸常量 `TACTICAL_CELL_SIZE_M = 8.0`，单拍常量 `TICK_SECONDS = 16`。

```ts
export type Team = 'red' | 'blue' | 'neutral';
export type VisTier = 'hidden' | 'suspected' | 'spotted' | 'full';
export type UnitRole =
  | 'platoon_hq'
  | 'infantry_squad'
  | 'heavy_weapon_team'
  | 'engineer_squad'
  | 'armored_vehicle'
  | 'recon_team';
export type MobilityKind = 'foot' | 'wheeled' | 'tracked';
export type MoveStance = 'normal' | 'fast' | 'cautious';
export type FireDiscipline = 'hold_fire' | 'return_fire' | 'fire_at_will' | 'suppressive';
export type EngineerWorkKind = 'minefield' | 'foxhole_line' | 'zigzag_trench' | 'demolition';
export type TerrainKindName =
  | 'plain'
  | 'forest'
  | 'urban'
  | 'water'
  | 'swamp'
  | 'mountain'
  | 'road'
  | 'bridge';
export type SchmidtPostureState = 'active' | 'pinned' | 'prone';

export interface WorldPos { x: number; y: number; }
export interface CellCoord { r: number; c: number; }

export interface SimRuleset {
  enforce_command_radius: boolean;
  enforce_platoon_cohesion: boolean;
  enforce_radio_deygout: boolean;
  enforce_suppression_posture: boolean;
  enforce_ammo_consumption: boolean;
  enforce_fog_of_war: boolean;
  enable_g_field_autonomy: boolean;
  enable_time_machine_deferral: boolean;
  allow_runtime_map_edit: boolean;
}

export type UnitOrder =
  | { kind: 'hold' }
  | { kind: 'move'; waypoints: WorldPos[]; stance?: MoveStance }
  | { kind: 'attack_pos'; target: WorldPos; fire_mode?: FireDiscipline }
  | { kind: 'attack_unit'; target: number; chase?: boolean; fire_mode?: FireDiscipline }
  | { kind: 'build_work'; work: EngineerWorkKind; waypoints: WorldPos[] }
  | { kind: 'pop_smoke'; target: WorldPos };

// ---------------- 伤害模型明细（仅友军 / 开放规则的 GET /api/units/:id 下发） ----------------

export interface BodyRegionState {
  integrity: number;         // 0.0..=1.0
  cumulative_damage: number;
}

export interface SoldierRecord {
  soldier_id: number;
  alive: boolean;
  hp: number;
  max_hp: number;
  /** 固定 4 元组：[0]=Head, [1]=Torso, [2]=UpperLimbs, [3]=LowerLimbs */
  regions: [BodyRegionState, BodyRegionState, BodyRegionState, BodyRegionState];
  bleeding_loss: number;
  bleed_rate_per_tick: number;
  suppression: number;
}

export interface ArmorProfileRha {
  front_mm: number;
  side_mm: number;
  rear_mm: number;
  top_mm: number;
}

export interface VehicleDamageState {
  armor_rha: ArmorProfileRha;
  /** 固定 8 元组：[0]=Engine, [1]=Tracks, [2]=MainGun, [3]=TurretRing, [4]=Optics, [5]=Radio, [6]=AmmoRack, [7]=FuelTank */
  subsystems: [number, number, number, number, number, number, number, number];
  crew_alive: number;
  crew_max: number;
  ammo_cooked_off: boolean;
  burning: boolean;
}

// ---------------- 态势观测（GET /api/state 与 GET /api/units/:id） ----------------

export interface VisibleUnitView {
  id: number;
  callsign: string;
  team: Team;
  platoon_id?: number;
  role?: UnitRole;
  pos: WorldPos;              // Suspected 时已按 16m 粗格中心脱敏
  facing_deg: number;
  alive: boolean;
  vis_tier: VisTier;
  men?: number;               // Full 可见；Spotted/Suspected 裁剪
  hp?: number;
  suppression?: number;
  posture?: SchmidtPostureState;
  weapon_id?: string;
  ammo_rounds?: number;       // 仅友军或关闭迷雾下发；敌军即使 Full 也裁剪为 undefined
  order?: UnitOrder;          // 同上，防泄漏敌军企图
  is_vehicle?: boolean;
  /** 仅 GET /api/units/:id 在友军或非迷雾模式返回 */
  soldiers?: SoldierRecord[];
  /** 仅 GET /api/units/:id 在友军或非迷雾模式返回 */
  vehicle_damage?: VehicleDamageState;
}

export type WorldInstanceKind =
  | 'timed_charge'
  | 'mine'
  | 'fortification'
  | 'dropped_item'
  | 'dropped_weapon'
  | 'corpse';

export interface VisibleInstanceView {
  instance_id: number;
  kind: WorldInstanceKind;
  team: Team;
  pos: WorldPos;
  vis_tier: VisTier;
  label?: string;             // Spotted 及以上返回
  owner_unit_id?: number;     // 仅本方 Full 返回
}

export interface ScheduledEventSummary {
  event_id: number;
  source_tick: number;
  target_tick: number;
  sec_in_tick: number;        // 1..=16
  exact_ms: number;           // 1..=16000
  kind: 'task_command' | 'detonation' | 'g_field_wave';
  subject_unit_id?: number;
  target_pos?: WorldPos;
  label?: string;
}

export interface ObjectiveSpec {
  id: string;
  name: string;
  center: WorldPos;
  radius_m: number;
  owner?: Team;
}

export interface ObservedStateView {
  tick: number;
  clock_seconds: number;
  observer_team: Team;
  fog_enforced: boolean;
  units: VisibleUnitView[];
  visible_instances: VisibleInstanceView[];
  /** 严格按迷雾过滤，仅包含本方（及非迷雾下的 Neutral）未来 StepCache 事件 */
  step_cache_queue: ScheduledEventSummary[];
  objectives: ObjectiveSpec[];
  authoritative_sha256: string;
}

// ---------------- 双场（GET /api/fields/i & GET /api/fields/g） ----------------

export interface IFieldTripletResponse {
  kind: 'i';
  observer_team: Team;
  rows: number;
  cols: number;
  cell_m: number;             // 8.0
  values: number[];           // 与 i_threat 一致（兼容视图），长度 rows * cols
  i_intel: number[];          // 0.0..=1.0 认知概率通道
  i_vis_tier: VisTier[];      // 经 0.05 迟滞门限输出的分格可见等级
  h_blast: number[];          // 冲击波/弹坑危险源（α=0.1690）
  i_threat: number[];         // 直射火线与压制威胁源（含山脊/坦克背坡死角）
}

export interface GFieldCell {
  g0: number;
  committed_en_route: number;
  actual_on_site: number;
  kind: 'state' | 'work';
}

export interface GFieldResponse {
  kind: 'g';
  rows: number;
  cols: number;
  cell_m: number;
  cells: Record<string, GFieldCell>; // Key 为格线性索引 string(r * cols + c)
}

// ---------------- 推进与时间演化演出载荷（POST /api/clock/step） ----------------

export type TickStage =
  | 'begin_tick'
  | 'clock_and_schedule'
  | 'persistent_field_evolve'
  | 'derived_refresh'
  | 'sense_and_arbitrate'
  | 'propose_motion'
  | 'constrain_motion'
  | 'commit_motion'
  | 'emit_events'
  | 'resolve_events'
  | 'commit_persistent'
  | 'validate'
  | 'publish_and_drop';

export interface CombatLogEntry {
  tick: number;
  actor_id: number;
  target_id?: number;
  kind: string;               // 'fire' | 'bleed_out' | 'mine_trigger' | ...
  dist_m: number;
  casualties: number;
  hp_loss: number;
  suppression_add: number;
  ammo_spent: number;
  message: string;
}

export interface UnitStartBaseline {
  hp: number;
  men: number;
  ammo_rounds: number;
  suppression: number;
  eff_speed_scale: number;
}

export interface TickReport {
  tick: number;
  clock_seconds: number;
  stages_executed: TickStage[];
  moved_units: number;
  fire_events: number;
  engineer_events: number;
  smoke_events: number;
  mine_events: number;
  casualties_total: number;
  prior_positions: Record<string, WorldPos>;
  unit_start_baselines: Record<string, UnitStartBaseline>;
  motion_waypoints: Record<string, WorldPos[]>;
  tick_logs: CombatLogEntry[];
  authoritative_sha256: string;
}

export interface UnitTickBaseline {
  piece_id: string;
  team: Team;
  hp_start?: number;
  men_start?: number;
  ammo_start?: number;
  suppression_start?: number;
}

export interface ParticleTrackNode {
  piece_id: string;
  team: Team;
  pos_start: [number, number];
  pos_end: [number, number];
  initial_visibility: 0 | 1;
  is_prerender_skirt: boolean;
  /** 按 [0..stop_ms_or_16000] 有序排列的多段拐点坐标（解决过桥/绕障直穿） */
  keyframes: Array<[number, [number, number]]>;
  /** 单位拍中阵亡或失能时的截断毫秒（1..=16000），达到后前端停止位移动画 */
  stop_exact_ms?: number;
}

export interface PlaybackEvent {
  exact_ms: number;
  category: 'Bullet' | 'Shell' | 'Command' | 'Event' | 'Movement';
  event_kind: string;
  source_piece_id?: string;
  target_piece_id?: string;
  exact_hp_loss?: number;
  exact_supp_add?: number;
  hp_delta?: number;
  casualties?: number;
  ammo_spent?: number;        // 仅友军或关闭迷雾下发
  terrain_cell?: [number, number];
  entity_id?: string;
  directive_id?: string;
}

export interface TimeMachineFramePayload {
  tick: number;
  window_ms: [number, number]; // [0, 16000]
  unit_baselines: UnitTickBaseline[];
  particle_tracks: ParticleTrackNode[];
  playback_events: PlaybackEvent[];
}

export interface EngineStepOutput {
  report: TickReport;
  playback: TimeMachineFramePayload;
  activated_commands: CommandOutcome[];
}
```

---

### 2.3 18 种 `TacticalCommand`（`POST /api/command`）与只读预览响应

调用 `POST /api/command` 时可在顶层 JSON 附带 `"cmd_sec_in_tick": 1..=16`（默认 `1`），供唯一外置时间机做 `(16 - t_cmd)` 混淆计算；统一返回 `CommandOutcome`：

```ts
export interface CommandOutcome {
  accepted: boolean;
  deferred_until_tick?: number;
  ticket_id?: number;
  /** 超出直接指挥半径时的远程偏移拍数（前端UI纸条据此明牌展示延迟） */
  remote_delay_ticks?: number;
  /** true 表示无线电被山体黑障压制，已自动降级为徒步传令兵（4m/s） */
  via_runner?: boolean;
  code: string;
  detail: string;
  created_id?: number;
}
```

#### 全部 18 种统一指令变体（`kind`）

```ts
export type WaveZoneKind = 'bombardment' | 'assault' | 'permitted_ao';

export interface WaveZoneSpec {
  kind: WaveZoneKind;
  center: WorldPos;
  radius_m: number;
  waypoints?: WorldPos[];
  g0_weight?: number; // 默认 1.0
}

export interface UnitSpawnSpec {
  callsign: string;
  team: Team;
  platoon_id?: number;
  role?: UnitRole;
  mobility?: MobilityKind;
  pos: WorldPos;
  facing_deg?: number;
  men?: number;
  hp?: number;
  armor_mm?: number;          // 自动映射 front=1.0x / side=0.60x / rear=0.35x / top=0.25x RHA
  weapon_id?: string;
  ammo_id?: string;
  fuze_id?: string;
  mount_ids?: string[];
  ammo_rounds?: number;
  smoke_grenades?: number;
  base_speed_mps?: number;
  vision_m?: number;
  base_radio_range_m?: number;
}

export type TacticalCommand =
  | { kind: 'move_unit'; unit_id: number; waypoints: WorldPos[]; stance?: MoveStance }
  | { kind: 'attack_pos'; unit_id: number; target: WorldPos; fire_mode?: FireDiscipline }
  | { kind: 'attack_unit'; unit_id: number; target_id: number; chase?: boolean; fire_mode?: FireDiscipline }
  | { kind: 'hold_unit'; unit_id: number }
  | { kind: 'set_fire_discipline'; unit_id: number; discipline: FireDiscipline }
  | { kind: 'pop_smoke'; unit_id: number; target: WorldPos }
  | { kind: 'build_work'; unit_id: number; work: EngineerWorkKind; waypoints: WorldPos[] }
  | { kind: 'arm_detonator'; unit_id: number; switch_pos: WorldPos; charge_positions: WorldPos[]; wire_polyline: WorldPos[] }
  | { kind: 'trigger_detonator'; unit_id: number; switch_id: number }
  | { kind: 'schedule_timed_charge'; unit_id: number; pos: WorldPos; delay_ticks: number; blast_radius_m?: number; blast_kpa?: number; crater_depth_m?: number }
  | { kind: 'cancel_step_event'; event_id: number }
  | { kind: 'issue_commander_wave'; team: Team; hq_unit_id?: number; delay_ticks: number; zones: WaveZoneSpec[] }
  | { kind: 'spawn_platoon'; name: string; team: Team }
  | { kind: 'spawn_unit'; spec: UnitSpawnSpec }
  | { kind: 'despawn_unit'; unit_id: number }
  | { kind: 'edit_terrain'; cell: CellCoord; elev_delta?: number; terrain_kind?: TerrainKindName; cover_add?: number; smoke_add?: number }
  | { kind: 'set_ruleset'; ruleset: SimRuleset }
  | { kind: 'set_night'; night: boolean };
```

> **踩坑警示（真后端校验实测）**：  
> - `terrain_kind` 中水域严格命名为 **`"water"`**（没有 `"river"` 枚举）；八个枚举值分别是 `plain`、`forest`、`urban`、`water`、`swamp`、`mountain`、`road`、`bridge`。  
> - `schedule_timed_charge` 的时间参数键名是 **`delay_ticks`**（非 `target_tick`）；`issue_commander_wave` 键名是 **`{ team, hq_unit_id?, delay_ticks, zones }`**。

#### 只读预览端点（`POST /api/query/*`，保证 `SHA-256` 零污染）

```ts
export interface PreviewPathResult {
  reachable: boolean;
  waypoints: WorldPos[];
  total_distance_m: number;
  estimated_seconds: number;
  estimated_ticks: number;
}
export interface PreviewLosLineResult {
  from: WorldPos; to: WorldPos; visible: boolean; dist_m: number;
  concealment: number; blocked_by_terrain: boolean; blocked_by_smoke: boolean;
}
export interface PreviewLosArcResult {
  origin: WorldPos;
  max_range_m: number;
  samples: Array<{
    angle_deg: number; end_pos: WorldPos; visible_dist_m: number;
    blocked_by_terrain: boolean; blocked_by_smoke: boolean;
  }>;
}
export interface PreviewWorkResult {
  work: EngineerWorkKind;
  points: WorldPos[];
  estimated_work_person_sec: number;
  estimated_ticks: number;
}
```

---

### 2.4 二进制地形流 `GET /api/terrain.bin` 解析规范

总字节长度严格等于 **`24 + rows * cols * 7` 字节**（全部为 Little-Endian）：

| 字节偏移 | 长度 | 类型 | 内容说明 |
| :--- | :--- | :--- | :--- |
| `0..8` | 8 B | ASCII | 固定魔数 `GTXV2TER` |
| `8..12` | 4 B | `u32 LE` | 版本号（当前 `1`） |
| `12..16` | 4 B | `u32 LE` | `rows`（南北总格数，对应 world Y 轴长度 `rows * 8.0 m`） |
| `16..20` | 4 B | `u32 LE` | `cols`（东西总格数，对应 world X 轴长度 `cols * 8.0 m`） |
| `20..24` | 4 B | `f32 LE` | `cell_m`（恒为 `8.0`） |
| `24 .. 24 + 4N` | `4N` B | `f32[] LE` | `elevation_m[r * cols + c]` 海拔高度（米） |
| `+N` | `N` B | `u8[]` | `kind`：`0=Plain, 1=Forest, 2=Urban, 3=Water, 4=Swamp, 5=Mountain, 6=Road, 7=Bridge` |
| `+N` | `N` B | `u8[]` | `cover_bonus` 定点量化（`raw / 255.0` -> `0.0..=1.0`） |
| `+N` | `N` B | `u8[]` | `smoke_density` 定点量化（`raw / 255.0` -> `0.0..=1.0`） |

前端即插即用解码函数（可直接用于 [frontend/src/engine/](../frontend/src/engine/httpEngineAdapter.ts)）：

```ts
export interface ParsedTerrainBinary {
  version: number;
  rows: number;
  cols: number;
  cellM: number;
  elevationM: Float32Array;
  kind: Uint8Array;
  coverBonus: Float32Array;
  smokeDensity: Float32Array;
}

export function parseTerrainBinary(buf: ArrayBuffer): ParsedTerrainBinary {
  const view = new DataView(buf);
  const magic = String.fromCharCode(...new Uint8Array(buf, 0, 8));
  if (magic !== 'GTXV2TER') throw new Error(`Invalid terrain magic: ${magic}`);
  const version = view.getUint32(8, true);
  const rows = view.getUint32(12, true);
  const cols = view.getUint32(16, true);
  const cellM = view.getFloat32(20, true);
  const n = rows * cols;
  const expected = 24 + n * 7;
  if (buf.byteLength !== expected) {
    throw new Error(`Terrain length mismatch: got ${buf.byteLength}, expected ${expected}`);
  }
  const elevationM = new Float32Array(n);
  for (let i = 0; i < n; i++) elevationM[i] = view.getFloat32(24 + i * 4, true);

  let off = 24 + n * 4;
  const kind = new Uint8Array(buf, off, n);
  off += n;
  const coverRaw = new Uint8Array(buf, off, n);
  off += n;
  const smokeRaw = new Uint8Array(buf, off, n);

  const coverBonus = new Float32Array(n);
  const smokeDensity = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    coverBonus[i] = coverRaw[i]! / 255;
    smokeDensity[i] = smokeRaw[i]! / 255;
  }
  return { version, rows, cols, cellM, elevationM, kind, coverBonus, smokeDensity };
}
```

---

### 2.5 `1s..16s` 逐秒防剧透走表与 `stop_exact_ms` 截断算法

调用 `POST /api/clock/step` 获取 `playback: TimeMachineFramePayload` 后，前端渲染与数字面板必须在 `[0ms..16000ms]` 之间严格遵守下列确定性回放法则：

1. **`t = 0ms` 起拍冻结基线**：
   - 将面板上可见单位的 `hp / men / ammo / suppression` 从 `playback.unit_baselines` 初始化，**严禁在 `t = 0ms` 直接显示回合末权威快照数值**；
   - `track.initial_visibility === 0`（`track.is_prerender_skirt === true`）的预渲染缓冲环敌军**必须保持透明/不绘制**，直到某帧事件或下一拍被点亮再现身（消除盲区瞬移感）。
2. **分段关键帧插值与阵亡毫秒截断**：
   ```ts
   export function sampleTrackPosition(track: ParticleTrackNode, elapsedMs: number): [number, number] {
     const cappedMs = track.stop_exact_ms !== undefined
       ? Math.min(elapsedMs, track.stop_exact_ms)
       : elapsedMs;
     const kfs = track.keyframes;
     if (!kfs || kfs.length === 0) return track.pos_start;
     if (cappedMs <= kfs[0]![0]) return kfs[0]![1];
     for (let i = 1; i < kfs.length; i++) {
       const [t1, p1] = kfs[i]!;
       if (cappedMs <= t1) {
         const [t0, p0] = kfs[i - 1]!;
         const f = t1 <= t0 ? 1 : (cappedMs - t0) / (t1 - t0);
         return [p0[0] + (p1[0] - p0[0]) * f, p0[1] + (p1[1] - p0[1]) * f];
       }
     }
     return kfs[kfs.length - 1]![1];
   }
   ```
   - 单位越过弯道/桥梁时，`keyframes` 按距离等比拆分为多个 `(ms, [x, y])` 节点，严防两点直线穿墙；
   - 一旦 `stop_exact_ms !== undefined` 且 `elapsedMs >= track.stop_exact_ms`，单位停止滑行并在原地切换为阵亡尸体/残骸起火样式。
3. **离散事件与数字跳字**：
   - 扫描 `playback_events`，仅累计 `ev.exact_ms <= elapsedMs` 的条目：扣减目标的 `hp_delta` / `casualties`、叠加 `exact_supp_add`、扣减射手 `ammo_spent`、并在 `ev.exact_ms` 瞬间在 Layer 4 触发枪口闪光/弹道脉冲，在 `shell`/`detonation`/`mine_trigger` 时刻触发 Layer 2 焦土弹坑与 Layer 9 微震屏；
   - 当 `elapsedMs === 16000` 时，平滑切换/校对至 `GET /api/state?team=...` 与 `GET /api/fields/i?team=...` 回合末权威终态。

---

## 三 · 九层渲染管线与前端美术生产规范（Layer 1 ~ Layer 9）

### 3.1 坐标尺度、像素比例与无网格铁律

- **逻辑坐标换算**：世界单位为米（`m`），左上角 `(0, 0)`，+X 向东，+Y 向南；每格固定 **`8.0 m`**，标准 `64×64` 战区为 `512m × 512m`，`128×128` 为 `1024m × 1024m`。
- **绝对视觉禁令**：`8.0m` 二进制格仅作后端模拟与鼠标射线拾取坐标转换（`c = Math.floor(x / 8)`, `r = Math.floor(y / 8)`），**任何画布图层（Layer 1~9）严禁绘制 8m 方格线或像素阶梯马赛克底图**。
- **美术底图与画布分辨率**：
  - 推荐以 **`4 px/m`（`32 px = 1 cell`）或 `8 px/m`（`64 px = 1 cell`）** 绘制预渲染高精度纹理块与矢量兵牌基准尺寸；
  - 随相机缩放 `camera.zoom`（`0.5x..4.0x`），利用离屏缓存 Canvas（`256×256` 或 `512×512` 区块）承担静态地形，缩放/平移时直接 `drawImage` + 少量动态脏区重绘，无需每帧重算成千上万笔地形路径。

---

### 3.2 九层分层制作规范（对应 [NineLayerViewport.tsx](../frontend/src/render/NineLayerViewport.tsx)）

| 层级 | `data-layer` / 名称 | 推荐融合/材质与美术职责 | 数据驱动来源 | 重绘频率与性能策略 |
| :---: | :--- | :--- | :--- | :--- |
| **L1** | `1` · `base-map-color` | **连续地形色彩与肌理**：按 `terrain.bin` 的 8 种 `TerrainKind` 与连续 `elevationM` 做自然羽化/噪声过渡（草地/白桦林冠阴影/公路碎石纹理/桥梁木铁质感/溪流河岸渐变），**绝不露网格边缘** | `GET /api/terrain.bin` 或手绘预烘焙全景底图 | 仅加载/地形被 `edit_terrain` 破坏时增量重绘离屏缓存 |
| **L2** | `2` · `terrain-destruction` | **地表创伤与工事实体**：圆形多环弹坑（焦黑内圈+外翻土晕）、折线战壕（土埂+木撑遮蔽纹）、散兵坑、焦黑火痕、雷区铁丝/三角警告暗记 | `visible_instances` (`fortification`/`mine`) + `PlaybackEvent.terrain_cell` | **铁律：严格在 L3 之下**；叠加在 L1 上，由 L3 统一盖压环境光影 |
| **L3** | `3` · `ambient-occlusion` | **全局环境光遮蔽与地形纵深明暗**：基于 `elevationM` 做 Horn 法线斜照阴影（如西北来光 `45°` 对山脊东南坡投射柔和暗蓝灰 AO）+ 河谷/战壕边缘暗角 (`globalCompositeOperation = 'multiply'`, 透明度 `0.18..0.38`) | `terrain.bin` 高程场 + 静态山谷/密林 AO 通道 | **压在 L1 与 L2 之上**；静态离屏烘焙一张即可，几乎零每帧耗时 |
| **L4** | `4` · `entity-tokens` | **拉格朗日动态兵牌与实物**：<br>1. 作战单位兵牌/仰视角坦克底盘与炮塔分件（独立朝向 `facing_deg`）；<br>2. 散落物 (`dropped_item`)、武器 (`dropped_weapon`)、阵亡士兵/残骸 (`corpse`)；<br>3. 烟幕 (`smoke_density > 0` 体积云团)、弹道火线微光与爆炸粒子。<br>**严禁直接绘制底层 `is_prerender_skirt` 缓冲粒子** | `ObservedStateView.units`、`visible_instances`、`TimeMachineFramePayload` (`keyframes` / `stop_exact_ms`)、`terrain.smokeDensity` | 每帧 `requestAnimationFrame`（60fps）；兵牌矢量/雪碧图缓存 |
| **L5** | `5` · `range-area-rings` | **战术线框与双场可视化层**：<br>1. 选中单位直指半径圈、实际无线电圈 `effective_radio_range_m`（含山体 Deygout 衰减收缩）、只读 `/api/query/arc` 多边形射界阴影扇区；<br>2. 点开顶栏指挥纸条时渲染：炮击预备区（红斜线）、友军推进箭头（虚实渐进蓝/红矢）、许可 AO 境界框；<br>3. 可选叠加 `/api/fields/i` 热区（红色火线 `i_threat` 与背坡阴影死角、绿/amber `i_intel` 侦察边界） | `ObservedStateView` + `POST /api/query/*` + `GET /api/fields/i` | 交互态（选中/拖拽/点击指挥纸条）按需重绘 |
| **L6** | `6` · `freehand-overlay` | **地图手绘辅助线（防篡改净土）**：战前铅笔/红蓝油性笔手绘草图、图钉标注、测距箭头；使用抗锯齿笔锋 + 轻微纸面粗糙度 | **仅客户端 `IndexedDB` / `localStorage`** | **绝对不发后端**，F5 刷新无损复原 |
| **L7** | `7` · `world-vfx-a` | **天气、日照色温与战场照明弹**（独立开关）：<br>- 细雨倾斜粒子 / 飘雪 / 战壕低矮平流晨雾；<br>- 昼夜色温层（`night: true` 时压冷蓝深灰基调，探照灯/照明弹/炮口火光用 `lighter` 或 `destination-out` 局部破开黑夜遮罩）。<br>**严格位于 L8 功能面板下方，禁止污染 HUD 字体可读性** | `World.night` + 前端天气模式 (`clear`/`rain`/`fog`/`night`) | 可由用户独立开关；关闭时零 Canvas 绘制开销 |
| **L8** | `8` · `functional-hud` | **内置军用桌面 HUD（HTML/CSS）**：做旧羊皮纸手写板 `UIO`（含回形针金属夹、冲突红章）、独立指挥面板、16s 黄铜/机械怀表式走表控制栏、`StepCache` 60 天未来事件提醒条、单位详情弹卡（按 `GET /api/units/:id` 渲染 **单兵 4 部位人形损伤图** 与 **坦克 8 子系统机械剖面**） | React HUD 状态机 + `/api/units/:id` | DOM 层，通过 `pointer-events: none` 外壳 + 面板 `auto` 透传地图点击 |
| **L9** | `9` · `atmosphere-vfx-b` | **桌面物理气氛组**（独立开关，最顶层 `pointer-events: none`）：边缘桌面压暗（Vignette）、微妙纸纹/胶片噪点纹理叠加、听到爆炸/重炮瞬间触发微幅 CSS `transform` 震屏 (`shakeAtMs`) | `PlaybackEvent` 爆破毫秒 + 全局开关 | 纯 CSS/轻量透明叠加，用户可一键完全关闭 |

---

### 3.3 迷雾视觉分级、状态机视觉钩子与资产标准

1. **四档 `VisTier` 迷雾视觉语言（严格对应 `I_intel` 状态）**：
   - `hidden`（及 `is_prerender_skirt = true`）：`opacity = 0`，不渲染、不触发鼠标 `hover`/点击拾取；
   - `suspected`（怀疑区，后端已按 `16m` 格脱敏）：渲染模糊粉笔虚线圈 / 未知震源波纹 `?`（坐标在 `pos` 周围有轻微呼吸抖动），不显示兵种与人数；
   - `spotted`（发现轮廓）：显示兵种北约军标（NATO App-6）或通用黑灰剪影 + 朝向箭头，隐藏具体人数、弹药与姿态细节；
   - `full`（完全查明）：显示完整涂装兵牌/载具双层（底盘 + 独立炮塔）、阵营色标、实时血条/存活人数标记与 `SchmidtPostureState` 姿态角标（站立 / 卧倒微缩图标 / 压制颤抖暗红描边）。
2. **车辆与单兵毁伤视觉反馈钩子（源自 Phase B2 数据）**：
   - 当 `/api/units/:id` 返回 `vehicle_damage`：
     - `subsystems[1] (Tracks) <= 0.0`：坦克停止履带尘烟并标“断履带抛锚”橘黄扳手图标；
     - `subsystems[2] (MainGun) <= 0.0`：炮管垂下或标火炮损坏；
     - `burning === true`：L4 在载具上方连续生成升腾黑烟与橘红火星粒子；
     - `ammo_cooked_off === true`：触发 L2/L4 炮塔掀飞残骸特效与焦黑环；
   - 当返回步兵 `soldiers[]`：
     - 检视面板按个人展示 4 部位 `integrity`（绿 `>0.7` -> 黄 `0.3..0.7` -> 红 `<0.3` -> 黑 `0.0`）及 `bleed_rate_per_tick > 0` 红色渗血动画脉冲，不再用单一模糊整班血条糊弄。
3. **纹理与资产交付格式建议**：
   - **静态地图贴图**：推荐 `WebP` / `PNG`（sRGB 色彩空间，无嵌入 ICC 冲突配置文件），建议按 `1024×1024 px` 分块切片命名（如 `map_tile_{tx}_{ty}.webp`）；
   - **矢量军标 & UI 控件**：优先纯 `SVG`（统一 `viewBox="0 0 64 64"`，中心 `(32, 32)` 对齐世界 `WorldPos` 质心，正右方 `0°` 顺时针与后端 `facing_deg` 的 `atan2(dy, dx).toDegrees()` 严格一致）；
   - **文件编码死规矩**：`frontend/` 内所有 `.ts` / `.tsx` / `.css` / `.json` / `.html` 与 `docs/*.md` 保持 **UTF-8 无 BOM**。
