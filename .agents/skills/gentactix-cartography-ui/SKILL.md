---
name: gentactix-cartography-ui
description: >-
  Gentactix 印刷地图战术指挥前端与无头浏览器像素级验真法典 (Gentactix Cartography UI & E2E Verification Codex). ACTIVATE this skill whenever reading, modifying, or testing frontend code in web/index.html, art-direction/, or tests/e2e/.
---

# Gentactix 印刷地图战术指挥前端施工法典 (Cartography UI Codex)

本技能定义 `web/index.html` 与 `art-direction/command-map-concept.html` 的视觉美学、DOM 契约、后端 API 绑定规范与无头 Chrome 自动化像素验真标准。

---

## 一 · 视觉与布局绝对红线 (SD2 / CMO Printed Map Aesthetics)

1. **色彩与材质变量锁死**：
   - 保持 `:root` 军用印刷色板：`--ink:#d6d9d4; --dim:#8f9b9b; --chrome:#242e32; --deep:#1b2428; --line:#3d484c; --paper:#ded8be; --blue:#436779; --red:#a94f43; --gold:#c5aa70; --green:#91a58c;`；
   - 保持 SVG `#paperGrain` 噪点滤镜、`#grid` 格网与 `#fieldHatch` / `#frontHatch` 印刷斜线填充。
2. **Workspace 双顶级容器铁律**：
   - `.workspace` 下**只允许存在两个直接子元素**：左侧 `#inspector.inspector`（深色检视面板）与右侧 `main.map-area`（内含 `#viewport.map-viewport`）；
   - **绝对禁止**引入旧版笨重的 `.sidebar`、`#rosterPane`、`#eventsPane` 或底部滚动日志条 `.map-log`；
   - 地图 `#viewport` 必须延伸至屏幕最底端（`bottom === innerHeight`）。
3. **19 个 UI-ID 评审标识与无滚动铁律**：
   - 全部 19 个 `[data-ui-id]`（`UI-01` 至 `UI-13` 及子项 `UI-03A~E`、`UI-05A`）必须保持唯一且伪元素 `::after` 渲染正常；
   - 桌面端（1600×900）下，`#inspector` 的全部核心内容与底部按钮（`#planButton`、`#fireButton`）必须一屏完整放下，严禁出现垂直滚动条（`inspector.scrollHeight === inspector.clientHeight`）。
4. **地图周边控件布局**：
   - 右上角：`.map-stamp`（作战用图）、`.compass-wrap`（指北针）、`.map-tools`（缩放与复位）垂直依次排列，严禁重叠；
   - 正下方居中：`#meterGrid.scale` 包含 4 段黑白交替印刷色块（`#scaleBar`）；
   - 右下角：`#layerDock` 图层抽屉（`#layerCard`）默认折叠且带 `inert` 与 `aria-hidden="true"`，点击 `#layerToggle` 平滑展开。

---

## 二 · 后端真实状态对接与二进制尺度映射

1. **纯二进制 $2^3 = 8\text{m}$ 比例尺对齐**：
   - 后端世界网格步进元为 $\Delta x = 8\text{m}$，宏观父网格为 $256\text{m} = 2^8\text{m}$；
   - `#meterGrid` 比例尺根据当前缩放倍率动态对齐二进制友好的米制跨度，实时更新 `#scaleBar` 宽度与 `#scaleDistance` 读数。
2. **动态 APP-6 战术兵牌 (`#unitsLayer`)**：
   - 从 `gentactix-server`（`/api/state`）拉取 `own_pieces` 与 `enemy_contacts`；
   - 友军使用蓝色矩形框（`#b7cbd0` 填充、`#3a5969` 描边），敌方接触点使用红色菱形框（`#e8c7b3` 填充、`#984d42` 描边）；
   - 框内绘制兵种符号（步兵交叉线、装甲椭圆、炮兵圆点、侦察斜线、HQ 标识），框顶标注梯队（`I` / `II` / `III`）。
3. **左侧检视器 (`#inspector`) 实时绑定**：
   - 选中单位后，实时同步单位名称、番号、编制人数/上限、组织度、弹药、补给进度条、8m 军用网格坐标、海拔与当前命令意图；
   - 选中敌方接触点时，隐藏未知真实实力与火控开关，仅显示接触置信度与推定时间。
4. **真实火控与行动路线闭环**：
   - 点击 `#planButton` 调用或展示真实规划路径（`#routesLayer`），支持提交移动命令；
   - 点击 `#fireButton` 调用后端 DDA 视线与火控接口（`/api/los` 或 `/api/fire_arc`），在 `#fireLayer` 上真实绘制红线通视段（`#fireLive`）、深灰遮挡段（`#fireBlocked`）与散布椭圆（`#dispersion`）。

---

## 三 · 必跑 E2E 自动化验证流程

每次修改 `web/index.html` 后，严禁凭肉眼猜测，必须通过 `run_command` 运行无头浏览器自动化验收脚本：

```powershell
node "D:\tools\gentactix\tests\e2e\verify_production_ui.mjs"
```

验收要求：
- 控制台零报错（`errors.length === 0`）；
- 所有布局、比例尺、图层焦点、兵牌选中、火控展开与响应式断言全部输出 `PASS`。
