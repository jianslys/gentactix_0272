# GenTactix 全量核心函数与接口速查手册 (Function & Interface Manual)

> **本手册定位**：纯代码事实驱动的工程与接口白皮书。彻底剔除任何抽象空谈，以 **“函数名 / 接口名 ➔ 源码路径与行号 ➔ 代码签名 ➔ 入参详析 ➔ 返回值 ➔ 实际业务逻辑与物理算子 ➔ 副作用”** 为唯一骨架。

---

## 目录索引 (Table of Contents)

- [第 1 章 · 空间几何、三态边界与事务时钟 (gentactix-core: foundation)](#第-1-章--空间几何三态边界与事务时钟-gentactix-core-foundation)
  - [1.1 纯二进制 8m 空间几何 (geometry/mod.rs)](#11-纯二进制-8m-空间几何-geometrymodrs)
  - [1.2 战术实例与增广弧长积分器 (geometry/tactical_instance.rs)](#12-战术实例与增广弧长积分器-geometrytactical_instancers)
  - [1.3 三态边界隔离体系 (state/mod.rs)](#13-三态边界隔离体系-statemodrs)
  - [1.4 13 阶段确定性事务时钟 (transaction/mod.rs)](#14-13-阶段确定性事务时钟-transactionmodrs)
  - [1.5 运动约束求解与单点提交 (motion/mod.rs)](#15-运动约束求解与单点提交-motionmodrs)
  - [1.6 地形事务与级联脏区 (operators/mod.rs)](#16-地形事务与级联脏区-operatorsmodrs)
  - [1.7 权威状态哈希指纹与存盘回放 (publication/mod.rs)](#17-权威状态哈希指纹与存盘回放-publicationmodrs)
- [第 2 章 · 地形张量、寻路与主仿真循环 (gentactix-core: world & sim)](#第-2-章--地形张量寻路与主仿真循环-gentactix-core-world--sim)
- [第 3 章 · 战斗杀伤、通视与编制 ECS (gentactix-core: combat & ecs)](#第-3-章--战斗杀伤通视与编制-ecs-gentactix-core-combat--ecs)
- [第 4 章 · Axum 生产服务端与 API 路由 (gentactix-server)](#第-4-章--axum-生产服务端与-api-路由-gentactix-server)
- [第 5 章 · 现代军用制图前端与通信管线 (web/index.html)](#第-5-章--现代军用制图前端与通信管线-webindexhtml)

---

# 第 1 章 · 空间几何、三态边界与事务时钟 (gentactix-core: foundation)

## 1.1 纯二进制 8m 空间几何 (`geometry/mod.rs`)

### `WorldPos::new`
- **文件与行号**：`crates/gentactix-core/src/geometry/mod.rs:70-72`
- **真实代码签名**：`pub const fn new(x: f32, y: f32) -> Self`
- **输入参数详析**：
  - `x`: `f32`，世界坐标系 X 轴水平距离（单位：米，沿网格列方向东向延伸）；
  - `y`: `f32`，世界坐标系 Y 轴垂直距离（单位：米，沿网格行方向南向延伸）。
- **返回值**：`WorldPos` 强类型米制世界坐标结构体。
- **底层业务逻辑**：直接构造不可分、防混淆的连续浮点坐标，与网格索引严格类型隔离。
- **副作用**：无。

### `WorldPos::distance_to`
- **文件与行号**：`crates/gentactix-core/src/geometry/mod.rs:75-79`
- **真实代码签名**：`pub fn distance_to(self, other: WorldPos) -> f32`
- **输入参数详析**：
  - `other`: `WorldPos`，目标点的米制世界坐标。
- **返回值**：`f32`，两点间的欧几里得距离（单位：米）。
- **底层业务逻辑**：计算 $\Delta x = x_2 - x_1$, $\Delta y = y_2 - y_1$，调用 IEEE-754 标准 `dx.hypot(dy)` 求模。
- **副作用**：无。

### `GridGeometry::standard_1024`
- **文件与行号**：`crates/gentactix-core/src/geometry/mod.rs:110-120`
- **真实代码签名**：`pub fn standard_1024(id: u32, origin_offset_m: [f32; 2]) -> Self`
- **输入参数详析**：
  - `id`: `u32`，几何实例标识符；
  - `origin_offset_m`: `[f32; 2]`，世界原点在局部空间的偏移量（单位：米）。
- **返回值**：`GridGeometry`，预配置好的 1024x1024（全图边长 $8192\text{m} = 8.192\text{km}$）二进制战术网格基准。
- **底层业务逻辑**：将行数与列数设为 1024，格边长锁死为 `TACTICAL_CELL_SIZE_M = 8.0`（位移量 3），总面积 $67.1\text{km}^2$。
- **副作用**：无。

### `GridGeometry::world_to_cell`
- **文件与行号**：`crates/gentactix-core/src/geometry/mod.rs:145-165`
- **真实代码签名**：`pub fn world_to_cell(&self, pos: WorldPos) -> Result<CellCoord, OutsideBoundsError>`
- **输入参数详析**：
  - `pos`: `WorldPos`，待转换的米制连续世界坐标。
- **返回值**：`Result<CellCoord, OutsideBoundsError>`：成功返回离散网格整数索引 `(r, c)`；失败返回越界原因。
- **底层业务逻辑**：
  1. 检查 `pos.x < 0.0 || pos.y < 0.0`，若是则显式抛出 `OutsideBoundsError::NegativeCoordinate`（严禁负数强转无符号造成回绕）；
  2. 减去原点偏移量，计算 `c = (x / 8.0).floor() as i32`, `r = (y / 8.0).floor() as i32`；
  3. 校验 `r >= rows || c >= cols`，越界抛出 `OutsideBoundsError::OutsideWorldExtent`。
- **副作用**：无。

### `GridGeometry::cell_to_world_center`
- **文件与行号**：`crates/gentactix-core/src/geometry/mod.rs:168-176`
- **真实代码签名**：`pub fn cell_to_world_center(&self, cell: CellCoord) -> Result<WorldPos, OutsideBoundsError>`
- **输入参数详析**：
  - `cell`: `CellCoord`，离散网格坐标 `(r, c)`。
- **返回值**：`Result<WorldPos, OutsideBoundsError>`：网格几何几何中心的米制坐标。
- **底层业务逻辑**：校验网格有效性后，计算 `x = (c as f32 + 0.5) * 8.0 + offset_x`, `y = (r as f32 + 0.5) * 8.0 + offset_y`。
- **副作用**：无。

### `GridGeometry::kernel_radius_to_cell_support`
- **文件与行号**：`crates/gentactix-core/src/geometry/mod.rs:198-204`
- **真实代码签名**：`pub fn kernel_radius_to_cell_support(&self, radius_m: f32) -> i32`
- **输入参数详析**：
  - `radius_m`: `f32`，连续物理影响半径（如爆轰超压杀伤半径、毒气扩散半径，单位：米）。
- **返回值**：`i32`，需要遍历的网格半宽支持域（格子数）。
- **底层业务逻辑**：若 `radius_m <= 0.0` 返回 0；否则返回 `(radius_m / 8.0).ceil() as i32`。
- **副作用**：无。

---

## 1.2 战术实例与增广弧长积分器 (`geometry/tactical_instance.rs`)

### `quantize_pos_78125`
- **文件与行号**：`crates/gentactix-core/src/geometry/tactical_instance.rs:17-22`
- **真实代码签名**：`pub fn quantize_pos_78125(pos: [f32; 2]) -> [f32; 2]`
- **输入参数详析**：
  - `pos`: `[f32; 2]`，待量化的连续米制坐标 `[x, y]`。
- **返回值**：`[f32; 2]`，对齐到 $7.8125\text{mm} = 1/128\text{m}$ 纯二进制定点网格上的坐标。
- **底层业务逻辑**：执行 `(pos * 128.0).round() / 128.0`，彻底消除跨平台浮点漂移。
- **副作用**：无。

### `generate_quincunx_minefield`
- **文件与行号**：`crates/gentactix-core/src/geometry/tactical_instance.rs:75-125`
- **真实代码签名**：`pub fn generate_quincunx_minefield(poly: &[[f32; 2]], spacing_m: f32, seed: u64, margin_m: f32) -> Vec<[f32; 2]>`
- **输入参数详析**：
  - `poly`: `&[[f32; 2]]`，雷区多边形外廓顶点序列；
  - `spacing_m`: `f32`，地雷阵布设间距（单位：米）；
  - `seed`: `u64`，确定性伪随机数种子；
  - `margin_m`: `f32`，边界缩进安全裕量（单位：米）。
- **返回值**：`Vec<[f32; 2]>`，生成的梅花桩交错雷位数组。
- **底层业务逻辑**：
  1. 计算多边形包围盒 AABB；
  2. 初始化 `SplitMix64(seed)`；
  3. 双重循环沿 X/Y 轴按 `spacing_m` 步进，奇数行施加 `0.5 * spacing_m` 的 X 轴梅花桩交错偏移；
  4. 叠加 `[-0.15 * spacing_m, 0.15 * spacing_m]` 的伪随机微扰，使用射线法判定落入多边形内部后量化输出。
- **副作用**：无。

### `AugmentedArcIntegrator::step_16s`
- **文件与行号**：`crates/gentactix-core/src/geometry/tactical_instance.rs:240-310`
- **真实代码签名**：`pub fn step_16s(&mut self, speed_mps: f32, labor_count: u32, suppression: f32, progress_req: f32) -> (ConstructionFsmState, f32)`
- **输入参数详析**：
  - `speed_mps`: `f32`，工兵作业移动速度（米/秒）；
  - `labor_count`: `u32`，参与作业的人手数量；
  - `suppression`: `f32`，当前所受压制度（$[0.0, 100.0]$）；
  - `progress_req`: `f32`，单实例完工所需标准工时点数。
- **返回值**：`(ConstructionFsmState, f32)`：当前有限状态机状态与本回合推进的有效增广弧长 $\tilde{s}$。
- **底层业务逻辑**：
  1. 压制判定：若 `suppression >= 50.0`，FSM 切换为 `SuppressedInterrupt`，进度完全冻结；
  2. 弧长积分：计算本回合行进米数 $\Delta s = \text{speed\_mps} \times 16\text{s} \times (1.0 - \text{suppression} / 100.0)$；
  3. 边走边干：沿拓扑巡回路线推进，进度累加超过 `progress_req` 时，将当前工事实例打上转正标记。
- **副作用**：修改 `self.fsm_state`, `self.current_arc_s`, `self.completed_instances`。

### `DetonatorSwitch::can_detonate`
- **文件与行号**：`crates/gentactix-core/src/geometry/tactical_instance.rs:370-395`
- **真实代码签名**：`pub fn can_detonate(&self, operator_pos: [f32; 2]) -> Result<(), &'static str>`
- **输入参数详析**：
  - `operator_pos`: `[f32; 2]`，操作工兵当前所处的世界坐标。
- **返回值**：`Result<(), &'static str>`：满足安全与物理起爆条件返回 `Ok(())`；否则返回错误原因。
- **底层业务逻辑**：
  1. 检查 `charges_placed`（炸药已安放）与 `wire_laid`（导线已连接）；
  2. 检查 `!detonated`（严禁重复起爆）；
  3. 安全走廊核验：计算工兵位置距离所有炸点的最小欧式距离，若小于 $40.0\text{m}$ 安全距离，拒绝起爆（防止工兵自爆）。
- **副作用**：无。

---

## 1.3 三态边界隔离体系 (`state/mod.rs`)

### `TerrainState::mutate_cell`
- **文件与行号**：`crates/gentactix-core/src/state/mod.rs:71-78`
- **真实代码签名**：`pub fn mutate_cell(&mut self, r: i32, c: i32, new_elev: f32, new_type: Option<u8>)`
- **输入参数详析**：
  - `r`: `i32`，目标网格行号；
  - `c`: `i32`，目标网格列号；
  - `new_elev`: `f32`，变异后的新高程（单位：米）；
  - `new_type`: `Option<u8>`，若有则更新地貌材质（如炸毁路面为弹坑泥泞）。
- **返回值**：无。
- **底层业务逻辑**：计算一维偏移 `idx = r * cols + c`，直接覆写高程与材质数组，并将权威地形版本号递增：`self.revision = self.revision.wrapping_add(1)`。
- **副作用**：变异地形数据，触发下游派生缓存（SDF、视线遮挡）全局失效。

### `EntityState::commit_position`
- **文件与行号**：`crates/gentactix-core/src/state/mod.rs:115-125`
- **真实代码签名**：`pub fn commit_position(&mut self, id: u32, new_pos: WorldPos)`
- **输入参数详析**：
  - `id`: `u32`，实体唯一 ID；
  - `new_pos`: `WorldPos`，经过刚体与地形约束求解后的终局世界坐标。
- **返回值**：无。
- **底层业务逻辑**：在 `self.entities` 哈希表中取出实体，更新 `pos = new_pos`，并将 `self.revision` 递增。
- **副作用**：修改实体世界坐标，更新实体权威版本号。

### `DerivedState::rebuild_sdf_if_needed`
- **文件与行号**：`crates/gentactix-core/src/state/mod.rs:210-235`
- **真实代码签名**：`pub fn rebuild_sdf_if_needed(&mut self, world: &WorldState)`
- **输入参数详析**：
  - `world`: `&WorldState`，权威世界状态引用。
- **返回值**：无。
- **底层业务逻辑**：
  1. 检查派生元数据记录的 `dependency_revision` 是否与 `world.terrain.revision` 一致；
  2. 若版本落后或为空，重新基于当前地形地貌计算 Signed Distance Field (SDF) 距离场；
  3. 将版本依赖号刷新为当前地形修订号。
- **副作用**：重构内存缓存，更新派生状态版本戳。

### `TickScratch::reset_scratch`
- **文件与行号**：`crates/gentactix-core/src/state/mod.rs:411-419`
- **真实代码签名**：`pub fn reset_scratch(&mut self)`
- **输入参数详析**：无。
- **返回值**：无。
- **底层业务逻辑**：调用 `.clear()` 清空 `intent_candidates`, `motion_proposals`, `transient_blast_cell_indices`, `grid_fire_scratch`, `completed_event_mutations` 和 `diagnostics`。
- **副作用**：彻底销毁单回合瞬态工作区，确保下一回合进入洁净内存态。

---

## 1.4 13 阶段确定性事务时钟 (`transaction/mod.rs`)

### `TickClock::advance_step`
- **文件与行号**：`crates/gentactix-core/src/transaction/mod.rs:85-90`
- **真实代码签名**：`pub fn advance_step(&mut self) -> (u64, f64)`
- **输入参数详析**：无。
- **返回值**：`(u64, f64)`：`(新Tick序号, 新逻辑时间秒)`。
- **底层业务逻辑**：`self.current_tick += 1; self.logic_time_sec += self.subtick_sec as f64;`（默认每次推进 16.0 秒）。
- **副作用**：逻辑时钟前进。

### `TickOrchestrator::step_phase`
- **文件与行号**：`crates/gentactix-core/src/transaction/mod.rs:135-225`
- **真实代码签名**：`pub fn step_phase(&mut self, phase: TransactionPhase, world: &mut WorldState, derived: &mut DerivedState, scratch: &mut TickScratch) -> Result<(), TransactionError>`
- **输入参数详析**：
  - `phase`: `TransactionPhase`，当前尝试执行的事务阶段；
  - `world`: `&mut WorldState`，权威状态；
  - `derived`: `&mut DerivedState`，派生缓存；
  - `scratch`: `&mut TickScratch`，单回合瞬态工作区。
- **返回值**：`Result<(), TransactionError>`：若乱序或跳过阶段，返回 `TransactionError::PhaseOutOfOrder`。
- **底层业务逻辑**：
  1. 严格断言 `phase == self.expected_phase`，杜绝任何阶段跳跃执行；
  2. 匹配具体阶段执行对应闭环：
     - `BeginTick`：初始化工作区，重置诊断表；
     - `ClockAndSchedule`：权威时钟推进 16s；
     - `PersistentFieldEvolve`：毒气/烟雾浓度衰减；
     - `DerivedRefresh`：更新视线阻断与 SDF 缓存；
     - `ProposeMotion`：意图转位移提案（坐标只读）；
     - `ConstrainMotion`：硬碰撞与通行阻尼求解；
     - `CommitMotion`：唯一合法位置写入点；
     - `EmitEvents` / `ResolveEvents`：即时交火与爆轰结算；
     - `CommitPersistent`：提交地形与持久场；
     - `PublishAndDrop`：计算全状态指纹并 Drop 瞬态内存。
- **副作用**：推进阶段计数器，记录 PhaseTrace 追踪日志。

### `TickOrchestrator::run_full_tick`
- **文件与行号**：`crates/gentactix-core/src/transaction/mod.rs:240-260`
- **真实代码签名**：`pub fn run_full_tick(&mut self, world: &mut WorldState, derived: &mut DerivedState, scratch: &mut TickScratch) -> Result<TickSummary, TransactionError>`
- **输入参数详析**：传入三大状态的可变借用。
- **返回值**：`Result<TickSummary, TransactionError>`：包含本回合执行总阶段数（13）、耗时与权威总修订号。
- **底层业务逻辑**：按数组 `TransactionPhase::ALL` 严格顺序循环调用 `step_phase`，任何阶段失败立即中断并回滚。
- **副作用**：推进完整 16 秒推演。

---

## 1.5 运动约束求解与单点提交 (`motion/mod.rs`)

### `MotionProposal::from_intent`
- **文件与行号**：`crates/gentactix-core/src/motion/mod.rs:22-50`
- **真实代码签名**：`pub fn from_intent(intent: &MotionIntent, current_pos: WorldPos, subtick_sec: f32, body_radius_m: f32) -> Self`
- **输入参数详析**：
  - `intent`: `&MotionIntent`，仲裁后的战术机动意图（速度、朝向或目标点）；
  - `current_pos`: `WorldPos`，单位当前权威位置；
  - `subtick_sec`: `f32`，回合步长（16.0s）；
  - `body_radius_m`: `f32`，单位刚体碰撞半径。
- **返回值**：`MotionProposal`，仅表达位移倾向的无害提案。
- **底层业务逻辑**：根据极速与步长计算最大位移量，计算位移矢量 $\Delta x, \Delta y$。**严禁修改实体世界坐标**。
- **副作用**：无。

### `MotionConstraintSolver::solve`
- **文件与行号**：`crates/gentactix-core/src/motion/mod.rs:95-150`
- **真实代码签名**：`pub fn solve(&mut self, geom: &GridGeometry, world: &WorldState, derived: &DerivedState, proposal: &MotionProposal) -> ConstrainedMotion`
- **输入参数详析**：
  - `geom`: 网格几何；`world`: 权威状态；`derived`: 派生缓存；`proposal`: 机动提案。
- **返回值**：`ConstrainedMotion`：受约束后的最终目标点，包含 `is_blocked` 与阻挡原因。
- **底层业务逻辑**：
  1. 路径连续采样：将本回合移动路径按 $2.0\text{m}$ 步长进行射线细分采样，拦截高空穿隧；
  2. 沿途检查地形不可通行属性（如水面、陡峭悬崖、硬质墙体）；
  3. 检查目标单元格容纳容量（`cell_capacity_reservations`），若超载则阻断在上一安全采样点。
- **副作用**：占用单元格预留槽位。

### `commit_motions`
- **文件与行号**：`crates/gentactix-core/src/motion/mod.rs:165-180`
- **真实代码签名**：`pub fn commit_motions(world: &mut WorldState, motions: &[ConstrainedMotion]) -> usize`
- **输入参数详析**：
  - `world`: `&mut WorldState`，权威世界状态；
  - `motions`: `&[ConstrainedMotion]`，求解器批准的终局移动结果切片。
- **返回值**：`usize`，成功写入位置的实体总数。
- **底层业务逻辑**：**全引擎唯一合法写入实体物理坐标的入口点**。遍历已批准结果，调用 `world.entities.commit_position`，递增实体修订号。
- **副作用**：物理坐标正式写入权威状态。

---

## 1.6 地形事务与级联脏区 (`operators/mod.rs`)

### `TerrainTransaction::apply`
- **文件与行号**：`crates/gentactix-core/src/operators/mod.rs:37-89`
- **真实代码签名**：`pub fn apply(world: &mut WorldState, derived: &mut DerivedState, mutation: TerrainMutation, halo_cells: i32) -> (u64, DirtyHaloBox)`
- **输入参数详析**：
  - `world`: `&mut WorldState`，权威状态；
  - `derived`: `&mut DerivedState`，派生缓存；
  - `mutation`: `TerrainMutation`，地形变异参数（爆心、半径、高程升降 $\Delta h$、新地貌材质）；
  - `halo_cells`: `i32`，级联受影响的脏光环边缘缓冲格数。
- **返回值**：`(u64, DirtyHaloBox)`：新地形版本号及包围脏区域的 AABB 盒。
- **底层业务逻辑**：
  1. 遍历以爆心为原点、`radius_cells` 为半径的圆形区域，修改高程与地貌；
  2. 外扩 `halo_cells` 构建 `DirtyHaloBox`；
  3. 调用 `derived.invalidate_by_mutation(StateKind::Terrain)`，将所有依赖旧地形的派生缓存强制打上失效标记。
- **副作用**：修改地形高程张量，递增 `terrain.revision`，使下游缓存全量重算。

---

## 1.7 权威状态哈希指纹与存盘回放 (`publication/mod.rs`)

### `compute_authority_fingerprint`
- **文件与行号**：`crates/gentactix-core/src/publication/mod.rs:12-90`
- **真实代码签名**：`pub fn compute_authority_fingerprint(world: &WorldState) -> String`
- **输入参数详析**：
  - `world`: `&WorldState`，权威状态。
- **返回值**：`String`，全量权威世界状态的 64 位十六进制 SHA-256 字符串指纹。
- **底层业务逻辑**：
  1. 顺序喂入时钟（tick, logic_time, revision）；
  2. 顺序喂入地形高程数据；
  3. **稳定排序后**喂入所有实体属性（ID、坐标、朝向、编制人数、组织度）；
  4. **稳定排序后**喂入库存账本与持续环境场；
  5. 派生状态与单回合瞬态工作区绝不参与哈希，确保回放与跨网络无歧义对账。
- **副作用**：无。

### `save_world_state_to_json`
- **文件与行号**：`crates/gentactix-core/src/publication/mod.rs:93-96`
- **真实代码签名**：`pub fn save_world_state_to_json(world: &WorldState) -> Result<String, serde_json::Error>`
- **输入参数详析**：`world`: 权威状态。
- **返回值**：`Result<String, serde_json::Error>`：序列化后的 JSON 字符串。
- **底层业务逻辑**：只序列化 `WorldState` 的权威字段，过滤所有派生缓存，输出跨版本可移植的紧凑存档。
- **副作用**：无。

### `load_world_state_from_json`
- **文件与行号**：`crates/gentactix-core/src/publication/mod.rs:99-102`
- **真实代码签名**：`pub fn load_world_state_from_json(json_str: &str) -> Result<WorldState, serde_json::Error>`
- **输入参数详析**：`json_str`: `&str`，存档 JSON 文本。
- **返回值**：`Result<WorldState, serde_json::Error>`：反序列化重构的权威世界状态。
- **底层业务逻辑**：解析 JSON 恢复权威字段，派生状态需在加载后首次调用 `DerivedRefresh` 自动重建。
- **副作用**：无。


---

# 第 2 章 · 地形张量、寻路与主仿真循环 (gentactix-core: world & sim)

## 2.1 地形张量与地图流算子 (`world_grid.rs`)

### `WorldGrid::from_r16_bytes`
- **文件与行号**：`crates/gentactix-core/src/world_grid.rs:736-840`
- **真实代码签名**：`pub fn from_r16_bytes(bytes: &[u8], landuse_bytes: Option<&[u8]>, road_bytes: Option<&[u8]>, cell_m: f32, elev_min: f32, elev_max: f32, invert_y: bool) -> Result<Self, String>`
- **输入参数详析**：
  - `bytes`: `&[u8]`，16 位小端单通道灰度高程图 RAW 二进制切片；
  - `landuse_bytes`: `Option<&[u8]>`，地貌地物分类覆盖层二进制流；
  - `road_bytes`: `Option<&[u8]>`，道路/桥梁/铁路矢量光栅化掩码流；
  - `cell_m`: `f32`，单像素物理宽度（米，默认 8.0m）；
  - `elev_min` / `elev_max`: `f32`，高程映射区间的绝对海拔高度（米）；
  - `invert_y`: `bool`，Y 轴图像颠倒校准标志。
- **返回值**：`Result<WorldGrid, String>`：成功解析的高保真地形张量实例。
- **底层业务逻辑**：
  1. 解码 16-bit uint 并线性插值映射至 `[elev_min, elev_max]` 浮点高程数组 `elevation_m`；
  2. 使用经典 3x3 **Horn 梯度算法** 计算中心差分法向量，生成全图坡度张量 `slope_deg`；
  3. 识别悬崖与陡坡（坡度 $> 35^\circ$），叠加地貌摩擦系数与水文掩码。
- **副作用**：初始化全图静态碰撞与阻尼张量。

### `WorldGrid::is_literal_ground_px`
- **文件与行号**：`crates/gentactix-core/src/world_grid.rs:1058-1070`
- **真实代码签名**：`pub fn is_literal_ground_px(&self, r: i32, c: i32) -> bool`
- **输入参数详析**：
  - `r`: `i32`，待检测像素行坐标；
  - `c`: `i32`，待检测像素列坐标。
- **返回值**：`bool`：该坐标是否为真硬质陆地实地。
- **底层业务逻辑**：
  - 检查当前地表分类，若为 `WATER`（深水区）或 `BRIDGE`（桥梁悬空结构），**一律严格判定为 `false`**；
  - 只有平原、森林、山地、城镇等实地陆面才返回 `true`，防止重武器或堑壕构筑在桥面与水上。
- **副作用**：无。

### `WorldGrid::find_nearest_literal_ground_px`
- **文件与行号**：`crates/gentactix-core/src/world_grid.rs:1074-1110`
- **真实代码签名**：`pub fn find_nearest_literal_ground_px(&self, r: i32, c: i32) -> (i32, i32)`
- **输入参数详析**：
  - `r`: `i32`，起始行索引；
  - `c`: `i32`，起始列索引。
- **返回值**：`(i32, i32)`：最近合法硬质陆地网格坐标。
- **底层业务逻辑**：以 `(r, c)` 为中心向外进行曼哈顿同心圆螺旋广度优先搜索（BFS），一旦命中 `is_literal_ground_px == true` 立即返回，用于防止单位跌落水体或在桥梁爆破后卡死。
- **副作用**：无。

### `WorldGrid::get_cell_modifiers`
- **文件与行号**：`crates/gentactix-core/src/world_grid.rs:1156-1210`
- **真实代码签名**：`pub fn get_cell_modifiers(&self, r: i32, c: i32, mob: MobilityClass) -> CellModifiers`
- **输入参数详析**：
  - `r`, `c`: 网格坐标；
  - `mob`: `MobilityClass`，机动类别（`Foot` 步兵步行、`Wheeled` 轮式车、`Tracked` 履带坦克）。
- **返回值**：`CellModifiers`，包含速度乘数 `speed_mult`、视线阻挡高度 `los_block_m` 与掩蔽防御系数 `cover_mult`。
- **底层业务逻辑**：
  - 基础摩擦力查表：森林减速步兵 20%，但重度减速轮式车辆 60%；
  - 叠加动态像素修改器（如雷场增加危险、堑壕增加步兵掩护）；
  - 叠加当前格子天气影响（如泥泞对车辆通行阻尼加权）。
- **副作用**：无。

### `WorldGrid::write_typed_frontline`
- **文件与行号**：`crates/gentactix-core/src/world_grid.rs:1680-1750`
- **真实代码签名**：`pub fn write_typed_frontline(&mut self, name_id: &str, team: TeamId, commander_id: usize, echelon_mask: u32, role: FrontlineRole, facing_deg: i32, phase: u32, unit_type_mask: u8, attraction_weight: f32, blur_radius: Option<f32>, points_px: &[[f32; 2]], sample_step_px: i32) -> u32`
- **输入参数详析**：
  - `name_id`: 战术防线唯一标识；
  - `team`: 归属阵营（红/蓝）；
  - `commander_id`: 下发该防线的指挥官 ID；
  - `role`: `FrontlineRole`（`Deploy` 部署线、`Screen` 警戒掩护、`Assault` 突击发起线）；
  - `unit_type_mask`: 允许被引导的兵种位掩码（步/坦/反坦/侦察）；
  - `attraction_weight`: 势能场吸引力权重；
  - `points_px`: 防线折线控制顶点数组。
- **返回值**：`u32`，系统生成的防线唯一数字索引。
- **底层业务逻辑**：
  1. 将输入折线按 `sample_step_px` 进行密采样；
  2. 构造 `TypedFrontline` 记录并推入 `self.frontlines`；
  3. 将防线吸引力势能烘焙至对应的动态引力通道，供底层微观 AI 自主决策机动对齐。
- **副作用**：更新全图战术势能场，影响单位机动趋向。

### `WorldGrid::encode_terrain_raw_bin`
- **文件与行号**：`crates/gentactix-core/src/world_grid.rs:2015-2045`
- **真实代码签名**：`pub fn encode_terrain_raw_bin(&self) -> Vec<u8>`
- **输入参数详析**：无。
- **返回值**：`Vec<u8>`，高度压缩的紧凑二进制流。
- **底层业务逻辑**：
  - 顺序写入魔数协议头、`rows` (u32)、`cols` (u32)、`cell_m` (f32)；
  - 顺序打包 `elevation_m`（归一化为 u16）、`terrain_type` (u8) 和 `slope_deg` (u8)；
  - 前端 WebGL 接收后可直接映射为 GPU 纹理或 ArrayBuffer，耗时 $< 20\text{ms}$。
- **副作用**：无。

---

## 2.2 仿真主循环与 A* 寻路优化 (`sim_loop.rs`)

### `plan_grid_path_f32`
- **文件与行号**：`crates/gentactix-core/src/sim_loop.rs:166-260`
- **真实代码签名**：`pub fn plan_grid_path_f32(grid: &WorldGrid, start: [f32; 2], goal: [f32; 2], mob: MobilityClass) -> Vec<[f32; 2]>`
- **输入参数详析**：
  - `grid`: `&WorldGrid`，当前世界地形张量；
  - `start`: `[f32; 2]`，起点米制坐标；
  - `goal`: `[f32; 2]`，终点米制坐标；
  - `mob`: `MobilityClass`，兵种机动类型。
- **返回值**：`Vec<[f32; 2]>`，A* 算法规划出的最优航路折线点序列。
- **底层业务逻辑**：
  1. 曼哈顿启发式二叉堆优先队列 `BinaryHeap` A* 实现；
  2. 针对对角线移动施加 $\sqrt{2}$ 惩罚与切角禁行判定；
  3. 节点代价 $g(n)$ 叠加地貌摩擦、道路加成与实时天气泥泞阻尼；
  4. 回溯最优网格路径，并在输出前进行视线拉直（String Pulling / LOS Smoothing），消除锯齿折角。
- **副作用**：无。

### `SimWorld::step_subtick_16s`
- **文件与行号**：`crates/gentactix-core/src/sim_loop.rs:2008-2320`
- **真实代码签名**：`pub fn step_subtick_16s(&mut self, driver: &mut dyn CommanderDriver) -> SubtickReport`
- **输入参数详析**：
  - `driver`: `&mut dyn CommanderDriver`，负责高阶指挥 AI 决策的抽象驱动接口。
- **返回值**：`SubtickReport`，详细推进报告（移动兵力数、交火次数、伤亡、SHA-256 状态指纹）。
- **底层业务逻辑**：**全仿真引擎的心脏驱动函数**，执行固定 16 秒推演：
  1. **时钟递进**：`self.subtick += 1`, `self.clock_hours += 16.0 / 3600.0`；
  2. **指令下发**：通过 `driver` 刷新各级指挥部通讯，排队执行战术意图；
  3. **空间分区重建**：刷新 `SpatialBucketGrid` 空间哈希桶，做极速近邻索敌；
  4. **机动与碰撞**：遍历存活单位，沿航路步进，更新微观朝向与人员疲劳；
  5. **交火与破片**：进行直瞄与火炮射击，结算命中、贯穿与人员伤亡；
  6. **电传战报生成**：生成弹药消耗与战损日志，计算全系统物质守恒校验；
  7. **计算权威哈希**：对全量单位坐标与兵力计算 SHA-256 指纹。
- **副作用**：全盘状态推进 16 秒，更新所有单位物理参数，产出最新战况日志。

### `build_birch_river_512_world`
- **文件与行号**：`crates/gentactix-core/src/sim_loop.rs:3157-3360`
- **真实代码签名**：`pub fn build_birch_river_512_world(_seed: u64) -> SimWorld`
- **输入参数详析**：
  - `_seed`: `u64`，地图生成随机种子。
- **返回值**：`SimWorld`，标准桦木河 512x512 预设沙盘。
- **底层业务逻辑**：
  - 构建一条贯穿地图中央的桦木河水系与核心桥梁；
  - 部署红军守备步兵连（配属 81mm 迫击炮与 Pak38 50mm 反坦克炮）；
  - 部署蓝军突击装甲排（配属 T-34/76 坦克与冲锋枪组）；
  - 设置双方补给站、前沿雷场与铁丝网工事。
- **副作用**：分配完整战备沙盘内存。

---

## 2.3 外置时间机器与错峰调度 (`time_machine.rs`)

### `mix_time_unit_hash`
- **文件与行号**：`crates/gentactix-core/src/time_machine.rs:31-35`
- **真实代码签名**：`pub fn mix_time_unit_hash(abs_sec: u64, unit_name: &str) -> u64`
- **输入参数详析**：
  - `abs_sec`: `u64`，绝对逻辑时间秒数；
  - `unit_name`: `&str`，单位唯一代号字符串。
- **返回值**：`u64`，高度混淆且跨平台一致的 64 位确定性哈希。
- **底层业务逻辑**：
  $$M = \operatorname{SplitMix64}\Big(\operatorname{SplitMix64}(\text{abs\_sec}) \oplus \operatorname{rotate\_left}\big(\operatorname{FNV1a\_64}(\text{unit\_name}), 27\big)\Big)$$
- **副作用**：无。

### `TimeMachinePendingQueue::enqueue_operation`
- **文件与行号**：`crates/gentactix-core/src/time_machine.rs:209-242`
- **真实代码签名**：`pub fn enqueue_operation(&mut self, current_tick: u64, abs_sec: u64, cat: TimeCategory, unit_name: &str, turn_subticks: u32, raw_op_payload: serde_json::Value) -> u64`
- **输入参数详析**：
  - `current_tick`: 当前推演 Tick；
  - `abs_sec`: 逻辑秒；
  - `cat`: `TimeCategory`（`Bullet`, `Shell`, `Command`, `Event`, `Movement`）；
  - `unit_name`: 下达指令的单位名；
  - `turn_subticks`: 跨回合步长；
  - `raw_op_payload`: 具体战术操作的 JSON Payload。
- **返回值**：`u64`，分配的全局唯一错峰排队工单号（Ticket ID）。
- **底层业务逻辑**：
  1. 利用双哈希算子提取时间常量表中的槽位索引 `idx = (hash >> 32) & 0x0F`；
  2. 查表获取物理动作真实微观毫秒延迟 `exact_ms` 与错峰跳步 `turn_skip`；
  3. 计算目标激活时间 `due_tick = current_tick + turn_skip`；
  4. 构建 `PendingOpTicket` 压入优先队列。
- **副作用**：在队列中注册延期执行的战术操作。

### `TimeMachinePendingQueue::drain_due_operations`
- **文件与行号**：`crates/gentactix-core/src/time_machine.rs:246-258`
- **真实代码签名**：`pub fn drain_due_operations(&mut self, current_tick: u64) -> Vec<PendingOpTicket>`
- **输入参数详析**：
  - `current_tick`: `u64`，当前到达的物理推进 Tick。
- **返回值**：`Vec<PendingOpTicket>`，所有在此时刻已成熟到期的战术工单列表。
- **底层业务逻辑**：提取所有满足 `due_tick <= current_tick` 的工单，从排队链表中弹出并交付引擎物理生效。
- **副作用**：排队队列出栈。


---

# 第 3 章 · 战斗杀伤、通视与编制 ECS (gentactix-core: combat & ecs)

## 3.1 战术通视与弹道杀伤算子 (`combat_logistics.rs`)

### `cast_los_ray_dda`
- **文件与行号**：`crates/gentactix-core/src/combat_logistics.rs:40-105`
- **真实代码签名**：`pub fn cast_los_ray_dda(grid: &WorldGrid, a_px: (i32, i32), b_px: (i32, i32), night: bool) -> LosResult`
- **输入参数详析**：
  - `grid`: `&WorldGrid`，地形张量；
  - `a_px`: `(i32, i32)`，观察者/射击源网格像素坐标 `(row, col)`；
  - `b_px`: `(i32, i32)`，目标点网格像素坐标 `(row, col)`；
  - `night`: `bool`，当前是否处于夜战状态（夜间基础视距衰减至 40%）。
- **返回值**：`LosResult { visibility: f32, blocked: bool, blocking_px: Option<(i32, i32)> }`：能见度因数、是否被阻断、首个阻挡视线的掩体/地形坐标。
- **底层业务逻辑**：
  1. 设定视线两端高度为地表高程 $+ 2.0\text{m}$（人体/潜望镜高度）；
  2. 计算 $\Delta r, \Delta c$，以 $\max(|\Delta r|, |\Delta c|)$ 为总步数执行标准 **整数 DDA 线性插值**；
  3. 沿途逐像素核算：视线在采样点的高度 $z_{\text{ray}}(t) = z_a + (z_b - z_a) \times t$；
  4. 比较 $z_{\text{ray}}(t)$ 与当前地形高程加障碍物高度 $z_{\text{terrain}} + \text{los\_block\_m}$；
  5. 一旦视线低于障碍顶高，立即判定 `blocked = true` 并记录 `blocking_px`。
- **副作用**：无。

### `compute_fire_arc`
- **文件与行号**：`crates/gentactix-core/src/combat_logistics.rs:113-210`
- **真实代码签名**：`pub fn compute_fire_arc(grid: &WorldGrid, pos: [f32; 2], max_range_m: f32, min_range_m: f32, indirect: bool, night: bool, max_scan_m: f32) -> Vec<FireArcCell>`
- **输入参数详析**：
  - `pos`: 武器当前物理坐标；
  - `max_range_m` / `min_range_m`: 最大射程与最小射击死区（米）；
  - `indirect`: `bool`，是否曲射（间瞄火炮可越过低矮地形，直瞄武器遇遮挡即止）；
  - `night`: 夜战模式；
  - `max_scan_m`: 空间扫描硬截断半径。
- **返回值**：`Vec<FireArcCell>`，包含可打击单元格列表 `(r, c)` 及该格综合通视效率 `eff`。
- **底层业务逻辑**：
  - 对有效射程圆环内的网格点进行角度分段扫描；
  - 直瞄武器调用 `cast_los_ray_dda` 判定通视，阻挡点之后的扇区全部剔除；
  - 曲射火炮（`indirect = true`）豁免中间视线遮挡，仅核查最大射程与弹道下坠余隙。
- **副作用**：无。

### `resolve_cell_strike_bucketed`
- **文件与行号**：`crates/gentactix-core/src/combat_logistics.rs:396-510`
- **真实代码签名**：`pub fn resolve_cell_strike_bucketed(grid: &mut WorldGrid, units: &mut UnitSoA, commanders: &CommanderSoA, events: &mut EventQueue, buckets: Option<&SpatialBucketGrid>, target_px: (i32, i32), firing_team: TeamId, fire: [f32; 6], shell_name: &str, firing_pos: [f32; 2], is_direct: bool, damage_flavor: &str, night: bool, spotter_uid: Option<UnitId>) -> StrikeReport`
- **输入参数详析**：
  - `grid`, `units`, `commanders`, `events`: 核心仿真组件可变借用；
  - `target_px`: 弹着点网格中心；
  - `fire`: `[f32; 6]`，弹药杀伤参数数组（高爆威力、破片动能、穿甲深度、震荡压制、发烟发火）；
  - `shell_name`: 炮弹型号代号（如 `howitzer_152`, `mortar_81`）；
  - `is_direct`: 是否直瞄。
- **返回值**：`StrikeReport`，战损报告（人员阵亡数、装备毁伤数、组织度衰减、弹坑深度）。
- **底层业务逻辑**：
  1. 以落点为中心应用 **2D 高斯散布椭圆** 衰减爆轰超压与破片通量；
  2. 检索落点附近空间桶内的所有步兵与载具；
  3. 扣减人员耐久（`PERSON_DURABILITY = 15.0`）与组织度（`ORG_DURABILITY = 25.0`）；
  4. 计算高爆弹坑：调用 `grid.mutate_cell` 产生微观弹坑下陷，将地表改为泥泞。
- **副作用**：扣减单位血量与人员数，生成地面弹坑。

### `gather_intel`
- **文件与行号**：`crates/gentactix-core/src/combat_logistics.rs:762-850`
- **真实代码签名**：`pub fn gather_intel(team: TeamId, grid: &WorldGrid, units: &UnitSoA, night: bool) -> Vec<ContactEntry>`
- **输入参数详析**：
  - `team`: 观察方阵营（红/蓝）；
  - `grid`: 地形张量；
  - `units`: 单位 SoA；
  - `night`: 夜战标志。
- **返回值**：`Vec<ContactEntry>`，侦察汇总的敌军接触列表（包含估计位置、迷雾级别 `fog_level`）。
- **底层业务逻辑**：
  1. 遍历友方所有存活哨戒兵、侦察组与观察所；
  2. 结合光学观测距离（`BASE_OBSERVE_KM = 1.5`）与高处俯瞰视野加成；
  3. 计算针对敌方单位的信噪比与 DDA 遮挡，按距离与掩体量化为三级接触：
     - `Level 1`（声音/雷达接触：仅知大致区域）；
     - `Level 2`（视线观察：得知兵种与方向）；
     - `Level 3`（完全判明：精确锁定坐标与编制）。
- **副作用**：无。

---

## 3.2 编制与单兵 ECS 运维算子 (`units_ecs.rs`)

### `UnitSoA::split_unit`
- **文件与行号**：`crates/gentactix-core/src/units_ecs.rs:2649-2720`
- **真实代码签名**：`pub fn split_unit(&mut self, src: UnitId, take_enlisted: u32, take_cadre: u32, new_str_id: &str) -> Result<UnitId, String>`
- **输入参数详析**：
  - `src`: `UnitId`，母班组 ID；
  - `take_enlisted`: `u32`，拆出划转的列兵人数；
  - `take_cadre`: `u32`，拆出划转的士官骨干人数；
  - `new_str_id`: `&str`，派生新战术小组的字符串代号。
- **返回值**：`Result<UnitId, String>`：新生成的战斗小组 `UnitId`。
- **底层业务逻辑**：
  1. 严格核验母班组剩余兵力：`src.enlisted >= take_enlisted` 且 `src.cadre >= take_cadre`；
  2. 严禁掏空母班组（母班组拆后必须至少保留 1 名人员）；
  3. 从母班组士兵列表 `self.soldiers[src]` 中弹出指定数量士兵，装配至新班组；
  4. 新班组继承母班组坐标、阵营与指挥官链接，军阶自动设为战斗小组（`Echelon::Team`）；
  5. 重新同步两组的三账本数据。
- **副作用**：分配新实体，变异母班组编制。

### `UnitSoA::merge_squads_direct`
- **文件与行号**：`crates/gentactix-core/src/units_ecs.rs:2743-2810`
- **真实代码签名**：`pub fn merge_squads_direct(&mut self, dst: UnitId, src: UnitId) -> Result<u32, String>`
- **输入参数详析**：
  - `dst`: `UnitId`，保留的主力接收班组；
  - `src`: `UnitId`，将被解散吸收的残破班组。
- **返回值**：`Result<u32, String>`：合并后的总编制人数。
- **底层业务逻辑**：
  1. 校验两班组同属一个阵营，且物理距离不超过合并接触阈值；
  2. 将 `src` 的全体士兵、士官背包与装备资产无损移入 `dst`；
  3. 将 `src.alive` 标为 `false`，解散注销其编制实体；
  4. 刷新 `dst` 的人员汇总与装备加权。
- **副作用**：注销 `src` 实体，扩充 `dst` 实体。

### `UnitSoA::set_heavy_operators`
- **文件与行号**：`crates/gentactix-core/src/units_ecs.rs:2381-2440`
- **真实代码签名**：`pub fn set_heavy_operators(&mut self, id: UnitId, op_uid: Option<UnitId>, soldier_ids: Vec<String>, op_count: u32) -> Result<(), String>`
- **输入参数详析**：
  - `id`: 重武器阵地（架设的重机枪/迫击炮）实体 ID；
  - `op_uid`: 派遣操具士兵的母班组 ID；
  - `soldier_ids`: 具体指派接管操具的单兵 UUID 列表；
  - `op_count`: 操具人数。
- **返回值**：`Result<(), String>`：成功与否。
- **底层业务逻辑**：
  - 建立班组士兵与重武器实体的“人装结合绑定”；
  - 操具士兵进入专职操炮状态，无法射击个人步枪；
  - 计算满编操具率，根据士官数量赋予该武器射速与装填时间加成。
- **副作用**：更新两实体的从属关系与射击效能参数。

### `UnitSoA::sync_totals_from_soldiers`
- **文件与行号**：`crates/gentactix-core/src/units_ecs.rs:2097-2150`
- **真实代码签名**：`pub fn sync_totals_from_soldiers(&mut self, id: UnitId)`
- **输入参数详析**：`id`: 目标班组 ID。
- **返回值**：无。
- **底层业务逻辑**：
  - 遍历底层 `self.soldiers[id]` 数组；
  - 重新精确统计总列兵数、士官数、健康状况；
  - 重新扫描每个单兵背包中的武器与弹药，汇总统合至 `self.equipment[id]` 账本，杜绝微观单兵与宏观编制数据断裂。
- **副作用**：更新宏观账本。

---

## 3.3 指挥链与通信算子 (`command_log.rs`)

### `can_commander_write_line`
- **文件与行号**：`crates/gentactix-core/src/command_log.rs:249-265`
- **真实代码签名**：`pub fn can_commander_write_line(player_cid: CommanderId, target_cid: CommanderId) -> bool`
- **输入参数详析**：
  - `player_cid`: 发起指令的玩家指挥官 ID；
  - `target_cid`: 防线被指派的目标指挥官 ID。
- **返回值**：`bool`：是否有权写入战术线。
- **底层业务逻辑**：
  - 允许指挥官为自己下辖部队绘制防线（`player_cid == target_cid`）；
  - 允许上级指挥官为下级编制绘制防线（军衔压制）；
  - **严禁平级越权或下级向上级瞎指挥**。
- **副作用**：无。

### `effective_commander_radius_km`
- **文件与行号**：`crates/gentactix-core/src/command_log.rs:126-155`
- **真实代码签名**：`pub fn effective_commander_radius_km(cmdr_id: CommanderId, commanders: &CommanderSoA, units: &UnitSoA, _grid: &WorldGrid) -> f32`
- **输入参数详析**：
  - `cmdr_id`: 指挥官 ID；`commanders`, `units`: ECS 组件。
- **返回值**：`f32`，该指挥官光环覆盖的物理公里半径。
- **底层业务逻辑**：
  - 基准半径：连长 $1.2\text{km}$，营长 $3.5\text{km}$，团长 $8.0\text{km}$；
  - 配备车载大功率电台加成 $+50\%$；
  - 若处于无线电静默或司令部遭受炮击压制，指挥半径衰减至 $30\%$。
- **副作用**：无。

---

## 3.4 欧拉场代数与即时事件 (`fields/mod.rs` & `events/mod.rs`)

### `TransientBlastWorkspace::splat_explosion`
- **文件与行号**：`crates/gentactix-core/src/fields/mod.rs:33-85`
- **真实代码签名**：`pub fn splat_explosion(&mut self, geom: &GridGeometry, origin: CellCoord, radius_m: f32, peak_kpa: f32, fragments: u32, event_id: u64) -> Result<usize, &'static str>`
- **输入参数详析**：
  - `origin`: 爆炸网格原点；`radius_m`: 物理半径；`peak_kpa`: 爆心峰值超压；`fragments`: 破片总发数；`event_id`: 溯源事件号。
- **返回值**：`Result<usize, &'static str>`：受波及的单元格总数。
- **底层业务逻辑**：
  1. 遍历支持域内的所有格子；
  2. 超压包络应用 `FieldAlgebra::Max` 代数（记录峰值）；
  3. 破片通量应用 `FieldAlgebra::Sum` 代数，严格使用 Rust `checked_add` 防溢出；
  4. 记录溯源事件 ID，Tick 结束由工作区全量清空。
- **副作用**：修改单回合瞬态爆轰内存。

### `execute_single_fire_slice`
- **文件与行号**：`crates/gentactix-core/src/fields/mod.rs:94-175`
- **真实代码签名**：`pub fn execute_single_fire_slice(world: &mut WorldState, derived: &DerivedState, scratch: &mut TickScratch, event: &InstantEvent) -> EventFinalStatus`
- **输入参数详析**：三大世界状态与当前处理的射击事件。
- **返回值**：`EventFinalStatus`：四大确定性终态之一（`Resolved`, `Rejected`, `DiscardedByRule`, `ConsumedByField`）。
- **底层业务逻辑**：
  - 贯通单一火力竖切：核验射手弹药库存 ➔ 扣除库存 ➔ 生成瞬态杀伤场 ➔ 结算目标伤害 ➔ 若有弹坑调用地形变异 ➔ 归档终态原因码。
- **副作用**：扣减弹药、造成伤害、生成弹坑。


---

# 第 4 章 · Axum 生产服务端与 API 路由 (gentactix-server)

## 4.1 核心 HTTP 路由与 Handler 汇总

### `get_terrain_raw`
- **文件与行号**：`crates/gentactix-server/src/main.rs:877-888`
- **请求方式与路由**：`GET /api/terrain_raw.bin`
- **入参 Query / Headers**：无。
- **返回 HTTP 状态与 Payload**：`200 OK`, `Content-Type: application/octet-stream`，二进制高程、材质与水文流。
- **底层业务逻辑**：获取 `SimWorld` 读锁，调用 `w.grid.encode_terrain_raw_bin()` 导出紧凑字节流返回。
- **副作用**：无。

### `get_state`
- **文件与行号**：`crates/gentactix-server/src/main.rs:984-1000`
- **请求方式与路由**：`GET /api/state?commander_id=...&team=...&seat_token=...`
- **入参 Query Schema**：
  - `commander_id`: `Option<String>`，当前客户端指挥官代号；
  - `team`: `Option<String>`，阵营过滤（`"red"` 或 `"blue"`）；
  - `seat_token`: `Option<String>`，多人席位安全口令。
- **返回 HTTP 状态与 Payload**：
  - `200 OK`: JSON 战场态势包，包含：
    - `units`: 该指挥官视野迷雾内可见的所有友军与已侦察敌军；
    - `commanders`: 指挥体系链路状态；
    - `frontlines`: 当前有效的战术攻防线；
    - `teletype`: 实时电传战报日志流水；
    - `mp`: 多人席位就绪状态；
    - `state_revision`: 状态版本号。
  - `403 FORBIDDEN`: 席位 Token 不匹配时拒止。
- **底层业务逻辑**：校验席位口令后，调用 `serialize_state(&w, ...)` 进行战争迷雾视野裁剪，屏蔽视距外敌军真实数据。
- **副作用**：无。

### `post_weapon_los`
- **文件与行号**：`crates/gentactix-server/src/main.rs:1097-1128`
- **请求方式与路由**：`POST /api/weapon_los`
- **JSON Payload Schema**：
  ```json
  {
    "piece_id": "infantry_squad_1",
    "weapon_model": "pak38_50mm"
  }
  ```
- **返回 HTTP 状态与 Payload**：
  - `200 OK`:
    ```json
    {
      "ok": true,
      "piece_id": "infantry_squad_1",
      "weapon_model": "pak38_50mm",
      "range_m": 1500.0,
      "indirect": false,
      "cells": [[120, 134, 0.95], [120, 135, 0.88]],
      "targetable_enemies": [{"id": "t34_tank_2", "name": "T-34/76", "px": [120, 135], "eff": 0.88}]
    }
    ```
- **底层业务逻辑**：根据装备射表提取射程，调用核心 `compute_fire_arc` 执行 DDA 射线追踪，遍历扇区内所有存活敌军单位，返回射击覆盖网格与可瞄准目标清单。
- **副作用**：无。

### `post_preview_plan`
- **文件与行号**：`crates/gentactix-server/src/main.rs:2915-2980`
- **请求方式与路由**：`POST /api/preview_plan`
- **JSON Payload Schema**：
  ```json
  {
    "plan_type": "move",
    "piece_id": "recon_squad_3",
    "start_pos": [120.0, 340.0],
    "goal_pos": [250.0, 480.0],
    "lambda": 0.5
  }
  ```
- **返回 HTTP 状态与 Payload**：
  - `200 OK`:
    ```json
    {
      "ok": true,
      "plan_type": "move",
      "streamline": [[120.0, 340.0], [185.0, 410.0], [250.0, 480.0]],
      "waypoints": [[15, 42], [23, 51], [31, 60]],
      "dist_m": 1824.5,
      "eta_sec": 456.1,
      "fatigue_cost": 0.31
    }
    ```
- **底层业务逻辑**：提取该单位机动类型，调用 `plan_grid_path_f32` 执行 A* 网格寻路；计算 RK2 连续流线平滑点序列；结合实际移速与地形阻尼核算全程里程、预期耗时（ETA）与人员疲劳代价。
- **副作用**：无。

### `post_commit_turn`
- **文件与行号**：`crates/gentactix-server/src/main.rs:2695-2913`
- **请求方式与路由**：`POST /api/commit_turn`
- **JSON Payload Schema**：
  ```json
  {
    "sync_barrier": true,
    "force": false,
    "player_commander_id": "red_hq",
    "planned_actions": [...],
    "planned_events": [...],
    "subticks": 1
  }
  ```
- **返回 HTTP 状态与 Payload**：
  - `200 OK`:
    ```json
    {
      "ok": true,
      "report": {"turn": 5, "clock_hours": 1.25, "last_fingerprint": "..."},
      "state": {...},
      "time_machine_frame": {"particle_tracks": [...]}
    }
    ```
  - `409 CONFLICT`: 开启 `sync_barrier` 时若尚有席位未处于 `ready` 状态，拒绝推进并返回等待提示。
- **底层业务逻辑**：
  1. 多人同步屏障核验；
  2. 批量将下达的 `planned_actions` 与 `planned_events` 注入物理推演世界；
  3. 激活时间机器中到期的错峰工单 `drain_due_operations`；
  4. 循环调用 `step_subtick_16s` 推进时钟；
  5. 打包生成前端 60fps 平滑渲染所需的 `time_machine_frame` 粒子插值帧（标记预渲染环 `is_prerender_skirt` 与敌军突现可见度）。
- **副作用**：推进全服物理时钟，变异世界状态，生成最新回合指纹。

---

## 4.2 战术行动统一分发路由 (`post_unit_op` 详解)

接口入口：`POST /api/unit_op`，根据 JSON 报文中的 `op` 字段进行路由分发：

### 1. `split_squad`（步兵战术拆组）
- **文件与行号**：`main.rs:1287-1341`
- **关键入参**：`piece_id`, `take_enlisted` (划转列兵数), `take_cadre` (划转士官数), `immediate` (布尔值)。
- **业务逻辑**：权限核验 ➔ 若 `immediate: true` 调用 `w.units.split_unit` 原地派生新战斗小组；若为 `false`，则压入时间机器生成错峰排队工单。
- **副作用**：生成新单位实体，扣减母班组兵员。

### 2. `merge_squads`（残部战场合并）
- **文件与行号**：`main.rs:1342-1412`
- **关键入参**：`target_piece_id` (接收方), `source_piece_id` (被合并方), `immediate`。
- **业务逻辑**：核验两单位同阵营且间距合法 ➔ 调用 `w.units.merge_squads_direct` 将人员装备资产平账至接收方，注销源单位。
- **副作用**：解散注销源单位实体。

### 3. `direct_action`（下达即时战术行动指令）
- **文件与行号**：`main.rs:1654-1757`
- **关键入参**：`actor_id`, `target_id`, `action` (`"move"` | `"attack"` | `"assault"` | `"retreat"` | `"stop"`), `goal` (`[x, y]`), `move_mode`, `fire_mode`, `priority_target`。
- **业务逻辑**：
  - 严密权限校验：班组仅能控制自身；指挥官仅能控制指挥光环半径内的直属编制；
  - 调用 `w.try_set_direct_unit_order_v33` 覆写航路目标点与战术交战规则。
- **副作用**：更新单位任务队列与机动航向。

### 4. `write_typed_frontline`（绘制战术攻防线）
- **文件与行号**：`main.rs:1555-1604`
- **关键入参**：`commander_id`, `points_px` (折线顶点坐标数组), `role`, `unit_type_mask`, `attraction_weight`。
- **业务逻辑**：校验指挥官指挥权与参谋部士官存活 ➔ 调用 `w.grid.write_typed_frontline` 烘焙势能通道。
- **副作用**：修改全图战术防线吸引力场。

### 5. `deploy_heavy`（架设重武器）
- **文件与行号**：`main.rs:2391-2452`
- **关键入参**：`piece_id`, `weapon`, `keep_manned`, `operator_soldier_ids`, `operator_count`。
- **业务逻辑**：将班组装备中的重机枪/迫击炮展开为地面固化阵地实体，指派操具士兵锁定操炮。
- **副作用**：生成重武器阵地实体。

### 6. `pack_heavy`（撤收重武器）
- **文件与行号**：`main.rs:2486-2536`
- **关键入参**：`piece_id`, `target_unit_id`, `attach`。
- **业务逻辑**：将展开的重武器转为携行/牵引态，挂接回步兵班组背包或卡车挂钩。
- **副作用**：注销阵地实体，恢复携行状态。

### 7. `operate_heavy_unit`（接管操具）
- **文件与行号**：`main.rs:2453-2485`
- **关键入参**：`piece_id` (操作方班组), `target_unit_id` (无人值守重武器), `operator_soldier_ids`。
- **业务逻辑**：指派步兵接管遗留在战场的重武器，重算射击效能。
- **副作用**：更新重武器操控权。

### 8. `engineering`（工兵阵地工程作业）
- **文件与行号**：`main.rs:2247-2274`
- **关键入参**：`piece_id`, `eng_op` (`"dig_trench"` 掘壕 / `"lay_mines"` 布雷 / `"clear_mines"` 排雷 / `"build_bridge"` 架桥), `target_px`, `duration_h`。
- **业务逻辑**：调用 `w.events.start_engineering` 挂起工兵作业定时事件，按进度变异网格地貌。
- **副作用**：向事件队列压入长程施工任务。

### 9. `demolition`（工兵定向爆破）
- **文件与行号**：`main.rs:2275-2298`
- **关键入参**：`piece_id`, `target_px`, `target_type` (`"bridge"` 桥梁爆破)。
- **业务逻辑**：指派工兵安装炸药并连接起爆开关，摧毁战术关键通道。
- **副作用**：瓦解桥梁连接，阻断重装备通行。

### 10. `detonate_switch`（遥控引爆预埋炸药）
- **文件与行号**：`main.rs:2585-2605`
- **关键入参**：`target_px`。
- **业务逻辑**：核验工兵撤离至安全距离后起爆，调用 `resolve_cell_strike_bucketed` 产生巨量爆轰与人员毁灭。
- **副作用**：产生巨型弹坑与人员伤亡。

### 11. `cargo_transfer`（后勤补给物资转运）
- **文件与行号**：`main.rs:2347-2372`
- **关键入参**：`piece_id` (供方), `target_piece_id` (需方), `food_t`, `ammo_t`, `fuel_t`。
- **业务逻辑**：两单位贴近后进行弹药与燃油吨位平账，严格保证物质守恒。
- **副作用**：变异两单位库存账本。

### 12. `resupply_pickup`（前沿班组领弹）
- **文件与行号**：`main.rs:2373-2390`
- **关键入参**：`piece_id`, `ammo_t`, `food_t`。
- **业务逻辑**：前沿步兵从临近补给站补充基数弹药。
- **副作用**：增加步兵携行弹药。

### 13. `move_soldier_item`（单兵微观背包与地面装备交互）
- **文件与行号**：`main.rs:1865-2054`
- **关键入参**：`from_kind`, `to_kind`, `from_soldier_id`, `to_soldier_id`, `item_index`, `to_slot` (`"hand"` 挂载手部持枪 / `"pack"` 放入背包)。
- **业务逻辑**：支持单兵之间互相传递弹夹、将地面阵亡者武器拾取装入背包、或将重武器扔在地上成为遗落物资。
- **副作用**：更新单兵格子背包，同步编制装备总账。

### 14. `pickup_body` / `drop_body`（搬运阵亡战友/遗落物资）
- **文件与行号**：`main.rs:1796-1864`
- **关键入参**：`target_id`, `body_id`。
- **业务逻辑**：将战场阵亡者遗体与随身装备抬上担架携行，或卸在后方野战医院。
- **副作用**：维护 `ground_bodies` 地面物品容器。

### 15. `war_council`（召开战术参谋协调会）
- **文件与行号**：`main.rs:2563-2584`
- **关键入参**：`commander_id`, `partner_id`, `duration_h`。
- **业务逻辑**：两级指挥官进行协同对表，赋予战术协调光环（`briefing_buff_hours`），大幅压降多端命令冲突带来的震荡惩罚（`order_churn_score`）。
- **副作用**：刷新指挥官协同属性。

### 16. `set_radio`（电台通信与无线电静默）
- **文件与行号**：`main.rs:1605-1635`
- **关键入参**：`entity_id`, `radio_channel`, `radio_silence`。
- **业务逻辑**：调整步兵或司令部无线电通讯频道（A~G 信道），开启静默可防止无线电测向侦听，但失去指挥光环支持。
- **副作用**：改变单位通信监听状态。


---

# 第 5 章 · 现代军用制图前端与通信管线 (web/index.html)

## 5.1 地形二进制流与瓦片渲染管线

### `loadTrueTerrainData`
- **文件与行号**：`web/index.html:920-942`
- **真实函数签名**：`async function loadTrueTerrainData()`
- **输入参数**：无。
- **返回值**：`Promise<void>`。
- **底层渲染逻辑**：
  1. 通过 `fetch('/api/terrain_raw.bin')` 拉取后端导出的纯二进制高程图流；
  2. 使用 `DataView` 按字节偏移量解码：
     - 读取魔数与尺寸（`rows`, `cols`, `cell_m`）；
     - 读取归一化高程数组（`Uint16Array`）、地貌材质数组（`Uint8Array`）与坡度数组；
  3. 构建 `terrainBuffer` 全局对象，随后调用 `renderTrueTerrain(tb)` 渲染至底图 Canvas。
- **副作用**：初始化全图三维地貌几何缓存。

### `renderTrueTerrain`
- **文件与行号**：`web/index.html:943-1037`
- **真实函数签名**：`function renderTrueTerrain(tb)`
- **输入参数**：`tb`: 地形二进制缓存对象。
- **返回值**：无。
- **底层渲染逻辑**：
  - 采用 **SD2/CMO 严肃军用纸质印刷色调**：
    - 基础纸底色采用牛皮纸灰 `#ded8be`；
    - 水体采用低饱和墨青色 `#617985`；
    - 森林采用淡橄榄绿 `#859b82`；
    - 等高线基于高程以 $10\text{m}$ 间距提取并绘制微观细线；
  - 绘制完毕后将离屏 Canvas 转换为 SVG 图片嵌入 `#terrainRasterLayer`。
- **副作用**：覆写地图背景图层。

### `gridToSvg` 与 `svgToGrid`
- **文件与行号**：`web/index.html:1124-1138`
- **真实函数签名**：
  - `function gridToSvg(r, c) -> { x: number, y: number }`
  - `function svgToGrid(svgX, svgY) -> { r: number, c: number }`
- **输入参数**：网格整数坐标 `(r, c)` 或 SVG 视口笛卡尔像素坐标 `(svgX, svgY)`。
- **返回值**：对应坐标系的投影换算结果。
- **底层业务逻辑**：按 $2^3=8\text{m}$ 战术格宽执行矩阵正向/逆向仿射变换，用于处理鼠标点击拾取与单位位置投影。
- **副作用**：无。

---

## 5.2 战术标号与单位渲染管线

### `symbolMarkup`
- **文件与行号**：`web/index.html:755-756`
- **真实函数签名**：`function symbolMarkup(u, light=false) -> string`
- **输入参数**：`u`: 单位态势数据对象；`light`: 是否采用高亮线框。
- **返回值**：`string`，标准 NATO APP-6 战术标号 SVG 片段。
- **底层渲染逻辑**：
  - **阵营几何框**：友军绘制军青色实线矩形框，敌军绘制绯红色菱形框；
  - **兵种核心符号**：步兵绘制交叉对角线（X），装甲绘制椭圆履带环，炮兵绘制中心实心圆，侦察兵绘制单对角斜线，司令部绘制旗杆与“HQ”字样；
  - **编制星杠标号**：在顶部居中渲染罗马数字或圆点（班 `•`、排 `•••`、连 `I`、营 `II`）。
- **副作用**：输出矢量标号字符串。

### `renderUnitsLayer`
- **文件与行号**：`web/index.html:782-806`
- **真实函数签名**：`function renderUnitsLayer()`
- **输入参数**：无。
- **返回值**：无。
- **底层渲染逻辑**：
  - 清空 `#unitsLayer` DOM 节点；
  - 遍历从后端同步的全部存活单位；
  - 依据 `u.pos` 计算屏幕位置，挂载 SVG 容器；
  - 绑定鼠标点击事件（触发检视器 `selectUnit`）与悬停轮廓描边。
- **副作用**：更新主图台战术单位图层。

### `openFloatingUnitCard`
- **文件与行号**：`web/index.html:1313-1422`
- **真实函数签名**：`function openFloatingUnitCard(unitId, offsetX = 320, offsetY = 135)`
- **输入参数**：`unitId`: 单位唯一代号；`offsetX`, `offsetY`: 窗口浮动定位偏移量。
- **返回值**：无。
- **底层渲染逻辑**：
  - 弹出浮动检视器（`Floating Unit Card`）；
  - 渲染单兵三账本明细：
    - **人员健康与组织度**：绿色/黄色进度条显示当前战力百分比；
    - **列兵与士官名单**：逐个列出单兵军衔、伤势（轻伤/重伤/阵亡）；
    - **单兵背包矩阵**：8x6 战术背包网格，图形化展示步枪、机枪弹夹、急救包、手雷。
- **副作用**：激活浮动检视窗。

---

## 5.3 战术作业与指令下发管线

### `triggerPreviewPlan`
- **文件与行号**：`web/index.html:2350-2420`
- **真实函数签名**：`async function triggerPreviewPlan()`
- **输入参数**：无（读取全局暂存的目标拾取点 `taskGoal`）。
- **返回值**：`Promise<void>`。
- **底层业务逻辑**：
  1. 提取当前选中单位物理坐标与目标点；
  2. 向后端发起 `POST /api/preview_plan`；
  3. 接收返回的 `streamline` 折线，在 `#tacticalPreviewLayer` 实时绘制战术虚线箭头；
  4. 在面板展示里程数（m）、预计耗时（ETA）与疲劳点数。
- **副作用**：渲染路径预览航迹。

### `submitTaskSlip`
- **文件与行号**：`web/index.html:2440-2554`
- **真实函数签名**：`async function submitTaskSlip(asDraft=false)`
- **输入参数**：`asDraft`: `bool`，是否仅保存为参谋手写夹板草稿。
- **返回值**：`Promise<void>`。
- **底层业务逻辑**：
  1. 收集机动方式（`"normal"` / `"fast"` / `"cautious"`）、交火规则（`"stop_to_fire"` / `"fire_on_the_move"`）；
  2. 组装 `POST /api/unit_op` 报文，`op: "direct_action"`；
  3. 发送网络请求，若成功则将该动作挂载至前端任务队列镜像，并在左下角电传战报视窗打印出令记录。
- **副作用**：向后端下发单位机动指令。

### `appendBattleLog`
- **文件与行号**：`web/index.html:3227-3269`
- **真实函数签名**：`function appendBattleLog(timeStr, text, type = 'normal')`
- **输入参数**：
  - `timeStr`: 时间戳文本（如 `[01:14:16]`）；
  - `text`: 战况描述；
  - `type`: 日志样式类别（`"combat"` 交火红字、`"eng"` 工程青字、`"sys"` 系统绿字）。
- **返回值**：无。
- **底层渲染逻辑**：
  - 向左下角实时电传战报视窗（`.teletype-stream`）追加 `<div class="teletype-line">`；
  - 自动将滚动条锁定至底部最新一条；
  - 当条数超过 200 条时自动剔除最早的历史节点，防止长时间推演造成浏览器 DOM 泄漏。
- **副作用**：更新电传战报视窗。

### `fetchBackendState`
- **文件与行号**：`web/index.html:3270-3350`
- **真实函数签名**：`async function fetchBackendState()`
- **输入参数**：无。
- **返回值**：`Promise<void>`。
- **底层业务逻辑**：
  1. 拼接当前席位 `commander_id`、阵营与安全口令；
  2. 调用 `fetch('/api/state')`；
  3. 刷新前端单位阵列、更新时钟显示、比对后端 `state_revision`；
  4. 调用 `renderUnitsLayer()` 刷新兵牌；
  5. 若返回包含新电传日志，循环调用 `appendBattleLog` 追加战报。
- **副作用**：驱动前端态势全量刷新。

---

## 5.4 实体参谋夹板与时间机粒子渲染 (UIO & TimeMachine)

### `renderUioBoard`
- **文件与行号**：`web/index.html:1927-2070`
- **真实函数签名**：`function renderUioBoard()`
- **输入参数**：无。
- **返回值**：无。
- **底层渲染逻辑**：
  - 渲染右上角**实木纹理弹簧金属夹手写板（UIO Clipboard）**；
  - 划分三大便签槽区：
    - `Draft Zone`（草拟中的战术便签）；
    - `Active Queue`（正在排队执行的任务条）；
    - `Archived Zone`（已执行完成的战术封口令）；
  - 支持便签条拖拽重排序（调用 `reorder_task_queue`）。
- **副作用**：更新参谋作业夹 DOM。

### 前端 60fps 时间机粒子线性插值渲染
- **文件与行号**：`web/index.html:3580-3635`
- **底层业务逻辑**：
  - 当前端接收到后端 `post_commit_turn` 响应中的 `time_machine_frame` 时：
  - 解析每一个单位的 `(pos_start, pos_end)` 轨迹节点；
  - 利用 `requestAnimationFrame` 在回合推进的动画时长内，对当前渲染坐标进行逐帧线性插值：
    $$\mathbf{p}(t) = \mathbf{p}_{\text{start}} + (\mathbf{p}_{\text{end}} - \mathbf{p}_{\text{start}}) \times \frac{t}{T_{\text{anim}}}$$
  - 对标记 `is_prerender_skirt: true` 的突现敌军，前置创建隐形节点，在能见度判明瞬间执行渐显动画（`opacity: 0 -> 1`），杜绝画面突变跳帧。
- **副作用**：平滑兵牌移动与炮火弹道轨迹。
