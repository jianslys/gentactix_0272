// GenTactix V2 frontend · 本地模拟沙盒预置测试世界（100% 对齐 SPEC 6.1）

import type { GroundObject, TerrainModel, UnitState, WaveDirective } from './types';

export const WORLD_WIDTH_M = 640;
export const WORLD_HEIGHT_M = 480;
export const DIRECT_COMMAND_RADIUS_M = 140;
export const RADIO_RADIUS_M = 600;
export const VISION_RADIUS_M = 200;

export const HQ_UNIT_ID = 1;
export const SQUAD1_ID = 2;
export const SQUAD2_ID = 3;
export const SQUAD3_ID = 4;
export const ENEMY_SQUAD_ID = 9;

/** #0 预埋：#4 回合起爆的己方定时炸药。 */
export const PRESET_CHARGE = {
  eventId: 'preset-charge-red-4',
  targetTick: 4,
  exactMs: 7300,
  pos: { x: 330, y: 330 },
  radiusM: 24,
} as const;

/** #0 预埋：敌方定时炸药（用于验证前向预警迷雾过滤，绝不向红方泄露）。 */
export const PRESET_ENEMY_CHARGE = {
  eventId: 'preset-charge-blue-4',
  targetTick: 4,
  exactMs: 5200,
  pos: { x: 560, y: 150 },
  radiusM: 20,
} as const;

function unit(
  id: number,
  name: string,
  team: UnitState['team'],
  role: UnitState['role'],
  x: number,
  y: number,
  soldiers: number,
  ammo: number,
): UnitState {
  return {
    id,
    name,
    team,
    role,
    pos: { x, y },
    soldiers,
    maxSoldiers: soldiers,
    ammo,
    maxAmmo: ammo,
    suppression: 0,
    workProgress: 0,
    discipline: 'return_fire',
    alive: true,
    activity: { type: 'idle' },
  };
}

export function presetUnits(): UnitState[] {
  return [
    // 排长指挥官（直接指挥半径 140m）
    unit(HQ_UNIT_ID, '排部 Platoon HQ', 'red', 'platoon_hq', 160, 240, 4, 120),
    // 直属 1 班：距 HQ 64m
    unit(SQUAD1_ID, '1 班', 'red', 'rifle_squad', 224, 240, 9, 360),
    // 直属 2 班：距 HQ 96m
    unit(SQUAD2_ID, '2 班', 'red', 'rifle_squad', 160, 336, 9, 360),
    // 远程 3 班：距 HQ 240m（超出直接指挥半径）
    unit(SQUAD3_ID, '3 班', 'red', 'rifle_squad', 400, 240, 9, 360),
    // 敌步兵班（迷雾隐藏，进入 200m 视距后可见）
    unit(ENEMY_SQUAD_ID, '敌步兵班', 'blue', 'enemy_squad', 590, 90, 9, 360),
  ];
}

export function presetGroundObjects(): GroundObject[] {
  return [
    { id: 'item-1', kind: 'item', pos: { x: 252, y: 298 }, label: '弹药箱' },
    { id: 'item-2', kind: 'item', pos: { x: 300, y: 196 }, label: '医疗包' },
    { id: 'weapon-1', kind: 'weapon', pos: { x: 272, y: 168 }, label: 'MG42' },
    { id: 'weapon-2', kind: 'weapon', pos: { x: 344, y: 306 }, label: 'Kar98k' },
    { id: 'corpse-1', kind: 'corpse', pos: { x: 362, y: 282 }, label: '阵亡士兵' },
    { id: 'corpse-2', kind: 'corpse', pos: { x: 292, y: 352 }, label: '阵亡士兵' },
  ];
}

export function presetWaves(): WaveDirective[] {
  return [
    {
      id: 'wave-1',
      title: '连部波次 Ⅰ：炮火准备 + 正面突击',
      issuer: '连部',
      createdTick: 0,
      effectiveTick: 2,
      effectiveExactMs: 9860,
      remote: true,
      delayTicks: 1,
      effective: false,
      geometry: {
        barrages: [{ type: 'circle', center: { x: 470, y: 150 }, radiusM: 48 }],
        assaultArrows: [
          [
            { x: 230, y: 250 },
            { x: 330, y: 220 },
            { x: 430, y: 172 },
          ],
        ],
        aos: [{ min: { x: 200, y: 140 }, max: { x: 470, y: 320 } }],
      },
    },
    {
      id: 'wave-2',
      title: '连部波次 Ⅱ：两翼迂回',
      issuer: '连部',
      createdTick: 0,
      effectiveTick: 6,
      effectiveExactMs: 4200,
      remote: true,
      delayTicks: 2,
      effective: false,
      geometry: {
        barrages: [
          {
            type: 'polygon',
            points: [
              { x: 520, y: 300 },
              { x: 600, y: 282 },
              { x: 610, y: 362 },
              { x: 532, y: 380 },
            ],
          },
        ],
        assaultArrows: [
          [
            { x: 170, y: 344 },
            { x: 300, y: 392 },
            { x: 470, y: 362 },
          ],
          [
            { x: 410, y: 230 },
            { x: 500, y: 202 },
          ],
        ],
        aos: [{ min: { x: 300, y: 180 }, max: { x: 620, y: 420 } }],
      },
    },
  ];
}

export function presetTerrain(): TerrainModel {
  return {
    widthM: WORLD_WIDTH_M,
    heightM: WORLD_HEIGHT_M,
    hills: [
      { center: { x: 470, y: 140 }, radiusM: 120, heightM: 24 },
      { center: { x: 120, y: 110 }, radiusM: 90, heightM: 14 },
      { center: { x: 540, y: 380 }, radiusM: 100, heightM: 18 },
      { center: { x: 300, y: 260 }, radiusM: 70, heightM: 6 },
    ],
    patches: [
      { center: { x: 90, y: 400 }, rx: 90, ry: 50, rotation: 0.3, kind: 'forest' },
      { center: { x: 520, y: 60 }, rx: 80, ry: 40, rotation: -0.2, kind: 'forest' },
      { center: { x: 330, y: 420 }, rx: 70, ry: 30, rotation: 0.1, kind: 'field' },
      { center: { x: 250, y: 90 }, rx: 60, ry: 36, rotation: 0.5, kind: 'field' },
      { center: { x: 40, y: 250 }, rx: 36, ry: 70, rotation: 0, kind: 'marsh' },
    ],
    river: [
      { x: -10, y: 190 },
      { x: 60, y: 210 },
      { x: 110, y: 300 },
      { x: 180, y: 430 },
      { x: 230, y: 490 },
    ],
    roads: [
      [
        { x: -10, y: 260 },
        { x: 150, y: 250 },
        { x: 320, y: 230 },
        { x: 480, y: 260 },
        { x: 650, y: 240 },
      ],
      [
        { x: 320, y: 230 },
        { x: 330, y: 120 },
        { x: 360, y: -10 },
      ],
    ],
  };
}

export function terrainElevation(t: TerrainModel, x: number, y: number): number {
  let h = 0;
  for (const hill of t.hills) {
    const d2 = (x - hill.center.x) ** 2 + (y - hill.center.y) ** 2;
    h += hill.heightM * Math.exp(-d2 / (2 * (hill.radiusM / 2) ** 2));
  }
  return h;
}
