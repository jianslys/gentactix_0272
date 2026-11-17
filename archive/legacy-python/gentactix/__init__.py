# -*- coding: utf-8 -*-
"""Gentactix V3 六分架构核心推演包。

模块分层拓扑（六分架构）：
1. 像素容器层：terrain_field（栅格地形、烟/照、暴露度场、CellStash 唯一真源）
2. 单位编制层：toe（树形编制至班、细化装备口径、Commander 实体与顺位继承）
3. 纯 AO 指挥层：command_v2（几何原语光栅化、AO 注册/配额/延迟、链路与分流 Buff）
4. 后勤补给层：logistics（道路网、末端搬运、口径校验、守恒校验）
5. 回合判定层：combat（伤害矩阵、ExposureTracker）、artillery、ww2_air、recon_fog、adjudicator_v2
6. 事件总线层：events（作战会议等跨单位协同事件）
"""

__version__ = "3.0.0"
