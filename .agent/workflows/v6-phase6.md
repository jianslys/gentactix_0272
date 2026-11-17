---
description: V6 Phase 6 · 重写无作弊 E2E 门禁（真点击多卡 + AABB零重叠 + 真实Δpos位移）与全栈推送验收
---

# V6 Phase 06 · 无作弊 E2E 总门禁与仓库最终交付

## 严正范围声明
- **本阶段聚焦端到端真验收**：`tests/e2e/verify_production_ui.mjs` 及最终全栈联测、截图自检、Git 提交推送。
- 若处于 Planning 模式，先列本 Phase 验证项清单 Artifact 待放行。

## 本阶段必做交付
1. 全面升级 `tests/e2e/verify_production_ui.mjs` 堵死一切作弊漏洞：
   - **全域 AABB 零重叠门禁**：补充检查 `UI-03A` 与 `.card-close-btn` 零交叠、`.toast (UI-11)` 与 `UI-04` 零交叠、初始卡片不压 `UI-04` 标题、顶部底栏红字零重叠；
   - **完整禁词扫查**：命中 `"ROUTE / 1.8km"`、`"~ 24 min"`、`"0友军"`、`"示意"`、`"示例范围"`、`"200m / 格"` 任意一词即直接红灯 FAIL；
   - **严禁调用 `window.openFloatingUnitCard` 作弊**：必须通过鼠标真实点击地图上 2 个不同的 `.map-unit` DOM 节点，断言界面同时存在 `>= 2` 张可拖拽独立 `.inspector.unit-card`，并验证独立拖拽与各自 `×` 关窗；
   - **真实地图点选 + 真实物理位移 (`Δpos > 0.01`) + 时间机载荷检验**：
     * 模拟鼠标在地图真实点选路径或工程目标下令并调用 `commit_turn` 推进回合；
     * 对比 `/api/state` 中目标单位推进前后真实坐标 `p0` 与 `p1`，强制断言 `Math.hypot(p1[0] - p0[0], p1[1] - p0[1]) > 0.01`；
     * 断言返回体中包含 `time_machine_frame`（校验 `particle_tracks` 含 `is_prerender_skirt` 及 `playback_events` 含 `exact_ms`）。
2. 全栈实测与截图核验：
   - 运行 `cargo test --manifest-path "D:\tools\gentactix\Cargo.toml" --workspace` 确保 100% 全绿；
   - 构建并拉起 `gentactix-server.exe --demo`，跑通新版 `node "D:\tools\gentactix\tests\e2e\verify_production_ui.mjs"`（zero console errors, exit code 0）；
   - **亲自调用图片读取工具查看** `D:\tools\gentactix\scratch\production_desktop_screenshot.png`，确认页面干净无叠字、多卡正常、0 console error。
3. 执行 `git add`、`git commit` 与 `git push origin main` 推回远端。

【硬阻断 · 强制停机等待确认】：
完成上述全部实测、视觉看图无误且推上 `origin/main` 后立刻停止！把所有测试通过行、改动清单、截图确认结果汇报给大王最终验收。
