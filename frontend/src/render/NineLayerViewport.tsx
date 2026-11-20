// GenTactix V2 frontend · 全屏自适应 9 层渲染堆栈容器（data-layer="1..9", z-index: 1..9）
// 支持滚轮缩放、中键/空格平移、橡皮筋拉框/拖圆预览、光标实时测距（W6: M1..M4）；严禁渲染 8m 网格线。

import React, { useEffect, useRef, useState } from 'react';
import { distance } from '../engine/directives';
import type {
  DirectiveTarget,
  TerrainModel,
  TickPlayback,
  UnitState,
  Vec2,
  WaveDirective,
  WaveGeometry,
  WorldSnapshot,
} from '../engine/types';
import type { FreehandStroke, FreehandTool, WeatherMode } from '../store/layerStore';
import { UiTag } from '../ui/UiTag';
import {
  pxToWorld,
  renderLayer1BaseMap,
  renderLayer2Destruction,
  renderLayer3AmbientOcclusion,
  renderLayer4Entities,
  renderLayer5Rings,
  renderLayer6Freehand,
  renderLayer7VfxA,
  type CameraView,
  type PlannedRouteOverlay,
  type RenderTransform,
} from './canvasLayers';

export interface NineLayerViewportProps {
  widthPx?: number;
  heightPx?: number;
  terrain: TerrainModel;
  snapshot: WorldSnapshot;
  hqUnitId: number;
  directRadiusM: number;
  radioRadiusM: number;
  selectedUnitId?: number;
  activeWave?: WaveDirective;
  waveDraftGeometry?: WaveGeometry;
  draftTarget?: DirectiveTarget;
  plannedRoutes?: PlannedRouteOverlay[];
  hoveredDirectiveId?: string;
  showPlannedRoutes?: boolean;
  pickingPoints?: Vec2[];
  circleDragMode?: boolean;
  onCircleDraw?: (center: Vec2, radiusM: number) => void;
  onFinishPolyline?: () => void;
  onCursorWorldMove?: (pos: Vec2 | undefined) => void;
  activePlayback?: TickPlayback | null;
  elapsedMs?: number;
  scheduledDetonations?: Vec2[];
  strokes: FreehandStroke[];
  freehandTool: FreehandTool;
  freehandColor: string;
  onStrokesChange: (next: FreehandStroke[]) => void;
  l7Enabled: boolean;
  l7Weather: WeatherMode;
  l9Enabled: boolean;
  shaking?: boolean;
  mapPickHint?: string;
  rightInsetPx?: number;
  onMapClick?: (worldPos: Vec2, hitUnit?: UnitState) => void;
  hudChildren?: React.ReactNode;
}

const DEFAULT_W = 1280;
const DEFAULT_H = 720;

export const NineLayerViewport: React.FC<NineLayerViewportProps> = ({
  widthPx,
  heightPx,
  terrain,
  snapshot,
  hqUnitId,
  directRadiusM,
  radioRadiusM,
  selectedUnitId,
  activeWave,
  waveDraftGeometry,
  draftTarget,
  plannedRoutes,
  hoveredDirectiveId,
  showPlannedRoutes = true,
  pickingPoints,
  circleDragMode = false,
  onCircleDraw,
  onFinishPolyline,
  onCursorWorldMove,
  activePlayback = null,
  elapsedMs = 0,
  scheduledDetonations,
  strokes,
  freehandTool,
  freehandColor,
  onStrokesChange,
  l7Enabled,
  l7Weather,
  l9Enabled,
  shaking = false,
  mapPickHint,
  rightInsetPx = 456,
  onMapClick,
  hudChildren,
}) => {
  const outerRef = useRef<HTMLDivElement | null>(null);
  const [measured, setMeasured] = useState<{ w: number; h: number }>({
    w: widthPx ?? DEFAULT_W,
    h: heightPx ?? DEFAULT_H,
  });

  const stageW = widthPx ?? measured.w;
  const stageH = heightPx ?? measured.h;

  const [camera, setCamera] = useState<CameraView>({ zoom: 1, panX: 0, panY: 0 });
  const [cursorWorld, setCursorWorld] = useState<Vec2 | undefined>(undefined);
  const [circleDrag, setCircleDrag] = useState<{ center: Vec2; radiusM: number } | null>(null);
  const [activeStroke, setActiveStroke] = useState<FreehandStroke | null>(null);
  const panDragRef = useRef<{ startX: number; startY: number; origPanX: number; origPanY: number } | null>(null);
  const suppressClickRef = useRef<boolean>(false);

  const l1Ref = useRef<HTMLCanvasElement | null>(null);
  const l2Ref = useRef<HTMLCanvasElement | null>(null);
  const l3Ref = useRef<HTMLCanvasElement | null>(null);
  const l4Ref = useRef<HTMLCanvasElement | null>(null);
  const l5Ref = useRef<HTMLCanvasElement | null>(null);
  const l6Ref = useRef<HTMLCanvasElement | null>(null);
  const l7Ref = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    if (widthPx !== undefined && heightPx !== undefined) return;
    const el = outerRef.current;
    if (!el) return;
    const updateSize = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) {
        setMeasured({ w: el.clientWidth, h: el.clientHeight });
      }
    };
    updateSize();
    if (typeof ResizeObserver === 'undefined') return;
    const obs = new ResizeObserver(updateSize);
    obs.observe(el);
    return () => obs.disconnect();
  }, [widthPx, heightPx]);

  const tf: RenderTransform = {
    widthPx: stageW,
    heightPx: stageH,
    worldWidthM: terrain.widthM,
    worldHeightM: terrain.heightM,
    camera,
  };

  // Layer 1 & Layer 3（地形与全局 AO）
  useEffect(() => {
    const c1 = l1Ref.current?.getContext('2d');
    if (c1) renderLayer1BaseMap(c1, terrain, tf);
    const c3 = l3Ref.current?.getContext('2d');
    if (c3) renderLayer3AmbientOcclusion(c3, terrain, tf);
  }, [terrain, stageW, stageH, camera]);

  // Layer 2（地形破坏，压在 L3 之下）
  useEffect(() => {
    const c2 = l2Ref.current?.getContext('2d');
    if (c2) renderLayer2Destruction(c2, snapshot, tf);
  }, [snapshot, terrain, stageW, stageH, camera]);

  // Layer 4（实例兵牌、物品、武器、尸体、走表瞬时特效）
  useEffect(() => {
    const c4 = l4Ref.current?.getContext('2d');
    if (c4) renderLayer4Entities(c4, snapshot, selectedUnitId, tf, { elapsedMs, playback: activePlayback });
  }, [snapshot, selectedUnitId, activePlayback, elapsedMs, terrain, stageW, stageH, camera]);

  // Layer 5（范围圈、队列航路、波次投影、橡皮筋预览）
  useEffect(() => {
    const c5 = l5Ref.current?.getContext('2d');
    if (!c5) return;
    const hq = snapshot.units.find((u) => u.id === hqUnitId);
    const selectedUnit = snapshot.units.find((u) => u.id === selectedUnitId);
    renderLayer5Rings(c5, {
      hq,
      selectedUnit,
      units: snapshot.units,
      directRadiusM,
      radioRadiusM,
      activeWave,
      waveDraftGeometry,
      draftTarget,
      plannedRoutes,
      hoveredDirectiveId,
      showPlannedRoutes,
      pickingPoints,
      cursorWorldPos: cursorWorld,
      circleDragPreview: circleDrag,
      scheduledDetonations,
      tf,
    });
  }, [
    snapshot,
    hqUnitId,
    selectedUnitId,
    directRadiusM,
    radioRadiusM,
    activeWave,
    waveDraftGeometry,
    draftTarget,
    plannedRoutes,
    hoveredDirectiveId,
    showPlannedRoutes,
    pickingPoints,
    cursorWorld,
    circleDrag,
    scheduledDetonations,
    terrain,
    stageW,
    stageH,
    camera,
  ]);

  // Layer 6（本地手绘）
  useEffect(() => {
    const c6 = l6Ref.current?.getContext('2d');
    if (c6) renderLayer6Freehand(c6, strokes, activeStroke, tf);
  }, [strokes, activeStroke, terrain, stageW, stageH, camera]);

  // Layer 7（特效 A：天气 / 日照 / 照明弹）
  useEffect(() => {
    const c7 = l7Ref.current?.getContext('2d');
    if (c7) renderLayer7VfxA(c7, { enabled: l7Enabled, weather: l7Weather, night: snapshot.night, tf });
  }, [l7Enabled, l7Weather, snapshot.night, terrain, stageW, stageH, camera]);

  const toWorldFromClient = (clientX: number, clientY: number, el: HTMLElement): Vec2 => {
    const rect = el.getBoundingClientRect();
    const px = {
      x: ((clientX - rect.left) / Math.max(1, rect.width)) * stageW,
      y: ((clientY - rect.top) / Math.max(1, rect.height)) * stageH,
    };
    return pxToWorld(px, tf);
  };

  const zoomAt = (nextZoomRaw: number, anchorPx?: Vec2) => {
    const nextZoom = Math.max(0.6, Math.min(2.5, Math.round(nextZoomRaw * 100) / 100));
    setCamera((prev) => {
      if (!anchorPx) return { ...prev, zoom: nextZoom };
      const baseScale = Math.min(stageW / terrain.widthM, stageH / terrain.heightM);
      const baseOriginX = (stageW - terrain.widthM * baseScale) / 2;
      const baseOriginY = (stageH - terrain.heightM * baseScale) / 2;
      const worldX = (anchorPx.x - (baseOriginX + prev.panX)) / (baseScale * prev.zoom);
      const worldY = (anchorPx.y - (baseOriginY + prev.panY)) / (baseScale * prev.zoom);
      return {
        zoom: nextZoom,
        panX: anchorPx.x - baseOriginX - worldX * baseScale * nextZoom,
        panY: anchorPx.y - baseOriginY - worldY * baseScale * nextZoom,
      };
    });
  };

  const eraseAt = (wPos: Vec2) => {
    const filtered = strokes.filter((s) => !s.points.some((p) => distance(p, wPos) <= 18));
    if (filtered.length !== strokes.length) onStrokesChange(filtered);
  };

  const handleL6PointerDown = (ev: React.PointerEvent<HTMLCanvasElement>) => {
    if (freehandTool === 'none') return;
    ev.stopPropagation();
    const pos = toWorldFromClient(ev.clientX, ev.clientY, ev.currentTarget);
    if (freehandTool === 'eraser') {
      eraseAt(pos);
      return;
    }
    const id = `stroke-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    setActiveStroke({ id, color: freehandColor, width: 3, points: [pos] });
  };

  const handleL6PointerMove = (ev: React.PointerEvent<HTMLCanvasElement>) => {
    if (freehandTool === 'none') return;
    ev.stopPropagation();
    const pos = toWorldFromClient(ev.clientX, ev.clientY, ev.currentTarget);
    if (freehandTool === 'eraser' && ev.buttons > 0) {
      eraseAt(pos);
      return;
    }
    if (activeStroke) {
      setActiveStroke({ ...activeStroke, points: [...activeStroke.points, pos] });
    }
  };

  const handleL6PointerUp = (ev: React.PointerEvent<HTMLCanvasElement>) => {
    if (!activeStroke) return;
    ev.stopPropagation();
    const committed = [...strokes, activeStroke];
    setActiveStroke(null);
    onStrokesChange(committed);
  };

  const handleStagePointerDown = (ev: React.PointerEvent<HTMLDivElement>) => {
    if (ev.button === 1 || (ev.button === 0 && ev.altKey)) {
      ev.preventDefault();
      panDragRef.current = {
        startX: ev.clientX,
        startY: ev.clientY,
        origPanX: camera.panX,
        origPanY: camera.panY,
      };
      return;
    }
    if (freehandTool === 'none' && circleDragMode && ev.button === 0) {
      const center = toWorldFromClient(ev.clientX, ev.clientY, ev.currentTarget);
      setCircleDrag({ center, radiusM: 42 });
    }
  };

  const handleStagePointerMove = (ev: React.PointerEvent<HTMLDivElement>) => {
    if (panDragRef.current) {
      const dx = ev.clientX - panDragRef.current.startX;
      const dy = ev.clientY - panDragRef.current.startY;
      setCamera((prev) => ({
        ...prev,
        panX: panDragRef.current!.origPanX + dx,
        panY: panDragRef.current!.origPanY + dy,
      }));
      suppressClickRef.current = true;
      return;
    }
    const wPos = toWorldFromClient(ev.clientX, ev.clientY, ev.currentTarget);
    setCursorWorld(wPos);
    onCursorWorldMove?.(wPos);

    if (circleDrag) {
      const r = distance(circleDrag.center, wPos);
      setCircleDrag({ center: circleDrag.center, radiusM: r >= 8 ? Math.min(140, r) : 42 });
    }
  };

  const handleStagePointerUp = () => {
    if (panDragRef.current) {
      panDragRef.current = null;
      return;
    }
    if (circleDrag) {
      const done = circleDrag;
      setCircleDrag(null);
      suppressClickRef.current = true;
      onCircleDraw?.(done.center, done.radiusM);
    }
  };

  const handleMapClick = (ev: React.MouseEvent<HTMLDivElement>) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    if (freehandTool !== 'none' || !onMapClick) return;
    const worldPos = toWorldFromClient(ev.clientX, ev.clientY, ev.currentTarget);
    const hitUnit = snapshot.units.find((u) => distance(u.pos, worldPos) <= 20);
    onMapClick(worldPos, hitUnit);
  };

  const handleWheel = (ev: React.WheelEvent<HTMLDivElement>) => {
    const rect = ev.currentTarget.getBoundingClientRect();
    const anchor = {
      x: ((ev.clientX - rect.left) / Math.max(1, rect.width)) * stageW,
      y: ((ev.clientY - rect.top) / Math.max(1, rect.height)) * stageH,
    };
    const delta = ev.deltaY < 0 ? 0.15 : -0.15;
    zoomAt(camera.zoom + delta, anchor);
  };

  const hq = snapshot.units.find((u) => u.id === hqUnitId);
  const cursorDistToHq = hq && cursorWorld ? Math.round(distance(hq.pos, cursorWorld)) : undefined;

  const layerStyle = (z: number, pointerEvents: 'auto' | 'none' = 'none'): React.CSSProperties => ({
    position: 'absolute',
    inset: 0,
    zIndex: z,
    pointerEvents,
  });

  const canvasClass: React.CSSProperties = {
    width: '100%',
    height: '100%',
    display: 'block',
  };

  return (
    <div
      ref={outerRef}
      data-testid="nine-layer-viewport"
      className={shaking && l9Enabled ? 'viewport-shake' : undefined}
      style={{
        position: 'relative',
        width: '100%',
        height: '100%',
        minHeight: '540px',
        overflow: 'hidden',
        background: '#1c1917',
        userSelect: 'none',
      }}
    >
      {/* 全屏自适应沙盘容器（仅在非 L6 手绘模式接收地图点击、滚轮缩放、中键/Alt 平移） */}
      <div
        data-testid="map-stage"
        onClick={handleMapClick}
        onDoubleClick={() => onFinishPolyline?.()}
        onContextMenu={(e) => {
          if (pickingPoints && pickingPoints.length > 0) {
            e.preventDefault();
            onFinishPolyline?.();
          }
        }}
        onWheel={handleWheel}
        onPointerDown={handleStagePointerDown}
        onPointerMove={handleStagePointerMove}
        onPointerUp={handleStagePointerUp}
        onPointerLeave={() => {
          setCursorWorld(undefined);
          onCursorWorldMove?.(undefined);
        }}
        style={{
          position: 'relative',
          width: widthPx ? `${widthPx}px` : '100%',
          height: heightPx ? `${heightPx}px` : '100%',
          margin: '0 auto',
          cursor: freehandTool !== 'none' ? 'crosshair' : mapPickHint ? 'cell' : 'grab',
        }}
      >
        {/* Layer 1: 基础地图色彩层（连续地形，无 8m 网格线） */}
        <div data-layer="1" data-layer-name="base-map-color" style={layerStyle(1)}>
          <canvas ref={l1Ref} width={stageW} height={stageH} style={canvasClass} />
        </div>

        {/* Layer 2: 地形破坏层（弹坑/散兵坑/堑壕/焦土，严格在 L3 之下） */}
        <div data-layer="2" data-layer-name="terrain-destruction" style={layerStyle(2)}>
          <canvas ref={l2Ref} width={stageW} height={stageH} style={canvasClass} />
        </div>

        {/* Layer 3: 环境光遮蔽层 */}
        <div data-layer="3" data-layer-name="ambient-occlusion" style={layerStyle(3)}>
          <canvas ref={l3Ref} width={stageW} height={stageH} style={canvasClass} />
        </div>

        {/* Layer 4: 实例兵牌层 */}
        <div data-layer="4" data-layer-name="entity-tokens" style={layerStyle(4)}>
          <canvas ref={l4Ref} width={stageW} height={stageH} style={canvasClass} />
        </div>

        {/* Layer 5: 战术范围圈 & 航路 & 波次投影层 */}
        <div data-layer="5" data-layer-name="range-area-rings" style={layerStyle(5)}>
          <canvas ref={l5Ref} width={stageW} height={stageH} style={canvasClass} />
        </div>

        {/* Layer 6: 地图手绘辅助线层（100% 本地持久化） */}
        <div
          data-layer="6"
          data-layer-name="freehand-overlay"
          style={layerStyle(6, freehandTool !== 'none' ? 'auto' : 'none')}
        >
          <canvas
            ref={l6Ref}
            data-testid="layer6-canvas"
            width={stageW}
            height={stageH}
            style={canvasClass}
            onPointerDown={handleL6PointerDown}
            onPointerMove={handleL6PointerMove}
            onPointerUp={handleL6PointerUp}
          />
        </div>

        {/* Layer 7: 特效 A 层（天气/日照/灯光，位于 HUD 之下，支持独立开关） */}
        <div
          data-layer="7"
          data-layer-name="world-vfx-a"
          data-enabled={String(l7Enabled)}
          style={{ ...layerStyle(7), opacity: l7Enabled ? 1 : 0 }}
        >
          <canvas ref={l7Ref} width={stageW} height={stageH} style={canvasClass} />
        </div>
      </div>

      {/* Layer 8: 内置功能面板层（UIO、指挥 UI、走表、手绘条、W6 底部图台遥测条） */}
      <div
        data-layer="8"
        data-layer-name="functional-hud"
        style={{
          position: 'absolute',
          inset: 0,
          zIndex: 8,
          pointerEvents: 'none',
        }}
      >
        {hudChildren}

        <div
          className="map-hud-bottom-wrap"
          style={{ left: '260px', right: `${rightInsetPx}px` }}
        >
          <div className="panel map-hud-bottom" data-testid="map-hud-bar" data-ui-id="W6">
            <UiTag id="W6" kind="win" />
          <div className="row" data-ui-id="M1">
            <UiTag id="M1" />
            <button
              type="button"
              data-testid="zoom-out"
              className="btn"
              onClick={() => zoomAt(camera.zoom - 0.2)}
            >
              -
            </button>
            <span className="meta-note" data-testid="zoom-level">
              {Math.round(camera.zoom * 100)}%
            </span>
            <button
              type="button"
              data-testid="zoom-in"
              className="btn"
              onClick={() => zoomAt(camera.zoom + 0.2)}
            >
              +
            </button>
            <button
              type="button"
              data-testid="zoom-reset"
              className="btn"
              onClick={() => setCamera({ zoom: 1, panX: 0, panY: 0 })}
            >
              复位视野
            </button>
          </div>

          <span className="meta-note" data-testid="cursor-telemetry" data-ui-id="M2">
            <UiTag id="M2" />
            {cursorWorld
              ? `(${Math.round(cursorWorld.x)}, ${Math.round(cursorWorld.y)})m · 距排部 ${cursorDistToHq ?? '-'}m (${cursorDistToHq !== undefined && cursorDistToHq <= directRadiusM ? '直指圈内' : '远程延迟区'})`
              : '移动鼠标查看坐标与直指圈测距'}
          </span>

          {mapPickHint && (
            <span className="remote-banner" data-ui-id="M3">
              <UiTag id="M3" />
              {mapPickHint}
            </span>
          )}

          <span className="meta-note" data-ui-id="M4">
            <UiTag id="M4" />
            Space 播停 · 滚轮缩放 · 中键/Alt拖图 · Esc 取消
          </span>
          </div>
        </div>
      </div>

      {/* Layer 9: 特效 B 层（气氛组：暗角/胶片颗粒/震屏，位于最顶层，支持独立开关） */}
      <div
        data-layer="9"
        data-layer-name="atmosphere-vfx-b"
        data-enabled={String(l9Enabled)}
        style={{
          position: 'absolute',
          inset: 0,
          zIndex: 9,
          pointerEvents: 'none',
          display: l9Enabled ? 'block' : 'none',
          background:
            'radial-gradient(ellipse at center, rgba(0,0,0,0) 58%, rgba(10, 9, 8, 0.45) 100%)',
          boxShadow: 'inset 0 0 64px rgba(0, 0, 0, 0.35)',
        }}
      />
    </div>
  );
};
