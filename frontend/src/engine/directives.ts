// GenTactix V2 frontend · 无头指令工具：准入门禁、远程指挥偏移、事件宏展开
// 纯函数模块，UIO 状态机与沙盒共用。

import type {
  AtomicCommandPlan,
  CommandDelayInfo,
  DirectiveTarget,
  DirectiveTaskKind,
  HeadlessDirective,
  TacticalCommandJson,
  UnitState,
  Vec2,
} from './types';
import { TICK_SECONDS } from './types';

/** 远程指挥：每超出直接指挥半径 80m 增加 1 拍延迟（电台通联乘数 η = 1）。 */
export const REMOTE_DELAY_METERS_PER_TICK = 80;

export const TASK_KIND_LABEL: Record<DirectiveTaskKind, string> = {
  move_unit: '机动 Move',
  attack_pos: '火力压制 AttackPos',
  attack_unit: '攻击单位 AttackUnit',
  hold_unit: '停止待命 Hold',
  set_fire_discipline: '开火纪律 Discipline',
  pop_smoke: '施放烟幕 PopSmoke',
  build_work: '工事构筑 BuildWork',
};

export const MACRO_LABEL = '事件宏：前出占领 → 构筑散兵坑 → 按纪律警戒';

export function distance(a: Vec2, b: Vec2): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/** 该任务类型需要的标的物形态；null 表示无需标的物。 */
export function requiredTargetType(d: Pick<HeadlessDirective, 'mode' | 'taskKind'>): DirectiveTarget['type'] | null {
  if (d.mode === 'macro') return 'point';
  switch (d.taskKind) {
    case 'move_unit':
      return 'point';
    case 'attack_pos':
      return 'point';
    case 'attack_unit':
      return 'unit';
    case 'pop_smoke':
      return 'point';
    case 'build_work':
      return 'polyline';
    case 'hold_unit':
    case 'set_fire_discipline':
    case undefined:
      return null;
  }
}

export function targetSatisfies(required: DirectiveTarget['type'] | null, target: DirectiveTarget | undefined): boolean {
  if (required === null) return true;
  if (!target) return false;
  if (required === 'point') return target.type === 'point' || (target.type === 'polyline' && target.points.length > 0);
  if (required === 'polyline') return (target.type === 'polyline' && target.points.length >= 2) || target.type === 'point';
  return target.type === 'unit';
}

export interface GateResult {
  ok: boolean;
  missing: Array<'unit' | 'target' | 'time'>;
  message: string;
}

/** 任务队列栏准入死门禁：未绑定执行单位或缺少标的物一律拦截。 */
export function checkQueueGate(d: HeadlessDirective): GateResult {
  const missing: GateResult['missing'] = [];
  if (d.unitId === undefined) missing.push('unit');
  if (!targetSatisfies(requiredTargetType(d), d.target)) missing.push('target');
  if (!(Number.isFinite(d.startSec) && Number.isFinite(d.endSec) && d.endSec > d.startSec && d.startSec >= 0)) {
    missing.push('time');
  }
  if (missing.length === 0) return { ok: true, missing, message: '' };
  const parts: string[] = [];
  if (missing.includes('unit')) parts.push('执行单位');
  if (missing.includes('target')) parts.push('标的物');
  if (missing.includes('time')) parts.push('合法起止时间');
  return {
    ok: false,
    missing,
    message: `拦截：纸条未绑定${parts.join('、')}，禁止进入任务队列栏，只能留在草稿栏。`,
  };
}

/** 直接/远程指挥判定与延迟偏移拍数。 */
export function computeCommandDelay(hq: UnitState | undefined, unit: UnitState, directRadiusM: number): CommandDelayInfo {
  if (!hq || hq.id === unit.id) {
    return { unitId: unit.id, distanceM: 0, remote: false, delayTicks: 0, delaySec: 0 };
  }
  const d = distance(hq.pos, unit.pos);
  if (d <= directRadiusM) {
    return { unitId: unit.id, distanceM: d, remote: false, delayTicks: 0, delaySec: 0 };
  }
  const delayTicks = Math.max(1, Math.ceil((d - directRadiusM) / REMOTE_DELAY_METERS_PER_TICK));
  return { unitId: unit.id, distanceM: d, remote: true, delayTicks, delaySec: delayTicks * TICK_SECONDS };
}

export function remoteDelayText(info: CommandDelayInfo): string {
  if (!info.remote) return '';
  return `远程指挥：超出直接指挥半径，预计延迟 +${info.delayTicks} 拍 (+${info.delaySec}s)`;
}

/** 施加延迟乘数与偏移后的起止秒。 */
export function effectiveWindow(d: Pick<HeadlessDirective, 'startSec' | 'endSec' | 'delayMultiplier' | 'delayOffsetSec'>): {
  start: number;
  end: number;
} {
  const mult = Number.isFinite(d.delayMultiplier) && d.delayMultiplier > 0 ? d.delayMultiplier : 1;
  const off = Number.isFinite(d.delayOffsetSec) ? d.delayOffsetSec : 0;
  return { start: Math.max(0, d.startSec * mult + off), end: Math.max(0, d.endSec * mult + off) };
}

function targetPoint(t: DirectiveTarget | undefined): Vec2 | undefined {
  if (!t) return undefined;
  if (t.type === 'point') return t.pos;
  if (t.type === 'polyline') return t.points[t.points.length - 1];
  return undefined;
}

function targetPolyline(t: DirectiveTarget | undefined): Vec2[] {
  if (!t) return [];
  if (t.type === 'polyline') return t.points.map((p) => ({ ...p }));
  if (t.type === 'point') return [{ ...t.pos }, { x: t.pos.x + 16, y: t.pos.y }];
  return [];
}

function buildTaskCommand(d: HeadlessDirective, unitId: number): TacticalCommandJson {
  const kind = d.taskKind ?? 'hold_unit';
  switch (kind) {
    case 'move_unit': {
      const pts = d.target?.type === 'polyline' ? d.target.points : d.target?.type === 'point' ? [d.target.pos] : [];
      return { kind, unit_id: unitId, waypoints: pts.map((p) => ({ ...p })), stance: d.stance };
    }
    case 'attack_pos':
      return { kind, unit_id: unitId, target: { ...(targetPoint(d.target) ?? { x: 0, y: 0 }) } };
    case 'attack_unit':
      return { kind, unit_id: unitId, target_id: d.target?.type === 'unit' ? d.target.unitId : -1, chase: false };
    case 'pop_smoke':
      return { kind, unit_id: unitId, target: { ...(targetPoint(d.target) ?? { x: 0, y: 0 }) } };
    case 'build_work':
      return { kind, unit_id: unitId, work: d.work, waypoints: targetPolyline(d.target) };
    case 'set_fire_discipline':
      return { kind, unit_id: unitId, discipline: d.discipline };
    case 'hold_unit':
      return { kind, unit_id: unitId };
  }
}

const round1 = (v: number): number => Math.round(v * 10) / 10;

/**
 * 前端拆宏：把单任务/事件宏按时间轴拆成原子指令；结束时刻自动补一条 hold_unit。
 * 调用前必须已通过 checkQueueGate。
 */
export function expandDirective(d: HeadlessDirective): AtomicCommandPlan[] {
  const gate = checkQueueGate(d);
  if (!gate.ok || d.unitId === undefined) {
    throw new Error(gate.message || '无头指令未绑定执行单位');
  }
  const unitId = d.unitId;
  const { start, end } = effectiveWindow(d);
  const span = end - start;
  const out: AtomicCommandPlan[] = [];
  if (d.mode === 'macro') {
    const dest = targetPoint(d.target) ?? { x: 0, y: 0 };
    out.push({
      directiveId: d.id,
      offsetSec: round1(start),
      command: { kind: 'move_unit', unit_id: unitId, waypoints: [{ ...dest }], stance: d.stance },
      isStop: false,
      label: '前出占领',
    });
    out.push({
      directiveId: d.id,
      offsetSec: round1(start + span * 0.4),
      command: {
        kind: 'build_work',
        unit_id: unitId,
        work: 'foxhole_line',
        waypoints: [
          { x: dest.x - 12, y: dest.y },
          { x: dest.x + 12, y: dest.y },
        ],
      },
      isStop: false,
      label: '构筑散兵坑',
    });
    out.push({
      directiveId: d.id,
      offsetSec: round1(start + span * 0.8),
      command: { kind: 'set_fire_discipline', unit_id: unitId, discipline: d.discipline },
      isStop: false,
      label: '按纪律警戒',
    });
  } else {
    const cmd = buildTaskCommand(d, unitId);
    out.push({
      directiveId: d.id,
      offsetSec: round1(start),
      command: cmd,
      isStop: false,
      label: TASK_KIND_LABEL[cmd.kind],
    });
  }
  out.push({
    directiveId: d.id,
    offsetSec: round1(end),
    command: { kind: 'hold_unit', unit_id: unitId },
    isStop: true,
    label: '结束停止 hold_unit',
  });
  return out;
}

export interface WindowConflict {
  otherId: string;
  start: number;
  end: number;
}

/** 同单位时间槽互斥检测：返回每张纸条的冲突列表（闭区间重叠，端点相接不算冲突）。 */
export function detectConflicts(directives: HeadlessDirective[]): Map<string, WindowConflict[]> {
  const out = new Map<string, WindowConflict[]>();
  for (let i = 0; i < directives.length; i++) {
    for (let j = i + 1; j < directives.length; j++) {
      const a = directives[i]!;
      const b = directives[j]!;
      if (a.unitId === undefined || a.unitId !== b.unitId) continue;
      const wa = effectiveWindow(a);
      const wb = effectiveWindow(b);
      const s = Math.max(wa.start, wb.start);
      const e = Math.min(wa.end, wb.end);
      if (s < e) {
        out.set(a.id, [...(out.get(a.id) ?? []), { otherId: b.id, start: s, end: e }]);
        out.set(b.id, [...(out.get(b.id) ?? []), { otherId: a.id, start: s, end: e }]);
      }
    }
  }
  return out;
}
