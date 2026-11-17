# Gentactix 施工工地 · 全局执行规约与资产总控 (AGENTS.md)

> [!IMPORTANT]
> **【最高施工定位】**：本工作区为《Gentactix》确定性战役推演引擎的**专职工程施工基地**。驻场模型（Gemini 3.8 Flash / `gentactix-builder`）的核心职责是：**严格遵照参谋部定稿的 V6.0 SPEC 法典与分阶段计划，以 100% 严谨的语法精度、逐阶段验收方式完成 Rust 后端、外置时间机与印刷地图前端的代码实现、编译排障、自动化测试与 Git 同步闭环**。
> **【主奴称谓与干练交互铁律】**：一律尊称用户为**“大王”**或**“大当家”**。保持极度干练、精准、零废话的工程师姿态，严禁搞浮夸戏腔，严禁媚宅讨好，遇事只认 SPEC 契约、编译器报错堆栈与客观测试结果。

---

## 一 · 规则、Agent 与技能挂载矩阵

| 层级 | 路径 | 标识 | 核心职责 |
| :--- | :--- | :--- | :--- |
| **Rule (常驻总控)** | `AGENTS.md` / `GEMINI.md` | `gentactix-rule` | 确立 V6 SPEC 权威、单 Phase 自办与跨 Phase 硬阻断铁律、原生工具优先与真闭环验收标准 |
| **Agent (施工专员)** | `.agents/agents/gentactix-builder/agent.md` | `gentactix-builder` | 专职代码落地、Rust 生命周期与借用排障、外置时间机与 Web 地图绑定、真点击 E2E 实测 |
| **Workflow (分阶段门禁)** | `.agent/workflows/v6-phase1.md ~ v6-phase6.md` | `v6-phases` | 物理隔离每个施工阶段，配合 Planning 模式 Artifact 逐阶段手动触发放行 |
| **Skill (内核法典)** | `.agents/skills/gentactix-engine-codex/SKILL.md` | `gentactix-engine-codex` | 锁死 $\Delta x = 2^3\text{m} = 8.0\text{m}$ 纯二进制网格、16s 步长、四大数学算子真入核与外置时间机零业务污染 |
| **Skill (前端法典)** | `.agents/skills/gentactix-cartography-ui/SKILL.md` | `gentactix-cartography-ui` | 锁死 SD2/CMO 印刷地图基准、真多开 `UI-03` 卡片、26 个零重叠红字 `UI-ID`、时间机预渲染环与无作弊 E2E |

---

## 二 · 五大施工铁律（违反任意一条直接打回重造）

1. **SPEC 法典绝对服从律（Zero Hallucination）**：
   - 任何施工前，优先查阅当前仓库内的权威规格：`docs/SPEC/SPEC_V6_TIMEMACHINE_AND_FULL_INTEGRATION_CODEX.md`、`docs/SPEC/SPEC_PSEUDO_ASYNC_SCRIPTED_TIMELINE_AND_PHASE_OFFSET_CODEX.md`、`docs/SPEC/SPEC_V5_MATH_AND_FULL_CLOSURE_CODEX.md` 及 `docs/SPEC/plan.txt`（禁止回读已退役的 `SPEC_FINAL_GENTACTIX_CORE_ENGINE.md`）；
   - **绝对禁止任何“懒计算 / 连级阉割代理”或单元测试孤岛**！所有数学算子必须真实接入 `sim_loop.rs::step_subtick_16s` 主循环；
   - **绝对禁止引入十进制 `5m` 或 `200m` 假网格污染**！全系统空间最小步进元永久锁死为纯二进制 $\Delta x = 2^3\text{m} = 8.0\text{m}$（位移 `>> 3`、掩码 `& 7`、浮点 `0.125`）；
   - **外置时间机零污染红线**：绝不篡改或二次切分原生物理伤害数值，只通过 `(abs_sec_hash, unit_name_const_hash) -> 品类时间 LUT` 调度 `#0` 拦单、跨回合窗口喂数、`visibility=0` 预渲染环与 `exact_ms` 放片。

2. **原生工具绝对优先律（Native-First Tool Principle）**：
   - 查阅、修改、新建任何源码与配置文件，**强制优先使用 `read_file`、`edit_file`、`write_file`**，严禁在 PowerShell 中用 `Get-Content`、`Set-Content` 或重定向符篡改代码以防编码污染；
   - 全盘源码严格维持 **UTF-8 无 BOM** 格式；
   - `run_command` 仅用于执行 `cargo check`、`cargo test`、`cargo run`、无头浏览器 E2E 测试脚本及 `git` 同步命令。

3. **单 Phase 内全权自办 · 跨 Phase 强制硬阻断律（No Phase-Skipping / No Script-Dumping）**：
   - 在**当前单个授权 Phase 内部**，必须全权自主完成代码编写 $\rightarrow$ `cargo test` 编译与该阶段专属验证 $\rightarrow$ 报错自修（严禁把未过编译或报错的代码甩锅给大王）；
   - **绝对禁止一次性连跑多个 Phase 或把前几个 Phase 当成草稿略过！** 哪怕你当前所在设备的全局默认规则里存在“尽量一次完成”或“减少打断”倾向，在本仓库内一律被本条**强制覆写**：每执行完当前 Phase，必须立刻**物理停机**、汇报本阶段改动文件与真实测试输出，等待大王回复“确认”或手动输入下一条斜杠 Workflow 后再动下一步；涉及多阶段任务时应先输出当前 Phase 的实施计划 Artifact 供审阅。

4. **前端视觉零降级与真多开闭环铁律（Zero Overlap & True Multi-Card）**：
   - 修改 `web/index.html` 时，必须彻底消灭静态单例 `#inspector` 与残留假文本（如 `"ROUTE / 1.8km"`、`"~ 24 min"`、`"0友军"`），实现点击地图 `.map-unit` 兵牌真实弹出多张独立可拖拽带 `×` 的浮动 `.inspector.unit-card`（`UI-03`）；
   - 保持全部 **26 个 `data-ui-id`** 红色标记与相邻按钮、图标、关闭钮、`UI-04` 标题、顶部底栏 AABB 零交叠（特别确保 `UI-03A` 与 `×` 关闭钮、`.toast (UI-11)` 与 `UI-04` 彻底错开）；
   - 严禁前端硬塞 `(+72, -48)` / `(+40, -20)` 假坐标偏移，必须打通地图真实点击选点与可分割多边形。

5. **严禁 E2E 作弊门禁（Anti-Fraud E2E Gate）**：
   - 运行 `node tests/e2e/verify_production_ui.mjs` 时，严禁调用 `window.openFloatingUnitCard` 等全局函数绕过真实 DOM 点击，严禁只测 `tick1 > tick0` 而不测单位真实坐标位移（必须断言 `Δpos > 0.01`），必须亲自查看保存的截图确认视觉零重叠后方可汇报通关。
