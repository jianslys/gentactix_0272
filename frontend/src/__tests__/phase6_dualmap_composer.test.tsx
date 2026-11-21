// @vitest-environment jsdom
import 'fake-indexeddb/auto';
import fs from 'node:fs';
import path from 'node:path';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { ART_ASSET_MANIFEST } from '../art/assetManifest';
import { applyThemeTokensToRoot, THEME_TOKENS } from '../art/themeTokens';

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.restoreAllMocks();
});

describe('Phase 6 · 单壳双地图视图 + 纯四区手写板 W5 / 独立指令编写器 W12 + 美术与主题变量闭环', () => {
  it('1) 默认全屏主地图(field)且 W5/W12 收起，按 [M]/按钮展开参谋副地图(tactical)带 unfolding 动画与纸质边饰再折回', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('nine-layer-viewport')).toBeTruthy());

    const viewport = screen.getByTestId('nine-layer-viewport');
    expect(viewport.getAttribute('data-map-view')).toBe('field');
    expect(viewport.getAttribute('data-map-transition')).toBe('idle');
    expect(screen.queryByTestId('uio-panel')).toBeNull();
    expect(screen.queryByTestId('directive-composer')).toBeNull();
    expect(screen.queryByTestId('tactical-map-title')).toBeNull();
    expect(screen.queryByTestId('tactical-map-stamp')).toBeNull();
    expect(screen.queryByTestId('tactical-compass')).toBeNull();
    expect(screen.queryByTestId('tactical-scale-bar')).toBeNull();

    fireEvent.click(screen.getByTestId('toggle-map-view'));
    expect(viewport.getAttribute('data-map-view')).toBe('tactical');
    expect(viewport.getAttribute('data-map-transition')).toBe('unfolding');
    expect(screen.getByTestId('tactical-map-title')).toBeTruthy();
    expect(screen.getByTestId('tactical-map-stamp')).toBeTruthy();
    expect(screen.getByTestId('tactical-compass')).toBeTruthy();
    expect(screen.getByTestId('tactical-scale-bar')).toBeTruthy();

    fireEvent.click(screen.getByTestId('top-toggle-map-view'));
    expect(viewport.getAttribute('data-map-view')).toBe('field');
    expect(viewport.getAttribute('data-map-transition')).toBe('folding');
    expect(screen.queryByTestId('tactical-map-title')).toBeNull();

    fireEvent.keyDown(window, { key: 'm', code: 'KeyM' });
    expect(viewport.getAttribute('data-map-view')).toBe('tactical');
    expect(viewport.getAttribute('data-map-transition')).toBe('unfolding');

    fireEvent.keyDown(window, { key: 'M', code: 'KeyM' });
    expect(viewport.getAttribute('data-map-view')).toBe('field');
    expect(viewport.getAttribute('data-map-transition')).toBe('folding');
  });

  it('2) W5 手写板严格只排纸条(4 栏)，W12 编写器独立生成无头草稿并写入 W5，且零 display:none 伪装节点', async () => {
    const { container } = render(<App />);
    await waitFor(() => expect(screen.getByTestId('dock-toggle-W5')).toBeTruthy());
    fireEvent.click(screen.getByTestId('dock-toggle-W5'));
    fireEvent.click(screen.getByTestId('dock-toggle-W12'));
    await waitFor(() => expect(screen.getByTestId('wave-slip-wave-1')).toBeTruthy());

    // 确认页面内不存在 display: none 的隐藏假控件兜底区
    const allEls = Array.from(container.querySelectorAll<HTMLElement>('*'));
    const hiddenInlineEls = allEls.filter((el) => el.style.display === 'none');
    expect(hiddenInlineEls).toHaveLength(0);

    const w5Panel = screen.getByTestId('uio-panel');
    expect(within(w5Panel).getByTestId('uio-wave-bar')).toBeTruthy();
    expect(within(w5Panel).getByTestId('uio-queue-lane')).toBeTruthy();
    expect(within(w5Panel).getByTestId('uio-draft-lane')).toBeTruthy();
    expect(within(w5Panel).getByTestId('uio-archive-lane')).toBeTruthy();
    expect(within(w5Panel).getByTestId('open-composer-w12')).toBeTruthy();
    expect(within(w5Panel).queryByTestId('btn-create-draft')).toBeNull();
    expect(within(w5Panel).queryByTestId('select-mode')).toBeNull();

    const w12Composer = screen.getByTestId('directive-composer');
    expect(within(w12Composer).getByTestId('select-mode')).toBeTruthy();
    expect(within(w12Composer).getByTestId('select-task-kind')).toBeTruthy();
    expect(within(w12Composer).getByTestId('btn-pick-target')).toBeTruthy();
    expect(within(w12Composer).getByTestId('btn-create-draft')).toBeTruthy();

    fireEvent.click(within(w12Composer).getByTestId('btn-create-draft'));
    const draftLane = screen.getByTestId('uio-draft-lane');
    expect(within(draftLane).getByTestId('draft-card-dir-1')).toBeTruthy();
  });

  it('3) Layer 2 多段工兵工事贴图按邻接线段逐段旋转平铺 drawImage（3 顶点调用 2 次）', async () => {
    vi.resetModules();
    const fakeTrenchImg = { complete: true, naturalWidth: 64, naturalHeight: 32 } as unknown as HTMLImageElement;
    vi.doMock('../art/assetLoader', () => ({
      getArtImage: (key: string) => (key === 'decal.trench' ? fakeTrenchImg : null),
    }));

    try {
      const { renderLayer2Destruction } = await import('../render/canvasLayers');
      const drawImageSpy = vi.fn();
      const noop = () => {};
      const mockCtx = {
        clearRect: noop,
        save: noop,
        restore: noop,
        translate: noop,
        rotate: noop,
        drawImage: drawImageSpy,
        beginPath: noop,
        moveTo: noop,
        lineTo: noop,
        stroke: noop,
        arc: noop,
        fill: noop,
        createRadialGradient: () => ({ addColorStop: noop }),
        setLineDash: noop,
        fillStyle: '',
        strokeStyle: '',
        lineWidth: 1,
        lineCap: 'round',
        lineJoin: 'round',
      } as unknown as CanvasRenderingContext2D;

      renderLayer2Destruction(
        mockCtx,
        {
          tick: 1,
          elapsedMs: 16000,
          observer: 'red',
          units: [],
          groundObjects: [],
          destruction: [
            {
              id: 'trench-multi-1',
              kind: 'trench',
              pos: { x: 100, y: 100 },
              radiusM: 6,
              points: [
                { x: 100, y: 100 },
                { x: 140, y: 120 },
                { x: 190, y: 115 },
              ],
              createdTick: 1,
            },
          ],
          smoke: [],
          night: false,
        },
        { widthPx: 800, heightPx: 600, worldWidthM: 640, worldHeightM: 480 },
      );

      expect(drawImageSpy).toHaveBeenCalledTimes(2);
      expect(drawImageSpy.mock.calls[0]?.[0]).toBe(fakeTrenchImg);
      expect(drawImageSpy.mock.calls[1]?.[0]).toBe(fakeTrenchImg);
    } finally {
      vi.doUnmock('../art/assetLoader');
      vi.resetModules();
    }
  });

  it('4) 美术 Manifest 新增参谋纸底图与纸张纹理槽位，且 styles.css 消费 --gtx-* 主题变量', () => {
    expect(ART_ASSET_MANIFEST['map.tactical_paper']).toBeTruthy();
    expect(ART_ASSET_MANIFEST['map.tactical_paper'].layer).toBe('L1');
    expect(ART_ASSET_MANIFEST['fx.paper_grain']).toBeTruthy();
    expect(ART_ASSET_MANIFEST['fx.paper_grain'].layer).toBe('L9');

    applyThemeTokensToRoot();
    expect(document.documentElement.style.getPropertyValue('--gtx-bg-app')).toBe(THEME_TOKENS.bgApp);
    expect(document.documentElement.style.getPropertyValue('--gtx-text-primary')).toBe(THEME_TOKENS.textPrimary);

    const cssPath = path.resolve(process.cwd(), 'src/styles.css');
    const cssText = fs.readFileSync(cssPath, 'utf8');
    expect(cssText).toContain('var(--gtx-bg-app');
    expect(cssText).toContain('var(--gtx-text-primary');
  });
});
