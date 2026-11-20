# GenTactix V2 Frontend · 四阶段连续施工总手册 (`docs/FRONTEND_PHASE_PLAN.md`)

> **文档定位**：供协同设备在 `v2/pure-engine` 分支上**一口气连续完成 Phase 1 -> Phase 4** 纯前端闭环工程的自包含施工手册。每过一个 Phase 须跑通当阶段全部门禁测试并本地提交一次，四阶段全部验真后再统一拉齐推送至 `origin/v2/pure-engine`。

---

## 一 · 铁律与操作边界（开干前必读）

1. **分支铁律**：仅在 `v2/pure-engine` 上施工（严禁切到或推送 `main`、`refactor/pure-engine`）。
2. **目录写入边界**：你的全部新增与修改**严格限定在顶层 `frontend/` 目录内**；严禁修改 `crates/`、`data/`、`docs/`、`AGENTS.md`、`GEMINI.md`、`README.md`（后端 Rust 内核由另一台设备按规格第 7 节同步改造）。
3. **阅读范围边界（严防上下文中毒）**：
   - **仅允许按顺序阅读这 5 个文件**：
     1. `AGENTS.md`
     2. `README.md`
     3. `docs/FRONTEND_PHASE_PLAN.md`（本文件）
     4. `docs/SPEC/SPEC_UIO_CLIPBOARD_AND_WAVE_DIRECTIVE_CODEX.md`（唯一业务与交互规格基准）
     5. `FUNCTION_AND_INTERFACE_MANUAL.md`（仅用于对齐 `HttpEngineAdapter` 的类型结构与端点插座）
   - **绝对禁区**：
     - **严禁读取 `art-direction/` 下的任何图片**（包括唯一保留的美术参考图，当前只做功能与交互闭环，绝不碰美术图）；
     - **严禁读取 `docs/research-theory/`**；
     - **严禁读取 `docs/SPEC/` 下除 `SPEC_UIO_CLIPBOARD_AND_WAVE_DIRECTIVE_CODEX.md` 之外的任何 V1 历史文档**。
4. **连续执行模式（无人值守一路干完）**：
   - 从 **Phase 1 -> Phase 2 -> Phase 3 -> Phase 4** 连续自主推进，**不要在 Phase 1/2/3 结束时停下来等待人工确认**；
   - 在工具链、依赖安装、TypeScript 类型检查、单测、生产构建与无头浏览器端到端自测全部全权自办闭环通过后，再一次性交付汇报。
5. **跨平台与编码标准**：
   - 全盘源码与配置严格使用 **UTF-8 无 BOM**（Windows / macOS / Linux 通用，路径处理使用正斜杠或 Node `path` 模块，严禁写入本机绝对路径）；
   - 确保根目录 `.gitignore` 已忽略 `frontend/node_modules/` 与 `frontend/dist/`，严禁把依赖或构建产物提交进 Git。

---

## 二 · 推荐技术栈与工程目录结构

- **技术栈**：`Vite + React 19 + TypeScript (strict) + Vitest + Playwright/浏览器端到端自测`（Canvas 2D 分层绘制沙盘图层 1~7，React UI 承载图层 8 功能面板，轻量只读滤镜叠加承载图层 9 气氛组）。
- **目标目录骨架（`frontend/`）**：

```text
frontend/
├── package.json
├── tsconfig.json
├── vite.config.ts
├── index.html
└── src/
    ├── main.tsx
    ├── App.tsx
    ├── engine/
    │   ├── types.ts                  # 全套强类型（World/Unit/Wave/UIO/StepCache/Playback）
    │   ├── timeMachine.ts            # 1:1 复刻 SplitMix64 + FNV-1a + 5×16 LUT 与时间混淆器
    │   ├── stepCache.ts              # 60 天（324,000 步长）稀疏步长缓存 BTree/Map 桶与阵营迷雾预警
    │   ├── backendAdapter.ts         # 唯一 BackendAdapter 接口定义
    │   ├── localMockSandbox.ts       # 第一阶段内置模拟沙盒 + 16s 逐秒防剧透走表器
    │   └── httpEngineAdapter.ts      # 第二阶段连真后端插座骨架（对齐 FUNCTION_AND_INTERFACE_MANUAL.md）
    ├── store/
    │   ├── uioStore.ts               # 手写板 UIO 四区状态机（顶栏/队列/草稿/归档）+ 宏展开 + 互斥检测
    │   ├── waveStore.ts              # 独立【指挥 UI】波次绘制与 G 场延迟签发状态机
    │   └── layerStore.ts             # Layer 6 手绘 IndexedDB 持久化 + Layer 7/9 独立开关态
    ├── render/
    │   ├── NineLayerViewport.tsx     # 物理 9 层堆栈容器（Z-Index 1..9，严禁渲染 8m 网格线）
    │   └── canvasLayers.ts           # Layer 1~5 与 Layer 7 Canvas 渲染函数
    ├── ui/
    │   ├── TopControlBar.tsx         # 16s 走表控制、#0..#324000 步长跳转与己方前向预警、L7/L9 开关
    │   ├── UioClipboardPanel.tsx     # 手写板（UIO）四区面板（含拖拽/点击重用、冲突标红、远程偏移提示）
    │   ├── CommanderWavePanel.tsx    # 独立【指挥 UI】（波次图例绘制、签发生成波次挂回形针栏）
    │   └── FreehandToolbar.tsx       # Layer 6 手绘工具条（画笔/橡皮/清空/持久化状态）
    └── __tests__/
        ├── phase1_core.test.ts       # 时间混淆、60天 StepCache、远程偏移、逐秒防剧透测试
        ├── phase2_layers.test.ts     # 9层容器、零8m网格线、Layer6持久化、L7/L9开关测试
        └── phase3_uio_wave.test.ts   # SPEC 第6节全套7项交互闭环状态机测试
```

---

## 三 · Phase 1：工程骨架 + `BackendAdapter` + 双哈希时间混淆 + 60 天 `StepCache` + 16s 本地沙盒

### 本阶段必做交付
1. 初始化 `frontend/`（React + TypeScript strict + Vite + Vitest，确保 `package.json` 含 `dev` / `build` / `typecheck` / `test` 脚本），安装全部依赖并验证可启动。
2. **`src/engine/timeMachine.ts`（与后端 `time_machine.rs` 比特级一致）**：
   - 使用 JS `BigInt`（`BigInt.asUintN(64, ...)`）1:1 复刻：
     - `splitmix64(z)`：乘数 `0xbf58476d1ce4e5b9n`、`0x94d049bb133111ebn`，右移 `30, 27, 31`；
     - `fnv1a_64(s)`：初值 `0xcbf29ce484222325n`，质数 `0x100000001b3n`，按 UTF-8 字节迭代；
     - `mixTimeUnitHash(absSec, unitName)`：`splitmix64(splitmix64(absSec) ^ rotl64(fnv1a_64(unitName), 27))`；
   - 1:1 复刻 5 张 16 槽常量表（槽位 `Number(hashM & 0x0fn)`）：
     - `LUT_BULLET`：`[{0,1,320},{0,1,580},{0,1,840},{0,2,1120},{0,2,1450},{0,2,1780},{0,2,2010},{0,3,2320},{0,3,2650},{0,3,2980},{0,3,3210},{0,4,3450},{0,4,3680},{0,4,3910},{0,4,4150},{0,4,4380}]`
     - `LUT_SHELL`：`[{1,4,4200},{1,5,5350},{1,6,6480},{1,7,7620},{1,8,8740},{1,9,9860},{1,10,10950},{1,11,11800},{1,12,12650},{1,13,13420},{1,14,14190},{1,15,14920},{1,16,15680},{1,5,5720},{1,8,8910},{1,11,11950}]`
     - `LUT_COMMAND`：`[{0,3,3100},{0,4,4250},{0,5,5400},{0,6,6600},{0,7,7800},{0,8,8950},{0,9,9980},{0,10,10850},{0,11,11720},{0,12,12550},{1,2,2100},{1,3,3300},{1,4,4500},{0,4,4680},{0,7,7420},{0,9,9650}]`
     - `LUT_EVENT`：`[{0,5,5200},{0,6,6400},{0,7,7700},{0,8,8900},{0,9,9950},{0,10,10880},{0,11,11750},{0,12,12410},{0,13,13200},{0,14,14100},{0,15,15050},{0,16,15900},{1,4,4300},{1,6,6700},{1,8,8850},{0,12,12890}]`
     - `LUT_MOVEMENT`：`[{0,1,1100},{0,1,1450},{0,2,2200},{0,2,2600},{0,3,3150},{0,3,3550},{0,4,4100},{0,4,4500},{0,5,5150},{0,5,5650},{0,6,6100},{0,6,6600},{0,2,2450},{0,3,3750},{0,4,4850},{0,5,5900}]`
3. **`src/engine/stepCache.ts`（60 天稀疏步长缓存）**：
   - `TICK_SECONDS = 16`，`MAX_STEP_CACHE_HORIZON_TICKS = 324_000`（`60 * 86400 / 16`）；
   - 基于 `Map<number, ScheduledStepEvent[]>` 稀疏存储（空白回合零占用）；
   - 实现 `schedule(currentTick, event)`（拒绝越界 `< currentTick` 或 `> currentTick + 324_000`）、`lookahead(targetTick, observerTeam)`（**严格迷雾过滤：只返回 `event.team === observerTeam` 的己方预警**，绝不泄露敌方未爆物）、`drainDue(tick)`、`cancel(eventId)`。
4. **`src/engine/backendAdapter.ts` + `localMockSandbox.ts` + `httpEngineAdapter.ts`**：
   - 全前端仅通过 `BackendAdapter` 读写状态、提交签发队列、签发波次、推进 `16s` 时钟、查询前向预警；
   - **唯一时间混淆口径（防第二阶段联调双重延迟）**：`LocalMockSandboxAdapter` 在阶段 1 内部按“唯一一次”双哈希混淆 + 远程偏移写入 `StepCache`；预留的 `HttpEngineAdapter` 则在玩家签发时**立刻**把展开后的原子指令发往后端 `/api/command`，由后端唯一算混淆/远程偏移并直接返回 `deferred_until_tick`；
   - **预置测试世界（100% 对齐 SPEC 6.1）**：
     - 指挥官：1 名红方排长（`Platoon HQ`，位置如 `(160, 240)`，直接指挥半径 `140m`）；
     - 2 个直属步兵班（位于直接指挥半径内：`1 班` 距 HQ `64m`，`2 班` 距 HQ `96m`）；
     - 1 个远程步兵班（`3 班` 位于直接指挥半径外，距 HQ `240m`，自动触发远程偏移提示与电台/传令兵延迟拍数）；
     - 地面散落物资（Items）、散落武器（Weapons）、阵亡尸体（Corpses）；
     - 顶栏回形针区预置 `>= 2` 张上级指挥波次纸条（含各自预定炮击区、友军冲锋箭头、许可 AO 边界、$G$ 场生效回合）；
     - `#0` 回合预埋将在 `#4` 回合某坐标起爆的己方定时炸药事件入 `StepCache[4]`（推演到 `#3` 时明确预警“下一回合 `#4` 定时炸药即将爆炸”，进 `#4` 准时引爆并生成 L2 弹坑破坏）。
   - **逐秒防剧透走表器（`1s..16s`）**：
     - 当推进至新步长（如 `#1`），回合初算定本拍 `[0..16000ms]` 剧本，同时保存 `baselineAtStartOfTick`；
     - 在 `0ms -> 16000ms` 走表期间，`getProgressiveSnapshot(elapsedMs)` 严格按 `elapsedMs` 逐秒插值位置与进度，到达对应事件 `exactMs` 时才同步跳字扣血/减员/扣弹药/生成弹坑，`t = 0s` 绝不提前剧透 `t = 16s` 终局值。

### Phase 1 门禁（必须全部通过并提交本地 commit）
- `npm run typecheck` 通过（0 错误）；
- `npm test` 通过：`timeMachine` 确定性、60 天边界（`#0` -> `#324000` 合法、`#324001` 拒绝）、`#3` 窥视 `#4` 爆炸且过滤敌方预警、远程指挥偏移计算、`t=0s` 与 `t=16000ms` 数值渐进零剧透；
- 本地提交 `chore(frontend-p1): init frontend engine, BackendAdapter, 60-day StepCache, and 16s sandbox`（仅含 `frontend/`）。

---

## 四 · Phase 2：九层物理渲染管线（`Layer 1` ~ `Layer 9`）+ 独立开关 + `Layer 6` 本地持久化

### 本阶段必做交付
1. **九层物理 DOM / Canvas 容器栈（`src/render/NineLayerViewport.tsx`）**：
   - 从底到顶真实存在 `9` 个独立物理 DOM 容器，属性 `data-layer="1"` 至 `data-layer="9"`，对应 `z-index: 1..9`，严格按 SPEC 第 2 节分工：
     - **Layer 1（Base Map Color）**：连续预渲染地形底色与等高缓坡/道路/水系着色，**严禁渲染 8m 基础网格线**；
     - **Layer 2（Terrain Destruction）**：弹坑、散兵坑、堑壕、焦土破坏（严格压在 Layer 3 之下）；
     - **Layer 3（Ambient Occlusion）**：全局环境光遮蔽与高程明暗层，叠压在 L1 与 L2 之上；
     - **Layer 4（Entity & Tokens）**：排长指挥官、直属/远程步兵班、地面散落物品、散落武器、阵亡尸体（随 `1s..16s` 走表毫秒戳平滑插值与伤亡/尸体切换）；
     - **Layer 5（Range & Area Rings）**：选中单位的直接指挥圈/无线电圈/射界扇区；以及点击顶栏指挥纸条时实时投射的**波次炮击区、友军冲锋路线箭头、许可 AO 范围边界**；
     - **Layer 6（Freehand Overlay）**：玩家手绘辅助线独立 Canvas 交互层（支持自由划线、颜色/橡皮/清空），**100% 仅存在浏览器 `IndexedDB`（不可用时降级 `localStorage` 双写兜底），绝不经 `BackendAdapter` 发送，按 `F5` 刷新页面必须完好复原**；
     - **Layer 7（World VFX A）**：天气（雨丝/飘雪/战场雾）、日照阴影、照明弹灯光层（位于 L1~L6 之上、**位于 L8 功能面板之下**，不遮挡面板文字），配真实 UI 独立开关；
     - **Layer 8（Functional HUD）**：承载全部交互面板（顶栏走表与预警、UIO 手写板、独立指挥 UI、L6 画笔条、L7/L9 开关按钮）；
     - **Layer 9（Atmosphere VFX B）**：全局场景气氛组（边缘暗角 vignette、胶片颗粒、爆炸瞬间微震屏），位于最顶层（`z-index: 9`, `pointer-events: none`），配真实 UI 独立开关。
2. **地图拾取交互**：
   - 支持点击地图上的单位（选作执行单位）与点击/折线绘制地图坐标/区域（快速绑定为无头指令的 `target` 标的物），且在手写绘制模式下切换至 L6 不误触选兵。

### Phase 2 门禁（必须全部通过并提交本地 commit）
- `npm run typecheck` + `npm run build` + `npm test` 全绿；
- 单测显式断言：9 层容器 `z-index` 1..9 严格递增、L2 低于 L3、L7 低于 L8 低于 L9、无 8m 网格绘制调用、L6 存储读写零泄漏至 `BackendAdapter`、L7/L9 开关可实时切换可见性；
- 本地提交 `feat(frontend-p2): implement 9-layer physical render stack, Layer 6 IndexedDB persistence, and L7/L9 toggles`（仅含 `frontend/`）。

---

## 五 · Phase 3：手写板（`UIO`）四区状态机 + 排长纸条驱动 + 独立【指挥 UI】

### 本阶段必做交付
1. **手写板（`UIO`）四区闭环（`src/store/uioStore.ts` + `src/ui/UioClipboardPanel.tsx`）**：
   - **顶栏回形针区**：只读波次指挥纸条（预置 `>= 2` 张），展示对应 $G$ 场将于何时生效；玩家排队列完全**无需对齐**顶栏纸条；点击任一纸条在 **Layer 5** 投射该波次炮击区、友军箭头、许可 AO（再次点击可取消高亮）。
   - **无头指令创建与宏展开**：
     - 支持创建“单任务（Move / AttackPos / AttackUnit / BuildWork / PopSmoke 等）”或“事件宏（如：前出占领 -> 构筑散兵坑 -> 按纪律警戒）”；
     - 事件宏带 `[startTime, endTime]`（秒/拍），展开成多条原子指令并在结束时刻自动补一条 `hold_unit`；
     - 若需延迟指令，支持直接对起止时间施加偏移/乘数。
   - **任务队列栏死门禁与实时检测**：
     - **未绑定门禁**：无头指令缺少 `unitId` 或缺少 `target`（需要标的物的类型）时，尝试拖入/移入【任务队列栏】**必须当场拦截**，弹出明确提示并只允许留在【草稿栏】；
     - **排长本人靠纸条动**：UI 上允许选中排长指挥官本人编写移动纸条；地图点击右键直接绕过手写板驱动排长的入口禁止存在；
     - **远程指挥明牌偏移提示**：一旦绑定的单位是直接指挥范围外（如 `3 班`），纸条卡片立刻显示明确中文提示（例如：`远程指挥：超出直接指挥半径，预计延迟 +2 拍 (+32s)`）；
     - **同单位时间槽互斥实时标红**：在同一单位时间区间 $[T_{\text{start}}, T_{\text{end}}]$ 重叠时，冲突纸条卡片实时标红高亮提示具体冲突时段，并**禁用【签发任务队列】按钮**；
     - **双重签发锁死**：仅放入任务队列栏绝不发给沙盒执行；点击【签发任务队列】后队列当场永久锁死不可撤销，按双哈希时间混淆拖延 `#1/#2` 写入 60 天 `StepCache` 并进入走表激活。
   - **草稿栏**：无任何约束，既可放未绑定草稿，也可从已签发或归档纸条“复制副本到草稿栏”当模板复用。
   - **归档栏**：队列指令全部执行完毕后自动流转进归档栏；支持**鼠标拖拽**（同时配一键“拖回任务队列”辅助按钮）将归档纸条放回任务队列栏，重检时间槽无冲突后再次【签发任务队列】。
2. **独立【指挥 UI】（`src/ui/CommanderWavePanel.tsx`）**：
   - 与手写板 UIO 物理分离的独立面板/标签页（可切换进入地图绘制模式）；
   - 支持玩家亲自在地图上绘制至少三类波次图例：**预定炮击区（多边形/圆）、友军冲锋箭头（折线）、许可 AO 区域（范围框）**；
   - 点击一次**【签发波次】**后，按直接/远程通联计算真实 $G$ 场生效回合写入 `StepCache`，并自动在手写板顶栏回形针区新增一张对应的只读指挥波次纸条，可立即点开投射到 Layer 5。

### Phase 3 门禁（必须全部通过并提交本地 commit）
- `npm run typecheck` + `npm test`（完整覆盖 SPEC 6.1 全部 7 项核心闭环的自动化用例）+ `npm run build` 全部通过；
- 本地提交 `feat(frontend-p3): complete UIO 4-lane state machine, macro expander, and Commander Wave UI`（仅含 `frontend/`）。

---

## 六 · Phase 4：全套端到端真实交互验收 + 推送回 `origin/v2/pure-engine`

### 本阶段必做交付
1. **端到端实机/浏览器验证**：
   - 拉起 Vite 本地服务，用真实无头浏览器（如系统已装 Playwright/Chromium/Edge，否则自行安装 `@playwright/test` 或运行无头验证脚本）对生产构建页面做真实点击交互验证，逐项核实：
     1. 9 个物理图层容器 `data-layer="1..9"` 均挂载且 `z-index` 严格有序，无 8m 基础网格；
     2. 未绑定无头指令移入任务队列被拦截并留在草稿栏；
     3. 绑定远程班组时显示明确中文远程延迟偏移文案，且排长可签发移动纸条移动；
     4. 同单位时间重叠实时标红冲突并锁死【签发任务队列】；
     5. 双重签发后不可撤销并写入 `StepCache`；
     6. 点击“快进至 `#3`”时界面明确展示“下一回合 `#4` 己方定时炸药即将爆炸”前向预警，进入 `#4` 走表到达引爆毫秒时准时爆破并在 Layer 2 留下弹坑；
     7. 16s 走表期间 `t=0s` 与 `t=16s` 之间可见平滑渐进变化，不提前剧透终点数值；
     8. 指令完成后自动进归档栏并可拖回任务队列再次签发；
     9. 点击顶栏回形针波次纸条 + 在独立【指挥 UI】新签发波次，Layer 5 实时画出炮击区、冲锋箭头与许可 AO；
     10. 在 Layer 6 手绘一笔后执行真实 **`page.reload()`（等效 F5 刷新）**，验证手绘线条完好无损保留；
     11. 点击 Layer 7 与 Layer 9 开关按钮，图层渲染真实启停变化；
     12. 全程浏览器控制台 `console.error` 与 `pageerror` 为 **0**。
2. **清洁仓库与编码审计**：
   - 检查 `frontend/` 下所有文本源码 **UTF-8 无 BOM**（`BOM_COUNT=0`）；
   - 确认 `git status` 未把 `node_modules/`、`dist/` 或测试临时截图带入仓库，且 `git diff --name-only e19aecc..HEAD` 仅包含 `frontend/` 目录文件（严禁动 `crates/`、`data/`、`docs/`）。
3. **同步推送 `origin/v2/pure-engine`**：
   - 执行：
     ```bash
     git fetch origin
     git pull --rebase origin v2/pure-engine
     npm test && npm run build
     git push origin v2/pure-engine
     ```
   - 保持本地预览端口运行（或给出干净启动命令与访问 URL），向大王汇报：**每个 Phase 提交哈希、全套单测与浏览器端到端实测结果、F5 持久化实测结果、0 控制台报错确认、本地预览地址**。
