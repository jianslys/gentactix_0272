---
description: V6 Hotfix Phase 2 · 前端真点击、单坐标系、TimeMachineRenderer 与 UI 零叠字溢出修复
---

# V6 Hotfix Phase 2 · 前端真点击、单坐标系与视觉溢出口径清零

## 严正范围与物理隔离声明
- **本阶段仅限修改**：`web/index.html`
- **绝对禁止触碰**：`crates/` 下任意 Rust 代码、`tests/e2e/verify_production_ui.mjs`。
- 详细法典请先阅读 `docs/SPEC/SPEC_V6_HOTFIX_CLOSURE_CODEX.md`（缺陷 4~5）与 `docs/SPEC/hotfix_plan.txt`。

## 本阶段必做交付
1. **彻底治好 `#viewport` 截胡点击与模态挡屏**（`web/index.html`）：
   - 取消 `$('viewport')` `pointerdown` 上无条件 `setPointerCapture(e.pointerId)`；改为仅在 `pointermove` 拖拽位移 `> 5px` 确认拖动时才捕获指针，确保真鼠标单击能稳定送达 `#mapSvg`（或由未拖拽 `pointerup` 精准换算 SVG 坐标并驱动 `mapPickMode` 与 `UI-09A` 工具）；
   - `startMapPick` 必须将 `$('integratedCmdBox').style.pointerEvents = 'none'`，`cancelMapPick` 复原 `'auto'`，彻底消除地图中心战场区点击被半透明弹窗挡死的问题。
2. **消灭两套坐标系分裂与假 `"8m"` / `/8` / `/10` / `/64` 魔数**：
   - 建立唯一共享函数 `gridToSvg(r, c, grid)` 与 `svgToGrid(svgX, svgY, grid)`，动态读取后端 `data.grid.rows / cols / cell_m`，全面替换 `triggerPreviewPlan`、`commitTaskToQueue`、`cursorGrid`、`updateScale`、`toolRuler`、`TimeMachineRenderer.step`、`triggerExactEvent` 中的 `/ 10`、`* 10`、硬编码 `64` 与写死 `8m / / 8`；
   - `subbarTurnState`、`meterGrid` 标题及单位卡地域名称动态按后端真实 `cell_m` 渲染（如 `${cellM}m战术基准` / `${cellM}m / 格`）。
3. **清零假路线、`HOLD` 错距与时间机像素圈视野**：
   - `calculateUnitOrderMetrics(u)` 正确识别 `'HOLD'` / `'hold'` 与静止态并输出 `'0.0 km'` / `'待命 · 现地'`，删除向 `e1` / `{x+80, y-60}` 凑距离逻辑；单位无真实队列路径时隐藏写死静态 `#routesLayer`；
   - `TimeMachineRenderer.step` 禁止再用 `< 120px` 固定屏幕像素圈判定裙边显隐，改为基于真实网格距离与后端 `cell_m` / `initial_visibility` 调度；
   - 对接 `REORG_EQUIP_OP_MAP` 返回的 `queued: true` 状态，不于暂停当回合提前虚假加兵牌。
4. **根治 1440×900 实机截图中 UI 重叠与卡片溢出**：
   - 调整 `.map-tools-pack` / `.tool-pack-unified` / `.layer-toggle` / `.map-stamp` 布局，消除右上角 `UI-05` 遮挡 `图层` 图标与 `UI-09` 压撞顶边问题，确保全部 26 个 `[data-ui-id]` 两两零重叠；
   - 收缩 `.inspector.unit-card` 紧凑高度并夹紧多卡叠放 `offsetY`，保证 `1440×900` 视口下全部卡片 `bottom <= window.innerHeight - 12`。

【硬阻断 · 强制停机等待确认】：
执行完当前 Phase 2 的 `web/index.html` 改动后必须立刻停止！列出对应行区与重构要点并做 `node` 语法或源码 grep 自查（确认 `/ 10`、`* 10`、`Math.hypot(u.x - mapX, u.y - mapY) < 120` 与写死 `8m` 魔数均已清理），等待大王回复“确认”或手动输入 `/v6-hotfix-3` 后再动 E2E 测试，严禁自动连跑或提交推送！
