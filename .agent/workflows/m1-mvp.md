---
description: Milestone 1 (MVP) · 512×512 纯二进制 8m《白桦河桥头》内置剧本、真地形渲染与可玩战斗闭环（含 4 阶段硬阻断）
---

# Milestone 1 (MVP) · `512×512`（`8.0m`）白桦河桥头内置剧本与真地形图台合一

> **权威法典**：动工前必须先用 `read_file` 完整阅读 `docs/SPEC/SPEC_M1_MVP_512_BIRCH_RIVER_AND_UNIFIED_TERRAIN_CODEX.md` 与 `docs/SPEC/m1_plan.txt`。
> **分步铁律**：本 Workflow 包含 **Phase M1-1 ~ Phase M1-4** 四个物理隔离阶段。**每执行完一个 Phase 的代码与自测，必须立刻停止（HARD STOP），等待大王回复“确认”后再进入下一 Phase！绝对禁止一次性连跑多个 Phase！**

---

## Phase M1-1 · 后端 `512×512`（`8.0m`）《白桦河桥头》确定性剧本与 V6 尾项清零

### 范围限制
- **仅限修改**：`crates/gentactix-core/src/world_grid.rs`、`crates/gentactix-core/src/sim_loop.rs`、`crates/gentactix-server/src/main.rs`
- **严禁触碰**：`web/index.html`、`tests/e2e/verify_production_ui.mjs`

### 必做事项
1. **清零 V6 后端尾项**（`crates/gentactix-server/src/main.rs`）：
   - 将 `serialize_state` 与 `build_time_machine_frame` 的可见敌军门槛由 `c.fog_level <= 1` 调整为 `c.fog_level <= 2`（配合理性距离判定），解决前沿近距敌军被误关进裙边导致开局 `0接触点` 的问题；
   - 在 `post_unit_op` 的 `deploy_heavy_unit` 与 `pack_heavy_unit` 非即时（`!immediate`）分支中，调用 `tm.enqueue_operation(...)` 后**立即 `return`**，消除继续向下执行 `w.events.push(...)` 的双重入队漏洞。
2. **扩展 `/api/terrain_raw.bin` 二进制编码**（`crates/gentactix-core/src/world_grid.rs`）：
   - 在 `encode_terrain_raw_bin(&self)` 原有 `rows(u32 LE) + cols(u32 LE) + terrain(n * u8) + raw_u16(n * u16 LE)` 之后，追加 `forest_density_u8`（遍历 `self.forest_density`，按 `(d.clamp(0.0, 1.0) * 255.0).round() as u8` 写入 `n` 字节），总长度精确为 `8 + 4 * n` 字节（`512×512` 下为 `1,048,584` 字节）。
3. **实装 `pub fn build_birch_river_512_world(seed: u64) -> SimWorld`**（`crates/gentactix-core/src/sim_loop.rs`）：
   - 保留原 `build_demo_world()`（64×64）不删以兼容老单测；
   - 新建 `build_birch_river_512_world(seed)` 生成 $512 \times 512$、`cell_m = 8.0` 战场：
     * 蜿蜒白桦河主航道（`WATER`）、中央公路大桥（`BRIDGE`，位于 $r \in [246, 250]$ 跨河段）、北侧浅滩涉渡带、东西向主干公路（`HIGHWAY`）与南北支线土路（`DIRT_ROAD`）；
     * 西岸 184 高地、东岸 216 高地、北翼山脊（`MOUNTAIN`）连续平滑高程场（同步写入 `elevation_m` 与 `raw_u16`）及成片连续密度森林（`FOREST` + `forest_density`）；
     * **红1连加强连**（`cmd-co-1` 连长 + `cmd-pl-1 / cmd-pl-2` 两排 6 个步兵班 + `red-tk-1` 坦克排 + `red-mtr-1` 82迫击炮 + `red-eng-1` 工兵破障班 + `red-rec-1` 前沿侦察组 + `depot-red-1`，共 $\ge 10$ 个己方作战棋子）与 **蓝方桥头防御连**（含距离红1排约 $210\text{m}$ 的桥西前哨班 `blue-outpost-1` 确保开局 `visible_enemies >= 1`，以及东岸桥头阵地、重机枪与预设反击计划的装甲组 `blue-tk-1`）。
4. **挂载 `--demo` 与 `mvp_objectives`**（`crates/gentactix-server/src/main.rs`）：
   - `--demo` 启动、`test_state()`（或新增 MVP 专用验证测试，注意保持既有单测断言全部通过）与 `/api/reset` 挂载 `build_birch_river_512_world(0x1944_0622_B1RC_4512)`；
   - 在 `serialize_state` 中输出 `"mvp_objectives"`（含 `OBJ. A / 白桦河大桥` 坐标 `[248, 276]`、控桥方、双方存活人数对比）。
5. **本阶段验收命令**：
   ```powershell
   cargo test --workspace
   ```
   确保 `gentactix-core` 与 `gentactix-server` 全部单测 100% 通过（新增 `test_m1_birch_river_512_world_and_terrain_bin` 断言 `512×512`、`cell_m == 8.0`、`encode_terrain_raw_bin().len() == 1_048_584`、`own_pieces >= 10`、`visible_enemies >= 1`）。

【硬阻断 · Phase M1-1 结束必须立刻停机】：
执行完当前 Phase M1-1 的代码改动与 `cargo test --workspace` 全绿后**必须立刻停止**，汇报改动与测试输出，等待大王回复“确认”后再进入 Phase M1-2，**严禁自动连续执行 Phase M1-2！**

---

## Phase M1-2 · 前端 `/api/terrain_raw.bin` 真 `8m` 印刷地图渲染器（撕掉静态 SVG 画皮）

### 范围限制
- **仅限修改**：`web/index.html`
- **严禁触碰**：`crates/`、`tests/e2e/verify_production_ui.mjs`

### 必做事项
1. **清零 V6 前端尾项**：
   - 修复 `REORG_EQUIP_OP_MAP` Toast 从 `resData.ticket` 解包 `category` 与 `(resData.ticket.activate_tick * 16)` 秒数；将 `.toast` 样式设为 `bottom: 78px` 避免触碰底部比例尺；
   - 将 `BINARY_STEPS` 扩展到 `4096`，`#meterGrid .scale-caption` 动态显示 `${cellM}m / 格 · 二进制比例尺`。
2. **拉取并解析 `/api/terrain_raw.bin` 真地形流**：
   - 在页面启动及 `fetchBackendState` 检测到地形版本变化时，请求 `/api/terrain_raw.bin` 解析 `rows(512), cols(512), terrain(Uint8Array), elevU16(Uint16Array), forestU8(Uint8Array)`；
   - 在 `#terrainLayer` 中移除写死的静态假河流/假森林/假道路 SVG 路径，改为由真实 `512×512` 数据驱动的 **SD2 暖灰军用印刷底图渲染器**（使用 `<image id="trueTerrainRaster" x="0" y="0" width="1200" height="880" preserveAspectRatio="none"/>` 配合矢量等高线/桥梁/路网图层）：
     * 绘制西北光源高程斜坡明暗晕渲（Hillshade）与每隔 $10\text{m}/20\text{m}$ 的真实等高线；
     * 绘制真实蜿蜒白桦河水体与岸线、中央公路大桥（`BRIDGE`）、北浅滩、主干公路（`HIGHWAY`）与支线土路（`DIRT_ROAD`）、以及按 `forestU8` 密度分布的林地色块与针叶树符号；
   - 将 `#labelsLayer` 中的战术地名（`DUBROVKA / 杜布罗夫卡`、`OBJ. A / 白桦河大桥`、`BEREZINO / 白桦镇`、`184 高地`、`216 高地`）全部用 `gridToSvg(r, c)` 绑定到真实格点位置。
3. **海拔、地形名称与 `UI-09A` 测距高差真值化**：
   - 单位卡片 `UI-03C` 的“所在地域”与“海拔”直接读取该单位格点的真实地形名称与 `elevU16` 海拔米数；
   - `UI-09A` 测距尺（`toolRuler`）完成两点拉线时，同时计算并显示直线米制距离与起点终点海拔高差 `Δh`（如 `420 m (Δh: +18m)`）。

【硬阻断 · Phase M1-2 结束必须立刻停机】：
执行完当前 Phase M1-2 的 `web/index.html` 改动后**必须立刻停止**，汇报真地形渲染管线与坐标锚定情况，等待大王回复“确认”后再进入 Phase M1-3，**严禁自动连续执行 Phase M1-3！**

---

## Phase M1-3 · 交火火线脉冲、`UI-08` 实时战报电传与桥头夺控状态闭环

### 范围限制
- **仅限修改**：`web/index.html`（如需补充战报摘要字段可微调 `crates/gentactix-server/src/main.rs`）

### 必做事项
1. **时间机交火火线与弹着反馈**：
   - 在 `TimeMachineRenderer` 播放 16s 步长期间，当 `playback_events` 触发打击或后端返回交火日志时，在发射单位与目标单位插值坐标之间绘制瞬态曳光火线脉冲（`#e8a342` 虚实射束）与弹着爆破圈、飘字减员提示。
2. **`UI-08` 实时战场电传流水与夺控简报**：
   - 将左下角 `UI-08` 扩展为紧凑可查阅的**战场电传日志流**（高度受控不碰左上 `UI-03` 卡片与中间 `UI-07` 比例尺），每次推进 16s 步长后自动追加带时间戳的交火伤亡、压制与命令生效记录；
   - 在顶部副栏或目标徽标处实时刷新 `mvp_objectives`（显示 `OBJ.A 白桦河大桥` 当前控桥态势与红蓝战力对比）。

【硬阻断 · Phase M1-3 结束必须立刻停机】：
执行完当前 Phase M1-3 后**必须立刻停止**，汇报交火视觉与电传战报闭环情况，等待大王回复“确认”后再进入 Phase M1-4，**严禁自动连续执行 Phase M1-4！**

---

## Phase M1-4 · `512×512` MVP 无作弊 E2E 终验、亲眼看图与推送回库

### 范围限制
- **仅限修改**：`tests/e2e/verify_production_ui.mjs`（及实测发现的前后端微调）

### 必做事项
1. **升级 `tests/e2e/verify_production_ui.mjs`**：
   - 新增 MVP 专项断言：
     * `/api/state` 返回 `grid.rows === 512 && grid.cols === 512 && grid.cell_m === 8`；
     * `/api/state` 开局 `own_pieces.length >= 10` 且 `visible_enemies.length >= 1`（绝非 `0接触点`）；
     * `/api/terrain_raw.bin` 响应 `200 OK` 且字节长度为 `1048584`，前端 `#trueTerrainRaster` 已挂载有效图像源；
   - 保持全部 21 项既有严格门禁（6 大禁词清零、26 个 `[data-ui-id]` AABB 零碰撞、1440×900 视口卡片 `bottom <= 900`、真点击多卡、真 CDP 鼠标地图选点、`#0` 暂停态 `btnSplitSquad` 不提前增兵、提交后 `Δpos > 0.01`）100% PASS。
2. **全栈实测、亲眼核图与 Git 推送**：
   ```powershell
   cargo test --workspace
   cargo build --bin gentactix-server
   node tests/e2e/verify_production_ui.mjs
   ```
   - 必须调用图片读取工具亲眼查看 `scratch/production_desktop_screenshot.png`，确认 $512 \times 512$ 真地形（白桦河、大桥、等高线、树林）、红蓝兵牌、多开卡片、右上工具栏、底部比例尺与左下电传框干净美观、零叠字、Zero Console Errors；
   - 执行 `git add`、`git commit` 与 `git push origin main`。

【硬阻断 · Phase M1-4 结束停机汇报】：
完成全量测试、核图无误并成功 `git push origin main` 后立刻停止，向大王呈交完整验收报告！
