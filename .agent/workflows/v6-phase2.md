---
description: V6 Phase 2 · 实装外置伪装异步时间机 (time_machine.rs) 与五张品类时间查找表并单步停机验收
---

# V6 Phase 02 · 外置伪装异步“时间机 (Time Machine)”与五张品类 LUT

## 严正范围与物理隔离声明
- **本阶段前置参考**：仅精读 `docs/SPEC/SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md` 与 `docs/SPEC/SPEC_V6_TIMEMACHINE_AND_FULL_INTEGRATION_CODEX.md` 第一章。
- **本阶段仅限修改以下文件**：
  - `crates/gentactix-core/src/time_machine.rs`（新建）
  - `crates/gentactix-core/src/lib.rs`（导出）
  - `crates/gentactix-server/src/main.rs`（挂接时间机队列与Frame导出，不改默认视角业务逻辑）
- **绝对禁止**：严禁修改原生物理伤害计算数值或人为切分 `exact_hp_loss`；严禁触碰 `web/index.html` 与 `tests/e2e/`。
- 若处于 Planning 模式，先提交本 Phase 实施清单 Artifact 等待批注放行后再写代码。

## 本阶段必做交付
1. 在 `time_machine.rs` 实现极简确定性双哈希与五张 16 槽常量表：
   - `mix_time_unit_hash(abs_sec: u64, unit_name: &str) -> u64`（`SplitMix64(abs_sec)` 结合常量 `FNV1a_64(unit_name)`）；
   - 五张独立 16 槽常量时间表：`LUT_BULLET`、`LUT_SHELL`、`LUT_COMMAND`、`LUT_EVENT`、`LUT_MOVEMENT`（只算激活秒 `tau_phase` 与触发毫秒 `exact_ms`，绝不改伤害）。
2. 时间机对内调度（`TimeMachinePendingQueue`）：
   - 拦截 `#0` 回合（严格 `1..=16` 秒，无 `0s`）下达的一切改状态指令（射击、移动、撤退、九大事件、起爆开关、AO 写 `G0`、整编与调谐），按准备时间 `T_prep` 拖进 `#1 / #2 / #3`；
   - 任务无须绑定 `16s` 正整数倍：交界时只把落进 `#k[1..16]` 窗口内的那一截任务数据切出来喂给步长引擎。
3. 时间机对外导出（`TimeMachineFramePayload`）：
   - 输出 `particle_tracks`（己方、可见敌军 `initial_visibility: 1`、以及我方 `I_intel` 外围 16s 最大机动半径缓冲环 `is_prerender_skirt: true, initial_visibility: 0`）；
   - 将步长引擎算完的结果原封不动封装为带 `exact_ms` 的 `playback_events`（携带原装 `exact_hp_loss` / `exact_supp_add`）。
4. 运行全量 Rust 测试并补齐时间机单测（`#0` 冻结、伤害原装一致、双哈希分表零误差复盘，在仓库根目录执行）：
   ```powershell
   cargo test --workspace
   ```

【硬阻断 · 强制停机等待确认】：
执行完本 Phase 的时间机模块与全量 Rust 测试后必须立刻停止！向大王汇报改动摘要与全部测试通过结果，等待大王回复“确认”或手动输入 `/v6-phase3` 后再继续，严禁自动连跑！
