// GenTactix V2 frontend · 逐秒防剧透走表：按 elapsedMs 渐进兑现回合初锁定的剧本
// t = 0 严格等于回合初基线；离散伤害/弹耗/弹坑只在到达 exactMs 时跳字；t = 16000 与回合末权威态对齐。

import type {
  BackendTimeMachinePayload,
  DiscreteChange,
  PlaybackLogEntry,
  TickPlayback,
  UnitKeyframe,
  UnitState,
  Vec2,
  WorldSnapshot,
} from './types';
import { TICK_MS } from './types';

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function interpolateKeyframes(frames: UnitKeyframe[] | undefined, ms: number): { pos: Vec2; workProgress: number } | undefined {
  if (!frames || frames.length === 0) return undefined;
  const first = frames[0]!;
  const effectiveMs = first.stopExactMs !== undefined ? Math.min(ms, first.stopExactMs) : ms;
  if (effectiveMs <= first.ms) return { pos: { ...first.pos }, workProgress: first.workProgress };
  for (let i = 1; i < frames.length; i++) {
    const b = frames[i]!;
    if (effectiveMs <= b.ms) {
      const a = frames[i - 1]!;
      const span = b.ms - a.ms;
      const t = span <= 0 ? 1 : (effectiveMs - a.ms) / span;
      return {
        pos: { x: lerp(a.pos.x, b.pos.x, t), y: lerp(a.pos.y, b.pos.y, t) },
        workProgress: lerp(a.workProgress, b.workProgress, t),
      };
    }
  }
  const last = frames[frames.length - 1]!;
  return { pos: { ...last.pos }, workProgress: last.workProgress };
}

function cloneUnit(u: UnitState): UnitState {
  return { ...u, pos: { ...u.pos }, activity: u.activity };
}

/** 渐进快照：只兑现 ms <= elapsedMs 的离散变化，连续量按关键帧插值。 */
export function progressiveSnapshot(pb: TickPlayback, elapsedMs: number): WorldSnapshot {
  const ms = Math.max(0, Math.min(TICK_MS, elapsedMs));
  if (ms >= TICK_MS) return { ...pb.final, elapsedMs: TICK_MS };
  const units = new Map<number, UnitState>();
  for (const u of pb.baseline.units) units.set(u.id, cloneUnit(u));
  const destruction = pb.baseline.destruction.slice();
  const smoke = pb.baseline.smoke.slice();
  const groundObjects = pb.baseline.groundObjects.slice();
  for (const ch of pb.changes) {
    if (ch.ms > ms) continue;
    switch (ch.type) {
      case 'unit': {
        const u = units.get(ch.unitId);
        if (u) u[ch.field] = ch.value;
        break;
      }
      case 'unit_alive': {
        const u = units.get(ch.unitId);
        if (u) u.alive = ch.alive;
        break;
      }
      case 'unit_activity': {
        const u = units.get(ch.unitId);
        if (u) u.activity = ch.activity;
        break;
      }
      case 'unit_discipline': {
        const u = units.get(ch.unitId);
        if (u) u.discipline = ch.discipline;
        break;
      }
      case 'destruction':
        destruction.push(ch.item);
        break;
      case 'smoke':
        smoke.push(ch.item);
        break;
      case 'ground_object':
        groundObjects.push(ch.item);
        break;
    }
  }
  for (const u of units.values()) {
    const k = interpolateKeyframes(pb.keyframes[u.id], ms);
    if (k) {
      u.pos = k.pos;
      u.workProgress = k.workProgress;
    }
  }
  return {
    tick: pb.tick,
    elapsedMs: ms,
    observer: pb.observer,
    units: pb.baseline.units.map((u) => units.get(u.id)!),
    groundObjects,
    destruction,
    smoke,
    night: pb.baseline.night,
  };
}

/** 截至 elapsedMs 已发生的剧本日志。 */
export function playedLog(pb: TickPlayback, elapsedMs: number): TickPlayback['log'] {
  return pb.log.filter((l) => l.ms <= elapsedMs);
}

/** 傻瓜走表黑盒：输入任意 TickPlayback 与当前毫秒，吐出可直接渲染的帧快照、日志与震屏态。 */
export function evaluateFrameAt(
  pb: TickPlayback | null | undefined,
  fallbackSnapshot: WorldSnapshot,
  elapsedMs: number,
): {
  snapshot: WorldSnapshot;
  elapsedMs: number;
  shaking: boolean;
  log: PlaybackLogEntry[];
} {
  if (!pb) {
    return {
      snapshot: fallbackSnapshot,
      elapsedMs: 0,
      shaking: false,
      log: [],
    };
  }
  return {
    snapshot: progressiveSnapshot(pb, elapsedMs),
    elapsedMs,
    shaking: pb.shakeAtMs.some((ms) => Math.abs(elapsedMs - ms) < 400),
    log: playedLog(pb, elapsedMs),
  };
}

/** 将真后端 `TimeMachineFramePayload` 转成防 t=0s 剧透、支持分段关键帧与 `stop_exact_ms` 的 `TickPlayback`。 */
export function convertBackendPlayback(
  prevSnap: WorldSnapshot,
  finalSnap: WorldSnapshot,
  payload: BackendTimeMachinePayload,
  observer: WorldSnapshot['observer'],
): TickPlayback {
  const prevById = new Map<number, UnitState>(prevSnap.units.map((u) => [u.id, cloneUnit(u)]));
  const finalById = new Map<number, UnitState>(finalSnap.units.map((u) => [u.id, cloneUnit(u)]));
  const baselineMap = new Map<number, UnitState>();

  for (const base of payload.unit_baselines ?? []) {
    const uid = Number(base.piece_id);
    if (!Number.isFinite(uid)) continue;
    const ref = prevById.get(uid) ?? finalById.get(uid);
    const track = (payload.particle_tracks ?? []).find((t) => Number(t.piece_id) === uid);
    const startPos = track ? { x: track.pos_start[0], y: track.pos_start[1] } : ref ? { ...ref.pos } : { x: 0, y: 0 };
    baselineMap.set(uid, {
      id: uid,
      name: ref?.name ?? `Unit-${uid}`,
      team: base.team,
      role: ref?.role ?? 'infantry_squad',
      pos: startPos,
      soldiers: base.men_start ?? ref?.soldiers ?? 0,
      maxSoldiers: ref?.maxSoldiers ?? base.men_start ?? 9,
      ammo: base.ammo_start ?? ref?.ammo ?? 0,
      maxAmmo: ref?.maxAmmo ?? base.ammo_start ?? 360,
      suppression: base.suppression_start ?? ref?.suppression ?? 0,
      workProgress: 0,
      discipline: ref?.discipline ?? 'return_fire',
      alive: true,
      activity: ref?.activity ?? { type: 'idle' },
      visTier: ref?.visTier ?? 'full',
      facingDeg: ref?.facingDeg ?? 0,
      isVehicle: ref?.isVehicle ?? false,
      hp: base.hp_start ?? ref?.hp,
      maxHp: ref?.maxHp ?? base.hp_start,
      posture: ref?.posture ?? 'active',
    });
  }
  for (const u of finalSnap.units) {
    if (!baselineMap.has(u.id)) {
      const prev = prevById.get(u.id);
      if (prev) baselineMap.set(u.id, cloneUnit(prev));
    }
  }

  const keyframes: Record<number, UnitKeyframe[]> = {};
  const changes: DiscreteChange[] = [];
  const log: PlaybackLogEntry[] = [];
  const shakeAtMs: number[] = [];

  for (const track of payload.particle_tracks ?? []) {
    const uid = Number(track.piece_id);
    if (!Number.isFinite(uid)) continue;
    // 严格遵守预渲染缓冲环铁律：initial_visibility === 0（is_prerender_skirt）在 t < 16s 禁止绘制泄露
    if (track.is_prerender_skirt && track.initial_visibility === 0) {
      baselineMap.delete(uid);
      continue;
    }
    const rawKfs =
      track.keyframes && track.keyframes.length > 0
        ? track.keyframes
        : [
            [0, track.pos_start] as [number, [number, number]],
            [track.stop_exact_ms ?? TICK_MS, track.pos_end] as [number, [number, number]],
          ];
    keyframes[uid] = rawKfs.map(([ms, [x, y]], idx) => ({
      ms,
      pos: { x, y },
      workProgress: idx / Math.max(1, rawKfs.length - 1),
      stopExactMs: track.stop_exact_ms,
    }));
    if (track.stop_exact_ms !== undefined) {
      changes.push({ ms: track.stop_exact_ms, type: 'unit_alive', unitId: uid, alive: false });
      changes.push({ ms: track.stop_exact_ms, type: 'unit', unitId: uid, field: 'soldiers', value: 0 });
    }
  }

  // 根据 exact_ms 渐进回放弹耗、伤亡、压制与弹坑事件
  const runningSoldiers = new Map<number, number>();
  const runningAmmo = new Map<number, number>();
  const runningSupp = new Map<number, number>();
  for (const [uid, u] of baselineMap) {
    runningSoldiers.set(uid, u.soldiers);
    runningAmmo.set(uid, u.ammo);
    runningSupp.set(uid, u.suppression);
  }

  const sortedEvents = [...(payload.playback_events ?? [])].sort((a, b) => a.exact_ms - b.exact_ms);
  for (const ev of sortedEvents) {
    const srcId = ev.source_piece_id ? Number(ev.source_piece_id) : undefined;
    const tgtId = ev.target_piece_id ? Number(ev.target_piece_id) : undefined;
    if (srcId !== undefined && Number.isFinite(srcId) && ev.ammo_spent && ev.ammo_spent > 0) {
      const nextAmmo = Math.max(0, (runningAmmo.get(srcId) ?? 0) - ev.ammo_spent);
      runningAmmo.set(srcId, nextAmmo);
      changes.push({ ms: ev.exact_ms, type: 'unit', unitId: srcId, field: 'ammo', value: nextAmmo });
    }
    if (tgtId !== undefined && Number.isFinite(tgtId)) {
      if (ev.casualties && ev.casualties > 0) {
        const nextMen = Math.max(0, (runningSoldiers.get(tgtId) ?? 0) - ev.casualties);
        runningSoldiers.set(tgtId, nextMen);
        changes.push({ ms: ev.exact_ms, type: 'unit', unitId: tgtId, field: 'soldiers', value: nextMen });
      }
      if (ev.exact_supp_add && ev.exact_supp_add > 0) {
        const nextSupp = Math.min(100, (runningSupp.get(tgtId) ?? 0) + ev.exact_supp_add * 10);
        runningSupp.set(tgtId, nextSupp);
        changes.push({ ms: ev.exact_ms, type: 'unit', unitId: tgtId, field: 'suppression', value: nextSupp });
      }
    }
    if (ev.event_kind === 'mine_trigger' || ev.event_kind === 'detonation' || ev.category === 'Shell') {
      shakeAtMs.push(ev.exact_ms);
      if (ev.terrain_cell) {
        changes.push({
          ms: ev.exact_ms,
          type: 'destruction',
          item: {
            id: `ev-crater-${payload.tick}-${ev.exact_ms}`,
            kind: 'crater',
            pos: { x: ev.terrain_cell[1] * 8 + 4, y: ev.terrain_cell[0] * 8 + 4 },
            radiusM: 12,
            createdTick: payload.tick,
          },
        });
      }
    }
    log.push({
      ms: ev.exact_ms,
      kind: ev.casualties && ev.casualties > 0 ? 'casualty' : 'fire',
      text: `[${(ev.exact_ms / 1000).toFixed(1)}s] ${ev.event_kind} ${srcId ?? '?'} -> ${tgtId ?? '-'}${ev.hp_delta ? ` (-${ev.hp_delta.toFixed(1)}HP)` : ''}`,
    });
  }

  return {
    tick: payload.tick,
    observer,
    baseline: {
      tick: Math.max(0, payload.tick - 1),
      elapsedMs: 0,
      observer,
      units: Array.from(baselineMap.values()),
      groundObjects: prevSnap.groundObjects,
      destruction: prevSnap.destruction,
      smoke: prevSnap.smoke,
      night: prevSnap.night,
    },
    keyframes,
    changes,
    log,
    shakeAtMs,
    completedDirectiveIds: [],
    effectiveWaveIds: [],
    final: finalSnap,
  };
}
