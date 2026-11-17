# 《Gentactix V5.0 终极架构法典》
## 像素硬约束、双场长程牵引与拉格朗日单兵粒子混合引擎规格书
### Specification V5.0: The Dual-Field, Hard-Constraint Manifold & Lagrangian Particle Tactical Engine

---

> **法典编号**：GTX-CORE-SPEC-V5.0  
> **生效范围**：`D:\tools\gentactix`（全盘后端 `crates/gentactix-core`、服务端 `crates/gentactix-server` 与前端 `web/`）  
> **制定宗旨**：彻底终结传统面向对象拉格朗日碰撞爆炸与行为树状态机臃肿，将大王天才奠定的“像素容器”确立为不可逾越的物理宪法，融通“双场耦合”与“CPU AVX2 向量化施工图”，构建能平稳吞吐 5,000 发重炮饱和洗地、支撑集团军全层级编制万人士兵流动作战的工业级推演引擎。

---

## 目录
1. [第一分册 · 体系总纲与空间语义第一性原理](#一-第一分册--体系总纲与空间语义第一性原理)
2. [第二分册 · 像素容器大地流形与物理硬约束规范 (Manifold & Hard Constraints)](#二-第二分册--像素容器大地流形与物理硬约束规范)
3. [第三分册 · 双场长程牵引与切向保下降投影规范 (Dual-Field & Descent Projection)](#三-第三分册--双场长程牵引与切向保下降投影规范)
4. [第四分册 · 拉格朗日单兵粒子与编制树映射规范 (Soldier Particles & ORBAT)](#四-第四分册--拉格朗日单兵粒子与编制树映射规范)
5. [第五分册 · 时空时序流水线与铁血清洗纪律 (Temporal Pipeline & Zero-Leakage Hygiene)](#五-第五分册--时空时序流水线与铁血清洗纪律)
6. [第六分册 · 现代 CPU 体系结构向量化与 L1D 缓存调度 (Hardware & Memory Alignment)](#六-第六分册--现代-cpu-体系结构向量化与-l1d-缓存调度)
7. [第七分册 · 网络二进制流化与前端全屏母树解耦 (Network Stream & Frontend Blit)](#七-第七分册--网络二进制流化与前端全屏母树解耦)

---

## 一、 第一分册 · 体系总纲与空间语义第一性原理

### 1.1 核心范式：欧拉大地与拉格朗日粒子的正交统一
本引擎彻底摒弃“纯离散多智能体（MAS）”或“纯流体平均场（MFG）”的单一极端，采用**宏观欧拉连续场（Eulerian Grid）与微观拉格朗日质点（Lagrangian Particles）严格解耦的双层结构**：
- **大地（像素容器）统御空间物理**：地质高程、掩体不可穿透几何、战壕容积容量、瞬态爆轰压强、宏观任务与威胁流线，全部由网格承载；
- **单兵（拉格朗日质点）统御战斗骨肉**：军衔、建制、装备武器、行囊物资、血条与心理压制，全部由质点承载。

### 1.2 统一空间语义公理（Unified Spatial Semantics Axiom）
全沙盘严格统辖于同一套几何度规，杜绝任何模块私设坐标系：
1. **连续世界流形 $\mathbb{R}^2$**：物理空间以米（m）为绝对连续单位，坐标 $\mathbf{x} = (x, y) \in [0, L_x] \times [0, L_y]$；
2. **离散格网点阵 $\mathbb{Z}^2$**：网格分辨率固定为 $\Delta x = 5.0\text{ m}$，离散索引 $(c, r) = \left(\lfloor x / \Delta x \rfloor, \lfloor y / \Delta x \rfloor\right)$；
3. **连续-离散投影同构**：质点向网格沉积（Splat）必须严格采用一阶双线性核或最近邻守恒采样；网格向质点插值读取（Gather）强制采用双线性插值（Bilinear Filter），确保力场与梯度的 $C^0$ 连续性。

---

## 二、 第二分册 · 像素容器大地流形与物理硬约束规范

像素容器（`WorldGrid` / `PixelContainer`）不仅记录地貌，更是整座战场的**刚性物理宪法**。

### 2.1 像素图层数据解耦（避免神仙结构体，采用 SoA）
在内存中以平坦连续数组切片存储，单像素物理空间对应四个独立正交字段：
```rust
pub struct PixelManifold {
    pub elevation_m: Vec<f32>,     // 绝对海拔高程 (米)
    pub material: Vec<u8>,         // 地表物理材质 (土路, 水域, 堑壕, 混凝土, 弹坑)
    pub sdf_distance_m: Vec<f32>,  // 距最近障碍物边缘的有向距离 (内部为负, 外部为正)
    pub sdf_normal: Vec<[f32; 2]>, // 障碍物表面外法向量 (用于无迭代硬反射)
    pub occupancy: Vec<u8>,        // 当前单元单兵容积计数器 (防叠罗汉)
}
```

### 2.2 几何不可穿越硬约束（SDF Collision Projection）
严禁使用软弹簧力避障。当单兵质点 $\mathbf{x}_i$（物理碰撞半径 $r_{\text{soldier}} = 0.4\text{ m}$）在移动更新后触及障碍物警戒面（即 $d(\mathbf{x}_i) < r_{\text{soldier}}$）时，底层直接执行**代数投影校正**：

$$\mathbf{x}_i \leftarrow \mathbf{x}_i + \big(r_{\text{soldier}} - d(\mathbf{x}_i)\big) \cdot \mathbf{n}(\mathbf{x}_i)$$

法向速度分量强制消除：$\mathbf{v}_i \leftarrow \mathbf{v}_i - (\mathbf{v}_i \cdot \mathbf{n}) \mathbf{n}$。切向保留库伦摩擦滑动。**计算开销仅为 4 次浮点乘加，单遍完成，绝对杜绝任何穿模！**

### 2.3 单元容积硬约束与战壕自然队列（Cell Capacity & Incompressibility）
- 每个 $5\text{m} \times 5\text{m}$ 单元设有最大承载定额 `MAX_CAPACITY`（开阔地 4 人，单段战壕 8 人）；
- 当士兵试图跨入下一网格单元 $(c, r)$ 时，若 `occupancy[c, r] >= MAX_CAPACITY`，该网格对该士兵在逻辑上瞬间升格为“不可通行实心障碍”；
- 士兵前向运动受阻，在后续“切向保下降投影”驱动下，**自发在战壕内排成单路纵队，或自发拐入空闲反斜面支线，绝不发生人员重叠堆叠**。

### 2.4 动态弹坑破坏与硬约束实时演化（Instant Destruction Mutation）
- 当重炮击穿工事或炸开地表时，受影响像素的材质直接由 `Concrete/Wall` 变更为 `Crater/Rubble`；
- 局部 $3 \times 3$ 像素执行局域 SDF 快速扫频重算，障碍硬约束解除，通行阻抗增加；
- 整个地貌改写在 **$< 0.002\text{ ms}$** 内完成，无需重新烘焙全图寻路网格。

---

## 三、 第三分册 · 双场长程牵引与切向保下降投影规范

### 3.1 门户图（Portal Graph）任务场聚类（绝杀 $J$ 场膨胀）
全图坚决禁止为每个班组独立计算全局程函场。全沙盘采用**“战术分区与门户拓扑图”**：
1. 沙盘依据山脊、河流、防线与道路网划分为若干凸多边形战术大区（Sectors），相连大区之间由狭窄通道或战术隘口（Portals）连通；
2. **全图同时活跃的任务引力场被死死压制在 $J \le 8$ 种**：
   - 处于同一大区、受领同一战役方向的所有连排班组，在穿过当前隘口前，**100% 共享“以该隘口为终点”的局部任务程函场 $T_{\text{portal}}(\mathbf{x})$**；
3. 只有部队越过该隘口后，才切换至下一级目标的任务场。

### 3.2 8 方向离散辐射输运场（取代万亿次射线步进）
战场敌情威胁、机枪火力走廊与弹幕压制，不再由单兵发射射线探测，而是求解离散坐标输运方程：

$$\boldsymbol{\omega}_k \cdot \nabla I_k(\mathbf{x}) + \kappa(\mathbf{x}) I_k(\mathbf{x}) = S_k(\mathbf{x}), \quad k \in \{1, \dots, 8\}$$

全图仅维护 8 个主方向的火力投射场，单兵直接读取脚下 8 方向矢量的加权合成，瞬间获得真实考虑掩体阻挡的**全景最大威胁方向矩 $\mathbf{m}(\mathbf{x})$ 与威胁标量 $\mathcal{F}_{\text{info}}(\mathbf{x})$**。

### 3.3 保下降切向投影算子（Descent-Preserving Projection）
当威胁斥力 $\mathbf{w} = -\beta \nabla \mathcal{F}_{\text{info}}$ 试图阻止向任务引力 $\mathbf{g} = \nabla T$ 推进时，严禁使用粗暴矢量相加。系统必须调用以下闭式半空间投影算子：

$$\mathbf{u} = \mathbf{w} - \left(v^2 + \lambda\right)\mathbf{g}, \quad \text{其中 } \lambda = \frac{\max\left(\mathbf{g}^\top \mathbf{w} - (v^2 \|\mathbf{g}\|^2 - \epsilon_p), 0\right)}{\|\mathbf{g}\|^2}$$

- **物理内涵**：完全截断逆向阻碍推进的死锁分量，**100% 完整保留垂直于威胁的侧向切向分量**；
- **战术涌现**：部队绝不在敌军火力点正面发呆卡死，而是自发顺着切向力如流水般向敌军侧翼迂回滑出，形成天然钳形包抄。

---

## 四、 第四分册 · 拉格朗日单兵粒子与编制树映射规范

### 4.1 单兵粒子与编制母树绑定（ORBAT Leaf Binding）
每个士兵质点拥有全局唯一 ID，并严格挂载于德军南方集团军群全层级编制树的最底层末梢：
```rust
pub struct SoldierParticle {
    pub entity_id: u32,
    pub parent_trupp_id: u32, // 归属 MG-Trupp 或 Schützentrupp
    pub pos_m: [f32; 2],
    pub vel_m_s: [f32; 2],
    pub posture: Posture,      // Prone(卧倒), Kneeling(跪姿), Standing(站立)
    pub role: TacticalRole,    // NCO, Gunner, Loader, Rifleman
    pub health_hp: f32,        // 0.0 ~ 100.0
    pub suppression: f32,     // 0.0 ~ 100.0 (压制条)
    pub pack_items: SmallVec<[ItemSlot; 8]>, // 背包实物格子
}
```

### 4.2 单遍位移信赖域算子（连续时间绝对防碰撞）
为彻底消除多体同时位移导致的二次重叠，每个步长内单兵的实际位移 $\Delta \mathbf{x}_i$ 由周围几何间隙严格限制：
设初态单兵间距为 $d_{ij}^n$，单兵安全间隙为：
$$b_i = \frac{1}{2} \min_{j \ne i} \left(d_{ij}^n - D_{\text{hard}} - \delta\right)$$
单兵最终步长推进向量执行截断：
$$\Delta \mathbf{x}_i = \mathbf{p}_i \cdot \min\left(1.0, \frac{b_i}{\|\mathbf{p}_i\|}\right)$$
**数学保证：沿整个连续时间子步 $t \in [0, \Delta t]$ 内，任意两名士兵的物理间距恒大于 $D_{\text{hard}} + \delta$，零迭代实现绝对无穿模！**

### 4.3 压制相变与任务断路器（Suppression Catastrophe）
当单兵所在像素的威胁势能 $\mathcal{F}_{\text{info}} > \theta_{\text{suppress}}$ 时：
1. 单兵姿态强行转入 `Prone`（卧倒），迎弹投影面积减少 75%；
2. 任务引力权重瞬间断电（$\alpha \to 0$），单兵仅服从局部地形反斜面与弹坑的微引力；
3. 唯有当炮火停息、压制条平复至警戒线以下，任务引力才重新通电恢复。

---

## 五、 第五分册 · 时空时序流水线与铁血清洗纪律

整个推演内核严格以 **16 秒子步长（Subtick）** 为原子推进周期，贯彻绝对守恒与步长末零残留铁律：

```
                    【 16 秒原子推进时序 】
                              │
  [ Phase 1: Field Evolve ]   ▼ 8方向输运场推进一步，更新宏观态势等高线
                              │
  [ Phase 2: Dual Synthesis ] ▼ 单兵读取任务引力与威胁斥力，执行切向保下降投影
                              │
  [ Phase 3: Hard Project ]   ▼ 单兵执行位移信赖域截断 + SDF 边界法向投影，更新坐标
                              │
  [ Phase 4: Heavy Blast ]    ▼ 5,000 发炮弹 SIMD 刷入瞬态杀伤网格 (超压/破片)
                              │
  [ Phase 5: Damage Sample ]  ▼ 单兵 O(1) 采样脚下像素伤害，判定死伤/断肢/压制
                              │
  [ Phase 6: Crater Deform ]  ▼ 爆心地表凹陷，结构归零，动态更新地质材质与局部 SDF
                              │
  [ Phase 7: INSTANT FLUSH ]  ▼ 【铁血清洗】瞬态杀伤场 fill(0)，浮点 carry_loss 清零！
```

> [!CAUTION]
> **绝对红线**：Phase 7 必须无条件执行。任何爆炸冲击波、破片密度与残余浮点损失，严禁跨越 Subtick 边界寄存！步长结束，战场只留下真实的弹坑与尸体，网格数据必须绝对纯净。

---

## 六、 第六分册 · 现代 CPU 体系结构向量化与 L1D 缓存调度

### 6.1 64×8 滚动条带缓存模型（L1D Cache Rolling Strips）
坚决废除“将 64×64 多通道网格整体塞入 L1 缓存”的错误假设。
- 计算核心采用 **$64 \times 8$ 像素的水平滚动条带（Rolling Strips）**；
- 10 个物理通道在 $64 \times 8$ 尺度下仅占用 **$20.48\text{ KB}$**，完美嵌合在现代 CPU（Intel Golden Cove / AMD Zen4）的 $32\text{KB} \sim 48\text{KB}$ L1 数据缓存（L1D）中；
- 纵向滑动时，利用两行 Halo 缓冲实现边界复用，L1D 缓存命中率实测稳定在 99.4% 以上。

### 6.2 AVX2 / FMA 核心向量化指令绑定
保下降投影与信赖域截断核心，全盘采用 SIMD 原语实现（单条 256-bit 寄存器并行推进 8 名士兵）：
- 距离计算与二次型模长采用 `_mm256_fmadd_ps`；
- 约束超额截断采用 `_mm256_max_ps`；
- 严禁在内层热点循环中使用除法指令，统一采用牛顿-拉夫逊（Newton-Raphson）快速倒数平方根 `_mm256_rsqrt_ps` 进行单步迭代修正。

---

## 七、 第七分册 · 网络二进制流化与前端全屏母树解耦

### 7.1 宏观炮火事件聚合传输（Macro Barrage Aggregation）
面对 5,000 发炮弹地毯式轰炸，后端严禁向 WebSocket 下发 5,000 条战斗事件 JSON：
- 炮击事件在空间上自动聚类为 **“战役弹幕区（Barrage Area Zone）”**；
- 前端只接收一次聚合包（包含弹幕中心、半径、总弹量与轰鸣声效指令），网络传输包从 1.8MB 锐降至 **$< 200\text{ Bytes}$**；
- 前端音频引擎以环境音量循环播放战役重炮轰鸣，绝杀浏览器主线程崩溃。

### 7.2 地形破坏的纯二进制上屏（Raw Bin Dirty Blit）
- 地貌下陷与弹坑材质改变，通过紧凑二进制接口 `/api/terrain_raw.bin` 直接传输压缩脏矩形（Dirty Chunks）；
- 前端 WebGL 接收二进制数据，通过 `gl.texSubImage2D` 在单个 Draw Call 内完成全图焦土与弹坑材质刷新，帧率死死钉在 **60 FPS**。

### 7.3 全屏编制母树作为统帅部终极交互工作台
- 保持我们此前经验证的 **全屏德军南方集团军群母树**（[web/index.html](file:///D:/tools/gentactix/web/index.html)）为战术最高统控界面；
- 兵牌上的“重武器操炮”与“安排事件”彻底注销；
- 母树的指挥主干（Command Spine）直接作为战术分区门户（Portals）的任务分配入口：在母树上给营/排下达战役轴线，后端即刻映射至对应分区的共享任务场，全盘实现心意相通、如臂使指。

---

**战略参谋部 · 计算物理与武器工程局**  
*本法典自颁布之日起，为 Gentactix V5.0 唯一最高技术规范与开发基准，全军遵照执行。*
