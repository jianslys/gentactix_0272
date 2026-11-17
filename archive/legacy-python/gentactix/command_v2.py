# -*- coding: utf-8 -*-
"""指挥层（V3 / 六分架构）：纯 AO 几何原语光栅化、注册配额、链路连通与分流 Buff 数值。

设计原则
--------
- 独立指挥官实体（Commander）与顺位继承（resolve_succession）已归位至 toe.py；
- 作战会议（War Council）等跨单位事件已归位至 events.py；
- 本模块只处理“作战区域（AO）几何、注册/配额/延迟激活、指挥链路连通与各级 AO 数值”。
"""

from __future__ import annotations

import math

from .toe import (
    Commander,
    DEMOTION_NEXT,
    PERSONAL_RADIUS_KM,
    RADIO_CHANNELS,
    RANK_ORDER,
    RANK_SPECS,
    _piece_cadre_count,
    _piece_has_radio,
    resolve_succession,
)

TACTICAL_AO_ECHELONS = ("platoon", "company")
BATTALION_AO_ECHELONS = ("battalion",)
STRATEGIC_AO_ECHELONS = ("regiment", "brigade", "division")
OPERATIONAL_AO_ECHELONS = BATTALION_AO_ECHELONS + STRATEGIC_AO_ECHELONS


# ==========================================
# 📐 200m/px AO 几何原语光栅化器
# ==========================================
def rasterize_circle(center_px, radius_km, cell_m=200.0, shape=None):
    """按圆心与公里半径生成像素集合。"""
    cr, cc = int(center_px[0]), int(center_px[1])
    r_px = max(0.0, float(radius_km) * 1000.0 / float(cell_m))
    lim = int(math.ceil(r_px))
    cells = set()
    for dr in range(-lim, lim + 1):
        for dc in range(-lim, lim + 1):
            if dr * dr + dc * dc <= r_px * r_px + 1e-9:
                rr, cc2 = cr + dr, cc + dc
                if shape is None or (0 <= rr < shape[0] and 0 <= cc2 < shape[1]):
                    cells.add((rr, cc2))
    return cells


def rasterize_box(p1_px, p2_px, shape=None):
    """按对角像素生成矩形区域像素集合。"""
    r0, r1 = sorted((int(p1_px[0]), int(p2_px[0])))
    c0, c1 = sorted((int(p1_px[1]), int(p2_px[1])))
    cells = set()
    for r in range(r0, r1 + 1):
        for c in range(c0, c1 + 1):
            if shape is None or (0 <= r < shape[0] and 0 <= c < shape[1]):
                cells.add((r, c))
    return cells


def rasterize_corridor(p1_px, p2_px, width_km, cell_m=200.0, shape=None):
    """按起止像素与走廊总宽度（km）生成斜向走廊像素集合。"""
    r1, c1 = float(p1_px[0]), float(p1_px[1])
    r2, c2 = float(p2_px[0]), float(p2_px[1])
    half_px = max(0.5, (float(width_km) * 1000.0 / float(cell_m)) * 0.5)
    dr, dc = r2 - r1, c2 - c1
    seg_len2 = dr * dr + dc * dc
    min_r = int(math.floor(min(r1, r2) - half_px))
    max_r = int(math.ceil(max(r1, r2) + half_px))
    min_c = int(math.floor(min(c1, c2) - half_px))
    max_c = int(math.ceil(max(c1, c2) + half_px))
    cells = set()
    for r in range(min_r, max_r + 1):
        for c in range(min_c, max_c + 1):
            if shape is not None and not (0 <= r < shape[0] and 0 <= c < shape[1]):
                continue
            if seg_len2 <= 1e-9:
                d2 = (r - r1) ** 2 + (c - c1) ** 2
            else:
                t = max(0.0, min(1.0, ((r - r1) * dr + (c - c1) * dc) / seg_len2))
                pr, pc = r1 + t * dr, c1 + t * dc
                d2 = (r - pr) ** 2 + (c - pc) ** 2
            if d2 <= half_px * half_px + 1e-9:
                cells.add((r, c))
    return cells


def build_ao_cells(spec, grid=None, pieces=None):
    """统一解析 AO 几何描述：支持显式 cells、circle、box、corridor、anchor_unit。"""
    cell_m = grid.cell_m if grid is not None else 200.0
    shape = grid.shape if grid is not None else None
    if isinstance(spec, (list, set, tuple)) and (not spec or isinstance(next(iter(spec)), (list, tuple))):
        return {(int(r), int(c)) for r, c in spec}
    if not isinstance(spec, dict):
        return set()
    if "cells" in spec:
        return {(int(r), int(c)) for r, c in spec["cells"]}
    kind = spec.get("shape", "circle")
    if kind == "circle":
        return rasterize_circle(spec["center"], spec.get("radius_km", 0.6), cell_m=cell_m, shape=shape)
    if kind == "box":
        return rasterize_box(spec["p1"], spec["p2"], shape=shape)
    if kind == "corridor":
        return rasterize_corridor(spec["p1"], spec["p2"], spec.get("width_km", 0.6), cell_m=cell_m, shape=shape)
    if kind == "anchor" and pieces is not None and spec.get("unit_id") in pieces:
        return rasterize_circle(pieces[spec["unit_id"]].px, spec.get("radius_km", 0.6), cell_m=cell_m, shape=shape)
    return set()


# ==========================================
# 🎯 纯 AO 区域与 Buff 力学
# ==========================================
class OperationalZone:
    """指挥官划定的作战区域（AO），支持回合延迟生效与分兵种战线绑定。"""

    def __init__(self, zone_id, commander_id, team, echelon, cells, activate_turn=0, frontlines=None):
        self.zone_id = zone_id
        self.commander_id = commander_id
        self.team = team
        self.echelon = echelon
        self.cells = {(int(r), int(c)) for r, c in cells}
        self.activate_turn = int(activate_turn)
        self.frontlines = [dict(fl) for fl in (frontlines or [])]

    @property
    def area_px(self):
        return len(self.cells)

    def to_dict(self):
        return dict(
            zone_id=self.zone_id,
            commander_id=self.commander_id,
            team=self.team,
            echelon=self.echelon,
            cells=[list(rc) for rc in sorted(self.cells)],
            activate_turn=self.activate_turn,
            frontlines=list(self.frontlines),
        )

    @classmethod
    def from_dict(cls, d):
        return cls(
            d["zone_id"],
            d["commander_id"],
            d["team"],
            d["echelon"],
            d["cells"],
            activate_turn=d.get("activate_turn", 0),
            frontlines=d.get("frontlines"),
        )


def register_zone(zones, zone, commanders, current_turn=0, pieces=None, grid=None):
    """注册作战区域：校验职级区块/总面积配额 + 同层级严禁重叠，写入延迟生效回合及地形容器通道。"""
    cmdr = commanders.get(zone.commander_id)
    if cmdr is None or not cmdr.alive:
        return False, "commander_unavailable"
    if zone.area_px == 0:
        return False, "empty_zone"

    for zid, existing in zones.items():
        if zid == zone.zone_id:
            continue
        if existing.team == zone.team and existing.echelon == zone.echelon:
            if existing.cells & zone.cells:
                return False, "same_echelon_overlap"

    mine = [z for zid, z in zones.items() if z.commander_id == cmdr.id and zid != zone.zone_id]
    if len(mine) + 1 > cmdr.max_zones:
        return False, "exceeds_max_zones"
    total_area = sum(z.area_px for z in mine) + zone.area_px
    max_area = effective_commander_max_area_px(cmdr, pieces, grid)
    if total_area > max_area:
        return False, "exceeds_max_area"

    zone.activate_turn = max(zone.activate_turn, int(current_turn) + cmdr.effective_delay_turns())
    zones[zone.zone_id] = zone
    if grid is not None and hasattr(grid, "write_ao_to_container"):
        grid.write_ao_to_container(zone.zone_id, zone.team, zone.cells, commander_id=zone.commander_id)
        for idx, fl in enumerate(zone.frontlines or ()):
            pts = fl.get("points_px") or fl.get("points") or []
            if pts:
                grid.write_typed_frontline(
                    fl.get("line_id") or f"{zone.zone_id}:fl:{idx}",
                    zone.team,
                    pts,
                    unit_type_mask=fl.get("unit_type_mask", 1),
                    attraction_weight=fl.get("attraction_weight", 1.0),
                    front_normal=fl.get("front_normal"),
                    commander_id=zone.commander_id,
                    zone_id=zone.zone_id,
                    spread_radius_px=fl.get("spread_radius_px", 4),
                )
    return True, "ok"


def enforce_zone_quotas(zones, commanders, pieces=None, grid=None):
    """顺位继承或无线电静默/丢失电台导致配额缩水时，自动裁撤超额作战区域。"""
    removed = []
    by_cmdr = {}
    for zid, z in list(zones.items()):
        by_cmdr.setdefault(z.commander_id, []).append(z)
    for cid, zlist in by_cmdr.items():
        cmdr = commanders.get(cid)
        if cmdr is None or not cmdr.alive:
            for z in zlist:
                zones.pop(z.zone_id, None)
                removed.append(z.zone_id)
                if grid is not None and hasattr(grid, "remove_ao_from_container"):
                    grid.remove_ao_from_container(z.zone_id, team=z.team)
            continue
        max_area = effective_commander_max_area_px(cmdr, pieces, grid)
        kept_area = 0
        kept_count = 0
        for z in zlist:
            if kept_count + 1 <= cmdr.max_zones and kept_area + z.area_px <= max_area:
                kept_count += 1
                kept_area += z.area_px
            else:
                zones.pop(z.zone_id, None)
                removed.append(z.zone_id)
                if grid is not None and hasattr(grid, "remove_ao_from_container"):
                    grid.remove_ao_from_container(z.zone_id, team=z.team)
    return removed


def zone_buff_value(zone, cmdr):
    """单个作战区域的集中度 Buff：面积越小越接近满额，越大越稀释。"""
    if zone.area_px <= 0:
        return 0.0
    if zone.area_px <= cmdr.ref_area_px:
        return cmdr.max_buff
    return round(cmdr.max_buff * math.sqrt(cmdr.ref_area_px / float(zone.area_px)), 4)


def _grid_cell_has_radio(grid, px, team):
    """检查指定像素格的 CellStash 上是否插有己方实体电台，返回 (has_port, has_hq_radio, channels)。"""
    if grid is None or not hasattr(grid, "cell_stash"):
        return False, False, set()
    st = grid.cell_stash.get((int(px[0]), int(px[1])))
    if not st:
        return False, False, set()
    owner = st.get("owner_team")
    if owner is not None and owner != team:
        return False, False, set()
    eq = st.get("equipment") or {}
    has_port = eq.get("radio", 0) > 0
    has_hq = eq.get("hq_radio", 0) > 0
    ch = st.get("radio_channel", "A")
    return has_port, has_hq, ({ch} if (has_port or has_hq) else set())


def active_comm_gear(cmdr, pieces=None, grid=None):
    """扫描指挥官 400m 肉身圈内可触及的移动电台（指挥部/通信班）与地上固定电台（CellStash）。"""
    has_port = False
    has_hq = False
    in_hq = False
    has_wire = bool(grid is not None and hasattr(grid, "has_wire_at") and grid.has_wire_at(cmdr.px, cmdr.team))
    channels = {cmdr.radio_channel}

    if pieces is not None and grid is not None:
        for p in pieces.values():
            if p.team != cmdr.team or p.persons() <= 0:
                continue
            if grid.distance_km(cmdr.px, p.px) <= PERSONAL_RADIUS_KM + 1e-9:
                if p.id == cmdr.cadre_piece_id or p.has_buff("command") or p.has_buff("signal"):
                    in_hq = True
                for sq in p.squads():
                    if sq.equipment.get("radio", 0) > 0:
                        has_port = True
                        channels.add(p.radio_channel)
                    if sq.equipment.get("hq_radio", 0) > 0:
                        has_hq = True
                        channels.add(p.radio_channel)
                    if sq.equipment.get("field_wire", 0) > 0:
                        has_wire = True

    if grid is not None and hasattr(grid, "cell_stash"):
        for s_px in list(grid.cell_stash):
            if grid.distance_km(cmdr.px, s_px) <= PERSONAL_RADIUS_KM + 1e-9:
                gp, gh, gch = _grid_cell_has_radio(grid, s_px, cmdr.team)
                has_port = has_port or gp
                has_hq = has_hq or gh
                channels.update(gch)
                st = grid.cell_stash.get(s_px, {})
                if (st.get("owner_team") in (None, cmdr.team)) and (st.get("equipment") or {}).get("field_wire", 0) > 0:
                    has_wire = True

    return {
        "in_hq": in_hq,
        "has_radio": has_port or has_hq,
        "has_hq_radio": has_hq,
        "has_wire": has_wire,
        "channels": channels,
    }


def non_radio_radius_km(cmdr, pieces=None, grid=None):
    """纯肉身+在位指挥部非无线电半径（不依赖电台，也不含电缆拓扑直连）。"""
    if pieces is None or grid is None:
        r = cmdr.effective_radius_km()
        return min(r, 0.8) if cmdr.radio_silence else r
    gear = active_comm_gear(cmdr, pieces, grid)
    r = PERSONAL_RADIUS_KM
    if gear["in_hq"]:
        r += {"company": 0.4, "battalion": 1.2, "regiment": 2.0, "brigade": 2.6, "division": 3.2}.get(cmdr.echelon, 0.4)
    if cmdr.briefing_buff_hours > 1e-9:
        r *= 1.35
    if cmdr.shock_hours > 1e-9:
        r *= 0.75
    return r


def effective_commander_radius_km(cmdr, pieces=None, grid=None):
    """计算指挥官真实最大指挥半径：无电台且离部一律 400m；静默按 35% 缩水(保底 0.8km)；开电台解锁远程。"""
    if pieces is None or grid is None:
        return cmdr.effective_radius_km()
    gear = active_comm_gear(cmdr, pieces, grid)
    non_radio_km = non_radio_radius_km(cmdr, pieces, grid)
    if not gear["has_radio"]:
        return non_radio_km
    if gear["has_hq_radio"]:
        base_km = cmdr.radius_km
    else:
        base_km = min(cmdr.radius_km, max(non_radio_km, 2.5))
    if cmdr.briefing_buff_hours > 1e-9:
        base_km *= 1.35
    if cmdr.shock_hours > 1e-9:
        base_km *= 0.75
    if cmdr.radio_silence:
        base_km = max(non_radio_km, min(base_km, max(0.8, base_km * 0.35)))
    return base_km


def effective_commander_max_area_px(cmdr, pieces=None, grid=None):
    """计算指挥官真实 AO 面积配额：无电台且离部缩至个人极限；静默缩水 50%。"""
    if pieces is None or grid is None:
        return cmdr.effective_max_area_px()
    gear = active_comm_gear(cmdr, pieces, grid)
    if not gear["has_radio"] and not gear["in_hq"] and not gear["has_wire"]:
        return max(40, int(round(cmdr.max_area_px * 0.15)))
    return cmdr.effective_max_area_px()


def validate_command_link(cmdr, piece, grid, pieces=None):
    """判定单位是否处于指挥官的有效指挥范围内：
    1) 肉身/非无线电半径；2) 完好有线电缆直连（无视静默与信道）；3) 同信道实体电台无线通联（静默缩水半径而非瞬间断绝）。
    """
    if cmdr is None or not cmdr.alive:
        return False, "commander_dead"
    if cmdr.team != piece.team:
        return False, "wrong_team"
    if cmdr.subordinate_ids and piece.id not in cmdr.subordinate_ids:
        return False, "not_subordinate"
    d_km = grid.distance_km(cmdr.px, piece.px)
    phys_r = non_radio_radius_km(cmdr, pieces, grid)
    if d_km <= phys_r:
        return True, "physical_radius"
    if hasattr(grid, "wire_connected") and grid.wire_connected(cmdr.px, piece.px, cmdr.team):
        return True, "wire_link"
    # 无线电通联检查（移动电台或地上插着的电台 + ABCDEFG 同信道 + 静默缩小有效电台半径）
    if pieces is not None:
        gear = active_comm_gear(cmdr, pieces, grid)
        cmdr_radio_ok = gear["has_radio"]
        cmdr_channels = gear["channels"]
    else:
        cmdr_radio_ok = cmdr.has_radio
        cmdr_channels = {cmdr.radio_channel}
    gp, gh, gch = _grid_cell_has_radio(grid, piece.px, piece.team)
    piece_radio_ok = _piece_has_radio(piece) or gp or gh
    if not (cmdr_radio_ok and piece_radio_ok):
        return False, "out_of_command_range"
    if piece.radio_channel not in cmdr_channels and not (gch & cmdr_channels):
        return False, "channel_mismatch"
    radio_r = effective_commander_radius_km(cmdr, pieces, grid)
    if getattr(piece, "radio_silence", False) and not cmdr.radio_silence:
        radio_r = max(phys_r, min(radio_r, max(0.8, radio_r * 0.35)))
    if pieces is not None and d_km > radio_r:
        return False, "radio_silence" if (cmdr.radio_silence or getattr(piece, "radio_silence", False)) else "out_of_command_range"
    if cmdr.radio_silence or getattr(piece, "radio_silence", False):
        return True, "radio_link_silenced"
    return True, "radio_link"


def validate_commander_link(sup_cmdr, sub_cmdr, grid, pieces=None):
    """判定下级指挥官是否处于上级指挥官有效指挥通联内（支持肉身半径/有线电话/同信道电台及静默缩水）。"""
    if sup_cmdr is None or not sup_cmdr.alive:
        return False, "superior_dead"
    if sub_cmdr is None or not sub_cmdr.alive:
        return False, "subordinate_dead"
    if sup_cmdr.team != sub_cmdr.team:
        return False, "wrong_team"
    if sup_cmdr.subordinate_ids and sub_cmdr.id not in sup_cmdr.subordinate_ids:
        return False, "not_subordinate"
    d_km = grid.distance_km(sup_cmdr.px, sub_cmdr.px)
    phys_r = non_radio_radius_km(sup_cmdr, pieces, grid)
    if d_km <= phys_r:
        return True, "physical_radius"
    if hasattr(grid, "wire_connected") and grid.wire_connected(sup_cmdr.px, sub_cmdr.px, sup_cmdr.team):
        return True, "wire_link"
    if pieces is not None:
        sup_gear = active_comm_gear(sup_cmdr, pieces, grid)
        sub_gear = active_comm_gear(sub_cmdr, pieces, grid)
        sup_ok, sup_chs = sup_gear["has_radio"], sup_gear["channels"]
        sub_ok, sub_chs = sub_gear["has_radio"], sub_gear["channels"]
    else:
        sup_ok, sup_chs = sup_cmdr.has_radio, {sup_cmdr.radio_channel}
        sub_ok, sub_chs = sub_cmdr.has_radio, {sub_cmdr.radio_channel}
    if not (sup_ok and sub_ok):
        return False, "out_of_command_range"
    if not (sup_chs & sub_chs):
        return False, "channel_mismatch"
    radio_r = effective_commander_radius_km(sup_cmdr, pieces, grid)
    if sub_cmdr.radio_silence and not sup_cmdr.radio_silence:
        radio_r = max(phys_r, min(radio_r, max(0.8, radio_r * 0.35)))
    if d_km > radio_r:
        return False, "radio_silence" if (sup_cmdr.radio_silence or sub_cmdr.radio_silence) else "out_of_command_range"
    if sup_cmdr.radio_silence or sub_cmdr.radio_silence:
        return True, "radio_link_silenced"
    return True, "radio_link"


def direct_commander_for(piece, commanders):
    """查找管理该班组的直属最低层级指挥官（优先遵守 direct_commander_id 单线直属）。"""
    if not commanders:
        return None
    explicit_id = (getattr(piece, "meta", None) or {}).get("direct_commander_id")
    if explicit_id and explicit_id in commanders:
        ec = commanders[explicit_id]
        if ec.team == piece.team:
            return ec
    candidates = [
        c for c in commanders.values()
        if c.team == piece.team and piece.id in c.subordinate_ids
    ]
    if not candidates:
        return None
    candidates.sort(key=lambda c: -RANK_ORDER.index(c.rank))
    return candidates[0]


def is_piece_chain_connected(piece, target_cmdr, commanders, grid, pieces=None):
    """逐级指挥链连通校验：若与直属排/连级军官断连，则即便身处连/排 AO 内也吃不到任何 Buff。"""
    direct = direct_commander_for(piece, commanders)
    if direct is not None:
        ok_direct, reason_direct = validate_command_link(direct, piece, grid, pieces=pieces)
        if not ok_direct:
            return False, f"disconnected_direct:{reason_direct}"
    return validate_command_link(target_cmdr, piece, grid, pieces=pieces)


def compute_ao_buff(piece, zones, commanders, grid, current_turn=None, pieces=None):
    """计算一线班组当前吃到的战术 AO Buff（仅限排/连级；断连归零；同级互斥、跨级可叠）。"""
    by_echelon = {}
    for z in zones.values():
        if current_turn is not None and z.activate_turn > int(current_turn):
            continue
        if z.team != piece.team or z.echelon not in TACTICAL_AO_ECHELONS or piece.px not in z.cells:
            continue
        cmdr = commanders.get(z.commander_id)
        ok, _ = is_piece_chain_connected(piece, cmdr, commanders, grid, pieces=pieces)
        if not ok:
            continue
        val = zone_buff_value(z, cmdr)
        if val > by_echelon.get(z.echelon, 0.0):
            by_echelon[z.echelon] = val
    return {"total": round(sum(by_echelon.values()), 4), "by_echelon": by_echelon}


def compute_operational_ao_buff(target_px, team, zones, commanders, current_turn=None):
    """计算高级 AO Buff：营级 -> 炮兵精度/侦察迷雾；团/旅/师级 -> 航空兵/战役后勤。"""
    px = (int(target_px[0]), int(target_px[1]))
    by_echelon = {}
    for z in zones.values():
        if current_turn is not None and z.activate_turn > int(current_turn):
            continue
        if z.team != team or z.echelon not in OPERATIONAL_AO_ECHELONS or px not in z.cells:
            continue
        cmdr = commanders.get(z.commander_id)
        if cmdr is None or not cmdr.alive:
            continue
        val = zone_buff_value(z, cmdr)
        if val > by_echelon.get(z.echelon, 0.0):
            by_echelon[z.echelon] = val
    bn_val = sum(v for k, v in by_echelon.items() if k in BATTALION_AO_ECHELONS)
    # 战略级 AO（团/旅/师）同类互斥，取最高值，禁止三层套娃无限叠加
    strat_val = max((v for k, v in by_echelon.items() if k in STRATEGIC_AO_ECHELONS), default=0.0)
    total = round(bn_val + strat_val, 4)
    return {
        "total": total,
        "by_echelon": by_echelon,
        "artillery_mult": round(1.0 + bn_val, 4),
        "recon_mult": round(1.0 + bn_val, 4),
        "air_mult": round(1.0 + strat_val, 4),
        "logistics_mult": round(1.0 + strat_val, 4),
    }
