---
description: V6 Phase 1 · 根治算子孤岛：将 V5 四大数学算子真实接入 sim_loop.rs 主循环并单步停机验收
---

# V6 Phase 01 · 算子真正接入 `sim_loop.rs` 主循环

## 严正范围与物理隔离声明
- **本阶段仅限修改以下文件**：
  - `crates/gentactix-core/src/sim_loop.rs`
  - `crates/gentactix-core/src/fields/mod.rs`
  - `crates/gentactix-core/src/geometry/tactical_instance.rs`
- **绝对禁止触碰**：`time_machine.rs`、`crates/gentactix-server/src/main.rs`、`web/index.html`、`tests/e2e/verify_production_ui.mjs`。
- 若大王当前开着 Planning 模式，请先给出仅针对本 Phase 的具体改动计划 Artifact，待批注放行后再改代码。

## 本阶段必做交付
1. 打开并检查 `crates/gentactix-core/src/fields/mod.rs`、`crates/gentactix-core/src/geometry/tactical_instance.rs` 与 `crates/gentactix-core/src/sim_loop.rs`。
2. 在 `SimWorld::step_subtick_16s` 主循环中真实接入调用（消灭孤岛与零调用）：
   - 调用 `fields::run_deterministic_multi_capacity_auction` 驱动下属对 `G_alloc` 的无冲突分配；
   - 调用 `fields::SchmidtPostureState` 与 `ALPHA_BLAST / ALPHA_SUPP` 驱动受击格双时间尺度衰减与敌军源项坍塌；
   - 调用 `fields::DeygoutRadioOperator` 与 `UnitZohDirectiveFilter` 驱动单单位 `<=128B` 稀疏 ZOH 滤波与施密特迟滞重寻思；
   - 调用 `geometry::tactical_instance::AugmentedArcIntegrator` 与 `DetonatorSwitchEntity` 驱动工兵 16s 步长边走边干转正写入 `WorldGrid`（半截退避持久化保留进度）。
3. 跑通专属验收测试：
   ```powershell
   cargo test --manifest-path "D:\tools\gentactix\Cargo.toml" -p gentactix-core
   ```
   确认全部通过并补充验证“主循环调用真实触发算子变化”的测试断言。

【硬阻断 · 强制停机等待确认】：
执行完本 Phase 的代码改动与 `gentactix-core` 全绿单测后必须立刻停止！向大王汇报修改位置、调用链路与完整测试输出，等待大王回复“确认”或手动输入 `/v6-phase2` 后再进入下一阶段，严禁自动连续执行！
