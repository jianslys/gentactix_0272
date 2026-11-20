// GenTactix V2 frontend · 丢图即生效的美术资源注册表（Drop-in Art Asset Manifest）
// 规则：美术只需要把同名 PNG 放进 `frontend/public/assets/...` 对应路径即可生效；
//       没放图时，Canvas 渲染器会自动回退到内置程序化矢量渲染，绝不白屏报错。

export type ArtSlotKey =
  | 'map.base'
  | 'map.ao'
  | 'decal.crater'
  | 'decal.scorch'
  | 'decal.foxhole'
  | 'decal.trench'
  | 'unit.platoon_hq_red'
  | 'unit.rifle_squad_red'
  | 'unit.enemy_squad_blue'
  | 'unit.armored_vehicle_red'
  | 'unit.armored_vehicle_blue'
  | 'unit.turret_red'
  | 'unit.turret_blue'
  | 'obj.item'
  | 'obj.weapon'
  | 'obj.corpse'
  | 'fx.smoke';

export interface ArtSlotSpec {
  key: ArtSlotKey;
  url: string;
  layer: 'L1' | 'L2' | 'L3' | 'L4';
  label: string;
  recommendedPx: string;
  description: string;
}

export const ART_ASSET_MANIFEST: Record<ArtSlotKey, ArtSlotSpec> = {
  'map.base': {
    key: 'map.base',
    url: '/assets/map/base_terrain.png',
    layer: 'L1',
    label: '战场连续基础底图',
    recommendedPx: '1920x1440',
    description: '覆盖全图 640x480m 的连续地形图；未放置时自动使用程序化等高线与道路底图',
  },
  'map.ao': {
    key: 'map.ao',
    url: '/assets/map/ao_shadow.png',
    layer: 'L3',
    label: '全局环境光遮蔽图 (AO)',
    recommendedPx: '1920x1440',
    description: '叠加在 L1/L2 之上的透明黑白阴影视效贴图，建议自带透明通道',
  },
  'decal.crater': {
    key: 'decal.crater',
    url: '/assets/decals/crater.png',
    layer: 'L2',
    label: '炮击弹坑贴花',
    recommendedPx: '128x128',
    description: '透明背景弹坑贴花，压在 L3 AO 之下',
  },
  'decal.scorch': {
    key: 'decal.scorch',
    url: '/assets/decals/scorch.png',
    layer: 'L2',
    label: '焦土燃烧贴花',
    recommendedPx: '128x128',
    description: '透明背景焦痕贴花',
  },
  'decal.foxhole': {
    key: 'decal.foxhole',
    url: '/assets/decals/foxhole.png',
    layer: 'L2',
    label: '散兵坑单段贴花',
    recommendedPx: '128x64',
    description: '散兵坑线段中点贴花（默认朝向为 0° 水平向右）',
  },
  'decal.trench': {
    key: 'decal.trench',
    url: '/assets/decals/trench.png',
    layer: 'L2',
    label: '堑壕单段贴花',
    recommendedPx: '128x64',
    description: '工兵构筑堑壕线段贴花（默认朝向为 0° 水平向右）',
  },
  'unit.platoon_hq_red': {
    key: 'unit.platoon_hq_red',
    url: '/assets/units/platoon_hq_red.png',
    layer: 'L4',
    label: '红方排部兵牌',
    recommendedPx: '96x96',
    description: '我方排部指挥单位图标',
  },
  'unit.rifle_squad_red': {
    key: 'unit.rifle_squad_red',
    url: '/assets/units/rifle_squad_red.png',
    layer: 'L4',
    label: '红方步兵班图标',
    recommendedPx: '96x96',
    description: '我方步兵/工兵/重火力班图标',
  },
  'unit.enemy_squad_blue': {
    key: 'unit.enemy_squad_blue',
    url: '/assets/units/enemy_squad_blue.png',
    layer: 'L4',
    label: '蓝方敌军班图标',
    recommendedPx: '96x96',
    description: '敌方单位通用图标',
  },
  'unit.armored_vehicle_red': {
    key: 'unit.armored_vehicle_red',
    url: '/assets/units/armored_vehicle_red.png',
    layer: 'L4',
    label: '红方装甲车底盘',
    recommendedPx: '128x128',
    description: '随 facingDeg 旋转的装甲车车体（默认车头朝右 +X）',
  },
  'unit.armored_vehicle_blue': {
    key: 'unit.armored_vehicle_blue',
    url: '/assets/units/armored_vehicle_blue.png',
    layer: 'L4',
    label: '蓝方装甲车底盘',
    recommendedPx: '128x128',
    description: '敌方装甲车车体（默认车头朝右 +X）',
  },
  'unit.turret_red': {
    key: 'unit.turret_red',
    url: '/assets/units/turret_red.png',
    layer: 'L4',
    label: '红方炮塔图层',
    recommendedPx: '96x96',
    description: '叠在装甲车底盘上方的炮塔（可选，默认炮口朝右 +X）',
  },
  'unit.turret_blue': {
    key: 'unit.turret_blue',
    url: '/assets/units/turret_blue.png',
    layer: 'L4',
    label: '蓝方炮塔图层',
    recommendedPx: '96x96',
    description: '敌方装甲车炮塔（可选，默认炮口朝右 +X）',
  },
  'obj.item': {
    key: 'obj.item',
    url: '/assets/objects/item.png',
    layer: 'L4',
    label: '地面物资箱图标',
    recommendedPx: '48x48',
    description: '散落弹药箱/补给物图标',
  },
  'obj.weapon': {
    key: 'obj.weapon',
    url: '/assets/objects/weapon.png',
    layer: 'L4',
    label: '遗落武器图标',
    recommendedPx: '48x48',
    description: '战损遗留重武器图标',
  },
  'obj.corpse': {
    key: 'obj.corpse',
    url: '/assets/objects/corpse.png',
    layer: 'L4',
    label: '阵亡标记图标',
    recommendedPx: '48x48',
    description: 'L4 层阵亡印记图标',
  },
  'fx.smoke': {
    key: 'fx.smoke',
    url: '/assets/fx/smoke.png',
    layer: 'L4',
    label: '烟幕贴图',
    recommendedPx: '256x256',
    description: '半透明烟雾团精灵贴图',
  },
};

export const ART_SLOT_LIST: ArtSlotSpec[] = Object.values(ART_ASSET_MANIFEST);
