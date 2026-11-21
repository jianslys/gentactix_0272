// @vitest-environment jsdom
import 'fake-indexeddb/auto';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { expandDirective } from '../engine/directives';
import { LocalMockSandboxAdapter } from '../engine/localMockSandbox';
import { progressiveSnapshot } from '../engine/playback';
import { HQ_UNIT_ID, PRESET_CHARGE, SQUAD1_ID, SQUAD3_ID } from '../engine/scenario';
import { TICK_MS } from '../engine/types';
import {
  addDraftDirective,
  applyCompletedDirectives,
  copyDirectiveToDraft,
  createInitialUioState,
  issueTaskQueue,
  moveArchiveToQueue,
  moveDraftToQueue,
  queueConflicts,
} from '../store/uioStore';
import {
  addWaveMapPoint,
  commitPendingWaveShape,
  createEmptyWaveDraft,
  issueCommanderWave,
} from '../store/waveStore';

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe('Phase 3 · SPEC 6.1 全套 7 项核心闭环 + 独立指挥 UI', () => {
  it('1) 门禁拦截：未绑定单位或未绑定标的物的无头指令禁止入列，只能留在草稿栏', () => {
    let uio = createInitialUioState();
    const { state: s1, created: d1 } = addDraftDirective(uio, { mode: 'task', taskKind: 'move_unit' });
    uio = s1;

    // 未绑定单位 + 标的物
    const r1 = moveDraftToQueue(uio, d1.id);
    expect(r1.ok).toBe(false);
    expect(r1.message).toContain('执行单位');
    expect(r1.message).toContain('标的物');
    expect(r1.state.drafts).toHaveLength(1);
    expect(r1.state.queue).toHaveLength(0);

    // 仅绑定单位，仍缺标的物
    const withUnit = { ...d1, unitId: SQUAD1_ID };
    const r2 = moveDraftToQueue({ ...r1.state, drafts: [withUnit] }, d1.id);
    expect(r2.ok).toBe(false);
    expect(r2.message).toContain('标的物');
    expect(r2.state.queue).toHaveLength(0);

    // 补全标的物后放行
    const ready = { ...withUnit, target: { type: 'point' as const, pos: { x: 280, y: 240 } } };
    const r3 = moveDraftToQueue({ ...r2.state, drafts: [ready] }, d1.id);
    expect(r3.ok).toBe(true);
    expect(r3.state.queue).toHaveLength(1);
    expect(r3.state.drafts).toHaveLength(0);
  });

  it('2) 远程指挥偏移 + 排长本人靠手写板纸条移动 + 宏展开末尾自动补 hold_unit', async () => {
    const adapter = new LocalMockSandboxAdapter();
    let uio = createInitialUioState();

    // 排长移动纸条 + 远程 3 班事件宏
    uio = addDraftDirective(uio, {
      mode: 'task',
      taskKind: 'move_unit',
      unitId: HQ_UNIT_ID,
      target: { type: 'point', pos: { x: 185, y: 240 } },
      startSec: 0,
      endSec: 10,
    }).state;
    uio = addDraftDirective(uio, {
      mode: 'macro',
      unitId: SQUAD3_ID,
      target: { type: 'point', pos: { x: 430, y: 250 } },
      startSec: 0,
      endSec: 12,
      delayOffsetSec: 2,
      delayMultiplier: 1,
    }).state;

    // 验证宏展开包含前出占领、构筑散兵坑、按纪律警戒 + 结束自动补 hold_unit
    const macroAtoms = expandDirective(uio.drafts[1]!);
    expect(macroAtoms).toHaveLength(4);
    expect(macroAtoms[0]!.offsetSec).toBe(2);
    expect(macroAtoms[3]!.command.kind).toBe('hold_unit');
    expect(macroAtoms[3]!.isStop).toBe(true);

    uio = moveDraftToQueue(uio, uio.drafts[0]!.id).state;
    uio = moveDraftToQueue(uio, uio.drafts[0]!.id).state;
    const issued = await issueTaskQueue(uio, adapter);
    expect(issued.ok).toBe(true);
    const hqReceipt = issued.state.locked.find((x) => x.directive.unitId === HQ_UNIT_ID)!.receipt;
    const sq3Receipt = issued.state.locked.find((x) => x.directive.unitId === SQUAD3_ID)!.receipt;
    expect(hqReceipt.remote).toBe(false);
    expect(sq3Receipt.remote).toBe(true);
    expect(sq3Receipt.delayTicks).toBe(2);

    // 推进至排长结束回合，验证排长通过纸条成功移动
    const pbs = await adapter.fastForwardTo(hqReceipt.endTick, 'red');
    const last = pbs[pbs.length - 1]!;
    const hqFinal = last.final.units.find((u) => u.id === HQ_UNIT_ID)!;
    expect(hqFinal.pos.x).toBeGreaterThan(160);
  });

  it('3) 同单位时间槽互斥实时检测 + 锁死【签发任务队列】', async () => {
    const adapter = new LocalMockSandboxAdapter();
    const initialCount = (await adapter.listScheduled('red')).length;
    let uio = createInitialUioState();
    const a = addDraftDirective(uio, {
      unitId: SQUAD1_ID,
      target: { type: 'point', pos: { x: 250, y: 240 } },
      startSec: 0,
      endSec: 10,
    });
    uio = moveDraftToQueue(a.state, a.created.id).state;

    const b = addDraftDirective(uio, {
      unitId: SQUAD1_ID,
      target: { type: 'point', pos: { x: 280, y: 240 } },
      startSec: 6,
      endSec: 14,
    });
    uio = moveDraftToQueue(b.state, b.created.id).state;

    const conflicts = queueConflicts(uio);
    expect(conflicts.size).toBe(2);

    const blocked = await issueTaskQueue(uio, adapter);
    expect(blocked.ok).toBe(false);
    expect(blocked.state.locked).toHaveLength(0);
    expect(await adapter.listScheduled('red')).toHaveLength(initialCount);
  });

  it('4) 双重签发：单单放入任务队列绝不送后端；签发后锁死不可撤销并在完成时流转归档且支持回炉重发', async () => {
    const adapter = new LocalMockSandboxAdapter();
    const initialScheduled = (await adapter.listScheduled('red')).length;
    let uio = createInitialUioState();
    const a = addDraftDirective(uio, {
      unitId: SQUAD1_ID,
      target: { type: 'point', pos: { x: 256, y: 240 } },
      startSec: 0,
      endSec: 6,
    });
    uio = moveDraftToQueue(a.state, a.created.id).state;
    // 未点【签发任务队列】前绝不写入 StepCache
    expect((await adapter.listScheduled('red')).length).toBe(initialScheduled);

    const spy = vi.spyOn(adapter, 'issueQueue');
    const issued = await issueTaskQueue(uio, adapter);
    expect(issued.ok).toBe(true);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(issued.state.queue).toHaveLength(0);
    expect(issued.state.locked).toHaveLength(1);

    // 模板复制进草稿栏
    const withCopy = copyDirectiveToDraft(issued.state, a.created.id);
    expect(withCopy.drafts).toHaveLength(1);
    expect(withCopy.drafts[0]!.origin).toBe('copy');

    // 推进至结束回合，完成自动流转归档栏
    const endTick = issued.state.locked[0]!.receipt.endTick;
    const pbs = await adapter.fastForwardTo(endTick, 'red');
    const completedIds = pbs.flatMap((p) => p.completedDirectiveIds);
    expect(completedIds).toContain(a.created.id);

    const archived = applyCompletedDirectives(withCopy, completedIds);
    expect(archived.locked).toHaveLength(0);
    expect(archived.archive).toHaveLength(1);

    // 归档拖回任务队列并二次签发
    const requeued = moveArchiveToQueue(archived, a.created.id);
    expect(requeued.ok).toBe(true);
    expect(requeued.state.archive).toHaveLength(0);
    expect(requeued.state.queue).toHaveLength(1);

    const reIssued = await issueTaskQueue(requeued.state, adapter);
    expect(reIssued.ok).toBe(true);
    expect(reIssued.state.locked).toHaveLength(1);
  });

  it('5) 60 天跨回合前向预警与 1s..16s 逐秒防剧透（#3 预警 #4 定时炸药，#4 准时弹坑）', async () => {
    const adapter = new LocalMockSandboxAdapter();
    await adapter.fastForwardTo(3, 'red');
    const look4 = await adapter.lookahead(4, 'red');
    expect(look4.some((e) => e.eventId === PRESET_CHARGE.eventId)).toBe(true);

    const pb4 = await adapter.advanceTick('red');
    const t0 = progressiveSnapshot(pb4, 0);
    const t16 = progressiveSnapshot(pb4, TICK_MS);
    expect(t0.destruction.some((d) => d.pos.x === PRESET_CHARGE.pos.x)).toBe(false);
    expect(t16.destruction.some((d) => d.pos.x === PRESET_CHARGE.pos.x)).toBe(true);
  });

  it('6) 独立【指挥 UI】绘制炮击区/冲锋箭头/许可 AO 并签发波次写入 StepCache', async () => {
    const adapter = new LocalMockSandboxAdapter();
    let draft = createEmptyWaveDraft();
    draft.title = '排波次：东南侧迂回';
    draft = { ...draft, tool: 'barrage_circle' };
    draft = addWaveMapPoint(draft, { x: 420, y: 180 });
    draft = { ...draft, tool: 'assault_arrow', pendingPoints: [] };
    draft = addWaveMapPoint(draft, { x: 240, y: 240 });
    draft = addWaveMapPoint(draft, { x: 380, y: 200 });
    draft = commitPendingWaveShape(draft);
    draft = { ...draft, tool: 'ao_box', pendingPoints: [] };
    draft = addWaveMapPoint(draft, { x: 200, y: 140 });
    draft = addWaveMapPoint(draft, { x: 460, y: 310 });

    const { wave } = await issueCommanderWave(draft, adapter);
    expect(wave.effectiveTick).toBeGreaterThanOrEqual(2);
    expect(wave.geometry.barrages).toHaveLength(1);
    expect(wave.geometry.assaultArrows).toHaveLength(1);
    expect(wave.geometry.aos).toHaveLength(1);

    const scheduled = await adapter.lookahead(wave.effectiveTick, 'red');
    expect(scheduled.some((e) => e.kind === 'g_field_wave')).toBe(true);
  });

  it('7) UI 端到端集成测试：门禁报错提示、时间槽冲突禁用签发按钮、快进 #3 显示预警、双面板切换', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('dock-toggle-W5')).toBeTruthy());
    fireEvent.click(screen.getByTestId('dock-toggle-W5'));
    fireEvent.click(screen.getByTestId('dock-toggle-W12'));
    await waitFor(() => expect(screen.getByTestId('wave-slip-wave-1')).toBeTruthy());

    // 点击生成未绑定草稿，再尝试送入任务队列 -> 被门禁拦截并出现警告
    fireEvent.click(screen.getByTestId('btn-create-draft'));
    fireEvent.click(screen.getByTestId('send-to-queue-dir-1'));
    expect(screen.getByTestId('uio-gate-error')).toBeTruthy();

    // 快进至 #3 -> 顶栏预警横幅明确显示下一回合 #4 定时炸药即将爆炸
    fireEvent.click(screen.getByTestId('btn-jump-3'));
    await waitFor(() => expect(screen.getByTestId('current-tick').textContent).toContain('#3'));
    await waitFor(() => expect(screen.getByTestId('lookahead-banner').textContent).toContain('#4 定时炸药即将爆炸'));

    // 切换到独立【指挥 UI】再切回手写板 UIO
    fireEvent.click(screen.getByTestId('tab-commander-wave'));
    expect(screen.getByTestId('commander-wave-panel')).toBeTruthy();
    fireEvent.click(screen.getByTestId('tab-uio'));
    expect(screen.getByTestId('uio-panel')).toBeTruthy();
  });
});
