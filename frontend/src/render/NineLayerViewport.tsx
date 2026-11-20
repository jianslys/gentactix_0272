// GenTactix V2 frontend · 物理 9 层渲染堆栈容器（data-layer="1..9", z-index: 1..9）
// 严禁渲染 8m 网格线；L2 < L3；L7 < L8(HUD) < L9(Atmosphere)。

import React, { useEffect, useRef, useState } from 'react';
import { distance } from '../engine/directives';
import type {
  DirectiveTarget,
  TerrainModel,
  UnitState,
  Vec2,
  WaveDirective,
  WaveGeometry,
  WorldSnapshot,
} from '../engine/types';
import type { FreehandStroke, FreehandTool, WeatherMode } from '../store/layerStore';
import {
  pxToWorld,
  renderLayer1BaseMap,
  renderLayer2Destruction,
  renderLayer3AmbientOcclusion,
  renderLayer4Entities,
  renderLayer5Rings,
  renderLayer6Freehand,
  renderLayer7VfxA,
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
  onMapClick?: (worldPos: Vec2, hitUnit?: UnitState) => void;
  hudChildren?: React.ReactNode;
}

const DEFAULT_W = 800;
const DEFAULT_H = 600;

export const NineLayerViewport: React.FC<NineLayerViewportProps> = ({
  widthPx = DEFAULT_W,
  heightPx = DEFAULT_H,
  terrain,
  snapshot,
  hqUnitId,
  directRadiusM,
  radioRadiusM,
  selectedUnitId,
  activeWave,
  waveDraftGeometry,
  draftTarget,
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
  onMapClick,
  hudChildren,
}) => {
  const l1Ref = useRef<HTMLCanvasElement | null>(null);
  const l2Ref = useRef<HTMLCanvasElement | null>(null);
  const l3Ref = useRef<HTMLCanvasElement | null>(null);
  const l4Ref = useRef<HTMLCanvasElement | null>(null);
  const l5Ref = useRef<HTMLCanvasElement | null>(null);
  const l6Ref = useRef<HTMLCanvasElement | null>(null);
  const l7Ref = useRef<HTMLCanvasElement | null>(null);

  const [activeStroke, setActiveStroke] = useState<FreehandStroke | null>(null);

  const tf: RenderTransform = {
    widthPx,
    heightPx,
    worldWidthM: terrain.widthM,
    worldHeightM: terrain.heightM,
  };

  // Layer 1 & Layer 3（地形与全局 AO）
  useEffect(() => {
    const c1 = l1Ref.current?.getContext('2d');
    if (c1) renderLayer1BaseMap(c1, terrain, tf);
    const c3 = l3Ref.current?.getContext('2d');
    if (c3) renderLayer3AmbientOcclusion(c3, terrain, tf);
  }, [terrain, widthPx, heightPx]);

  // Layer 2（地形破坏，压在 L3 之下）
  useEffect(() => {
    const c2 = l2Ref.current?.getContext('2d');
    if (c2) renderLayer2Destruction(c2, snapshot, tf);
  }, [snapshot, terrain, widthPx, heightPx]);

  // Layer 4（实例兵牌、物品、武器、尸体）
  useEffect(() => {
    const c4 = l4Ref.current?.getContext('2d');
    if (c4) renderLayer4Entities(c4, snapshot, selectedUnitId, tf);
  }, [snapshot, selectedUnitId, terrain, widthPx, heightPx]);

  // Layer 5（范围圈 & 顶栏波次投影）
  useEffect(() => {
    const c5 = l5Ref.current?.getContext('2d');
    if (!c5) return;
    const hq = snapshot.units.find((u) => u.id === hqUnitId);
    const selectedUnit = snapshot.units.find((u) => u.id === selectedUnitId);
    renderLayer5Rings(c5, {
      hq,
      selectedUnit,
      directRadiusM,
      radioRadiusM,
      activeWave,
      waveDraftGeometry,
      draftTarget,
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
    scheduledDetonations,
    terrain,
    widthPx,
    heightPx,
  ]);

  // Layer 6（本地手绘）
  useEffect(() => {
    const c6 = l6Ref.current?.getContext('2d');
    if (c6) renderLayer6Freehand(c6, strokes, activeStroke, tf);
  }, [strokes, activeStroke, terrain, widthPx, heightPx]);

  // Layer 7（特效 A：天气 / 日照 / 照明弹）
  useEffect(() => {
    const c7 = l7Ref.current?.getContext('2d');
    if (c7) renderLayer7VfxA(c7, { enabled: l7Enabled, weather: l7Weather, night: snapshot.night, tf });
  }, [l7Enabled, l7Weather, snapshot.night, terrain, widthPx, heightPx]);

  const toWorldFromPointer = (ev: React.PointerEvent<HTMLElement>): Vec2 => {
    const rect = ev.currentTarget.getBoundingClientRect();
    const px = {
      x: ((ev.clientX - rect.left) / Math.max(1, rect.width)) * widthPx,
      y: ((ev.clientY - rect.top) / Math.max(1, rect.height)) * heightPx,
    };
    return pxToWorld(px, tf);
  };

  const eraseAt = (wPos: Vec2) => {
    const filtered = strokes.filter((s) => !s.points.some((p) => distance(p, wPos) <= 18));
    if (filtered.length !== strokes.length) onStrokesChange(filtered);
  };

  const handleL6PointerDown = (ev: React.PointerEvent<HTMLCanvasElement>) => {
    if (freehandTool === 'none') return;
    ev.stopPropagation();
    const pos = toWorldFromPointer(ev);
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
    const pos = toWorldFromPointer(ev);
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

  const handleMapClick = (ev: React.MouseEvent<HTMLDivElement>) => {
    if (freehandTool !== 'none' || !onMapClick) return;
    const rect = ev.currentTarget.getBoundingClientRect();
    const px = {
      x: ((ev.clientX - rect.left) / Math.max(1, rect.width)) * widthPx,
      y: ((ev.clientY - rect.top) / Math.max(1, rect.height)) * heightPx,
    };
    const worldPos = pxToWorld(px, tf);
    const hitUnit = snapshot.units.find((u) => distance(u.pos, worldPos) <= 20);
    onMapClick(worldPos, hitUnit);
  };

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
      {/* 沙盘拾取点击底层容器（仅在非 L6 手绘模式接收地图点击） */}
      <div
        data-testid="map-stage"
        onClick={handleMapClick}
        style={{
          position: 'relative',
          width: `${widthPx}px`,
          height: `${heightPx}px`,
          margin: '0 auto',
          cursor: freehandTool !== 'none' ? 'crosshair' : mapPickHint ? 'cell' : 'pointer',
          boxShadow: '0 8px 32px rgba(0,0,0,0.45)',
        }}
      >
        {/* Layer 1: 基础地图色彩层（连续地形，无 8m 网格线） */}
        <div data-layer="1" data-layer-name="base-map-color" style={layerStyle(1)}>
          <canvas ref={l1Ref} width={widthPx} height={heightPx} style={canvasClass} />
        </div>

        {/* Layer 2: 地形破坏层（弹坑/散兵坑/堑壕/焦土，严格在 L3 之下） */}
        <div data-layer="2" data-layer-name="terrain-destruction" style={layerStyle(2)}>
          <canvas ref={l2Ref} width={widthPx} height={heightPx} style={canvasClass} />
        </div>

        {/* Layer 3: 环境光遮蔽层 */}
        <div data-layer="3" data-layer-name="ambient-occlusion" style={layerStyle(3)}>
          <canvas ref={l3Ref} width={widthPx} height={heightPx} style={canvasClass} />
        </div>

        {/* Layer 4: 实例兵牌层 */}
        <div data-layer="4" data-layer-name="entity-tokens" style={layerStyle(4)}>
          <canvas ref={l4Ref} width={widthPx} height={heightPx} style={canvasClass} />
        </div>

        {/* Layer 5: 战术范围圈 & 波次投影层 */}
        <div data-layer="5" data-layer-name="range-area-rings" style={layerStyle(5)}>
          <canvas ref={l5Ref} width={widthPx} height={heightPx} style={canvasClass} />
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
            width={widthPx}
            height={heightPx}
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
          <canvas ref={l7Ref} width={widthPx} height={heightPx} style={canvasClass} />
        </div>
      </div>

      {/* Layer 8: 内置功能面板层（UIO、指挥 UI、走表、手绘条） */}
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
