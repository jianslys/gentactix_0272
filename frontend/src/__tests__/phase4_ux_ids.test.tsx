// @vitest-environment jsdom
import 'fake-indexeddb/auto';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { App } from '../App';
import { SQUAD1_ID } from '../engine/scenario';
import {
  addDraftDirective,
  createInitialUioState,
  moveDraftToQueue,
  queueConflicts,
  updateQueueDirectiveTime,
} from '../store/uioStore';
import {
  addWaveCircle,
  addWaveMapPoint,
  createEmptyWaveDraft,
  previewWaveGeometry,
  undoWavePoint,
} from '../store/waveStore';

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe('Phase 4 · UI ID 标牌系统 + P0/P1 交互升级', () => {
  it('默认在界面上标出全部窗口与控件 ID，且 T12 开关可一键隐藏/恢复', async () => {
    const { container } = render(<App />);
    await waitFor(() => expect(screen.getByTestId('wave-slip-wave-1')).toBeTruthy());

    for (const id of ['W1', 'W2', 'W3', 'W4', 'W5', 'W5A', 'W6', 'T2', 'T12', 'U8', 'U9']) {
      expect(container.querySelector(`[data-ui-tag="${id}"]`)).not.toBeNull();
    }

    fireEvent.click(screen.getByTestId('toggle-ui-ids'));
    expect(container.querySelectorAll('[data-ui-tag]').length).toBe(0);

    fireEvent.click(screen.getByTestId('toggle-ui-ids'));
    expect(container.querySelector(`[data-ui-tag="W1"]`)).not.toBeNull();
  });

  it('右侧抽屉折叠开关与走表倍速切换正常工作', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('uio-panel')).toBeTruthy());

    fireEvent.click(screen.getByTestId('btn-collapse-right'));
    expect(screen.queryByTestId('uio-panel')).toBeNull();

    fireEvent.click(screen.getByTestId('btn-collapse-right'));
    expect(screen.getByTestId('uio-panel')).toBeTruthy();

    const speedSel = screen.getByTestId('select-playback-speed') as HTMLSelectElement;
    fireEvent.change(speedSel, { target: { value: '4' } });
    expect(speedSel.value).toBe('4');
  });

  it('waveStore 支持自定义圆半径、橡皮筋光标实时预览与单步撤销', () => {
    let draft = createEmptyWaveDraft();
    draft = addWaveCircle(draft, { x: 400, y: 200 }, 65);
    expect(draft.geometry.barrages[0]).toEqual({ type: 'circle', center: { x: 400, y: 200 }, radiusM: 65 });

    draft = { ...draft, tool: 'ao_box', pendingPoints: [] };
    draft = addWaveMapPoint(draft, { x: 100, y: 120 });
    const rubberAo = previewWaveGeometry(draft, { x: 260, y: 280 });
    expect(rubberAo?.aos).toHaveLength(1);
    expect(rubberAo?.aos[0]).toEqual({ min: { x: 100, y: 120 }, max: { x: 260, y: 280 } });

    draft = undoWavePoint(draft);
    expect(draft.pendingPoints).toHaveLength(0);
    draft = undoWavePoint(draft);
    expect(draft.geometry.barrages).toHaveLength(0);
  });

  it('uioStore 内联修改队列纸条起止时间可实时消除互斥冲突', () => {
    let uio = createInitialUioState();
    const d1 = addDraftDirective(uio, {
      unitId: SQUAD1_ID,
      target: { type: 'point', pos: { x: 250, y: 240 } },
      startSec: 0,
      endSec: 10,
    });
    uio = moveDraftToQueue(d1.state, d1.created.id).state;
    const d2 = addDraftDirective(uio, {
      unitId: SQUAD1_ID,
      target: { type: 'point', pos: { x: 280, y: 240 } },
      startSec: 5,
      endSec: 14,
    });
    uio = moveDraftToQueue(d2.state, d2.created.id).state;
    expect(queueConflicts(uio).size).toBe(2);

    uio = updateQueueDirectiveTime(uio, d2.created.id, 10, 16);
    expect(queueConflicts(uio).size).toBe(0);
  });
});
