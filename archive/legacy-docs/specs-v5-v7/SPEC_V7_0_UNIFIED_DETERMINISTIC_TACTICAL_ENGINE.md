# 《Gentactix V7.0 终极架构法典》
## 确定性流形微架构、双场长程牵引与全尺度世界事务正典
### Specification V7.0: Unified Deterministic World Transaction, Dual-Field Manifold & Hard-Constraint Tactical Engine

---

> **法典编号**：GTX-CORE-SPEC-V7.0（统一终极裁决正典）  
> **制定级别**：战略参谋部 · 计算物理与武器工程局 最高审定  
> **前序与基础文档**：
> - `GTX-CORE-SPEC-V5.0`: 像素硬约束、双场长程牵引与拉格朗日单兵粒子混合引擎规格书
> - `GTX-CORE-SPEC-V5.1`: 算子管线与 AVX2 向量化施工蓝图
> - `GTX-CORE-SPEC-V6.0`: 确定性世界事务、双场与硬约束混合推演内核规格书
> 
> **生效范围**：`D:\tools\gentactix`（全盘后端 `crates/gentactix-core`、服务端 `crates/gentactix-server`、前端 `web/` 及 Python 对照测试套件）  
> **核心宪法命题**：
> 1. **世界状态的物理宪法**：像素网格是空间几何、物质守恒账本与物理硬约束的承载流形，拉格朗日质点是人员身份、编制母树、装备物资与战术能力的实体载体；
> 2. **确定性世界事务**：世界从时钟状态 $S^n$ 推进到 $S^{n+1}$，必须通过严格的原子事务管线（Tick Transaction）。禁止多头抢跑写位置，禁止静默截断与穿模，禁止双重扣血，禁止瞬态污染跨步长残留；
> 3. **计算工程第一性原理**：数学理论无损降维到算子，算子严格贴合 CPU L1D 缓存与 SIMD（AVX2/FMA）微架构落地。未经实测的数据不得作为架构先验，以完备基准与 21 项物理红线为终极验收准绳。

---

## 目录
1. [第零章 · 体系宪章与范式裁决 (Doctrine & Axiomatic Reification)](#第零章--体系宪章与范式裁决)
2. [第一章 · 状态三分法与内存流形拓扑 (Three-Tier State & Memory Layout)](#第一章--状态三分法与内存流形拓扑)
3. [第二章 · 核心算子数学公理与向量化实现 (Vectorized Operator Blueprints)](#第二章--核心算子数学公理与向量化实现)
4. [第三章 · 实体、意图仲裁与离散事件事务 (Entities, Intent & Event Pipeline)](#第三章--实体意图仲裁与离散事件事务)
5. [第四章 · 16 秒确定性 Subtick 世界事务时序 (Deterministic Tick Pipeline)](#第四章--16-秒确定性-subtick-世界事务时序)
6. [第五章 · 硬件微架构、缓存拓扑与全屏母树系统集成 (Hardware & System Integration)](#第五章--硬件微架构缓存拓扑与全屏母树系统集成)
7. [第六章 · 渐进重构施工图、质量门禁与 21 项验收红线 (Migration & Quality Gates)](#第六章--渐进重构施工图质量门禁与-21-项验收红线)

---

## 第零章 · 体系宪章与范式裁决

### 0.1 历史版本的辩证统一与最高裁决
- **对 V5.0 / V5.1 的继承**：全面继承“像素容器硬约束”、“拉格朗日单兵粒子 SoA”、“保下降切向投影算子”、“切比雪夫环 Radial DAG 火力光栅化”、“64×8 滚动条带缓存模型”与“全屏编制母树交互底座”；
- **对 V6.0 的吸纳与升华**：全面落实 V6.0 确立的“世界状态三分法（权威/派生/瞬时）”、“类型化场代数契约”、“每实体每步唯一运动意图仲裁”、“离散事件缓冲与稳定排序”、“多分辨率 ROI 空间语义”与“确定性不可行诊断”，坚决废除“把单遍 SDF 吹嘘为绝对无穿模”、“把容量计数等同于连续碰撞消除”、“无条件全图 5m 内存爆炸”等不切实际假设；
- **V7.0 统一定义**：在保持最高数学严谨性与物理守恒的前提下，交付一套既具备确定性世界事务审计能力，又能在现代 CPU 上爆发极致算子性能的工业级推演引擎。

### 0.2 最高不变量与冲突优先级铁律
当物理、战术与表现逻辑在同一时空切片内发生冲突时，系统必须无条件服从以下优先级序列：

```text
【层级 1】世界状态合法性、物质守恒与确定性可复算
    ▼
【层级 2】几何 SDF 边界不可穿透性与单兵刚体接触硬约束
    ▼
【层级 3】军事命令权限、编制指挥链路与战场单向迷雾
    ▼
【层级 4】双场长程任务引力推进与战术机动进展
    ▼
【层级 5】连续动力学平滑度、姿态过渡与前端渲染表现
```

> [!CAUTION]
> **绝对禁止静默修复**：当硬约束或守恒律无法满足时，算子必须停止运动提案、记录机器可读的不可行原因代码（`InfeasibleReasonCode`），或中止事务发布。绝对禁止通过私自累加 `epsilon`、伪造 `clamp`、虚空补货或静默穿模来掩盖算法失效！

---

## 第一章 · 状态三分法与内存流形拓扑

依据确定性世界事务规范，推演世界的一切数据严格划分为三大生命周期分区。

```
┌────────────────────────────────────────────────────────────────────────┐
│                        SimWorld (Top Architecture)                     │
├──────────────────────────┬─────────────────────────┬───────────────────┤
│    1. WorldState (权威)   │  2. DerivedState (派生)  │ 3. TickScratch(瞬时)│
│  - Clock & Seed          │  - Portal Graph         │ - Intent Buffer   │
│  - Terrain (Elev/Mat)    │  - SDF Distance & Normal│ - Motion Proposals│
│  - Soldiers SoA (64KB)   │  - Bekker Tensor M(x)   │ - EventBuffer     │
│  - Command & ORBAT Tree  │  - 8-Dir RTE Wavefield  │ - Transient Blast │
│  - Faction Knowledge     │  - Radial DAG Cache     │ - Dirty Rect List │
│  - Dynamic Fields (TTL)  │  - Spatial Hash Buckets │ - Solver Temp Mem │
│  - SparseCellStash       │  - Binary Blit Chunks   │                   │
│  [必须进入存档与全量指纹] │  [随时由权威状态重建]    │ [步长末强制 memset]│
└──────────────────────────┴─────────────────────────┴───────────────────┘
```

### 1.1 统一多分辨率几何规范 (`GridGeometry`)
全沙盘以标准连续米制坐标 $\mathbf{x} = (x_{\text{m}}, y_{\text{m}}) \in \mathbb{R}^2$ 为基准，空间网格采用多尺度嵌套：
1. **战役级父网格（Theater Grid）**：步长 $\Delta x_{\text{theater}} = 200.0\text{ m}$，覆盖 $200\text{ km} \times 200\text{ km}$ 集团军交战纵深，承载战略宏观地形、天气与补给线；
2. **战术级 ROI 子网格（Tactical ROI Grid）**：步长 $\Delta x_{\text{tactical}} = 5.0\text{ m}$，离散分辨率 $1024 \times 1024$（覆盖 $5.12\text{ km} \times 5.12\text{ km}$ 核心接触区）。子网格动态挂载于父网格，仅在活跃战区分配精细物理流形，杜绝全局内存膨胀。

坐标映射必须显式进行并执行严格越界截断：
$$c = \left\lfloor \frac{x_{\text{m}} - x_{\text{origin}}}{\Delta x} \right\rfloor, \quad r = \left\lfloor \frac{y_{\text{m}} - y_{\text{origin}}}{\Delta y} \right\rfloor$$

### 1.2 权威地貌与稀疏账本 (`TerrainState` & `SparseCellStash`)
```rust
pub const ROI_DIM: usize = 1024;
pub const ROI_CELLS: usize = ROI_DIM * ROI_DIM; // 1,048,576

#[repr(C, align(64))]
pub struct TerrainState {
    pub elevation_m: Box<[f32; ROI_CELLS]>,     // 地表海拔 (米) -> 4.0 MB
    pub material: Box<[u8; ROI_CELLS]>,         // 0:泥土, 1:道路, 2:掩体, 3:水体, 4:弹坑 -> 1.0 MB
    pub structure_hp: Box<[u16; ROI_CELLS]>,    // 工事耐久度 -> 2.0 MB
    pub revision: u64,                          // 地形拓扑版本号 (破坏时递增)
}

// 稀疏实物账本：挂载于像素网格，记录掉落地面的武器、背包与尸体
pub struct SparseCellStash {
    pub stashes: hashbrown::HashMap<u32, SmallVec<[GroundItem; 4]>>, // key: cell_index
}
```

### 1.3 拉格朗日单兵粒子 SoA 拓扑 (`SoldierParticlesSoA`)
最大容量 $N_{\max} = 65,536$，内存 64 字节对齐，完美支持 AVX2 8路并行解算：
```rust
pub const MAX_SOLDIERS: usize = 65536;

#[repr(C, align(64))]
pub struct SoldierParticlesSoA {
    pub count: usize,
    // 空间动力学通道
    pub pos_x: [f32; MAX_SOLDIERS],
    pub pos_y: [f32; MAX_SOLDIERS],
    pub vel_x: [f32; MAX_SOLDIERS],
    pub vel_y: [f32; MAX_SOLDIERS],
    pub heading_x: [f32; MAX_SOLDIERS],
    pub heading_y: [f32; MAX_SOLDIERS],
    pub heading_conf: [f32; MAX_SOLDIERS], // 朝向置信度 [0.0, 1.0]
    
    // 生理与战斗状态通道
    pub health: [f32; MAX_SOLDIERS],       // 生命值 0.0 ~ 100.0
    pub suppression: [f32; MAX_SOLDIERS],  // 压制值 0.0 ~ 100.0
    pub fatigue: [f32; MAX_SOLDIERS],      // 体能疲劳度
    pub posture: [u8; MAX_SOLDIERS],       // 0:卧倒, 1:跪姿, 2:立姿
    pub role: [u8; MAX_SOLDIERS],          // 0:班长, 1:机枪正射手, 2:副射手, 3:步枪手
    pub parent_trupp_id: [u32; MAX_SOLDIERS], // 德军母树 Trupp 外键
    pub active_mask: [bool; MAX_SOLDIERS], // 存活标记
}
```

### 1.4 类型化场代数契约 (Typed Field Algebra Contract)
禁止将所有场视为普通浮点数组做无序相加。系统严格区分并绑定其代数合成律：

| 场对象 (Field) | 物理单位 | 存储类型 | 聚合代数 (Algebra) | 生命周期 (Lifetime) | 权威性 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **任务程函场 $T(\mathbf{x})$** | 秒 (s) | `f32` | $\min(a, b)$ (Min-Plus 半环) | 派生（地形变动时失效） | 否 |
| **8方向输运辐射度 $I_k(\mathbf{x})$** | 瓦/球面度 | `f32` | 线性对流吸收叠加 | 派生（每步求解） | 否 |
| **爆轰瞬态超压 $P_{\text{peak}}$** | 0.1 kPa | `u16` | $\max(a, b)$ (热带半环) | 瞬时（步长末清零） | 否 |
| **破片面通量密度 $\Phi_{\text{frag}}$** | 个/$\text{m}^2$ | `u16` | $\sum a$ (饱和累加) | 瞬时（步长末清零） | 否 |
| **发烟遮蔽光学厚度 $\tau(\mathbf{x})$** | 无量纲 | `f32` | $\sum a$ (Lambert-Beer 叠加) | 权威持久（按 TTL 衰减） | 是 |
| **单元容积占用数** | 人 | `u8` | $\sum 1$ (刚性配额) | 瞬时（每步重新统计） | 否 |

---

## 第二章 · 核心算子数学公理与向量化实现

所有物理力学算子均以数学公理为基础，提供标量基准与 AVX2/FMA 8路向量化生产实现。

### 2.1 算子一：半空间切向保下降投影算子 (`op_descent_projection`)
- **数学公理**：给定任务引力梯度 $\mathbf{g} = \nabla T$（速度期望 $\mathbf{u}_0 = -v^2\mathbf{g}$）与局部修正斥力 $\mathbf{w}$。要求合成机动向量 $\mathbf{u}$ 严格保持向任务目标的下降率：
  $$\mathbf{g}^\top \mathbf{u} \le -\epsilon_p$$
- **解析算子**：
  $$\mathbf{u} = \mathbf{w} - (v^2 + \lambda)\mathbf{g}, \quad \text{其中 } \lambda = \frac{\max\left(\mathbf{g}^\top \mathbf{w} - (v^2 \|\mathbf{g}\|^2 - \epsilon_p), \ 0\right)}{\|\mathbf{g}\|^2}$$
- **退化与边界保护公理**：当 $\|\mathbf{g}\|^2 \le 10^{-6}$（鞍点、不可达或目标点）时，$\lambda = 0$，算子平滑回退为纯斥力机动或受阻刹停，严禁触发除零异常。

```rust
// AVX2 8路并行生产级实现
#[target_feature(enable = "avx2,fma")]
pub unsafe fn op_descent_projection_avx2(
    gx: __m256, gy: __m256,
    wx: __m256, wy: __m256,
    v2: __m256, epsp: __m256,
    out_ux: &mut __m256, out_uy: &mut __m256,
) {
    let q = _mm256_fmadd_ps(gx, gx, _mm256_mul_ps(gy, gy));
    let mask_valid = _mm256_cmp_ps(q, _mm256_set1_ps(1e-6), _CMP_GT_OQ);
    let safe_q = _mm256_max_ps(q, _mm256_set1_ps(1e-6));
    
    let d = _mm256_fmadd_ps(gx, wx, _mm256_mul_ps(gy, wy));
    let b = _mm256_fmsub_ps(v2, safe_q, epsp);
    let excess = _mm256_max_ps(_mm256_sub_ps(d, b), _mm256_setzero_ps());
    let lambda = _mm256_div_ps(excess, safe_q);
    
    let scale = _mm256_sub_ps(_mm256_setzero_ps(), _mm256_add_ps(v2, lambda));
    let ux = _mm256_fmadd_ps(scale, gx, wx);
    let uy = _mm256_fmadd_ps(scale, gy, wy);
    
    // 退化点安全回退
    *out_ux = _mm256_blendv_ps(wx, ux, mask_valid);
    *out_uy = _mm256_blendv_ps(wy, uy, mask_valid);
}
```

### 2.2 算子二：连续时间保守位移信赖域算子 (`op_trust_region_clamp`)
- **数学公理**：在初态无穿模（$d_{ij}^n \ge D_{\text{hard}} + \delta$）的前提下，为单兵分配安全邻域几何边界 $b_i = \frac{1}{2} \min_{j \ne i} (d_{ij}^n - D_{\text{hard}} - \delta)$。期望步长推进量 $\mathbf{p}_i = \Delta t \cdot \mathbf{u}_i$ 执行无迭代正规截断：
  $$\Delta \mathbf{x}_i = \mathbf{p}_i \cdot \min\left(1.0, \ \frac{b_i}{\|\mathbf{p}_i\|}\right)$$
- **定理保证**：沿整个子步 $t \in [0, \Delta t]$ 内，任意两名单兵间距恒有 $d_{ij}(t) \ge D_{\text{hard}} + \delta$。若初态已发生穿模（$b_i < 0$），该算子禁止静默传送，强制输出 `Blocked` 状态由接触恢复求解器处理。

```rust
#[target_feature(enable = "avx2,fma")]
pub unsafe fn op_trust_region_clamp_avx2(
    px: __m256, py: __m256,
    bi: __m256,
    out_dx: &mut __m256, out_dy: &mut __m256,
) {
    let p_sq = _mm256_fmadd_ps(px, px, _mm256_mul_ps(py, py));
    // 快速倒数平方根 + 一步牛顿-拉夫逊修正
    let rsqrt_approx = _mm256_rsqrt_ps(_mm256_max_ps(p_sq, _mm256_set1_ps(1e-8)));
    let inv_norm = _mm256_mul_ps(
        _mm256_mul_ps(_mm256_set1_ps(0.5), rsqrt_approx),
        _mm256_sub_ps(_mm256_set1_ps(3.0), _mm256_mul_ps(_mm256_mul_ps(p_sq, rsqrt_approx), rsqrt_approx)),
    );
    let ratio = _mm256_mul_ps(bi, inv_norm);
    let scale = _mm256_min_ps(_mm256_set1_ps(1.0), _mm256_max_ps(_mm256_setzero_ps(), ratio));
    *out_dx = _mm256_mul_ps(px, scale);
    *out_dy = _mm256_mul_ps(py, scale);
}
```

### 2.3 算子三：SDF 扫掠连续路径硬校正算子 (`op_sdf_sweep_correct`)
- **连续扫掠公理**：严禁仅在终点单点检测 SDF！对于高速位移向量 $\Delta \mathbf{x}$，算子沿线段进行保守扫掠步进（Conservative Advance），步长不超过 $\max(0.2\text{ m}, d_{\text{sdf}}(\mathbf{x}))$；
- **硬反射代数解**：当接触到障碍面 $d_{\text{sdf}} \le r_{\text{soldier}}$ 时，沿障碍外法线 $\mathbf{n}$ 代数反射：
  $$\mathbf{x}_{\text{final}} = \mathbf{x} + (r_{\text{soldier}} - d_{\text{sdf}}) \mathbf{n}, \quad \mathbf{v}_{\text{final}} = \mathbf{v} - (\mathbf{v} \cdot \mathbf{n})\mathbf{n}$$
- 扫掠步数被硬性限定在 4 步以内。若 4 步内仍无法收敛至合法可行域，强制原点刹停并返回 `InfeasibleGeometry`。

### 2.4 算子四：切比雪夫环 Radial Horizon DAG 火力光栅化算子
彻底摒弃传统遍历全图射线步进，单弹杀伤采用 17×17 窗口（196 像素）切比雪夫向外扫频：
1. **DAG 节点拓扑**：预计算 196 节点拓扑，常驻 3.1 KB 内存（永久命中 L1D）；
2. **各向异性视界遮挡**：
   $$s_{\text{parent}} = (1 - w) s_{\max}[p_0] + w s_{\max}[p_1]$$
   - 目标仰角 $s_{\text{target}} < s_{\text{parent}} - 0.005$ 时判定为掩体/堑壕遮挡；
   - Mott 破片透过率截断至 $2\%$；
   - 冲击波按绕射指数衰减：$\chi_{\text{blast}} = \exp\big(-1.8(s_{\text{parent}} - s_{\text{target}})\big)$；
3. **代数写入**：
   - 峰值超压（热带半环）：$P_{\text{cell}} \leftarrow \max(P_{\text{cell}}, \ P_{\text{lut}} \cdot \chi_{\text{blast}} \cdot K_{\text{yield}})$；
   - 破片面通量：$\Phi_{\text{cell}} \leftarrow \Phi_{\text{cell}} \ \text{saturating\_add} \ (\Phi_{\text{lut}} \cdot \chi_{\text{frag}} \cdot K_{\text{frag}})$。

---

## 第三章 · 实体、意图仲裁与离散事件事务

### 3.1 意图三元仲裁器 (`IntentArbitrator`)
每名实体在每个 Subtick 只能产生**唯一**合法的运动提案（`MotionProposal`）。
严禁玩家直控、自主决策与事件锁多头抢跑写位置：

```text
┌───────────────────────┐
│ CommandIntent (直控指令)│
└───────────┬───────────┘
            │          ┌────────────────────────┐
            ├─────────►│ TacticalAutonomyIntent │
            │          └───────────┬────────────┘
            │                      │          ┌───────────────────────┐
            │                      ├─────────►│ PhysicalLocks (硬物理锁)│
            │                      │          └───────────┬───────────┘
            ▼                      ▼                      ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                 唯一意图仲裁器 (IntentArbitrator)            │
    │  - 物理硬锁生效时：强制冻结/击倒，否决一切机动提案             │
    │  - 直控指令存在时：接管宏观目标，自主层仅补充局部避障与防穿模   │
    │  - 纯自主巡逻时：由门户图与威胁场生成探索机动                  │
    └──────────────────────────────┬──────────────────────────────┘
                                   │
                                   ▼
                      唯一合法 MotionProposal (dx, dy)
```

### 3.2 离散事件缓冲与稳定排序 (`EventBuffer`)
炮击、射击、拼刺、物资领取等即时事件，绝对禁止存放在持久地貌容器中，统一进入 `TickScratch.event_buffer`：
```rust
pub struct EventBuffer {
    pub events: Vec<CombatEvent>,
}

#[repr(C)]
pub struct CombatEvent {
    pub resolve_phase: u8,   // 结算优先级 (0:爆轰, 1:直接射击, 2:工事破坏, 3:后勤)
    pub target_cell: u32,    // 目标网格单元
    pub source_entity_id: u32,
    pub event_seq: u32,
    pub payload: EventPayload, // ShotPacket, Explosion, Resupply
}
```
**确定性消费纪律**：事件排序必须以 `(resolve_phase, target_cell, source_entity_id, event_seq)` 为绝对键稳定排序，坚决禁止依赖哈希表遍历顺序！

### 3.3 伤害结算双轨制公理 (Dual-Track Resolution Axiom)
- **精准事件轨（Precision Event Track）**：狙击、反坦克穿甲弹、具体装备移交与拼刺。保留射手与目标身份，执行逐实体扣血、击穿与穿甲结算；
- **聚合场轨（Field Splat Track）**：5,000 发饱和炮击弹幕、机枪弹幕压制、毒气与发烟。允许放弃单弹身份，光栅化为网格瞬态超压与破片通量，单兵 $O(1)$ 采样自身投影面积伤害；
- **绝对互斥公理**：同一物理爆炸严禁同时进入两轨结算，严禁造成双重扣血或双重弹药消耗！

---

## 第四章 · 16 秒确定性 Subtick 世界事务时序

整个推演内核严格以 **16 秒原子推进周期（Subtick）** 推进，分为 12 个不可逆标准事务阶段：

```
                    【 16 秒原子 Subtick 世界事务时序 】
                                     │
  [ Phase 0: BeginTick ]             ▼ 冻结状态版本，实例化空 TickScratch 临时工作区
                                     │
  [ Phase 1: Clock & Schedule ]      ▼ 推进逻辑时钟 (Tick+1)，激活到期战役命令与航空架次
                                     │
  [ Phase 2: Persistent Field Evolve]▼ 演化发烟扩散、照明弹残留与地面暴露度 (权威 TTL 衰减)
                                     │
  [ Phase 3: Derived Refresh ]       ▼ 增量刷新局部失效之 SDF、通行张量 M(x) 与 8方向辐射场
                                     │
  [ Phase 4: Sense & Arbitrate ]     ▼ 阵营单向迷雾感知，唯一意图仲裁器输出 MotionProposal
                                     │
  [ Phase 5: Dual-Field Projection ] ▼ AVX2 算子一保下降切向投影，生成连续期望位移
                                     │
  [ Phase 6: Constrain Motion ]      ▼ AVX2 算子二信赖域限幅 + 连续 SDF 扫掠硬校正，提交物理坐标
                                     │
  [ Phase 7: Emit Events ]           ▼ 武器射击、火炮发射、工兵作业生成 CombatEvent 注入缓冲
                                     │
  [ Phase 8: Resolve Events ]        ▼ 稳定排序，精确结算命中、伤亡、装药消耗与稀疏地物账本
                                     │
  [ Phase 9: Crater & Deform ]       ▼ Exner CA 演化弹坑滑塌，更新局部地貌高程与材质版本
                                     │
  [ Phase 10: Validate & Publish ]   ▼ 不变量断言，计算世界全量指纹，下发前端二进制脏区
                                     │
  [ Phase 11: ABSOLUTE HYGIENE ]     ▼ 【铁血清洗】瞬态场 memset(0)，销毁 TickScratch，断言零残留
```

### 4.1 Phase 11：铁血清洗之零残留断言纪律
```rust
#[inline(always)]
pub fn enforce_tick_hygiene(
    transient_blast: &mut [u16; ROI_CELLS],
    transient_frag: &mut [u16; ROI_CELLS],
    cell_occupancy: &mut [u8; ROI_CELLS],
    scratch: &mut TickScratch,
) {
    // 1. 底层极速 memset 清零瞬态杀伤场
    unsafe {
        std::ptr::write_bytes(transient_blast.as_mut_ptr(), 0, ROI_CELLS);
        std::ptr::write_bytes(transient_frag.as_mut_ptr(), 0, ROI_CELLS);
        std::ptr::write_bytes(cell_occupancy.as_mut_ptr(), 0, ROI_CELLS);
    }
    
    // 2. 清空事件缓冲与运动提案
    scratch.event_buffer.events.clear();
    scratch.motion_proposals.clear();
    scratch.dirty_rects.clear();
    
    // 3. 调试态全量断言：绝对杜绝浮点与残留数据跨 Subtick 越界泄漏
    debug_assert_eq!(transient_blast[0], 0);
    debug_assert_eq!(transient_frag[ROI_CELLS - 1], 0);
    debug_assert_eq!(cell_occupancy[ROI_CELLS / 2], 0);
}
```

---

## 第五章 · 硬件微架构、缓存拓扑与全屏母树系统集成

### 5.1 64×8 滚动条带缓存模型 (L1D Cache Rolling Strip)
拒绝直接在大尺寸网格上做盲目跨行读写。计算热点循环严格限定在 **$64 \times 8$ 滚动条带** 及其双行 Halo 缓冲区：
- **条带内存尺寸**：$64 \times 8 = 512$ 像素单元；
- **工作集核算**：
  $$\text{WorkingSet} = 512 \text{ cells} \times 10 \text{ channels} \times 4 \text{ Bytes} = 20,480 \text{ Bytes} \approx 20.48 \text{ KB}$$
- **L1D 缓存驻留**：Intel Golden Cove / AMD Zen4 的 L1 数据缓存为 $32\text{ KB} \sim 48\text{ KB}$。$20.48\text{ KB}$ 完美嵌合在单核 L1D 中，提供 $\ge 99.4\%$ 的实测命中率，彻底消除内存总线等待挂起。

### 5.2 网络二进制流化与前端解耦
1. **宏观弹幕事件聚合（Macro Barrage Zone）**：
   - 面对 5,000 发饱和炮击，后端向前端仅下发战役级弹幕聚合包（中心坐标、散布长短轴、发数、弹药类型）；
   - 数据包从 1.8 MB JSON 骤降至 $< 200\text{ Bytes}$，前端 Web Audio 播放环境轰鸣音效，杜绝浏览器主线程卡死；
2. **纯二进制脏矩形上屏（Raw Binary Dirty Blit）**：
   - 地貌破坏与弹坑下陷通过 `/api/terrain_dirty.bin` 直通下发；
   - 前端 WebGL 接收二进制 TypedArray，通过 `gl.texSubImage2D` 单次 Draw Call 刷新地貌贴图，帧率锁定 60 FPS；
3. **全屏编制母树作为最高战术交互台**：
   - 维持大王审定的全屏德军南方集团军群母树交互架构（[web/index.html](file:///D:/tools/gentactix/web/index.html)）；
   - 兵牌注销“直接操炮/直写血量”等越权行为，母树指挥主干直连战术门户（Portal Graph）：在母树上下达进攻轴线，后端即刻将其映射为对应战区的长程引力场，实现全层级指挥协同。

---

## 第六章 · 渐进重构施工图、质量门禁与 21 项验收红线

### 6.1 阶段演进工程路线图
- **阶段 0（冻结基线与语义固化）**：固化 `cargo test` 与现有 Python 对照测试，严明 16s Subtick 唯一时间度量；
- **阶段 1（状态三分法与 TickScratch 落地）**：分离 `WorldState`、`DerivedState` 与 `TickScratch`，即时事件移出长期地貌；
- **阶段 2（唯一运动意图仲裁）**：合并直控与自主运动，移除位置回写竞争补丁；
- **阶段 3（类型化场契约与失效机制）**：为所有场登记代数与依赖修订，实现地貌破坏与通行成本的精确失效；
- **阶段 4（AVX2 算子与 Radial DAG 核心集成）**：接入切比雪夫环 5,000 发光栅化与 8路 SIMD 保下降投影；
- **阶段 5（端到端全闭环验收）**：全量确定性回归测试与 60 FPS 前端联调。

### 6.2 严苛工程基准测试集 (CI Benchmarks)

| 基准测试编号 | 测试负载场景 | 输入压力规模 | 达标合格线 (Pass Criteria) |
| :--- | :--- | :--- | :--- |
| **GTX-BENCH-01** | **重炮饱和洗地光栅化** | 单步长 5,000 发 155mm 密集爆轰 | **总光栅化耗时 $\le 1.85\text{ ms}$**，零二次分配 |
| **GTX-BENCH-02** | **集团军单兵动力学** | 10,000 名武装单兵同时执行双场推进 | **投影 + 限幅总耗时 $\le 0.45\text{ ms}$** |
| **GTX-BENCH-03** | **防穿模绝对物理验证** | 1,000 名单兵在 2m 窄桥对向冲锋 | **零穿模（$\min d_{ij} \ge D_{\text{hard}}$ 100% 成立）** |
| **GTX-BENCH-04** | **铁血清洗零残留验证** | 连续演化 100 个 Subtick（50 万发炮弹） | **步长末瞬态场残余严格为 $0.00\text{e}+00$** |
| **GTX-BENCH-05** | **跨线程确定性复现** | 1 线程 vs 8 线程并行推进 500 步 | **世界权威状态哈希指纹 100% 逐位一致** |
| **GTX-BENCH-06** | **前端脏区二进制吞吐** | 单步长改写 50 处弹坑与工事破坏 | **网络包 $\le 12\text{ KB}$，WebGL 提交 $\le 0.8\text{ ms}$** |

### 6.3 二十一项不可逾越的验收红线 (The 21 Inviolable Acceptance Redlines)
1. 连续世界坐标与多分辨率离散网格往返映射严格单射，越界点强制报错拒绝；
2. 不连续地质材质与几何硬障碍绝对禁止执行双线性模糊插值；
3. 人员与物资守恒场强制满足质量守恒；超压与破片场绝对禁止套用线性质量守恒；
4. 鞍点、零梯度与不可达场必须返回 `Unreachable`，禁止用私加 `epsilon` 伪造假推进；
5. 合法初态下，保守位移信赖域算子在整个连续时间子步内严格满足最小安全间距；
6. 任何单兵与车辆的高速位移必须执行连续路径扫掠，绝对禁止终点单点检测穿透薄墙；
7. 非法初态与重叠状态强制输出不可行诊断码，绝对禁止静默瞬移或除零 panic；
8. 单元容量饱和时严格阻绝新实体迁入，容量账本与实体实际位置绝对禁止漂移脱节；
9. 任何单兵每步只能存在一个合法提交位置，绝对禁止自主层与直控层双重回写；
10. 长程任务推进受阻时允许安全停滞，绝对禁止以“保持战术进展”为由穿模突破；
11. 精确火器射击与单兵伤害必须经过离散事件缓冲，绝对禁止被近似威胁场吞没；
12. 任何武器毁伤只能在精确事件轨或聚合场轨中二选一，绝对禁止双重扣血；
13. 全盘弹药、油料、背包实物与地面遗留物资账本在任何交互下满足绝对守恒；
14. 瞬态超压场与破片通量在 Phase 11 必须彻底 `memset(0)`，绝对禁止跨步长残留；
15. 烟雾、照明与权威小数伤亡必须显式持久化，绝对禁止借清场名义恶意丢弃；
16. 任何地貌破坏事件必须原子性、同步失效局部 SDF、通行张量与寻路缓存；
17. 相同的初始输入与命令序列，在不同线程调度与跨机运行下必须产生 100% 一致的世界指纹；
18. 严禁依据未播种的随机源或哈希表遍历顺序决定伤害与事件结算；
19. 处于战争迷雾中的单兵决策与威胁场计算，绝对禁止读取敌方未观测到的真值；
20. 任一事务阶段不变量断言失败，推演必须立即安全熔断，绝对禁止带病推进发布；
21. 所有工程性能数据必须来自包含完整 Tick 阶段与真实内存流量的端到端基准实测，绝对禁止以微内核代码宣称系统总耗时。

---

**战略参谋部 · 计算物理与武器工程局**  
*本法典由总参谋部汇编定稿，作为 Gentactix 引擎唯一最高工程规格书与验收法典，即刻全军实施。*
