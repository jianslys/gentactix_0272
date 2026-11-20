# GenTactix V2 前端美术资源投放指南 (`src/assets` + `public/assets/manifest.json`)

本前端采用**零 404、双轨丢图即生效**机制：
- **未放置任何图片时，浏览器不发起任何 404 请求，控制台零报错，系统自动用程序化矢量兜底。**
- 只要放入对应名称的图片文件（推荐 PNG / WebP / SVG，支持 Vite 热更新），立刻替换对应图层！

## 方式 A（最推荐 · 零配置热更新）：直接放入 `frontend/src/assets/...`

Vite 已配置 `import.meta.glob` 自动扫描 `frontend/src/assets/`。把图存到下方相对路径即自动生效：

| 槽位 ID | 放入 `frontend/src/assets/` 路径 | 所属层 | 建议尺寸 | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| `map.base` | `map/base_terrain.png` | L1 底图 | `1920x1440` | 覆盖 `640x480m` 战场的连续手绘地形 |
| `map.ao` | `map/ao_shadow.png` | L3 AO | `1920x1440` | 透明背景环境光遮蔽阴影贴图 |
| `decal.crater` | `decals/crater.png` | L2 破坏 | `128x128` | 透明背景弹坑贴花 |
| `decal.scorch` | `decals/scorch.png` | L2 破坏 | `128x128` | 透明背景焦痕贴花 |
| `decal.foxhole` | `decals/foxhole.png` | L2 破坏 | `128x64` | 散兵坑贴花（默认朝右 `0°`） |
| `decal.trench` | `decals/trench.png` | L2 破坏 | `128x64` | 堑壕贴花（默认朝右 `0°`） |
| `unit.platoon_hq_red` | `units/platoon_hq_red.png` | L4 单位 | `96x96` | 红方排部兵牌 |
| `unit.rifle_squad_red` | `units/rifle_squad_red.png` | L4 单位 | `96x96` | 红方步兵/工兵/重火力班图标 |
| `unit.enemy_squad_blue` | `units/enemy_squad_blue.png` | L4 单位 | `96x96` | 蓝方敌军班图标 |
| `unit.armored_vehicle_red` | `units/armored_vehicle_red.png` | L4 单位 | `128x128` | 红方装甲车车体（车头默认朝右 `+X`） |
| `unit.armored_vehicle_blue` | `units/armored_vehicle_blue.png` | L4 单位 | `128x128` | 蓝方装甲车车体（车头默认朝右 `+X`） |
| `unit.turret_red` | `units/turret_red.png` | L4 单位 | `96x96` | 红方炮塔（可选，炮口朝右 `+X`） |
| `unit.turret_blue` | `units/turret_blue.png` | L4 单位 | `96x96` | 蓝方炮塔（可选，炮口朝右 `+X`） |
| `obj.item` | `objects/item.png` | L4 物体 | `48x48` | 地面物资箱 |
| `obj.weapon` | `objects/weapon.png` | L4 物体 | `48x48` | 散落武器 |
| `obj.corpse` | `objects/corpse.png` | L4 物体 | `48x48` | 阵亡标识 |
| `fx.smoke` | `fx/smoke.png` | L4 特效 | `256x256` | 半透明烟幕精灵图 |

## 方式 B（打包后不重新编译覆盖）：放在 `public/assets/` 并登记 `manifest.json`

如果想在构建产物（`dist/`）后不重编译直接替换贴图：
1. 把图片放进 `frontend/public/assets/...`；
2. 打开 [manifest.json](./manifest.json)，在 `"overrides"` 中写上槽位与 URL，例如：
   ```json
   {
     "overrides": {
       "unit.armored_vehicle_red": "/assets/units/my_tank.png"
     }
   }
   ```
3. 点击右上角 `W1.dock.W7 -> 热重载` 立即生效。

## 朝向与色板规范

1. 有朝向旋转的素材（装甲车、炮塔、线段贴花）**统一以向右 `+X` 为 `0°`**。
2. 想要改界面和兵牌的整体颜色时，只改 [`src/art/themeTokens.ts`](../../src/art/themeTokens.ts) 里的 `THEME_TOKENS`。
