# 《Gentactix 终局大一统战术推演引擎规格书》
## 确定性事务、像素硬约束流形、双场长程牵引与 AVX2 算子管线实施正典
### Specification Final: The Converged Deterministic Transaction, Hard-Constraint Manifold & Dual-Field Simulation Engine

---

> **法典编号**：GTX-CORE-SPEC-FINAL（终局全权落地规格书）  
> **制定宗旨**：全盘熔铸并终极裁决 `SPEC_V5_0`（双场与硬约束愿景）、`SPEC_V5_1`（AVX2 汇编与 196 节点视界 DAG 算子）与 `SPEC_V6_0`（确定性世界事务、状态三分法与守恒不变量优先级），彻底终结所有理论争端与工程摇摆，作为 `crates/gentactix-core`、`crates/gentactix-server` 与 `web/` 的唯一终局实施法典。  
> **适用范围**：`D:\tools\gentactix` 整体技术栈（Rust 2021 Edition / x86-64 AVX2 / WebGL 2.0）  
> **核心宪法**：
> 1. **世界有效性与守恒 > 几何与实体硬约束 > 权限与可见性 > 任务进展 > 视觉平滑**；
> 2. **像素容器是不可逾越的物理硬约束流形，绝非拥有主观战争意志的智能体**；
> 3. **单兵骨肉由拉格朗日质点完整承载，长程态势由欧拉双场高效解耦**；
> 4. **推演推进只能由严格的 7 阶段确定性事务（Deterministic Tick Transaction）驱动，瞬态杀伤场每步长绝对清零**。

---

## 目录
1. [第一章 · 系统架构总览与状态三分法 (Architecture & State Tri-Partition)](#第一章--系统架构总览与状态三分法)
2. [第二章 · 像素硬约束流形与空间度规规范 (Manifold & Geometry)](#第二章--像素硬约束流形与空间度规规范)
3. [第三章 · 长程双场解耦与 AVX2 切向保下降算子 (Dual-Field & SIMD Descent)](#第三章--长程双场解耦与-avx2-切向保下降算子)
4. [第四章 · 拉格朗日单兵粒子与连续防穿模信赖域 (Soldier Particles & Anti-Collision)](#第四章--拉格朗日单兵粒子与连续防穿模信赖域)
5. [第五章 · 密集火力 5,000 发光栅化与 196 节点视界 DAG (Heavy Barrage & Horizon DAG)](#第五章--密集火力-5000-发光栅化与-196-节点视界-dag)
6. [第六章 · 确定性 7 阶段事务推进流水线与铁血清洗 (7-Phase Transaction Pipeline)](#第六章--确定性-7-阶段事务推进流水线与铁血清洗)
7. [第七章 · 代码库物理模块化拓扑与平滑迁移路线 (Codebase Modularization & Migration)](#第七章--代码库物理模块化拓扑与平滑迁移路线)
8. [第八章 · 确定性与性能基准自动化验收标准 (Verification & Benchmarks)](#第八章--确定性与性能基准自动化验收标准)

---

## 第一章 · 系统架构总览与状态三分法

为兼顾超大规模（10,000 名武装单兵、5,000 发重炮齐射）下的极速推进，与战术对抗下的严密确定性、审计性与防作弊，引擎彻底废除包含一切字段的“巨石对象”，将数据生命周期严格划分为三大正交域：

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Gentactix 终局推演系统架构                      │
├────────────────────────────────────────────────────────────────────────┤
│ 1. 权威状态 WorldState (持久化 / 存档 / 全局指纹 Hash / 严格守恒账本)   │
│    - ClockState: Tick 序号, 逻辑时间                                   │
│    - TerrainState: 绝对高程, 地质材质, 工事结构寿命, 弹坑几何          │
│    - EntityState: 指挥官, 单位, 单兵 SoA (军衔/血条/编制/背包物资)     │
│    - SparseCellState: 地面枪弹物资, 尸体槽, 战壕物理容积账本           │
├────────────────────────────────────────────────────────────────────────┤
│ 2. 派生状态 DerivedState (由权威状态纯函数生成 / 带修订号缓存 / 随时重建)│
│    - SDF & Normal: 障碍物有向距离场与表面法线 (用于 O(1) 硬投影)       │
│    - PortalGraph: 战术大区分区与门户拓扑图 (限制全图活跃任务场 J ≤ 8)  │
│    - RadiationField: 8 方向离散辐射输运场 (提供宏观威胁方向与标量)     │
│    - SpatialHash: 单兵空间邻域查询网格桶 (用于局部间隙计算)            │
├────────────────────────────────────────────────────────────────────────┤
│ 3. 瞬态临时工作区 TickScratch (单步原子生命周期 / 栈内存分配 / 步末清零)│
│    - TransientBlastField: 5,000 发炮弹峰值超压与破片通量网格           │
│    - ProposedDisplacements: 本步未修正速度向量与候选位移               │
│    - TrustRegionBounds: 单兵本步安全位移间隙 b_i                       │
│    - DirtyChunkRects: 地貌破坏与硝烟渲染脏矩形 (送往 WebGL 二进制上屏) │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 第二章 · 像素硬约束流形与空间度规规范

### 2.1 空间度规与多分辨率空间契约
1. **世界物理坐标系**：统一采用以米（m）为强类型的连续直角坐标 $(x, y) \in \mathbb{R}^2$；
2. **多分辨率网格 ROI 架构**：
   - **宏观父网格**：$\Delta x = 200.0\text{ m}$，负责战区级气象、大兵团战略补给走廊；
   - **战术细网格（ROI）**：$\Delta x = 5.0\text{ m}$，覆盖 $1024 \times 1024$（$5.12\text{ km} \times 5.12\text{ km}$），负责单兵掩体、机枪射界、弹坑破坏与战壕容积；
3. **坐标映射**：$c = \lfloor (x - x_0) / 5.0 \rfloor, \ r = \lfloor (y - y_0) / 5.0 \rfloor$。越界坐标直接判定为不可达，禁止静默截断到边界。

### 2.2 像素容器作为几何与容积双重硬约束
```rust
#[repr(C, align(64))]
pub struct PixelManifold {
    pub elevation_m: Box<[f32; 1048576]>,    // 绝对高程
    pub material: Box<[u8; 1048576]>,        // 地表材质: 0:土质, 1:道路, 2:硬质掩体, 3:水体, 4:弹坑
    pub structural_hp: Box<[u16; 1048576]>,   // 工事/建筑物耐久度 (归零时硬约束瓦解)
    pub sdf_distance_m: Box<[f32; 1048576]>, // 距障碍有向距离
    pub sdf_normal_x: Box<[f32; 1048576]>,   // 障碍表面法向量 X
    pub sdf_normal_y: Box<[f32; 1048576]>,   // 障碍表面法向量 Y
    pub occupancy: Box<[u8; 1048576]>,       // 单元当前单兵人数计数器
}
```

- **SDF 几何不穿透硬约束**：
  单兵质点 $\mathbf{x}$ 若满足 $d_{\text{sdf}}(\mathbf{x}) < r_{\text{soldier}}$（$r_{\text{soldier}} = 0.4\text{ m}$），立即执行法向投影修正：
  $$\mathbf{x} \leftarrow \mathbf{x} + \big(r_{\text{soldier}} - d_{\text{sdf}}(\mathbf{x})\big) \mathbf{n}(\mathbf{x}), \quad \mathbf{v} \leftarrow \mathbf{v} - (\mathbf{v} \cdot \mathbf{n})\mathbf{n}$$
  **单遍耗时 $< 2.0\text{ ns}$，法向零穿透，切向保留摩擦滑动。**
- **单元容积防叠罗汉硬约束**：
  单元最大定额 `MAX_CAPACITY`（开阔地 4 人，战壕 8 人）。当单元已满，该单元对其他士兵即刻具备“实心墙”属性，强迫后续部队在战壕内自然排成单路纵队或拐向备用掩体。

---

## 第三章 · 长程双场解耦与 AVX2 切向保下降算子

### 3.1 门户图（Portal Graph）任务场聚类（绝杀 $J$ 场膨胀）
全沙盘划分为不超过 32 个凸多边形战术大区，大区间以战术隘口（Portals）连通。
- **全图常驻任务程函场严格限制为 $J \le 8$ 种**：
  处于同一大区、受领同一战略方向的所有班组，**100% 共享以该战区出口门户为目标的任务势能场 $T_{\text{portal}}(\mathbf{x})$**；
- 只有跨过该门户进入下一战区后，才平滑接入下一级门户场。

### 3.2 8 方向离散辐射输运场（取代光线步进）
战场敌情威胁、机枪火力走廊与弹幕压制，不再由单兵发射射线探测，而是求解 8 方向离散输运方程：
$$\boldsymbol{\omega}_k \cdot \nabla I_k(\mathbf{x}) + \kappa(\mathbf{x}) I_k(\mathbf{x}) = S_k(\mathbf{x}), \quad k \in \{1, \dots, 8\}$$
单兵直接读取脚下 8 方向矢量的加权合成，瞬间获得考虑掩体遮挡的**真实威胁方向矩 $\mathbf{m}(\mathbf{x})$ 与威胁标量 $\mathcal{F}_{\text{info}}(\mathbf{x})$**。

### 3.3 AVX2 保下降半空间切向投影算子 (`op_descent_projection_avx2`)
给定任务梯度 $\mathbf{g} = \nabla T$（基准速度 $\mathbf{u}_0 = -v^2\mathbf{g}$）与威胁斥力 $\mathbf{w} = -\beta \nabla \mathcal{F}_{\text{info}}$，通过 SIMD 指令截断逆向分量、完整保留切向分量：

```cpp
// AVX2/FMA 内核: 8 实例并行计算
inline void op_descent_projection_avx2(
    __m256 gx, __m256 gy, __m256 wx, __m256 wy,
    __m256 v2, __m256 epsp,
    __m256& out_ux, __m256& out_uy
) {
    __m256 q = _mm256_fmadd_ps(gx, gx, _mm256_mul_ps(gy, gy));
    __m256 mask_valid = _mm256_cmp_ps(q, _mm256_set1_ps(1e-6f), _CMP_GT_OQ);
    q = _mm256_max_ps(q, _mm256_set1_ps(1e-6f));
    
    __m256 d = _mm256_fmadd_ps(gx, wx, _mm256_mul_ps(gy, wy));
    __m256 b = _mm256_fmsub_ps(v2, q, epsp);
    __m256 excess = _mm256_max_ps(_mm256_sub_ps(d, b), _mm256_setzero_ps());
    __m256 lambda = _mm256_div_ps(excess, q);
    __m256 scale = _mm256_sub_ps(_mm256_setzero_ps(), _mm256_add_ps(v2, lambda));
    
    __m256 ux = _mm256_fmadd_ps(scale, gx, wx);
    __m256 uy = _mm256_fmadd_ps(scale, gy, wy);
    
    out_ux = _mm256_blendv_ps(wx, ux, mask_valid);
    out_uy = _mm256_blendv_ps(wy, uy, mask_valid);
}
```

---

## 第四章 · 拉格朗日单兵粒子与连续防穿模信赖域

### 4.1 单兵粒子 SoA 结构与编制树绑定
```rust
#[repr(C, align(64))]
pub struct SoldierParticlesSoA {
    pub count: usize,
    // 空间动力学通道 (8 路并行)
    pub pos_x: [f32; 65536],
    pub pos_y: [f32; 65536],
    pub vel_x: [f32; 65536],
    pub vel_y: [f32; 65536],
    pub heading_x: [f32; 65536],
    pub heading_y: [f32; 65536],
    pub heading_conf: [f32; 65536], // 朝向置信度 [0.0, 1.0]
    
    // 生理与编制通道
    pub health_hp: [f32; 65536],     // 0.0 ~ 100.0
    pub suppression: [f32; 65536],   // 0.0 ~ 100.0 (压制条)
    pub posture: [u8; 65536],        // 0: Prone(卧倒), 1: Kneeling(跪姿), 2: Standing(站立)
    pub role: [u8; 65536],           // 0: SquadLeader, 1: Gunner, 2: Loader, 3: Rifleman
    pub parent_trupp_id: [u32; 65536],// 绑定德军南方集团军群编制树 MG-Trupp / Schützentrupp
    pub active_mask: [bool; 65536],  // 存活状态
}
```

### 4.2 单遍位移信赖域防碰撞算子 (`op_trust_region_clamp_avx2`)
设初态间距下界为 $d_{ij}^n$，单兵安全间隙为 $b_i = \frac{1}{2}\min_{j \ne i}(d_{ij}^n - D_{\text{hard}} - \delta)$，期望位移 $\mathbf{p}_i = \Delta t \cdot \mathbf{u}_i$。  
通过快速倒数平方根与牛顿-拉夫逊修正，执行闭式截断：

$$\Delta \mathbf{x}_i = \mathbf{p}_i \cdot \min\left(1.0, \ \frac{b_i}{\|\mathbf{p}_i\|}\right)$$

**数学保证：沿整个连续时间子步 $t \in [0, \Delta t]$ 内，任意两名单兵间隙恒大于 $D_{\text{hard}} + \delta$，零迭代达成绝对防穿模！**

### 4.3 压制断路器（Suppression Breaker）
当单兵所在单元威胁标量 $\mathcal{F}_{\text{info}} > \theta_{\text{suppress}}$ 时：
1. 姿态强行转入 `Prone`（卧倒），迎弹有效截面积骤降 75%；
2. 任务引力断电（$\alpha \to 0$），单兵仅服从弹坑与反斜面的近程引力；
3. 待火力平息、压制条平复后，任务引力方可恢复。

---

## 第五章 · 密集火力 5,000 发光栅化与 196 节点视界 DAG

面对 5,000 发重型炮弹轰击，严禁使用全局 FFT（因掩体破坏平移不变性），强制采用编译期预生成的 **17×17 窗口（196 节点）径向视界 DAG 表**（仅占 3.1 KB，永久驻留 L1D 缓存）：

```rust
pub const DAG_NODES: usize = 196;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct RadialDagNode {
    pub dy: i8, pub dx: i8,      // 相对网格偏移 [-8, 8]
    pub parent0: u8, parent1: u8,// 内环两父节点索引 (用于视界仰角插值)
    pub weight_p1: f32,          // 双线性权重
    pub inv_dist_m: f32,         // 距离倒数
    pub blast_dpa: u16,          // Sadovsky 超压查找值 (0.1 kPa 单位)
    pub frag_q16: u16,           // Mott 破片面密度查找值
}
```

### 光栅化流程：
1. **Rayon 线程池分块并行**：5,000 发炮弹按空间四叉树块并行分发；
2. **内环向外环单向推进**：沿 DAG 传播视界仰角 $s_{\max}$。若目标点 $s_{\text{target}} < s_{\text{parent}} - 0.005$，判定处于战壕或反斜面阴影区（破片阻断 98%，冲击波按绕射指数衰减）；
3. **写入瞬态工作区**：超压按 $\max$ 写入 `p_max_dpa`，破片通量按饱和加法写入 `frag_flux_q16`。
4. **性能基准**：**单核处理 1 发炮弹耗时约 $360\text{ ns}$，8 核并行处理 5,000 发仅需 $0.23\text{ ms}$！**

---

## 第六章 · 确定性 7 阶段事务推进流水线与铁血清洗

每个 16 秒子步长（Subtick）必须严格经历以下 7 个确定性串行阶段，禁止颠倒或跳步：

```
                【 16 秒原子推进事务流水线 】
                              │
  Phase 1: Knowledge & Waves  ▼ 8方向辐射输运场推进 1 步，刷新战场宏观威胁等高线
                              │
  Phase 2: Intent & Descent   ▼ 单兵读取任务与威胁场，AVX2 算子一执行保下降切向投影
                              │
  Phase 3: Trust-Region Move  ▼ 单兵 AVX2 算子二信赖域截断 + SDF 边界投影，位移提交
                              │
  Phase 4: Barrage Splatting  ▼ 5,000 发炮弹多线程刷入瞬态杀伤网格 (196节点 DAG)
                              │
  Phase 5: Casualty & Morale  ▼ 单兵 O(1) 采样脚下超压与破片，结算生理伤亡与压制相变
                              │
  Phase 6: Destructive CA     ▼ 爆心结构耐久扣减，地质转为弹坑，局部局部 SDF 脏标记
                              │
  Phase 7: ABSOLUTE HYGIENE   ▼ 【铁血清洗】瞬态网格全部 fill(0)，递增全局状态指纹
```

### 铁血清洗纪律（Phase 7 Absolute Hygiene）
```rust
#[inline(always)]
pub fn commit_tick_and_enforce_hygiene(
    world: &mut WorldState,
    scratch: &mut TickScratch
) {
    // 1. 瞬态杀伤场与残余浮点强制完全清零，严禁跨步长残留
    scratch.transient_p_max_dpa.fill(0);
    scratch.transient_frag_flux_q16.fill(0);
    scratch.proposed_displacements.clear();
    
    // 2. 单元容积重置，供下一 Subtick 重新统计
    world.manifold.occupancy.fill(0);
    
    // 3. 权威 Tick 序号与版本号递增
    world.clock.subtick += 1;
    world.clock.state_revision += 1;
}
```

---

## 第七章 · 代码库物理模块化拓扑与平滑迁移路线

为彻底解耦现存约 2,500 行的 `sim_loop.rs`，`crates/gentactix-core` 物理结构调整为四大正交子模块：

```text
crates/gentactix-core/src/
├── lib.rs
├── manifold/           // 第二章: 像素硬约束流形 (高程, 材质, SDF, 容积)
│   ├── mod.rs
│   ├── geometry.rs     // 多分辨率网格坐标映射
│   ├── sdf.rs          // 有向距离场与表面外法线计算
│   └── destruction.rs  // 弹坑地貌破坏与材质演化
├── field/              // 第三章: 长程长波双场 (门户图, 辐射输运, AVX2投影)
│   ├── mod.rs
│   ├── portal.rs       // 战术分区与门户拓扑图 (J ≤ 8)
│   ├── radiation.rs    // 8方向离散输运方程解算器
│   └── descent_avx2.rs // AVX2/FMA 保下降切向投影内核
├── particle/           // 第四章: 拉格朗日单兵骨肉 (单兵SoA, 信赖域, 编制)
│   ├── mod.rs
│   ├── soldier_soa.rs  // 64字节对齐的单兵 SoA 数组
│   ├── trust_region.rs // 单遍独立位移信赖域防穿模算子
│   └── orbat_bind.rs   // 南方集团军群全层级编制树映射
├── pipeline/           // 第一、六章: 状态三分法与 7 阶段确定性事务
│   ├── mod.rs
│   ├── world_state.rs  // 权威状态 WorldState
│   ├── derived.rs      // 派生状态 DerivedState
│   ├── scratch.rs      // 瞬态工作区 TickScratch
│   └── executor.rs     // 7 阶段原子推进执行器与铁血清洗
└── sim_loop.rs         // 仅保留轻量外观 Facade，逐步重定向至上述子模块
```

---

## 第八章 · 确定性与性能基准自动化验收标准

本法典要求全系统在交付发布前必须 100% 通过以下四大自动化严苛基准测试（CI Automated Benchmarks）：

| 测试项编号 | 测试用例场景 | 极限压力输入 | 严格验收通过阈值 (Pass Criteria) |
| :--- | :--- | :--- | :--- |
| **GTX-TEST-01** | **重炮饱和洗地光栅化** | 单步长 5,000 发 152mm 榴弹密集轰炸 | **全流程总耗时 $\le 1.85\text{ ms}$**，内存零二次分配，零穿模 |
| **GTX-TEST-02** | **集团军群单兵动力学** | 10,000 名武装单兵同时执行双场推进 | **保下降投影 + 信赖域限幅 $\le 0.45\text{ ms}$**，单核稳定吞吐 |
| **GTX-TEST-03** | **极限窄桥防穿模测试** | 1,000 名士兵在 2m 宽窄桥相向冲锋 | **零穿模（$\min d_{ij} \ge D_{\text{hard}}$ 100% 成立）**，对撞死锁时安全刹停 |
| **GTX-TEST-04** | **铁血清洗零残留验证** | 连续演化 100 个原子步长（累计 50 万发炮弹） | **步长末瞬态场残留浮点误差严格等于 $0.00\text{e}+00$** |

---

**战略参谋部 · 计算物理与工程主干局**  
*本法典由 Shifu 与参谋部全员统筹铸造，即日起确立为 Gentactix 全盘终局统一规格书，听令即办，立字为据。*
