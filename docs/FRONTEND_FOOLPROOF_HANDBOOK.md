# GenTactix V2 前端与美术“傻瓜手把手”开发手册

> **这本手册写给谁？** 写给想直接改颜色、换贴图、加按钮、写新弹窗的前端新手和美术同学。  
> **阅读原则**：**不讲底层数学和复杂原理**，只写 **“你想干嘛 -> 打开哪个文件 -> 照抄哪几行”**。  
> *(底层 HTTP 协议、二进制流和 9 层物理规格在 [FRONTEND_DEV_AND_ART_MANUAL.md](FRONTEND_DEV_AND_ART_MANUAL.md)；让 AI 帮干活看 [AI_FOOLPROOF_PLAYBOOK.md](AI_FOOLPROOF_PLAYBOOK.md)。)*

---

## 第 1 章 · 5 分钟把画面跑起来

### 1.1 纯前端离线练习（只要 1 个终端，适合做美术/改 UI）

```powershell
Set-Location D:\tools\gentactix\frontend
npm install
npm run dev
```

打开浏览器访问 `http://127.0.0.1:5173`，看到**米色战区地图 + 顶部黑色控制栏 + 左边三块浮窗 + 右边手写板抽屉**，就代表成功了。

### 1.2 连接真 Rust 引擎服务（2 个终端）

**终端 1（先启动真后端，监听 `8767`）：**

```powershell
Set-Location D:\tools\gentactix
cargo run --release -p gentactix-server -- --listen 127.0.0.1:8767 --rows 64 --cols 64
```

**终端 2（再启动前端开发服务，监听 `5173`）：**

```powershell
Set-Location D:\tools\gentactix\frontend
npm run dev
```

打开网页 `http://127.0.0.1:5173` 后，在界面上做 **3 下点击**：
1. 点顶栏最右边的 **`开发`** 按钮（角标 `W1.dock.W7`）；
2. 在弹出的 `W7` 面板里点 **`HTTP 真后端`**（`W7.B1.opt_http`）；
3. 点 **`测连通 /health`**（`W7.B3`），下面出现绿色 **`健康 v2.0.0`** 即连上真后端！

### 1.3 常见 3 个报错自救

| 报错现象 | 原因 | 一行解决 |
| :--- | :--- | :--- |
| `'vite' 不是内部或外部命令` | 没装前端依赖包 | 在 `frontend/` 下跑一次 `npm install` |
| 点 `测连通 /health` 显示 `离线/无法连接` | 终端 1 没启动，或端口没开在 `8767` | 检查终端 1 是否正运行 `gentactix-server`；或把 `W7.B2` 地址框留空走 Vite 代理 |
| 窗口拖乱了 / 被自己关掉了找不到 | 窗口位置存进了浏览器缓存 | 点顶部坞栏最右的 **`重置布局`**（`W1.dock.reset`）一秒复原 |

---

## 第 2 章 · “我想改 X，该打开哪个文件？”人话速查表

| 你想干什么 | 打开这个文件（相对于仓库根目录） | 改哪里 |
| :--- | :--- | :--- |
| **改整体配色**（红方色、蓝方色、地图纸底色、字体） | [`frontend/src/art/themeTokens.ts`](../frontend/src/art/themeTokens.ts) | 改 `THEME_TOKENS` 里的 `#十六进制色号` |
| **换地图底图 / 坦克 / 士兵 / 弹坑贴图** | 直接把图片拖进 `frontend/src/assets/...` | 文件路径对齐第 3 章表即可，无需改代码 |
| **打包后不重编译直接替换贴图** | [`frontend/public/assets/manifest.json`](../frontend/public/assets/manifest.json) | 在 `"overrides"` 填上槽位名和图片 URL |
| **改面板边框/背景/暗色按钮样式** | [`frontend/src/styles.css`](../frontend/src/styles.css) | 找 `.panel`、`.btn`、`.floating-win-header` |
| **改各浮动窗口默认宽度/标题/开闭状态** | [`frontend/src/store/windowStore.ts`](../frontend/src/store/windowStore.ts) | 改 `createDefaultWindowLayout()` 里的对应项 |
| **改顶栏时间/倍速/天气/一键藏标** | [`frontend/src/ui/TopControlBar.tsx`](../frontend/src/ui/TopControlBar.tsx) | 对应控件 `T1`~`T14` |
| **改左上画笔/颜色块** | [`frontend/src/ui/FreehandToolbar.tsx`](../frontend/src/ui/FreehandToolbar.tsx) | 改 `COLORS` 或 `F1`~`F6` |
| **改部队信息 / 单兵 4 部位 / 坦克 8 部件展示** | [`frontend/src/ui/UnitInspector.tsx`](../frontend/src/ui/UnitInspector.tsx) | 找 `UnitInspectorBody` 与 `W3.I6` / `W3.I7` |
| **加一个自己的新功能面板** | [`frontend/src/ui/ExampleCustomWindow.tsx`](../frontend/src/ui/ExampleCustomWindow.tsx) | **整文件复制**改成你自己的（见第 4 章） |
| **改兵牌/地形 Canvas 程序化画法** | [`frontend/src/render/canvasLayers.ts`](../frontend/src/render/canvasLayers.ts) | 找 `renderLayer1BaseMap` ~ `renderLayer7VfxA` |

---

## 第 3 章 · 美术零代码换图与换色实操

### 3.1 铁定不会报错的机制：没图用矢量兜底，有图立刻覆盖

前端内置 **零 404 机制**：
- 你不放任何 PNG 时，网页 **零网络报错**，自动画军用地图底纸与北约兵牌；
- 你放进去**第 1 张**图片，只有那 1 个槽位切换成你的手绘图，其他 16 个继续兜底。

### 3.2 全量 17 个美术槽位放置路径

> **推荐做法（方式 A）**：直接放到 **`frontend/src/assets/`** 下（支持 `.png` / `.webp` / `.svg`，保存即刻热更新）。

| 槽位 ID（`W7` 显示） | 放到 `frontend/src/assets/` 里的精确路径 | 所属层级 | 建议像素尺寸 | 用途说明 |
| :--- | :--- | :---: | :---: | :--- |
| `map.base` | `map/base_terrain.png` | L1 | `1920x1440` | 覆盖整张 `640x480m` 战场的连续手绘底图 |
| `map.ao` | `map/ao_shadow.png` | L3 | `1920x1440` | 透明黑白全局光影遮蔽贴图（压在底图和弹坑之上） |
| `decal.crater` | `decals/crater.png` | L2 | `128x128` | 透明背景弹坑 |
| `decal.scorch` | `decals/scorch.png` | L2 | `128x128` | 透明背景焦黑灼痕 |
| `decal.foxhole` | `decals/foxhole.png` | L2 | `128x64` | 散兵坑贴花（默认朝右 `0°`） |
| `decal.trench` | `decals/trench.png` | L2 | `128x64` | 工兵堑壕贴花（默认朝右 `0°`） |
| `unit.platoon_hq_red` | `units/platoon_hq_red.png` | L4 | `96x96` | 红方排部兵牌 |
| `unit.rifle_squad_red` | `units/rifle_squad_red.png` | L4 | `96x96` | 红方步兵/工兵/重火力班图标 |
| `unit.enemy_squad_blue` | `units/enemy_squad_blue.png` | L4 | `96x96` | 蓝方敌军班图标 |
| `unit.armored_vehicle_red` | `units/armored_vehicle_red.png` | L4 | `128x128` | 红方装甲车车体（**车头朝右 +X = 0°**） |
| `unit.armored_vehicle_blue` | `units/armored_vehicle_blue.png` | L4 | `128x128` | 蓝方装甲车车体（**车头朝右 +X = 0°**） |
| `unit.turret_red` | `units/turret_red.png` | L4 | `96x96` | 红方独立炮塔（可选，**炮口朝右 +X = 0°**） |
| `unit.turret_blue` | `units/turret_blue.png` | L4 | `96x96` | 蓝方独立炮塔（可选，**炮口朝右 +X = 0°**） |
| `obj.item` | `objects/item.png` | L4 | `48x48` | 地面物资箱 |
| `obj.weapon` | `objects/weapon.png` | L4 | `48x48` | 散落武器 |
| `obj.corpse` | `objects/corpse.png` | L4 | `48x48` | 阵亡标识 |
| `fx.smoke` | `fx/smoke.png` | L4 | `256x256` | 半透明烟幕云团 |

> **方式 B（打包成 `dist/` 后不用重新编译直接换图）**：把图片放到 `frontend/public/assets/...`，再编辑 [`frontend/public/assets/manifest.json`](../frontend/public/assets/manifest.json)：
> ```json
> {
>   "overrides": {
>     "unit.armored_vehicle_red": "/assets/units/armored_vehicle_red.png"
>   }
> }
> ```
> 然后点页面里 `W7 -> 热重载 public/assets`（`W7.A1`）立刻见效。

### 3.3 画图 3 条硬规矩（画错方向会倒着开）

1. **所有会旋转的贴图，统一画成“车头 / 炮口 / 线段朝正右边（+X 方向 = 0°）”**。代码里会自动乘 `facingDeg` 旋转；
2. **底图（L1 / L3）绝对不能出现 8m 小方格线**，保持连续手绘地形质感；
3. 画小图/兵牌优先输出 **透明底 PNG**（`sRGB`，不要带奇怪的颜色 ICC Profile）。

### 3.4 打开 `W7` 美术调试台验证效果

点顶栏坞栏 **`开发`**（`W1.dock.W7`）：
- **看命中情况**：列表里写 **`已命中PNG`** 说明文件放对了；写 **`矢量兜底`** 说明还没放或路径没对上；
- **一键看损伤 UI 样子**：点 **`预览单兵4部位卡`**（`W7.D3`）或 **`预览坦克8部件卡`**（`W7.D4`），左侧 `W3` 窗格立刻展示真实样式的 4 部位 / 8 子系统绿黄红状态条；
- **测效果**：点 **`L5 i_threat 威胁场`**（`W7.D1`）看红色火线热区，点 **`测试爆破震屏`**（`W7.D2`）看 L9 震屏手感；
- **截纯净效果图**：点顶栏 **`ID 标牌：开/关`**（`T12`），屏幕所有彩框数字角标一键全部隐藏。

---

## 第 4 章 · 前端程序员 3 步照抄加新窗口 & 一行发指令

### 4.1 保姆心法：不要往外传 `props`，用 `useGame()` 一把拿齐所有数据

以前改一个按钮要从 `App.tsx` 一层层往下传十几个参数。现在**任何浮动窗口内部**，只需要 1 行：

```tsx
const game = useGame();
```

### 4.2 三步做出你自己的新面板 `W9`（直接复制下面的代码就能跑）

#### 第 1 步：新建 `frontend/src/ui/MyTacticalPanel.tsx`（整段复制）

```tsx
import React, { useState } from 'react';
import { useGame } from '../store/gameContext';
import { UiButton, UiSelect, UiToggle } from './UiControls';
import { UiTag } from './UiTag';

export const MyTacticalPanel: React.FC = () => {
  const game = useGame();
  const [stance, setStance] = useState<'normal' | 'fast' | 'cautious'>('normal');
  const [chase, setChase] = useState(true);

  const unit = game.selectedUnit ?? game.displaySnapshot?.units[0] ?? null;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div className="meta-note">
        <UiTag id="W9.info" />
        回合 #{game.tickNumber} · 选中：{unit ? unit.name : '无单位'}
      </div>

      <UiSelect
        uiId="W9.stance"
        ariaLabel="机动姿态"
        selectClassName="select"
        value={stance}
        onChangeValue={(v) => setStance(v as 'normal' | 'fast' | 'cautious')}
        options={[
          { value: 'normal', label: '正常速度', optId: 'W9.stance.opt_normal' },
          { value: 'fast', label: '急行军', optId: 'W9.stance.opt_fast' },
          { value: 'cautious', label: '谨慎警戒', optId: 'W9.stance.opt_cautious' },
        ]}
      />

      <UiToggle
        uiId="W9.chase"
        checked={chase}
        onChange={setChase}
        label="追击开关"
        optOnId="W9.chase.opt_on"
        optOffId="W9.chase.opt_off"
      />

      <UiButton
        uiId="W9.btn_advance"
        className="btn primary"
        disabled={!unit}
        onClick={() => {
          if (!unit) return;
          void game.sendQuickCommand({
            kind: 'move_unit',
            unit_id: unit.id,
            waypoints: [{ x: unit.pos.x + 40, y: unit.pos.y }],
            stance,
          });
        }}
      >
        向右前出 40m
      </UiButton>
    </div>
  );
};
```

#### 第 2 步：去 [`frontend/src/store/windowStore.ts`](../frontend/src/store/windowStore.ts) 登记 `'W9'`

在文件顶部加进 `'W9'`：

```ts
export type ManagedWindowId = 'W2' | 'W3' | 'W4' | 'W5' | 'W7' | 'W8' | 'W9';
export const MANAGED_WINDOW_IDS: ManagedWindowId[] = ['W2', 'W3', 'W4', 'W5', 'W7', 'W8', 'W9'];
```

在 `createDefaultWindowLayout()` 里照抄加一项：

```ts
W9: {
  id: 'W9',
  title: '我的自定义指挥板',
  shortLabel: '自定',
  sourceFile: 'src/ui/MyTacticalPanel.tsx',
  floating: true,
  x: 340,
  y: 150,
  width: 320,
  open: false,
  minimized: false,
  zIndex: 32,
},
```

#### 第 3 步：去 [`frontend/src/App.tsx`](../frontend/src/App.tsx) 把它挂上画面

引入 `MyTacticalPanel`，在 `<FloatingWindow id="W8">` 后面多贴一段：

```tsx
<FloatingWindow
  id="W9"
  winState={winMgr.windows.W9}
  onFocus={winMgr.focusWindow}
  onMove={winMgr.moveWindow}
  onToggleMinimize={winMgr.toggleMinimizeWindow}
  onClose={winMgr.closeWindow}
>
  <MyTacticalPanel />
</FloatingWindow>
```

顶栏 `W1.dock.W9` 按钮、拖动、最小化、关闭、缓存位置、`T12` 藏标**全部自动拥有**！

---

### 4.3 `useGame()` 最常用的 12 个东西（人话速览）

| 写法 | 拿来干嘛 |
| :--- | :--- |
| `game.displaySnapshot` | **当前画面上应该渲染的世界快照**（已按 16s 走表插值且防剧透，画图一律读它） |
| `game.selectedUnit` | 当前选中的单位对象（`id`, `name`, `pos`, `soldiers`, `ammo`, `suppression`） |
| `game.selectedUnitDetail` | 后端返回的详细单兵 4 部位 / 坦克 8 子系统数据 |
| `game.tickNumber` | 当前所在回合号（整数） |
| `game.elapsedMs` | 当前 16s 回合内的走表毫秒（`0..16000`） |
| `game.backendMode` | `'mock'` 或 `'http'`，看现在是离线演练还是连真后端 |
| `game.sendQuickCommand(cmd)` | **一行发战术指令**并自动刷新画面与 `game.lastCommandOutcome` 回执 |
| `game.stepNext()` | 推进下一拍（`+16s`） |
| `game.jumpToTick(n)` | 快进到第 `#n` 回合 |
| `game.openWindow('W3')` / `closeWindow('W7')` | 代码控制某个浮动窗口开关 |
| `game.resetWindows()` | 把所有被拖乱/关掉的窗口复位 |
| `game.reloadArt()` / `game.triggerSampleShake()` | 热重载贴图 / 触发 L9 爆破震屏 |

---

### 4.4 6 个直接能复制运行的发令代码（`sendQuickCommand`）

```ts
// 1) 命令选中单位机动到指定点（normal / fast / cautious）
void game.sendQuickCommand({
  kind: 'move_unit',
  unit_id: unit.id,
  waypoints: [{ x: 320, y: 240 }],
  stance: 'cautious',
});

// 2) 命令向地图坐标火力压制
void game.sendQuickCommand({
  kind: 'attack_pos',
  unit_id: unit.id,
  target: { x: 400, y: 220 },
});

// 3) 命令追打特定敌方单位 ID
void game.sendQuickCommand({
  kind: 'attack_unit',
  unit_id: unit.id,
  target_id: 9,
  chase: true,
});

// 4) 就地固守待命
void game.sendQuickCommand({ kind: 'hold_unit', unit_id: unit.id });

// 5) 施放烟幕遮蔽（'hold_fire' | 'return_fire' | 'fire_at_will' | 'suppressive' 另配 set_fire_discipline）
void game.sendQuickCommand({ kind: 'pop_smoke', unit_id: unit.id, target: { x: 300, y: 240 } });

// 6) 工兵构筑工事（'foxhole_line' | 'zigzag_trench' | 'minefield' | 'demolition'）
void game.sendQuickCommand({
  kind: 'build_work',
  unit_id: unit.id,
  work: 'foxhole_line',
  waypoints: [{ x: 240, y: 240 }, { x: 280, y: 240 }],
});
```

---

## 第 5 章 · 全量界面 UI ID 寻址字典（在画面看角标 -> 直接定位源码）

> 画面上黄底黑字是**窗口 ID（`W?`）**，蓝底白字是**按钮/区域 ID**，青底白字或 `[...]` 是**选项 ID**。悬停窗口角标可直接看到源码文件名；点顶栏 `T12` 可一键全部隐藏/恢复。

| 窗口 ID | 窗口名称 | 源码文件 | 内部控件与选项 ID 全清单 |
| :---: | :--- | :--- | :--- |
| **`W1`** | 顶部 16s 时钟与坞栏 | [`TopControlBar.tsx`](../frontend/src/ui/TopControlBar.tsx)<br>[`WindowDockBar.tsx`](../frontend/src/ui/WindowDockBar.tsx) | `T1` 当前回合/秒数、`T2` 推进 +16s、`T3` 播停、`T4` 倍速（选项 `W1.T4.opt_1x`/`opt_2x`/`opt_4x`）、`T5` 走表滑块、`T6` 跳转输入框、`T7` 快进 `#3`、`T8` 未来书签栏、`T9` L7 开关、`T10` 天气（`W1.T10.opt_fog`/`opt_rain`/`opt_snow`）、`T11` L9 开关、`T12` **ID 标牌显隐**、`T13` 右抽屉收折、`T14` 前向预警横幅；坞栏 `W1.dock.W2`~`W8`、`W1.dock.reset` |
| **`W2`** | L6 手绘板（仅本地） | [`FreehandToolbar.tsx`](../frontend/src/ui/FreehandToolbar.tsx) | 窗口按钮 `W2.btn_min` / `W2.btn_close`；`F1` 鼠标选兵、`F2` 手绘笔、`F3` 橡皮擦、`F4` 颜色（`W2.F4.opt_red`/`opt_blue`/`opt_green`/`opt_black`）、`F5` 清空、`F6` 笔画计数 |
| **`W3`** | 单位态势检视器 | [`UnitInspector.tsx`](../frontend/src/ui/UnitInspector.tsx) | `W3.btn_min` / `W3.btn_close`；`I1` 单位切换、`I2` 名称坐标、`I3` 兵力弹药压制、`I4` 姿态进度、`I5` 排长/远程延迟提示、`W3.I6` **单兵 4 部位健康卡**、`W3.I7` **装甲 8 子系统与 RHA 卡** |
| **`W4`** | 16s 演出日志 | [`UnitInspector.tsx`](../frontend/src/ui/UnitInspector.tsx) | `W4.btn_min` / `W4.btn_close`；`G1` 逐毫秒事件播报区 |
| **`W5`** | 右侧手写板 & 司令部抽屉 | [`App.tsx`](../frontend/src/App.tsx)<br>[`UioClipboardPanel.tsx`](../frontend/src/ui/UioClipboardPanel.tsx)<br>[`CommanderWavePanel.tsx`](../frontend/src/ui/CommanderWavePanel.tsx) | `W5.btn_min` / `W5.btn_close`；页签 `R1` UIO / `R2` 指挥 UI / `R3` 折叠；<br>**`W5A`（UIO）**：`U1` (`U1-1`..) 回形针波次、`U2` 指令形态（`W5A.U2.opt_task`/`opt_macro`）、`U3` 参数下拉（`W5A.U3.task/work/stance/fire.opt_*`）、`U4` 时间窗、`U5` 延迟倍率、`U6` 绑目标、`U7` 生成草稿、`U8` 队列区、`U9` 签发按钮、`U10` 冲突时间轴、`U11`~`U17` 卡片/报错/归档、`U18` L5 航路预览；<br>**`W5B`（指挥 UI）**：`C1` 波次名、`C2` 取消工具、`C3` 炮击圆、`C4` 冲锋箭头、`C5` AO 框、`C6` 确认图元、`C7` 撤销、`C8` 清空、`C9` 签发波次 |
| **`W6`** | 地图左下角缩放测距条 | [`NineLayerViewport.tsx`](../frontend/src/render/NineLayerViewport.tsx) | `M1` 缩放 `+/-/100%`、`M2` 实时米制坐标、`M3` 拾取提示条、`M4` 图层图例 |
| **`W7`** | 保姆美术与真后端面板 | [`DevArtPanel.tsx`](../frontend/src/ui/DevArtPanel.tsx) | `W7.btn_min` / `W7.btn_close`；`W7.B1` 数据源（`W7.B1.opt_mock` / `W7.B1.opt_http`）、`W7.B2` 地址框、`W7.B3` 测 `/health`、`W7.A1` 热重载贴图、`W7.D1` `L5 i_threat` 开关、`W7.D2` 测试震屏、`W7.D3` 单兵样板、`W7.D4` 坦克样板、`W7.D6` 开关 W8、`W7.W1` 重置布局 |
| **`W8`** | 示例扩展模板（复制即用） | [`ExampleCustomWindow.tsx`](../frontend/src/ui/ExampleCustomWindow.tsx) | `W8.btn_min` / `W8.btn_close`；`W8.S1` 状态卡、`W8.opt_hold_selected` 一键固守、`W8.opt_step_next` 推进一步 |

---

## 第 6 章 · 绝对别踩的 6 条红线 & 交作业前体检命令

1. **禁止直接在组件里写 `fetch('/api/...')`**：一律走 `game.sendQuickCommand(...)` 或 `BackendAdapter`，否则离线 Mock 模式会直接报错；
2. **禁止在任何 Canvas 图层画 `8m` 小方格线**：后端是 8m 网格，前端视觉必须保持连续地形画风；
3. **禁止删改现有的 `data-testid`**：自动化测试（`phase1`~`phase5` & E2E）靠它们找按钮；
4. **不要把 `W2`~`W8` 的窗口 ID 标错或写重**：每一个 `W?` 窗口角标同一时间只该出现 1 次（由 `<FloatingWindow>` 自动渲染，别在内层再手写一遍 `<UiTag id="W2" />`）；
5. **所有文件保持 UTF-8 无 BOM 编码**；
6. **改完跑一次这三句，全绿再提交：**

```powershell
Set-Location D:\tools\gentactix\frontend
npm run typecheck
npm test
npm run build
```
