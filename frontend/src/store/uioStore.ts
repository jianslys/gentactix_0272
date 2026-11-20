// GenTactix V2 frontend · 手写板（UIO）四区状态机 + 宏展开 + 门禁 + 互斥检测 + 双重签发锁死

import type { BackendAdapter } from '../engine/backendAdapter';
import {
  checkQueueGate,
  detectConflicts,
  expandDirective,
  type WindowConflict,
} from '../engine/directives';
import type {
  DirectiveIssueReceipt,
  DirectiveMode,
  DirectiveTarget,
  DirectiveTaskKind,
  EngineerWorkKind,
  FireDiscipline,
  HeadlessDirective,
  MoveStance,
} from '../engine/types';

export interface LockedDirectiveEntry {
  directive: HeadlessDirective;
  receipt: DirectiveIssueReceipt;
}

export interface UioState {
  nextSeq: number;
  drafts: HeadlessDirective[];
  queue: HeadlessDirective[];
  locked: LockedDirectiveEntry[];
  archive: HeadlessDirective[];
  gateError: string | null;
}

export function createInitialUioState(): UioState {
  return {
    nextSeq: 1,
    drafts: [],
    queue: [],
    locked: [],
    archive: [],
    gateError: null,
  };
}

export interface CreateDraftInput {
  mode?: DirectiveMode;
  taskKind?: DirectiveTaskKind;
  unitId?: number;
  target?: DirectiveTarget;
  startSec?: number;
  endSec?: number;
  delayOffsetSec?: number;
  delayMultiplier?: number;
  stance?: MoveStance;
  discipline?: FireDiscipline;
  work?: EngineerWorkKind;
  note?: string;
}

export function addDraftDirective(state: UioState, input: CreateDraftInput = {}): { state: UioState; created: HeadlessDirective } {
  const seq = state.nextSeq;
  const created: HeadlessDirective = {
    id: `dir-${seq}`,
    seq,
    mode: input.mode ?? 'task',
    taskKind: input.taskKind ?? 'move_unit',
    macroKind: input.mode === 'macro' ? 'advance_dig_watch' : undefined,
    unitId: input.unitId,
    target: input.target,
    startSec: input.startSec ?? 0,
    endSec: input.endSec ?? 12,
    delayOffsetSec: input.delayOffsetSec ?? 0,
    delayMultiplier: input.delayMultiplier ?? 1,
    stance: input.stance ?? 'normal',
    discipline: input.discipline ?? 'return_fire',
    work: input.work ?? 'foxhole_line',
    origin: 'new',
    note: input.note,
  };
  return {
    created,
    state: {
      ...state,
      nextSeq: seq + 1,
      drafts: [...state.drafts, created],
      gateError: null,
    },
  };
}

export function updateDraftDirective(
  state: UioState,
  id: string,
  patch: Partial<HeadlessDirective>,
): UioState {
  return {
    ...state,
    drafts: state.drafts.map((d) => (d.id === id ? { ...d, ...patch } : d)),
    gateError: null,
  };
}

export function removeDraftDirective(state: UioState, id: string): UioState {
  return {
    ...state,
    drafts: state.drafts.filter((d) => d.id !== id),
  };
}

/**
 * 尝试将草稿放入【任务队列栏】：
 * 若未绑定 unitId 或缺少 target，当场拦截并留在草稿栏！
 */
export function moveDraftToQueue(state: UioState, id: string): { state: UioState; ok: boolean; message: string } {
  const target = state.drafts.find((d) => d.id === id);
  if (!target) {
    return { state, ok: false, message: '未找到对应草稿纸条。' };
  }
  const gate = checkQueueGate(target);
  if (!gate.ok) {
    return {
      ok: false,
      message: gate.message,
      state: { ...state, gateError: gate.message },
    };
  }
  return {
    ok: true,
    message: '',
    state: {
      ...state,
      drafts: state.drafts.filter((d) => d.id !== id),
      queue: [...state.queue, target],
      gateError: null,
    },
  };
}

export function moveQueueToDraft(state: UioState, id: string): UioState {
  const item = state.queue.find((d) => d.id === id);
  if (!item) return state;
  return {
    ...state,
    queue: state.queue.filter((d) => d.id !== id),
    drafts: [...state.drafts, item],
    gateError: null,
  };
}

/** 将队列/执行中/归档纸条复制副本到草稿栏（模板复用）。 */
export function copyDirectiveToDraft(state: UioState, id: string): UioState {
  const source =
    state.drafts.find((d) => d.id === id) ??
    state.queue.find((d) => d.id === id) ??
    state.locked.find((x) => x.directive.id === id)?.directive ??
    state.archive.find((d) => d.id === id);
  if (!source) return state;
  const seq = state.nextSeq;
  const copy: HeadlessDirective = {
    ...source,
    id: `dir-${seq}`,
    seq,
    origin: 'copy',
  };
  return {
    ...state,
    nextSeq: seq + 1,
    drafts: [...state.drafts, copy],
    gateError: null,
  };
}

/** 将归档栏历史纸条拖回任务队列栏。 */
export function moveArchiveToQueue(state: UioState, id: string): { state: UioState; ok: boolean; message: string } {
  const item = state.archive.find((d) => d.id === id);
  if (!item) {
    return { state, ok: false, message: '归档栏未找到该纸条。' };
  }
  const gate = checkQueueGate(item);
  if (!gate.ok) {
    return { ok: false, message: gate.message, state: { ...state, gateError: gate.message } };
  }
  const revived: HeadlessDirective = { ...item, origin: 'archive' };
  return {
    ok: true,
    message: '',
    state: {
      ...state,
      archive: state.archive.filter((d) => d.id !== id),
      queue: [...state.queue, revived],
      gateError: null,
    },
  };
}

export function queueConflicts(state: UioState): Map<string, WindowConflict[]> {
  return detectConflicts(state.queue);
}

/**
 * 双重签发门禁：
 * - 单单放入 queue 绝不送入后端；
 * - 必须无冲突，点击【签发任务队列】后一次性提交并永久锁死进入 locked，等待完成流转进归档栏。
 */
export async function issueTaskQueue(
  state: UioState,
  adapter: BackendAdapter,
): Promise<{ state: UioState; ok: boolean; message: string }> {
  if (state.queue.length === 0) {
    return { state, ok: false, message: '任务队列栏为空，无法签发。' };
  }
  const conflicts = queueConflicts(state);
  if (conflicts.size > 0) {
    return {
      state: { ...state, gateError: '拦截：同单位时间槽存在冲突，请调整起止时间后再签发。' },
      ok: false,
      message: '同单位时间槽存在冲突，签发已锁定。',
    };
  }
  for (const d of state.queue) {
    const gate = checkQueueGate(d);
    if (!gate.ok) {
      return { state: { ...state, gateError: gate.message }, ok: false, message: gate.message };
    }
  }

  const directives = state.queue.map((d) => ({
    directiveId: d.id,
    unitId: d.unitId!,
    atoms: expandDirective(d),
  }));
  const res = await adapter.issueQueue({ directives });
  const receiptById = new Map<string, DirectiveIssueReceipt>(res.receipts.map((r) => [r.directiveId, r]));

  const newlyLocked: LockedDirectiveEntry[] = state.queue.map((d) => ({
    directive: d,
    receipt: receiptById.get(d.id)!,
  }));

  return {
    ok: true,
    message: `已签发 ${newlyLocked.length} 条纸条并锁死写入 60 天步长缓存`,
    state: {
      ...state,
      queue: [],
      locked: [...state.locked, ...newlyLocked],
      gateError: null,
    },
  };
}

/** 回合演进中收到已执行完毕的指令 ID，自动从 locked 流转进归档栏。 */
export function applyCompletedDirectives(state: UioState, completedIds: string[]): UioState {
  if (completedIds.length === 0) return state;
  const doneSet = new Set(completedIds);
  const stillLocked: LockedDirectiveEntry[] = [];
  const newlyArchived: HeadlessDirective[] = [];
  for (const entry of state.locked) {
    if (doneSet.has(entry.directive.id)) {
      newlyArchived.push(entry.directive);
    } else {
      stillLocked.push(entry);
    }
  }
  if (newlyArchived.length === 0) return state;
  return {
    ...state,
    locked: stillLocked,
    archive: [...newlyArchived, ...state.archive],
  };
}
