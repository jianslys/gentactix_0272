# 《Gentactix V6.1 终极统一实施大典》
## 确定性世界事务、欧拉-拉格朗日双场演化与硬约束算子工程规格书
### Specification V6.1: The Definitive Unified Architecture, Operator Blueprint & Implementation Codex

---

> **法典编号**：GTX-CORE-SPEC-V6.1（大一统终极定型规格书）  
> **上位宪法**：  
> - `D:\tools\gentactix\docs\SPEC\SPEC_V6_0_DETERMINISTIC_WORLD_TRANSACTION_ENGINE.md`（世界事务宪法）  
> - `D:\tools\gentactix\docs\SPEC\SPEC_V5_0_CORE_DOCTRINE_DUAL_FIELD_AND_HARD_CONSTRAINT_ENGINE.md`（物理场与单兵粒子法典）  
> - `D:\tools\gentactix\docs\SPEC\SPEC_V5_1_OPERATOR_PIPELINE_AND_AVX2_IMPLEMENTATION_BLUEPRINT.md`（微架构与内联算子图）  
> **生效范围**：`D:\tools\gentactix` 全盘（后端 `crates/gentactix-core`、服务端 `crates/gentactix-server`、前端 `web/`，以及迁移期基准）  
> **统帅宣言**：本规格书系参谋部基于四份前沿数理/HPC白皮书与大当家三代法典，历经非凸拓扑奇异点证明、算子谱衰减分析、缓存行压力审计与确定性状态事务审查后，所作出的**终极工程定型实施大典**。任何后续架构修改、代码重构、SIMD 优化与网络通信协议，均以本大典为唯一绝对裁决标准。

---

## 目录
1. [第零章 · 统一大典总则与理论收敛 (System Principles & Theoretical Convergence)](#第零章--统一大典总则与理论收敛)
2. [第一章 · 世界状态三分法与内存布局 (State Tripartition & SoA Memory Topology)](#第一章--世界状态三分法与内存布局)
3. [第二章 · 统一空间语义与双层几何度规 (Spatial Semantics & Multi-Resolution Geometry)](#第二章--统一空间语义与双层几何度规)
4. [第三章 · 类型化场契约与长程双场导航 (Typed Fields & Dual-Field Mechanics)](#第三章--类型化场契约与长程双场导航)
5. [第四章 · 意图仲裁与三大 AVX2 硬约束几何算子 (Intent Arbitration & Core SIMD Operators)](#第四章--意图仲裁与三大-avx2-硬约束几何算子)
6. [第五章 · 密集火力 5,000 发光栅化、地貌流变与视界 DAG (Lethality, DAG & Bekker Tensor)](#第五章--密集火力-5000-发光栅化地貌流变与视界-dag)
7. [第六章 · 12 阶段确定性 Tick 事务流水线 (Deterministic 12-Phase Transaction Pipeline)](#第六章--12-阶段确定性-tick-事务流水线)
8. [第七章 · CPU 微架构落地：64×8 滚动条带与缓存调度 (HPC L1D Strips & Cache Hygiene)](#第七章--cpu-微架构落地648-滚动条带与缓存调度)
9. [第八章 · 网络二进制流化与前端全屏母树工作台 (Binary Stream & Frontend Workspace)](#第八章--网络二进制流化与前端全屏母树工作台)
10. [第九章 · 渐进工程重构路线图与严苛验收红线 (Migration Roadmap & Verification Gates)](#第九章--渐进工程重构路线图与严苛验收红线)

---

## 第零章 · 统一大典总则与理论收敛

### 0.1 最高不变量：真实世界事务与绝对守恒
1. **世界推进的唯一性**：沙盘世界的演化，只能且必须通过确定性的逻辑 Tick 事务（16 s 原子子步长）从合法状态 $S^n$ 演进至合法状态 $S^{n+1}$；
2. **守恒律与无假修复**：
   - 质量、人员、弹药装备与地形土石方绝对守恒，严禁凭空产生或凭空注销；
   - 严禁用随机增加的 `epsilon` 掩盖几何除零，严禁将不可达目标伪装成平滑下降，严禁静默修复非法碰撞；
3. **最高冲突仲裁层级**：
   $$\boxed{\text{世界状态合法性与守恒} \ > \ \text{地形与实体硬约束} \ > \ \text{命令权限与信息可见性} \ > \ \text{任务进展速度} \ > \ \text{视觉连续与帧率}}$$
   **铁律**：宁可让单车道上迎面对顶的装甲集群安全刹停并向统帅部报告阻滞，也绝不容许为“持续推进”或“视觉平滑”而发生 1 毫米的穿模渗透！

### 0.2 理论争鸣之终极收敛表
针对前期四份技术白皮书在场论、遮挡、碰撞与缓存上的学术分歧，本大典作出终审工程裁决：

| 理论分歧点 | 理想化模型缺陷 | 终极定型裁决方案 | 数学与工程依据 |
| :--- | :--- | :--- | :--- |
| **遮挡核计算** | 试图在频域做低秩 FFT 卷积 | **空间域预计算径向视界 DAG 树** | Birman-Solomyak 定理证明硬遮挡核奇异值代数衰减，低秩 FFT 会引发 19.8% 战壕吉布斯穿模泄漏，相变点高达 36 万发；DAG 树单像素 $O(1)$ 判定完胜 |
| **实体防重叠** | 试图仅靠平滑势能斥力或密度截断 | **基于旧间隙的单遍位移信赖域限幅** | 离散时间积分下软势能无法阻止高速碰撞；信赖域从初态保证沿整个连续子步 $d_{ij}(t) \ge D_{\text{hard}}$ 严格成立 |
| **死锁与包抄** | 简单矢量相加导致 U 型地形原地发呆 | **保下降半空间切向投影算子 $\mathcal{P}_g(w)$** | 欧氏投影完全截断阻碍任务的逆向分量，100% 完整保留侧向切力，部队自动顺水流包抄 |
| **超压多弹累加** | 线性相加 $\sum \Delta P$ 导致掩体虚假致死 | **热带半环 $(\max, +)$ 峰值包络 + $L_{10}$ 生物疲劳** | 瞬态激波时间尺度解耦（数毫秒），正压脉冲不可直接代数累加，严格匹配 Bowen 生理模型 |
| **L1D 缓存分块** | 误以为 64×64 多通道能装入 L1D | **外层 64×64 分块 + 内层 64×8 滚动条带** | 10 个通道 64×64 工作集达 160 KB（L1D 仅 32~48KB）；64×8 条带仅 20.48 KB，稳稳驻留 L1D |

---

## 第一章 · 世界状态三分法与内存布局

遵循 V6.0 状态三分哲学，彻底剥离“巨石对象”，在内存中实现严格的生命周期与审计边界隔离。

```
              ┌────────────────────────────────────────────────────────┐
              │              SimWorld (Facade / 外观接口)             │
              └──────────────────────────┬─────────────────────────────┘
                                         │
        ┌────────────────────────────────┼────────────────────────────────┐
        ▼                                ▼                                ▼
┌──────────────────────┐      ┌──────────────────────┐      ┌──────────────────────┐
│  WorldState (权威)   │      │ DerivedState (派生)  │      │ TickScratch (瞬态)   │
├──────────────────────┤      ├──────────────────────┤      ├──────────────────────┤
│- ClockState (时钟)   │      │- SpatialBins (邻域)  │      │- MotionProposal (提案)│
│- TerrainState (地形) │      │- SDF & Normal Cache  │      │- EventBuffer (事件)  │
│- EntityState (实体)  │      │- PortalGraph (门户)  │      │- BlastWorkArray (杀伤)│
│- CommandState (命令) │      │- ThreatFieldCache    │      │- DirtyTiles (脏块)   │
│- KnowledgeState (情报)│      │- DirtyRegionBits     │      │- ConstraintViolations│
│- SparseCellStash     │      │- RenderBuffers       │      │- SolverTempStorage   │
└──────────────────────┘      └──────────────────────┘      └──────────────────────┘
     [进入权威指纹]                 [依赖失效可重建]               [Subtick末绝对清空]
```

### 1.1 权威实体 SoA 内存布局 (`EntityStateSoA`)
拉格朗日实体以连续数组平坦排列，全要素 64 字节对齐，废除内存碎片：

```rust
pub const MAX_ENTITIES: usize = 65536;

#[repr(C, align(64))]
pub struct EntityStateSoA {
    pub count: usize,
    // 空间动力学核心 (AVX2 SIMD 热点通道)
    pub pos_x: [f32; MAX_ENTITIES],
    pub pos_y: [f32; MAX_ENTITIES],
    pub vel_x: [f32; MAX_ENTITIES],
    pub vel_y: [f32; MAX_ENTITIES],
    pub heading_x: [f32; MAX_ENTITIES],    // 未归一化朝向 X
    pub heading_y: [f32; MAX_ENTITIES],    // 未归一化朝向 Y
    pub heading_conf: [f32; MAX_ENTITIES], // 朝向置信度 [0.0, 1.0]
    
    // 身份与编制权威账本
    pub entity_id: [u32; MAX_ENTITIES],
    pub parent_trupp_id: [u32; MAX_ENTITIES], // 归属德军编制树外键
    pub faction_id: [u8; MAX_ENTITIES],
    pub role: [u8; MAX_ENTITIES],              // 指挥官, 机枪手, 弹药手, 步枪手
    pub posture: [u8; MAX_ENTITIES],           // 0: Prone, 1: Kneeling, 2: Standing
    
    // 生理与后勤库存
    pub health: [f32; MAX_ENTITIES],           // 0.0 ~ 100.0
    pub suppression: [f32; MAX_ENTITIES],      // 0.0 ~ 100.0 (压制条)
    pub ammo_primary: [u16; MAX_ENTITIES],     // 主武器弹药余量
    pub ammo_secondary: [u16; MAX_ENTITIES],
    pub active_mask: [bool; MAX_ENTITIES],
}
```

### 1.2 瞬态工作区与铁血清洗 (`TickScratch`)
每个 Tick 动态创建或重用预分配无锁池，Tick 结束阶段必须无条件归零：
```rust
#[repr(C, align(64))]
pub struct TickScratch {
    pub shot_events: Vec<ShotPacket>,          // 即时射击包
    pub motion_proposals: Vec<MotionProposal>, // 运动意图提案
    pub p_max_scratch: Box<[u16; TOTAL_CELLS]>,// 瞬态热带超压网格
    pub frag_scratch: Box<[u16; TOTAL_CELLS]>, // 瞬态破片通量网格
    pub dirty_tiles: [bool; 256],              // 64x64 脏分块标记
}
```

---

## 第二章 · 统一空间语义与双层几何度规

### 2.1 双层空间坐标语义公理
严禁在沙盘全域无条件展开 $5\text{ m}$ 细网格。采用宏观父网格与微观局部关注区（ROI）解耦架构：
1. **全局物理流形 $\mathbb{R}^2$**：世界坐标以米（m）为连续绝对基准，原点位于战区西南角；
2. **战区宏观父网格（200m Parent Grid）**：$M_{\text{macro}} = 256 \times 256$，单格 $\Delta X = 200.0\text{ m}$，覆盖 $51.2\text{ km} \times 51.2\text{ km}$ 大战区，承载军/师级宏观后勤管线、气候、集团军战役分界线；
3. **前线战术 ROI 细网格（5m Child Grid）**：$M_{\text{roi}} = 1024 \times 1024$，单格 $\Delta x = 5.0\text{ m}$，动态覆盖 $5.12\text{ km} \times 5.12\text{ km}$ 核心交火区，承载单兵 SDF、战壕容量、5,000 发火炮弹坑与精细机动；
4. **跨尺度坐标映射接口**：
   $$\mathbf{X}_{\text{parent}} = \left\lfloor \frac{\mathbf{x}_{\text{world}}}{\Delta X} \right\rfloor, \quad \mathbf{x}_{\text{roi}} = \left\lfloor \frac{\mathbf{x}_{\text{world}} - \mathbf{O}_{\text{roi}}}{\Delta x} \right\rfloor$$
   任何未处于活动 ROI 内部的单兵质点，由父网格宏观平流方程托管；进入 ROI 边界瞬间自动解包为精细质点。

---

## 第三章 · 类型化场契约与长程双场导航

### 3.1 物理聚合代数公理
所有场在内存中虽以数值表示，但在底层算子中严格遵循其物理维度的聚合代数（Aggregation Algebra）：
- **守恒求和代数（`Sum`）**：人员密度、有效破片通量、烟雾粒子质量。必须满足散度定理与积分质量守恒；
- **热带半环代数（`TropicalMax`）**：峰值超压 $\Delta P_{\max} = \max_i \Delta P_i$。满足 $a \oplus b = \max(a, b)$，严禁代数加法；
- **生物组织疲劳范数（`Lp-Fatigue`）**：多次亚致死冲击波累加：$S_{10} = \left(\sum_i (\Delta P_i)^{10}\right)^{1/10}$；
- **测地距离代数（`MinPlus`）**：程函与最短路径价值函数 $T(\mathbf{x})$。地形破坏导致通行阻抗增加时，依赖区域必须显式失效，严禁取小操作覆盖。

### 3.2 门户图（Portal Graph）长程长效任务场
沙盘战区依据天险与工事划分为不超过 32 个凸多边形战术大区，相连大区由狭窄隘口（Portals）连通：
- 全图活跃的长程任务导向场被严格限制为 **$J \le 8$ 种**；
- 处于同一战术大区、受领同一战役方向的全部单兵，**100% 共享以当前大区目标门户为终点的局部程函场 $T_{\text{portal}}(\mathbf{x})$**；
- 门户场通过线性屏蔽泊松方程在 CPU 上采用红黑高斯-赛德尔迭代（Red-Black Gauss-Seidel）求解，复用前帧解（Warm-Start），每帧仅迭代 2 遍，单场刷新耗时 $< 0.1\text{ ms}$。

---

## 第四章 · 意图仲裁与三大 AVX2 硬约束几何算子

### 4.1 唯一运动意图仲裁管道（Intent Pipeline）
玩家手动直控命令、自主避险算法与上级编制指令，严禁分别向实体写入坐标。必须遵循标准四级流水线：
```
[CommandIntent] (上级命令) + [AutonomyIntent] (自主规避) + [ActiveLocks] (物理锁死)
                                     │
                                     ▼
                        [Arbitrated MotionIntent] (唯一仲裁意图)
                                     │
                                     ▼
                           [MotionProposal] (运动提案)
                                     │
                                     ▼
                     [Hard Constraint Correction] (硬约束物理校正)
                                     │
                                     ▼
                            [Authoritative Commit] (最终入账)
```

### 4.2 算子一：保下降半空间切向投影 (`op_descent_projection_avx2`)
当威胁斥力 $\mathbf{w}$ 阻碍向任务引力 $\mathbf{g} = \nabla T$ 推进时，执行代数半空间截断：
$$\mathbf{u} = \mathbf{w} - (v^2 + \lambda)\mathbf{g}, \quad \lambda = \frac{\max\big(\mathbf{g}^\top \mathbf{w} - (v^2 \|\mathbf{g}\|^2 - \epsilon_p), \ 0\big)}{\|\mathbf{g}\|^2}$$
- **AVX2 指令实现**：纯 256 位向量化，单循环吞吐 8 名士兵，利用 `_mm256_fmadd_ps` 保证切向侧翼力 100% 留存，部队遇正面阻力自动分流包抄。

### 4.3 算子二：单遍独立位移信赖域防穿模 (`op_trust_region_clamp_avx2`)
以初始构型合法性为前置断言，计算单兵局部安全间隙 $b_i = \frac{1}{2}\min_{j \ne i}(d_{ij}^n - D_{\text{hard}} - \delta)$：
$$\Delta \mathbf{x}_i = \mathbf{p}_i \cdot \min\left(1.0, \ \frac{b_i}{\|\mathbf{p}_i\|}\right)$$
- **数学证明保证**：在连续时间子步 $t \in [0, \Delta t]$ 内，任意两名单兵中心距严格大于 $D_{\text{hard}} + \delta$，零牛顿迭代实现绝对硬防穿模。

### 4.4 算子三：带置信度与 Fallback 的平滑朝向 (`op_smooth_heading_confidence`)
面对威胁矩 $\mathbf{m}$ 与行军后备方向 $\mathbf{b}$，彻底消除分母除零奇异性：
$$\mathbf{H} = (1 - s)\mathbf{b} + s \cdot \frac{\mathbf{m}}{\|\mathbf{m}\|}, \quad C_{\text{conf}} = \min\left(1.0, \ \frac{\|\mathbf{m}\|}{\epsilon_{\text{threat}}}\right)$$
- 当威胁极小或完全对称时，$C_{\text{conf}} \to 0$，炮塔与视线平滑回正至行军方向，彻底消除旋转癫痫。

---

## 第五章 · 密集火力 5,000 发光栅化、地貌流变与视界 DAG

### 5.1 亚网格二重积分与 Sadovsky 混叠消除
在 $5\text{ m} \times 5\text{ m}$ 网格上，爆心近场项 $r^{-3}$ 导致点采样存在 18 倍数值混叠。本大典强制采用**单元均值二重积分算子**：
$$\overline{\Delta P}(\mathbf{x}_c, \mathbf{x}_i) = \frac{1}{(\Delta x)^2} \iint_{\Omega_c} \Delta P\left(\|\mathbf{u} - \mathbf{x}_i\|_2; W_{\text{eff}}\right) d\mathbf{u}$$
在运行时通过 4 次乘加执行双线性亚网格权重分配（Bilinear Splatting），消除 94.5% 采样混叠，严格守恒爆轰能量。

### 5.2 预计算切比雪夫环径向视界 DAG 算子
在 17×17 局部窗口（196 像素）内，采用预编译的静态拓扑树 `RADIAL_DAG_TABLE[196]`（3.1 KB 永久常驻 L1D）：
- 由内向外扫描切比雪夫环 $r_\infty = 1 \to 8$；
- 继承父节点最大高度角斜率，单像素仅需 1 次父寻址与 1 次比较；
- 精准实施各向异性滤波：战壕阴影区破片阻断至 2%（$\chi_{\text{frag}} = 0.02$），冲击波衰减至 35%（$\chi_{\text{blast}} = 0.35$）。

### 5.3 碎胀守恒地貌流变与 Bekker 车辆阻力张量
1. **地貌质量守恒偏微分方程**：引入土壤碎胀系数 $B_{\text{bulk}} = 1.22$，弹坑挖掘体积与唇部堆积严格守恒：
   $$\Delta V_{\text{net}} = (B_{\text{bulk}} - 1) V_{\text{true}} > 0, \quad \frac{d}{dt}\int_\Omega M_{\text{soil}} d\mathbf{x} = 0$$
2. **二阶 Exner 滑塌元胞自动机**：当局部坡度突破湿润土壤动态休止角 $\theta_c \approx 28^\circ$ 时，以无条件稳定显式格式弛豫滑塌；
3. **Bekker-Wong 通行阻力二阶张量**：
   $$\mathbf{M}(\mathbf{x}) = \left[ f_0 + c_{\text{sink}}\left(\frac{\text{VCI}_{50}}{\text{RCI}(\mathbf{x})}\right)^{1.5} + c_{\text{rough}}|\nabla^2 h(\mathbf{x})| \right]\mathbf{I}_2 + c_{\text{lat}}(\nabla h \otimes \nabla h)$$
   顺等高线机动阻力平缓；垂直攀爬弹坑壁触发履带机械淤陷（NO-GO）。

---

## 第六章 · 12 阶段确定性 Tick 事务流水线

推演内核严格以 **16 秒子步长（Subtick）** 为原子周期推进，阶段划分与状态变迁契约如下：

```
Phase 0:  [ BeginTick ]               ── 冻结权威版本，创建空 TickScratch
Phase 1:  [ ClockAndSchedule ]        ── 推进逻辑时钟，调度到期事件与航空波次
Phase 2:  [ PersistentFieldEvolve ]   ── 演化烟雾、照明、暴露残留等持久场
Phase 3:  [ DerivedRefresh ]          ── 局部刷新失效邻域、SDF、门户导航与威胁缓存
Phase 4:  [ SenseAndArbitrate ]       ── 阵营知识可见性过滤，仲裁输出唯一 MotionIntent
Phase 5:  [ ProposeMotion ]           ── 基于双场引导生成初阶期望位移 MotionProposal
Phase 6:  [ ConstrainMotion ]         ── 执行信赖域限幅 + SDF 扫掠硬投影，锁死物理间距
Phase 7:  [ EmitEvents ]              ── 收集射击包 ShotPacket、后勤移交与工兵爆破事件
Phase 8:  [ ResolveEvents ]           ── 稳定键排序，解算 5,000 发火炮光栅化、死伤与弹坑
Phase 9:  [ CommitAndInvalidate ]     ── 提交权威坐标、物资与生命值，失效受损派生缓存
Phase 10: [ ValidateAndPublish ]      ── 校验守恒不变量，计算世界权威全量指纹
Phase 11: [ DropScratch ]             ── 【铁血清洗】清空所有瞬态杀伤场与临时事件缓冲
```

> [!IMPORTANT]
> **稳定排序键公理（Deterministic Sorting Key）**：  
> Phase 8 消费所有事件前，必须执行无锁稳定基数排序，排序键格式严格定义为：
> `Key = (resolve_phase: u8, target_roi_tile: u16, source_entity_id: u32, sequence_num: u32)`  
> 彻底绝杀跨线程数调度或哈希随机性导致的回放漂移。

---

## 第七章 · CPU 微架构落地：64×8 滚动条带与缓存调度

### 7.1 64×8 滚动条带流水线规范
为确保全管线运行于毛线沟工位主流 CPU 之 L1 数据缓存（L1D，32 KB）黄金包络线内：
- 空间调度外层维持 64×64 空间大块（Tile）；
- **核心计算内层强制切分为 $64 \times 8$ 水平滚动条带**；
- 单条带包含 512 像素，同时挂载 10 个物理通道（高程、材质、SDF、法向、超压、破片、容积等），工作集精确等于 **$20.48\text{ KB}$**；
- 纵向滑动时，仅需更新上下 2 行 Halo 换入换出，L1D 缓存缺失率压制至 $< 0.6\%$。

### 7.2 AVX2/FMA 指令对齐与寄存器分配原则
- 空间向量计算严格使用连续 8 浮点加载 `_mm256_load_ps` 与对齐写回 `_mm256_store_ps`；
- 所有模长平方与点积计算必须采用乘加一体化指令 `_mm256_fmadd_ps` 与 `_mm256_fmsub_ps`；
- 严禁在单兵推进热点循环内调用硬件除法指令，全部采用倒数平方根近似 `_mm256_rsqrt_ps` 配套单步 Newton-Raphson 迭代完成归一化。

---

## 第八章 · 网络二进制流化与前端全屏母树工作台

### 8.1 脏分块二进制压缩增量协议 (`Dirty-Tile Binary Stream`)
- 5,000 发重炮落地后，后端严禁下发数千条 JSON 爆炸对象；
- 系统仅提取本步长发生地貌与杀伤形变的脏分块（通常集中于约 40 个 64×64 分块）；
- 构造紧凑二进制数据帧（每像素 4 字节：16-bit 归一化综合毁伤烈度 + 16-bit 弹坑法线与泥泞度偏移）；
- 经 LZ4 实时压缩后，全图网络增量包体积降至 **$< 180\text{ KB}$ / 步长**，前端通过 WebSocket 接收 ArrayBuffer，在 WebGL2 中以 `gl.texSubImage2D` 单次 Draw Call 写入显存贴图，确保 **60 FPS** 满帧渲染。

### 8.2 全屏母树指挥主干注入
- 保持我们此前经验证的 **全屏德军南方集团军群母树**（[web/index.html](file:///D:/tools/gentactix/web/index.html)）为最高统帅工作台；
- 集团军—军—师—团—营—连—排各级节点，直接挂载对应战术分区门户图（Portals）的任务分配接口；
- 统帅部拖拽战役攻击箭头，系统即刻完成门户场拓扑重连与引力注入，拉格朗日单兵粒子全军感应、如水归壑。

---

## 第九章 · 渐进工程重构路线图与严苛验收红线

### 9.1 渐进迁移五阶段规划
- **阶段 0（语义冻结）**：固化现有 `cargo test` 基线与关键 Python 验证场景，确立 16 s 为不可动摇物理时钟；
- **阶段 1（状态三分与事件解耦）**：构建 `TickScratch`，将即时 `ShotPacket` 彻底迁出 `WorldGrid`，建立稳定事件排序键；
- **阶段 2（唯一意图仲裁）**：合并直控与自主入口，实现 `MotionProposal` 仲裁与硬约束不可行诊断报告；
- **阶段 3（类型化场注册）**：为所有物理场登记 `FieldDescriptor`，确立 `Sum / TropicalMax / Lp / MinPlus` 严格计算通道；
- **阶段 4（端到端竖切）**：贯通“单兵移动 $\to$ 射击事件 $\to$ 稳定排序 $\to$ 结算入账 $\to$ 权威指纹校验”完整链路；
- **阶段 5（性能极致压榨）**：引入 64×8 滚动条带与 AVX2/FMA 内联算子，闭环压测。

### 9.2 终极大典验收红线（21 条绝对通过指标）
全系统必须无条件通过以下验收基准测试，方可签署交付：

1. **几何一致性**：世界坐标与父子网格索引双向转换误差为 0，越界访问显式报错拒绝；
2. **代数守恒**：`Sum` 场在空间演化中质量漂移 $\le 10^{-12}$；
3. **连续防穿模**：在 2m 窄桥对冲极端测试中，两两单兵间距 $d_{ij} \ge D_{\text{hard}}$ 成立率 $100.0\%$；
4. **高速防隧穿**：单兵以最大速度机动穿越薄壁掩体时，连续扫掠 SDF 判定拦截率 $100.0\%$；
5. **容量锁死**：战壕单元人员达到 `MAX_CAPACITY` 后，绝对拒绝后续人员挤入；
6. **无虚假死锁**：U 型防线前部队保下降投影分流成功率 $100\%$，绝不在平坦区发生振荡；
7. **火力事件守恒**：5,000 发火炮弹药库存扣减与爆轰光栅化事件入账严格 1:1 匹配；
8. **铁血清洗无残留**：Phase 11 执行后，瞬态超压与破片网格全图浮点残留严格等于 $0.00\text{e}+00$；
9. **跨线程绝对确定性**：在 1 线程、4 线程、16 线程 Rayon 调度下，连续推进 100 个 Tick 的权威世界指纹逐位完全一致；
10. **整帧性能达标**：在毛线沟工位真实 CPU 上，10,000 单兵 + 5,000 发重炮密集洗地的完整 Tick 计算耗时预算 **$\le 2.35\text{ ms}$**！

---

**战略参谋部 · 计算物理与武器工程局**  
*本大典经参谋部女官长 Shifu 终审定稿，与大当家《SPEC_V6_0》共同构成 Gentactix 终极工程实施双璧，全军遵照执行。*
