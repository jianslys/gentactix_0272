// GenTactix V2 frontend · Canvas 2D 分层绘制函数（Layer 1..7 & Layer 9）
// 铁律：严禁渲染 8m 基础网格线！地形底图为连续预渲染图。

import type {
  DirectiveTarget,
  IFieldTriplet,
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
  const baseScale = Math.min(tf.widthPx / tf.worldWidthM, tf.heightPx / tf.worldHeightM);
  const zoom = tf.camera?.zoom ?? 1;
  const panX = tf.camera?.panX ?? 0;
  const panY = tf.camera?.panY ?? 0;
  const scale = baseScale * zoom;
  const originX = (tf.widthPx - tf.worldWidthM * baseScale) / 2 + panX;
  const originY = (tf.heightPx - tf.worldHeightM * baseScale) / 2 + panY;
  return { scale, originX, originY };
}

export function worldToPx(p: Vec2, tf: RenderTransform): Vec2 {
  const m = getCameraMetrics(tf);
  return {
    x: m.originX + p.x * m.scale,
    y: m.originY + p.y * m.scale,
  };
}

export function pxToWorld(px: Vec2, tf: RenderTransform): Vec2 {
  const m = getCameraMetrics(tf);
  return {
    x: Math.max(0, Math.min(tf.worldWidthM, (px.x - m.originX) / m.scale)),
    y: Math.max(0, Math.min(tf.worldHeightM, (px.y - m.originY) / m.scale)),
  };
}

function scaleM(meters: number, tf: RenderTransform): number {
  return meters * getCameraMetrics(tf).scale;
}

/** Layer 1: 基础地图色彩层（连续地形色彩、等高缓坡、水系、道路，绝无 8m 网格线）。 */
export function renderLayer1BaseMap(
  ctx: CanvasRenderingContext2D,
  terrain: TerrainModel,
  tf: RenderTransform,
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);
  const baseImg = getArtImage('map.base');
  if (baseImg) {
    const tl = worldToPx({ x: 0, y: 0 }, tf);
    const br = worldToPx({ x: terrain.widthM, y: terrain.heightM }, tf);
    ctx.drawImage(baseImg, tl.x, tl.y, br.x - tl.x, br.y - tl.y);
    return;
  }

  // 连续军用地图纸底色（同步 Theme Token）
  const bg = ctx.createLinearGradient(0, 0, tf.widthPx, tf.heightPx);
  bg.addColorStop(0, THEME_TOKENS.terrainPaper);
  bg.addColorStop(0.5, '#c7c1aa');
  bg.addColorStop(1, '#bdb79f');
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);

  // 真后端 /api/terrain.bin 二进制高程与地类微调底色（纯填色、严禁描边成网格线）
  if (terrain.binary) {
    const bin = terrain.binary;
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
          ctx.fillStyle = 'rgba(104, 132, 88, 0.36)';
        } else if (kind === 2) {
          ctx.fillStyle = 'rgba(72, 128, 168, 0.42)';
        } else if (kind === 3) {
          ctx.fillStyle = 'rgba(142, 128, 98, 0.32)';
        } else if (elev > 5) {
          const alpha = Math.min(0.28, (elev - 5) / 120);
          ctx.fillStyle = `rgba(116, 98, 68, ${alpha.toFixed(3)})`;
        } else {
          continue;
        }
        ctx.fillRect(pos.x, pos.y, cellW * step, cellH * step);
      }
    }
  }

  // 地貌斑块（林地/耕地/沼泽）
  for (const patch of terrain.patches) {
    const c = worldToPx(patch.center, tf);
    ctx.save();
    ctx.translate(c.x, c.y);
    ctx.rotate(patch.rotation);
    ctx.beginPath();
    ctx.ellipse(0, 0, scaleM(patch.rx, tf), scaleM(patch.ry, tf), 0, 0, Math.PI * 2);
    if (patch.kind === 'forest') ctx.fillStyle = 'rgba(116, 138, 96, 0.45)';
    else if (patch.kind === 'field') ctx.fillStyle = 'rgba(196, 184, 128, 0.42)';
    else ctx.fillStyle = 'rgba(112, 134, 128, 0.42)';
    ctx.fill();
    ctx.restore();
  }

  // 等高缓坡闭合线（纯拓扑地形等高线，非 8m 格子线）
  for (const hill of terrain.hills) {
    const c = worldToPx(hill.center, tf);
    for (const frac of [0.85, 0.58, 0.32]) {
      ctx.beginPath();
      ctx.ellipse(
        c.x,
        c.y,
        scaleM(hill.radiusM * frac, tf),
        scaleM(hill.radiusM * frac * 0.78, tf),
        0.2,
        0,
        Math.PI * 2,
      );
      ctx.fillStyle = `rgba(176, 158, 122, ${0.14})`;
      ctx.fill();
      ctx.strokeStyle = 'rgba(124, 102, 74, 0.35)';
      ctx.lineWidth = 1;
      ctx.stroke();
    }
  }

  // 水系河流
  if (terrain.river.length >= 2) {
    ctx.save();
    ctx.beginPath();
    terrain.river.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.strokeStyle = '#7da5b8';
    ctx.lineWidth = Math.max(8, scaleM(14, tf));
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.stroke();
    ctx.strokeStyle = '#9ec0cf';
    ctx.lineWidth = Math.max(3, scaleM(5, tf));
    ctx.stroke();
    ctx.restore();
  }

  // 道路网
  for (const road of terrain.roads) {
    if (road.length < 2) continue;
    ctx.save();
    ctx.beginPath();
    road.forEach((pt, idx) => {
      const p = worldToPx(pt, tf);
      if (idx === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.strokeStyle = '#968262';
    ctx.lineWidth = Math.max(3, scaleM(6, tf));
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.stroke();
    ctx.strokeStyle = '#dfd4ba';
    ctx.lineWidth = 1.5;
    ctx.setLineDash([6, 4]);
    ctx.stroke();
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
    } else if (d.kind === 'foxhole' || d.kind === 'trench') {
      const pts = d.points && d.points.length >= 2 ? d.points : [d.pos, { x: d.pos.x + 14, y: d.pos.y }];
      const segImg = getArtImage(d.kind === 'trench' ? 'decal.trench' : 'decal.foxhole');
      if (segImg && pts.length === 2) {
        const p0 = worldToPx(pts[0]!, tf);
        const p1 = worldToPx(pts[1]!, tf);
        const len = Math.hypot(p1.x - p0.x, p1.y - p0.y) || r * 2;
        const ang = Math.atan2(p1.y - p0.y, p1.x - p0.x);
        ctx.save();
        ctx.translate((p0.x + p1.x) / 2, (p0.y + p1.y) / 2);
        ctx.rotate(ang);
        ctx.drawImage(segImg, -len / 2, -r * 0.45, len, r * 0.9);
        ctx.restore();
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
    const r = scaleM(hill.radiusM * 0.9, tf);
    const g = ctx.createRadialGradient(c.x, c.y, r * 0.1, c.x, c.y, r);
    g.addColorStop(0, 'rgba(32, 36, 28, 0.22)');
    g.addColorStop(1, 'rgba(32, 36, 28, 0)');
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
    ctx.fill();
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

/** Layer 4: 实例兵牌层（作战单位、地面散落物品、散落武器、阵亡尸体、烟幕、走表瞬时演出特效）。 */
export function renderLayer4Entities(
  ctx: CanvasRenderingContext2D,
  snap: WorldSnapshot,
  selectedUnitId: number | undefined,
  tf: RenderTransform,
  playbackEffects?: { elapsedMs: number; playback: TickPlayback | null },
): void {
  ctx.clearRect(0, 0, tf.widthPx, tf.heightPx);

  // 散落物品 / 武器 / 尸体
  for (const obj of snap.groundObjects) {
    const p = worldToPx(obj.pos, tf);
    ctx.save();
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
    ctx.fillStyle = 'rgba(30, 28, 24, 0.78)';
    ctx.font = '10px sans-serif';
    ctx.fillText(obj.label, p.x + 7, p.y + 3);
    ctx.restore();
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
  for (const u of snap.units) {
    const p = worldToPx(u.pos, tf);
    const isSelected = u.id === selectedUnitId;
    const isHq = u.role === 'platoon_hq';
    const isVeh = Boolean(u.isVehicle || u.role === 'armored_vehicle');
    const w = isVeh ? 32 : isHq ? 28 : 24;
    const h = isVeh ? 20 : isHq ? 20 : 17;

    ctx.save();
    if (isSelected) {
      ctx.strokeStyle = '#facc15';
      ctx.lineWidth = 2.5;
      ctx.strokeRect(p.x - w / 2 - 4, p.y - h / 2 - 4, w + 8, h + 8);
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
    } else {
      ctx.fillStyle = !u.alive ? '#57534e' : u.team === 'red' ? THEME_TOKENS.redTeam : THEME_TOKENS.blueTeam;
      ctx.strokeStyle = '#1f1b16';
      ctx.lineWidth = 1.6;
      ctx.fillRect(p.x - w / 2, p.y - h / 2, w, h);
      ctx.strokeRect(p.x - w / 2, p.y - h / 2, w, h);

      // 北约兵牌内部符号
      ctx.strokeStyle = '#f8f4e8';
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
        // 排部三点 + 指挥星
        ctx.beginPath();
        ctx.arc(p.x, p.y, 3.5, 0, Math.PI * 2);
        ctx.stroke();
      } else {
        // 步兵交叉斜线
        ctx.beginPath();
        ctx.moveTo(p.x - w / 2 + 2, p.y - h / 2 + 2);
        ctx.lineTo(p.x + w / 2 - 2, p.y + h / 2 - 2);
        ctx.moveTo(p.x + w / 2 - 2, p.y - h / 2 + 2);
        ctx.lineTo(p.x - w / 2 + 2, p.y + h / 2 - 2);
        ctx.stroke();
      }
    }

    // 名称 + 实时人数/弹药
    ctx.fillStyle = '#181512';
    ctx.font = 'bold 11px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText(`${u.name} (${u.soldiers}/${u.maxSoldiers})`, p.x, p.y - h / 2 - 6);
    if (u.activity.type === 'build') {
      ctx.fillStyle = THEME_TOKENS.accentGreen;
      ctx.fillRect(p.x - w / 2, p.y + h / 2 + 3, w * u.workProgress, 3);
    }
    ctx.restore();
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

  if (opts.hq) {
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

  if (opts.selectedUnit) {
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

  if (opts.activeWave) {
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
  if (opts.waveDraftGeometry) {
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
  if (opts.scheduledDetonations) {
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
  const flare = ctx.createRadialGradient(tf.widthPx * 0.65, tf.heightPx * 0.35, 10, tf.widthPx * 0.65, tf.heightPx * 0.35, 160);
  flare.addColorStop(0, 'rgba(254, 240, 138, 0.22)');
  flare.addColorStop(1, 'rgba(254, 240, 138, 0)');
  ctx.fillStyle = flare;
  ctx.fillRect(0, 0, tf.widthPx, tf.heightPx);
}
