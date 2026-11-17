# GenTactix

> **High-Fidelity Deterministic Tactical Simulation & Military Cartography Engine**
> 
> Pure binary $2^3 = 8\text{m}$ tactical grid doctrine, deterministic Eulerian-Lagrangian field mechanics, high-performance Rust core, and SD2/CMO-inspired military printed cartography interface.

---

## 架构体系 (System Architecture)

```
D:\tools\gentactix\
├── crates/
│   ├── gentactix-core/      # 核心物理推演引擎 (13-phase deterministic transaction, 8m grid, Euler fields)
│   └── gentactix-server/    # 高性能 Axum HTTP/WS 生产服务端 (API endpoints, seat tokens, LOS/DDA)
├── web/
│   ├── index.html           # 现代严肃军用印刷地图全功能图台 (SD2/CMO 风格, APP-6 兵牌, 零滚动检视器)
│   └── assets/              # 前端静态图标与地图资源
├── data/
│   └── items_catalog.json   # 二战武器装药、火炮射表与编制权威数据
├── docs/
│   └── SPEC/                # 终极物理推演法典与工程规约 (SPEC_FINAL_GENTACTIX_CORE_ENGINE.md)
├── tests/
│   └── e2e/                 # 无头 Chrome 端到端 UI 自动化验收套件
├── art-direction/           # 视觉基准概念稿、设计规范与无头验证断言
└── archive/                 # 历史 Python 单体源码、早期探索文档与过往版本备份
```

---

## 核心技术准则 (Core Principles)

1. **纯二进制 $2^3 = 8\text{m}$ 基准网格**：
   - 彻底废除十进制近似，全图基于定点与二进制网格（$8\text{m}$ 最小网格，全图 $8192\text{m} = 8.192\text{km}$）。
   - 严格的坐标整数网格转换算子，杜绝浮点穿透与边界非确定性。

2. **十三阶段确定性事务时钟（13-Phase Deterministic Transaction）**：
   - 每回合基准步长 $\Delta t = 16\text{s}$。
   - 严格单点提交运动（`commit_motions`），整段路径采样拦截高空穿隧。
   - 强类型欧拉伤害场、散布椭圆与 DDA 视线阻挡遮挡判定。

3. **现代军用印刷地图图台（Military Cartography Web UI）**：
   - SD2 / CMO 现代严肃军用印刷地图质感（暖灰纸底色 `#ded8be`、低饱和等高线与植被、APP-6 战术兵牌体系）。
   - 双顶级容器（Inspector + Viewport），桌面分辨率主要操作零垂直滚动条。
   - 内置德军编制母树（ORBAT）抽屉与多图层实时过滤。

---

## 构建与运行 (Build & Run)

### 1. 运行核心推演引擎测试
```bash
cargo test --manifest-path Cargo.toml --workspace
```

### 2. 启动生产服务端
```bash
cargo run --manifest-path Cargo.toml -p gentactix-server --bin gentactix-server -- --web-index web/index.html
```
默认服务端口：`http://127.0.0.1:8767`

### 3. 运行端到端 UI 自动化验证
```bash
node tests/e2e/verify_production_ui.mjs
```

---

## 许可协议 (License)

本项目遵循 [MIT License](LICENSE)。历史探索资产已统一封存于 `archive/`。
