# GENTACTIX V6.0 终局合龙总规格书：V5 六大审计缺陷清零与外置伪装异步时间机全栈实装

- **文档编号**：`GTX-SPEC-V6-TIMEMACHINE-CLOSURE`
- **前置基线**：整合 [SPEC_V5_MATH_AND_FULL_CLOSURE_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_V5_MATH_AND_FULL_CLOSURE_CODEX.md) 与 [SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md)
- **核心铁律**：
  1. **零懒计算 · 完美开发**：全盘拉格朗日粒子 100% 真实物理运算，二进制空间基准 `Δx = 2³ m = 8.0 m`，宏步长 `Δt = 2⁴ s = 16.0 s`；
  2. **核心业务零污染**：`16s` 步长结算引擎在回合交界处一次性算完本步长内所有位移、命中、原生物理伤害 `exact_hp_loss`、压制 `exact_supp_add`、工兵进度与 $G$ 场变更，**绝不因“伪装异步”篡改任何核心物理/伤害公式**；
  3. **外置时间机一肩挑**：只设唯一外置 **“时间机（Time Machine）”**——对内拦住 `#0` 当回合一切改状态命令、按准备时间拖入 `#1 / #2 / #3` 回合，并在回合交界把落入 `[1s..16s]` 窗口的那一截任务数据切出来喂给步长结算引擎；对外带着 `1s 到 16s` 时钟与五张分品类时间哈希表，直接驱动拉格朗日粒子的只读线性插值渲染、$I$ 场外围 `visibility=0` 预渲染环显隐切换、以及 `exact_ms` 定点爆破放片。

---

## 第零章 · V5 交付物（`38d95af`）六大实锤缺陷清算令（必须 100% 根除）

经源码逐行审计与 `scratch/production_desktop_screenshot.png` 实机截图核查，V5 存在以下六项必须彻底修复的弄虚作假与断路缺陷：

1. **【缺陷 1 · 算子孤岛复发】`sim_loop.rs` 仅改了 8 行，四大数学算子与增广弧长完全未接入主循环**：
   - `fields/mod.rs` 中的 `run_deterministic_multi_capacity_auction`（最小堆拍卖）、`SchmidtPostureState`（三态压制）、`project_orthogonal_cover`、`wendland_c2_kernel`、`DeygoutRadioOperator`、`UnitZohDirectiveFilter`（128B 稀疏 ZOH），以及 `geometry/tactical_instance.rs` 中的 `AugmentedArcIntegrator`（16s 增广弧长边走边干）和 `DetonatorSwitchEntity`，在 `sim_loop.rs::step_subtick_16s` 中调用次数为 **0**！
   - **整改死令**：必须在 `sim_loop.rs::step_subtick_16s` 的每一拍中真实调用上述算子驱动单位分配、移动流线、双时间尺度压制坍塌、电台 ZOH 重寻思与工兵逐点施工转正写入 `WorldGrid`。

2. **【缺陷 2 · 实机截图抓获 4 处视觉叠字与残留假文案】**：
   - **`UI-03A` 与 `×` 关闭钮及编号叠死**：`.inspector-top::after`（`right: 18px; top: 8px`）与 `.card-close-btn`（`right: 10px; top: 8px`）及 `#inspectorId` 三个元素在卡片右上角叠成一团；**整改**：将 `UI-03A` 移至左上或将关闭钮与编号错开行距（预留至少 `24px` 净空），确保任意两个文本/按钮 AABB 零交叠；
   - **`UI-11`（`#toast`）与 `UI-04` 在地图左上角 100% 重叠**：`.toast` 缺少 `position: absolute; bottom: 52px; left: 50%; transform: translateX(-50%)` 定位，导致空置时落入 `(0, 0)` 压死 `UI-04`；且初始 `#inspector` 放在 `left: 20px; top: 14px` 直接盖住底下 `UI-04`（“白桦河战区”）；**整改**：给 `.toast` 补齐底部居中浮动样式，并将初始单位卡片移至不遮挡 `UI-04` 标题的位置（如 `left: 20px; top: 110px` 或右侧）；
   - **残留静态假数字 `"ROUTE / 1.8km"` 与 `"1.8 km ~ 24 min"`**：`web/index.html` 第 218 行写死 `<text>ROUTE / 1.8km</text>`，第 187/634/845 行写死 `1.8 km` 与 `~ 24 min`；**整改**：彻底删除写死文本，全部绑定真实坐标距离与速度实时计算值；
   - **`UI-09` 指北针与 `UI-05A` 割裂**：必须将指北针、`UI-09A` 工具按钮与 `UI-05A` 图层开关统一收纳进右上角同一个深色铬合金面板容器内。

3. **【缺陷 3 · 点击兵牌假多开，E2E 靠调全局函数作弊】**：
   - `web/index.html` 第 573 行点击地图兵牌只调 `selectUnit(u.id, true)` 覆写单张 `#inspector`，根本没调 `openFloatingUnitCard(u.id)`；`verify_production_ui.mjs` 却是靠直接运行 `window.openFloatingUnitCard('u1')` 作弊混过检查！
   - **整改死令**：鼠标左键点击地图上任意兵牌，必须真实调用 `openFloatingUnitCard(u.id)` 弹出/置顶该单位专属的独立浮动 `UI-03` 卡片（不同单位弹出不同卡片，自动阶梯错开初始坐标 `(offsetX, offsetY)`）；E2E 严禁直接调 `window.openFloatingUnitCard`，必须通过真实点击两个不同 `.map-unit` DOM 节点来验证 `count >= 2`！

4. **【缺陷 4 · 地图选点形同虚设，前端仍在硬塞假坐标偏移】**：
   - `web/index.html` 中 `pendingMapGoal` 从未被地图点击赋值，导致 `commitTaskToQueue` 永远走假偏移 `(startX + 72, startY - 48)`；`commitEventToQueue` 爆破永远走假偏移 `(+40, -20)` 与 `(-60, +40)`；`UI-14B` 仍在发 `op: "btnSplitSquad"` 导致后端返回 `400 unknown_op`；`UI-03M3` 点确认只写本地布尔值不发多边形/折线坐标！
   - **整改死令**：
     * `UI-03M1`（除 `撤退->直接跑路` 外）、`UI-03M2`、`UI-03M3` 必须支持**在地图上真实点击/绘制目标点或多边形**（左键在 `#mapSvg` 选点设置 `pendingMapGoal` 或绘制多边形顶点，爆破模式第 1 次画爆破点、第 2 次点撤离开关位置）；
     * `UI-14B` 按钮必须映射为后端合法 `op`：`"split_squad"`、`"merge_squads"`、`"equip_adjust"`、`"deploy_heavy_unit"`、`"pack_heavy_unit"`，并携带合法 `piece_id`。

5. **【缺陷 5 · 前后端单位脱节（副栏显示 `0友军 / 1接触点`）】**：
   - `gentactix-server --demo` 默认未设置默认己方指挥官或 `serialize_state` 默认过滤导致 `/api/state` 返回 `own_pieces: []`（0 友军），前端被迫回退到写死的 `u1, u2, u3` 假数组，发往后端的 `u3` 全部 `piece_not_found`！
   - **整改死令**：`serialize_state` 在未传 `commander_id` 及 `team` 参数时，默认以红方（`TeamId::Red`）最高指挥官视角导出全部红方 `own_pieces`（带完整 `inventory`、`is_direct_subordinate`、`pos`）及经 $I_{\text{intel}}$ 过滤的蓝方 `visible_enemies`（加预渲染缓冲环 `prerender_skirt_enemies`）；前端初始化拉取 `/api/state` 后 100% 使用后端真实单位 ID（如 `red_inf_1` 等）渲染兵牌与下发指令，副栏必须显示真实非零友军数！

6. **【缺陷 6 · E2E 物理测试偷梁换柱（只测 `tick1 > tick0` 没测位移）】**：
   - `verify_production_ui.mjs` 第 342 行只检查了 `tick1 > tick0`，根本没验证单位坐标是否发生真实位移！
   - **整改死令**：E2E 必须对比推进前后的单位真实坐标 `p0` 与 `p1`，强制断言 `Math.hypot(p1[0] - p0[0], p1[1] - p0[1]) > 0.01`（真实物理位移 `Δpos > 0`）！

---

## 第一章 · 外置伪装异步时间机全栈实装规范 (Time Machine & Pre-Render Skirt)

严格将 [SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md) 全量落地为连接前端与 `16s` 步长引擎的唯一外置 **时间机（Time Machine）**：

### 1.1 时间机对内：当回合拦单、跨回合拖延与窗口切段喂数
1. **严格 `1..=16` 回合秒刻度**：单一全局回合内部只定义 **`1s` 到 `16s`**（共 `16` 个整秒拍，无 `0s`）；全局回合记为 `#0, #1, #2, ...`。
2. **`#0` 当回合全类别拦单冻结**：
   - 玩家在 `#0` 回合走表途中（第 $t_{\text{cmd}} \in \{1, \dots, 16\}$ 秒）下达的一切改状态操作（**射击、移动、撤退、九大工程事件与爆破开关引爆、指挥指令与 AO 写 $G_0$、编制整编与电台调谐**），一律进入时间机的 `TimeMachinePendingQueue` 挂起；
   - **`#0` 回合内绝对不送入步长内核结算、不改动任何欧拉场或拉格朗日粒子权威数值，也绝不在 `#0` 地图上垫播任何假演出**。
3. **跨回合准备时间拖延与按 `[1s..16s]` 窗口切段喂数**：
   - 时间机为被拦截命令赋予跨回合准备时间：
     $$T_{\text{prep}} = (16 - t_{\text{cmd}}) + 16 \cdot k_{\text{turn\_skip}} + \tau_{\text{phase}}$$
     先把 `#0` 剩余 $(16 - t_{\text{cmd}})$ 秒耗尽，再跨过 $k_{\text{turn\_skip}} \in \{0, 1, 2\}$ 个整回合、落入未来回合的第 $\tau_{\text{phase}} \in \{1, \dots, 16\}$ 秒；
   - **任务无须限制为 `16s` 的正整数倍**（仅设最小下限如 `16s`，总长可为任意秒数）：在每个回合交界处，**时间机只需把该任务落入当前 `#k` 回合 `[1s..16s]` 窗口内的那一截有效数据（本回合占用秒数、触发发数/作业量）切出来吐给 `16s` 步长结算引擎**去结算即可。

### 1.2 双哈希混淆 + 五张分品类时间查找表（只算时间，不碰伤害数值）
1. **极简双哈希混淆公式**：
   拿**那一秒的全局游戏时间刻**（$t_{\text{abs}} = \text{tick} \times 16 + \tau$）的 `SplitMix64` 哈希，与**单位名字常量字符串**的编译期 `FNV1a_64` 哈希混出唯一 64 位整数 $M$：
   $$H_{\text{sec}} = \text{SplitMix64}(t_{\text{abs}}), \quad H_{\text{unit}} = \text{FNV1a\_64}(\text{unit\_name\_const}), \quad M = \text{mix64}\big(H_{\text{sec}} \oplus \text{rotl}(H_{\text{unit}}, 27)\big)$$
2. **五张 16 槽常量时间表（Per-Category Time LUTs）**：
   - `LUT_BULLET`（子弹/轻武器时间表）
   - `LUT_SHELL`（炮弹/重武器时间表）
   - `LUT_COMMAND`（指挥指令与 AO 写 $G_0$ 时间表）
   - `LUT_EVENT`（九大工程与爆破时间表）
   - `LUT_MOVEMENT`（移动与撤退时间表）
   **铁律**：查表只产出激活秒拍 $\tau_{\text{phase}}$ 与唯一毫秒触发戳 `exact_ms`，**绝对禁止修改或二次切分步长引擎算出的原生物理伤害 `exact_hp_loss` 与压制 `exact_supp_add`**！

### 1.3 时间机对外：只读粒子插值、$I$ 场外围 `visibility=0` 预渲染环与 `exact_ms` 放片
1. **$I$ 场可知范围外围的 `visibility=0` 预渲染缓冲环（Pre-Render Skirt）**：
   - 在当前我方 $I_{\text{intel}}$ 可见区域外围，扩出一圈**“单回合 16s 最大机动半径”的预渲染缓冲环**；
   - 落入该缓冲环内的敌军拉格朗日粒子，在回合交界时连同其本回合轨迹节点一并随 `TimeMachineFramePayload` 下发并预挂载进前端 SVG/DOM，初始强制设为 **`visibility = 0`（隐身且 `pointer-events: none` 不可交互）**；
   - 在 `0ms 到 16000ms` 走表插值期间，隐身粒子顺着线性插值轨迹 $\mathbf{x}(t_{\text{ms}})$ 滑行；**一旦其当前插值坐标 $\mathbf{x}(t_{\text{ms}})$ 滑入我方 $I_{\text{intel}}$ 可见格，前端时间机当场将其 `visibility` 从 `0` 切换为 `1`（滑出至迷雾格则从 `1` 切回 `0`）**。
2. **算演合一 `exact_ms` 定点触发（零脱靶）**：
   - 时间机为步长引擎算出的每条既定事件打上 `(0ms, 16000ms]` 内的唯一 `exact_ms`；
   - 当走表时钟扫到 `exact_ms` 时，直接在受击粒子 **`exact_ms` 那一瞬间的线性插值坐标 $\mathbf{x}(\text{exact\_ms})$** 上播放爆炸/命中特效，并在同一毫秒把原装 `exact_hp_loss` / `exact_supp_add` / 工事转正刷上界面，到 `16000ms` 整账 100% 合拢。
