// GenTactix V2 frontend · 独立【指挥 UI】波次绘制与签发状态机

import type { BackendAdapter } from '../engine/backendAdapter';
import type { AoBox, BarrageShape, Vec2, WaveDirective, WaveGeometry } from '../engine/types';

export type WaveDrawTool = 'none' | 'barrage_circle' | 'barrage_polygon' | 'assault_arrow' | 'ao_box';

export interface CommanderWaveDraft {
  title: string;
  tool: WaveDrawTool;
  geometry: WaveGeometry;
  pendingPoints: Vec2[];
}

export function createEmptyWaveDraft(): CommanderWaveDraft {
  return {
    title: '排波次：火力突击',
    tool: 'none',
    geometry: {
      barrages: [],
      assaultArrows: [],
      aos: [],
    },
    pendingPoints: [],
  };
}

export function hasWaveGeometry(g: WaveGeometry): boolean {
  return g.barrages.length > 0 || g.assaultArrows.length > 0 || g.aos.length > 0;
}

export function addWaveMapPoint(draft: CommanderWaveDraft, pos: Vec2): CommanderWaveDraft {
  if (draft.tool === 'none') return draft;

  if (draft.tool === 'barrage_circle') {
    const circle: BarrageShape = { type: 'circle', center: { ...pos }, radiusM: 42 };
    return {
      ...draft,
      geometry: {
        ...draft.geometry,
        barrages: [...draft.geometry.barrages, circle],
      },
    };
  }

  if (draft.tool === 'ao_box') {
    if (draft.pendingPoints.length === 0) {
      return { ...draft, pendingPoints: [{ ...pos }] };
    }
    const p0 = draft.pendingPoints[0]!;
    const box: AoBox = {
      min: { x: Math.min(p0.x, pos.x), y: Math.min(p0.y, pos.y) },
      max: { x: Math.max(p0.x, pos.x), y: Math.max(p0.y, pos.y) },
    };
    return {
      ...draft,
      pendingPoints: [],
      geometry: {
        ...draft.geometry,
        aos: [...draft.geometry.aos, box],
      },
    };
  }

  // barrage_polygon 或 assault_arrow 收集折线/多边形点
  return {
    ...draft,
    pendingPoints: [...draft.pendingPoints, { ...pos }],
  };
}

export function commitPendingWaveShape(draft: CommanderWaveDraft): CommanderWaveDraft {
  if (draft.tool === 'barrage_polygon' && draft.pendingPoints.length >= 3) {
    return {
      ...draft,
      pendingPoints: [],
      geometry: {
        ...draft.geometry,
        barrages: [...draft.geometry.barrages, { type: 'polygon', points: draft.pendingPoints }],
      },
    };
  }
  if (draft.tool === 'assault_arrow' && draft.pendingPoints.length >= 2) {
    return {
      ...draft,
      pendingPoints: [],
      geometry: {
        ...draft.geometry,
        assaultArrows: [...draft.geometry.assaultArrows, draft.pendingPoints],
      },
    };
  }
  return draft;
}

/** 把待闭合点包含进实时预览几何体，供 Layer 5 即时绘制。 */
export function previewWaveGeometry(draft: CommanderWaveDraft): WaveGeometry | undefined {
  const committed = commitPendingWaveShape(draft);
  const g: WaveGeometry = {
    barrages: [...committed.geometry.barrages],
    assaultArrows: [...committed.geometry.assaultArrows],
    aos: [...committed.geometry.aos],
  };
  if (draft.tool === 'assault_arrow' && draft.pendingPoints.length >= 2) {
    // 已经通过 commitPendingWaveShape 预览
  } else if (draft.tool === 'barrage_polygon' && draft.pendingPoints.length >= 2 && draft.pendingPoints.length < 3) {
    g.barrages.push({ type: 'polygon', points: draft.pendingPoints });
  }
  return hasWaveGeometry(g) ? g : undefined;
}

export async function issueCommanderWave(
  draft: CommanderWaveDraft,
  adapter: BackendAdapter,
): Promise<{ wave: WaveDirective; nextDraft: CommanderWaveDraft }> {
  const ready = commitPendingWaveShape(draft);
  if (!hasWaveGeometry(ready.geometry)) {
    throw new Error('请先在地图上绘制至少一处炮击区、冲锋箭头或许可 AO。');
  }
  const wave = await adapter.issueWave({
    title: ready.title,
    geometry: ready.geometry,
  });
  return {
    wave,
    nextDraft: createEmptyWaveDraft(),
  };
}
