// GenTactix V2 frontend · 全套强类型定义（World / Unit / Wave / UIO / StepCache / Playback）
// 所有坐标单位为米（m），世界原点在左上角，x 向右、y 向下。

/** 单步长秒数：与后端 `TICK_SECONDS = 16` 永久锁死一致。 */
export const TICK_SECONDS = 16;
/** 单步长毫秒数。 */
export const TICK_MS = TICK_SECONDS * 1000;
/** 后端 8m 二进制战术格（仅作内部坐标/拾取基准，前端严禁渲染网格线）。 */
export const TACTICAL_CELL_SIZE_M = 8.0;
/** 60 天步长缓存跨度：60 * 86400 / 16 = 324_000 步长。 */
export const MAX_STEP_CACHE_HORIZON_TICKS = 324_000;

export type Team = 'red' | 'blue' | 'neutral';

export interface Vec2 {
  x: number;
  y: number;
}

export type UnitRole = 'platoon_hq' | 'rifle_squad' | 'enemy_squad';

export type FireDiscipline = 'hold_fire' | 'return_fire' | 'fire_at_will';

export type MoveStance = 'normal' | 'fast' | 'stealth';

export type EngineerWorkKind = 'minefield' | 'foxhole_line' | 'zigzag_trench' | 'demolition';

/** 单位的当前执行任务（沙盒内部权威态，前端展示用摘要）。 */
export type UnitActivity =
  | { type: 'idle' }
  | { type: 'hold' }
  | { type: 'move'; path: Vec2[] }
  | { type: 'fire'; target: Vec2; targetUnitId?: number }
  | { type: 'build'; work: EngineerWorkKind; points: Vec2[] }
  | { type: 'smoke'; target: Vec2 };

export interface UnitState {
  id: number;
  name: string;
  team: Team;
  role: UnitRole;
  pos: Vec2;
  /** 存活士兵数（按个人记账口径，严禁按整班涂抹）。 */
  soldiers: number;
  maxSoldiers: number;
  ammo: number;
  maxAmmo: number;
  suppression: number;
  /** 施工进度 0..1。 */
  workProgress: number;
  discipline: FireDiscipline;
  alive: boolean;
  activity: UnitActivity;
}

export type GroundObjectKind = 'item' | 'weapon' | 'corpse';

export interface GroundObject {
  id: string;
  kind: GroundObjectKind;
  pos: Vec2;
  label: string;
}

export type DestructionKind = 'crater' | 'foxhole' | 'trench' | 'scorch';

export interface TerrainDestruction {
  id: string;
  kind: DestructionKind;
  pos: Vec2;
  radiusM: number;
  /** 堑壕/散兵坑线的折线点（可选）。 */
  points?: Vec2[];
  createdTick: number;
}

export interface SmokeCloud {
  id: string;
  pos: Vec2;
  radiusM: number;
  createdTick: number;
}

export interface WorldSnapshot {
  tick: number;
  /** 当前 16s 步长内的走表毫秒（0..16000）。 */
  elapsedMs: number;
  observer: Team;
  units: UnitState[];
  groundObjects: GroundObject[];
  destruction: TerrainDestruction[];
  smoke: SmokeCloud[];
  night: boolean;
}

// ---------------------------------------------------------------------------
// 时间机 / 步长缓存
// ---------------------------------------------------------------------------

export type TimeCategory = 'bullet' | 'shell' | 'command' | 'event' | 'movement';

export interface TimeLutEntry {
  /** 额外拖延回合数（0 = 下一步长 #k+1，1 = 下下步长 #k+2）。 */
  turnOffset: number;
  /** 生效秒拍 1..=16。 */
  secInTick: number;
  /** 生效毫秒戳 1..=16000。 */
  exactMs: number;
}

export type StepEventKind = 'detonation' | 'task_command' | 'g_field_wave' | 'macro_stop';

export interface DetonationPayload {
  type: 'detonation';
  radiusM: number;
  label: string;
}

export interface TaskCommandPayload {
  type: 'task_command';
  directiveId: string;
  command: TacticalCommandJson;
  label: string;
}

export interface MacroStopPayload {
  type: 'macro_stop';
  directiveId: string;
  command: TacticalCommandJson;
  label: string;
}

export interface WavePayload {
  type: 'g_field_wave';
  waveId: string;
  label: string;
}

export type StepEventPayload = DetonationPayload | TaskCommandPayload | MacroStopPayload | WavePayload;

export interface ScheduledStepEvent {
  eventId: string;
  /** 事件归属阵营：前向预警严格按阵营迷雾过滤。 */
  team: Team;
  /** 产生该预约事件的回合号（如 #0）。 */
  sourceTick: number;
  /** 预定生效的目标回合号（最大允许 sourceTick + 324000，即 60 天）。 */
  targetTick: number;
  /** 1..=16 秒拍。 */
  secInTick: number;
  /** 1..=16000 毫秒戳。 */
  exactMs: number;
  kind: StepEventKind;
  subjectUnitId?: number;
  targetPos?: Vec2;
  payload: StepEventPayload;
}

// ---------------------------------------------------------------------------
// 无头指令（对齐后端 TacticalCommand 的 snake_case JSON）
// ---------------------------------------------------------------------------

export type TacticalCommandJson =
  | { kind: 'move_unit'; unit_id: number; waypoints: Vec2[]; stance?: MoveStance }
  | { kind: 'attack_pos'; unit_id: number; target: Vec2 }
  | { kind: 'attack_unit'; unit_id: number; target_id: number; chase?: boolean }
  | { kind: 'hold_unit'; unit_id: number }
  | { kind: 'set_fire_discipline'; unit_id: number; discipline: FireDiscipline }
  | { kind: 'pop_smoke'; unit_id: number; target: Vec2 }
  | { kind: 'build_work'; unit_id: number; work: EngineerWorkKind; waypoints: Vec2[] };

export type DirectiveTaskKind = TacticalCommandJson['kind'];

/** 事件宏：前出占领 -> 构筑散兵坑 -> 按纪律警戒。 */
export type MacroKind = 'advance_dig_watch';

export type DirectiveTarget =
  | { type: 'point'; pos: Vec2 }
  | { type: 'polyline'; points: Vec2[] }
  | { type: 'unit'; unitId: number };

export type DirectiveMode = 'task' | 'macro';

export interface HeadlessDirective {
  id: string;
  seq: number;
  mode: DirectiveMode;
  taskKind?: DirectiveTaskKind;
  macroKind?: MacroKind;
  unitId?: number;
  target?: DirectiveTarget;
  /** 相对签发锚点的起始秒。 */
  startSec: number;
  /** 相对签发锚点的结束秒。 */
  endSec: number;
  /** 延迟偏移（秒），作用于起止时间。 */
  delayOffsetSec: number;
  /** 延迟乘数，作用于起止时间。 */
  delayMultiplier: number;
  stance: MoveStance;
  discipline: FireDiscipline;
  work: EngineerWorkKind;
  /** 来源说明（新建 / 复制副本 / 归档回炉）。 */
  origin: 'new' | 'copy' | 'archive';
  note?: string;
}

/** 宏展开后的原子指令计划。 */
export interface AtomicCommandPlan {
  directiveId: string;
  /** 相对签发锚点的偏移秒（已施加延迟乘数与偏移）。 */
  offsetSec: number;
  command: TacticalCommandJson;
  /** 是否为结束时刻自动补发的停止指令（hold_unit）。 */
  isStop: boolean;
  label: string;
}

export interface IssueDirectiveRequest {
  directiveId: string;
  unitId: number;
  atoms: AtomicCommandPlan[];
}

export interface IssueQueueRequest {
  directives: IssueDirectiveRequest[];
}

export interface DirectiveIssueReceipt {
  directiveId: string;
  unitId: number;
  category: TimeCategory;
  lutSlot: number;
  /** 双哈希混淆后的锚点回合与毫秒（未含远程偏移）。 */
  anchorTick: number;
  anchorExactMs: number;
  remote: boolean;
  delayTicks: number;
  /** 首条原子指令真实生效回合 / 毫秒。 */
  firstTick: number;
  firstExactMs: number;
  /** 结束停止指令生效回合 / 毫秒。 */
  endTick: number;
  endExactMs: number;
  eventIds: string[];
}

export interface IssueQueueReceipt {
  issueTick: number;
  receipts: DirectiveIssueReceipt[];
}

// ---------------------------------------------------------------------------
// 波次（指挥纸条）
// ---------------------------------------------------------------------------

export type BarrageShape =
  | { type: 'circle'; center: Vec2; radiusM: number }
  | { type: 'polygon'; points: Vec2[] };

export interface AoBox {
  min: Vec2;
  max: Vec2;
}

export interface WaveGeometry {
  barrages: BarrageShape[];
  assaultArrows: Vec2[][];
  aos: AoBox[];
}

export interface WaveDirective {
  id: string;
  title: string;
  issuer: string;
  createdTick: number;
  effectiveTick: number;
  effectiveExactMs: number;
  remote: boolean;
  delayTicks: number;
  geometry: WaveGeometry;
  /** 是否已在世界上真实生效（G 场落地）。 */
  effective: boolean;
}

export interface IssueWaveRequest {
  title: string;
  geometry: WaveGeometry;
}

// ---------------------------------------------------------------------------
// 走表剧本（回合初账单锁定 + 逐秒渐进兑现）
// ---------------------------------------------------------------------------

export interface UnitKeyframe {
  ms: number;
  pos: Vec2;
  workProgress: number;
}

export type DiscreteChange =
  | { ms: number; type: 'unit'; unitId: number; field: 'soldiers' | 'ammo' | 'suppression'; value: number }
  | { ms: number; type: 'unit_alive'; unitId: number; alive: boolean }
  | { ms: number; type: 'unit_activity'; unitId: number; activity: UnitActivity }
  | { ms: number; type: 'unit_discipline'; unitId: number; discipline: FireDiscipline }
  | { ms: number; type: 'destruction'; item: TerrainDestruction }
  | { ms: number; type: 'smoke'; item: SmokeCloud }
  | { ms: number; type: 'ground_object'; item: GroundObject };

export interface PlaybackLogEntry {
  ms: number;
  text: string;
  kind: StepEventKind | 'fire' | 'casualty';
}

export interface TickPlayback {
  tick: number;
  observer: Team;
  baseline: WorldSnapshot;
  keyframes: Record<number, UnitKeyframe[]>;
  changes: DiscreteChange[];
  log: PlaybackLogEntry[];
  /** 爆炸毫秒（供 L9 震屏）。 */
  shakeAtMs: number[];
  /** 本拍内执行完毕（停止指令兑现）的无头指令 ID。 */
  completedDirectiveIds: string[];
  /** 本拍内生效的波次 ID。 */
  effectiveWaveIds: string[];
  final: WorldSnapshot;
}

export interface CommandDelayInfo {
  unitId: number;
  distanceM: number;
  remote: boolean;
  delayTicks: number;
  delaySec: number;
}

export interface InitialWorld {
  snapshot: WorldSnapshot;
  waves: WaveDirective[];
  hqUnitId: number;
  directCommandRadiusM: number;
  radioRadiusM: number;
  terrain: TerrainModel;
}

// ---------------------------------------------------------------------------
// 预渲染地形描述（L1/L3 连续底图，严禁网格线）
// ---------------------------------------------------------------------------

export interface TerrainHill {
  center: Vec2;
  radiusM: number;
  heightM: number;
}

export interface TerrainPatch {
  center: Vec2;
  rx: number;
  ry: number;
  rotation: number;
  kind: 'forest' | 'field' | 'marsh';
}

export interface TerrainModel {
  widthM: number;
  heightM: number;
  hills: TerrainHill[];
  patches: TerrainPatch[];
  river: Vec2[];
  roads: Vec2[][];
}
