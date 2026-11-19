# GenTactix V2 Pure Engine

> **Deterministic Platoon/Squad Tactical Simulation Core (`8.0m` Binary Grid · `16s` Integer Clock · Headless HTTP API)**

GenTactix V2 是彻底剥离历史包袱后的纯粹 8 大支柱战术推演引擎（主分支工作流：`v2/pure-engine`），以排/班/单车为主尺度，通过无头 Axum HTTP 服务向未来新前端提供确定性状态推进、只读战术预览与零污染时间机播片剧本。

---

## 目录结构

```text
gentactix/
├── Cargo.toml / Cargo.lock            # Rust Workspace (gentactix-core + gentactix-server)
├── AGENTS.md / GEMINI.md              # 多端 AI 协同规约、架构红线与开发工作流
├── FUNCTION_AND_INTERFACE_MANUAL.md   # 核心 12 模块、TacticalCommand 与 18 条 HTTP 路由速查手册
├── crates/
│   ├── gentactix-core/                # 纯无头仿真内核（三态、13步时钟、四维算子、时间机、迷雾、预览）
│   └── gentactix-server/              # 极简 Axum HTTP 服务壳（src 合计 < 400 行，零物理业务逻辑）
├── data/
│   └── items_catalog.json             # 二战单兵/班组武器、弹药、引信、枪架唯一权威目录
├── art-direction/                     # 军用印刷地图 UI 视觉参考图（供后续新建前端对齐用）
├── docs/                              # 历史理论与早期物理 SPEC 留档（参考用，以 V2 代码为准）
└── LICENSE
```

*(历史 V0/V1 单体内核与旧版 HTML 前端完整留档于远端 `main` 与 `refactor/pure-engine` 分支。)*

---

## 快速编译、测试与启动

```bash
# 1. 全工作区编译检查（零警告门禁）
cargo check --workspace --all-targets

# 2. 运行全部单元测试、集成测试与 HTTP 端到端闭环测试
cargo test --workspace

# 3. 启动无头引擎服务（默认监听 127.0.0.1:8767，初始 64x64 @ 8.0m 白板地图）
cargo run -p gentactix-server -- --listen 127.0.0.1:8767 --rows 64 --cols 64
```

服务就绪后可访问：
- `GET http://127.0.0.1:8767/health` — 健康检查与 `8.0m` / `16s` 基准确认
- `GET http://127.0.0.1:8767/api/schema` — 获取自描述指令集、预设规则与全套接口清单

---

## 协同开发入口

- 多设备 AI 协同规范与分支流程：请先通读 [AGENTS.md](AGENTS.md)
- 全量 API 入参/出参与数据结构：请查阅 [FUNCTION_AND_INTERFACE_MANUAL.md](FUNCTION_AND_INTERFACE_MANUAL.md)
- 新建前端请独立建在 `frontend/` 目录，仅通过 HTTP 接口驱动引擎。
