# GENTACTIX 四大数学研究报告审定、工程解构与内核转化总论

- **文档路径**：`D:\tools\gentactix\docs\RESEARCH_THEORY_SYNTHESIS_AND_ARCHITECTURAL_VERDICT.md`
- **审阅对象**：`D:\tools\gentactix\docs\research-theory\` 下四篇专项数学建模报告
- **基准约束**：纯二进制空间网格 $\Delta x = 2^3\text{ m} = 8.0\text{ m}$（`i = int(x) >> 3`），原子时间步长 $\Delta t = 2^4\text{ s} = 16.0\text{ s}$，全图拉格朗日粒子 100% 实算，跨平台位级确定性（Bitwise Determinism）
- **审定结论**：四篇报告在连续-离散混合动力系统建模上达到了极高水准，正面攻克了此前困扰双场架构的四大物理悖论；但其中存在 **3 处危及跨平台确定性与内存带宽的学院派陷阱**，必须经本总论裁剪修正后方可合入 `gentactix-core` 生产内核。

---

## 一 · 总体审定：四篇报告如何闭环解决四大战术悖论？

| 报告编号与主题 | 彻底解决的战术/物理悖论 | 核心数学武器（入典级算子） | 实测定量收益 |
| :--- | :--- | :--- | :--- |
| **报告一**：任务场 $G$ 的在途预扣核与异质粒子无冲突分流 | 1. 500m 外多班组响应时的“在途超卖”与到场瞬间 90° 急转弯鬼畜；<br>2. 排雷/救护完工撤离后的“地雷复活 Bug”；<br>3. 在途班组半路伤亡引发的邻班高频抢单横跳。 | 1. Amanatides-Woo 线段切割 + Sutherland-Hodgman 多边形裁剪 + 九点保质量核 $K_{\text{iso}}(\alpha=0.25)$；<br>2. 五元组状态空间 $(G_0, \hat{\Phi}_v, \Phi_u, R_{\text{pool}}, G_{\text{alloc}})$ 与双模态门控 $\beta_{\text{mode}} \in \{0, 1\}$；<br>3. 二次代价 Laguerre 晶胞（Power Diagram）无交叉分流；<br>4. 指数驰豫池 $R_{\text{pool}}$（$\tau_{\text{rel}}=32\text{s}$）+ 进度迟滞壁垒 $\Delta C_{\text{hyst}}$（$p_m \ge 0.75$ 硬锁）。 | • 旋转账单守恒误差从 SDF 的 **20%** 降至 **$1.15 \times 10^{-16}$**；<br>• 在途 $>45^\circ$ 急转弯从 **13 次降为 0 次**，完工时间缩短 **40.9%**；<br>• 战损抢单振荡从 **18 次降为 0 次**。 |
| **报告二**：`UI-03M2` 任意多边形/折线虚拟实例生成与 16s 边走边干巡回规划 | 1. 传统战棋“原地读进度条、时间一到整片雷区凭空冒出”的虚假表现；<br>2. L 型/U 型非凸多边形内施工路径穿墙与自交；<br>3. 施工中途被炮火打断时“99% 进度归零作废”的不自洽。 | 1. $\Delta x / 1024 = 7.8125\text{ mm}$ 子格定点量化 + `SplitMix64` 循环移位不变种子；<br>2. 三模态 $O(K)$ 生成：正六边形抗殉爆梅花桩、迎敌前沿斜接内缩（6m/18m）、FM 5-15 之字形折曲堑壕（$A_{\text{zig}}=1.2\text{m}$）；<br>3. 测地牛耕 + 带状 2-Opt 解交 + 凹角内缩 $0.50\text{m}$ 路标绕行；<br>4. 移动-作业一维增广弧长轴 $\tilde{s}$（$\lambda_{\text{eq}}=0.20\text{ m/(人}\cdot\text{s)}$）解析积分与四态打断状态机。 | • 点集生成复杂度 **$O(m + K)$**，零事后碰撞检测（AT 雷最小间距 **$4.59\text{ m} > 4.0\text{ m}$**）；<br>• L 型非凸域路径缩短 **26.3%**，**0 自交**；<br>• 炮火打断时前半截已转正实体 100% 永久留存。 |
| **报告三**：信息场 $I$ 正交双通道、炮火源项压制坍塌与双点测距标定核 | 1. “我方重炮压制降低目标格威胁 $\rightarrow$ 导致友军步兵顶着自家 150mm 榴弹冲进弹坑”的自杀悖论；<br>2. 单位经过战壕掩体后被后向负梯度死死拉住走不到终点；<br>3. 测距标定增益随观察-射击位置与夹角变化的物理建模缺失。 | 1. 正交三元组 $I = I_{\text{intel}} \oplus (H_{\text{blast}}, I_{\text{threat}})$；<br>2. 对数生存概率融合与 0.05 迟滞回差显影门限；<br>3. 快衰减落点爆炸危险 $H_{\text{blast}}$（$\tau_{\text{blast}}=9\text{s}$）与慢恢复敌方压制 $S_{\text{supp}}$（$\tau_{\text{supp}}=36\text{s}$）双时间尺度分离；<br>4. 掩体梯度正交横切投影 $\mathbf{P}_\perp(\mathbf{d})\nabla G_{\text{safe}}$ + 3×3 Scharr 各向同性核 + CFL 4 子步 RK2 过零阻尼；<br>5. 紧支撑 $C^2$ Wendland $\psi_{3,1}$ 双点核 $K_{\text{reg}}$ 与视差角 Angle T 衰减。 | • 自然涌现出 **32.0s（2 步长）步炮协同黄金突击窗口**；<br>• Scharr 梯度方向误差仅 **$0.385^\circ$**（降低 70.6%）；<br>• 800m 同位测距令 8m 格综合毁伤倍增 **$6.46\times$**，友邻 16m 享 **$4.03\times$**。 |
| **报告四**：`UI-03M4` 军用电台物理调谐算子与二阶临界阻尼迟滞重寻思稳定性 | 1. 电台简化为无脑开关，缺乏波段/地形/频偏/天线方位角与敌方测向暴露的真实博弈；<br>2. 电磁边缘地带因 $\eta$ 轻微抖动引发全军每隔十几秒“取消队列 $\rightarrow$ 发呆 $\rightarrow$ 重建”的帕金森抽搐（Zeno 现象）。 | 1. 无开方主刃搜索算子 $\xi_k = \frac{h_k|h_k|}{(s_k-s_a)(s_b-s_k)}$ + 3 刃 Deygout 衍射；<br>2. ITU-R P.526 三阶有理分式逼近 $J_{\text{rat}}(\nu)$ 与 $T_{\text{diff}}(\nu)$（零超越函数）；<br>3. 向量内积升余弦心形方向图与洛伦兹谐振曲线 $L(\Delta f)$；<br>4. 非对称二阶临界阻尼（$\zeta=1$）Exact ZOH 滤波器 + 施密特双阈值（$\Theta_{\text{cancel}}=3.5, \Theta_{\text{commit}}=1.8$）+ 最小驻留步长 $N_{\text{dwell}}=4$。 | • 全区间衍射逼近最大误差仅 **$0.1391\text{ dB}$**，内循环零 `sqrt`/`log10`/`atan2`；<br>• 奈奎斯特抖动抑制提升 **$7.3\times$（$-32.9\text{ dB}$）**；<br>• 120 步边缘测试（含 2 步深黑障）误取消从 **35 次降至 0 次**（仅真实改令触发 1 次）。 |

---

## 二 · 参谋部深度挑刺：必须动刀裁剪与修正的三大工程陷阱

尽管理论推导非常漂亮，但如果我们照搬报告中的所有式子直接写进五万实例的 Rust 内核，会在**跨平台位级确定性**、**内存容量墙**与**存档持久性**上踩中三颗大雷。以下是我的强制修正意见：

### 1. 第一刀（针对报告一第 3 节）：砍掉浮点 Sinkhorn 矩阵迭代，全面采用“档位 B：多容量最小堆确定性拍卖”！
- **隐患剖析**：
  报告一为了求取 Kantorovich 对偶势 $\tilde{v}_n^*$，设计了 10~25 步的熵正则 Sinkhorn 迭代 $u^{(l+1)} = \bar{q} \oslash (K v^{(l)})$，其中核矩阵 $K_{mn} = M_{mn}\exp(-c_{mn}/\varepsilon)$，最后还要取对数 $\tilde{v}_n^* = \varepsilon \ln v_n^{(L)}$。
  在 C/Rust 工程中，不同 CPU 微架构或编译器的 `expf` / `logf` 实现存在 1 ULP 的尾数差异；当两个班组距离相近时，对偶势的微小浮点漂移会在硬化投影阶段产生截然不同的目标分配，直接摧毁回放与联机的 `state_fingerprint`！
- **参谋部裁决**：
  观察报告一自己的基准测试：当活跃任务格 $N_{\text{act}} \le 64$、单次响应下属班组数 $M \le 512$ 时，报告中的**“档位 B（多容量最小堆确定性拍卖算子）”**不仅不需要调任何超越函数（纯四则运算比较距离平方与对偶价格 $p_n$），而且与精确解的最优性间隙仅有 `+0.057%`，耗时不到 `1ms`！
  因此，内核实装时**一律采用纯代数/定点化的多容量最小堆拍卖算子（以 `(cost_fixed_i64, unit_id, cell_idx)` 字典序决胜平局）**，彻底舍弃易产生浮点分叉的指数/对数 Sinkhorn 预热！

---

### 2. 第二刀（针对报告四第 2 节）：严禁为每个下属维护全图二维滤波场，必须降维至“稀疏指令簇模态权重向量”滤波！
- **隐患剖析**：
  报告四第 2.2 节写出的二阶临界阻尼 ZOH 方程：
  $$\hat{G}_{\text{local}}(t_k + \Delta t) = \gamma_{\text{leak}}(\eta_k)\Big[\mu(\eta_k)G_0(t_k) + e^{-\phi_k}\big((1+\phi_k)e_k + \Delta t \cdot V_{\text{local}}(t_k)\big)\Big]$$
  如果 literally 让全图 $10,000$ 个下属单位各自存一张 $1024 \times 1024$ 的二维网格场 $\hat{G}_{\text{local}}(x, y)$ 和变化率场 $V_{\text{local}}(x, y)$，光这两项就要吃掉 $10000 \times 2 \times 4\text{ MB} = 80\text{ GB}$ 内存，直接爆掉你的机器！
- **参谋部裁决（线性算子可交换律降维）**：
  注意到报告一中，上级指挥官下达的任务场 $G_0(x, y, t) = \sum_{d=1}^{D} w_d(t) \cdot \Psi_d(x, y)$ 是由有限个（通常每个指挥官同时激活的指令/AO 数量 $D \le 16$）固定空间基底 $\Psi_d(x, y)$ 线性张成的！
  由于二阶 ZOH 滤波器关于空间 $(x, y)$ 是线性时不变（或仅依赖于标量 $\eta_k$）的，**对整张二维场做滤波，在数学上 100% 等价于仅对该下属接收到的 $D$ 个指令标量权重 $(w_1, \dots, w_D)$ 做二阶 ZOH 滤波**！
  - 每个下属单位仅需在自身状态中存储一个长度为 $D \le 16$ 的小数组 `local_directive_weights: [(f32, f32); 16]`（每个指令存 `(w_hat, v_hat)`，仅占 **128 字节**！）；
  - 10,000 个单位总共只占 **1.28 MB** 内存，全部常驻 CPU L2 Cache，单步更新耗时不到 `0.05 ms`，而失配泛函 $\Delta \mathcal{J}(t_k)$ 与施密特迟滞定理 1 的数学性质 **100% 严丝合缝保留**！

---

### 3. 第三刀（针对报告二第 4 节）：被打断的“半截土方工时”与“在途预扣状态”的归属边界
- **隐患剖析**：
  报告二在炮火打断状态机中设计了一个极佳的细节：当焦点虚拟实例 $z_{j^*}$ 处于原位施工半截（$\phi_{j^*} \in (0, 1)$）被打断时，地雷/炸药类销毁退回，而散兵坑/堑壕/碉堡类将已累积工时 $W_{\text{saved}} = \phi_{j^*} W_0$ 缓存在 $8\text{m}$ 网格上供后续复工继承。
- **参谋部裁决**：
  必须明确：**任何跨步长存活且影响后续物理结算的数据，绝不能只放在瞬态 `TickScratch` 或易失的纯预览层**！
  - 已被推进转正的前半截实体 $\{z_1, \dots, z_{j^*-1}\}$，以及被打断在半截的土方累积工时 $W_{\text{saved}}(r, c)$，在打断发生的那个 16s 步长 Stage 10，必须写入 **`WorldState`（权威状态）** 并纳入 `state_fingerprint`；
  - 未触及的后半截虚拟实例 $\{z_{j^*+1}, \dots, z_K\}$ 则随原队列取消从 `DerivedState` 中干净抹除。

---

## 三 · 可直接焊入 `gentactix-core` 的四大黄金算子速查表

经过上述三刀修剪后，以下四组核心闭式公式可直接由施工官（`gentactix-builder`）翻译为 Rust 代码：

### 1. 零超越函数刀刃衍射与电台乘数算子（源自报告四，用于 `UI-03M4`）
- **DDA 剖面主刃无开方搜索**：
  $$\xi_k(a, b) = \frac{h_k(a, b) \cdot |h_k(a, b)|}{(s_k - s_a)(s_b - s_k)}, \quad k^* = \arg\max_{a < k < b} \xi_k(a, b)$$
- **三阶有理衍射透过率（令 $u = \max(0, \nu + 0.78)$）**：
  $$T_{\text{diff}}(\nu) = \frac{1 + 0.2512 u}{1 + 1.5048 u + 5.9201 u^3} \in (0, 1]$$
- **向量内积心形定向天线增益（零反三角函数）**：
  $$G_{\text{ant}}(\theta - \psi) = G_{\text{back}} + (G_{\text{max}} - G_{\text{back}})\left(\frac{1 + \hat{d}_{\text{tr}} \cdot \hat{u}(\psi)}{2}\right)^2$$
- **洛伦兹频偏谐振与综合传导乘数**：
  $$L(\Delta f) = \frac{1}{1 + (\Delta f / B_{1/2})^2}, \quad \eta = \max\left(\eta_{\text{wire}},\; 1.2 \cdot \frac{\Gamma_{\text{radio}}^2}{\Gamma_{1/2}^2 + \Gamma_{\text{radio}}^2}\right) \in [0, 1.2]$$

### 2. 双时间尺度爆炸危险与源项压制坍塌算子（源自报告三，用于 Stage 8~9）
- **落点瞬态爆炸危险（$\tau_{\text{blast}} = 9.0\text{s} \implies \alpha_{\text{blast}} = 0.1690$）**：
  $$H_{\text{blast}}(x, y, t + 16\text{s}) = 1 - \big(1 - 0.1690 \cdot H_{\text{blast}}(x, y, t)\big)\exp(-E_{\text{impact}}(x, y))$$
- **敌军压制密度与施密特三态源强坍塌（$\tau_{\text{supp}} = 36.0\text{s} \implies \alpha_{\text{supp}} = 0.6412$）**：
  $$S_{\text{supp}}(t + 16\text{s}) = 0.6412 \cdot S_{\text{supp}}(t) + (1 - \kappa_{\text{cover}} G_{\text{safe}}) \Delta S_{\text{in}}$$
  $$\Phi_{\text{supp}}(S_{\text{supp}}, \text{State}) = \frac{\zeta(\text{State})}{1 + S_{\text{supp}}^3}, \quad \zeta \in \{1.00\text{ (Active)},\; 0.35\text{ (Pinned)},\; 0.02\text{ (Prone)}\}$$

### 3. 掩体正交横切投影与 $\lambda$ 滑块流线合成算子（源自报告三，用于 `UI-03M1`）
- **消除掩体后向拉力锁定的正交投影**：
  $$\mathbf{P}_\perp(\mathbf{d})\nabla G_{\text{safe}} = \nabla G_{\text{safe}} - (\nabla G_{\text{safe}} \cdot \mathbf{d})\mathbf{d}$$
- **移动任务（$\mu_{\text{rout}} = 0$，受滑块 $\lambda \in [0, 1]$ 控制）与直接跑路（$\mu_{\text{rout}} = 1$）统一方向场**：
  $$\varphi(\lambda) = (1 - \lambda)^{1.25}, \quad \mathbf{v}_{\text{move}} = \text{Normalize}\Big(\mathbf{d}_{\text{goal}} + \varphi(\lambda)\big(-w_H \nabla H_{\text{blast}} - w_I \nabla I_{\text{threat}} + w_{\text{cov}}\mathbf{P}_\perp(\mathbf{d}_{\text{goal}})\nabla G_{\text{safe}} + \mathbf{F}_{\text{skirt}}\big)\Big)$$
  $$\mathbf{v}_{\text{rout}} = \text{Normalize}\big(-3.5\nabla H_{\text{blast}} - 2.0\nabla I_{\text{threat}} + 1.8\nabla G_{\text{safe}}\big) \quad (\text{配 4 子步 RK2 过零阻尼截断})$$

### 4. Wendland $C^2$ 双点测距标定核（源自报告三，用于 `UI-03M2` 测距）
- **紧支撑径向基函数 $\psi_{3,1}(r) = (1 - r)_+^4 (4r + 1)$**：
  $$K_{\text{reg}}(x_s, P_{\text{obs}}; x_a, P_{\text{tgt}}) = \psi_{3,1}\!\left(\frac{\|x_s - P_{\text{obs}}\|}{96\text{m}}\right) \cdot \psi_{3,1}\!\left(\frac{\|x_a - P_{\text{tgt}}\|}{64\text{m}}\right) \cdot \exp\!\left(-\left(\frac{\theta_T}{35^\circ}\right)^2\right)$$
  直接驱动目标散布椭圆半轴收缩并向 $P_{\text{tgt}}$ 邻域注入 $\Delta J_{\text{reg}} = 1.05 \cdot K_{\text{reg}}$ 拉高 $I_{\text{intel}}$。

---

## 四 · 下一步整合路线建议

1. **第一优先级（当前正在进行的 UI 与基础闭环）**：
   先让隔壁的 `3.8 Flash` 按已下发的 V4.0 `plan.txt` 把前端全屏图台、`UI-03` 多开拖拽卡片、`UI-03M1~M4` 面板以及基础双场正负号逻辑跑通并交付；
2. **第二优先级（数学算子精炼升级包 V4.1）**：
   待前端骨架验收合入后，我们将上述裁剪定型后的**四大黄金闭式算子（Cut-Cell 九点核、增广弧长 $\tilde{s}$ 边走边干积分器、双时间尺度 $H_{\text{blast}}/S_{\text{supp}}$ + 正交投影流线、零超越函数 Deygout 电台算子 + 模态向量二阶 ZOH 施密特迟滞）**打包为纯后端数学算子升级任务，一次性替换 `gentactix-core` 中的启发式近似公式。
