---
description: V6 Phase 3 · 消灭 0 友军前后端脱节 Bug 与 UI-14B 操作枚举对齐并单步停机验收
---

# V6 Phase 03 · 修复 `0友军` 脱节与 `UI-14B` 接口契约对齐

## 严正范围与物理隔离声明
- **本阶段仅限解决“前后端单位同步与 `UI-14B` 枚举对齐”**，范围严格锁定：
  - `crates/gentactix-server/src/main.rs`（`serialize_state` 默认视角导出及契约校验）
  - `web/index.html`（仅修改 `fetchBackendState()` 数据替换逻辑及 `UI-14B` 按钮 `op` 映射）
- **绝对禁止**：先不要在本阶段大改视觉 CSS、卡片重叠或新建重绘逻辑（留待 Phase 4 & 5）。
- 若处于 Planning 模式，先列本阶段改动方案 Artifact 待大王放行后再动手。

## 本阶段必做交付
1. 修复默认视角 `0友军` Bug：
   - 检查 `crates/gentactix-server/src/main.rs` 的 `serialize_state`：当未显式传 `commander_id` / `team` 时，默认以红方（`TeamId::Red`）最高指挥官视角导出全部红方 `own_pieces`（带真实 `id`、`pos`、`inventory`、`is_direct_subordinate`）、经 `I_intel` 过滤的 `visible_enemies` 与 `prerender_skirt_enemies`；
   - 更新 `web/index.html` 的 `fetchBackendState()`：用真实 `/api/state` 的 `own_pieces` 完全覆盖静态演示 `units` 数组，确保顶部副栏显示非零友军计数，且发出的指令全部采用后端真实 `piece_id`（严禁混用无主 `u3` 导致 `piece_not_found`）。
2. 修复 `UI-14B` 按钮协议名：
   - 将 `btnSplitSquad` 等按钮从发送 DOM ID 改为后端标准枚举：`"split_squad"`、`"merge_squads"`、`"equip_adjust"`、`"use_item"`、`"deploy_heavy_unit"`、`"pack_heavy_unit"`，携带合法 `piece_id` 发往 `/api/unit_op` 验证返回 `200 OK`。
3. 运行验证：
   ```powershell
   cargo test --manifest-path "D:\tools\gentactix\Cargo.toml" --workspace
   ```
   并起一个临时的 server 实测 `GET /api/state` 确认 `own_pieces.length > 0`、`POST /api/unit_op` 对 `split_squad` 返回 `200 OK`（测完清理临时进程）。

【硬阻断 · 强制停机等待确认】：
完成本 Phase 代码与接口实测后必须立刻停止！向大王汇报 `/api/state` 返回的真实友军数量及测试结果，等待大王回复“确认”或手动输入 `/v6-phase4` 后再进入下一阶段！
