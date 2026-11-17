---
description: V6 Phase 5 · 打通地图真点击选点与前端时间机渲染驱动器（Pre-Render Skirt + exact_ms）并停机验收
---

# V6 Phase 05 · 真地图选点交互与时间机前后端渲染合龙

## 严正范围与物理隔离声明
- **本阶段仅限修改前端交互与时间机渲染逻辑**：`web/index.html`
- **绝对禁止**：修改后端 `.rs` 源码（除非发现契约小字段笔误需报备）或擅自写 Phase 6 E2E。
- 若处于 Planning 模式，先提交本 Phase 清单 Artifact 待批注放行。

## 本阶段必做交付
1. 废除所有硬编码假偏移，打通真地图点选与绘制：
   - 彻底清理 `commitTaskToQueue` 的 `(+72, -48)` 与 `commitEventToQueue` 爆破的 `(+40, -20)` / `(-60, +40)` 假偏移代码；
   - 为 `UI-03M1`（除 `撤退 -> 直接跑路`）、`UI-03M2`、`UI-03M3` 绑定完整的地图拾取态：点“在地图标定”后进入准星模式，鼠标左键点击 `#mapSvg` 真实写入 `pendingMapGoal`（爆破模式第 1 步划出爆破位置/多边形，第 2 步点击设定安全撤离起爆开关位置）；拖动 `UI-03M1` 约束滑块实时请求 `/api/preview_plan` 重绘流线。
2. 实装前端时间机只读渲染与 `visibility=0` 预渲染缓冲环：
   - 收到后端 `time_machine_frame` 后，预先挂载所有处于缓冲环的敌军粒子节点（`is_prerender_skirt: true`），初始设为不可见与禁用交互（`visibility: hidden` 或 `opacity: 0; pointer-events: none`）；
   - 在 `0ms..16000ms` 走表期间：
     * 所有己方与预渲染敌军粒子按 `particle_tracks` 的 `keyframes` 实时做连续线性插值移动 $\mathbf{x}(t_{\text{ms}})$；
     * 一旦某预渲染敌军粒子的当前插值位置跨入我方可见区域，当场把 `visibility` 切为可见（滑出迷雾则切回隐藏）；
     * 时钟扫到 `playback_events` 某事件的 `exact_ms` 时，直接在受击粒子此刻的线性插值位置 $\mathbf{x}(\text{exact\_ms})$ 上爆出爆炸/命中/施工特效，并在同一毫秒把后端算好的原装 `exact_hp_loss` / 压制变化刷上 UI。

【硬阻断 · 强制停机等待确认】：
完成本 Phase 前端真地图选点与时间机插值放片验证后必须立刻停止！汇报操作与效果，等待大王回复“确认”或手动输入 `/v6-phase6` 后再进入最后验收阶段！
