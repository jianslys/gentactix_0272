---
description: V6 Hotfix Phase 3 · 跨机便携 E2E 全真点击与视口验收、截图目视核对与推送
---

# V6 Hotfix Phase 3 · 跨机便携 E2E 全链路验收与仓内推送

## 严正范围与物理隔离声明
- **本阶段聚焦端到端验真与仓库同步**：
  - `tests/e2e/verify_production_ui.mjs`（如需要便携依赖管理，可附最小 `tests/e2e/package.json`，并保持 `.gitignore` 对 `node_modules/` 与 `scratch/` 忽略有效）
- 若在前两阶段验证中发现细小前后端衔接缺陷，仅允许最小精准修正后复测。
- 详细法典请参考 `docs/SPEC/SPEC_V6_HOTFIX_CLOSURE_CODEX.md`（缺陷 6）与 `docs/SPEC/hotfix_plan.txt`。

## 本阶段必做交付
1. **彻底消除本机硬编码路径，实现跨机一键便携**：
   - 移除 `tests/e2e/verify_production_ui.mjs` 中写死的 `D:/target/debug/gentactix-server.exe`、`C:/Users/rdpuser/...` 与 `D:/scratch/...`；
   - 改为通过 `import.meta.url` 解析仓库根目录，自动定位 `target/debug/gentactix-server.exe`（允许 `process.env.GENTACTIX_BIN` 覆盖）与 `os.tmpdir()` / 仓库内 `scratch/`（注意不把运行临时图加入 git 追踪）。
2. **补全此前漏查的 4 项真用户核心门禁**：
   - **全量 26 ID 零重叠**：AABB 检测必须囊括 **`UI-05`、`UI-08`、`UI-09`、`UI-09A`、`UI-03A`、`.card-close-btn`、`.map-stamp`**，全部两两无交叠；
   - **1440×900 视口零溢出**：真点击多开 `.unit-card >= 2` 张后，断言每一张浮动卡 `rect.top >= 0 && rect.bottom <= 900`；
   - **真鼠标点击地图选点全链路（严禁 JS 绕过）**：真实鼠标点击 `行动 -> 在地图标定` 后，分别点击屏幕中心偏战场区及空旷区地图坐标，断言 `pendingMapGoal` 真落点且发起真实 `/api/preview_plan`（非零路径长度）；
   - **`#0` 入队与 `Δpos` 真理校验**：断言开局敌军 `blue-tk-2` 未穿帮亮出、Turn 0 真点击 `#btnSplitSquad` 后 `commit_turn` 前 `own_pieces` 不提前增多、`commit_turn` 后才增兵且移动单位位移 `Δpos > 0.01`。
3. **全栈实测、亲眼看图与推送回 `origin/main`**：
   ```powershell
   cargo test --workspace
   cargo build --bin gentactix-server
   node tests/e2e/verify_production_ui.mjs
   ```
   - 必须亲眼读取最新生成截图（`scratch/production_desktop_screenshot.png`），确认无 `UI-05/09` 叠字、无卡片切底边、单位与比例尺符合实际格网；
   - 清理临时非跟踪测试残留后，执行 `git add`、`git commit` 与 `git push origin main`。

【硬阻断 · 强制停机等待确认】：
完成全量编译测试、E2E 真通过、亲眼核图无误并成功 `git push origin main` 后立刻停止！把全部实跑输出摘要、修复清单与截图确认结论呈报给大王最终复核。
