// @vitest-environment jsdom
import 'fake-indexeddb/auto';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { getArtImage } from '../art/assetLoader';
import { ART_SLOT_LIST } from '../art/assetManifest';
import { applyThemeTokensToRoot, THEME_TOKENS } from '../art/themeTokens';
import { HttpEngineAdapter } from '../engine/httpEngineAdapter';
import { convertBackendPlayback, evaluateFrameAt } from '../engine/playback';
import { presetUnits } from '../engine/scenario';
import { parseTerrainBinary, terrainBinaryToModel } from '../engine/terrainBinary';
import type { IFieldTriplet, WorldSnapshot } from '../engine/types';
import { renderLayer1BaseMap, renderLayer4Entities, renderLayer5Rings } from '../render/canvasLayers';

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.restoreAllMocks();
});

function buildMockTerrainBinary(rows: number, cols: number, cellM = 8): ArrayBuffer {
  const n = rows * cols;
  const buf = new ArrayBuffer(24 + n * 7);
  const u8 = new Uint8Array(buf);
  const magic = 'GTXV2TER';
  for (let i = 0; i < 8; i++) u8[i] = magic.charCodeAt(i);
  const view = new DataView(buf);
  view.setUint32(8, 2, true);
  view.setUint32(12, rows, true);
  view.setUint32(16, cols, true);
  view.setFloat32(20, cellM, true);
  for (let i = 0; i < n; i++) {
    view.setFloat32(24 + i * 4, 12 + (i % 4), true);
  }
  const kindOff = 24 + n * 4;
  u8[kindOff + 0] = 1; // forest
  u8[kindOff + 1] = 2; // water
  u8[kindOff + n + 0] = 255; // cover = 1.0
  u8[kindOff + n * 2 + 1] = 128; // smoke ~= 0.5
  return buf;
}

describe('Phase 5 · 保姆级前端底座（真后端契约 / 浮动窗口 / 全级ID / 美术槽位）', () => {
  it('terrainBinary 解码合法 GTXV2TER 流并拒绝错魔数与截断缓冲', () => {
    const buf = buildMockTerrainBinary(4, 4, 8);
    const parsed = parseTerrainBinary(buf);
    expect(parsed.version).toBe(2);
    expect(parsed.rows).toBe(4);
    expect(parsed.cols).toBe(4);
    expect(parsed.cellM).toBe(8);
    expect(parsed.elevationM[0]).toBeCloseTo(12, 3);
    expect(parsed.kind[0]).toBe(1);
    expect(parsed.kind[1]).toBe(2);
    expect(parsed.coverBonus[0]).toBeCloseTo(1, 2);
    expect(parsed.smokeDensity[1]).toBeCloseTo(128 / 255, 2);

    const model = terrainBinaryToModel(parsed);
    expect(model.widthM).toBe(32);
    expect(model.heightM).toBe(32);
    expect(model.binary).toBe(parsed);

    expect(() => parseTerrainBinary(new ArrayBuffer(12))).toThrow(/short/i);
    const bad = new Uint8Array(buf.slice(0));
    bad[0] = 88; // 'X'
    expect(() => parseTerrainBinary(bad.buffer)).toThrow(/magic/i);
  });

  it('convertBackendPlayback 与 evaluateFrameAt 严防 0s 剧透、剔除预渲染环并支持 stop_exact_ms', () => {
    const baseUnits = presetUnits();
    const prevSnap: WorldSnapshot = {
      tick: 0,
      elapsedMs: 0,
      observer: 'red',
      units: baseUnits,
      groundObjects: [],
      destruction: [],
      smoke: [],
      night: false,
    };
    const finalSnap: WorldSnapshot = {
      ...prevSnap,
      tick: 1,
      units: baseUnits.map((u) => (u.id === 9 ? { ...u, soldiers: 6, alive: true } : u)),
    };
    const pb = convertBackendPlayback(
      prevSnap,
      finalSnap,
      {
        tick: 1,
        window_ms: [0, 16000],
        unit_baselines: [
          { piece_id: '2', team: 'red', men_start: 9, ammo_start: 400, suppression_start: 0 },
          { piece_id: '9', team: 'blue', men_start: 8, ammo_start: 300, suppression_start: 0 },
          { piece_id: '999', team: 'blue', men_start: 5 },
        ],
        particle_tracks: [
          {
            piece_id: '2',
            team: 'red',
            pos_start: [188, 248],
            pos_end: [248, 248],
            initial_visibility: 1,
            is_prerender_skirt: false,
            keyframes: [
              [0, [188, 248]],
              [8000, [218, 248]],
              [16000, [248, 248]],
            ],
          },
          {
            piece_id: '9',
            team: 'blue',
            pos_start: [380, 240],
            pos_end: [380, 240],
            initial_visibility: 1,
            is_prerender_skirt: false,
            keyframes: [[0, [380, 240]]],
            stop_exact_ms: 9000,
          },
          {
            piece_id: '999',
            team: 'blue',
            pos_start: [500, 100],
            pos_end: [500, 100],
            initial_visibility: 0,
            is_prerender_skirt: true,
            keyframes: [[0, [500, 100]]],
          },
        ],
        playback_events: [
          {
            exact_ms: 5000,
            category: 'Bullet',
            event_kind: 'hit',
            source_piece_id: '2',
            target_piece_id: '9',
            casualties: 2,
            ammo_spent: 25,
          },
          {
            exact_ms: 12000,
            category: 'Shell',
            event_kind: 'detonation',
            terrain_cell: [10, 20],
          },
        ],
      },
      'red',
    );

    const frame0 = evaluateFrameAt(pb, finalSnap, 0);
    expect(frame0.snapshot.units.find((u) => u.id === 999)).toBeUndefined();
    expect(frame0.snapshot.units.find((u) => u.id === 9)?.soldiers).toBe(8);
    expect(frame0.shaking).toBe(false);

    const frame6k = evaluateFrameAt(pb, finalSnap, 6000);
    expect(frame6k.snapshot.units.find((u) => u.id === 9)?.soldiers).toBe(6);
    expect(frame6k.snapshot.units.find((u) => u.id === 2)?.ammo).toBe(375);

    const frame9k = evaluateFrameAt(pb, finalSnap, 9000);
    expect(frame9k.snapshot.units.find((u) => u.id === 9)?.alive).toBe(false);

    const frame12k = evaluateFrameAt(pb, finalSnap, 12000);
    expect(frame12k.shaking).toBe(true);
    expect(frame12k.snapshot.destruction.length).toBe(1);
  });

  it('HttpEngineAdapter 正确映射 /health /api/terrain.bin /api/command /api/units/:id /api/fields/i', async () => {
    const binBuf = buildMockTerrainBinary(8, 8, 8);
    const postedBodies: unknown[] = [];

    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input);
        if (url.endsWith('/health')) {
          return new Response(JSON.stringify({ status: 'ok', version: '2.0.0', cell_m: 8, tick_s: 16 }), {
            status: 200,
          });
        }
        if (url.endsWith('/api/terrain.bin')) {
          return new Response(binBuf, { status: 200 });
        }
        if (url.includes('/api/state')) {
          return new Response(
            JSON.stringify({
              tick: 1,
              clock_seconds: 16,
              observer_team: 'red',
              fog_enforced: true,
              units: [
                { id: 1, callsign: '排部', team: 'red', role: 'platoon_hq', pos: { x: 100, y: 200 }, facing_deg: 0, alive: true, vis_tier: 'full', men: 4, ammo_rounds: 200 },
                { id: 2, callsign: '1班', team: 'red', role: 'rifle_squad', pos: { x: 140, y: 200 }, facing_deg: 0, alive: true, vis_tier: 'full', men: 9, ammo_rounds: 360 },
              ],
              visible_instances: [{ instance_id: 7, kind: 'dropped_weapon', team: 'neutral', pos: { x: 200, y: 210 }, vis_tier: 'full', label: '机枪' }],
              step_cache_queue: [],
              authoritative_sha256: 'abc',
            }),
            { status: 200 },
          );
        }
        if (url.endsWith('/api/command')) {
          const parsed = init?.body ? JSON.parse(String(init.body)) : {};
          postedBodies.push(parsed);
          return new Response(
            JSON.stringify({
              accepted: true,
              deferred_until_tick: 3,
              ticket_id: 42,
              remote_delay_ticks: 1,
              code: 'QUEUED',
              detail: 'ok',
            }),
            { status: 200 },
          );
        }
        if (url.includes('/api/units/2')) {
          return new Response(
            JSON.stringify({
              id: 2,
              callsign: '1班',
              team: 'red',
              pos: { x: 140, y: 200 },
              facing_deg: 0,
              alive: true,
              vis_tier: 'full',
              soldiers: [],
            }),
            { status: 200 },
          );
        }
        if (url.includes('/api/fields/i')) {
          return new Response(
            JSON.stringify({
              kind: 'i',
              observer_team: 'red',
              rows: 2,
              cols: 2,
              cell_m: 8,
              values: [0, 0, 0, 0],
              i_intel: [1, 1, 0, 0],
              i_vis_tier: ['full', 'full', 'hidden', 'hidden'],
              h_blast: [0, 0, 0, 0],
              i_threat: [0, 0.8, 0, 0],
            }),
            { status: 200 },
          );
        }
        return new Response('not found', { status: 404 });
      }),
    );

    const http = new HttpEngineAdapter({ baseUrl: '' });
    const h = await http.checkHealth();
    expect(h.ok).toBe(true);
    expect(h.cell_m).toBe(8);

    const init = await http.loadInitial('red');
    expect(init.terrain.widthM).toBe(64);
    expect(init.snapshot.groundObjects).toHaveLength(1);

    const receipt = await http.issueQueue({
      directives: [
        {
          directiveId: 'd-1',
          unitId: 2,
          atoms: [
            {
              directiveId: 'd-1',
              offsetSec: 2,
              command: { kind: 'move_unit', unit_id: 2, waypoints: [{ x: 200, y: 200 }], stance: 'stealth' },
              isStop: false,
              label: '移动测试',
            },
          ],
        },
      ],
    });
    expect((postedBodies[0] as { stance: string }).stance).toBe('cautious');
    expect(receipt.receipts[0]?.firstTick).toBe(3);
    expect(receipt.receipts[0]?.remote).toBe(true);

    const wave = await http.issueWave({
      title: 'T波次',
      geometry: { barrages: [{ type: 'circle', center: { x: 120, y: 120 }, radiusM: 30 }], assaultArrows: [], aos: [] },
    });
    expect(wave.effectiveTick).toBe(3);

    const detail = await http.getUnitDetail(2, 'red');
    expect(detail?.callsign).toBe('1班');
    const field = await http.getIField('red');
    expect(field?.i_threat[1]).toBe(0.8);
  });

  it('浮动窗口支持最小化/关闭/坞栏重启/拖拽分离持久化与重置，选项 ID 与 T12 开关全覆盖', async () => {
    const { container } = render(<App />);
    await waitFor(() => expect(screen.getByTestId('dock-toggle-W5')).toBeTruthy());
    fireEvent.click(screen.getByTestId('dock-toggle-W5'));
    fireEvent.click(screen.getByTestId('dock-toggle-W12'));
    await waitFor(() => expect(screen.getByTestId('wave-slip-wave-1')).toBeTruthy());

    // 选项级与坞栏 ID 完整存在
    expect(container.querySelector('[data-ui-id="W2.F4.opt_red"]')).not.toBeNull();
    expect(container.querySelector('[data-ui-id="W1.T4.opt_1x"]')).not.toBeNull();
    expect(container.querySelector('[data-ui-id="W1.T10.opt_fog"]')).not.toBeNull();
    expect(container.querySelector('[data-ui-id="W5A.U2.opt_task"]')).not.toBeNull();
    expect(screen.getByTestId('dock-toggle-W7')).toBeTruthy();

    // T12 一键隐藏全量 data-ui-tag 并剥除下拉 [optId] 前缀
    const modeSel = screen.getByTestId('select-mode') as HTMLSelectElement;
    expect(modeSel.options[0]?.textContent).toContain('[W5A.U2.opt_task]');
    fireEvent.click(screen.getByTestId('toggle-ui-ids'));
    expect(container.querySelectorAll('[data-ui-tag]').length).toBe(0);
    expect(modeSel.options[0]?.textContent).not.toContain('W5A.U2.opt_task');
    fireEvent.click(screen.getByTestId('toggle-ui-ids'));

    // 最小化 W2
    fireEvent.click(screen.getByTestId('win-min-W2'));
    expect(screen.getByTestId('freehand-toolbar').getAttribute('data-window-minimized')).toBe('true');

    // 拖拽 W2 标题栏分离成 floating-detached 并存入 localStorage
    const dragHeader = screen.getByTestId('win-drag-W2');
    fireEvent.pointerDown(dragHeader, { clientX: 20, clientY: 60, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 180, clientY: 140, pointerId: 1 });
    fireEvent.pointerUp(window, { pointerId: 1 });
    expect(screen.getByTestId('freehand-toolbar').className).toContain('floating-detached');
    expect(localStorage.getItem('gentactix_window_layout_v1')).toContain('"floating":true');

    // 关闭 W2 再从 Dock 栏重开，并一键重置全部窗口
    fireEvent.click(screen.getByTestId('win-close-W2'));
    expect(screen.queryByTestId('freehand-toolbar')).toBeNull();
    fireEvent.click(screen.getByTestId('dock-toggle-W2'));
    expect(screen.getByTestId('freehand-toolbar')).toBeTruthy();
    fireEvent.click(screen.getByTestId('dock-reset-windows'));
    expect(screen.getByTestId('freehand-toolbar').className).toContain('floating-docked');
  });

  it('W7 保姆开发面板可触发单兵4部位/坦克8部件透视，W8 模板可经 useGame 一键发令', async () => {
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('dock-toggle-W7')).toBeTruthy());

    // 打开 W7 面板
    fireEvent.click(screen.getByTestId('dock-toggle-W7'));
    expect(screen.getByTestId('dev-art-panel')).toBeTruthy();

    fireEvent.click(screen.getByTestId('btn-sample-soldier'));
    expect(screen.getByTestId('unit-detail-soldiers')).toBeTruthy();
    expect(screen.getByTestId('unit-detail-soldiers').textContent).toContain('躯干');

    fireEvent.click(screen.getByTestId('btn-sample-tank'));
    expect(screen.getByTestId('unit-detail-vehicle')).toBeTruthy();
    expect(screen.getByTestId('unit-detail-vehicle').textContent).toContain('履带');

    // 打开 W8 示例模板窗口，测试 useGame().sendQuickCommand
    fireEvent.click(screen.getByTestId('dock-toggle-W8'));
    const exampleWin = screen.getByTestId('example-custom-window');
    expect(exampleWin).toBeTruthy();
    const holdBtn = within(exampleWin).getByText(/hold_unit/);
    fireEvent.click(holdBtn);
    await waitFor(() => expect(screen.getByTestId('example-cmd-outcome')).toBeTruthy());
  });

  it('美术 Manifest、ThemeTokens 与 I场热图安全回退（零 PNG 且零网格线）', () => {
    applyThemeTokensToRoot();
    expect(document.documentElement.style.getPropertyValue('--gtx-red-team')).toBe(THEME_TOKENS.redTeam);
    expect(ART_SLOT_LIST.length).toBeGreaterThanOrEqual(16);
    expect(getArtImage('map.base')).toBeNull();

    const fillRectCalls: number[] = [];
    const strokeRectCalls: number[] = [];
    const noop = () => {};
    const mockCtx = {
      clearRect: noop,
      createLinearGradient: () => ({ addColorStop: noop }),
      createRadialGradient: () => ({ addColorStop: noop }),
      fillRect: () => fillRectCalls.push(1),
      strokeRect: () => strokeRectCalls.push(1),
      beginPath: noop,
      moveTo: noop,
      lineTo: noop,
      arc: noop,
      ellipse: noop,
      fill: noop,
      stroke: noop,
      fillText: noop,
      save: noop,
      restore: noop,
      translate: noop,
      rotate: noop,
      setLineDash: noop,
      fillStyle: '',
      strokeStyle: '',
      lineWidth: 1,
      lineCap: 'round',
      lineJoin: 'round',
      font: '',
      textAlign: 'left',
      globalAlpha: 1,
    } as unknown as CanvasRenderingContext2D;

    const tf = { widthPx: 640, heightPx: 480, worldWidthM: 640, worldHeightM: 480 };
    const binModel = terrainBinaryToModel(parseTerrainBinary(buildMockTerrainBinary(4, 4, 8)));
    renderLayer1BaseMap(mockCtx, binModel, tf);

    const units = presetUnits();
    renderLayer4Entities(
      mockCtx,
      { tick: 0, elapsedMs: 0, observer: 'red', units: [{ ...units[0]!, isVehicle: true, role: 'armored_vehicle' }], groundObjects: [], destruction: [], smoke: [], night: false },
      undefined,
      tf,
    );

    const iField: IFieldTriplet = {
      kind: 'i',
      observer_team: 'red',
      rows: 2,
      cols: 2,
      cell_m: 8,
      values: [0, 0, 0, 0],
      i_intel: [1, 1, 1, 1],
      i_vis_tier: ['full', 'full', 'full', 'full'],
      h_blast: [0, 0, 0, 0],
      i_threat: [0.9, 0.01, 0.5, 0],
    };
    renderLayer5Rings(mockCtx, { directRadiusM: 140, radioRadiusM: 600, iField, showThreatOverlay: true, tf });

    expect(fillRectCalls.length).toBeGreaterThan(2);
    expect(strokeRectCalls.length).toBe(1); // 只有装甲车单位牌本身 1 个边框，L1/L5 热图均 0 strokeRect
  });
});
