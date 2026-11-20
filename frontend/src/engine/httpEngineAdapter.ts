// GenTactix V2 frontend · 第二阶段真实后端适配器（全功能对齐 Phase B1~B4 HTTP 契约）
// 连真后端时：原子指令直发 POST /api/command，由 Rust 核心唯一计算双哈希混淆、远程偏移与欧拉弹道场。

import type { BackendAdapter } from './backendAdapter';
import { computeCommandDelay } from './directives';
import { convertBackendPlayback } from './playback';
import { parseTerrainBinary, terrainBinaryToModel } from './terrainBinary';
import {
  TICK_MS,
  TICK_SECONDS,
  type BackendTimeMachinePayload,
  type CommandDelayInfo,
  type CommandOutcomeJson,
  type GroundObject,
  type IFieldTriplet,
  type InitialWorld,
  type IssueQueueReceipt,
  type IssueQueueRequest,
  type IssueWaveRequest,
  type ScheduledStepEvent,
  type TacticalCommandJson,
  type Team,
  type TerrainDestruction,
  type TerrainModel,
  type TickPlayback,
  type UnitActivity,
  type UnitDetailView,
  type UnitState,
  type Vec2,
  type VisTier,
  type WaveDirective,
  type WorldSnapshot,
} from './types';

export interface HttpEngineAdapterOptions {
  baseUrl: string;
}

interface RawObservedUnit {
  id: number;
  callsign: string;
  team: Team;
  role?: UnitState['role'];
  pos: Vec2;
  facing_deg: number;
  alive: boolean;
  vis_tier: VisTier;
  men?: number;
  hp?: number;
  suppression?: number;
  weapon_id?: string;
  ammo_rounds?: number;
  is_vehicle?: boolean;
  posture?: UnitState['posture'];
  order?: {
    kind: string;
    waypoints?: Vec2[];
    target?: Vec2 | number;
    work?: UnitState['activity'] extends { work: infer W } ? W : never;
  };
}

interface RawVisibleInstance {
  instance_id: number;
  kind: 'timed_charge' | 'mine' | 'fortification' | 'dropped_item' | 'dropped_weapon' | 'corpse';
  team: Team;
  pos: Vec2;
  vis_tier: VisTier;
  label?: string;
}

interface RawScheduledSummary {
  event_id: number;
  source_tick: number;
  target_tick: number;
  sec_in_tick: number;
  exact_ms: number;
  kind: 'task_command' | 'detonation' | 'g_field_wave';
  subject_unit_id?: number;
  target_pos?: Vec2;
  label?: string;
}

interface RawObservedState {
  tick: number;
  clock_seconds: number;
  observer_team: Team;
  fog_enforced: boolean;
  units: RawObservedUnit[];
  visible_instances?: RawVisibleInstance[];
  step_cache_queue?: RawScheduledSummary[];
  authoritative_sha256: string;
}

function mapOrderToActivity(order?: RawObservedUnit['order']): UnitActivity {
  if (!order || order.kind === 'hold') return { type: 'hold' };
  if (order.kind === 'move' && Array.isArray(order.waypoints)) {
    return { type: 'move', path: order.waypoints };
  }
  if (order.kind === 'attack_pos' && order.target && typeof order.target === 'object') {
    return { type: 'fire', target: order.target };
  }
  if (order.kind === 'attack_unit' && typeof order.target === 'number') {
    return { type: 'fire', target: { x: 0, y: 0 }, targetUnitId: order.target };
  }
  if (order.kind === 'pop_smoke' && order.target && typeof order.target === 'object') {
    return { type: 'smoke', target: order.target };
  }
  return { type: 'idle' };
}

function normalizeCommandForRust(cmd: TacticalCommandJson): Record<string, unknown> {
  if (cmd.kind === 'move_unit') {
    const stance = cmd.stance === 'stealth' ? 'cautious' : (cmd.stance ?? 'normal');
    return { kind: 'move_unit', unit_id: cmd.unit_id, waypoints: cmd.waypoints, stance };
  }
  return { ...cmd };
}

export class HttpEngineAdapter implements BackendAdapter {
  readonly kind = 'http' as const;
  private readonly baseUrl: string;
  private cachedTick = 0;
  private cachedHqId = 1;
  private cachedUnits: UnitState[] = [];
  private cachedSnapshot: WorldSnapshot | null = null;
  private cachedScheduled: ScheduledStepEvent[] = [];
  private waveSeq = 100;

  constructor(options: HttpEngineAdapterOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, '');
  }

  private async requestJson<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      headers: { 'Content-Type': 'application/json' },
      ...init,
    });
    if (!res.ok) {
      throw new Error(`HTTP ${res.status} ${path}: ${await res.text()}`);
    }
    return (await res.json()) as T;
  }

  private async requestBinary(path: string): Promise<ArrayBuffer> {
    const res = await fetch(`${this.baseUrl}${path}`);
    if (!res.ok) {
      throw new Error(`HTTP ${res.status} ${path}: ${await res.text()}`);
    }
    return res.arrayBuffer();
  }

  getCurrentTick(): number {
    return this.cachedTick;
  }

  async checkHealth(): Promise<{ ok: boolean; version?: string; cell_m?: number; tick_s?: number }> {
    try {
      const h = await this.requestJson<{ status: string; version?: string; cell_m?: number; tick_s?: number }>('/health');
      return { ok: h.status === 'ok', version: h.version, cell_m: h.cell_m, tick_s: h.tick_s };
    } catch {
      return { ok: false };
    }
  }

  async loadInitial(observer: Team): Promise<InitialWorld> {
    const [snapshot, binBuf] = await Promise.all([
      this.getSnapshot(observer),
      this.requestBinary('/api/terrain.bin').catch(() => null),
    ]);
    const terrain: TerrainModel = binBuf
      ? terrainBinaryToModel(parseTerrainBinary(binBuf))
      : { widthM: 512, heightM: 512, hills: [], patches: [], river: [], roads: [] };
    const hq = snapshot.units.find((u) => u.role === 'platoon_hq' && u.team === observer) ?? snapshot.units[0];
    this.cachedHqId = hq?.id ?? 1;
    return {
      snapshot,
      waves: [],
      hqUnitId: this.cachedHqId,
      directCommandRadiusM: 140,
      radioRadiusM: 600,
      terrain,
    };
  }

  async getSnapshot(observer: Team): Promise<WorldSnapshot> {
    const raw = await this.requestJson<RawObservedState>(`/api/state?team=${observer}`);
    this.cachedTick = raw.tick ?? this.cachedTick;

    const units: UnitState[] = (raw.units ?? []).map((u) => ({
      id: u.id,
      name: u.callsign,
      team: u.team,
      role: u.role ?? 'infantry_squad',
      pos: u.pos,
      soldiers: u.men ?? 0,
      maxSoldiers: Math.max(u.men ?? 9, 9),
      ammo: u.ammo_rounds ?? 0,
      maxAmmo: Math.max(u.ammo_rounds ?? 360, 360),
      suppression: Math.round((u.suppression ?? 0) * 10),
      workProgress: 0,
      discipline: 'return_fire',
      alive: u.alive,
      activity: mapOrderToActivity(u.order),
      visTier: u.vis_tier,
      facingDeg: u.facing_deg,
      isVehicle: u.is_vehicle,
      hp: u.hp,
      posture: u.posture,
    }));

    const groundObjects: GroundObject[] = [];
    const destruction: TerrainDestruction[] = [];
    for (const inst of raw.visible_instances ?? []) {
      if (inst.kind === 'fortification') {
        destruction.push({
          id: `inst-${inst.instance_id}`,
          kind: 'foxhole',
          pos: inst.pos,
          radiusM: 8,
          createdTick: this.cachedTick,
        });
      } else {
        const mappedKind: GroundObject['kind'] =
          inst.kind === 'corpse' ? 'corpse' : inst.kind === 'dropped_weapon' ? 'weapon' : 'item';
        groundObjects.push({
          id: `inst-${inst.instance_id}`,
          kind: mappedKind,
          pos: inst.pos,
          label: inst.label ?? inst.kind,
        });
      }
    }

    this.cachedScheduled = (raw.step_cache_queue ?? []).map((ev) => ({
      eventId: String(ev.event_id),
      team: observer,
      sourceTick: ev.source_tick,
      targetTick: ev.target_tick,
      secInTick: ev.sec_in_tick,
      exactMs: ev.exact_ms,
      kind: ev.kind,
      subjectUnitId: ev.subject_unit_id,
      targetPos: ev.target_pos,
      payload:
        ev.kind === 'detonation'
          ? { type: 'detonation', radiusM: 24, label: ev.label ?? `炸药#${ev.event_id}` }
          : ev.kind === 'g_field_wave'
            ? { type: 'g_field_wave', waveId: ev.label ?? String(ev.event_id), label: ev.label ?? '指挥波次' }
            : {
                type: 'task_command',
                directiveId: String(ev.event_id),
                command: { kind: 'hold_unit', unit_id: ev.subject_unit_id ?? 0 },
                label: ev.label ?? `队列#${ev.event_id}`,
              },
    }));

    this.cachedUnits = units;
    const snap: WorldSnapshot = {
      tick: this.cachedTick,
      elapsedMs: 0,
      observer,
      units,
      groundObjects,
      destruction,
      smoke: [],
      night: false,
    };
    this.cachedSnapshot = snap;
    return snap;
  }

  previewCommandDelay(unitId: number): CommandDelayInfo | undefined {
    const hq = this.cachedUnits.find((u) => u.id === this.cachedHqId);
    const target = this.cachedUnits.find((u) => u.id === unitId);
    if (!hq || !target) return undefined;
    return computeCommandDelay(hq, target, 140);
  }

  async submitRawCommand(cmd: Record<string, unknown>, cmdSecInTick = 1): Promise<CommandOutcomeJson> {
    return this.requestJson<CommandOutcomeJson>('/api/command', {
      method: 'POST',
      body: JSON.stringify({ ...cmd, cmd_sec_in_tick: cmdSecInTick }),
    });
  }

  async issueQueue(request: IssueQueueRequest): Promise<IssueQueueReceipt> {
    const receipts: IssueQueueReceipt['receipts'] = [];
    for (const dir of request.directives) {
      const eventIds: string[] = [];
      let firstTick = this.cachedTick + 1;
      let endTick = this.cachedTick + 1;
      let delayTicks = 0;
      let remote = false;

      for (const atom of dir.atoms) {
        const secInTick = Math.max(1, Math.min(TICK_SECONDS, Math.floor((atom.offsetSec % TICK_SECONDS) + 1)));
        const out = await this.submitRawCommand(normalizeCommandForRust(atom.command), secInTick);
        if (out.ticket_id !== undefined) eventIds.push(String(out.ticket_id));
        if (out.deferred_until_tick !== undefined) {
          if (eventIds.length <= 1) firstTick = out.deferred_until_tick;
          endTick = Math.max(endTick, out.deferred_until_tick);
        }
        if (out.remote_delay_ticks && out.remote_delay_ticks > 0) {
          remote = true;
          delayTicks = Math.max(delayTicks, out.remote_delay_ticks);
        }
      }
      receipts.push({
        directiveId: dir.directiveId,
        unitId: dir.unitId,
        category: 'command',
        lutSlot: 0,
        anchorTick: firstTick,
        anchorExactMs: 1000,
        remote,
        delayTicks,
        firstTick,
        firstExactMs: 1000,
        endTick,
        endExactMs: 16000,
        eventIds,
      });
    }
    return { issueTick: this.cachedTick, receipts };
  }

  async issueWave(request: IssueWaveRequest): Promise<WaveDirective> {
    const zones: Array<Record<string, unknown>> = [];
    for (const b of request.geometry.barrages) {
      if (b.type === 'circle') {
        zones.push({ kind: 'bombardment', center: b.center, radius_m: b.radiusM, g0_weight: 1.5 });
      } else if (b.points.length > 0) {
        const cx = b.points.reduce((s, p) => s + p.x, 0) / b.points.length;
        const cy = b.points.reduce((s, p) => s + p.y, 0) / b.points.length;
        zones.push({ kind: 'bombardment', center: { x: cx, y: cy }, radius_m: 36, waypoints: b.points, g0_weight: 1.5 });
      }
    }
    for (const arrow of request.geometry.assaultArrows) {
      if (arrow.length > 0) {
        zones.push({
          kind: 'assault',
          center: arrow[arrow.length - 1]!,
          radius_m: 24,
          waypoints: arrow,
          g0_weight: 1.2,
        });
      }
    }
    for (const ao of request.geometry.aos) {
      const center = { x: (ao.min.x + ao.max.x) / 2, y: (ao.min.y + ao.max.y) / 2 };
      const radiusM = Math.max(16, Math.hypot(ao.max.x - ao.min.x, ao.max.y - ao.min.y) / 2);
      zones.push({ kind: 'permitted_ao', center, radius_m: radiusM, g0_weight: 1.0 });
    }

    const out = await this.submitRawCommand({
      kind: 'issue_commander_wave',
      team: 'red',
      hq_unit_id: this.cachedHqId,
      delay_ticks: 1,
      zones,
    });
    const effTick = out.deferred_until_tick ?? this.cachedTick + 2;
    return {
      id: `wave-http-${this.waveSeq++}`,
      title: request.title,
      issuer: '指挥员(真后端)',
      createdTick: this.cachedTick,
      effectiveTick: effTick,
      effectiveExactMs: 4200,
      remote: (out.remote_delay_ticks ?? 0) > 0,
      delayTicks: out.remote_delay_ticks ?? 1,
      geometry: request.geometry,
      effective: effTick <= this.cachedTick,
    };
  }

  async advanceTick(observer: Team): Promise<TickPlayback> {
    const prevSnap =
      this.cachedSnapshot ??
      (await this.getSnapshot(observer).catch(() => ({
        tick: this.cachedTick,
        elapsedMs: 0,
        observer,
        units: this.cachedUnits,
        groundObjects: [],
        destruction: [],
        smoke: [],
        night: false,
      })));

    const stepRes = await this.requestJson<{
      tick?: number;
      steps?: Array<{ playback?: BackendTimeMachinePayload }>;
    }>('/api/clock/step', {
      method: 'POST',
      body: JSON.stringify({ ticks: 1, observer_team: observer }),
    });
    this.cachedTick = stepRes.tick ?? this.cachedTick + 1;
    const finalSnap = await this.getSnapshot(observer);
    const payload = stepRes.steps?.[0]?.playback;
    if (payload) {
      return convertBackendPlayback(prevSnap, finalSnap, payload, observer);
    }
    return {
      tick: this.cachedTick,
      observer,
      baseline: prevSnap,
      keyframes: {},
      changes: [],
      log: [],
      shakeAtMs: [],
      completedDirectiveIds: [],
      effectiveWaveIds: [],
      final: { ...finalSnap, elapsedMs: TICK_MS },
    };
  }

  async fastForwardTo(targetTick: number, observer: Team): Promise<TickPlayback[]> {
    const out: TickPlayback[] = [];
    const goal = Math.max(this.cachedTick, Math.floor(targetTick));
    while (this.cachedTick < goal) {
      out.push(await this.advanceTick(observer));
    }
    return out;
  }

  async lookahead(targetTick: number, observer: Team): Promise<ScheduledStepEvent[]> {
    if (this.cachedSnapshot === null) {
      await this.getSnapshot(observer).catch(() => null);
    }
    return this.cachedScheduled.filter((e) => e.targetTick === targetTick);
  }

  async listScheduled(observer: Team): Promise<ScheduledStepEvent[]> {
    if (this.cachedSnapshot === null) {
      await this.getSnapshot(observer).catch(() => null);
    }
    return [...this.cachedScheduled];
  }

  async getUnitDetail(unitId: number, observer: Team): Promise<UnitDetailView | undefined> {
    try {
      return await this.requestJson<UnitDetailView>(`/api/units/${unitId}?team=${observer}`);
    } catch {
      return undefined;
    }
  }

  async getIField(observer: Team): Promise<IFieldTriplet | undefined> {
    try {
      return await this.requestJson<IFieldTriplet>(`/api/fields/i?team=${observer}`);
    } catch {
      return undefined;
    }
  }
}
