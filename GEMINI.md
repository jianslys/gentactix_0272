# GenTactix V2 Pure Engine · 多端协同施工规约 (`AGENTS.md` / `GEMINI.md`)

> **适用范围**：本文件随仓库跨设备生效（Windows / macOS / Linux 通用，严禁写入本机绝对路径）。任何接入本仓库的 AI 协作者须严守以下工作方式与架构边界。

---

## 一 · 交互与执行纪律

1. **称谓与语言**：一律尊称用户为**“大王”**，用平实干练的中文直说事实、数据与编译器/测试实证；严禁虚伪奉承或冗长戏腔；如大王判断或设计有漏洞，必须直接摆出技术依据指正。
2. **默认讨论，指令才动手**：未经大王明确下令执行（如“开干 / 动手 / 按这个改”），一律先做方案分析与逻辑对齐，不擅自修改代码或提交；下令执行后，在**当前授权的单个任务/阶段内全权自办闭环**（编码 → 编译 → 自测 → 排障），严禁让大王当人肉终端代跑报错脚本；阶段完成后停机汇报并等待下一指令。
3. **仅报实测事实**：只汇报本轮亲眼看过的源码、命令退出码与真实测试结果；禁止凭记忆猜测文件路径、行号或测试数量。

---

## 二 · 新设备 AI 首次接手阅读顺序（防上下文中毒）

首次在任何设备上启动协作时，**按以下精确顺序阅读**，不要一上来扎进历史目录：
1. `AGENTS.md`（本文件 — 协作与架构红线）
2. `README.md`（仓库当前结构、编译与启停命令）
3. `FUNCTION_AND_INTERFACE_MANUAL.md`（`gentactix-core` 12 模块、`TacticalCommand` 15 类指令、`gentactix-server` 18 条路由契约）
4. **前端 / 美术专项任务直达必读**：若任务涉及 `frontend/` 的 UI、美术贴图、Canvas 绘制或交互扩展，**必须先读 [docs/AI_FOOLPROOF_PLAYBOOK.md](docs/AI_FOOLPROOF_PLAYBOOK.md) 与 [docs/FRONTEND_FOOLPROOF_HANDBOOK.md](docs/FRONTEND_FOOLPROOF_HANDBOOK.md)**，严格遵守零 404、零 8m 网格、UI ID 唯一性与 `useGame()` 规范。
5. **严禁默认读取美术图**：日常引擎、架构、数据契约与逻辑开发阶段禁止读取任何图片；*仅当大王明确下令执行 UI / 视觉美术还原任务时*，才按需读取唯一允许的美术参考图 `art-direction/2f89d1e3-b843-410a-b72a-4001b2c64ffc.png`。

> [!NOTE]
> `docs/SPEC/SPEC_UIO_CLIPBOARD_AND_WAVE_DIRECTIVE_CODEX.md` 为 **V2 现役前后端权威演进规格**（前端 9 层栈 / 60 天步长缓存 / UIO 手写板 / 后端零 P2P 欧拉连续路径场、坦克绝不过穿内部破片与步兵按个人记账）；`docs/FRONTEND_PHASE_PLAN.md` 为**前端四阶段连续施工总手册**（供多端协同直接按 Phase 1~4 一气呵成落地 `frontend/`）；`docs/SPEC/` 下其余历史文档为 V1 时期底稿留档，冲突时以现役 V2 代码、手册及上述 V2 规格为准（旧 V1 代码与旧前端已归档于 `refactor/pure-engine` 与 `main` 分支，严禁搬回现役代码）。

---

## 三 · 多设备 Git 协作协议

1. **核心集成与归档分支**：
   - **唯一日常开发集成分支**：`v2/pure-engine`；
   - **前端与美术干净开发分支**：`v2/frontend-dev`（基于最新 `v2/pure-engine` 基线切出，供前端/美术及轻量 AI 专心作业后合回 `v2/pure-engine`）；
   - **禁止直接推送**：`main` 与 `refactor/pure-engine`（保留作历史回溯基线）。
2. **开机拉齐流程**：
   - 开始新任务前务必先同步远端：`git fetch origin`；
   - 两台设备**错峰顺次开发**：执行 `git pull --rebase origin v2/pure-engine` 拉到最新提交后再动刀；
   - 两台设备**同时并行开发**：基于 `origin/v2/pure-engine` 切出职责分支（如 `feat/<topic>` 或 `dev/<machine>-<topic>`），单边验证全绿后再合入 `v2/pure-engine`。
3. **目录分工防冲突边界**：
   - **后端引擎域**（`crates/gentactix-core/`、`crates/gentactix-server/`）：`gentactix-server/src/` 严格维持薄传输壳（`< 400` 行），禁止在 HTTP handler 内写游戏物理/坐标硬编码；
   - **未来前端域**（新建时一律放在独立顶层目录 `frontend/`）：严格通过 HTTP API（`/api/schema`、`/api/command`、`/api/clock/step`、`/api/state`、`/api/terrain.bin`、`/api/query/*`）与后端通信，禁止为了前端便捷篡改内核不变量；
   - **契约同步**：任何 HTTP 路由、指令字段或 `data/items_catalog.json` 结构变动，必须在同一提交内更新 `FUNCTION_AND_INTERFACE_MANUAL.md` 与对应测试。
4. **密钥安全铁律**：严禁把 GitHub PAT（`ghp_*`）、`.env` 或本地凭据写入仓库或输出到控制台/回复正文。

---

## 四 · V2 八大核心支柱不变量（改引擎代码必守）

1. **纯二进制 8m 网格与 16s 整数时钟**：最小战术格永久锁死为 `TACTICAL_CELL_SIZE_M = 8.0`（$2^3\text{m}$）；整数时钟永久锁死为 `TICK_SECONDS = 16`（`clock_seconds = epoch_seconds + (tick << 4)`）。
2. **三态严格正交（`state/`）**：`WorldState` 为唯一存盘与 `authoritative_sha256()` 真相源；`DerivedState` 销毁后调 `rebuild_if_needed(&world)` 须无损零漂移重建；`TickScratch` 必须在 Stage 12 调用 `clear_and_assert_empty()`。
3. **13 阶段时钟单点写入（`clock/`）**：严格按 Stage `0..=12` 推进；**仅 `Stage 7` 允许改写坐标 `pos`**（Stage 6 做 $\le 3.5\text{m}$ 连续扫掠防穿隧），**仅 `Stage 10` 允许写入战损/弹药/压制/地形与不可逆 `G^work` 消耗**。
4. **手令优先与四维算子入核**：`IntentSource::DirectOrder` 永久压过自主系统；Deygout 电台无状态重算（不锁死半径）、G 场拍卖、I 场正交掩体、工兵增广弧长与施密特三态姿态均须保持接通。
5. **时间机零污染 & 认知迷雾门禁**：时间机仅调度激活拍与出口 `exact_ms` / 64m 预渲染环（`visibility=0, is_prerender_skirt=true`），严禁修改原生伤害/弹耗；迷雾按 `Hidden / Suspected / Spotted / Full` 裁剪，禁止泄密。
6. **只读预览沙盒**：`preview.rs` 仅借用 `&WorldState`，严禁任何状态突变或版本号自增。

---

## 五 · 编码与提交前必过门禁

- 全仓文本与源码严格保持 **UTF-8 无 BOM**（禁止在终端用脚本重定向写源码，优先用原生文件读写工具）；
- 每次提交与 `git push origin v2/pure-engine` 前，必须实跑以下两项且 **0 错误、0 警告、0 失败**：
  1. `cargo check --workspace --all-targets`
  2. `cargo test --workspace`
