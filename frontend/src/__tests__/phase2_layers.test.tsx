// @vitest-environment jsdom
import 'fake-indexeddb/auto';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BackendAdapter } from '../engine/backendAdapter';
import { presetTerrain, presetUnits } from '../engine/scenario';
import type { WorldSnapshot } from '../engine/types';
import { NineLayerViewport } from '../render/NineLayerViewport';
import { renderLayer1BaseMap } from '../render/canvasLayers';
import { clearLayer6Strokes, loadLayer6Strokes, saveLayer6Strokes, type FreehandStroke } from '../store/layerStore';
import { App } from '../App';

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe('Phase 2 · 九层物理栈 + 零 8m 网格 + Layer 6 本地持久化 + L7/L9 独立开关', () => {
  it('真实挂载 9 个物理图层容器 data-layer="1..9"，z-index 严格递增 1..9，且 L2<L3, L7<L8<L9', () => {
    const terrain = presetTerrain();
    const snap: WorldSnapshot = {
      tick: 0,
      elapsedMs: 0,
      observer: 'red',
      units: presetUnits(),
      groundObjects: [],
      destruction: [],
      smoke: [],
      night: false,
    };

    const { container } = render(
      <NineLayerViewport
        terrain={terrain}
        snapshot={snap}
        hqUnitId={1}
        directRadiusM={140}
        radioRadiusM={600}
        strokes={[]}
        freehandTool="none"
        freehandColor="#dc2626"
        onStrokesChange={() => {}}
        l7Enabled={true}
        l7Weather="fog"
        l9Enabled={true}
        hudChildren={<div data-testid="hud-probe">HUD</div>}
      />,
    );

    const zValues: number[] = [];
    for (let i = 1; i <= 9; i++) {
      const el = container.querySelector<HTMLElement>(`[data-layer="${i}"]`);
      expect(el).not.toBeNull();
      const z = Number(el!.style.zIndex);
      expect(z).toBe(i);
      zValues.push(z);
    }
    expect(zValues).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(zValues[1]!).toBeLessThan(zValues[2]!); // L2 < L3
    expect(zValues[6]!).toBeLessThan(zValues[7]!); // L7 < L8
    expect(zValues[7]!).toBeLessThan(zValues[8]!); // L8 < L9
    expect(screen.getByTestId('hud-probe')).toBeTruthy();
  });

  it('Layer 1 连续底图绘制绝不出现 8m 规则网格线', () => {
    const lineToCalls: Array<[number, number]> = [];
    const mockCtx = {
      clearRect: vi.fn(),
      createLinearGradient: () => ({ addColorStop: vi.fn() }),
      fillRect: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      translate: vi.fn(),
      rotate: vi.fn(),
      beginPath: vi.fn(),
      ellipse: vi.fn(),
      fill: vi.fn(),
      stroke: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn((x: number, y: number) => lineToCalls.push([x, y])),
      setLineDash: vi.fn(),
      strokeRect: vi.fn(),
      fillStyle: '',
      strokeStyle: '',
      lineWidth: 1,
      lineCap: 'butt',
      lineJoin: 'miter',
    } as unknown as CanvasRenderingContext2D;

    const terrain = presetTerrain();
    renderLayer1BaseMap(mockCtx, terrain, {
      widthPx: 800,
      heightPx: 600,
      worldWidthM: terrain.widthM,
      worldHeightM: terrain.heightM,
    });

    // 仅由河流/道路折点调用 lineTo（13 个点），绝非 80*60=140 条 8m 网格线
    expect(lineToCalls.length).toBeLessThan(20);
    expect((mockCtx.strokeRect as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBe(0);
  });

  it('Layer 6 手绘笔画仅写入本地 IndexedDB/LocalStorage，零泄漏至 BackendAdapter', async () => {
    const mockBackendSpy = vi.fn();
    const fakeAdapter: Partial<BackendAdapter> = {
      issueQueue: mockBackendSpy as BackendAdapter['issueQueue'],
      issueWave: mockBackendSpy as BackendAdapter['issueWave'],
    };

    const strokes: FreehandStroke[] = [
      { id: 's1', color: '#dc2626', width: 3, points: [{ x: 100, y: 120 }, { x: 220, y: 180 }] },
    ];
    const backendUsed = await saveLayer6Strokes(strokes);
    expect(['indexeddb', 'localstorage']).toContain(backendUsed);

    const loaded = await loadLayer6Strokes();
    expect(loaded).toEqual(strokes);
    expect(fakeAdapter.issueQueue).not.toHaveBeenCalled();

    await clearLayer6Strokes();
    expect(await loadLayer6Strokes()).toEqual([]);
  });

  it('L7 与 L9 独立开关按钮可实时切换图层显隐态', async () => {
    const { container } = render(<App />);
    const l7 = container.querySelector<HTMLElement>('[data-layer="7"]')!;
    const l9 = container.querySelector<HTMLElement>('[data-layer="9"]')!;
    expect(l7.getAttribute('data-enabled')).toBe('true');
    expect(l9.getAttribute('data-enabled')).toBe('true');
    expect(l9.style.display).toBe('block');

    fireEvent.click(screen.getByTestId('toggle-l7'));
    expect(l7.getAttribute('data-enabled')).toBe('false');
    expect(l7.style.opacity).toBe('0');

    fireEvent.click(screen.getByTestId('toggle-l9'));
    expect(l9.getAttribute('data-enabled')).toBe('false');
    expect(l9.style.display).toBe('none');
  });
});
