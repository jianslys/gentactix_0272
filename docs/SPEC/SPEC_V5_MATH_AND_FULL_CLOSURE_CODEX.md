# GENTACTIX V5.0 全栈闭环核心规格书：四大数学算子内核化、前后端接口合龙与 V4 缺陷根治规范

- **文档编号**：`GTX-SPEC-V5-FULL-CLOSURE`
- **基准对齐**：`Δx = 2³ m = 8.0 m`（整数右移 `>> 3`，浮点 `* 0.125`）；`Δt = 2⁴ s = 16.0 s`；全图拉格朗日粒子 **100% 实算（严禁懒计算）**；跨平台位级确定性（Bitwise Determinism）
- **理论底座**：严格采纳 `docs/RESEARCH_THEORY_SYNTHESIS_AND_ARCHITECTURAL_VERDICT.md` 审定裁剪后的四大闭式数学算子

---

## 第零章 · V4 交付物审计暴露出的七大缺陷（必杀清算清单）

对上一轮代码进行逐行源码审阅与 E2E 实机截图验收后，确认以下七类严重缺陷必须在本版（V5.0）彻底连根拔除，严禁带病运行：

1. **假消费与孤岛算法**：
   - `combat_logistics.rs` 第 468 行将 `pixel_containers` 中的 `incoming_packets` 取出存入 `_consumed_packets` 直接丢弃，后面依旧暴力遍历 `0..units.id.len()` 扣血；
   - `fields/mod.rs` 新增的 `compute_g_alloc`、`apply_g_work_consumption`、`ThreatRadiationOperator`、`check_hysteresis_and_rethink` 仅在单元测试中运行，完全未接入 `sim_loop.rs` / `spatial_autonomy.rs` 主循环。
2. **前后端接口协议完全错位（请求进黑洞）**：
   - 前端 `commitSimulationTurn` 发送的 `planned_actions`、`planned_events`、`staging_ops`、`field_directives` 四个字段，后端 `gentactix-server::post_commit_turn` 完全不识别；
   - 前端 `UI-14B` 发送的 `op: "btnSplitSquad"` 等按钮 ID，与后端 `/api/unit_op` 要求的 `"split_squad"` / `player_commander_id` / `piece_id` 不匹配。
3. **`UI-03` 假多开与残留旧概念死文案**：
   - 屏幕仅有一张静态 `#inspector` 被写死在左上角并覆盖了 `UI-04` 标题，点击其他兵牌不会创建新卡片；
   - 地图与比例尺残留 `1.8km ~ 24 min`、`林缘遮挡 · 散布示意`、`FIRE ENVELOPE / 示例范围`、`200m / 格 · 示意` 等硬编码假文本，且比例尺未按 $2^n$ 二进制刻度对齐 `8m` 网格。
4. **红字 `UI-ID` 与标题文字严重叠压**：
   - `UI-01` 压在盾牌 Logo 上、`UI-02` 压在“白桦行动”文字上、`UI-09A` 压在工具按钮上，`UI-09` 与指北针/`UI-05` 排版散乱割裂。
5. **前端“造假式虚拟实例”与零地图绘点交互**：
   - `commitTaskToQueue` / `commitEventToQueue` 未让玩家在地图上点选目标或框选多边形，而是胡乱向右上偏移 `(+60*seq, -30*seq)` 塞入假文本标签；
   - `UI-03M1` 拖动滑块不变形流线，`#2 射击` 不从 `@#1` 发出 DDA 射线；`UI-03M2` 无可裂变任意多边形绘制、无单位边走边干逐个转正、爆破不强接撤离点与开关电缆实例；`UI-09A` 工具栏点击无任何绘制能力。

---

## 第一章 · 后端核心数学内核全面实装（四大黄金算子接入主循环）

**核心规则**：所有新增算子必须直接挂载进 `SimWorld::step_subtick_16s` 与 `gentactix-server` 运行链路中，严禁只写孤岛单元测试！

### 1.1 模块 A · 任务场 $G$ 几何切割、双层核销与 $O(M)$ 确定性拍卖分流
1. **统一几何切割投射（`UI-03M3` $\rightarrow$ `8m` 网格 `G_0`）**：
   - 一维折线指令 $\Gamma$（进攻、突击、移动、撤退、防御、架设与拉线）采用 **Amanatides-Woo 参数化体素遍历**按段长写入；
   - 二维多边形 $\Omega$（侦察、集合、区域压制、警戒扇区、战地救护、排雷与破障、补给与搜刮）采用 **Sutherland-Hodgman 裁剪 + Shoelace 面积积分**写入；
   - 叠加后施加一次**九点各向同性保质量平滑核 $K_{\text{iso}}(\alpha=0.25)$**（中心 $1-\alpha$，正交 $\alpha/6$，对角 $\alpha/12$）。
2. **双层核销五元组 $(G_0, \hat{\Phi}_v, \Phi_u, R_{\text{pool}}, G_{\text{alloc}})$**：
   - 在途预扣 $\hat{\phi}_v$（队列锁定目标瞬间扣除）与到场实扣 $\phi_u$（进入目标格半径 $5.66\text{ m}$ 交接）；
   - **严格模态分流 $\beta_{\text{mode}} \in \{0, 1\}$**：
     - 状态占坑型 $G^{\text{state}}$（$\beta=0$：防御/警戒/集合）：保持 $G_0$ 不变，人撤或阵亡则缺口回流；
     - 积分做功型 $G^{\text{work}}$（$\beta=1$：排雷/救护/搜刮/拉线/筑城）：同步永久削减 $G_0$ 与 $\phi_u$，撤离后绝不反弹。
   - **战损平滑驰豫池 $R_{\text{pool}}$**：取消队列/阵亡的残余份额进入 $R_{\text{pool}}$，按 $\tau_{\text{rel}} = 32.0\text{ s}$（$e^{-16/32} = 0.6065$）指数衰减并切向扩散入 $G_{\text{alloc}} = \max(0, G_0 - \hat{\Phi}_v - \Phi_u - R_{\text{pool}})$；
   - 粒子切换施加在途进度迟滞壁垒 $\Delta C_{\text{hyst}}(p_m, \Delta\theta)$，当 $p_m \ge 0.75$ 时硬锁定。
3. **装备位掩码门控与跨平台确定性分配（裁决采用档位 B 拍卖，禁用 Sinkhorn！）**：
   - 强制校验装备位掩码 `(cap_mask & req_mask) == req_mask`；
   - **严禁引入含 `exp/log` 的浮点 Sinkhorn 迭代**！一律使用**多容量最小堆确定性拍卖算子（Deterministic Multi-Capacity Auction）**，使用整数或定点代价 `(cost_fixed_i64, unit_id.0, cell_idx)` 严格字典序打破平局，确保 x86/ARM 跨平台比特级完全一致。

---

### 1.2 模块 B · 虚拟实例生成与单步 16s 增广弧长边走边干积分器 (`UI-03M2`)
1. **顶点定量化与确定性种子**：
   - 前端传入的任意多边形（含点边分裂后的顶点）或折线坐标在后端量化至 $\Delta x / 1024 = 7.8125\text{ mm}$ 定点子格，CCW 规范化后由 `SplitMix64` 生成循环移位不变种子 $H_{\text{seed}}$。
2. **三模态 $O(K)$ 虚拟实例生成与施工巡回路线 $\gamma(s)$**：
   - **模态 A（布雷/爆破）**：周长补偿（$\alpha=1.08, \beta=0.50$）交错梅花桩晶格（受限极坐标抖动确保抗殉爆最小间距 $> 4.0\text{ m}$）；
   - **模态 B（散兵坑/碉堡）**：筛选迎敌法向边（$n_{\text{out}} \cdot n_{\text{threat}} \ge 0.15$），斜接内缩生成主/预备梯队；
   - **模态 C（堑壕/铁丝网/拉线）**：等弧长采样加奇偶交替抗纵射折曲（$A_{\text{zig}} = 1.2\text{ m}$）；
   - **无自交巡回轨迹 $\gamma(s)$**：测地牛耕排序 + 带状 2-Opt 解交 + 非凸凹角内缩 $0.50\text{ m}$ 绕行路标。
3. **16s 增广弧长 $\tilde{s}$ 边走边干与半截永久持久化**：
   - 设折算比 $\lambda_{\text{eq}} = 0.20\text{ m/(人}\cdot\text{s)}$，将净工时定额 $W_0$ 转为等价作业弧长 $L_{\text{work}}^{\text{eq}} = \lambda_{\text{eq}} W_0$，并受压制指数非对称衰减：行进保留 $\sigma_{\text{move}}(S) = \max(0.25, 1 - 0.5 S)$，暴露作业保留 $\sigma_{\text{work}}(S) = (1 - S)^2$；
   - 每个 16s 步长推进 $\tilde{s}$，跨过 `work_end` 阈值的虚拟实例当场**在 Stage 10 转正写入 `WorldState`**（地雷、散兵坑、堑壕、碉堡、电话线边或拆除清理），返回最新剩余未完虚拟实例供前端渲染；
   - 若中途被炮火打断撤退：前半截已转正实体永久留存战场；处于施工半截的焦点土方工事（散兵坑/堑壕/碉堡），其已累计工时 $\phi_{j^*} W_0$ 必须**写入 `WorldState` 的 `cell_mods` 权威持久化**，地雷/炸药引信则退回库存。
   - **爆破宏专属闭环**：事件必须含撤离目标格；若为 `开关引爆`（校验携带 `wire_cable` 电缆），单位装药后沿撤离路线步进至终点时，向 `WorldGrid::wire_edges` 写入整条电缆边并在撤离终点生成可交互的 **`爆破起爆开关` 实体**（`kind = "detonator_switch"`）。

---

### 1.3 模块 C · 正交信息场 $I$、真实 8m 网格寄存打击、流线集成与双点测距核
1. **彻底根治 `combat_logistics.rs` 假消费漏洞**：
   - P2G（Stage 8）：开火仅将 `ShotPacket` 推入目标格 `pixel_containers`；
   - Grid-to-Field（Stage 9A）：真实消费并遍历所有非空 `pixel_containers` 中的每一个 `ShotPacket`！汇总包内 `shots`、`ammo_spec_id`、`incoming_dir_deg` 与 `priority_target`，计算：
     - 目标格及其半径内 $8\text{m}$ 格的**快衰减瞬态爆炸危险 $H_{\text{blast}}$**（$\tau_{\text{blast}} = 9.0\text{ s}, \alpha_{\text{blast}} = 0.1690$）；
     - **慢恢复压制密度 $S_{\text{supp}}$**（$\tau_{\text{supp}} = 36.0\text{ s}, \alpha_{\text{supp}} = 0.6412$），驱动三态施密特姿态机：`Active (ζ=1.00) <-> Pinned (ζ=0.35) <-> Prone (ζ=0.02)`，压垮敌军对外投射的辐射威胁源项 $S_k = \frac{\zeta}{1 + S_{\text{supp}}^3} S_{k,0}$，从而自然涌现 **32s（2 步长）步炮协同黄金突击窗口**；
   - G2P（Stage 9B~10）：从受击 $8\text{m}$ 格的**空间桶索引 (`SpatialBucketGrid`)** 中精确取出该格内的驻留单位（$O(|\text{ActiveCells}|)$，删除 `0..units.id.len()` 全图遍历！）计算伤亡并消费清空 `incoming_packets`。
2. **情报清晰度通道 $I_{\text{intel}}(x, y) \in [0, 1]$**：
   - 每 16s 步长按 $\alpha_{\text{intel}} = 0.8409$（半衰期 $64\text{ s}$）自然遗忘，叠加 DDA 视线、上报、与测距证据的对数生存融合，采用 `0.05` 迟滞回差门限决定目标显影等级：`[0.20 接触 | 0.45 识别 | 0.75 标定]`。
3. **掩体正交投影与 $\lambda$ 滑块流线（`UI-03M1`）**：
   - 采用 3×3 各向同性 Scharr 梯度核与掩体正交横切投影 $\mathbf{P}_\perp(\mathbf{d})\nabla G_{\text{safe}}$；
   - 移动滑块 $\lambda \in [0, 1]$ 通过 $\varphi(\lambda) = (1 - \lambda)^{1.25}$ 控制避障与掩体绕行权重（$\lambda=1$ 为纯几何直线）；“直接跑路”撤退模式开启 CFL 4 子步长 RK2 积分 + 势谷过零阻尼截断。
4. **双点测距核 $K_{\text{reg}}$（`UI-03M2` 测距）**：
   - 采用紧支撑 $C^2$ Wendland $\psi_{3,1}(r)$（观察半径 $96\text{ m}$，目标半径 $64\text{ m}$）乘视差角因子 $\exp(-(\theta_T / 35^\circ)^2)$，收缩散布椭圆并向目标格注入 $\Delta J_{\text{reg}} = 1.05 K_{\text{reg}}$ 瞬间拉高 $I_{\text{intel}}$ 突破 `0.75` 标定门限。

---

### 1.4 模块 D · 零超越函数电台衍射算子与 128 字节稀疏模态向量二阶 ZOH 滤波 (`UI-03M4`)
1. **无超越函数物理链路**：
   - VHF(45MHz)/HF(6MHz) 沿 DDA 8m 高程剖面使用无开方主刃搜索 $\xi_k = \frac{h_k|h_k|}{(s_k-s_a)(s_b-s_k)}$ 定位 3 刃 Deygout 脊；
   - 衍射损耗用三阶有理分式 $T_{\text{diff}}(\nu) = \frac{1 + 0.2512 u}{1 + 1.5048 u + 5.9201 u^3}$ 替代 `sqrt/log10`；
   - 天线方向图用向量点积升余弦心形函数替代 `atan2/cos`，耦合洛伦兹频偏 $L(\Delta f)$ 与并联有线电话 `wire_edges`，产出 $\eta \in [0, 1.2]$ 与敌测向暴露源 $E_{\text{radio}}$。
2. **降维版二阶临界阻尼（$\zeta=1$）Exact ZOH + 施密特死区（绝禁 80GB 内存爆炸）**：
   - 严禁对全图二维网格做单单位滤波！在每个单位状态上维护稀疏指令权重滤波器 `directive_filters: Vec<(u32, f32, f32)>`（记录指令 ID、过滤后权重 $\hat{w}_d$ 与变化率 $V_d$，单单位 $\le 128$ 字节）；
   - 按 Exact ZOH 闭合递推更新 $\hat{w}_d$，计算失配泛函 $\Delta\mathcal{J}$：仅在驻留步数 $\ge N_{\text{dwell}}$（默认 4 步）且 $\Delta\mathcal{J} \ge \Theta_{\text{cancel}}$（默认 3.5）时取消当前队列，进入 $N_{\text{rethink}}(\eta) = \lceil 2 + 3(1-\min(\eta, 1))^{1.5} \rceil$ 步长重寻思，待满足锁定条件后再自主生成新 `UI-03M1/M2` 队列去消解残余 $G$ 场！

---

## 第二章 · 前后端全真协议合龙（彻底消灭请求黑洞）

`crates/gentactix-server/src/main.rs` 与 `web/index.html` 必须使用以下统一的 JSON 契约，一个字段也不许自创篡改：

1. **`GET /api/state`（导出完整物理态势、双场显影与队列快照）**：
   - 时钟：`tick`, `clock_seconds`, `clock_str`, `step_seconds: 16`；
   - 网格与地形：`grid: { rows, cols, cell_m: 8.0 }`，及供 `/api/terrain_raw.bin` 渲染的水系/林地/高程；
   - 己方单位 `own_pieces`：补充导出 `inventory`（实际背负物品数组，如地雷型号与数量、炸药 kg、电缆 m、铲子等，直接驱动 `UI-03M2` 门控隐藏！）、`is_direct_subordinate`、`unified_queue`、`rethink_cooldown_ticks`、`ghost_instances`、`comms_params`、`eta_multiplier`；
   - 敌方接触 `visible_enemies`：附带实时计算的 `intel_clarity`（$I_{\text{intel}} \in [0, 1]$）与 `vis_tier`（0 迷雾 / 1 接触 / 2 识别 / 3 标定），直接驱动 `UI-03` 卡片字段裁剪！
   - 战场新增实体与场图层：`deployed_structures`（真实地雷、散兵坑、堑壕、碉堡、起爆开关、有线电缆 `wire_edges`）、`virtual_instances`（施工中/移动终点 `@#1`）、`active_command_fields`（上级画的指令线与 AO）。
2. **`POST /api/preview_plan`（实时预览计算，不改 `WorldState`）**：
   - 前端打开 `UI-03M1` 拖动滑块 $\lambda$、或在 `UI-03M2` 画完多边形/折线、或切到 `#2 射击` 时调用；
   - 后端基于当时 `DerivedState` 真实算出：**`UI-03M1` RK2 预测流线坐标数组 + 第四行预估数值**、**从 `@#1` 虚拟实例发出的 DDA 通视/遮挡火控线**、或 **`UI-03M2` 多边形内生成的所有虚拟地雷/工事坐标点集 + 2-Opt 施工路径**，零延时返给前端绘图！
3. **`POST /api/unit_op`（真实挂载队列指令、宏事件、电台调谐、起爆开关点火与整编）**：
   - 前端所有按钮严格使用后端真 `op` 枚举并传入 `player_commander_id` & `piece_id`：
     - `enqueue_task`（携带 `UI-03M1` 全部四行参数 + 地图画点坐标）；
     - `enqueue_event`（携带 `UI-03M2` 宏类型、多边形/折线顶点、密度、爆破撤离点与是否开关引爆）；
     - `issue_command_field`（携带 `UI-03M3` 受令范围、指令/AO 模式、13 选 1 类型、线/面坐标、ROE 开关，瞬间写入 $G_0$）；
     - `tune_comms`（携带 `UI-03M4` 波段、频偏 $\Delta f$、功率、天线模式、方位角 $\psi$，返回真实 $\eta$、指挥半径与暴露度）；
     - `detonate_switch`（触发地图上某“起爆开关”实例，按电缆连接瞬时引爆炸药）；
     - `split_squad` / `merge_squads` / `equip_adjust` / `deploy_heavy_unit`（对齐 `UI-14B`）。
4. **`POST /api/commit_turn`**：
   - 接收 `{"subticks": 1}`，推进一次完整的 16s 原子步长，返回最新 `state`；前端严格同步真实步进坐标与施工实体转正状态。

---

## 第三章 · 前端全屏图台彻底精修与 V4 缺陷根治规范

1. **红字 `data-ui-id` 防碰撞布局清洗（严禁任何遮挡文字或图标）**：
   - **`UI-01`（高 `36px`）**：左侧 `.brand` 增加 `padding-left: 38px`，让左上角红字 `UI-01` 独立落在左侧微缩区内，绝不压住盾牌 Logo；
   - **`UI-02`（高 `26px`）**：左侧增加 `padding-left: 38px`，红字 `UI-02` 独立居左，绝不压在“白桦行动”文字上；同时移除顶栏重复多余的“部队”按钮，保留简洁清晰的 `编制`、按钮化视图与 `UI-18` 读秒框；
   - **`UI-09`（右上角一体化参谋工具盒）**：将指北针、**`UI-09A`**（测距尺/画笔/橡皮擦/多边形/折线工具）与 **`UI-05`/`UI-05A`**（图层折叠列表）统一收拢在同一个右上角深色铬合金紧凑面板框内；各子角标红字留足 `padding-top: 14px` 留白区，**100% 消灭红字叠字**！
   - **`UI-04`（战区标题）**：因左侧固定边栏已废除，默认无弹窗挡住 `UI-04`（初始打开不默认焊死一张占满左侧的大卡，改由点击兵牌按需多开，或初始在左下/右中侧适当位置打开一张紧凑卡片），确保 `UI-04` 与左上角视野开阔。
2. **真 · 多开可自由拖拽 `UI-03` 浮动卡片架构**：
   - 废除单例 `#inspector`！改为容器 `#cardsContainer` 动态创建 `.unit-card[data-unit-id="..."]`；
   - 支持同时点开多张单位卡片（最多同时平铺多张，各有唯一编号标记与各自独立的 `×` 关闭钮、头部可全屏拖拽、`z-index` 点击置顶、金色边框标识当前手动选中态 `.active-card`）；
   - 彻底剔除残留的 `@media` 把 `.inspector` 强行变成左侧固定条的旧规则；
   - 卡片内显示严格绑定后端返回的 `is_direct_subordinate` 与 `vis_tier`（`I_intel`）：低清晰度敌军自动隐藏规模/武器/弹药；友好非直属自动折叠细目。
3. **真 · 全量清空静态概念假数据 & 对接 $8\text{m}$ 真实世界坐标系**：
   - 彻底删除白桦河 SVG 里的写死假文本 `PHASE 01 / 1.8km`、`林缘遮挡 · 散布示意`、`FIRE ENVELOPE / 示例范围`、`200m / 格 · 示意` 以及写死的 `1.8 km ~ 24 min`；
   - 建立双向精确坐标换算器 `worldPxToSvg` / `svgToWorldPx`，将后端的 $(r, c)$ 与 $8\text{m}$ 真实世界米制坐标无缝映射到 SVG 视野；底图实时叠加后端 `/api/terrain_raw.bin` 高程阴影、水系、森林、道路及 $I_{\text{intel}} / H_{\text{blast}} / I_{\text{threat}} / G_{\text{alloc}}$ 场热力图层（可开关）；
   - 底部居中四段印刷比例尺 `UI-07` 按 **纯二进制 $2^n$ 米制刻度**（`32m / 64m / 128m / 256m / 512m / 1024m / 2048m`）动态对齐四段色块，标明 **`8m / 格 · 纯二进制基准`**！
4. **真 · 交互式地图标定、多边形裂变与施工实体渲染**：
   - **选 `UI-03M1` 后必须先配完 1~3 行 $\rightarrow$ 点“在地图标定” $\rightarrow$ 进入准星选点态 $\rightarrow$ 真用鼠标点击地图位置才确认入队**（如果是 `撤退 -> 直接跑路` 则隐藏第 3 行并直接点击生效无需地图选点）；拖动移动滑块 $\lambda$ 实时调用 `/api/preview_plan` 重绘预测流线；
   - **`UI-03M2` 真多边形绘制与边分裂**：激活绘制后，鼠标左键在地图拖出默认四边形（或依次点折线），**点击四边形任意边的中点 `+` 锚点即可分裂出新顶点并拖拽调整为任意非凸多边形**！画完立即调用 `/api/preview_plan` 在区域内绘制出真实排列的**每一颗虚拟地雷/散兵坑/堑壕/碉堡**与 2-Opt 施工路径；爆破模式强迫在画完爆破区后进入第二步“点选撤离开关位置”，确认后在地图上画出撤离路径与开关待建标记；
   - **`UI-09A` 真绘制工具**：测距尺真能在地图两点间拉出红色精密标尺显示精确米数（按 `8m` 网格欧氏距离计算）并查看沿途 DDA 通视；画笔真能在地图 `#drawDraftLayer` 手绘战术线；橡皮擦真能点擦草稿！
   - **紧凑弹出定位 `UI-03M`**：按下 `Shift + A` 或右键地图兵牌时，`UI-03M` 智能锚定在所选卡片或兵牌一侧（自适应屏幕边缘防溢出），不再一刀切死死糊在屏幕正上方。
