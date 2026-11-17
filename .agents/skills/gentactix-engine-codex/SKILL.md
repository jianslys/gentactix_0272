---
name: gentactix-engine-codex
description: >-
  Gentactix 确定性世界事务内核与纯二进制 2^3=8m 基准施工法典 (Gentactix Deterministic Core Engine Codex). ACTIVATE this skill whenever reading, modifying, or testing Rust code in crates/gentactix-core or crates/gentactix-server.
---

# Gentactix 确定性世界事务内核施工法典 (Engine Codex)

本技能锁死 `crates/gentactix-core` 与 `crates/gentactix-server` 的底层物理、数学与事务边界。一切代码实现必须与 [SPEC_FINAL_GENTACTIX_CORE_ENGINE.md](file:///D:/tools/gentactix/docs/SPEC/SPEC_FINAL_GENTACTIX_CORE_ENGINE.md) 保持 100% 严丝合缝。

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

## 四 · 并行确定性与验证命令

- **严禁无序归约**：任何多线程并行阶段只能写线程本地缓冲，合并时必须按键稳定排序后再归约；严禁遍历 `HashMap` 直接修改权威状态。
- **必跑验证命令**：
  ```powershell
  cargo test --manifest-path "D:\tools\gentactix\Cargo.toml" -p gentactix-core
  cargo check --manifest-path "D:\tools\gentactix\Cargo.toml" --workspace
  ```
