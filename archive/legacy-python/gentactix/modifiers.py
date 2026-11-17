# -*- coding: utf-8 -*-
"""Buff + Debuff 引擎：统一数值乘区与状态修饰器合成（事件 + 战损/故障 Debuff + 动态超载 + 车组缺编）。"""

from __future__ import annotations

import hashlib
import math

from . import locks as lock_mod

MULT_KEYS = ("speed_mult", "fuel_mult", "combat_power_mult", "recon_mult")
ADD_KEYS = ("exposure_delta", "org_delta_per_h")

OVERLOAD_FAULT_POOL = (
    ("fault_axle_snap", dict(forbid_move=True, label="爆胎/断轴趴窝")),
    ("fault_gearbox", dict(speed_mult=0.35, label="变速箱过热降速")),
    ("fault_engine_overheat", dict(speed_mult=0.5, fuel_mult=1.8, exposure_delta=2.0, label="发动机拉缸冒黑烟")),
)


def empty_modifiers():
    out = lock_mod.empty_locks()
    for k in MULT_KEYS:
        out[k] = 1.0
    for k in ADD_KEYS:
        out[k] = 0.0
    return out


def merge_modifiers(*sources):
    out = empty_modifiers()
    lock_list = []
    for src in sources:
        if not src:
            continue
        lock_list.append(src)
        for k in MULT_KEYS:
            if k in src:
                out[k] *= float(src[k])
        for k in ADD_KEYS:
            if k in src:
                out[k] += float(src[k])
    out.update(lock_mod.merge_locks(*lock_list))
    return out


def collect_piece_modifiers(piece, event_manager=None, world=None):
    """汇总某单位当前全部 Buff/Debuff 与状态锁：活动事件 + 战损/故障 Debuff + 软超载 + 车组人数修正。"""
    evs = event_manager.active_for(piece.id) if event_manager is not None else []
    ev_specs = [e.spec for e in evs]

    db = piece.aggregate_debuffs() if hasattr(piece, "aggregate_debuffs") else {}
    pieces_map = world.pieces if world is not None else None
    ov = piece.overload_debuff(pieces_map) if hasattr(piece, "overload_debuff") else {"load_ratio": 0.0}
    vc = piece.vehicle_crew_modifiers(pieces_map) if hasattr(piece, "vehicle_crew_modifiers") else {"single_crew": False}

    cmd_specs = []
    active_debuff_tags = []
    active_buff_tags = []
    kc_mult = 1.0
    tau_mult = 1.0
    shock_m_add = 0.0
    if world is not None and getattr(world, "commanders", None):
        from . import command_v2
        cmdr = command_v2.direct_commander_for(piece, world.commanders)
        if cmdr is not None and cmdr.alive:
            c_meta = getattr(cmdr, "meta", None) or {}
            partner_id = c_meta.get("council_partner_id")
            if partner_id and partner_id in world.commanders:
                partner = world.commanders[partner_id]
                if not partner.alive:
                    cmdr.war_council_sync_hours = 0.0
                elif (not c_meta.get("council_completed")) and getattr(world, "grid", None) is not None:
                    rad_km = float(c_meta.get("council_radius_km", 1.0))
                    if world.grid.distance_km(cmdr.px, partner.px) > rad_km + 1e-6:
                        cmdr.war_council_sync_hours = 0.0

            churn = float(getattr(cmdr, "order_churn", 0.0))
            churn_speed = 1.0
            churn_power = 1.0
            churn_org_dh = 0.0
            if churn > 1.0 + 1e-9:
                s = churn - 1.0
                churn_speed = max(0.55, 1.0 - 0.10 * s)
                churn_power = max(0.60, 1.0 - 0.08 * s)
                churn_org_dh = -1.5 * s
                cmd_specs.append({
                    "speed_mult": churn_speed,
                    "combat_power_mult": churn_power,
                    "org_delta_per_h": churn_org_dh,
                })
                active_debuff_tags.extend(["ORDER_CHURN_DEBUFF", "ORDER_CONFUSION"])
                kc_mult = max(0.30, math.exp(-0.25 * s))
                tau_mult = 1.0 + 0.45 * s
                shock_m_add = min(0.45, 0.08 * s)

            if float(getattr(cmdr, "war_council_sync_hours", 0.0)) > 1e-9:
                # 面对面开会不删除 ORDER_CHURN_DEBUFF，而是挂正向 WAR_COUNCIL_SYNC_BUFF 在最终汇总数值上拉平甚至净正
                comp_speed = (1.0 / churn_speed) * 1.08
                comp_power = (1.0 / churn_power) * 1.10
                comp_org_dh = (-churn_org_dh + 1.0) if churn_org_dh < 0.0 else 1.0
                cmd_specs.append({
                    "speed_mult": comp_speed,
                    "combat_power_mult": comp_power,
                    "org_delta_per_h": comp_org_dh,
                })
                active_buff_tags.append("WAR_COUNCIL_SYNC_BUFF")
                kc_mult = 1.15
                tau_mult = 0.75
                shock_m_add = 0.0

    merged = merge_modifiers(*ev_specs, db, ov, vc, *cmd_specs)

    busy_persons = sum(e.labor_persons for e in evs)
    operate_eff = 1.0
    operating_target = None
    for e in evs:
        if e.kind == "operate_heavy_unit":
            operate_eff = min(operate_eff, float(e.payload.get("operator_eff", 1.0)))
            operating_target = e.payload.get("target_unit_id")

    tot_p = max(1, piece.persons())
    labor_mult = max(0.05, (tot_p - min(tot_p, busy_persons)) / float(tot_p)) if busy_persons > 0 else 1.0
    merged["speed_mult"] = round(merged["speed_mult"], 4)
    merged["combat_power_mult"] = round(merged["combat_power_mult"] * labor_mult, 4)
    merged["recon_mult"] = round(merged["recon_mult"], 4)
    merged["labor_persons"] = busy_persons
    merged["operator_eff"] = operate_eff
    merged["operating_target"] = operating_target
    merged["single_crew"] = bool(vc.get("single_crew", False))
    merged["load_ratio"] = float(ov.get("load_ratio", 0.0))
    merged["active_debuff_tags"] = active_debuff_tags
    merged["active_buff_tags"] = active_buff_tags
    merged["kc_mult"] = round(kc_mult, 4)
    merged["tau_mult"] = round(tau_mult, 4)
    merged["shock_m_add"] = round(shock_m_add, 4)
    return merged


def tick_overload_faults(world, dt_hours):
    """按 5 分钟 (1/12h) 最小推演切片对超载强行机动的载具掷骰，保持与 15 分钟切片数学期望完全一致。"""
    dt = max(0.0, float(dt_hours))
    if dt <= 1e-9:
        return []
    step_h = 5.0 / 60.0
    ticks = max(1, int(round(dt / step_h)))
    actual_step_h = dt / ticks
    faults = []
    for p in world.pieces.values():
        if not getattr(p, "vehicle_model", None) or not p.moved_last_turn:
            continue
        ov = p.overload_debuff(world.pieces)
        ratio = ov.get("load_ratio", 0.0)
        if ratio <= 1.0:
            continue
        p_15m = min(0.85, 0.18 * (ratio - 1.0))
        prob_step = 1.0 - (1.0 - p_15m) ** (actual_step_h / 0.25)
        for t in range(ticks):
            h = hashlib.sha1(f"{world.turn}|{p.id}|{t}|{ratio:.3f}".encode("utf-8")).hexdigest()
            roll = int(h[:8], 16) / 0xFFFFFFFF
            if roll < prob_step:
                idx = int(h[8:10], 16) % len(OVERLOAD_FAULT_POOL)
                fkey, fspec = OVERLOAD_FAULT_POOL[idx]
                p.add_debuff(fkey, fspec)
                faults.append({"piece": p.id, "fault": fkey, "load_ratio": ratio, "tick_5m": t + 1})
    return faults
