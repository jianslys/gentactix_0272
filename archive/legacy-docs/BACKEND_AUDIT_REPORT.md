# Gentactix V3 后端全量源码审查报告

> **审查范围**：`D:\tools\gentactix\gentactix\` 全量 18 个模块 + `web_server_v3.py` + `r16_platoon_loader.py`（总计约 1.3 万行 Python 源码）。  
> **审查原则**：逐文件核验真实代码、精确行号与符号引用，零猜测、零虚构。

---

## 一 · 总体定性与核心矛盾

Gentactix 的**底层物理与数学底座（地形微积分、TO&E 三账本、指数承伤与小数累计、质量守恒校验、Zero-LLM 断连黑盒）**设计得相当扎实；但**中游事件调度、自主机动执行、以及 Web 接口适配层在多次迭代中出现了严重断裂与偷懒式硬编码**：
1. **“写死”与“直线”背离物理引擎**：一边在 `terrain_field.py` 写了 2000 行的微积分坡度与 Dijkstra 寻路，一边在 `autonomy_v3.py` 和 `web_server_v3.py` 里写死 `BASE_SPEED_KMH`、拿目标坐标拉直线、用单格截断把 12 km/h 载具阉割成 2.4 km/h 乌龟。
2. **模块之间“各写各的、互不接通”**：`toe.py` 写了 `heavy_operator_efficiency` 但 `Squad.power()` 不调；`locks.py` 定义了 7 种行动锁但核心战斗模块全未导入；`events.py` 写了 `target_unit_id` 部署重武器但入口校验直接把它拦死；`web_server_v3.py` 缺了操炮与事件取消接口。
3. **局部破坏项目核心准则**：`adjudicator_v2.py` 里多处写死 `org < 80 / 40 / 25` 替指挥官与士兵做心理撤退/停滞裁决；`recon_fog.py` 地面侦察直接绕过山体高程透视；`web_server_v3.py` 直接向前端明文泄漏敌方 `active_events` 且未校验敌我权限。

---

## 二 · 全量 20 个后端文件健康度总览

| 序号 | 模块文件 | 行数 | 健康度 | 核心定性 |
| :---: | :--- | :---: | :---: | :--- |
| 1 | `gentactix/llm_seat_v3.py` | 58 | **95 / 100** | 干净，Zero-LLM 物理隔离到位，无明显缺陷。 |
| 2 | `gentactix/combat.py` | 643 | **85 / 100** | 承伤曲线与 `carry` 稳健；缺面杀伤能量分摊，轻武器肉盾全倒给首辆坦克。 |
| 3 | `gentactix/modifiers.py` | 166 | **85 / 100** | 乘区/泊松故障自洽；合并白名单漏掩体类键，会议常量粗暴覆写政令混乱。 |
| 4 | `gentactix/logistics.py` | 324 | **82 / 100** | 两段式守恒可用；细分弹药与总库存脱节可致负库存，空库误判兼容，漏补兵员。 |
| 5 | `gentactix/command_v2.py` | 498 | **82 / 100** | AO 几何与三层链路严密；`pieces is None` 时放行无限距离无线电通联，5 处死导入。 |
| 6 | `gentactix/baguwen_v3.py` | 1394 | **80 / 100** | 断连黑盒严密；`off_cadence` 前置拦截导致上级加急新命令被下级整点节拍死锁。 |
| 7 | `gentactix/terrain_field.py` | 2046 | **78 / 100** | 微积分与势场核心强；NPZ 读档丢失 `line_role`，道路上坡未减阻，`_SubGridWindow` 缺高程字段。 |
| 8 | `gentactix/adjudicator_v2.py` | 2053 | **76 / 100** | 主循环完备；**`run_turn_v3` 次回合 `tick_index` 平方级跳变**，多处硬编码组织度心理阈值。 |
| 9 | `gentactix/viz_v2.py` | 213 | **70 / 100** | 纯内存 PNG 打包可用；**山体阴影东南向打光倒错**，独立载具被画成步兵叉，迷雾透视山体。 |
| 10 | `gentactix/toe.py` | 2284 | **68 / 100** | 三账本基础好；**操炮效率未接入 `Squad.power()` 致无人阵地火力为 0**，载具表封闭，无人载具降为步行。 |
| 11 | `gentactix/recon_fog.py` | 240 | **68 / 100** | 模糊区间可用；**地面侦察完全无视高程 LOS（隔山透视）**，加噪坐标未 clamp 越界。 |
| 12 | `gentactix/events.py` | 1206 | **64 / 100** | **爆破/布雷零资质零距离隔空炸桥炸库**，`deploy_heavy_unit` 入口死锁，撤收蒸发弹药，会议暴露度 2.5×+组合爆炸。 |
| 13 | `gentactix/artillery.py` | 228 | **62 / 100** | **无 `carry` 小数累计导致徐进弹幕流产**，`max(1.0, accuracy_mult)` 吞掉精度惩罚，脱离 TO&E 编制。 |
| 14 | `gentactix/ww2_air.py` | 390 | **62 / 100** | **单方制空时 `victim=None` 导致低空扫射迟滞场完全失效**，空袭漏传 `weapon_model`。 |
| 15 | `r16_platoon_loader.py` | 277 | **52 / 100** | 硬编码 `E:\商单1100` 与 `(696, 2700)`，多层覆盖毁林，寻格拒绝山地且单位重叠同格。 |
| 16 | `gentactix/autonomy_v3.py` | 1488 | **52 / 100** | **单 subtick 迈 1 格截断致载具限速 2.4 km/h**，无全局寻路仅 8 邻域拉直线，自动架炮 100% 失败，追击透视。 |
| 17 | `web_server_v3.py` | 1030 | **45 / 100** | **`/api/unit_op` 无敌我鉴权可操控敌军**，泄漏全图 `active_events`，长锁阻塞，缺操炮/事件取消/背包接口。 |
| 18 | `gentactix/scenario_v2.py` | 230 | **40 / 100** | **调用不存在的 `add_wire_link` 与 `register_ao_zone` 加载即崩**，形参倒置。 |
| 19 | `gentactix/locks.py` | 47 | **35 / 100** | 架构孤岛：4 大战斗模块全未导入，`toe.py` 自行硬编码子集还漏掉 `hold_still`。 |
| 20 | `gentactix/__init__.py` | 13 | **100 / 100** | 仅包文档与版本号 `3.0.0`。 |

---

## 三 · Top 10 致命与阻断级问题（P0 清单）

### 1. 机动物理截断：12 km/h 载具被强行阉割成 2.4 km/h 乌龟
- **位置**：`autonomy_v3.py:1403`（及 `_linear_step_piece:1436`）
- **代码证据**：
  ```python
  max_step_px = min(d_step_px, (budget_km / t_cost) / px_km)
  piece.pos = (pr + step_dr * (max_step_px / d_step_px), pc + step_dc * (max_step_px / d_step_px))
  return True
  ```
- **真相**：12 km/h 载具在 5 分钟 sub-tick 内预算为 1.0 km = 5.0 像素（fast 模式 7.25 像素），但 `_linear_step_piece` 只调用一次 `_fast_local_step`，`min(d_step_px, ...)` 把单次位移死锁在 1 格（≤1.414 px），**剩余 70%–80% 移动预算直接丢弃**。全图载具实际移速被钉死在 `2.4 km/h`。

### 2. 速度写死 + 缺最短路径寻址（`fast / normal / slow` 全是直线乘数）
- **位置**：`autonomy_v3.py:21, 1127-1138, 1158-1162, 1357-1409`；`web_server_v3.py:868, 876-884`
- **代码证据**：
  - `autonomy_v3.py:21` 写死 `BASE_SPEED_KMH = {"foot": 3.5, "wheeled": 12.0, "tracked": 12.0}`，无视具体载具型号、重武器拖拽与地形耦合；
  - `autonomy_v3.py:1128-1138`：`fast` 只是 `advance_scale = 1.45`，`slow/stealth` 只是 `0.55`；
  - `autonomy_v3.py:1357-1409`：直控任务从未调用 `terrain_field.shortest_path`，只在 8 邻域按与目标直线夹角贪婪打分，遇到河流/悬崖直接死锁不动；
  - `web_server_v3.py:876-884`：`estimate_plan_time` 在真空中按欧氏直线算距离，且每个航路点强行 `max(5, ...)` 保底 5 分钟（5 个密集路点直接估出 25 分钟）。

### 3. 重武器操炮火力管线彻底断裂（无人阵地火力为 0）
- **位置**：`toe.py:414-448` vs `toe.py:1369-1394`；`adjudicator_v2.py:1486-1495`；`web_server_v3.py:600-804`
- **真相**：
  - `toe.py:1369` 写了精细的 `heavy_operator_efficiency(squad, weapon_model)`，但 `Squad.power()`（`toe.py:414-448`）**从未调用它**，只认自身 `self.persons`；
  - 分离出去的重武器棋子自身 `persons == 0`，即便步兵进格操炮，开火时 `crew_factor()` 算得 `0.0`，火力直接归零；
  - 更致命的是 `web_server_v3.py` 的 `/api/unit_op` **根本没有暴露 `operate_heavy_unit` 接口**，前端无法指派操炮。

### 4. 重武器部署死锁 + 撤收蒸发弹药
- **位置**：`autonomy_v3.py:1461`；`events.py:403-409, 638-647, 669, 847-856`
- **真相**：
  - `autonomy_v3.py:1461` 调用 `deploy_heavy_unit` 时漏传 `weapon` 字段，100% 失败；
  - `events.py:403-409` 入口强行要求牵引车自身 Squad 库存必须有 `weapon`，把 638 行已实现的“通过 `target_unit_id` 展开挂靠重武器实体”逻辑在入口处 100% 拦死；
  - 新建阵地（`events.py:669`）未挂入任何指挥官 `subordinate_ids`，变成断链孤儿；
  - `pack_heavy_unit` 撤收合并回母部队时（`events.py:847-856`），直接 `world.pieces.pop(tgt.id)`，阵地携带的全部 `stock` 与 `ammo_by_type` 凭空蒸发，且未加权更新 `org` 与 `clamp_org()`。

### 5. `run_turn_v3` 次回合 `tick_index` 平方级暴涨
- **位置**：`adjudicator_v2.py:1953` vs `adjudicator_v2.py:1681`
- **代码证据**：
  `run_turn` 每步末尾都会 `world.turn += 1`（1681 行）。师级推演 72 个 subtick 后，`world.turn` 变成 72；第二回合 `base_turn_tick = world.turn * n_ticks = 72 * 72 = 5184`，**直接跳过 5,112 个 tick**，彻底打废 `baguwen_v3.py` 依赖 `tick_index % cadence == 0` 的全层级指挥官节拍。

### 6. 工兵爆破 / 布雷零资质 + 隔空全图摧毁
- **位置**：`events.py:465-473, 610-636, 799-813`
- **真相**：`demolition` 与 `engineering` **完全不校验工兵资质、不消耗炸药/地雷、不校验目标格距离**。位于 `(0,0)` 的普通步兵甚至观测员传入 `target_px: [80, 80]`，到期直接隔空炸毁敌方后方大桥、清空敌方基地仓库 `Depot`；排雷 `clear_mines` 只删 `cell_modifiers`，漏删 `terrain_features` 预置雷区。

### 7. 战争迷雾与高程物理的三大穿透漏洞
1. **地面侦察透视大山**：`recon_fog.py:73, 86-90` 只算欧氏距离与目标格 `vis`，**从未调用 `tf.line_visibility` 做山体高程遮蔽（LOS）**；加噪坐标未 `clamp`，边缘单位可产生负坐标。
2. **追击目标全图 GPS**：`autonomy_v3.py:1086-1089, 1103-1106` 对 `follow_target` 直接从 `world.pieces` 读敌方真身坐标，无视迷雾。
3. **Web 接口明文泄露敌方事件**：`web_server_v3.py:421` 把全图 `w.event_manager.events.values()` 未经阵营过滤直接下发给前端 JSON。

### 8. Web 接口无敌我鉴权 + 并发死锁
- **位置**：`web_server_v3.py:157, 655-792, 806-826`
- **真相**：
  - `/api/unit_op` 的 `direct_action`、`set_radio`、队列增删改**完全不校验 `p.team` 与指挥链**，前端传敌方 `target_id` 可直接勒令敌军撤退瘫痪 1 小时或开启敌台无线电静默；
  - `/api/commit_turn` 只要不传 `player_commander_id` 就跳过权限校验；
  - 全服共用一把粗粒度 `RLock`，`commit_turn` 推演期间全服 `/api/state` 轮询全部挂起死等；
  - `serialize_state`（206 行）跳过 `persons() <= 0` 的棋子，使无人载具/无人炮位在前端彻底隐形。

### 9. 空军单方制空扫射迟滞场完全失效
- **位置**：`ww2_air.py:331-347`
- **真相**：`teams = {x.team for x in wings}` 只从航空联队提取阵营。敌方若无飞机（纯地面防空/步兵），`victim` 恒为 `None`，战斗机 `strafe` 扫射迟滞场 `FireFields` 永远无法生成。

### 10. `scenario_v2.py` 必崩 + `terrain_field.py` 读档丢战线角色
- **位置**：`scenario_v2.py:186-197`；`terrain_field.py:1551-1559`
- **真相**：`scenario_v2.py` 调用了 `grid.add_wire_link` 与 `command_v2.register_ao_zone`（两个根本不存在的方法），且 `build_ao_cells` 形参倒置；`terrain_field.py:load_npz` 读档时漏传 `line_role`、`source_deploy_line_id`、`target_deploy_line_id`，导致存档加载后进攻/机动战线全退化为部署线。

---

## 四 · 按领域分类的其他重要缺陷（P1 / P2 汇总）

### A. 违背“不写死人性与物理”的硬编码清单
| 位置 | 写死内容 | 危害 |
| :--- | :--- | :--- |
| `adjudicator_v2.py:1256` | `p.mean_org() < 80.0` 伏击必停 | 80 算高战备，稍有接火零伤亡也强行停滞，剥夺突击意志。 |
| `adjudicator_v2.py:308, 1128` | `p.mean_org() < 25.0` 判溃退 | 直接失去 ZOC 被装甲碾穿。 |
| `adjudicator_v2.py:1556-1558` | CQB `org < 40.0` 硬判败退 | 无视 100 人对 3 人的兵力与火力悬殊直接判退格。 |
| `toe.py:90-96` | `VEHICLE_SPECS` 封闭 5 种 | 无 `register_vehicle_spec()`，自定义载具直接 `KeyError` 或失效。 |
| `toe.py:1083-1094` | `mobility()` 只按人头加权 | 无人载具（`persons==0`）退化为 `"foot"` 步行速度。 |
| `toe.py:449-454` | 半履带与卡车合并算 3.0t | 半履带载重虚增一倍（实为 1.5t），6.0t 牵引车被算成 0。 |
| `toe.py:1439-1443` | `tow_profile` 只判 `at_gun_50` | 75mm 等更重反坦克炮落入 `else` 分支反而拖得快 3.6 倍。 |
| `terrain_field.py:1787-1794` | 道路未减坡度阻力倍率 | `kk=9.0` 对干线公路照罚不误，无视工程筑路推坡减阻。 |
| `viz_v2.py:53` | `0.75 + 2.2*(gx+gy)` | 东南向打光致山脊/河谷视觉凹凸倒错（应为西北减号）。 |
| `r16_platoon_loader.py:16, 137` | `E:\商单1100` 与 `(696,2700)` | 外部机器绝对路径与单张瓦片固定坐标，换图即失效。 |

### B. 算法漏洞、守恒破坏与死锁
1. **炮兵无 `carry` 小数累计（`artillery.py:123-126, 178-182`）**：小步长徐进弹幕算得 `0.5` 发被 `math.floor` 截断为 `0` 发直接流产；`accuracy_mult` 被 `max(1.0, ...)` 锁死，无法施加精度劣势惩罚；完全脱离 `MapPiece` 编制，全员阵亡仍能满负荷开炮。
2. **后勤负库存与兼容误判（`logistics.py:48-50, 233, 307-308, 321`）**：有 `ammo_by_type` 无 `stock["ammo"]` 时扣成负库存；补给点弹药为 `{}` 空字典时误判“全口径兼容”；自动需求漏补 `replacements` 兵员。
3. **TO&E 拆班虚增满编配额（`toe.py:963-972, 1352-1364`）**：缴获武器拆班导致双方 `equipment_full` 之和大于原班组；`split_squad_to_piece` 拆出无人装备分队按 `persons=0` 划转 0 弹药 0 补给。
4. **上级加急命令被节拍闸门拦截（`baguwen_v3.py:920` vs `961, 1005`）**：`int(tick_index) % cadence != 0` 在解析 `parent_order_fresh` 之前就 `return`，导致下级收到上级急电必须傻等到整点才处理。
5. **军官会议暴露度 2.5× + 组合爆炸（`events.py:1133-1185`）**：单次会议对中心圈、双端坐标、落圈棋子重复加 3 遍暴露，多军官同格按 $C_K^2$ 配对叠加且 Buff 不去重；另外 `radio_silence` 未进 `EVENT_SPECS`。
6. **战斗面杀伤与肉盾缺陷（`combat.py:493-505, 602-606`）**：同格多个步兵偏转的 35% 轻武器全砸到第一辆坦克首车；高爆破片对同格多单位不按密度分摊能量。
7. **`locks.py` 全线失联**：`toe.py`、`combat.py`、`artillery.py`、`ww2_air.py` 均未导入，`toe.py:798` 自己硬编码 6 种锁还漏掉 `hold_still`。
8. **无线电与指挥链漏判（`command_v2.py:380`）**：`pieces is None` 时跳过 `d_km > radio_r` 校验，允许无限距离通联；AO 附带战线默认 `mask=1`（仅步兵），与 `adjudicator_v2.py:488`（`31` 全兵种）冲突。

---

## 五 · 整改批次建议（供大王定夺）

- **第一批（止血 · 物理与执行主干）**：
  1. 修复 `autonomy_v3.py` 机动步进截断（加距离预算循环）+ 接入 `terrain_field.shortest_path` 实现真寻路，按大王定义重做“快速（最短路径寻址）/ 正常 / 慢速（速度 Debuff）”并取消写死载具速度；
  2. 修复 `adjudicator_v2.py:1953` 的 `tick_index` 平方级跳变；
  3. 贯通 `toe.py` `Squad.power()` 操炮效率与 `events.py` 重武器部署/撤收补给守恒，顺带修复 `ww2_air.py` 单方制空扫射失效与 `artillery.py` `carry` 小数累计。
- **第二批（防作弊 · 迷雾与权限隔离）**：
  1. `recon_fog.py` 接入 `tf.line_visibility` 堵死隔山透视；
  2. `events.py` 补齐工兵资质与距离校验；
  3. `web_server_v3.py` 补齐敌我权限校验、过滤 `active_events`、补充 `operate_heavy_unit` / 事件排队取消 / 单兵背包接口。
- **第三批（清理 · 残缺接口与硬编码）**：
  1. 修复 `scenario_v2.py`、`terrain_field.py` `load_npz`、`viz_v2.py` 打光、`locks.py` 统一接入；
  2. 清理 `adjudicator_v2.py` 内硬编码的 `80 / 40 / 25` 心理裁决阈值。
