---
name: gentactix-engine-codex
description: >-
  Gentactix 确定性世界事务内核与纯二进制 2^3=8m 基准施工法典 (Gentactix Deterministic Core Engine Codex). ACTIVATE this skill whenever reading, modifying, or testing Rust code in crates/gentactix-core or crates/gentactix-server.
---

# Gentactix 确定性世界事务内核施工法典 (Engine Codex)

本技能锁死 `crates/gentactix-core` 与 `crates/gentactix-server` 的底层物理、数学与事务边界。一切代码实现必须与 [SPEC_V5_MATH_AND_FULL_CLOSURE_CODEX.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_V5_MATH_AND_FULL_CLOSURE_CODEX.md)（权威总规，整合历史架构基线 [SPEC_FINAL_GENTACTIX_CORE_ENGINE.md](file:///D:/tools/gentactix/archive/legacy-docs/specs-v5-v7/SPEC_FINAL_GENTACTIX_CORE_ENGINE.md)）保持 100% 严丝合缝。

> **【绝对禁令 · 反懒计算与反孤岛】**：
> 1. 在集团军级（数万拉格朗日粒子）本地基准验证通过前，严禁任何“懒计算/连级阉割代理”，所有单位与粒子 100% 全量执行真实计算；
> 2. 严禁只把数学算子留在 `fields/mod.rs` 供单元测试自嗨，却不接入 `sim_loop.rs`；严禁出现 `combat_logistics.rs` 中 `let _consumed_packets = std::mem::take(...)` 式的假消费与线性全表扫，以及测试里残留 `200.0 m` 网格假基准。

---

## 一 · 纯二进制空间基准铁律 ($\Delta x = 2^3\text{m} = 8.0\text{m}$)

1. **严禁十进制 5m 污染**：
   - 全系统唯一合法的基础网格步进元为 **$\Delta x = 8.0\text{ m} = 2^3\text{ m}$**（常量 `CELL_METERS = 8.0` / `CELL_SHIFT = 3`）；
   - 任何源码、注释、接口、测试中绝对禁止出现将 5m 作为基础网格的逻辑。
2. **位运算与零误差浮点换算**：
   - 整数米坐标转网格索引：强制使用位移 `meters >> 3`；
   - 格内相对偏移：强制使用位与掩码 `meters & 7`；
   - 浮点米转网格：乘以 `0.125`（即 $2^{-3}$，IEEE-754 尾数位精确为 0，零截断误差）。
3. **多尺度二进制层级对齐**：
   - **SIMD Strip（$64 \times 8$ 格）**：$512\text{ m} \times 64\text{ m}$；
   - **Spatial Tile（$64 \times 64$ 格）**：$512\text{ m} \times 512\text{ m} = 2^9\text{ m}$；
   - **Macro Parent Cell（$32 \times 32$ 格）**：$256\text{ m} \times 256\text{ m} = 2^8\text{ m}$；
   - **Full Map（$1024 \times 1024$ 格）**：$8192\text{ m} \times 8192\text{ m} = 8.192\text{ km} = 2^{13}\text{ m}$。

---

## 二 · 世界状态三分法 (State Tripartition)

在 `crates/gentactix-core/src/state/mod.rs` 中，任何新增状态必须严格归类，严禁混放：

1. **`AuthoritativeWorldState`（权威世界状态）**：
   - 唯一跨 Tick 持久化的真相源，纳入 `state_hash` 计算；
   - 包含：Tick 计数、确定性 RNG 种子、地形网格与工事、ECS 单位权威状态、指挥关系、活跃原子意图、在途电台/传令兵消息。
2. **`DerivedReadModel`（派生只读视图）**：
   - 每 Tick 重建或增量更新的加速索引与场数据；
   - 包含：Tile 空间索引、连通域、导航代价场、信息斥力场 $I$、任务引力场 $G$、火力威胁场、指挥官迷雾过滤快照；
   - **铁律**：删除全部 `DerivedReadModel` 后，从 `AuthoritativeWorldState` 重建必须得到完全一致的后续 Tick 结果。
3. **`TransientTickScratch`（瞬态工作缓冲）**：
   - 仅在单 Tick 内部存活的候选意图、路径提案、事件缓冲与归约桶，Tick 结束即清空。

---

## 三 · 13 阶段原子 Tick 事务流水线

`crates/gentactix-core/src/transaction/mod.rs` 中的 `execute_tick` 必须严格按以下 13 个固定阶段顺序执行，严禁跳步或穿插：

1. **Stage 0 `TickSeal`**：递增 Tick，封印外部命令队列；
2. **Stage 1 `IngressValidation`**：校验外部命令权限、电台连通性与幂等 ID，按 `(tick, commander_id, sequence_no, command_id)` 确定性排序；
3. **Stage 2 `SpatialIndexRebuild`**：重建 $64 \times 64$ Tile 桶与占用计数；
4. **Stage 3 `FieldRefresh`**：刷新导航、视野、信息场 $I$、任务场 $G$ 与火力场；
5. **Stage 4 `CommandPropagation`**：推进在途命令延迟，到期写入单位收件箱；
6. **Stage 5 `IntentArbitration`**：五级优先级仲裁（硬失效 > 生存退避 > 直接命令 > 自治跟进 > 保持），每单位仅产出一个 `ActiveIntent`；
7. **Stage 6 `MotionProposal`**：并行计算期望速度与连续子步防穿墙采样，仅产出 `MotionProposal`，**严禁直接修改单位坐标**；
8. **Stage 7 `MotionCommit`（唯一合法位置写点）**：按 `UnitId` 升序与容量上限解决目标格冲突，原子更新单位位置与朝向；
9. **Stage 8 `PerceptionAndRecon`**：基于新位置执行共享 DDA 视线扫描，生成侦察事件；
10. **Stage 9 `CombatAndLogisticsProposal`**：生成直射、曲射、压制、补给与组织度恢复候选事件；
11. **Stage 10 `EventCommit`（唯一合法战斗/后勤写点）**：全体候选事件按 `(tick, phase, priority, target_unit_id, source_unit_id, kind_code, sub_index)` 稳定排序后顺序归约，夹逼更新非负库存与伤亡；
12. **Stage 11 `CommandAndReorgCommit`**：处理指挥官阵亡接替、孤立编制重组；
13. **Stage 12 `SnapshotPublish`**：生成指挥官迷雾过滤视图、更新 `state_hash` 并发布只读快照。

---

## 四 · V5.0 剪枝定型四大数学算子与防伪铁律（全量入核闭环）

1. **铁律一 · $G$ 场在途预扣、占功二分与确定性 Auction 分流**：
   - **空间裁剪与核卷积**：采用 Cut-cell Radon 几何投影与 9 点各向同性高斯核 $K_{\text{iso}}$；
   - **占坑 vs 做功严格分层**：`β_mode = 0`（守备/警戒/集合）为状态约束，人走/死则恢复；`β_mode = 1`（排雷/筑城/救护/布线）为不可逆积分做功，永久削减权威 `WorldState` 的格点基底 $G_0$（哪怕工兵半途撤退，已施工进度依然按网格单元持久化，绝不保存在瞬态 `TickScratch` 中或弹回 100）；
   - **确定性离散最优传输**：用**在途预扣弛豫池 $R_{\text{pool}}$ + $\Delta C_{\text{hyst}}$ + Tier-B 多容量最小堆拍卖（Multi-Capacity Min-Heap Auction）**替代浮点敏感的 `exp/log` 熵正则 Sinkhorn，保证逐位跨平台确定性。
2. **铁律二 · $I$ 场正交双通道、双时间尺度压制窗口与真 P2G/G2P 输运**：
   - **三元正交场**：$I = I_{\text{intel}} \oplus (H_{\text{blast}}, I_{\text{threat}})$；侦察衰减 $T_{1/2}=64\text{ s}$（$\alpha_{\text{intel}} \approx 0.8409$）；
   - **双时间尺度涌现突击窗口**：落点爆炸危害 $\tau_{\text{blast}}=9\text{ s}$ 远快于人员压制恢复 $\tau_{\text{supp}}=36\text{ s}$，自然涌现停炮后约 `32s` 步兵突击窗口；
   - **3 态施密特源项坍塌**：我方火力推高目标格 $H_{\text{blast}}$，将敌发射点打入 `Active -> Pinned -> Prone` 使向外辐射源项 $S_k \to 0$，严禁拿我方火力直接去减目标格自身威胁；
   - **正交绕行与标定**：$P_\perp(d)\nabla G_{\text{safe}}$ + Scharr 梯度 + RK2 $\lambda$-流线 + Wendland C2 双点测距标定核 $K_{\text{reg}}$；
   - **真 P2G/G2P 闭环**：在 `combat_logistics.rs` 与 `sim_loop.rs` 中彻底消灭 `_consumed_packets = std::mem::take(...)` 弃置代码与 `for uid in 0..units.id.len()` 线性扫表，实现真实粒子-网格互换。
3. **铁律三 · 电台物理调谐与 128 字节稀疏 ZOH 迟滞死区**：
   - **物理链路因子**：无开方地形脊线比 $\xi_k$、3 阶有理衍射透射 $T_{\text{diff}}(\nu)$、点积心形天线方向图、洛伦兹失谐因子 $L(\Delta f)$；
   - **内存剪枝红线（≤128B/单位）**：绝对禁止为每个单位分配 2D 全图场数组！必须采用稀疏指令权值向量 `Vec<(u32, f32, f32)>` 执行二阶临界阻尼 Exact ZOH；
   - **施密特抗抽搐死区**：锁定 $\Theta_{\text{cancel}}=3.5, \Theta_{\text{commit}}=1.8, N_{\text{dwell}}=4$（及 30% 场偏门槛），电台乘数 $\eta$ 仅调节本地视野/同步滞后率，绝不删减上级总额 $G_0$ 或令部队原地震颤重置。
4. **铁律四 · 任意多边形实例、增广弧长 `s_tilde` 与爆破开关实体**：
   - 7.8125 mm 量化基座 + `SplitMix64` 确定性生成：梅花桩雷场（Quincunx）、斜接偏置防线（Miter-offset）、锯齿堑壕（Zigzag $A_{\text{zig}}=1.2\text{ m}$）；
   - 测地线牛耕扫掠 + 2-Opt + 凹点绕障，单步 $\Delta t=16\text{ s}$ 增广弧长 $\tilde{s}$ 边走边干积分及 4 态中断恢复 FSM；
   - 爆破实体闭环：强制撤离安全点 + 铺设导线折线 + `DetonatorSwitch` 开关实体。

---

## 五 · Server HTTP 契约闭环与验证命令

- **`crates/gentactix-server` 契约严密化**：
  - `POST /api/commit_turn` 必须全量消费前端下发的 `planned_actions`、`planned_events`、`staging_ops`、`field_directives`（含多边形与电台参数），绝不静默丢弃；
  - `POST /api/unit_op` 严格收口规范操作枚举（`split_squad`、`merge_squad`、`trigger_detonator` 等）及 `piece_id` / `player_commander_id` 校验；
  - `POST /api/preview_plan` 基于 `DerivedReadModel` 输出无副作用规划预览；`GET /api/state` 严格执行 `is_direct_subordinate` 与 `vis_tier` 迷雾裁剪。
- **严禁无序归约**：任何多线程并行阶段只能写线程本地缓冲，合并时必须按键稳定排序后再归约；严禁遍历 `HashMap` 直接修改权威状态。
- **必跑验证命令**：
  ```powershell
  cargo test --manifest-path "D:\tools\gentactix\Cargo.toml" --workspace
  ```
