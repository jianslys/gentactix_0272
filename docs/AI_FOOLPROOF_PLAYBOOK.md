# GenTactix V2 “傻子 AI”前端与美术操作手册 (`AI_FOOLPROOF_PLAYBOOK.md`)

> **定位**：写给任意外部 AI / 轻量模型 / 新会话 AI 协作者的**防走偏操作手册**。  
> **阅读指令**：只要大王让你修改 `frontend/` 的 UI、美术、Canvas 渲染、样式或交互，**动手前必须先读本文件 + [FRONTEND_FOOLPROOF_HANDBOOK.md](FRONTEND_FOOLPROOF_HANDBOOK.md)**；**严禁擅自修改 `crates/` 后端源码**（除非大王明确点名要求改后端）。

---

## 一 · 给大王的“万能指挥 AI 话术模板”（直接复制填空）

> 大王以后想让任何 AI 改前端，只需把界面上看到的 **UI ID** 报给它，照着下方模板粘一句就行：

- **模板 A · 按 UI ID 改界面/按钮/配色/样式**
  > “先读 `docs/AI_FOOLPROOF_PLAYBOOK.md` 和 `docs/FRONTEND_FOOLPROOF_HANDBOOK.md`。把界面上 **`[填 UI ID，比如 W3.I6 / W1.T4 / W2.F4.opt_red]`** 改成 **`[描述你的要求，例如：文字改大、颜色改成军绿色 / 默认位置再往右挪一点]`**。改完在 `frontend/` 跑通 `npm run typecheck`、`npm test`、`npm run build`。”

- **模板 B · 新增/替换美术贴图或修改 Canvas 图层绘制**
  > “先读 `docs/AI_FOOLPROOF_PLAYBOOK.md` 第 3 节和 `frontend/src/art/assetManifest.ts`。我已经在 `frontend/src/assets/...` 放了 **`[文件名]`**（或者帮我新增一个槽位 **`[名称]`** 在 Layer **`[L1~L5]`** 绘制），保持无图自动回退、零 404 请求、零 8m 网格线，跑通全套测试和构建。”

- **模板 C · 复制 `W8` 新增一个功能窗口**
  > “先读 `docs/FRONTEND_FOOLPROOF_HANDBOOK.md` 第 4 章和 `frontend/src/ui/ExampleCustomWindow.tsx`。复制 `W8` 新建一个窗口 **`W9`**（标题：**`[名字]`**），用 `useGame()` 和 `sendQuickCommand()` 实现 **`[功能]`**，挂到 `<FloatingWindow id="W9">` 并把新 UI ID 补充进手册与测试。”

- **模板 D · 接入尚未用到界面的真后端只读预览（路径/视线/射界/工时）**
  > “先读 `docs/FRONTEND_DEV_AND_ART_MANUAL.md` 第 2.3 节只读端点契约，在 `BackendAdapter`（`LocalMockSandboxAdapter` + `HttpEngineAdapter`）补齐 **`[/api/query/plan | /api/query/los | /api/query/arc | /api/query/work]`**，严禁在 UI 组件裸写 `fetch()`，并在 `W3` 或 Layer 5 渲染出来。”

---

## 二 · AI 强制 4 步冷启动 SOP（严禁跳步与盲改）

1. **第 1 步（先查字典，绝不乱翻）**
   - 用户给出界面 ID（例如 `W5A.U3.task.opt_move_unit` 或 `W7.D3`）时，直接在 [FRONTEND_FOOLPROOF_HANDBOOK.md](FRONTEND_FOOLPROOF_HANDBOOK.md) 第 5 章查出唯一对应的 `.tsx` 文件。
   - **禁止一上来全盘扫 `docs/research-theory/`、`docs/SPEC/` 历史稿，严禁默认打开 `art-direction/` 图片**（仅当大王下令做美术视觉对图时才读 `art-direction/2f89d1e3-b843-410a-b72a-4001b2c64ffc.png`）。
2. **第 2 步（核对 Git 分支）**
   - 前端与美术日常开发在 **`v2/frontend-dev`**（或当前特性分支，合并主干为 **`v2/pure-engine`**）；
   - **绝对禁止触碰或推送 `main`、`refactor/pure-engine`**（旧 V1 历史归档分支）。
3. **第 3 步（局部精准读取再落刀）**
   - 修改已有文件前，只读相关函数上下百余行；新建窗口直接照抄 `frontend/src/ui/ExampleCustomWindow.tsx` 架构。
4. **第 4 步（全权自办闭环）**
   - 自己调终端跑构建和测试，有编译报错或测试报错当场自己修通；**绝对禁止让大王复制命令代跑或粘贴报错**。

---

## 三 · 4 类高频任务的 AI 标准手术清单

| 任务类型 | 允许改动的文件清单 | 严禁破坏的不变量 |
| :--- | :--- | :--- |
| **1. 改颜色 / 字体 / HUD 布局** | `frontend/src/art/themeTokens.ts`<br>`frontend/src/styles.css`<br>`frontend/src/store/windowStore.ts` | 1) 默认状态下（`W2..W5` 打开、`W7/W8` 关闭）屏幕中央战场必须保持无遮挡；<br>2) `FloatingWindow` 保留 `floating-docked -> floating-detached` 与 `localStorage` 持久化 |
| **2. 改按钮文字 / 选项 / 新增 UI ID** | 对应 `frontend/src/ui/*.tsx`<br>`frontend/src/ui/UiControls.tsx`<br>`docs/FRONTEND_FOOLPROOF_HANDBOOK.md` | 1) **禁止删除或重命名已有的 `data-testid`**；<br>2) 新控件用 `<UiButton>` / `<UiSelect>` / `<UiToggle>`；<br>3) 每个 `W1..W8` 的 `data-ui-tag` 同一时刻全局只能有 **1 个**，且按 `T12` 必须归 `0` |
| **3. 增改美术贴图 / Canvas 渲染** | `frontend/src/art/assetManifest.ts`<br>`frontend/src/art/assetLoader.ts`<br>`frontend/src/render/canvasLayers.ts`<br>`frontend/src/assets/...` | 1) 缺图必须静默回退矢量绘制，**禁止向不存在路径发 fetch / new Image() 导致 404 `console.error`**；<br>2) Layer 1~5 **禁止 `strokeRect` 画网格**（`phase2_layers.test.tsx` 与 `phase5_nanny_foundation.test.tsx` 硬校验） |
| **4. 新增浮动面板 / 接新指令** | `frontend/src/ui/ExampleCustomWindow.tsx`（复制）<br>`frontend/src/store/windowStore.ts`<br>`frontend/src/App.tsx`<br>`frontend/src/store/gameContext.tsx` | 1) 新面板只准调 `useGame()` 和 `sendQuickCommand()` / `adapter`；<br>2) 同时兼容 `Mock 沙盒` 与 `HTTP 真后端`（如 `stealth` 在 `httpEngineAdapter.ts` 自动映射为 `cautious`） |

---

## 四 · AI 严禁触碰的 7 条死红线

1. **红线 1 · 后端隔离铁律**：改前端/美术任务时，**一个字节都不准改 `crates/`**；若真受命调整 `crates/gentactix-server/src/`，`src/*.rs` 合计行数必须严格保持 **`< 400` 行**。
2. **红线 2 · 严禁 8m 物理网格暴露**：`TACTICAL_CELL_SIZE_M = 8.0` 只用于坐标换算与射线拾取；任何图层画出 8m 方格线即判定失败。
3. **红线 3 · 严禁组件内散落 `fetch()`**：除 `src/engine/httpEngineAdapter.ts` 和 `src/art/assetLoader.ts`（仅读 `manifest.json`）外，全前端不得直接调 `fetch()`。
4. **红线 4 · 防剧透时间机铁律**：Tick 播放期间必须经 `evaluateFrameAt()` / `progressiveSnapshot()` 按 `elapsedMs` 展示状态，`t = 0s` 严禁提前显示回合末结果，严禁渲染 `is_prerender_skirt = true` 隐藏单位。
5. **红线 5 · 测试与选择器守护**：必须保住既有 5 套 Vitest（`phase1_core` ~ `phase5_nanny_foundation`）与 `e2e/frontend.spec.ts` 全部选择器及 `console.error === 0` 门禁。
6. **红线 6 · UTF-8 无 BOM 铁律**：所有 `.ts` / `.tsx` / `.css` / `.json` / `.html` / `.md` / `.rs` 严禁写入 UTF-8 BOM（`0xEF 0xBB 0xBF`）。
7. **红线 7 · Git 干净交付**：严禁把 `frontend/node_modules/`、`frontend/dist/`、临时脚本或任何 `ghp_*` 密钥提交进仓库。

---

## 五 · AI 交作业前必过 4 项实测门禁（跑完贴真实数字）

每次修改完前端或文档，AI 必须自己先跑完下面全部检查，全部退出码为 `0` 后才准向大王汇报：

```powershell
# 1. 类型检查 + 单元测试 + 生产打包
Set-Location D:\tools\gentactix\frontend
npm run typecheck
npm test
npm run build

# 2. 全仓零 BOM 检查 + 后端行数门禁 + Git 洁净度检查
Set-Location D:\tools\gentactix
$bom = 0
Get-ChildItem -Recurse -File -Include *.rs,*.md,*.ts,*.tsx,*.css,*.html,*.json |
  Where-Object { $_.FullName -notmatch '\\target\\|\\\.git\\|\\node_modules\\|\\dist\\' } |
  ForEach-Object {
    $b = [System.IO.File]::ReadAllBytes($_.FullName)
    if ($b.Length -ge 3 -and $b[0] -eq 239 -and $b[1] -eq 187 -and $b[2] -eq 191) { $bom++ }
  }
Write-Host "BOM_COUNT=$bom"
git status --short
```

*(若还同步改动了任何后端 `.rs` 文件或联调契约，额外必跑 `cargo check --workspace --all-targets` 与 `cargo test --workspace`。)*
