# GenTactix V2 Pure Engine · 仓库执行规约 (GEMINI.md)

> [!IMPORTANT]
> **【工作区定位】**：本仓库已升级为 **GenTactix V2 纯 8 大支柱排/班战术推演引擎（纯无头 Rust 内核 + 极简 Axum 传输壳）**。一切工程改动须严守本规约、真实编译警告零容忍与端到端确定性验证；一律尊称用户为**“大王”**，保持干练直白、实事求是，绝不浮夸媚宅。

---

## 一 · 八大核心支柱与工程铁律

1. **纯二进制 8m 空间与 16s 整数时钟红线**：
   - 战术网格单元永久锁死为 `TACTICAL_CELL_SIZE_M = 8.0`（$2^3\text{m}$），禁止混入旧 200m / 5m 尺标假设；
   - 仿真推进步长锁死为 `TICK_SECONDS = 16`（`clock_seconds = epoch_seconds + (tick << 4)`）。
2. **三态严格正交（`state/`）**：
   - `WorldState`：唯一权威存盘与 `authoritative_sha256()` 真相源；
   - `DerivedState`：仅存空间桶、G/I 场与 Deygout 绕射衰减派生缓存，销毁调用 `rebuild_if_needed(&world)` 须零漂移无损还原；
   - `TickScratch`：单步瞬态草稿纸，Stage 12 结算结束强制调用 `clear_and_assert_empty()`。
3. **13 阶段时钟单点写入铁律（`clock/`）**：
   - 每拍严格按 `Stage 0 (BeginTick) -> ... -> Stage 12 (PublishAndDrop)` 顺序推进；
   - **仅 `Stage 7 (CommitMotion)` 允许写入单位坐标 `pos`**，前置 Stage 6 做连续 $\le 3.5\text{m}$ 防穿隧扫掠；
   - **仅 `Stage 10 (CommitPersistent)` 允许写入战损、弹药、压制、地形弹坑/烟雾与不可逆 `G^work` 消耗**。
4. **手令优先与四维算子真入核**：
   - `IntentSource::DirectOrder` 永久压过自主提案；
   - `math_ops/` 中的 `DeygoutRadioOperator`（无状态重算不锁死半径）、`run_deterministic_multi_capacity_auction`、`project_orthogonal_cover`、`AugmentedArcIntegrator` 与施密特三态姿态已全部接通主循环。
5. **时间机零污染与认知迷雾门禁**：
   - 外置时间机（`time_machine.rs` + `engine.rs`）只在入口拦单跨回合激活、出口生成 `exact_ms` 播片剧本与 `visibility=0, is_prerender_skirt=true` 缓冲环（64m），**严禁篡改底层伤害与消耗数值**；
   - `visibility.rs` 按 `Hidden / Suspected / Spotted / Full` 对敌方裁剪字段，防止透视泄密。
6. **只读预览沙盒与极简服务壳**：
   - `preview.rs` 仅借用 `&WorldState`，严禁任何副作用；
   - `crates/gentactix-server/src/` 严格保持无头薄传输壳（`< 400` 行），禁止在 HTTP 路由内塞入任何仿真物理逻辑或硬编码剧本坐标。

---

## 二 · 代码规范与验收门禁

1. **原生工具优先 & 纯 UTF-8 无 BOM**：读写源码强制使用原生文件工具（禁止在终端用 `Get-Content` / 重定向符改文本），全仓源码与文档维持 UTF-8 无 BOM（`BOM_COUNT=0`）。
2. **跨 Phase 单步停机**：每完成一个阶段，须自办通过 `cargo check --workspace --all-targets`（0 错误 0 警告）与 `cargo test --workspace`（0 失败），随后停机向大王汇报实测数字。
3. **禁止对旧资产回魂**：原 V1 旧代码与历史前端保留在旧分支 `refactor/pure-engine` 上；`v2/pure-engine` 分支仅维护纯净的 V2 引擎，新建前端时直接通过 `gentactix-server` HTTP 接口与 `/api/schema` 契约对接。
