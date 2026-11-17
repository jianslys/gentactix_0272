# Gentactix V3 全栈重构与弹道模块化毁伤施工总纲（交接隔壁执行版）

> **施工前提铁律**：
> 1. 动笔前先通读 `D:\tools\gentactix\BACKEND_AUDIT_REPORT.md` 与对应模块源码，严禁凭空臆造字段名、数值或接口。
> 2. **先修复后端底座与 API 契约，再改造前端 UI**；严禁前端画空壳按钮或调用后端不存在的 `op`。
> 3. **严禁删毁现有前端已有功能**：`D:\tools\gentactix\web\index.html` 现有的地图渲染、堆叠展开、单兵列表/背包弹窗、装备弹药展示、回合提交等必须完整保留。

---

## 第一部分 · 后端物理底座与 API 契约修复

### 1. 机动速度解耦 + 真最短路径寻址（`autonomy_v3.py`, `toe.py`, `web_server_v3.py`）
- **禁止写死载具速度**：移除 `autonomy_v3.py:21` 对所有轮式/履带一刀切 `12.0 km/h` 的硬编码；在 `toe.py` 中补充 `register_vehicle_spec()`，修复 `MapPiece.mobility()`（`toe.py:1083-1094`）无人载具退化为 `"foot"` 的 Bug。速度由**具体载具型号/步兵编成 + 重武器拖累/超载 + `terrain_field` 地形与有向坡度**动态决定。
- **修复单格位移截断（当前载具被阉割为 2.4 km/h）**：修复 `autonomy_v3.py:1403`（`_fast_local_step` / `_linear_step_piece`）：单次 5 分钟 Sub-tick 内必须按距离预算循环推进，严禁只走 1 格就把剩余 70%–80% 移动预算丢弃。
- **按真实语义重构三种机动模式**：
  - `fast`：接入 `terrain_field` 最短路径/最小代价寻址（自动贴路、寻桥、绕开绝壁与高阻区），不再只是直线向量乘 `1.45`；
  - `normal`：常规标准机动推进；
  - `slow` / `stealth`：直接对单位施加真实速度 Debuff，并保留隐蔽/低暴露增益。
- **耗时估算对齐**：修改 `web_server_v3.py:858-916`（`handle_estimate_plan_time`），基于上述真实寻路路径与动态速度估算各段耗时，取消每个航路点强行 `max(5, ...)` 保底 5 分钟的失真逻辑。

### 2. 重武器脱离班组 + 部署/打包 + 操炮人数 Buff 闭环（`toe.py`, `events.py`, `autonomy_v3.py`）
- **打通操炮火力**：`toe.py:414-448` `Squad.power()` 必须接入 `heavy_operator_efficiency()`（`toe.py:1369`），支持分离后的无人重武器阵地（`persons == 0`，由 `operated_by` 指向操炮班组/人员）按实际操炮人数计算火力、出弹数加成与多人 Buff（如 4 人满编操机枪）。
- **修复部署与撤收死锁**：修复 `events.py:403-409` `deploy_heavy_unit` 入口拦截 `target_unit_id` 的死锁；修复 `autonomy_v3.py:1461` 漏传 `weapon` 字段；`events.py:669` 新建阵地挂入指挥官 `subordinate_ids`；`events.py:847-856` `pack_heavy_unit` 撤收合并回母部队时回填 `stock` 与 `ammo_by_type` 并加权更新 `org` 与 `clamp_org()`。

### 3. 事件资质校验、周期 Bug、迷雾与遗留 Bug 修复（`events.py`, `adjudicator_v2.py`, `recon_fog.py`, `ww2_air.py`, `locks.py`, `terrain_field.py`）
- **工兵/通信/军官资质与距离校验**：`events.py:465-473, 610-636, 799-813` 为 `demolition`、`engineering` 补齐工兵资质、炸药/地雷消耗及目标邻近格距离校验；`clear_mines` 同步清理 `terrain_features` 雷区；`EVENT_SPECS` 补齐 `radio_silence`。
- **修复 `run_turn_v3` tick 跳变**：修复 `adjudicator_v2.py:1953` `base_turn_tick = world.turn * n_ticks` 的平方级跳变 Bug。
- **堵死迷雾透视**：`recon_fog.py:73-90` 地面侦察接入 `tf.line_visibility` 做山体高程遮蔽；`autonomy_v3.py:1086-1106` `follow_target` 丢失目标视野时停止全图 GPS 透视追击。
- **修复空军与存档断链**：`ww2_air.py:331-347` 从全图地面单位提取 `victim` 阵营；`terrain_field.py:1551-1559` `load_npz` 完整还原战线 `line_role` 等元数据；`locks.py` 接入核心模块并修复 `toe.py:798` 漏判 `hold_still`。

### 4. Web 服务 API 契约补全与鉴权（`web_server_v3.py`）
- **状态序列化修复**：`serialize_state`（206 行）不能因 `persons() <= 0` 隐藏无人载具或无人重武器阵地；`421` 行 `active_events` 严格按观察方 `team` 过滤，严禁明文泄露敌方事件。
- **权限校验**：`/api/unit_op` 与 `/api/commit_turn` 校验操作者 `team` 与指挥链归属，禁止越权操控敌方单位。
- **补齐 `/api/unit_op` 缺失路由**（与前端 1:1 对齐）：
  1. 重武器操炮指派：`operate_heavy_unit`（接收操炮人员名单/人数，返回当前人数与 Buff）；
  2. 事件入队与取消：`deploy_heavy_unit`、`pack_heavy_unit`、`demolition`、`engineering`、`lay_wire`、`cancel_event`；
  3. 补齐当前 `index.html` 已调用但后端缺失的接口：`move_soldier_item`、`pickup_body`、`drop_body`。

---

## 第二部分 · 全新弹道命中与模块化毁伤体系（废除抽象伤害数值）

> **设计主旨**：彻底废除 `combat.py` 现行 `intensity` 标量与血条指数扣减公式。**被打中就会死（高致死失血），逼迫指挥官研究战术、火力压制、掩体隐蔽与交替掩护，绝杀无脑人海平推。**

### 1. 武器与弹药物理参数（零“伤害数值”，全系统统一 `mm RHA`）
- **武器（发射器）**：只带物理发射属性——口径、兼容弹种、实战持续射速/装填时间、射程与弹道类型（`direct`/`indirect`）、基础散布（受炮管 `ext_gun`、观瞄 `optics`、移动射击 `fire_on_move` 修正）。
- **弹药（弹丸）**：只带物理弹道属性——**距离穿深曲线 `pen_mm(dist_m)`**、**距离剩余动能曲线 `ke_j(dist_m)`**、是否爆炸弹药、合法创伤域（`9mm` 允许出擦伤，`7.62mm` 及以上不出擦伤）。
- **取消过穿机制**：暂不做“过穿后穿出波及后方串联模块”的二次判定。

### 2. 5 分钟 Sub-tick 每次射击出弹数
- 严禁用“理论射速 × 5分钟”，必须按以下物理公式算出实际出弹数并**实扣对应口径弹药库存**：
  $$\text{实际出弹数} = \min\Big(\text{库存剩余该口径弹药},\ \lfloor \text{武器 5 分钟实战基准发数} \times \text{战术模式乘数} \times \text{操炮人数Buff} \times \text{压制/模块状态系数} \rfloor\Big)$$
  - 实战基准发数（含换弹/瞄准/散热间隙）：如栓动步枪 ~10–15 发、半自动 ~20–30 发、冲锋枪 ~40–80 发、单人机枪 ~60–80 发（4 人满编操机枪受 Buff 大幅翻倍提升）、坦克炮/火炮按装填时间算。
  - 压制与模块惩罚：自身 `org` 被压制越低，敢探头扣扳机的发数越少；`int_gun` 受损或装填手伤亡大幅延长装填；`cannot_fire` 或弹药归零则出 `0` 发。

### 3. 像素容器寄存与 `priority_target`（两段式统一入口，本阶段暂不做提前量）
- **统一入口铁律**：任何弹药（直射子弹、穿甲弹、高爆弹及爆炸产生的破片）**一律先打进目标格像素容器**寄存（`ShotPacket`），再由像素容器向格内单位结算，**绝不绕过容器做单位间直连伤害**。
- **本阶段暂不做提前量**：移动目标的提前量预判与跨格动态拦截算法**本轮暂不开发**，先在 `ShotPacket` 中保留 `origin_pos`、`shooter_accuracy`、`incoming_dir`、`dist_m` 等接口字段，留待后续阶段迭代。
- **阶段 1（携带 `priority_target` 寄存进像素容器）**：开火方将本 5 分钟 Sub-tick 真实打出的 `ShotPacket` 寄存至目标像素容器 `(r, c)`，附带玩家在射击任务中选定的 **`priority_target` 优先瞄准标签**（具体 `unit:<target_unit_id>`，或目标类别 `armor` / `infantry` / `heavy_weapon` / `any`；外部爆炸破片包强制标为 `any`），以及 `ammo_spec`、整数 `shots`、`origin_pos`、`incoming_dir`、`dist_m`、`shooter_accuracy`。
- **阶段 2（像素容器按标签与回退规则向格内单位结算）**：
  - 容器优先将 `ShotPacket` 匹配给符合 `priority_target` 的单位；若指定 `armor` 的直瞄反甲目标已离开或已摧毁且格内无其他载具，**穿甲弹直接打空进地形（不自动转火单点步兵）**；仅 `priority_target == any` 的覆盖火力或爆炸破片可按规则分发给格内单位。
  - **命中率核心权重**：由距离、高程遮蔽（`line_visibility`）、地形掩体、烟雾 `visibility_mod`、姿态（`fast` 暴露 vs `slow/stealth` 隐蔽）以及**射手自身压制状态（`org` 跌落导致的命中率暴跌）**决定。未命中不扣耐久，但弹着密度持续压低目标 `org`。

### 4. 单一通用弹道函数 & 破片三段式发数过滤 + 跨格转发
- **全弹种共用一个底层函数 `resolve_projectile_hit(target, ammo_spec, dist_m, ...)`**：
  - 过 `mm RHA` 穿深门禁（`pen_mm(dist_m)` vs 受击模块等效 `mm RHA`）；未击穿只扣震荡压制 `org`；
  - 击穿后按 $\sqrt{E_k(dist\_m)} \times \text{clamp}(0.6 + \dots, 0.6, 1.5) \div \text{module\_bulk}$ 扣减对应模块耐久。
- **爆炸类型弹药额外生成破片包入容器（直接套用子弹规格，全程只结算发数 `shots`）**：
  - **破片规格与千发级基数**：小口径炮弹 / 迫击炮 / 手榴弹破片套用 **`9mm`** 规格（如 82mm 迫击炮 ~150–250 片）；大口径炮弹（如 105mm/152mm）/ 重型航弹破片套用 **`.50` 或 `20mm`** 规格（按真实水平有效破片基数配置，如 105mm ~400–600 片、152mm ~800–1,200 片），**外溢跨格时不硬降档成 `9mm`**，保持原规格。
  - **炸点坐标与自动距离/方向结算**：破片 `ShotPacket` 的 `origin_pos` 设为**起爆格 `(r0, c0)`**；任意容器 `(r, c)` 收到破片包时，直接算 $\text{dist\_m} = \max\big(50,\; \sqrt{(r - r_0)^2 + (c - c_0)^2} \times 200\big)$（中心格保底 `50m`，正交邻格 `200m`，斜角邻格 `283m`）及飞来方向 `incoming_dir`，随后传入 `resolve_projectile_hit()` 顺着弹药自身的 `pen_mm(dist_m)` 与 `ke_j(dist_m)` 曲线自然衰减（破片风阻衰减应陡于流线型子弹）。
  - **整数与删包硬约束**：破片发数全程**必须为整数（`int`），一律向下取整（`floor`），任何阶段算出 `< 1` 发直接删除丢弃**（绝不留存小数破片或 0 发空包）。
  - **三段式破片发数过滤**：
    1. **容器层 · 优先算一次破片打空发数**：仅按格内单位基础体型占格权重之和 $\min(1.0, \sum W_i)$ 核销掉打在空地上的发数（避免“格内只有 1 个班就吃满全部破片”或“人多反而稀释”的悖论）；
    2. **容器层 · 按基础乘数分配发数**：将剩余有效破片发数按各单位基础体型乘数占比 $W_i / \sum W_i$ 向下取整分派给格内各单位；
    3. **单位层 · 自身暴露度与 Buff 过滤实弹发数**：每个单位收到分派发数后，结合自身**当前暴露度（卧倒/散兵/战壕）与各自 Buff**，向下取整算出最终**真正打中本单位的实弹发数 $M$**（$< 1$ 直接删除），再逐发调用 `resolve_projectile_hit()`。
  - **跨格扩散（$> 100\text{ m}$ 大口径破片容器转发）**：中心容器结算完本格后，若破片包携带的 `remaining_radius > 100m`（如 152mm 榴弹、重型航弹），扣除本格落地损耗后，将剩余可外溢发数（向下取整，`< 1` 删除）转发给周围邻格容器，同时令 `remaining_radius -= 200m`；邻格容器收到后若 `remaining_radius <= 0` 则停止转发，打空破片直接销毁，严禁无限扩散。

### 5. 人员模块化毁伤与高致死失血
- **单兵 6 模块与 `mm RHA`**：`head`（头部，裸露 `0.5 mm RHA`）、`torso`（躯干，`0.0 mm`）、四肢（`left_arm`, `right_arm`, `left_leg`, `right_leg`，各 `0.0 mm`），初始均为 `100%` 耐久；钢盔/防弹插板按穿戴装具叠加 `mm RHA` 至对应部位。
- **双态创伤与必挂失血**：
  - **`grazed`（擦伤）**：按弹药规格扣少量命中部位耐久，**无 Debuff**；
  - **`wounded`（受伤）**：按弹药规格扣命中部位耐久，且**100% 必挂失血 Debuff**；按该部位当前剩余耐久分不同等级的高致死失血速率（每 5 分钟 Sub-tick 同步扣全模块耐久：轻度约 30–60 分钟拖入重伤、中度约 15–30 分钟、严重大出血 5–15 分钟内致死）。
- **硬阈值**：
  - **重伤**：任一四肢 `< 25%` **或** 躯干 `< 50%` **或** 头部 `< 75%`（上肢失能触发 `cannot_fire`；下肢失能转为需抬护重伤员）；
  - **死亡**：躯干 `< 25%` **或** 头部 `< 55%`。

### 6. 载具模块化毁伤（外部 4 面无血条 + 内部乘员与 8 模块自结算）
- **外部装甲面（纯 `mm RHA` 门禁，无耐久度）**：车体 4 面（`hull_front`, `hull_left`, `hull_right`, `hull_rear`）与炮塔 4 面（`turret_front`, `turret_left`, `turret_right`, `turret_rear`），按 `incoming_dir` 与 `facing_deg` 判定受弹面。
- **外部暴露模块（各自带 `mm RHA` + 独立耐久度）**：`ext_gun`（火炮）、`ext_optics`（外部观瞄）、`track_left`（左履带）、`track_right`（右履带）。
- **击穿后由载具自己结算一次内部破片**：
  - 内部受击池包含 **8 个机械模块**（`int_gun` 炮闩、`int_optics` 炮手镜、`int_mg` 车载机枪、`engine` 发动机、`transmission` 传动、`fuel_tank` 油箱[可引燃]、`radio` 电台[毁坏坠入 `baguwen_v3` 断连]、`ammo_rack` 弹药架[高概率殉爆]）**+ 车内乘员**（乘员被分到的破片直接走第 5 节人员模块+失血判定）；
  - 每型载具在 `VEHICLE_SPECS`（及 `register_vehicle_spec`）中自挂一套按**被击穿装甲面**索引的内部受损乘数表，决定破片如何分摊到内部 8 模块与乘员身上。
- **三状态阶梯、使用自损与两级维修**（阈值与自损速率均按载具/部件独立配置，如履带耐用自损慢、发动机不耐用自损快）：
  - **受损（`damaged`）**：可强行使用，但**每次使用（机动/开火 Sub-tick）扣减自身耐久度**，正常维修可修；
  - **严重受损（`severely_damaged`，如 `< 30%~45%`）**：**彻底停用**（映射 `locks.py` 触发 `forbid_move` / `forbid_fire` / 致盲 / 断连），正常维修仍可修；
  - **损毁（`destroyed`，如 `< 15%~30%`）**：**停用**且常规维修无效，**只能走 `events.py` 战地事件大维修**。

---

## 第三部分 · 前端交互与 UI 全量重构（`D:\tools\gentactix\web\index.html`）

1. **兵牌右键 Windows 级选项栏（`#unitContextMenu`）**：
   - 在 Canvas 上右键命中兵牌时，拦截地图平移，弹出 Windows 风格菜单（下达任务、安排事件、编制拆分/整编、重武器部署/操炮、聚焦）；右键空白地形保持平移。
2. **独立任务 UI 窗口 & Shift 新建同类队列窗口**：
   - 从右键菜单进入，自带队列编号（`#1`, `#2`, `#3`...），内部选定 `action`、`move_mode`（`fast`/`normal`/`slow`）、`fire_mode`（`stop_to_fire`/`fire_on_move`）、**`priority_target`（优先瞄准目标：具体 `unit_id` / `armor` / `infantry` / `heavy_weapon` / `any`）**、地图选点及调 `/api/estimate_plan_time` 计算 ETA。
   - **纯粹实例隔离**：每个窗口持有独立 `TaskDraft` 实例（绝不共享全局可变配置）；窗口开着时按住 `Shift` **新建第二个同类独立窗口**作为下一个队列任务，点击“确认”后正式入列。
3. **独立事件 UI 窗口 & Shift 新建队列窗口**：
   - 同理自带编号、独立 `EventDraft` 实例与 Shift 新建下一个事件窗口，确认后加入事件队列；
   - 按单位实时人员资质过滤（无工兵禁爆破/布雷、无电台/通信兵禁无线电静默、无军官禁开会；含载具大维修事件入口）。
4. **ORBAT 卡片重构（保留既有功能，收拢为双队列看板）**：
   - 保留单兵列表、背包入口、装备、补给与伤员拾取；将任务/事件下单表单抽离进上述独立弹窗，卡片内专门存放并展示**任务队列**（排序/改点/删除）与**事件队列**（查看/取消），并在单兵与载具详情中直观展示人员 6 模块状态/失血与载具 12 部件三状态耐久。
5. **班组拆分整编窗 & 重武器部署/打包选人 UI 表**：
   - 班组编制与重武器分离（重武器放携带装备表），班组拆分/整编做成类似现单兵背包的独立勾选窗口（对接 `split_squad` / `merge_squad_pieces`）；
   - 部署与打包重武器做成人员多选 UI 表（对接 `deploy_heavy_unit` / `pack_heavy_unit` / `operate_heavy_unit`），直观展示操炮人数与当前激活 Buff。

---

## 第四部分 · 验收标准
1. **后端**：`python verify_v3_full.py` 全量通过 + 新增单测覆盖：载具真实 5 分钟位移、`fast` 真寻路、`9mm`/`7.62mm`/`.50` 穿深与 `mm RHA` 模块耐久/失血死亡判定、爆炸破片复用、载具外部无血条及内部按受弹面乘数分摊（含乘员）、部件三状态自损与大修门槛、`/api/state` 不泄露敌方事件。
2. **前端**：提取 `web/index.html` 中全部 `<script>` 执行 `node --check` 必须 0 语法错误；启动 `python web_server_v3.py` 浏览器实测右键菜单、Shift 多窗独立实例、重武器选人表、单兵/载具模块展示与全量 API 交互无报错。
