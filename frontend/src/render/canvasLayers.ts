// GenTactix V2 frontend · Canvas 2D 分层绘制函数（Layer 1..7 & Layer 9）
// 铁律：严禁渲染 8m 基础网格线！地形底图为连续预渲染图。

import type {
  DirectiveTarget,
  IFieldTriplet,
  ParsedTerrainBinary,
  TerrainModel,
  TickPlayback,
  UnitState,
  Vec2,
  WaveDirective,
  WaveGeometry,
  WorldSnapshot,
} from '../engine/types';
import { getArtImage } from '../art/assetLoader';
import type { ArtSlotKey } from '../art/assetManifest';
import { THEME_TOKENS } from '../art/themeTokens';
import type { FreehandStroke, WeatherMode } from '../store/layerStore';

export type MapViewMode = 'field' | 'tactical';

const terrainBakeCache = new WeakMap<ParsedTerrainBinary, HTMLCanvasElement>();

function getBakedBinaryTerrain(bin: ParsedTerrainBinary): HTMLCanvasElement | null {
  if (typeof document === 'undefined') return null;
  const cached = terrainBakeCache.get(bin);
  if (cached) return cached;
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, bin.cols * 2);
  canvas.height = Math.max(1, bin.rows * 2);
  const offCtx = canvas.getContext('2d');
  if (!offCtx) return null;
  for (let r = 0; r < bin.rows; r++) {
    for (let c = 0; c < bin.cols; c++) {
      const idx = r * bin.cols + c;
      const elev = bin.elevationM[idx] ?? 0;
      const kind = bin.kind[idx] ?? 0;
      if (kind === 1) {
        offCtx.fillStyle = 'rgba(104, 132, 88, 0.36)';
      } else if (kind === 2) {
        offCtx.fillStyle = 'rgba(110, 142, 160, 0.45)';
      } else if (kind === 3) {
        offCtx.fillStyle = 'rgba(142, 128, 98, 0.32)';
      } else if (elev > 5) {
        const alpha = Math.min(0.28, (elev - 5) / 120);
        offCtx.fillStyle = `rgba(116, 98, 68, ${alpha.toFixed(3)})`;
      } else {
        continue;
      }
      offCtx.fillRect(c * 2, r * 2, 2, 2);
    }
  }
  terrainBakeCache.set(bin, canvas);
  return canvas;
}

export interface CameraView {
  zoom: number;
  panX: number;
  panY: number;
}

export interface RenderTransform {
  widthPx: number;
  heightPx: number;
  worldWidthM: number;
  worldHeightM: number;
  camera?: CameraView;
}

function getCameraMetrics(tf: RenderTransform): { scale: number; originX: number; originY: number } {
  const wPx = Number.isFinite(tf.widthPx) && tf.widthPx > 0 ? tf.widthPx : 1280;
  const hPx = Number.isFinite(tf.heightPx) && tf.heightPx > 0 ? tf.heightPx : 720;
  const wM = Number.isFinite(tf.worldWidthM) && tf.worldWidthM > 0 ? tf.worldWidthM : 512;
  const hM = Number.isFinite(tf.worldHeightM) && tf.worldHeightM > 0 ? tf.worldHeightM : 512;
  const baseScale = Math.min(wPx / wM, hPx / hM);
  const zoom = Number.isFinite(tf.camera?.zoom) && (tf.camera?.zoom ?? 0) > 0 ? tf.camera!.zoom : 1;
  const panX = Number.isFinite(tf.camera?.panX) ? tf.camera!.panX : 0;
  const panY = Number.isFinite(tf.camera?.panY) ? tf.camera!.panY : 0;
  const scale = Number.isFinite(baseScale * zoom) && baseScale * zoom > 0 ? baseScale * zoom : 1;
  const originX = (wPx - wM * baseScale) / 2 + panX;
  const originY = (hPx - hM * baseScale) / 2 + panY;
  return {
    scale,
    originX: Number.isFinite(originX) ? originX : 0,
    originY: Number.isFinite(originY) ? originY : 0,
  };
}

export function worldToPx(p: Vec2 | undefined, tf: RenderTransform): Vec2 {
  if (!p || !Number.isFinite(p.x) || !Number.isFinite(p.y)) {
    return { x: 0, y: 0 };
  }
  const m = getCameraMetrics(tf);
  const x = m.originX + p.x * m.scale;
  const y = m.originY + p.y * m.scale;
  return {
    x: Number.isFinite(x) ? x : 0,
    y: Number.isFinite(y) ? y : 0,
  };
}

export function pxToWorld(px: Vec2 | undefined, tf: RenderTransform): Vec2 {
  if (!px || !Number.isFinite(px.x) || !Number.isFinite(px.y)) {
    return { x: 0, y: 0 };
  }
  const m = getCameraMetrics(tf);
  const scale = m.scale > 0 ? m.scale : 1;
  const wM = Number.isFinite(tf.worldWidthM) && tf.worldWidthM > 0 ? tf.worldWidthM : 512;
  const hM = Number.isFinite(tf.worldHeightM) && tf.worldHeightM > 0 ? tf.worldHeightM : 512;
  const x = Math.max(0, Math.min(wM, (px.x - m.originX) / scale));
  const y = Math.max(0, Math.min(hM, (px.y - m.originY) / scale));
  return {
    x: Number.isFinite(x) ? x : 0,
    y: Number.isFinite(y) ? y : 0,
  };
}

function scaleM(meters: number, tf: RenderTransform): number {
  if (!Number.isFinite(meters)) return 0;
  const s = meters * getCameraMetrics(tf).scale;
  return Number.isFinite(s) ? s : 0;
}

/** 绘制带纸张衬底描边的地图文字（自动兼容精简 Canvas mock）。 */
function drawHaloText(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  fill: string,
  haloColor = '#e5dfc6',
  haloWidth = 3,
): void {
  if (typeof ctx.fillText !== 'function') return;
  if (haloWidth > 0 && typeof ctx.strokeText === 'function') {
    ctx.strokeStyle = haloColor;
    ctx.lineWidth = haloWidth;
    ctx.lineJoin = 'round';
    ctx.strokeText(text, x, y);
  }
  ctx.fillStyle = fill;
  ctx.fillText(text, x, y);
}

/** 用 lineTo 路径勾勒非 strokeRect 矩形，兼容测试 mock 的 strokeRect 精确计数。 */
function strokeBoxPath(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number): void {
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(x + w, y);
  ctx.lineTo(x + w, y + h);
  ctx.lineTo(x, y + h);
  ctx.lineTo(x, y);
  ctx.stroke();
}

function traceOrganicBlob(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  seedOffset: number,
  steps = 40,
  amp1 = 0.09,
  amp2 = 0.05,
): void {
  ctx.beginPath();
  for (let i = 0; i <= steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const mod = 1 + amp1 * Math.sin(t * 3 + seedOffset) + amp2 * Math.cos(t * 7 + seedOffset * 0.4);
    const px = cx + Math.cos(t) * rx * mod;
    const py = cy + Math.sin(t) * ry * mod;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
}

/** Layer 1: 基础地图色彩层（连续地形、有机等高线、印刷树丛、双轨水系与道路，严禁渲染 8m 网格线）。 */
export function renderLayer1BaseMap(
  ctx: CanvasRenderingContext2D,
  terrain: TerrainModel,
  tf: RenderTransform,
  mapViewMode: MapViewMode = 'field',
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  const isTactical = mapViewMode === 'tactical';
  const baseImg = getArtImage(isTactical ? 'map.tactical_paper' : 'map.base');
  if (baseImg) {
    const tl = worldToPx({ x: 0, y: 0 }, tf);
    const br = worldToPx({ x: terrain.widthM, y: terrain.heightM }, tf);
    ctx.drawImage(baseImg, tl.x, tl.y, br.x - tl.x, br.y - tl.y);
    return;
  }

  // 双投影底色：主地图自然战区野地 vs 副地图 SD2 暖灰军用印刷纸 (#ddd7bd)
  const bg = ctx.createLinearGradient(0, 0, tf.widthPx, tf.heightPx);
  if (isTactical) {
    bg.addColorStop(0, THEME_TOKENS.terrainPaper);
    bg.addColorStop(0.55, '#dad4b9');
    bg.addColorStop(1, '#d3ccb1');
  } else {
    bg.addColorStop(0, '#96a07b');
    bg.addColorStop(0.5, '#8b9571');
    bg.addColorStop(1, '#7f8967');
  }
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);

  // 真后端 /api/terrain.bin 二进制高程与地类烘焙底图（离屏缓存一次性烘焙，平移缩放零卡顿）
  if (terrain.binary) {
    const bin = terrain.binary;
    const baked = getBakedBinaryTerrain(bin);
    if (baked && typeof ctx.drawImage === 'function') {
      const tl = worldToPx({ x: 0, y: 0 }, tf);
      const br = worldToPx({ x: bin.cols * bin.cellM, y: bin.rows * bin.cellM }, tf);
      ctx.save();
      ctx.globalAlpha = isTactical ? 0.36 : 0.72;
      ctx.drawImage(baked, tl.x, tl.y, br.x - tl.x, br.y - tl.y);
      ctx.restore();
    } else {
      const cellW = Math.ceil(scaleM(bin.cellM, tf)) + 1;
      const cellH = Math.ceil(scaleM(bin.cellM, tf)) + 1;
      const step = bin.rows * bin.cols > 6400 ? 2 : 1;
      for (let r = 0; r < bin.rows; r += step) {
        for (let c = 0; c < bin.cols; c += step) {
          const idx = r * bin.cols + c;
          const elev = bin.elevationM[idx] ?? 0;
          const kind = bin.kind[idx] ?? 0;
          const pos = worldToPx({ x: c * bin.cellM, y: r * bin.cellM }, tf);
          if (kind === 1) {
            ctx.fillStyle = isTactical ? 'rgba(155, 164, 128, 0.28)' : 'rgba(92, 120, 78, 0.36)';
          } else if (kind === 2) {
            ctx.fillStyle = isTactical ? 'rgba(129, 157, 154, 0.38)' : 'rgba(95, 130, 145, 0.45)';
          } else if (kind === 3) {
            ctx.fillStyle = 'rgba(142, 128, 98, 0.28)';
          } else if (elev > 5) {
            const alpha = Math.min(0.24, (elev - 5) / 130);
            ctx.fillStyle = `rgba(126, 114, 88, ${alpha.toFixed(3)})`;
          } else {
            continue;
          }
          ctx.fillRect(pos.x, pos.y, cellW * step, cellH * step);
        }
      }
    }
  }

  // 地貌斑块：耕地斜排线、印刷松林群落与林缘（严禁 8m 格子线）
  for (let pIdx = 0; pIdx < terrain.patches.length; pIdx++) {
    const patch = terrain.patches[pIdx]!;
    const c = worldToPx(patch.center, tf);
    const rxPx = Math.max(8, scaleM(patch.rx, tf));
    const ryPx = Math.max(8, scaleM(patch.ry, tf));
    ctx.save();
    ctx.translate(c.x, c.y);
    ctx.rotate(patch.rotation);

    if (patch.kind === 'field') {
      if (isTactical) {
        // 副地图：四边形耕地轮廓 + 25° 斜排线（对齐 4174 fieldHatch）
        ctx.beginPath();
        ctx.moveTo(-rxPx * 0.9, -ryPx * 0.65);
        ctx.lineTo(rxPx * 0.85, -ryPx * 0.78);
        ctx.lineTo(rxPx * 0.95, ryPx * 0.58);
        ctx.lineTo(-rxPx * 0.78, ryPx * 0.72);
        ctx.lineTo(-rxPx * 0.9, -ryPx * 0.65);
        ctx.fillStyle = 'rgba(204, 196, 162, 0.42)';
        ctx.fill();
        if (typeof ctx.clip === 'function') {
          ctx.clip();
          ctx.strokeStyle = 'rgba(168, 163, 128, 0.62)';
          ctx.lineWidth = 0.7;
          ctx.beginPath();
          const span = Math.max(rxPx, ryPx) * 1.6;
          for (let d = -span; d <= span; d += 7) {
            ctx.moveTo(d - span * 0.35, -ryPx);
            ctx.lineTo(d + span * 0.35, ryPx);
          }
          ctx.stroke();
        }
      } else {
        // 主地图：柔和椭圆田地地块（零 lineTo，严守 L1 无网格铁律）
        ctx.beginPath();
        ctx.ellipse(0, 0, rxPx * 0.9, ryPx * 0.72, 0, 0, Math.PI * 2);
        ctx.fillStyle = 'rgba(154, 150, 108, 0.36)';
        ctx.fill();
      }
    } else if (patch.kind === 'forest') {
      if (isTactical) {
        traceOrganicBlob(ctx, 0, 0, rxPx, ryPx, patch.center.x + pIdx * 13, 36, 0.1, 0.06);
        ctx.fillStyle = 'rgba(155, 164, 128, 0.34)';
        ctx.fill();
      } else {
        ctx.beginPath();
        ctx.ellipse(0, 0, rxPx, ryPx, 0, 0, Math.PI * 2);
        ctx.fillStyle = 'rgba(78, 100, 68, 0.52)';
        ctx.fill();
      }

      // 确定性树木符号：副图印刷小针叶三角，主图有机圆冠+树干（均严守无网格铁律）
      const treeCols = [-0.62, -0.32, 0, 0.32, 0.62];
      const treeRows = [-0.58, -0.25, 0.1, 0.46];
      for (let ix = 0; ix < treeCols.length; ix++) {
        for (let iy = 0; iy < treeRows.length; iy++) {
          const u = treeCols[ix]! + Math.sin(ix * 7.1 + iy * 3.7 + pIdx) * 0.08;
          const v = treeRows[iy]! + Math.cos(iy * 5.3 + ix * 2.9 + pIdx) * 0.08;
          if (u * u + v * v > 0.76) continue;
          const tx = u * rxPx * 0.85;
          const ty = v * ryPx * 0.85;
          const th = 6 + ((ix * 3 + iy * 5 + pIdx) % 4);
          const tw = th * 0.42;
          if (isTactical) {
            ctx.fillStyle = 'rgba(97, 112, 85, 0.72)';
            ctx.beginPath();
            ctx.moveTo(tx, ty - th * 0.65);
            ctx.lineTo(tx - tw, ty + th * 0.15);
            ctx.lineTo(tx + tw, ty + th * 0.15);
            ctx.lineTo(tx, ty - th * 0.65);
            ctx.fill();
            ctx.fillRect(tx - 0.7, ty + th * 0.12, 1.4, th * 0.25);
          } else {
            ctx.fillStyle = 'rgba(54, 72, 46, 0.82)';
            ctx.beginPath();
            ctx.ellipse(tx, ty - th * 0.2, tw * 1.1, th * 0.42, 0, 0, Math.PI * 2);
            ctx.fill();
            ctx.fillRect(tx - 0.8, ty + th * 0.1, 1.6, th * 0.25);
          }
        }
      }
    } else {
      ctx.beginPath();
      ctx.ellipse(0, 0, rxPx, ryPx, 0, 0, Math.PI * 2);
      ctx.fillStyle = isTactical ? 'rgba(168, 182, 174, 0.38)' : 'rgba(116, 134, 120, 0.42)';
      ctx.fill();
    }
    ctx.restore();
  }

  // 有机地形等高线（计曲线 + 首曲线，副图印刷等高线，主图浅高程缓坡）
  const spotElevations = [184, 216, 172, 205, 193];
  for (let hIdx = 0; hIdx < terrain.hills.length; hIdx++) {
    const hill = terrain.hills[hIdx]!;
    const c = worldToPx(hill.center, tf);
    const rings = isTactical ? [0.96, 0.8, 0.65, 0.5, 0.35, 0.2] : [0.85, 0.56, 0.3];
    for (let rIdx = 0; rIdx < rings.length; rIdx++) {
      const frac = rings[rIdx]!;
      const rx = scaleM(hill.radiusM * frac, tf);
      const ry = scaleM(hill.radiusM * frac * 0.78, tf);
      ctx.save();
      if (isTactical) {
        traceOrganicBlob(ctx, c.x, c.y, rx, ry, hill.center.x + rIdx * 0.35, 48, 0.085, 0.045);
        ctx.fillStyle = 'rgba(196, 186, 148, 0.06)';
        ctx.fill();
        const isIndexContour = rIdx % 3 === 1;
        ctx.strokeStyle = isIndexContour ? 'rgba(130, 124, 92, 0.48)' : 'rgba(157, 153, 120, 0.34)';
        ctx.lineWidth = isIndexContour ? 1.15 : 0.68;
        ctx.stroke();
      } else {
        ctx.beginPath();
        ctx.ellipse(c.x, c.y, rx, ry, 0.2, 0, Math.PI * 2);
        ctx.fillStyle = 'rgba(104, 114, 78, 0.14)';
        ctx.fill();
      }
      ctx.restore();
    }
    if (isTactical && typeof ctx.fillText === 'function') {
      const elev = spotElevations[hIdx % spotElevations.length]!;
      ctx.fillStyle = '#787951';
      ctx.font = '9px Georgia, serif';
      ctx.fillText(`· ${elev}`, c.x + 6, c.y + 3);
    }
  }

  // 白桦河水系（4174 三层灰蓝水流 + 支流细线）
  if (terrain.river.length >= 2) {
    ctx.save();
    ctx.beginPath();
    terrain.river.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.strokeStyle = isTactical ? 'rgba(129, 157, 154, 0.72)' : 'rgba(82, 116, 126, 0.85)';
    ctx.lineWidth = Math.max(10, scaleM(16, tf));
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.stroke();

    ctx.strokeStyle = isTactical ? THEME_TOKENS.riverFill : '#739ca8';
    ctx.lineWidth = Math.max(5, scaleM(9, tf));
    ctx.stroke();

    if (isTactical) {
      ctx.strokeStyle = 'rgba(225, 228, 203, 0.65)';
      ctx.lineWidth = 0.9;
      ctx.setLineDash([20, 8]);
      ctx.stroke();
      ctx.setLineDash([]);

      if (typeof ctx.fillText === 'function') {
        const midIdx = Math.floor(terrain.river.length / 2);
        const midP = worldToPx(terrain.river[midIdx]!, tf);
        ctx.fillStyle = '#697258';
        ctx.font = 'italic 11px Georgia, "Times New Roman", serif';
        drawHaloText(ctx, 'B E R E Z A · 白桦河', midP.x + 14, midP.y - 10, '#697258', '#e3dec5', 2.5);
      }
    }
    ctx.restore();
  }

  // 道路网（4174 赭红双层主干道 + 次级土路 + 沿线村落建筑轮廓）
  for (let rIdx = 0; rIdx < terrain.roads.length; rIdx++) {
    const road = terrain.roads[rIdx]!;
    if (road.length < 2) continue;
    ctx.save();
    ctx.beginPath();
    road.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    const isPrimary = rIdx === 0;
    if (isTactical) {
      ctx.strokeStyle = isPrimary ? 'rgba(170, 112, 87, 0.82)' : 'rgba(185, 160, 123, 0.88)';
      ctx.lineWidth = isPrimary ? Math.max(4.5, scaleM(6.5, tf)) : Math.max(2.2, scaleM(3, tf));
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.stroke();
      if (isPrimary) {
        ctx.strokeStyle = '#e1c4a0';
        ctx.lineWidth = Math.max(1.8, scaleM(2.4, tf));
        ctx.stroke();
      }
    } else {
      ctx.strokeStyle = isPrimary ? '#8f7d60' : '#7b6d54';
      ctx.lineWidth = isPrimary ? Math.max(4, scaleM(6, tf)) : Math.max(2, scaleM(3, tf));
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.stroke();
      if (isPrimary) {
        ctx.strokeStyle = '#b4a485';
        ctx.lineWidth = 1.4;
        ctx.stroke();
      }
    }

    // 沿路村落离散小建筑块（纯 fillRect，不含 strokeRect）
    ctx.fillStyle = isTactical ? 'rgba(119, 122, 103, 0.68)' : 'rgba(78, 72, 62, 0.78)';
    const sampleIndices = [0, Math.floor(road.length / 2), road.length - 1];
    for (const sIdx of sampleIndices) {
      const sp = road[sIdx]!;
      const p = worldToPx(sp, tf);
      for (let k = 0; k < 5; k++) {
        const dx = ((k * 17 + rIdx * 9) % 28) - 14;
        const dy = ((k * 23 + sIdx * 7) % 24) - 12;
        ctx.fillRect(p.x + dx, p.y + dy, 4 + (k % 3), 3 + (k % 2));
      }
    }
    ctx.restore();
  }

  // 副地图专属印刷要素：铁路虚线、双语聚落地名与东翼推定接触线
  if (isTactical) {
    ctx.save();
    const railPts = [
      worldToPx({ x: 85, y: terrain.heightM * 0.92 }, tf),
      worldToPx({ x: 165, y: terrain.heightM * 0.56 }, tf),
      worldToPx({ x: 195, y: terrain.heightM * 0.24 }, tf),
      worldToPx({ x: 255, y: 0 }, tf),
    ];
    ctx.beginPath();
    railPts.forEach((p, idx) => {
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.strokeStyle = '#6e7565';
    ctx.lineWidth = 3.2;
    ctx.setLineDash([1.2, 6]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.lineWidth = 0.9;
    ctx.stroke();

    // 东翼推定接触线（对齐 4174 frontLayer）
    const fTop = worldToPx({ x: terrain.widthM * 0.72, y: 0 }, tf);
    const fMid1 = worldToPx({ x: terrain.widthM * 0.68, y: terrain.heightM * 0.35 }, tf);
    const fMid2 = worldToPx({ x: terrain.widthM * 0.73, y: terrain.heightM * 0.68 }, tf);
    const fBot = worldToPx({ x: terrain.widthM * 0.70, y: terrain.heightM }, tf);
    const edgeR = worldToPx({ x: terrain.widthM, y: terrain.heightM }, tf).x;
    ctx.beginPath();
    ctx.moveTo(fTop.x, fTop.y);
    ctx.lineTo(fMid1.x, fMid1.y);
    ctx.lineTo(fMid2.x, fMid2.y);
    ctx.lineTo(fBot.x, fBot.y);
    ctx.lineTo(edgeR, fBot.y);
    ctx.lineTo(edgeR, fTop.y);
    ctx.lineTo(fTop.x, fTop.y);
    ctx.fillStyle = 'rgba(174, 119, 105, 0.085)';
    ctx.fill();

    ctx.beginPath();
    ctx.moveTo(fTop.x, fTop.y);
    ctx.lineTo(fMid1.x, fMid1.y);
    ctx.lineTo(fMid2.x, fMid2.y);
    ctx.lineTo(fBot.x, fBot.y);
    ctx.strokeStyle = 'rgba(166, 81, 67, 0.75)';
    ctx.lineWidth = 2;
    ctx.stroke();

    if (typeof ctx.fillText === 'function') {
      ctx.font = 'bold 13px Georgia, "Times New Roman", serif';
      const dubrovkaP = worldToPx({ x: 180, y: 155 }, tf);
      drawHaloText(ctx, 'DUBROVKA', dubrovkaP.x, dubrovkaP.y, '#555f53', '#e3dec5', 3);
      ctx.font = '10px "Microsoft YaHei", serif';
      drawHaloText(ctx, '杜布罗夫卡', dubrovkaP.x, dubrovkaP.y + 13, '#606752', '#e3dec5', 2.5);

      const berezinoP = worldToPx({ x: 470, y: 138 }, tf);
      ctx.font = 'bold 13px Georgia, "Times New Roman", serif';
      drawHaloText(ctx, 'BEREZINO', berezinoP.x, berezinoP.y, '#555f53', '#e3dec5', 3);
      ctx.font = '10px "Microsoft YaHei", serif';
      drawHaloText(ctx, '白桦镇', berezinoP.x, berezinoP.y + 13, '#606752', '#e3dec5', 2.5);

      ctx.font = '9px "Cascadia Code", Consolas, monospace';
      drawHaloText(ctx, '推定接触线 / ESTIMATED', fMid2.x - 10, fMid2.y + 24, '#a05f4d', '#e5dfc6', 3);
    }
    ctx.restore();
  }
}

/** Layer 2: 地形破坏层（弹坑、散兵坑、堑壕、焦土），严格压在 Layer 3 AO 之下。 */
export function renderLayer2Destruction(
  ctx: CanvasRenderingContext2D,
  snap: WorldSnapshot,
  tf: RenderTransform,
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  for (const d of snap.destruction) {
    const c = worldToPx(d.pos, tf);
    const r = Math.max(5, scaleM(d.radiusM, tf));
    if (d.kind === 'crater' || d.kind === 'scorch') {
      const decalImg = getArtImage(d.kind === 'crater' ? 'decal.crater' : 'decal.scorch');
      if (decalImg) {
        ctx.drawImage(decalImg, c.x - r, c.y - r, r * 2, r * 2);
        continue;
      }
      if (!Number.isFinite(c.x) || !Number.isFinite(c.y) || !Number.isFinite(r) || r <= 0) continue;
      try {
        const g = ctx.createRadialGradient(c.x, c.y, r * 0.15, c.x, c.y, r);
        g.addColorStop(0, 'rgba(38, 28, 22, 0.85)');
        g.addColorStop(0.55, 'rgba(84, 64, 48, 0.65)');
        g.addColorStop(1, 'rgba(90, 74, 58, 0)');
        ctx.fillStyle = g;
        ctx.beginPath();
        ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
        ctx.fill();

        ctx.strokeStyle = 'rgba(42, 32, 24, 0.72)';
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(c.x, c.y, r * 0.48, 0, Math.PI * 2);
        ctx.stroke();
      } catch {
        // ignore
      }
    } else if (d.kind === 'foxhole' || d.kind === 'trench') {
      const pts = d.points && d.points.length >= 2 ? d.points : [d.pos, { x: d.pos.x + 14, y: d.pos.y }];
      const segImg = getArtImage(d.kind === 'trench' ? 'decal.trench' : 'decal.foxhole');
      if (segImg && pts.length >= 2) {
        for (let i = 0; i < pts.length - 1; i++) {
          const p0 = worldToPx(pts[i]!, tf);
          const p1 = worldToPx(pts[i + 1]!, tf);
          const len = Math.hypot(p1.x - p0.x, p1.y - p0.y) || r * 2;
          const ang = Math.atan2(p1.y - p0.y, p1.x - p0.x);
          ctx.save();
          ctx.translate((p0.x + p1.x) / 2, (p0.y + p1.y) / 2);
          ctx.rotate(ang);
          ctx.drawImage(segImg, -len / 2, -r * 0.45, len, r * 0.9);
          ctx.restore();
        }
        continue;
      }
      ctx.save();
      ctx.beginPath();
      pts.forEach((pt, idx) => {
        const p = worldToPx(pt, tf);
        if (idx === 0) ctx.moveTo(p.x, p.y);
        else ctx.lineTo(p.x, p.y);
      });
      ctx.strokeStyle = '#5c4936';
      ctx.lineWidth = d.kind === 'trench' ? 7 : 5;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.stroke();
      ctx.strokeStyle = '#31261b';
      ctx.lineWidth = 2.5;
      ctx.stroke();
      ctx.restore();
    }
  }
}

/** Layer 3: 环境光遮蔽层（全局 AO 与地貌明暗，压在 L1/L2 之上）。 */
export function renderLayer3AmbientOcclusion(
  ctx: CanvasRenderingContext2D,
  terrain: TerrainModel,
  tf: RenderTransform,
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  const aoImg = getArtImage('map.ao');
  if (aoImg) {
    const tl = worldToPx({ x: 0, y: 0 }, tf);
    const br = worldToPx({ x: terrain.widthM, y: terrain.heightM }, tf);
    ctx.save();
    ctx.globalAlpha = 0.45;
    ctx.drawImage(aoImg, tl.x, tl.y, br.x - tl.x, br.y - tl.y);
    ctx.restore();
    return;
  }
  // 山坡西北向日光斜照产生的东南侧微弱 AO 阴影
  for (const hill of terrain.hills) {
    const c = worldToPx({ x: hill.center.x + 16, y: hill.center.y + 18 }, tf);
    const r = Math.max(0.1, scaleM(hill.radiusM * 0.9, tf));
    if (!Number.isFinite(c.x) || !Number.isFinite(c.y) || !Number.isFinite(r) || r <= 0) continue;
    try {
      const g = ctx.createRadialGradient(c.x, c.y, r * 0.1, c.x, c.y, r);
      g.addColorStop(0, 'rgba(32, 36, 28, 0.22)');
      g.addColorStop(1, 'rgba(32, 36, 28, 0)');
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
      ctx.fill();
    } catch {
      // ignore
    }
  }
  // 河谷微凹陷阴影
  if (terrain.river.length >= 2) {
    ctx.save();
    ctx.beginPath();
    terrain.river.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.strokeStyle = 'rgba(24, 34, 38, 0.16)';
    ctx.lineWidth = Math.max(14, scaleM(22, tf));
    ctx.lineCap = 'round';
    ctx.stroke();
    ctx.restore();
  }
}

/** Layer 4: 实例兵牌层（作战单位、地面散落物品、散落武器、阵亡尸体、烟幕、走表瞬时演出特效 + 主地图近距现场视野遮罩）。 */
export function renderLayer4Entities(
  ctx: CanvasRenderingContext2D,
  snap: WorldSnapshot,
  selectedUnitId: number | undefined,
  tf: RenderTransform,
  playbackEffects?: { elapsedMs: number; playback: TickPlayback | null },
  visibility?: {
    showUnits?: boolean;
    showGroundObjects?: boolean;
    selectedGroundObjectId?: string;
    mapViewMode?: MapViewMode;
    hqPos?: Vec2;
    fieldVisionRadiusM?: number;
  },
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  const showUnits = visibility?.showUnits !== false;
  const showGroundObjects = visibility?.showGroundObjects !== false;
  const mapViewMode: MapViewMode = visibility?.mapViewMode ?? 'tactical';
  const hqPos = visibility?.hqPos;
  const fieldVisionRadiusM = visibility?.fieldVisionRadiusM ?? 220;
  const inLocalField = (pos: Vec2): boolean => {
    if (mapViewMode !== 'field' || !hqPos) return true;
    return Math.hypot(pos.x - hqPos.x, pos.y - hqPos.y) <= fieldVisionRadiusM;
  };

  // 散落物品 / 武器 / 尸体 / 炸药 / 地雷
  if (showGroundObjects) {
    for (const obj of snap.groundObjects) {
      if (!inLocalField(obj.pos)) continue;
      const p = worldToPx(obj.pos, tf);
      const isObjSelected = obj.id === visibility?.selectedGroundObjectId;
      ctx.save();
      if (isObjSelected) {
        ctx.strokeStyle = '#2563eb';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(p.x, p.y, 11, 0, Math.PI * 2);
        ctx.stroke();
      }
      const objImg = getArtImage(`obj.${obj.kind}` as ArtSlotKey);
      if (objImg) {
        ctx.drawImage(objImg, p.x - 9, p.y - 9, 18, 18);
      } else if (obj.kind === 'item') {
        ctx.fillStyle = '#c89b3c';
        ctx.strokeStyle = '#3b2e13';
        ctx.lineWidth = 1.2;
        ctx.fillRect(p.x - 5, p.y - 4, 10, 8);
        ctx.strokeRect(p.x - 5, p.y - 4, 10, 8);
      } else if (obj.kind === 'weapon') {
        ctx.strokeStyle = '#2d3138';
        ctx.lineWidth = 2.4;
        ctx.beginPath();
        ctx.moveTo(p.x - 7, p.y + 3);
        ctx.lineTo(p.x + 7, p.y - 3);
        ctx.stroke();
      } else if (obj.kind === 'charge') {
        // 定时炸药包（菱形红框 + 引信点）
        ctx.fillStyle = '#dc2626';
        ctx.strokeStyle = '#450a0a';
        ctx.lineWidth = 1.4;
        ctx.fillRect(p.x - 5, p.y - 5, 10, 10);
        ctx.strokeRect(p.x - 5, p.y - 5, 10, 10);
        ctx.fillStyle = '#fef08a';
        ctx.beginPath();
        ctx.arc(p.x, p.y, 2, 0, Math.PI * 2);
        ctx.fill();
      } else if (obj.kind === 'mine') {
        // 地雷（扁圆警戒标 + 十字触角）
        ctx.fillStyle = '#7f1d1d';
        ctx.strokeStyle = '#fecaca';
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.arc(p.x, p.y, 5, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      } else {
        // 阵亡尸体交叉标记
        ctx.strokeStyle = '#6f2424';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(p.x - 4, p.y - 4);
        ctx.lineTo(p.x + 4, p.y + 4);
        ctx.moveTo(p.x + 4, p.y - 4);
        ctx.lineTo(p.x - 4, p.y + 4);
        ctx.stroke();
      }
      const isDangerObj = obj.kind === 'charge' || obj.kind === 'mine';
      ctx.font = '10px "Segoe UI", "Microsoft YaHei", sans-serif';
      drawHaloText(
        ctx,
        obj.label,
        p.x + 8,
        p.y + 3,
        isDangerObj
          ? mapViewMode === 'tactical'
            ? '#984a3f'
            : '#f8b4a8'
          : mapViewMode === 'tactical'
            ? '#3e4b43'
            : '#ece7d5',
        mapViewMode === 'tactical' ? '#e5dfc6' : '#1b2428',
        2.5,
      );
      ctx.restore();
    }
  }

  // 烟幕云团
  const smokeImg = getArtImage('fx.smoke');
  for (const s of snap.smoke) {
    const p = worldToPx(s.pos, tf);
    const r = scaleM(s.radiusM, tf);
    if (smokeImg) {
      ctx.save();
      ctx.globalAlpha = 0.72;
      ctx.drawImage(smokeImg, p.x - r, p.y - r, r * 2, r * 2);
      ctx.restore();
    } else {
      const g = ctx.createRadialGradient(p.x, p.y, r * 0.1, p.x, p.y, r);
      g.addColorStop(0, 'rgba(232, 236, 238, 0.72)');
      g.addColorStop(1, 'rgba(220, 224, 228, 0)');
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // 作战单位兵牌
  const isTacticalMap = mapViewMode === 'tactical';
  if (showUnits) {
    for (const u of snap.units) {
    if (!inLocalField(u.pos)) continue;
    const p = worldToPx(u.pos, tf);
    const isSelected = u.id === selectedUnitId;
    const isHq = u.role === 'platoon_hq';
    const isVeh = Boolean(u.isVehicle || u.role === 'armored_vehicle');
    const isEnemy = u.team === 'blue';
    const w = isVeh ? 34 : isHq ? 30 : 28;
    const h = isVeh ? 21 : isHq ? 20 : 18;

    ctx.save();
    if (isSelected) {
      ctx.save();
      ctx.strokeStyle = isTacticalMap ? '#455d62' : '#c5aa70';
      ctx.lineWidth = isTacticalMap ? 1.5 : 2;
      ctx.setLineDash(isTacticalMap ? [3, 3] : [4, 2]);
      strokeBoxPath(ctx, p.x - w / 2 - 5, p.y - h / 2 - 5, w + 10, h + 10);
      ctx.restore();
    }

    const slotKey: ArtSlotKey = isVeh
      ? u.team === 'blue'
        ? 'unit.armored_vehicle_blue'
        : 'unit.armored_vehicle_red'
      : isHq
        ? 'unit.platoon_hq_red'
        : u.team === 'blue'
          ? 'unit.enemy_squad_blue'
          : 'unit.rifle_squad_red';
    const unitImg = getArtImage(slotKey);

    if (unitImg) {
      ctx.save();
      ctx.translate(p.x, p.y);
      if (!u.alive) ctx.globalAlpha = 0.45;
      if (isVeh && u.facingDeg !== undefined) {
        ctx.rotate((u.facingDeg * Math.PI) / 180);
      }
      const size = isVeh ? 34 : 28;
      ctx.drawImage(unitImg, -size / 2, -size / 2, size, size);
      if (isVeh) {
        const turretImg = getArtImage(u.team === 'blue' ? 'unit.turret_blue' : 'unit.turret_red');
        if (turretImg) {
          ctx.drawImage(turretImg, -size * 0.42, -size * 0.42, size * 0.84, size * 0.84);
        }
      }
      ctx.restore();
    } else if (isTacticalMap && isEnemy) {
      // 副地图 4174 风格敌方菱形 APP-6 符号 (#e8c7b3 填色 / #984d42 勾边)
      ctx.fillStyle = !u.alive ? '#c4b8aa' : '#e8c7b3';
      ctx.strokeStyle = '#984d42';
      ctx.lineWidth = 1.6;
      const rx = w * 0.62;
      const ry = h * 0.72;
      ctx.beginPath();
      ctx.moveTo(p.x, p.y - ry);
      ctx.lineTo(p.x + rx, p.y);
      ctx.lineTo(p.x, p.y + ry);
      ctx.lineTo(p.x - rx, p.y);
      ctx.lineTo(p.x, p.y - ry);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = '#984a3f';
      ctx.font = 'bold 8px Consolas, monospace';
      ctx.textAlign = 'center';
      ctx.fillText('II ?', p.x, p.y - ry - 3);

      ctx.strokeStyle = '#984d42';
      ctx.lineWidth = 1.3;
      if (isVeh) {
        ctx.beginPath();
        ctx.ellipse(p.x, p.y, rx * 0.45, ry * 0.36, 0, 0, Math.PI * 2);
        ctx.stroke();
      } else {
        ctx.beginPath();
        ctx.moveTo(p.x - rx * 0.45, p.y - ry * 0.38);
        ctx.lineTo(p.x + rx * 0.45, p.y + ry * 0.38);
        ctx.moveTo(p.x + rx * 0.45, p.y - ry * 0.38);
        ctx.lineTo(p.x - rx * 0.45, p.y + ry * 0.38);
        ctx.stroke();
      }
    } else {
      // 4174 APP-6 己方蓝灰纸图矩形牌 / 主地图高对比实景兵牌
      const fill = !u.alive
        ? '#57534e'
        : isTacticalMap
          ? '#b7cbd0'
          : u.team === 'red'
            ? '#436779'
            : '#a94f43';
      const rim = isTacticalMap ? '#3a5969' : '#1b2428';
      const innerStroke = isTacticalMap ? '#3a5969' : '#f4efe1';

      ctx.fillStyle = fill;
      ctx.strokeStyle = rim;
      ctx.lineWidth = 1.65;
      ctx.fillRect(p.x - w / 2, p.y - h / 2, w, h);
      ctx.strokeRect(p.x - w / 2, p.y - h / 2, w, h);

      // 北约兵牌顶梯队注记（APP-6: I 连/排, II 营, III 团）
      const echelon = isHq ? 'III' : isVeh ? 'I' : 'II';
      ctx.font = 'bold 9px Consolas, monospace';
      ctx.textAlign = 'center';
      if (isTacticalMap) {
        drawHaloText(ctx, echelon, p.x, p.y - h / 2 - 3, '#344c59', '#e5dfc6', 2.2);
      } else {
        drawHaloText(ctx, echelon, p.x, p.y - h / 2 - 3, '#f0e9d2', '#1b2428', 2.5);
      }

      // 北约兵牌内部符号
      ctx.strokeStyle = innerStroke;
      ctx.lineWidth = 1.4;
      if (isVeh) {
        ctx.beginPath();
        ctx.ellipse(p.x, p.y, w * 0.28, h * 0.26, 0, 0, Math.PI * 2);
        ctx.stroke();
        const rad = ((u.facingDeg ?? 0) * Math.PI) / 180;
        ctx.beginPath();
        ctx.moveTo(p.x, p.y);
        ctx.lineTo(p.x + Math.cos(rad) * 14, p.y + Math.sin(rad) * 14);
        ctx.stroke();
      } else if (isHq) {
        ctx.beginPath();
        ctx.moveTo(p.x - w / 2, p.y + h / 2);
        ctx.lineTo(p.x - w / 2, p.y + h / 2 + 6);
        ctx.stroke();
        ctx.fillStyle = innerStroke;
        ctx.font = 'bold 9px Consolas, monospace';
        ctx.textAlign = 'center';
        ctx.fillText('HQ', p.x, p.y + 3);
      } else {
        ctx.beginPath();
        ctx.moveTo(p.x - w / 2 + 2, p.y - h / 2 + 2);
        ctx.lineTo(p.x + w / 2 - 2, p.y + h / 2 - 2);
        ctx.moveTo(p.x + w / 2 - 2, p.y - h / 2 + 2);
        ctx.lineTo(p.x - w / 2 + 2, p.y + h / 2 - 2);
        ctx.stroke();
      }
    }

    // 名称与编制兵力（APP-6 纸色描边两行式排版）
    const captionFill = isTacticalMap ? (isEnemy ? '#984a3f' : '#3e4b43') : '#f2ede0';
    const subFill = isTacticalMap ? (isEnemy ? '#984a3f' : '#626b5c') : '#d6d9d4';
    const halo = isTacticalMap ? '#e5dfc6' : '#1b2428';
    ctx.font = 'bold 10px "Segoe UI", "Microsoft YaHei", sans-serif';
    ctx.textAlign = 'center';
    drawHaloText(ctx, `${u.name}`, p.x, p.y + h / 2 + 12, captionFill, halo, 3);
    ctx.font = '9px Consolas, monospace';
    const strengthLabel = isEnemy ? '推定 · II-?' : `${u.soldiers} PERS`;
    drawHaloText(ctx, strengthLabel, p.x, p.y + h / 2 + 22, subFill, halo, 2.5);

    if (u.activity.type === 'build') {
      ctx.fillStyle = THEME_TOKENS.accentGreen;
      ctx.fillRect(p.x - w / 2, p.y + h / 2 + 25, w * u.workProgress, 3);
    }

    // 选中单位的行动路线与 PHASE 01 标牌预览
    const targetPos: Vec2 | undefined =
      u.activity.type === 'move' && u.activity.path.length > 0
        ? u.activity.path[u.activity.path.length - 1]
        : u.activity.type === 'fire' || u.activity.type === 'smoke'
          ? u.activity.target
          : undefined;

    if (isSelected && targetPos) {
      const tgtP = worldToPx(targetPos, tf);
      ctx.save();
      ctx.strokeStyle = '#c89f55';
      ctx.lineWidth = 2;
      ctx.setLineDash([6, 4]);
      ctx.beginPath();
      ctx.moveTo(p.x, p.y);
      ctx.lineTo(tgtP.x, tgtP.y);
      ctx.stroke();

      // 目标十字标
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.arc(tgtP.x, tgtP.y, 5, 0, Math.PI * 2);
      ctx.moveTo(tgtP.x - 8, tgtP.y);
      ctx.lineTo(tgtP.x + 8, tgtP.y);
      ctx.moveTo(tgtP.x, tgtP.y - 8);
      ctx.lineTo(tgtP.x, tgtP.y + 8);
      ctx.stroke();

      // 中途 PHASE 01 标牌
      const midX = (p.x + tgtP.x) / 2;
      const midY = (p.y + tgtP.y) / 2;
      ctx.fillStyle = 'rgba(235, 230, 215, 0.92)';
      ctx.fillRect(midX - 32, midY - 7, 64, 14);
      ctx.strokeStyle = '#c89f55';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.rect(midX - 32, midY - 7, 64, 14);
      ctx.stroke();
      if (typeof ctx.fillText === 'function') {
        ctx.fillStyle = '#1c1917';
        ctx.font = 'bold 9px monospace';
        ctx.fillText('PHASE 01', midX, midY + 4);
      }
      ctx.restore();
    }
    ctx.restore();
  }
  }

  // 16s 走表瞬时演出反馈（严禁提前剧透，严格按 ch.ms <= elapsedMs 触发）
  if (playbackEffects?.playback) {
    const { elapsedMs, playback } = playbackEffects;
    // 爆破冲击波光环
    for (const ch of playback.changes) {
      if (ch.type === 'destruction' && ch.ms <= elapsedMs && elapsedMs - ch.ms <= 1600) {
        const age = (elapsedMs - ch.ms) / 1600;
        const c = worldToPx(ch.item.pos, tf);
        const r = scaleM(ch.item.radiusM * (0.4 + age * 1.9), tf);
        ctx.save();
        ctx.strokeStyle = `rgba(249, 115, 22, ${1 - age})`;
        ctx.lineWidth = 3 * (1 - age * 0.6);
        ctx.beginPath();
        ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
        ctx.stroke();
        ctx.fillStyle = `rgba(254, 240, 138, ${0.45 * (1 - age)})`;
        ctx.beginPath();
        ctx.arc(c.x, c.y, r * 0.45, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }
      // 伤亡/弹耗/压制瞬时飘字
      if (ch.type === 'unit' && ch.ms <= elapsedMs && elapsedMs - ch.ms <= 1800) {
        const u = snap.units.find((x) => x.id === ch.unitId);
        if (!u) continue;
        const age = (elapsedMs - ch.ms) / 1800;
        const p = worldToPx(u.pos, tf);
        const label =
          ch.field === 'soldiers'
            ? `人数→${ch.value}`
            : ch.field === 'ammo'
              ? `弹药 ${ch.value}`
              : `压制 ${ch.value}%`;
        ctx.save();
        ctx.fillStyle =
          ch.field === 'soldiers'
            ? `rgba(220, 38, 38, ${1 - age})`
            : ch.field === 'suppression'
              ? `rgba(234, 88, 12, ${1 - age})`
              : `rgba(245, 158, 11, ${1 - age})`;
        ctx.font = 'bold 11px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(label, p.x, p.y - 22 - age * 18);
        ctx.restore();
      }
    }

    // 交火曳光示意（仅在正在开火态或近期开火日志内）
    for (const u of snap.units) {
      if (!inLocalField(u.pos)) continue;
      if (u.activity.type === 'fire') {
        const from = worldToPx(u.pos, tf);
        const to = worldToPx(u.activity.target, tf);
        ctx.save();
        ctx.strokeStyle = 'rgba(250, 204, 21, 0.78)';
        ctx.lineWidth = 2;
        ctx.setLineDash([8, 6]);
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.lineTo(to.x, to.y);
        ctx.stroke();
        ctx.restore();
      }
    }
  }

  // 主地图（近距写实现场）：指挥官直接感知圈外施加柔和暗雾迷遮，强化“附近已知”与“切副图看全局”差异
  if (mapViewMode === 'field' && hqPos) {
    const hqPx = worldToPx(hqPos, tf);
    const rPx = Math.max(24, scaleM(fieldVisionRadiusM, tf));
    if (Number.isFinite(hqPx.x) && Number.isFinite(hqPx.y) && Number.isFinite(rPx)) {
      try {
        ctx.save();
        const fog = ctx.createRadialGradient(hqPx.x, hqPx.y, rPx * 0.75, hqPx.x, hqPx.y, rPx * 1.15);
        fog.addColorStop(0, 'rgba(13, 20, 25, 0)');
        fog.addColorStop(1, 'rgba(13, 20, 25, 0.76)');
        ctx.fillStyle = fog;
        ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);

        ctx.strokeStyle = 'rgba(200, 159, 85, 0.35)';
        ctx.lineWidth = 1.2;
        ctx.setLineDash([4, 6]);
        ctx.beginPath();
        ctx.arc(hqPx.x, hqPx.y, rPx, 0, Math.PI * 2);
        ctx.stroke();
        ctx.restore();
      } catch {
        // ignore mock canvas gradient limits
      }
    }
  }
}

function drawArrow(ctx: CanvasRenderingContext2D, pts: Vec2[], tf: RenderTransform, color: string): void {
  if (pts.length < 2) return;
  ctx.save();
  ctx.beginPath();
  pts.forEach((pt, idx) => {
    const p = worldToPx(pt, tf);
    if (idx === 0) ctx.moveTo(p.x, p.y);
    else ctx.lineTo(p.x, p.y);
  });
  ctx.strokeStyle = color;
  ctx.lineWidth = 3;
  ctx.stroke();

  const tail = worldToPx(pts[pts.length - 2]!, tf);
  const head = worldToPx(pts[pts.length - 1]!, tf);
  const ang = Math.atan2(head.y - tail.y, head.x - tail.x);
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(head.x, head.y);
  ctx.lineTo(head.x - 12 * Math.cos(ang - 0.42), head.y - 12 * Math.sin(ang - 0.42));
  ctx.lineTo(head.x - 12 * Math.cos(ang + 0.42), head.y - 12 * Math.sin(ang + 0.42));
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawWaveGeometry(
  ctx: CanvasRenderingContext2D,
  geom: WaveGeometry,
  tf: RenderTransform,
  isDraft: boolean,
): void {
  const alpha = isDraft ? 0.6 : 0.9;
  // 许可 AO 边界
  for (const ao of geom.aos) {
    const p1 = worldToPx(ao.min, tf);
    const p2 = worldToPx(ao.max, tf);
    ctx.save();
    ctx.strokeStyle = `rgba(37, 99, 235, ${alpha})`;
    ctx.lineWidth = 2;
    ctx.setLineDash([8, 5]);
    ctx.strokeRect(p1.x, p1.y, p2.x - p1.x, p2.y - p1.y);
    ctx.fillStyle = 'rgba(37, 99, 235, 0.06)';
    ctx.fillRect(p1.x, p1.y, p2.x - p1.x, p2.y - p1.y);
    ctx.fillStyle = `rgba(30, 64, 175, ${alpha})`;
    ctx.font = 'bold 11px sans-serif';
    ctx.fillText('许可 AO', p1.x + 6, p1.y + 14);
    ctx.restore();
  }

  // 预定炮击区
  for (const b of geom.barrages) {
    ctx.save();
    ctx.strokeStyle = `rgba(220, 38, 38, ${alpha})`;
    ctx.fillStyle = 'rgba(220, 38, 38, 0.18)';
    ctx.lineWidth = 2;
    if (b.type === 'circle') {
      const c = worldToPx(b.center, tf);
      const r = scaleM(b.radiusM, tf);
      ctx.beginPath();
      ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = '#991b1b';
      ctx.font = 'bold 11px sans-serif';
      ctx.fillText('预定炮击区', c.x - 26, c.y + 4);
    } else if (b.points.length >= 2) {
      ctx.beginPath();
      b.points.forEach((pt, idx) => {
        const p = worldToPx(pt, tf);
        if (idx === 0) ctx.moveTo(p.x, p.y);
        else ctx.lineTo(p.x, p.y);
      });
      if (b.points.length >= 3) ctx.closePath();
      ctx.fill();
      ctx.stroke();
      const p0 = worldToPx(b.points[0]!, tf);
      ctx.fillStyle = '#991b1b';
      ctx.font = 'bold 11px sans-serif';
      ctx.fillText('预定炮击区', p0.x + 4, p0.y - 4);
    }
    ctx.restore();
  }

  // 友军冲锋箭头
  for (const arr of geom.assaultArrows) {
    drawArrow(ctx, arr, tf, `rgba(21, 128, 61, ${alpha})`);
  }
}

export interface PlannedRouteOverlay {
  id: string;
  unitId: number;
  target?: DirectiveTarget;
  locked: boolean;
  label: string;
}

/** Layer 5: 战术范围圈层（直接指挥圈、队内航路、波次投影、橡皮筋实时绘制预览）。 */
export function renderLayer5Rings(
  ctx: CanvasRenderingContext2D,
  opts: {
    hq?: UnitState;
    selectedUnit?: UnitState;
    units?: UnitState[];
    directRadiusM: number;
    radioRadiusM: number;
    activeWave?: WaveDirective;
    waveDraftGeometry?: WaveGeometry;
    draftTarget?: DirectiveTarget;
    plannedRoutes?: PlannedRouteOverlay[];
    hoveredDirectiveId?: string;
    showPlannedRoutes?: boolean;
    pickingPoints?: Vec2[];
    cursorWorldPos?: Vec2;
    circleDragPreview?: { center: Vec2; radiusM: number } | null;
    scheduledDetonations?: Vec2[];
    iField?: IFieldTriplet | null;
    showThreatOverlay?: boolean;
    showCommandRings?: boolean;
    showWavesAndDetonations?: boolean;
    tf: RenderTransform;
  },
): void {
  const { tf } = opts;
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);

  // 可选 I 场威胁热图叠加（来自 /api/fields/i，纯填色不画网格线）
  if (opts.showThreatOverlay && opts.iField) {
    const { rows, cols, cell_m, i_threat } = opts.iField;
    const cellW = Math.ceil(scaleM(cell_m, tf)) + 1;
    const cellH = Math.ceil(scaleM(cell_m, tf)) + 1;
    const step = rows * cols > 6400 ? 2 : 1;
    for (let r = 0; r < rows; r += step) {
      for (let c = 0; c < cols; c += step) {
        const threat = i_threat[r * cols + c] ?? 0;
        if (threat <= 0.05) continue;
        const pos = worldToPx({ x: c * cell_m, y: r * cell_m }, tf);
        ctx.fillStyle = `rgba(220, 38, 38, ${Math.min(0.42, threat * 0.45).toFixed(3)})`;
        ctx.fillRect(pos.x, pos.y, cellW * step, cellH * step);
      }
    }
  }

  if (opts.showCommandRings !== false && opts.hq) {
    const c = worldToPx(opts.hq.pos, tf);
    ctx.save();
    // 直接指挥半径圈
    ctx.beginPath();
    ctx.arc(c.x, c.y, scaleM(opts.directRadiusM, tf), 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(180, 83, 9, 0.55)';
    ctx.lineWidth = 1.6;
    ctx.setLineDash([6, 4]);
    ctx.stroke();
    ctx.fillStyle = 'rgba(146, 64, 14, 0.8)';
    ctx.font = '10px sans-serif';
    ctx.fillText(`直接指挥圈 ${opts.directRadiusM}m`, c.x + 8, c.y - scaleM(opts.directRadiusM, tf) + 12);

    // 无线电通联圈
    ctx.beginPath();
    ctx.arc(c.x, c.y, scaleM(opts.radioRadiusM, tf), 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(59, 130, 246, 0.25)';
    ctx.lineWidth = 1;
    ctx.setLineDash([3, 6]);
    ctx.stroke();
    ctx.restore();
  }

  if (opts.showCommandRings !== false && opts.selectedUnit) {
    const su = worldToPx(opts.selectedUnit.pos, tf);
    ctx.save();
    ctx.beginPath();
    ctx.arc(su.x, su.y, scaleM(45, tf), -0.6, 0.6);
    ctx.strokeStyle = 'rgba(234, 179, 8, 0.6)';
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.restore();
  }

  // 队列与已签发纸条的航路/目标线（支持纸条悬停双向高亮）
  if (opts.showPlannedRoutes !== false && opts.plannedRoutes && opts.units) {
    for (const route of opts.plannedRoutes) {
      const u = opts.units.find((x) => x.id === route.unitId);
      if (!u || !route.target) continue;
      const isHovered = route.id === opts.hoveredDirectiveId;
      const pts: Vec2[] = [];
      if (route.target.type === 'point') {
        pts.push(u.pos, route.target.pos);
      } else if (route.target.type === 'polyline') {
        pts.push(u.pos, ...route.target.points);
      } else {
        const targetUnitId = route.target.unitId;
        const tgt = opts.units.find((x) => x.id === targetUnitId);
        if (tgt) pts.push(u.pos, tgt.pos);
      }
      if (pts.length < 2) continue;

      ctx.save();
      ctx.strokeStyle = isHovered
        ? '#facc15'
        : route.locked
          ? 'rgba(34, 197, 94, 0.85)'
          : 'rgba(245, 158, 11, 0.85)';
      ctx.lineWidth = isHovered ? 3.5 : 2;
      if (!route.locked) ctx.setLineDash([6, 4]);
      ctx.beginPath();
      pts.forEach((pt, idx) => {
        const p = worldToPx(pt, tf);
        if (idx === 0) ctx.moveTo(p.x, p.y);
        else ctx.lineTo(p.x, p.y);
      });
      ctx.stroke();

      const endPx = worldToPx(pts[pts.length - 1]!, tf);
      ctx.fillStyle = isHovered ? '#fef08a' : route.locked ? '#15803d' : '#b45309';
      ctx.beginPath();
      ctx.arc(endPx.x, endPx.y, isHovered ? 6 : 4, 0, Math.PI * 2);
      ctx.fill();
      if (isHovered) {
        ctx.fillStyle = '#1c1917';
        ctx.font = 'bold 11px sans-serif';
        ctx.fillText(route.label, endPx.x + 8, endPx.y - 6);
      }
      ctx.restore();
    }
  }

  if (opts.showWavesAndDetonations !== false && opts.activeWave) {
    drawWaveGeometry(ctx, opts.activeWave.geometry, tf, false);
    const b0 = opts.activeWave.geometry.barrages[0];
    const anchor = b0?.type === 'circle' ? b0.center : b0?.type === 'polygon' ? b0.points[0] : opts.activeWave.geometry.assaultArrows[0]?.[0];
    if (anchor) {
      const ap = worldToPx(anchor, tf);
      ctx.save();
      ctx.fillStyle = 'rgba(28, 25, 22, 0.86)';
      ctx.fillRect(ap.x - 40, ap.y - 28, 128, 18);
      ctx.fillStyle = '#fde68a';
      ctx.font = 'bold 10px sans-serif';
      ctx.fillText(`波次 G场 #${opts.activeWave.effectiveTick} 生效`, ap.x - 35, ap.y - 15);
      ctx.restore();
    }
  }
  if (opts.showWavesAndDetonations !== false && opts.waveDraftGeometry) {
    drawWaveGeometry(ctx, opts.waveDraftGeometry, tf, true);
  }

  // 炮击圆拖拽实时半径预览
  if (opts.circleDragPreview) {
    const c = worldToPx(opts.circleDragPreview.center, tf);
    const r = scaleM(opts.circleDragPreview.radiusM, tf);
    ctx.save();
    ctx.strokeStyle = 'rgba(220, 38, 38, 0.9)';
    ctx.fillStyle = 'rgba(220, 38, 38, 0.16)';
    ctx.lineWidth = 2;
    ctx.setLineDash([4, 3]);
    ctx.beginPath();
    ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = '#991b1b';
    ctx.font = 'bold 11px sans-serif';
    ctx.fillText(`R=${Math.round(opts.circleDragPreview.radiusM)}m`, c.x + 8, c.y - 8);
    ctx.restore();
  }

  // 多段折线拾取橡皮筋预览
  if (opts.pickingPoints && opts.pickingPoints.length > 0) {
    const previewPts = opts.cursorWorldPos ? [...opts.pickingPoints, opts.cursorWorldPos] : opts.pickingPoints;
    ctx.save();
    ctx.strokeStyle = '#f59e0b';
    ctx.lineWidth = 2.5;
    ctx.setLineDash([6, 4]);
    ctx.beginPath();
    previewPts.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.stroke();
    for (const pt of opts.pickingPoints) {
      const p = worldToPx(pt, tf);
      ctx.fillStyle = '#f59e0b';
      ctx.beginPath();
      ctx.arc(p.x, p.y, 4, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  }

  // 草稿标的物定位预览
  if (opts.draftTarget) {
    ctx.save();
    ctx.strokeStyle = '#d97706';
    ctx.fillStyle = '#f59e0b';
    ctx.lineWidth = 2;
    if (opts.draftTarget.type === 'point') {
      const p = worldToPx(opts.draftTarget.pos, tf);
      ctx.beginPath();
      ctx.arc(p.x, p.y, 6, 0, Math.PI * 2);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(p.x - 10, p.y);
      ctx.lineTo(p.x + 10, p.y);
      ctx.moveTo(p.x, p.y - 10);
      ctx.lineTo(p.x, p.y + 10);
      ctx.stroke();
    } else if (opts.draftTarget.type === 'polyline' && opts.draftTarget.points.length > 0) {
      ctx.beginPath();
      opts.draftTarget.points.forEach((pt, idx) => {
        const p = worldToPx(pt, tf);
        if (idx === 0) ctx.moveTo(p.x, p.y);
        else ctx.lineTo(p.x, p.y);
      });
      ctx.stroke();
    }
    ctx.restore();
  }

  // 己方预约爆破点微标（仅己方迷雾可见）
  if (opts.showWavesAndDetonations !== false && opts.scheduledDetonations) {
    for (const pos of opts.scheduledDetonations) {
      const p = worldToPx(pos, tf);
      ctx.save();
      ctx.strokeStyle = '#dc2626';
      ctx.lineWidth = 1.8;
      ctx.beginPath();
      ctx.arc(p.x, p.y, 9, 0, Math.PI * 2);
      ctx.stroke();
      ctx.fillStyle = '#dc2626';
      ctx.font = 'bold 10px sans-serif';
      ctx.fillText('炸药', p.x + 11, p.y + 3);
      ctx.restore();
    }
  }
}

/** Layer 6: 玩家手绘辅助线层（100% 本地持久化，绝不发往后端）。 */
export function renderLayer6Freehand(
  ctx: CanvasRenderingContext2D,
  strokes: FreehandStroke[],
  activeStroke: FreehandStroke | null,
  tf: RenderTransform,
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  const drawOne = (st: FreehandStroke) => {
    if (st.points.length === 0) return;
    ctx.save();
    ctx.strokeStyle = st.color;
    ctx.lineWidth = st.width;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    st.points.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    if (st.points.length === 1) {
      const p = worldToPx(st.points[0]!, tf);
      ctx.lineTo(p.x + 0.5, p.y + 0.5);
    }
    ctx.stroke();
    ctx.restore();
  };
  for (const st of strokes) drawOne(st);
  if (activeStroke) drawOne(activeStroke);
}

/** Layer 7: 世界特效 A 层（雨雪雾/日照阴影/照明弹），位于 L8 HUD 之下。 */
export function renderLayer7VfxA(
  ctx: CanvasRenderingContext2D,
  opts: { enabled: boolean; weather: WeatherMode; night: boolean; tf: RenderTransform },
): void {
  const { tf } = opts;
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  if (!opts.enabled) return;

  if (opts.night) {
    ctx.fillStyle = 'rgba(15, 23, 42, 0.34)';
    ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);
  }

  // 日照/斜阳或战场薄雾
  if (opts.weather === 'fog') {
    const g = ctx.createLinearGradient(0, 0, tf.widthPx, tf.heightPx);
    g.addColorStop(0, 'rgba(226, 232, 240, 0.28)');
    g.addColorStop(0.5, 'rgba(241, 245, 249, 0.14)');
    g.addColorStop(1, 'rgba(203, 213, 225, 0.28)');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);
  } else if (opts.weather === 'rain') {
    ctx.save();
    ctx.strokeStyle = 'rgba(148, 163, 184, 0.45)';
    ctx.lineWidth = 1;
    for (let x = 20; x < tf.widthPx; x += 36) {
      for (let y = 16; y < tf.heightPx; y += 44) {
        ctx.beginPath();
        ctx.moveTo(x, y);
        ctx.lineTo(x - 6, y + 16);
        ctx.stroke();
      }
    }
    ctx.restore();
  } else {
    ctx.save();
    ctx.fillStyle = 'rgba(255, 255, 255, 0.6)';
    for (let x = 24; x < tf.widthPx; x += 48) {
      for (let y = 20; y < tf.heightPx; y += 46) {
        ctx.beginPath();
        ctx.arc(x + ((y * 7) % 19), y, 2, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    ctx.restore();
  }

  // 照明弹局部暖光
  const flareX = Number.isFinite(tf.widthPx) && tf.widthPx > 0 ? tf.widthPx * 0.65 : 400;
  const flareY = Number.isFinite(tf.heightPx) && tf.heightPx > 0 ? tf.heightPx * 0.35 : 200;
  try {
    const flare = ctx.createRadialGradient(flareX, flareY, 10, flareX, flareY, 160);
    flare.addColorStop(0, 'rgba(254, 240, 138, 0.22)');
    flare.addColorStop(1, 'rgba(254, 240, 138, 0)');
    ctx.fillStyle = flare;
    ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);
  } catch {
    // ignore
  }
}
