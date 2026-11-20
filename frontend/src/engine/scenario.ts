// GenTactix V2 frontend · 本地模拟沙盒预置测试世界（100% 对齐 SPEC 6.1）

import type { GroundObject, TerrainModel, UnitState, WaveDirective } from './types';

export const WORLD_WIDTH_M = 640;
export const WORLD_HEIGHT_M = 480;
export const DIRECT_COMMAND_RADIUS_M = 140;
export const RADIO_RADIUS_M = 600;
export const VISION_RADIUS_M = 300;

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
    {
      id: 'item-1',
      kind: 'item',
      pos: { x: 252, y: 298 },
      label: '弹药箱',
      category: '补给物品',
      spec: '7.92×57mm Mauser 木制弹药箱',
      detail: '内含 300 发步机通用弹（15 匣），可用于前线班组紧急补给。',
      status: '密封完好 · 可拾取',
    },
    {
      id: 'item-2',
      kind: 'item',
      pos: { x: 300, y: 196 },
      label: '医疗包',
      category: '医疗物资',
      spec: '野战急救箱（绷带×6 / 止血带×2 / 吗啡×2）',
      detail: '供步兵班卫生员或单兵止血包扎使用，降低失血速率。',
      status: '完好 · 剩余使用 4 次',
    },
    {
      id: 'weapon-1',
      kind: 'weapon',
      pos: { x: 272, y: 168 },
      label: 'MG42',
      category: '散落武器',
      spec: 'MG42 通用机枪（7.92mm · 附 50 发弹鼓）',
      detail: '枪管微热，脚架展开态，可供重火力组或步兵回收架设。',
      status: '余弹 38/50 发 · 可拾取',
    },
    {
      id: 'weapon-2',
      kind: 'weapon',
      pos: { x: 344, y: 306 },
      label: 'Kar98k',
      category: '散落武器',
      spec: 'Karabiner 98k 栓动步枪（7.92mm）',
      detail: '枪托有轻微擦伤，膛线正常，附弹仓 4 发及背带。',
      status: '余弹 4/5 发 · 机械正常',
    },
    {
      id: 'corpse-1',
      kind: 'corpse',
      pos: { x: 362, y: 282 },
      label: '阵亡士兵',
      category: '阵亡遗体',
      spec: '步兵阵亡遗体 #A-04（前哨侦察兵）',
      detail: '躯干受弹片致命伤阵亡；随身携带 7.92mm 弹袋×2、工兵铲、水壶。',
      status: '已确认阵亡 · 携具可搜检',
    },
    {
      id: 'corpse-2',
      kind: 'corpse',
      pos: { x: 292, y: 352 },
      label: '阵亡士兵',
      category: '阵亡遗体',
      spec: '敌方机枪副射手遗体 #B-11',
      detail: '头部创伤阵亡；随身携带 50 发弹链盒×1、望远镜、急救包。',
      status: '已确认阵亡 · 携具可搜检',
    },
    {
      id: 'charge-1',
      kind: 'charge',
      pos: { x: 330, y: 330 },
      label: '定时炸药',
      category: '定时爆破物',
      spec: '3kg 制式爆破药包（延时引信）',
      detail: '预设于 #4 回合 7.3s 自动起爆，杀伤半径 24m，注意规避。',
      status: '引信已激活 · #4 回合起爆',
    },
    {
      id: 'mine-1',
      kind: 'mine',
      pos: { x: 396, y: 214 },
      label: '跳雷',
      category: '防步兵地雷',
      spec: 'S-Mine 35 反步兵破片跳雷',
      detail: '触发压发/绊发双引信，抛射高度 1.2m，破片致死半径 18m。',
      status: '待发警戒状态 · 勿踏入',
    },
    {
      id: 'mine-2',
      kind: 'mine',
      pos: { x: 458, y: 256 },
      label: '反坦雷',
      category: '反装甲地雷',
      spec: 'Tellermine 42 反坦克压发雷',
      detail: '装药 5.5kg TNT，起爆压力 >180kg，封锁东侧道路咽喉。',
      status: '待发警戒状态 · 道路埋设',
    },
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
