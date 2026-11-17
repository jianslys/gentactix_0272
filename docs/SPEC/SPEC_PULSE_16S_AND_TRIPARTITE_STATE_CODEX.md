# GENTACTIX 核心规格书：三态边界、16s 脉冲时序与前端红字 UI-ID 规范

- **文档编号**：`GTX-SPEC-PULSE16S-TRIPARTITE-2.0`
- **基准对齐**：承接 `SPEC_FINAL_GENTACTIX_CORE_ENGINE.md`（纯二进制空间基准 $\Delta x = 2^3\text{ m} = 8.0\text{ m}$ 与时间步长 $\Delta t = 2^4\text{ s} = 16.0\text{ s}$）
- **状态**：正式定稿

---

## 第一章 · 世界状态三分法与四类回合内修改的底层归属

全引擎状态严格隔离为三大物理容器：**`WorldState`**（权威持久真相）、**`DerivedState`**（派生缓存与 1~15s 规划暂存层）、**`TickScratch`**（第 16s 脉冲结算瞬间的 CPU 双缓冲草稿纸）。

### 1.1 `WorldState`（权威持久状态 —— 16s 步长边界的唯一真相源）
仅存储跨 $16\text{ s}$ 步长持久化的最小完备种子数据，是唯一参与 `state_fingerprint`（SHA-256）、存档序列化与回放的数据源：
1. **纯二进制整数权威时钟 (`ClockState`)**：
   - **彻底废除 `clock_hours: f32` 浮点累加**（消除 $16 / 3600 = 1/225$ 二进制无限循环小数导致的跳秒漂移）；
   - 权威字段仅含无损整数：`tick: u64`（步长计数）、`epoch_seconds: u64`（起始基准秒，如 `06:30:00` 对应 `23400`）、`clock_seconds: u64 = epoch_seconds + (tick << 4)`（每步长精确 `+ 16`）。
2. **地形与工事权威账本 (`TerrainState`)**：
   - $\Delta x = 2^3\text{ m} = 8.0\text{ m}$ 网格的高程 `elevation_m`、基础地貌 `terrain_type`、已竣工工事/雷区/爆破标识 `feature_flags`，及单调递增的 `terrain_revision: u64`。
3. **拉格朗日实体与编制物资账本 (`EntityState` & `InventoryLedger`)**：
   - 基础实例（步兵班组、指挥班、重武器班组）的权威坐标 `px`/`pos`、朝向 `facing_deg`、四本人员分类账（军官/士官/士兵/伤员）、组织度 `org`、弹药/口粮/油料库存、单兵装备与重武器架设状态。
4. **跨步长持久物理场 (`PersistentFieldState`)**：
   - 具备物理半衰期、跨步长演化的环境残留（烟雾浓度与剩余秒数 `smoke_ttl_sec`、照明剩余秒数 `illum_ttl_sec`、炮击标定残余修正槽）。

---

### 1.2 `DerivedState`（派生缓存层 —— 四类回合内修改与欧拉连续场的统一载体）
带版本依赖元数据（`CacheMeta`），不进存档，随时可由 `WorldState` 叠加玩家当前步长输入无损重建。**玩家在 1~15s 走表期内进行的四类修改及其几何/场计算产物，统一归口于 `DerivedState`**：

1. **第一类：计划行动缓存与任务队列 (`PlannedActionCache`)**
   - 承载基础实例（班组、指挥班、重武器班组）排入任务队列的 **移动、射击、隐蔽、停止、撤退** 行动；
   - 同步派生缓存：寻路器算出的路径点集（`waypoints`）、分段距离与预计耗时（`ETA`）、共享 DDA 视线扫描算出的通视段（`#fireLive`）、受阻段（`#fireBlocked`）及散布椭圆。
2. **第二类：计划事件缓存与事件队列 (`PlannedEventCache`)**
   - 承载排入事件队列的 **布雷、挖坑（工事）、建造、爆破、拆除、无线电（静默/改频/诱饵）、测距（试射标定）** 等带读秒周期的事件及其剩余秒数预览。
3. **第三类：非队列直接调整缓存 (`ImmediateStagingCache`)**
   - 承载不走行动队列、亦不走事件队列的直接操作：**拆分/整编等调整编制、调整士兵装备、使用物品、使用/架设/收拢重武器**；
   - 在 1~15s 走表期内作为可随时修改或撤销的派生预览态；一旦到达第 16s 脉冲结算点，在移动与交火开始前的 **第一步（Stage 1）优先原子落盘**进 `WorldState`。
4. **第四类：指挥安排与欧拉连续场缓存 (`CommandFieldCache`)**
   - 承载指挥官在地图上绘制的 **指挥 AO（作战责任区）、进攻线、移动线、部署线** 矢量数据；
   - 承载由这些线条与地形光栅化、扩散生成的 **欧拉连续场（任务引力场 $G$、信息斥力与威胁场 $I$、导航通行代价场、SDF 符号距离场）**。版本号未变时跨步长零开销复用。

---

### 1.3 `TickScratch`（第 16 秒脉冲结算瞬间的 CPU 双缓冲草稿纸）
仅在第 16 秒脉冲触发、执行单步原子结算的那几毫秒内有效，实现**“读旧权威状态 + 读派生场 $\rightarrow$ 写草稿纸提案 $\rightarrow$ 冲突仲裁与场归约后唯一写回权威状态”**的双缓冲隔离：
1. **`intent_candidates`**：各单位在 Stage 4 裁决出的本步唯一动作意图；
2. **`motion_proposals`**：各单位在 Stage 5 由基础速度、欧拉连续场（$I, G$）与拉格朗日粒子耦合算出的 16s 位移提案（Stage 7 解决冲突前严禁修改权威坐标）；
3. **`grid_fire_scratch`（$2^3=8\text{m}$ 网格杀伤寄存桶）**：存放本步长所有开火单位打入目标 $8\text{m}$ 网格的 `ShotPacket` 及展开后的瞬态破片通量（`Sum` 代数）、冲击波超压（`Max` 代数）与压制密度；
4. **`completed_event_mutations`**：本步长读秒归零的计划事件产出的地形改造与属性变更提案；
5. **零分配复用铁律**：每步长结束（Stage 12）立即调用 `.clear()` 清空元素但保留底层 `Vec`/`Map` 容量，严禁跨步长残留。

---

## 第二章 · 16s 脉冲计时器时序与 $2^3=8\text{m}$ 网格寄存场结算

### 2.1 后端 16s 步长结算的 6 大物理因果步骤（对应底层 13 阶段算子）

当第 16 秒脉冲到达，后端单向执行以下 6 大物理步骤：

1. **第一步：封盘跳表 & 非队列整编先行（Stage 0 `BeginTick` ~ Stage 1 `ClockAndSchedule`）**
   - 清空 `TickScratch`，整数推进 `tick += 1`, `clock_seconds += 16`；
   - **最先落盘【第三类修改：非队列直接调整】**（拆分/整编编制、调整士兵装备、使用物品、架设/收拢重武器），确保本步后续移动与交火直接采用新编制与新武器。
2. **第二步：环境衰减 & 指挥线刷出欧拉场（Stage 2 `PersistentFieldEvolve` ~ Stage 3 `DerivedRefresh`）**
   - 烟雾、照明、标定热度按 $\Delta t = 16\text{ s}$ 衰减；
   - **消化【第四类修改：指挥安排】**：若 AO、进攻线、移动线、部署线或地形 `revision` 有更新，重算任务引力场 $G$、信息斥力场 $I$、导航场与 SDF 存入 `DerivedState`。
3. **第三步：班组意图仲裁（Stage 4 `SenseAndArbitrate`）**
   - 按五级优先级（硬失效 > 溃退 > **【第一类修改：计划行动队列】直接命令** > 欧拉场 $I,G$ 自治跟进 > 原地保持）裁决各单位本步意图写入 `TickScratch`。
4. **第四步：全图算腿脚 —— 移动双缓冲落盘（Stage 5 `ProposeMotion` ~ Stage 7 `CommitMotion`）**
   - 执行计划行动中的移动/撤退：基础速度与欧拉连续场、拉格朗日粒子耦合产出位移提案写草稿纸（Stage 5）$\rightarrow$ 防穿墙与 $8\text{m}$ 格容量冲突消解（Stage 6）$\rightarrow$ **唯一合法位置写点**更新 `WorldState` 坐标（Stage 7）。
5. **第五步：$8\text{m}$ 网格开火寄存与计划事件读秒落盘（Stage 8 `EmitEvents` ~ Stage 10 `CommitPersistent`）**
   - **推进【第二类修改：计划事件队列】**：布雷、挖坑、建造、爆破、拆除、无线电、测距读秒减 $16\text{ s}$，到期事件产出变异提案；
   - **执行 $2^3=8\text{m}$ 网格开火寄存场结算（彻底废除开火函数直接遍历单位扣血）**：
     - **第一拍 P2G（Stage 8 射弹注入网格）**：射手扣减弹药，通过 DDA 射线将 `ShotPacket`（弹种、整数发数、入射角 $\theta$、破片半径、优先目标标签）打入目标 $8\text{m}$ 网格寄存桶 `TickScratch.grid_fire_scratch`；
     - **第二拍 Grid-to-Field（Stage 9A 网格展开为场）**：活跃 $8\text{m}$ 网格将寄存包展开为瞬态破片通量（`Sum`）与超压（`Max`），同时将弹着压制密度 `suppression_density` 与火力威胁写回 `DerivedState` 欧拉威胁场（供后续机动规避采样）；
     - **第三拍 G2P（Stage 9B~10 网格向驻留单位结算）**：仅取出受击 $8\text{m}$ 网格内的驻留单位，结合该格地形工事掩体、入射角夹角余弦 $\cos(\theta_i)$ 与步坦遮蔽算出伤亡、组织度损失与弹坑变异，在 **Stage 10 唯一合法写点** 统一写回 `WorldState`。
6. **第六步：守恒验账、撕草稿与快照发布（Stage 11 `Validate` ~ Stage 12 `PublishAndDrop`）**
   - 校验人员/物资非负守恒与指挥官接替，`TickScratch.clear()` 归零，输出战争迷雾过滤快照与 `state_fingerprint`。

---

## 第三章 · 前端实现规范、美术基准与全窗口红字 UI-ID 强制契约

前端（`web/index.html`）的开发与扩展必须 100% 服从大当家审定的印刷地图美术概念（`art-direction/command-map-concept.html`），并严格执行**全窗口红字 `data-ui-id` 标识制度**。

### 3.1 印刷地图美术与布局铁律 (SD2 / CMO Cartography Standard)
1. **色彩与材质锁死**：
   - 严格使用 `:root` 军用印刷色板：`--ink:#d6d9d4; --dim:#8f9b9b; --chrome:#242e32; --deep:#1b2428; --line:#3d484c; --paper:#ded8be; --blue:#436779; --red:#a94f43; --gold:#c5aa70; --green:#91a58c;`；
   - 保持 SVG `#paperGrain` 纸张肌理噪点、`#grid` 格网、`#fieldHatch` 与 `#frontHatch` 印刷斜线、APP-6 风格战术兵牌（友方蓝矩形、敌方红菱形）。
2. **Workspace 纯净结构与零滚动条铁律**：
   - `.workspace` 网格下仅允许存在左侧 `#inspector.inspector` 与右侧 `main.map-area`（内含全屏 `#viewport`）两个主布局块；
   - 严禁出现旧版丑陋侧边栏或底部滚动日志条，地图 `#viewport` 必须直抵屏幕底边（`bottom === innerHeight`）；
   - 桌面端（1600×900）下，`#inspector` 的默认主视图内容必须一屏完整呈现，严禁出现垂直滚动条。

---

### 3.2 全窗口与子区块“红字 UI-ID”强制显示规范 (Red UI-ID Overlay Rule)

> [!IMPORTANT]
> **【红字 ID 铁律】**：为了便于大当家在审阅界面时随时精准点名任意窗口或区域进行修改，**界面上每一个独立窗口、顶栏、侧边栏、抽屉、浮动卡片、模态弹窗（Dialog）以及面板内部的每一个功能子区块，都必须挂载唯一的 `data-ui-id="UI-XX"` 属性，并在界面上直接渲染出可见的红色小字编号！**

1. **CSS 伪元素渲染基准（严禁隐藏或删除）**：
   ```css
   [data-ui-id] { position: relative; }
   [data-ui-id]::after {
     content: attr(data-ui-id);
     position: absolute;
     left: 7px;
     top: 5px;
     font: 9px/1 var(--mono);
     letter-spacing: .3px;
     color: #b34135; /* 浅底色区域使用深红 #b34135，深色面板内部使用亮红 #ee9286 */
     pointer-events: none;
     z-index: 12;
     white-space: nowrap;
   }
   .inspector [data-ui-id]::after,
   .inspector::after,
   .orbat-drawer [data-ui-id]::after,
   .orbat-drawer::after,
   .cmd-drawer [data-ui-id]::after,
   .cmd-drawer::after,
   .toast::after,
   dialog::after {
     color: #ee9286;
   }
   ```
2. **可见性硬约束**：
   - 任何带有 `[data-ui-id]` 的容器，必须确保其 `position` 为 `relative` / `absolute` / `fixed`，且 `::after` 伪元素不得被子元素遮挡或因 `overflow: hidden` 裁切在视野外；
   - 字体大小严格控制在 `8px ~ 9px`，颜色必须满足红色通道显著高于绿蓝通道（`R > G && R > B`）。

---

### 3.3 全量窗口与子区块 `data-ui-id` 法定编码表

前端所有存量面板与围绕“四类回合内修改 + 16s 计时器”新增的功能窗口，必须严格按以下编号表挂载 `data-ui-id`，严禁漏挂或私自乱编：

#### A. 既有 19 个基准窗口与区块（必须 100% 保持）
| `data-ui-id` | 对应 DOM 选择器 / 容器 | 窗口/区块说明 |
| :--- | :--- | :--- |
| **`UI-01`** | `header.topbar` | 顶部主导航栏（品牌、三大视图切换 Tab、现实时钟 `#clock`、设计说明按钮） |
| **`UI-02`** | `div.subbar` | 次级战役与计时器控制栏（行动名、步长读数、天气、部队/编制按钮、`#play` 暂停/开始、`#speed` 倍速） |
| **`UI-03`** | `aside#inspector` | 左侧选中单位检视器主窗口 |
| **`UI-03A`** | `.inspector-top` | 检视器头部：单位 APP-6 大图标、名称、番号与通信状态标签 |
| **`UI-03B`** | `.inspector-section` (作战实力) | 检视器实力区：实有人数/上限、组织度、弹药储备、补给水平进度条 |
| **`UI-03C`** | `.inspector-section` (位置与环境) | 检视器位置区：8m 军用格网、所在地域、海拔、态势可信度 |
| **`UI-03D`** | `.inspector-section` (当前意图) | 检视器意图区：当前命令意图卡片（`.order-card`）、距离与预计耗时 |
| **`UI-03E`** | `.inspector-actions` | 检视器底部主操作按钮区（查看行动路线、火控与视线分析等入口） |
| **`UI-04`** | `div#mapTitle` | 地图左上角可折叠战区印刷标题栏 |
| **`UI-05`** | `section#layerCard` | 地图右下角图层开关浮动面板（地形、地名、接触线、命令、火控） |
| **`UI-05A`** | `button#layerToggle` | 地图右下角图层抽屉展开/收起按钮 |
| **`UI-06`** | `div.map-tools` | 地图右上侧缩放（`+`/`−`）、复位与缩放百分比工具条 |
| **`UI-07`** | `div#meterGrid.scale` | 地图正下方居中四段黑白交替印刷比例尺 |
| **`UI-08`** | `footer.footer` | 地图左下角连接状态灯与鼠标实时军用格网读数栏 |
| **`UI-09`** | `div.compass-wrap` | 地图右上侧印刷指北针 |
| **`UI-10`** | `dialog#designDialog` | 美术设计说明模态弹窗 |
| **`UI-11`** | `div#toast` | 屏幕底部居中悬浮操作通知条 |
| **`UI-12`** | `div.map-stamp` | 地图右上角竖排“作战用图”印刷印章 |
| **`UI-13`** | `div.map-notice` | 地图左下侧交互操作提示语 |

#### B. 新增业务窗口与四类回合内修改面板编号（强制补齐与挂载）
| `data-ui-id` | 对应 DOM 容器 | 关联的 `DerivedState` 四类修改与业务功能 |
| :--- | :--- | :--- |
| **`UI-14`** | `aside#orbatDrawer` | **战术编制母树与整编主抽屉**（修复现有 `web/index.html` 漏挂 ID 问题） |
| **`UI-14A`** | `div#orbatTreeList` | 编制层级树列表区（展示团-营-连-排-班指挥链与实力，支持点击定位） |
| **`UI-14B`** | `div#reorgEquipPanel` | **【第三类修改 · 非队列直接调整窗口】**：拆分/整编调整编制、调整士兵装备、使用物品、架设/收拢重武器 |
| **`UI-15`** | `section#actionQueuePanel` | **【第一类修改 · 计划行动任务队列窗口】**：为单位排入移动、射击、隐蔽、停止、撤退任务队列，设置行军/开火模式 |
| **`UI-16`** | `section#eventQueuePanel` | **【第二类修改 · 计划事件队列窗口】**：安排布雷、挖坑、建造、爆破、拆除、无线电（静默/改频/诱饵）、测距标定读秒事件 |
| **`UI-17`** | `section#commandFieldPanel` | **【第四类修改 · 指挥安排与场绘制窗口】**：在地图上绘制指挥 AO、进攻线、移动线、部署线并预览欧拉场 |
| **`UI-18`** | `div#pulseStagingBar` | **16s 步长读秒与本步暂存修改概览区**：显示当前步长 `00/16s` 读秒进度及本步已暂存待结算的操作计数 |

> **抽屉与子窗口布局铁律**：`UI-14` ~ `UI-17` 均采用与左侧检视器深色铬底（`--chrome: #242e32` / `#202a2ef7`）一致的滑出式侧边抽屉或内嵌切换面板（`position: fixed; top: 101px; left: 286px; bottom: 0` 或模态框），按需展开、互斥关闭（按 `Escape` 键或 `×` 关闭），**绝对不得挤压 `.workspace` 的网格尺寸，亦不得导致 `#inspector` 产生垂直滚动条**。

---

### 3.4 前端 16s 双频计时器与 API 对接约束
1. **初始暂停与 $2^n$ 倍速控制**：
   - 页面初始化时 `running = false`，`#play` 按钮显示播放图标与 `开始`（或 `预演`）；
   - `#speed` 按钮点击在 **`1×` $\rightarrow$ `2×` $\rightarrow$ `4×` $\rightarrow$ `8×` $\rightarrow$ `16×`** 之间循环切换；
   - 废除原先每 2.5 秒无脑 `setInterval(fetchBackendState, 2500)` 冲掉玩家本地未提交规划的轮询逻辑。
2. **1~15s 走表与第 16s 脉冲提交闭环**：
   - 当 `running === true` 时，前端定时器按当前倍速推进 `clockSeconds`（每走 1 逻辑秒更新一次 `#clock` 与 `UI-18` 的 `01/16s ~ 15/16s` 读数）；
   - 在 1~15s 期间，玩家在 `UI-14B`、`UI-15`、`UI-16`、`UI-17` 中的操作存入前端/后端的 `DerivedState` 暂存队列，并立即调用 `/api/weapon_los`、`/api/estimate_plan_time` 在地图上渲染路线与火控预览；
   - 每当逻辑秒走满 **16 秒**（`stepElapsed === 16`），前端自动调用 `POST /api/commit_turn`（携带 `{ "subticks": 1 }` 及本步暂存指令），后端执行 6 大步（13 阶段）原子结算并返回最新权威 `state`，前端用最新权威数据同步刷新地图兵牌（`#unitsLayer`）、左侧检视器（`UI-03`）与状态栏。

---

## 第四章 · 全栈物理铲除与自动化验收清单 (Verification Checklist)

1. **后端 (`crates/gentactix-core` & `crates/gentactix-server`) 验收项**：
   - [ ] `ClockState` / `SimWorld` 彻底改用整数 `subtick: u64` 与 `clock_seconds: u64`（每次 `+ 16`），铲除 `clock_hours += dt` 浮点累加；
   - [ ] `/api/commit_turn` 支持整数 `"subticks": 1` 单步 16s 原子推进；
   - [ ] `combat_logistics.rs` 彻底删除绕过网格直接遍历 `units` 扣血的旧循环，100% 走 $2^3=8\text{m}$ 网格寄存（P2G $\rightarrow$ Field $\rightarrow$ G2P）；
   - [ ] `cargo test -p gentactix-core` 与 `cargo test -p gentactix-server` 100% PASS。
2. **前端 (`web/index.html` & `tests/e2e/verify_production_ui.mjs`) 验收项**：
   - [ ] 原有 19 个 `data-ui-id`（`UI-01` ~ `UI-13`）及所有新增窗口/子块（`UI-14` ~ `UI-18`）全部挂载唯一红字 ID，且 `getComputedStyle(el, "::after")` 校验红字可见、字号 $\le 9\text{px}$、无重叠；
   - [ ] `#speed` 倍速按钮严格按 `1× / 2× / 4× / 8× / 16×` 切换，每走满 16s 逻辑秒触发一次后端步长结算；
   - [ ] 桌面端 1600×900 下 `#inspector` 无垂直滚动条，地图 `#viewport` 延伸至屏幕底端，比例尺居中四段黑白交替；
   - [ ] 无头浏览器运行 `node tests/e2e/verify_production_ui.mjs` 零报错、全项 `PASS`。
