---
name: gentactix-cartography-ui
description: >-
  Gentactix 印刷地图战术指挥前端、全窗口红字 UI-ID 强制规范与无头浏览器像素级验真法典 (Gentactix Cartography UI & Red UI-ID Codex). ACTIVATE this skill whenever reading, modifying, or testing frontend code in web/index.html, art-direction/, or tests/e2e/.
---

# Gentactix 印刷地图战术指挥前端施工法典 (Cartography UI Codex)

本技能定义 `web/index.html` 的视觉美学、**全窗口红字 `data-ui-id` 强制挂载制度**、16s 双频计时器、四类回合内修改面板以及无头 Chrome 自动化像素验真标准。一切前端实现必须严格服从 [SPEC_PULSE_16S_AND_TRIPARTITE_STATE_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_PULSE_16S_AND_TRIPARTITE_STATE_CODEX.md) 第三章。

---

## 一 · 视觉与布局绝对红线 (SD2 / CMO Printed Map Aesthetics)

1. **色彩与材质变量锁死**：
   - 保持 `:root` 军用印刷色板：`--ink:#d6d9d4; --dim:#8f9b9b; --chrome:#242e32; --deep:#1b2428; --line:#3d484c; --paper:#ded8be; --blue:#436779; --red:#a94f43; --gold:#c5aa70; --green:#91a58c;`；
   - 保持 SVG `#paperGrain` 噪点滤镜、`#grid` 格网与 `#fieldHatch` / `#frontHatch` 印刷斜线填充。
2. **Workspace 双顶级容器铁律**：
   - `.workspace` 下**只允许存在两个直接子元素**：左侧 `#inspector.inspector`（深色检视面板）与右侧 `main.map-area`（内含 `#viewport.map-viewport`）；
   - **绝对禁止**引入旧版笨重的 `.sidebar`、`#rosterPane`、`#eventsPane` 或底部滚动日志条 `.map-log`；
   - 地图 `#viewport` 必须延伸至屏幕最底端（`bottom === innerHeight`）；桌面端（1600×900）下 `#inspector` 严禁出现垂直滚动条。

---

## 二 · 全窗口与子区块“红字 UI-ID”强制显示铁律 (Red UI-ID Overlay Rule)

> [!IMPORTANT]
> **【最高前端铁律】**：界面上**每一个窗口、顶栏、侧栏、抽屉、浮动面板、模态弹窗（Dialog）以及内部独立功能子区块**，都必须显式挂载唯一的 `data-ui-id="UI-XX"` 属性，并通过 CSS `::after` 伪元素在容器角部渲染可见的红色小字编号（`font: 8px~9px var(--mono); color: #b34135 / #ee9286`），以便大王直接点名任意窗口修改！

### 法定 `data-ui-id` 编号表（严禁遗漏或私自乱编）
- **既有 19 个核心基准锚点**：
  - `UI-01`：`header.topbar`（顶部导航栏）
  - `UI-02`：`div.subbar`（次级战役与计时器控制栏）
  - `UI-03`：`aside#inspector`（左侧单位检视主窗口）
  - `UI-03A`：`.inspector-top`（单位头部标识区）
  - `UI-03B`：`.inspector-section`（作战实力与三维进度条区）
  - `UI-03C`：`.inspector-section`（位置与 8m 军用格网区）
  - `UI-03D`：`.inspector-section`（当前命令意图卡片区）
  - `UI-03E`：`.inspector-actions`（检视器底部主操作按钮区）
  - `UI-04`：`div#mapTitle`（地图左上角战区标题折页）
  - `UI-05`：`section#layerCard`（地图右下角图层控制抽屉）
  - `UI-05A`：`button#layerToggle`（图层抽屉开关按钮）
  - `UI-06`：`div.map-tools`（地图右上侧缩放与复位工具条）
  - `UI-07`：`div#meterGrid.scale`（地图正下方居中四段印刷比例尺）
  - `UI-08`：`footer.footer`（地图左下角状态灯与光标格网栏）
  - `UI-09`：`div.compass-wrap`（地图右上侧指北针）
  - `UI-10`：`dialog#designDialog`（美术设计说明弹窗）
  - `UI-11`：`div#toast`（底部居中悬浮通知条）
  - `UI-12`：`div.map-stamp`（地图右上角“作战用图”印章）
  - `UI-13`：`div.map-notice`（地图左下侧操作提示语）
- **新增业务窗口与四类修改面板法定编号**：
  - `UI-14`：`aside#orbatDrawer`（战术编制母树与整编主抽屉）
  - `UI-14A`：`div#orbatTreeList`（编制层级树列表子区）
  - `UI-14B`：`div#reorgEquipPanel`（【第三类修改】非队列直接调整窗口：拆分/整编、调整士兵装备、使用物品、架设/收拢重武器）
  - `UI-15`：`section#actionQueuePanel`（【第一类修改】计划行动任务队列窗口：移动、射击、隐蔽、停止、撤退）
  - `UI-16`：`section#eventQueuePanel`（【第二类修改】计划事件读秒队列窗口：布雷、挖坑、建造、爆破、拆除、无线电、测距）
  - `UI-17`：`section#commandFieldPanel`（【第四类修改】指挥安排与场绘制窗口：绘制指挥 AO、进攻线、移动线、部署线）
  - `UI-18`：`div#pulseStagingBar`（16s 步长读秒 `00/16s` 与本步暂存操作概览区）

---

## 三 · 16s 双频计时器与后端真实状态对接

1. **1s 走表 + 16s 脉冲结算 + $2^n$ 倍速**：
   - 初始状态 `running = false`（暂停）；
   - `#speed` 倍速按钮严格按 `1× -> 2× -> 4× -> 8× -> 16×` 循环切换；
   - 运行态下，每秒更新 `#clock` 与 `UI-18` 步长读秒；每满 16s 逻辑秒调用一次 `POST /api/commit_turn`（传 `{ "subticks": 1 }`），结算并同步最新权威状态。
2. **必跑 E2E 自动化验证流程**：
   ```powershell
   node "D:\tools\gentactix\tests\e2e\verify_production_ui.mjs"
   ```
   确保控制台零报错，所有 `[data-ui-id]` 红字伪元素可见且唯一，布局与交互断言 100% `PASS`。
