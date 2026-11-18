# SPEC · GENTACTIX Milestone 1 (MVP) — 512×512 纯二进制 8m《白桦河桥头》内置剧本与真地形图台合一法典

> **基线提交**：`6718f82` (`fix(v6-hotfix): complete full closure of backend contracts, ui grid, and portable e2e tests`)
> **里程碑目标**：彻底告别 `64×64 / 200m` 假演示网格、`E:\商单1100` 外部文件强依赖、以及前后端“两张皮”静态 SVG 画板；在引擎内直接用确定性 `SplitMix64` 算法内生 **`512×512` 格（$\Delta x = 2^3\text{m} = 8.0\text{m}$，全幅 $4096\text{m} \times 4096\text{m}$）《白桦河桥头》标准连级加强对抗剧本**，前端通过 `/api/terrain_raw.bin` 实时渲染与后端物理网格 `1:1` 对齐的暖灰印刷等高线、河道公路桥与连续森林，打通从“下令部署 $\rightarrow$ 16s 时间机走表交火 $\rightarrow$ 电传战报 $\rightarrow$ 桥头夺控/战损结算”的完整可玩 MVP 闭环。
> **施工纪律**：本规格绑定 `docs/SPEC/m1_plan.txt` 与单一工作流 `.agent/workflows/m1-mvp.md`（内含 `Phase M1-1 ~ Phase M1-4` 四个阶段）。**每完成一个 Phase 必须立刻物理停机汇报，等待大王确认后再进入下一 Phase，严禁跨阶段连续执行！**

---

## 一 · 顺手清零 V6-Hotfix 遗留的 4 根收尾线头（并入 Phase M1-1 / M1-2）

在进入 MVP 建设时，必须同步清掉 `6718f82` 实机审查留下的 4 处细节瑕疵：
1. **近距接触敌军被 `fog_level > 1` 误杀成 `0接触点`（`crates/gentactix-server/src/main.rs`）**：
   - 现状：`serialize_state` 与 `build_time_machine_frame` 将门槛卡在 `fog_level <= 1`，导致距离友军仅 $1.0\text{km}$ 内、处于粗略接触等级（`fog_level == 2`）的前沿敌军也被关进 `prerender_skirt`，开局顶栏显示 `0接触点`。
   - 修正：将前沿已知接触门槛放宽为 `c.fog_level <= 2` 且在友军有效侦察/通信距离（如 $2.5\text{km}$）内即列入 `visible_enemies`（`initial_visibility: 1, is_prerender_skirt: false`）；仅将 `fog_level > 2` 或超出直视侦察圈但处于外围缓冲环内的敌军归入 `prerender_skirt_enemies`（`initial_visibility: 0, is_prerender_skirt: true`）。
2. **`deploy_heavy_unit` / `pack_heavy_unit` 入队后漏写早退 `return`（`crates/gentactix-server/src/main.rs`）**：
   - 现状：在 `!immediate` 分支调用 `tm.enqueue_operation(...)` 后未立即 `return`，继续向下执行了 `w.events.push(...)` 导致双重入队。
   - 修正：与 `split_squad` 一致，入队后立即返回 `{"ok": true, "queued": true, "ticket": ticket, "state": st_json}`。
3. **Toast 排程文案解包字段错位 & 压比例尺（`web/index.html`）**：
   - 现状：前端读取 `resData.category / resData.due_sec`（实际在 `resData.ticket.category` 与 `resData.ticket.activate_tick * 16`），导致弹出 `"类别: op, 预计于第 0 秒生效"`；且 `.toast` (`bottom: 52px`) 与底部比例尺（`UI-07`）边缘轻微贴碰。
   - 修正：从 `resData.ticket` 正确解包类别与预计生效秒数；将 `.toast` 上移至 `bottom: 78px`，彻底让出底部 `UI-07` 比例尺。
4. **残留硬编码 `"8m"` 文案与二进制比例尺档位上限（`web/index.html`）**：
   - 修正：`#meterGrid .scale-caption` 与单位卡 `u.terrain` 动态绑定后端 `grid.cell_m` 与格点真实地形名称（平原/林地/高地/公路/河岸橋头）；`BINARY_STEPS` 扩展为 `[16, 32, 64, 128, 256, 512, 1024, 2048, 4096]`。

---

## 二 · Phase M1-1：后端内置 `512×512`（`8.0m`）《白桦河桥头》确定性剧本与胜负结算契约

### 1. 新增确定性战场构造函数 `build_birch_river_512_world(seed: u64) -> SimWorld`
在 `crates/gentactix-core/src/sim_loop.rs` 中新增公开函数 `pub fn build_birch_river_512_world(seed: u64) -> SimWorld`（默认种子 `0x1944_0622_B1RC_4512`），**零外部磁盘文件依赖**，纯 CPU 确定性生成 $512 \times 512$、$\Delta x = 8.0\text{m}$（$4096\text{m} \times 4096\text{m}$）战术流形：
- **水文与渡口拓扑**：
  - **白桦河主航道（`WATER`）**：自北向南蜿蜒贯穿全图，中心列随行号 `r` 按确定性正弦缓弯分布：$c_{\text{river}}(r) = 276 + \lfloor 28 \sin(r / 56.0) + 12 \cos(r / 29.0) \rfloor$，河宽 $10 \sim 14$ 格（$80\text{m} \sim 112\text{m}$），河床高程 $112.0\text{m}$。
  - **中央公路大桥（`BRIDGE`，“OBJ. A / 白桦河大桥”）**：位于 $r \in [246, 250]$ 横跨白桦河水面的格点，地形设为 `BRIDGE`（可通行装甲与步兵，可作为爆破/夺控核心目标）。
  - **北侧浅滩涉渡点（`DIRT_ROAD` / 浅水涉渡带）**：位于 $r \in [118, 121]$ 横跨河道处，供轻步兵/侦察兵迂回。
- **道路网（`HIGHWAY` & `DIRT_ROAD`）**：
  - 主干公路（`HIGHWAY`）：从西侧红方出发区 $(260, 40)$ 经杜布罗夫卡村 $(248, 175)$ 过中央大桥 $(248, 280)$ 直通东岸白桦镇 $(236, 385)$；
  - 南北向支线土路（`DIRT_ROAD`）：连接北侧林场 $(120, 190)$、杜布罗夫卡 $(248, 175)$ 与南侧绿原高地 $(380, 210)$，以及东岸沿河巡逻路。
- **高程场（`elevation_m` & `raw_u16`）与高地**：
  - 全图基准海拔 $118\text{m} \sim 228\text{m}$，利用确定性多频平滑核叠加生成：
    - 西岸 **184 高地**（中心 $(195, 155)$，峰值约 $184\text{m}$）；
    - 东岸 **216 主高地**（中心 $(310, 355)$，峰值约 $216\text{m}$，俯瞰河桥东口）；
    - 北翼陡峭山脊（`MOUNTAIN`，局部海拔 $>220\text{m}$，位于 $(45..80, 290..340)$）；
  - 同步填充 `grid.elevation_m[idx]` 与 `grid.raw_u16[idx] = elevation_m[idx].round() as u16`，保证 `/api/terrain_raw.bin` 输出精确高程。
- **连续密度森林（`FOREST` & `forest_density`）**：
  - 在北部林区、河岸林缘掩护带、216 高地反斜面布置成片林区（`terrain = FOREST`，`forest_density` 分布在 $0.35 \sim 0.95$），并调用 `grid.sync_feature_flags_from_terrain()` 与 `grid.refresh_attrition()`。

### 2. 红蓝双方连级加强实战编制（完整 4 本账 + 电台/器材）
在 `build_birch_river_512_world` 中配置真实可打的红蓝作战序列（双方处于交火/侦察临界距离，推进 $1 \sim 3$ 个脉冲即可接火）：
- **红方（玩家：红1连加强连，约 115 人）**：
  - **连部**：`cmd-co-1`（“红1连连长”，上尉，坐标 $(252, 145)$）+ 直属连部指挥班 `red-co-hq`（配电台、望远镜/测距仪、急救包）；
  - **第 1 步兵排**：排长 `cmd-pl-1` $(246, 215)$，辖 `red-1pl-sq1` $(242, 232)$、`red-1pl-sq2` $(248, 236)$、`red-1pl-sq3` $(254, 232)$（逼近西岸桥头，距桥仅 $350\text{m}$）；
  - **第 2 步兵排**：排长 `cmd-pl-2` $(185, 205)$，辖 `red-2pl-sq1` $(178, 222)$、`red-2pl-sq2` $(192, 224)$（北翼林缘穿插梯队）；
  - **装甲与重火力支援**：`red-tk-1`（“红1坦支援排”，T-34 中坦组，$(250, 205)$）、`red-mtr-1`（“红82迫击炮组”，配迫击炮与电台，$(258, 175)$）、`red-eng-1`（“红工兵破障班”，配炸药、起爆电缆、地雷、铁锹，$(246, 225)$）、`red-rec-1`（“红前沿侦察组”，$(228, 245)$）；
  - **前线补给站**：`depot-red-1` $(255, 125)$。
- **蓝方（AI 守军：蓝方桥头防御连，配自主守备与反击指令）**：
  - **蓝方连部**：`cmd-blue-co` $(238, 375)$ + **1 排排部** `cmd-blue-pl1` $(246, 305)$；
  - **西岸桥头前哨（开局即处于红方接触视野内！）**：`blue-outpost-1`（“蓝桥西警戒班”，$(247, 262)$，距离红1排仅约 $210\text{m}$，确保开局 `visible_enemies >= 1` 绝不出现 `0接触点`，且首回合即可发生步枪/机枪交火）；
  - **东岸桥头主阵地（部分可见、部分处于预渲染裙边）**：`blue-def-1` $(244, 292)$、`blue-def-2` $(252, 294)$、`blue-hmg-1`（重机枪阵地，$(248, 289)$）；
  - **纵深机动预备队与炮兵（处于预渲染裙边或迷雾）**：`blue-tk-1`（蓝装甲反击组，$(238, 335)$，预设向桥头 `(248, 285)` 机动反击计划）、`blue-art-1`（蓝迫击炮阵地，$(295, 345)$）。

### 3. `--demo` 挂载与既有单测兼容保护
- **重要兼容规则**：保留原 `pub fn build_demo_world() -> SimWorld`（$64 \times 64$）函数体不删，供既有单元测试内部直接调用，确保老测试 100% 不破；
- 在 `crates/gentactix-server/src/main.rs` 中：
  - 将 `--demo` 启动与 `/api/reset` 默认切为调用 **`build_birch_river_512_world(0x1944_0622_B1RC_4512)`**（日志打印 `"Loaded 512x512 (8.0m) Birch River MVP world"`）；
  - 在 `serialize_state` 返回 JSON 中增加 `"mvp_objectives"` 与 `"battle_outcome"` 字段：
    - 统计夺控点 `OBJ. A / 白桦河大桥`（中心 $(248, 276)$ 半径 12 格内双方存活兵力对比）、红蓝双方总伤亡、当前回合数，提供前端顶部与结算面板展示。
- **优化 `/api/terrain_raw.bin` 编码（`world_grid.rs::encode_terrain_raw_bin`）**：
  - 在原有 `rows(u32 LE) + cols(u32 LE) + terrain(n * u8) + raw_u16(n * u16 LE)` 基础上，末尾追加 `forest_density_u8`（`n * u8`，将 `0.0..=1.0` 量化为 `0..=255`），使前端能 `1:1` 画出连续疏密树林；同时确保任何已有调用只读前 `8 + 3*n` 字节也完全向后兼容。

---

## 三 · Phase M1-2：前端撕掉静态画皮 —— 基于 `/api/terrain_raw.bin` 的真 `8m` 地形 Canvas+SVG 合一渲染器

在 `web/index.html` 中，**彻底废除写死在 HTML 里的假山假河假树林静态路径**，改为由后端 `/api/terrain_raw.bin` 驱动的真实 $512 \times 512$ 印刷地图渲染管线：
1. **拉取并解析 `/api/terrain_raw.bin`**：
   - 页面初始化（及 `terrain_revision` 变化时）请求 `GET /api/terrain_raw.bin` 获取 `ArrayBuffer`；
   - 解析 `rows = 512, cols = 512`、`terrain: Uint8Array(n)`、`elevU16: Uint16Array(n)` 及末尾 `forestU8: Uint8Array(n)`，存入前端 `window.__terrainBuffer = { rows, cols, terrain, elevU16, forestU8 }`，同步提供 `sampleElevationAtGrid(r, c)` 与 `sampleTerrainNameAtGrid(r, c)`。
2. **真地形印刷底图合成（保持 SD2 暖灰军用印刷图美学，严禁退化成简陋色块）**：
   - 在 `#terrainLayer` 内挂载一张与 SVG 坐标系（`0 0 1200 880`）严格对齐的高清离屏渲染 `<image id="trueTerrainRaster" x="0" y="0" width="1200" height="880" preserveAspectRatio="none"/>`（内部用 `1024×1024` 或 `1200×880` Canvas 生成 DataURL/BlobURL）叠加矢量要素层：
     - **底色与高程晕渲（Hillshade）**：基于 `elevU16` 差分计算西北光源斜坡明暗，叠合在 `#ddd6ba ~ #cfc7a9` 暖灰印刷底纸上；
     - **真实等高线（Contour Lines）**：每隔 $10\text{m}$ 海拔（主等高线每 $20\text{m}$ 加粗 `#9e9275`，辅等高线 `#b8ae92`）按 `elevU16` 零交叉边精确绘出；
     - **真实连续森林（`FOREST` / `forestU8`）**：在林地格点绘制暖灰军用绿印刷晕染底色（`#b8c2a6` $\sim$ `#94a382`）与疏密对应的针叶林符号簇；
     - **真实白桦河水系（`WATER`）与浅滩**：按 `terrain == 2` 格点绘制普鲁士灰蓝水体（`#8aa6ad` 填充 + `#5d7b85` 岸线双勾边）；
     - **真实道路与白桦河大桥（`HIGHWAY=4`, `DIRT_ROAD=5`, `BRIDGE=6`）**：按格点连通性绘制朱砂褐主干公路、虚线支线土路、以及河道上的双轨桥梁符号；
   - **动态地名注记与夺控旗标（`#labelsLayer`）**：将 `DUBROVKA / 杜布罗夫卡`、`OBJ. A / 白桦河大桥`、`BEREZINO / 白桦镇`、`184 高地`、`216 高地` 的 SVG 坐标全部通过 `gridToSvg(r, c)` 锚定到后端真实格点坐标上！
3. **测距尺、高程剖面与单位卡环境真值化**：
   - 单位卡片 `UI-03C` 的“海拔”与“所在地域”直接从 `__terrainBuffer` 读取单位当前 `(r, c)` 的真实海拔米数与真实地形类别（如 `公路 · 8m流形`、`密林 (掩蔽78%)`、`白桦河大桥`）；
   - `UI-09A` 测距标尺（`toolRuler`）拉线后，不仅显示真实米制距离，还自动采样起点与终点海拔显示高差 `Δh = +24m`！

---

## 四 · Phase M1-3：可玩单局战斗闭环 —— 交火火线、实时战报电传 (`UI-08`) 与桥头夺控状态栏

让大王打开网页就能真刀真枪指挥红1连打一场有来有回的桥头争夺战：
1. **一键预设战法与快捷全连推进体验**：
   - 除了单点单位下达 `UI-03M1` 移动/射击任务外，在 `UI-08` 左下角电传日志框与顶部 `UI-02` 副栏之间打通**实时战况滚动流**：每次点击 `预演 / 推进 16s`（`#play` 或 `pulseStagingBar`），自动调用 `commitSimulationTurn()` 推进一个 `16s` 脉冲并启动 `TimeMachineRenderer` 播片。
2. **时间机交火视觉与 `UI-08` 电传战报联动**：
   - 在 `TimeMachineRenderer` 播放期间，对 `playback_events` 中的每次射击/打击事件，不仅在目标插值位置炸开弹着圈，还从 `source_piece_id` 到 `target_piece_id` 绘出瞬态火线脉冲（曳光弹道线 `#e8a342`），并在左下角 `UI-08` 电传框内追加带毫秒戳的战报流水（如 `[06:30:14.190] 红1排1班 -> 蓝桥西警戒班 步机枪齐射，歼敌 1，压制 +12%`）。
3. **桥头夺控进度与战局结算框**：
   - 在副栏或右上战况简报显示 `mvp_objectives`（如 `OBJ.A 河桥: 争夺中 | 己方 115人 vs 接触敌军`）；当红方控制大桥核心区或经过若干回合交火后，支持随时查看连级战果统计简报。

---

## 五 · Phase M1-4：512×512 MVP 全链路无作弊 E2E 与实机截图终验

更新 `tests/e2e/verify_production_ui.mjs` 并完成全栈实机验证：
1. 校验 `/api/state` 返回 `grid.rows === 512 && grid.cols === 512 && grid.cell_m === 8`；
2. 校验 `/api/terrain_raw.bin` 返回字节长度精确等于 `8 + 512*512*4`（`1,048,584` 字节，含 `terrain + raw_u16 + forest_density_u8`），且前端 `#trueTerrainRaster` 成功完成非空纹理挂载；
3. 校验开局 `own_pieces.length >= 8`（红1连完整建制）且 `visible_enemies.length >= 1`（彻底告别 `0接触点`）；
4. 保持原有 21 项门禁（6 大禁词清零、26 个 `data-ui-id` 两两 AABB 零碰撞、1440×900 卡片 `bottom <= 900`、真点击多卡、真 CDP 鼠标地图选点、`#0` 暂停态 `btnSplitSquad` 不提前增兵、提交后真实物理位移 `Δpos > 0.01` 与交火事件生成）100% PASS；
5. 保存并亲眼核验 `scratch/production_desktop_screenshot.png`，确认真实 $512 \times 512$ 白桦河河道、大桥、等高线、树林与红蓝连排兵牌交相辉映、零叠字、零 Console 报错后推送 `origin/main`。
