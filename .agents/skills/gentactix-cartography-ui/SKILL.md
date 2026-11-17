---
name: gentactix-cartography-ui
description: >-
  Gentactix 全屏印刷地图前端、多开拖拽 UI-03 卡片、Shift+A/右键 UI-03M、UI-03M1~M4 全套面板（四行任务、九大装备门控事件宏与虚拟实例、六行指挥 6/7 互斥场、通信手动调谐与队列重寻思）、红字 UI-ID 强制规范及 E2E 验真法典。ACTIVATE this skill whenever reading, modifying, or testing frontend code in web/index.html, art-direction/, or tests/e2e/.
---

# Gentactix 全屏印刷地图与战术卡片前端施工法典 (Cartography UI Codex V4.0)

本技能定义 `web/index.html` 的全屏图台架构、`UI-03M1 ~ UI-03M4` 完整面板体系、外置时间机只读粒子插值与 $I$ 场 `visibility=0` 预渲染环、红字 `data-ui-id` 零重叠强制制度与无欺骗 Chrome E2E 验真标准。一切实现必须 100% 服从 [SPEC_V6_TIMEMACHINE_AND_FULL_INTEGRATION_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_V6_TIMEMACHINE_AND_FULL_INTEGRATION_CODEX.md) 与 [SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md)。

> **【V6 前端红线 · 根治 V5 六大缺陷与时间机放片】**：
> - 严禁 `UI-03A` 与右上角 `×` 关闭钮及编号重叠；严禁 `#toast (UI-11)` 落在 `(0,0)` 与 `UI-04` 重叠；严禁初始卡片遮挡 `UI-04` 标题；严禁残留 `"ROUTE / 1.8km"`、`"~ 24 min"`、`"0友军"` 等写死假文本；
> - 点击地图上任意 `.map-unit` 兵牌必须真实调用 `openFloatingUnitCard(u.id)` 弹出多张独立可拖拽 `UI-03` 卡片（E2E 严禁调全局函数作弊，必须真点 DOM 兵牌且实测推进后物理位移 `Δpos > 0`）；
> - 严禁前端硬编假偏移（如 `(+72, -48)`、`(+40, -20)`）或未赋值 `pendingMapGoal`；必须支持在地图上真实点击设定目标点与多边形；
> - **前端时间机只读粒子渲染**：预挂载 $I$ 场外围 16s 预渲染缓冲环敌军粒子（`initial_visibility = 0`），走表期间沿 `particle_tracks` 线性插值 $\mathbf{x}(t_{\text{ms}})$，滑入可见格自动翻为 `visibility = 1`；时钟到达 `exact_ms` 时在目标插值坐标 $\mathbf{x}(\text{exact\_ms})$ 上爆特效并刷出原装 `exact_hp_loss`。

---

## 一 · 美术风格统一与全屏图台布局铁律

1. **SD2 / CMO 印刷地图色板与红牌 `data-ui-id` 零重叠铁律**：
   - 严格沿用 `:root` 变量：`--ink:#d6d9d4; --dim:#8f9b9b; --chrome:#242e32; --deep:#1b2428; --line:#3d484c; --paper:#ded8be; --blue:#436779; --red:#a94f43; --gold:#c5aa70; --green:#91a58c;`；
   - 所有卡片与弹出窗口左上角通过 `::after` 渲染 `8px~9px` 红色等宽 `data-ui-id`，必须预留外缘与内距净空：**严禁 `UI-01` 压住盾徽 SVG、`UI-02` 遮挡“白桦行动”标题、`UI-09` 与 `UI-09A` 打架、或与指北针/`UI-05A` 散架浮空**。
2. **极窄顶栏 + 100% 全屏动态真实地图 + 伪命题清理**：
   - `UI-01` 高 `36px`，`UI-02` 高 `26px`；彻底删除固定左侧 `#inspector` 与残余 `@media` 吸边规则，`#viewport` 100% 铺满下方；
   - 彻底删除 `UI-06`（缩放条）、`UI-12`（作战用图印章）、`UI-13`（左下角提示语）；
   - **比例尺必须对齐二进制 `8.0 m / 格` (`updateScale()`)**，删除一切 `"示意"` / `"示例范围"` / `"200m / 格"` 静态 SVG 贴片；
   - 激活 `UI-09` / `UI-09A` `#drawDraftLayer` 真工具（测距尺、战线铅笔+斜接预览、橡皮擦、四分割边界多边形），禁止仅仅弹出 Toast 敷衍。

---

## 二 · `UI-03` 真多开卡片与 `UI-03M` 旗下四大子窗口 (`UI-03M1 ~ UI-03M4`) 刚性规范

### 1. 真·多开可拖拽 `UI-03` 与 `UI-03M` 紧凑指令条
- 点击不同棋子或爆破开关实体必须**各自独立弹出可自由拖拽、置顶叠放、各自带右上角关闭按钮 `×` 的浮动 `UI-03` 窗口**（严禁单卡覆写！）；卡片内容严格按 `/api/state` 下发的 `is_direct_subordinate` 与 `I_intel` 算出之 `vis_tier` 裁剪渲染；
- 选中卡片按 **`Shift + A`** 或地图上**右键点击实例**，弹出紧凑 `UI-03M` 动作条（共享统一队列列表 `#1, #2...` 及 `[任务] -> UI-03M1`、`[事件] -> UI-03M2`、`[指挥] -> UI-03M3`、`[通信] -> UI-03M4`），所有动作按钮一律进入**真实地图交互拾取态**，严禁编写 `(+60*seq, -30*seq)` 或 `mine-1..3 (+20*k, +10*k)` 式假位移偏移。

### 2. `UI-03M1`（任务队列四行矩阵 + `/api/preview_plan` 真路径/射界形变）
- **必须先选好前三行，才能进入地图拾取点位确认**；
- 第一行：`移动 | 射击 | 停止 | 撤退`：
  - `移动`：二行 `快速|正常|隐蔽`；三行 `场强约束滑块（拖动实时请求 /api/preview_plan 驱动 DerivedReadModel 流线形变）`；四行 `速度|预估时间|体能组织度消耗`；
  - `射击`：二行 `快速|正常|精准`；三行 `射击轮数滑块（最大=持续压制直到弹药打光）`；四行 `每轮及总时间|预估弹药消耗`；
  - `停止`：二行 `趴下|掩体|原地不动`；三行 `停止时间滑块`；四行 `掩体减伤|隐蔽加成|组织度恢复`；
  - `撤退`：二行 `有序撤退|直接跑路`；三行仅 `有序撤退` 显示 `抛弃什么物资`（选 `直接跑路` 隐藏第三行并启动 $I/G$ 场自治逃逸）；四行显示移速与控制状态；
- `#1 移动` 终点生成基于后端计算的 **`@#1 虚拟实例`**，后续 `#2 射击` 严格从 `@#1` 实位发射并校验 DDA 与 Wendland C2 $K_{\text{reg}}$ 火控椭圆，禁止脱离己方单位在 `#2` 头上凭空起爆。

### 3. `UI-03M2`（九大事件宏 —— 装备门控 + 可分割多边形 + 增广弧长边走边干 + 爆破开关）
- **与任务共享同一队列列表**；**未携带对应装备/器材的事件按钮直接隐藏（`display: none`）**；
- 第一行 9 大事件：**`布雷 | 挖掘 | 建造 | 爆破 | 拆除 | 测距 | 战地救护 | 补给与搜刮 | 架设与拉线`**；
- **地图绘制与工事实例铁律**：
  - 支持在地图交互绘制**可分割多边形面积（默认四边形，点击边插入顶点成任意多边形）**或**折线段**；
  - 由后端预览驱动生成梅花桩雷场（Quincunx）/ 斜接防线（Miter）/ 1.2m 锯齿堑壕（Zigzag）虚拟实例，并在 16s 步进下驱动工兵单位按增广弧长 $\tilde{s}$ 边走边建，进度持久写入 `WorldState`；
  - **爆破实体铁律**：二行选炸药量，三行选 `定时引爆 | 开关引爆`；画完爆破区后**必须再在地图添加强制安全撤离移动点（开关位置）**，沿途拉设导线折线并在终点生成可点击的 `DetonatorSwitch` 引爆开关实体；
  - **测距铁律**：复用 `UI-09` 测距工具并绑定后端 $K_{\text{reg}}$ 标定核，提升友军杀伤精度与该处 $I_{\text{intel}}$ 置信。

### 4. `UI-03M3`（指挥面板 —— 瞬间生成场，下个 16s 步长生效驱动下属自套 `UI-03M1/M2`）
- 严格**六行结构**：
  - 第一行：`[ 指定单一单位 ] | [ 指定许多单位 ] | [ 下属全局 ]`；二行虚线分隔；三行 `[ 指令 ] | [ AO ]`；
  - 第四行（13 项严格按第三行选择互斥可用）：
    - 选 `[ 指令 ]` 仅可用：`进攻 | 突击 | 移动 | 撤退 | 防御 | 架设与拉线`（其余 7 项禁用）；
    - 选 `[ AO ]` 仅可用：`侦察 | 集合 | 区域压制 | 警戒扇区 | 战地救护 | 排雷与破障 | 弹药补给与搜刮`（其余 6 项禁用）；
  - 五行虚线分隔；六行居中标题 `交战规则开关`：`自由开火 | 仅还击 | 严禁开火` 及 `节约弹药模式 | 队形模式 | 装备模式`。

### 5. `UI-03M4`（电台物理调谐与平滑同步 —— 禁止无迟滞帕金森取消队列）
- 独立硬件调谐面板（VHF/HF 频段、洛伦兹谐振滑块、功率档位、心形天线定向角滑块、有线交换插孔），参数经 `planned_actions`/`field_directives` 传入后端物理计算脊线衍射 $\xi_k, T_{\text{diff}}$ 与链路乘数 $\eta$；
- **V5 稳定铁律**：调谐劣质时通过 **128 字节稀疏二阶 Exact ZOH 滤波**产生滞后与信息视野缩水，仅在越过 **施密特迟滞死区（$\Theta_{\text{cancel}}=3.5, N_{\text{dwell}}=4$）或原目标彻底失效** 时方才安全重寻思，严禁 $\eta$ 一抖就清空下属现行队列导致全军原地发呆。

---

## 三 · HTTP 契约严密化与无欺骗 E2E 门禁 (`verify_production_ui.mjs`)

- **`UI-14B` 与交互动作协议对齐**：禁止发送错误操作名（如 `btnSplitSquad`），所有交互动作一律发标准 `/api/unit_op`（`enqueue_task`, `enqueue_event`, `issue_command_field`, `tune_comms`, `detonate_switch`, `split_squad`, `merge_squads`, `equip_adjust`, `deploy_heavy_unit`）并携带 `piece_id` 与 `player_commander_id`；`/api/commit_turn` 推进 `{"subticks": 1}`（若附带批处理字段 `planned_actions` / `planned_events` / `staging_ops` / `field_directives` 须保证前后端全真入核不丢弃）；
- **E2E 红灯一票否决项**：
  1. 用 `getBoundingClientRect()` 遍历全图可见 `data-ui-id` 及文字/徽章，AABB 非法交叠直接 FAIL；
  2. 命中 `"示意"` / `"示例范围"` / `"200m / 格"` 字符串直接 FAIL；
  3. 必须真正同时点开 `>= 2` 张独立浮动 `UI-03` 并各自拖拽关窗，通过；
  4. 必须在 SVG 地图真实点击下达移动/多边形工兵任务并 `commit_turn` 推进 `16s` 步长，校验 `/api/state` 真实物理位移 $\Delta \text{pos} > 0$ 与进度持久化。
