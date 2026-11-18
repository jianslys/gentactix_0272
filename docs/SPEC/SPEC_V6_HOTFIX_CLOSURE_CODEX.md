# SPEC · GENTACTIX V6-Hotfix 收口与实机缺陷清零法典

> **基线提交**：`04e8725` (`feat(v6): complete full closure of deterministic engine and interactive ui`)
> **制定背景**：参谋部在真实 Windows 宿主（1440×900 无头 Chrome CDP 真鼠标交互 + `cargo test --workspace` + 截图目视验收）上对 `04e8725` 完成全量复核后，确认默认态 `0友军` 已消除且时间机核心库已建立，但前后端接线、真实鼠标交互、坐标系与视觉验收仍存在 **6 项硬阻断缺陷**。
> **施工原则**：严格拆分为 `H1 -> H2 -> H3` 三个物理隔离 Phase（对应 `.agent/workflows/v6-hotfix-1.md ~ v6-hotfix-3.md`与 `docs/SPEC/hotfix_plan.txt`），**每完成一个 Phase 必须立刻物理停机汇报，严禁跨 Phase 连跑**。

---

## 一 · 实测核查出的 6 项硬阻断缺陷档案

### 缺陷 1（后端 / `main.rs`）：`#0` 暂停态防篡改与 `TimeMachinePendingQueue` 假接线
- **实测铁证**：
  - `git grep enqueue_operation` 在 `crates/gentactix-server/src/main.rs` 中 **0 命中**（仅在 `crates/gentactix-core/src/time_machine.rs` 内部有定义和单测）。
  - `crates/gentactix-server/src/main.rs` 第 2573–2576 行 `post_commit_turn` 仅调用 `let _due_ops = tm.drain_due_operations(w.subtick as u64 + 1);` 并直接丢弃返回值。
  - 在 Chrome 实时点击 `#btnSplitSquad`（Turn 2 暂停未推演状态）后，后端 `GET /api/state` 返回 `own_pieces` 立即由 `3` 暴涨为 `4`，证明 `post_unit_op` 仍在实时旁路原地篡改 `w.units`。
- **修复规约**：
  1. `post_unit_op` 受理非即时指令（特别在 `w.turn == 0` 或编制调整类 `split_squad / merge_squads / equip_adjust / use_item / deploy_heavy_unit / pack_heavy_unit` 且未绕过时间机闸门时），必须先通过 `st.time_machine.lock().unwrap().enqueue_operation(...)` 入队挂起，返回 `{"ok": true, "queued": true, "ticket": ..., "state": ...}`，**绝对禁止在暂停当回合内提前修改 `w.units` 数量与坐标状态**。
  2. `post_commit_turn` 必须接收 `let due_ops = tm.drain_due_operations(...)` 并在推进 `step_subtick_16s` 前/期间真实执行每一张到期票据对应的 `SimWorld` 变更（同步反映至当帧 `state` 与 `time_machine_frame`），**严禁再写 `let _due_ops = ...` 丢弃结果**。

### 缺陷 2（后端 / `sim_loop.rs`）：四大算子返回值被下划线丢弃 & `format!("u{}", uid)` 错键查找
- **实测铁证**：
  - `crates/gentactix-core/src/sim_loop.rs` 第 2076 行 `let _dominant_edge = DeygoutRadioOperator::find_dominant_knife_edge(&profile);` 被丢弃；
  - 第 2088 行 `let _rethink = filter.evaluate_schmidt_rethink(&[(1, target_w)], 1.0);` 被丢弃；
  - 第 2099 行 `self.direct_unit_plans.get(&format!("u{}", uid))` 使用 `"u0"` 查表，而真实单位 ID 为 `"red-sq-1"` / `"red-tk-1"` 等，命中率为 0；
  - 第 2122 行 `let _allocations = run_deterministic_multi_capacity_auction(bids, &cell_capacities);` 算完直接丢弃。
- **修复规约**：
  1. 将第 2099 行改为查 `&self.units.str_id[uid]`。
  2. 将上述三个结果变量去除前置 `_` 并在 `step_subtick_16s` 中实际消耗：
     - `dominant_edge` 驱动指挥-单位电台链路的地形绕射衰减或下令迟滞；
     - `rethink` 真实控制单位在阈值内保持/重置 ZOH 决策状态或自治行动门限；
     - `allocations` 真实仲裁冲突目标格归属或调整局部推进目标格（并保证 `cargo test --workspace` 全部通过）。

### 缺陷 3（后端 / `main.rs`）：`visible_enemies` 与 `build_time_machine_frame` 的 `fog_level` 口径相反
- **实测铁证**：
  - `serialize_state`（第 411–425 行）规定 `fog_level == 0` 清晰度 `1.0`、`1 -> 0.65`、`2 -> 0.35`（越小越清晰、越大越模糊）。
  - 但 `build_time_machine_frame` 第 114–117 行却写成了 `.filter(|c| c.fog_level >= 2)` 作为 `visible_set`，导致逻辑完全颠倒；且 `serialize_state` 第 403 行在 `cid_opt` 为 `None` 时无条件放行全部 `intel`，使得远方裙边单位 `blue-tk-2` 开局 Turn 0 既在 `GET /api/state` 亮着显示于白桦镇，又在 `time_machine_frame` 里被标记为 `is_prerender_skirt: true, initial_visibility: 0`。
- **修复规约**：统一两处判定口径：`fog_level <= 1`（或明确对齐清晰度与视距半径阈值）才进入开局可见 `visible_enemies` 与 `initial_visibility: 1`；外围缓冲环敌军统一仅入 `prerender_skirt_enemies` 与 `is_prerender_skirt: true, initial_visibility: 0`，杜绝开局 Turn 0 穿帮亮标。

### 缺陷 4（前端 / `web/index.html`）：`#viewport` 截胡点击 + 弹窗挡屏 + 前端坐标系分裂
- **实测铁证**：
  - 第 791–797 行 `$('viewport').addEventListener('pointerdown', ...)` 无条件调用 `$('viewport').setPointerCapture(e.pointerId)`。实机无头 Chrome 真实左键点击地图空白点 `(1100, 520)`，测得 `#viewport` 收到 `1` 次 `click` 而 `#mapSvg` 收到 **`0` 次 `click`**，`#btnPickTaskGoal` / `#btnPickEventGoal` / `UI-09A` 绘测工具在真实人类鼠标操作下全部失效。
  - 第 1244 行 `startMapPick` 仅修改 `$('integratedCmdBox').style.opacity = '0.45'`，未设 `pointerEvents = 'none'`（结束未复原 `'auto'`），居中模态框直接阻塞地图中心战场区点击。
  - `renderUnitsLayer` / `TimeMachineRenderer.step`（第 2200 行）投影用 `100 + (c / gridCols) * 1000`、`90 + (r / gridRows) * 700` 且写死 `gridRows = 64, gridCols = 64`（第 2194、2245 行），而 `triggerPreviewPlan` / `commitTaskToQueue` / `cursorGrid`（第 806、1279、1366、1389 行）却还在用 `/ 10` 和 `* 10`；同时 `meterGrid`、`subbarTurnState`、卡片地域说明、`updateScale`（`/ 8`）与 `toolRuler`（`* 8 / 10`）全部写死 `"8m"`，与后端实际返回 `grid.cell_m = 200` 矛盾。
  - `calculateUnitOrderMetrics(u)` 第 607 行只判中文 `'维持警戒'` / `'待命'`，遇到后端 `'HOLD'` 时回退到第一个敌军算假里程 `0.6 km ~ 3 min`，与静态 `#routesLayer` 假虚线一起穿帮。
  - `TimeMachineRenderer.step` 第 2210 行用屏幕像素圆 `Math.hypot(u.x - mapX, u.y - mapY) < 120` 假装判断裙边敌军显隐。
- **修复规约**：
  1. 改写 `#viewport` 拖拽捕获：仅在 `pointermove` 移动超过 `5px` 确认拖拽后才调 `setPointerCapture`，普通点击不抢夺子元素 `#mapSvg` 的 `click`（或在非拖拽 `pointerup` 时直接完成选点/绘测派发）；`startMapPick` 必须同步加 `pointerEvents = 'none'` 并在 `cancelMapPick` 复原 `'auto'`。
  2. 全文件统一由 `gridToSvg(r, c, grid)` / `svgToGrid(svgX, svgY, grid)` 从后端 `data.grid`（`rows, cols, cell_m`）动态映射，**清零** `/ 10`、`* 10`、硬编码 `/ 64` 与写死 `"8m"` / `/ 8` 魔数，界面格距文案真实绑定 `${cellM}m`。
  3. 修复 `calculateUnitOrderMetrics(u)` 支持 `'HOLD'` / `'hold'` 与空队列返静，隐藏静态假 `#routesLayer`；升级 `TimeMachineRenderer.step` 按真实网格米制距离（`cell_m`）及粒子遮蔽参数调度 `visibility`。
  4. `REORG_EQUIP_OP_MAP` 接通入队语义，收到 `queued: true` 时明确提示已入时间机排程，不在暂停当回合提前凭空多出新兵牌。

### 缺陷 5（前端 / `web/index.html`）：1440×900 实拍图可见 `UI-05/09` 叠字与卡片底部溢出视口
- **实测铁证**：
  - 桌面实拍截图 `1440×900` 中，右上角 `.layer-toggle` 的 `UI-05` 徽标直接覆盖在 `图层` 图标上，`UI-09` 徽标贴顶压撞并逼近竖排 `.map-stamp`。
  - 左侧 `.inspector.unit-card` 高度达到 **`717px`**，点击第二单位在 `y=232` 弹出第 2 张卡后底边落至 `y ≈ 949px > 900px`，底部动作按钮被视口直接裁切半截。
- **修复规约**：重新调整 `.map-tools-pack` / `.tool-pack-unified` / `.layer-toggle` / `.map-stamp` 间距与伪元素标签坐标，保证含 `UI-05`、`UI-08`、`UI-09`、`UI-09A` 在内的全部 **26 个 `[data-ui-id]`** 相互之间及与图标文字零交叉；限制 `.unit-card` 紧凑高度与 `offsetY` 夹紧计算，确保任意展开卡片 `rect.bottom <= window.innerHeight - 12`。

### 缺陷 6（E2E / `tests/e2e/verify_production_ui.mjs`）：跨机不可跑路径与作弊盲区
- **实测铁证**：
  - 第 27、50、52、441–442 行硬编码 `D:/target/debug/gentactix-server.exe`、`C:/Users/rdpuser/...` 与 `D:/scratch/...`，离开原宿主机即报错。
  - 原 E2E 仅查 6 个 ID 的 AABB，漏查 `UI-05`、`UI-08`、`UI-09`、漏查卡片底边溢出视口、且未能检测 `#viewport` 对真实点击 `#mapSvg` 的吞没与 `#0 split_squad` 抢跑变更。
- **修复规约**：全部改为基于仓库根目录相对路径 + `os.tmpdir()` / `scratch/`（若运行环境未预装 `playwright`，支持通过 `package.json` 或清晰提示），补全全部可见 `[data-ui-id]` 两两 AABB、视口溢出断言、`actions -> 在地图标定` 真鼠标点击地图（含屏幕中央海域/战区点与空白区点）后写入 `pendingMapGoal`、以及 Turn 0 点 `btnSplitSquad` 后 `own_pieces` 在未 `commit_turn` 前保持不变、提交后才增兵的严格断言。
