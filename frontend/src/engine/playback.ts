// GenTactix V2 frontend · 逐秒防剧透走表：按 elapsedMs 渐进兑现回合初锁定的剧本
// t = 0 严格等于回合初基线；离散伤害/弹耗/弹坑只在到达 exactMs 时跳字；t = 16000 与回合末权威态对齐。

import type { TickPlayback, UnitKeyframe, UnitState, Vec2, WorldSnapshot } from './types';
import { TICK_MS } from './types';

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function interpolateKeyframes(frames: UnitKeyframe[] | undefined, ms: number): { pos: Vec2; workProgress: number } | undefined {
  if (!frames || frames.length === 0) return undefined;
  const first = frames[0]!;
  if (ms <= first.ms) return { pos: { ...first.pos }, workProgress: first.workProgress };
  for (let i = 1; i < frames.length; i++) {
    const b = frames[i]!;
    if (ms <= b.ms) {
      const a = frames[i - 1]!;
      const span = b.ms - a.ms;
      const t = span <= 0 ? 1 : (ms - a.ms) / span;
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
