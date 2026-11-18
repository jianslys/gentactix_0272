---
description: V6 Hotfix Phase 1 · 后端真接线、#0 时间机入队闭环与 fog_level 口径统一
---

# V6 Hotfix Phase 1 · 后端真接线与 `#0` 入队闭环

## 严正范围与物理隔离声明
- **本阶段仅限修改以下 Rust 源码**：
  - `crates/gentactix-core/src/sim_loop.rs`
  - `crates/gentactix-core/src/fields/mod.rs`
  - `crates/gentactix-core/src/geometry/tactical_instance.rs`
  - `crates/gentactix-core/src/time_machine.rs`
  - `crates/gentactix-server/src/main.rs`
- **绝对禁止触碰**：`web/index.html`、`tests/e2e/verify_production_ui.mjs`。
- 本阶段**保持 `build_demo_world()` 现有测试基准尺度不变**（不改动其 `64x64` 布局以免破坏既有核测试；该重构留待后续 Milestone）。
- 详细法典请先阅读 `docs/SPEC/SPEC_V6_HOTFIX_CLOSURE_CODEX.md`（缺陷 1~3）与 `docs/SPEC/hotfix_plan.txt`。

## 本阶段必做交付
1. **`#0 暂停态零篡改 & TimeMachinePendingQueue 真接线`**（`crates/gentactix-server/src/main.rs`）：
   - 在 `post_unit_op` 中为整编/非即时调整操作真实调用 `st.time_machine.lock().unwrap().enqueue_operation(...)`，返回 `{"ok": true, "queued": true, "ticket": ...}`，**严禁在暂停/未推演当回合原地直接修改 `w.units` 数量与实机编制**；
   - 在 `post_commit_turn` 中删除 `let _due_ops = ...` 占位丢弃写法，真实消费 `tm.drain_due_operations(...)` 返回票据并在 `SimWorld` 中生效。
2. **清除 `sim_loop.rs` 算子假调用与错位 ID 查询**：
   - 修复第 2099 行 `self.direct_unit_plans.get(&format!("u{}", uid))` 为 `&self.units.str_id[uid]`；
   - 真实消费 `dominant_edge`、`rethink`、`allocations` 三个计算结果，严禁再保留 `let _dominant_edge` / `let _rethink` / `let _allocations`。
3. **统一 `visible_enemies` 与 `time_machine_frame` 的视野/裙边口径**：
   - 修正 `build_time_machine_frame` 中 `.filter(|c| c.fog_level >= 2)` 与 `serialize_state` 清晰度逻辑相反的错误（并统一未指定 `commander_id` 时的远距裙边过滤），确保 Turn 0 初态开局 `blue-tk-2` 处于隐藏裙边态，不提前明牌亮出。
4. **运行验收命令（仓库根目录执行）**：
   ```powershell
   cargo test --workspace
   git grep -n -E "let _due_ops|let _allocations|let _dominant_edge|let _rethink|format!\(\"u\{\}\"" crates/
   ```
   确保单测 100% 通过（含新增 `#0 split_squad 入队不增兵 -> commit_turn 生效` 与 `blue-tk-2` 开局裙边验证）且 `git grep` 返回 **0 命中**。

【硬阻断 · 强制停机等待确认】：
执行完当前 Phase 1 的代码改动、单测与 grep 自查后必须立刻停止！向大王汇报修改位置、核心单测输出与 0-hit grep 结果，等待大王回复“确认”或手动输入 `/v6-hotfix-2` 后再进入下一阶段，严禁自动连续执行！
