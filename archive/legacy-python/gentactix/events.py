# -*- coding: utf-8 -*-
"""事件引擎（V3 / 六分架构）：统一「行动约束锁（站住不许动/互斥） + Buff/Debuff 赋予器」。

设计哲学
--------
事件不重复实现底层物理算法，而是通过两个核心原语支配推演：
1. **行动约束锁**：如 `hold_still`（站住不许动）、`forbid_rest`（禁止休整）、`forbid_deploy`（禁止架重武器）；
2. **Buff / Debuff 赋予器**：实时调节单位的速度、油耗、组织度变化、作业人手占用火力折扣（`combat_power_mult`）以及暴露度增量。

支持事件：
- `war_council`：作战会议（站住不许动 + 连续指数暴露/会后高频发报暴露 + 班组破上限组织度 + AO 扩域）
- `rapid_move`：急行军（禁休整/禁架重武器 + 速度 1.4x + 高油耗 + 掉组织度 + 高暴露 + 战斗脆弱）
- `resupply_pickup`：取补给装卸（站住不许动 + 作业人手折扣 + 装卸暴露）
- `demolition`：爆破作业（站住不许动 + 作业人手折扣 + 爆破完成释放巨额声光暴露与地形/桥梁破坏）
- `deploy_heavy_unit`：架设创建重武器独立单位（站住不许动 + 作业人手折扣 + 完成后划出 crew 与重武器建独立单位）
- `pack_heavy_unit`：撤收重武器单位（站住不许动 + 完成后回收人手与武器并注销阵地单位）
- `operate_heavy_unit`：进格主动操作重武器单位（站住不许动 + 母班组扣除 crew 火力 + 士官属性决定重武器效率）
"""

from __future__ import annotations

import heapq
import math

from . import modifiers as mod_engine
from . import terrain_field as tf
from . import toe

REORG_DURATION_BY_ECHELON = {
    "squad": 0.25,
    "platoon": 0.25,
    "company": 1.0,
    "battalion": 2.0,
    "regiment": 3.0,
    "brigade": 4.0,
    "division": 6.0,
}


def _grid_dims(grid):
    if hasattr(grid, "shape") and len(grid.shape) >= 2:
        return int(grid.shape[0]), int(grid.shape[1])
    return int(getattr(grid, "rows", 0)), int(getattr(grid, "cols", 0))


def is_literal_ground_px(grid, rc):
    """严格字面意义判断像素是否为陆地（排除 WATER、BRIDGE 地形及 water/bridge/river/impassable 特征）。"""
    if grid is None:
        return True
    r, c = int(round(rc[0])), int(round(rc[1]))
    rows, cols = _grid_dims(grid)
    if not (0 <= r < rows and 0 <= c < cols):
        return False
    if hasattr(grid, "terrain"):
        t_code = int(grid.terrain[r, c])
        if t_code in (getattr(tf, "WATER", 4), getattr(tf, "BRIDGE", 13)):
            return False
    feats = set()
    mods_fn = getattr(grid, "get_cell_modifiers", None)
    if callable(mods_fn):
        try:
            mods = mods_fn((r, c)) or {}
            if mods.get("bridge_over_water") or mods.get("impassable"):
                return False
            feats.update(mods.get("features") or ())
        except Exception:
            pass
    fn = getattr(grid, "get_terrain_features", None)
    if callable(fn):
        try:
            feats.update(fn((r, c)) or ())
        except TypeError:
            try:
                feats.update(fn(r, c) or ())
            except Exception:
                pass
    if hasattr(grid, "cell_modifiers") and isinstance(grid.cell_modifiers, dict):
        for k in (grid.cell_modifiers.get((r, c), {}) or {}).keys():
            feats.add(str(k))
    for f in feats:
        fl = str(f).lower()
        if fl in ("water", "bridge", "bridge_over_water", "river", "impassable") or "bridge" in fl or "water" in fl:
            return False
    return True


def find_nearest_literal_ground_px(grid, center_px):
    """若在桥梁或非陆地像素整编，搜索最近的字面陆地（存在陆地类）像素。"""
    c0 = (int(round(center_px[0])), int(round(center_px[1])))
    if is_literal_ground_px(grid, c0):
        return c0
    rows, cols = _grid_dims(grid)
    pq = [(0, c0[0], c0[1])]
    visited = {c0}
    while pq:
        _, r, c = heapq.heappop(pq)
        if is_literal_ground_px(grid, (r, c)):
            return (r, c)
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                if dr == 0 and dc == 0:
                    continue
                nr, nc = r + dr, c + dc
                if 0 <= nr < rows and 0 <= nc < cols and (nr, nc) not in visited:
                    visited.add((nr, nc))
                    heapq.heappush(pq, ((nr - c0[0]) ** 2 + (nc - c0[1]) ** 2, nr, nc))
    return c0

EVENT_SPECS = {
    "rapid_move": dict(
        hold_still=False, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.4, fuel_mult=1.6, org_delta_per_h=-2.5,
        exposure_delta=1.5, combat_power_mult=0.75, default_hours=6.0,
    ),
    "resupply_pickup": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=False,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=1.2, combat_power_mult=1.0, default_hours=0.5,
    ),
    "demolition": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=2.0, burst_exposure=12.0, combat_power_mult=1.0, default_hours=0.5,
    ),
    "deploy_heavy_unit": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=1.0, combat_power_mult=1.0, default_hours=0.5,
    ),
    "pack_heavy_unit": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=0.8, combat_power_mult=1.0, default_hours=0.5,
    ),
    "operate_heavy_unit": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=0.6, combat_power_mult=1.0, default_hours=6.0,
    ),
    "operate_vehicle": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=0.3, combat_power_mult=1.0, default_hours=6.0,
    ),
    "repair_debuff": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=1.0, combat_power_mult=0.5, default_hours=0.5,
    ),
    "cargo_transfer": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=False,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=0.8, combat_power_mult=0.8, default_hours=0.25,
    ),
    "fire_weapon": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=2.5, combat_power_mult=1.0, default_hours=5.0 / 60.0,
    ),
    "register_fire": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=1.2, combat_power_mult=0.85, default_hours=0.25,
    ),
    "engineering": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=1.8, combat_power_mult=0.5, default_hours=0.5,
    ),
    "fortify": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=0.8, combat_power_mult=0.6, default_hours=0.5,
    ),
    "radio_decoy": dict(
        hold_still=True, forbid_rest=False, forbid_deploy=False,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=3.5, combat_power_mult=1.0, default_hours=2.0,
    ),
    "reorganize_echelon": dict(
        hold_still=True, forbid_rest=True, forbid_deploy=True,
        speed_mult=0.0, fuel_mult=0.0, org_delta_per_h=0.0,
        exposure_delta=1.0, combat_power_mult=0.8, default_hours=1.0,
    ),
    "radio_silence": dict(
        hold_still=False, forbid_rest=False, forbid_deploy=False,
        speed_mult=1.0, fuel_mult=1.0, org_delta_per_h=0.0,
        exposure_delta=-1.5, combat_power_mult=1.0, default_hours=4.0,
    ),
}


def get_piece_ao_scopes(world, piece):
    """解析单位所属的排级（或自身）作用域与连级作用域，用于校射诸元共享与侦察穿透。"""
    platoon_scope = None
    company_scope = None
    for z in (getattr(world, "ao_zones", None) or {}).values():
        if z.team == piece.team and z.activate_turn <= world.turn and piece.px in z.cells:
            if z.echelon == "platoon" and platoon_scope is None:
                platoon_scope = z.zone_id
                cmd = (world.commanders or {}).get(z.commander_id)
                if cmd and cmd.parent_commander_id and cmd.parent_commander_id in world.commanders:
                    pc = world.commanders[cmd.parent_commander_id]
                    company_scope = company_scope or pc.unit_id or pc.id
            elif z.echelon == "company" and company_scope is None:
                company_scope = z.zone_id
    if company_scope is None and getattr(world, "commanders", None):
        for c in world.commanders.values():
            if c.team == piece.team and c.echelon == "company" and piece.id in c.subordinate_ids:
                company_scope = c.unit_id or c.id
                break
        if company_scope is None:
            for c in world.commanders.values():
                if c.team == piece.team and piece.id in c.subordinate_ids and c.parent_commander_id:
                    pc = world.commanders.get(c.parent_commander_id)
                    if pc:
                        company_scope = pc.unit_id or pc.id
                        break
    platoon_scope = platoon_scope or piece.id
    company_scope = company_scope or platoon_scope
    return platoon_scope, company_scope


def resolve_registration_bonus(world, piece, target_px, bgroup):
    """查询目标像素容器上对当前单位生效的最大校射精度增量（bonus = acc_mult - 1.0）：
    - 同混编棋子 / 连级侦察穿透 / 同排同弹道族：100% 满额；
    - 同排 AO 内不同棋子跨弹道族：50% 折半共享。
    """
    rc = (int(round(target_px[0])), int(round(target_px[1])))
    bucket = world.grid.cell_modifiers.get(rc, {})
    if not bucket:
        return 0.0
    platoon_scope, company_scope = get_piece_ao_scopes(world, piece)
    team = piece.team
    piece_key = f"reg:{team}:piece:{piece.id}"
    company_key = f"reg:{team}:company:{company_scope}:all"
    same_group_key = f"reg:{team}:ao:{platoon_scope}:{bgroup}"
    ao_prefix = f"reg:{team}:ao:{platoon_scope}:"

    max_bonus = 0.0
    for key, entry in bucket.items():
        val = float((entry.get("modifiers") or {}).get("accuracy_mult", 1.0)) - 1.0
        if val <= 0.0:
            continue
        if key in (piece_key, company_key, same_group_key):
            eff_bonus = val
        elif key.startswith(ao_prefix):
            eff_bonus = 0.5 * val
        else:
            continue
        if eff_bonus > max_bonus:
            max_bonus = eff_bonus
    return round(max_bonus, 4)


def apply_registration_to_cells(world, piece, target_px, radius_px, weapon, dist_m, duration_min, long_term=False):
    """按三锚点分段幂律向目标区域像素写入校射精度（普通开火 15min 快消 vs 专门校射 4h 长效 + 侦察连级穿透）。"""
    g = world.grid
    has_recon = any((sq.kind == "recon_squad" or sq.has_buff("recon_radius")) and sq.persons > 0 for sq in piece.squads())
    eff_weapon = weapon
    eff_dist = dist_m
    if has_recon and toe.EQUIPMENT.get(weapon, {}).get("range_m", 0.0) < 1500.0:
        eff_dist = float(dist_m) * (400.0 / 1500.0)
        eff_weapon = "rifle"
    delta, cap = toe.compute_registration_delta(eff_weapon, eff_dist, duration_min=duration_min)
    bgroup = toe.weapon_ballistic_group(weapon)
    platoon_scope, company_scope = get_piece_ao_scopes(world, piece)
    ttl = 4.0 if long_term else 0.25

    keys = [
        f"reg:{piece.team}:piece:{piece.id}",
        f"reg:{piece.team}:ao:{platoon_scope}:{bgroup}",
    ]
    if has_recon:
        keys.append(f"reg:{piece.team}:company:{company_scope}:all")

    lim = int(math.ceil(radius_px))
    tx, ty = int(round(target_px[0])), int(round(target_px[1]))
    reg_cells = 0
    best_next = 1.0
    for dr in range(-lim, lim + 1):
        for dc in range(-lim, lim + 1):
            if dr * dr + dc * dc <= radius_px * radius_px + 1e-9:
                rr, cc = tx + dr, ty + dc
                if 0 <= rr < g.shape[0] and 0 <= cc < g.shape[1]:
                    cell_bucket = g.cell_modifiers.get((rr, cc), {})
                    for k in keys:
                        old_entry = cell_bucket.get(k, {})
                        old_mult = float((old_entry.get("modifiers") or {}).get("accuracy_mult", 1.0))
                        old_ttl = old_entry.get("ttl_hours")
                        new_mult = min(1.0 + cap, round(old_mult + delta, 5))
                        keep_ttl = ttl if old_ttl is None else max(float(old_ttl), ttl)
                        g.set_cell_modifier((rr, cc), k, {"accuracy_mult": new_mult}, ttl_hours=keep_ttl)
                        if new_mult > best_next:
                            best_next = new_mult
                    reg_cells += 1
    return {"delta": round(delta, 5), "cap": round(cap, 4), "ttl_hours": ttl, "reg_cells": reg_cells,
            "best_acc_mult": round(best_next, 4), "recon_company_scope": company_scope if has_recon else None}


class ActiveEvent:
    """挂载在单位上的活动事件实例（行动锁 + Buff 赋予器）。"""

    def __init__(self, eid, kind, actor_id, remaining_hours, payload=None, labor_persons=0):
        self.id = eid
        self.kind = kind
        self.actor_id = actor_id
        self.remaining_hours = float(remaining_hours)
        self.payload = dict(payload or {})
        self.labor_persons = int(labor_persons)

    @property
    def spec(self):
        base = dict(EVENT_SPECS.get(self.kind, {
            "hold_still": False, "forbid_rest": False, "forbid_deploy": False,
            "speed_mult": 1.0, "fuel_mult": 1.0, "org_delta_per_h": 0.0,
            "exposure_delta": 0.0, "combat_power_mult": 1.0, "default_hours": 1.0,
        }))
        custom = self.payload.get("custom_spec")
        if isinstance(custom, dict):
            base.update(custom)
        return base

    def to_dict(self):
        return dict(
            id=self.id,
            kind=self.kind,
            actor_id=self.actor_id,
            remaining_hours=self.remaining_hours,
            payload=dict(self.payload),
            labor_persons=self.labor_persons,
        )

    @classmethod
    def from_dict(cls, d):
        return cls(
            d["id"], d["kind"], d["actor_id"], d["remaining_hours"],
            payload=d.get("payload"), labor_persons=d.get("labor_persons", 0),
        )


class EventManager:
    """统一事件调度表。"""

    def __init__(self, events=None, seq=0):
        self.events = {e.id: e for e in (events or [])}
        self._seq = int(seq)

    def _next_id(self, kind):
        self._seq += 1
        return f"ev-{kind}-{self._seq}"

    def active_for(self, actor_id):
        out = []
        for e in self.events.values():
            if e.actor_id == actor_id:
                out.append(e)
            elif e.kind == "reorganize_echelon":
                if actor_id == e.payload.get("commander_id") or actor_id == e.payload.get("commander_piece_id"):
                    out.append(e)
                elif actor_id in (e.payload.get("source_unit_ids") or ()):
                    out.append(e)
        return out

    def piece_modifiers(self, piece, world=None):
        """委托 modifiers 引擎汇总全部行动锁与 Buff/Debuff 乘区。"""
        return mod_engine.collect_piece_modifiers(piece, event_manager=self, world=world)

    @staticmethod
    def register_custom_spec(kind, spec):
        """开放自定义事件原语注册接口：业务层无需改引擎即可注册新锁与 Buff 组合。"""
        base = {
            "hold_still": False, "forbid_rest": False, "forbid_deploy": False,
            "speed_mult": 1.0, "fuel_mult": 1.0, "org_delta_per_h": 0.0,
            "exposure_delta": 0.0, "combat_power_mult": 1.0, "default_hours": 1.0,
        }
        base.update(spec or {})
        EVENT_SPECS[str(kind)] = base

    def start_event(self, world, kind, actor_id, payload=None, duration_hours=None, modifiers=None):
        payload = dict(payload or {})
        if modifiers:
            payload["custom_spec"] = dict(modifiers)
        if kind not in EVENT_SPECS and "custom_spec" not in payload:
            return {"ok": False, "reason": f"unknown_event:{kind}"}
        if kind == "reorganize_echelon":
            return self.start_reorganize_echelon(
                world,
                payload.get("commander_id") or actor_id,
                payload,
                duration_hours=duration_hours,
                commander_piece_id=payload.get("commander_piece_id") or (actor_id if actor_id in world.pieces else None),
            )
        piece = world.pieces.get(actor_id)
        if piece is None:
            return {"ok": False, "reason": "unknown_actor"}
        mods = self.piece_modifiers(piece)

        if kind == "deploy_heavy_unit" and mods["forbid_deploy"]:
            return {"ok": False, "reason": "forbidden_while_rapid_move"}

        labor = 0
        default_h = EVENT_SPECS.get(kind, {}).get("default_hours", 1.0)
        if isinstance(payload.get("custom_spec"), dict) and "default_hours" in payload["custom_spec"]:
            default_h = payload["custom_spec"]["default_hours"]
        dur = float(duration_hours) if duration_hours is not None else float(default_h)
        if kind == "deploy_heavy_unit":
            existing_id = payload.get("target_unit_id")
            if existing_id and existing_id in world.pieces:
                tgt = world.pieces[existing_id]
                if tgt.team != piece.team or math.hypot(tgt.pos[0] - piece.pos[0], tgt.pos[1] - piece.pos[1]) > 1.5:
                    return {"ok": False, "reason": "target_unit_not_here"}
                weapon = payload.get("weapon") or tgt.deployed_weapon
                if not weapon:
                    for m, n in tgt.equipment_counts().items():
                        if int(n) > 0 and toe.EQUIPMENT.get(m, {}).get("deployable"):
                            weapon = m
                            break
                eq_spec = toe.EQUIPMENT.get(weapon, {})
                if not eq_spec.get("deployable"):
                    return {"ok": False, "reason": "weapon_not_deployable"}
                if piece.persons() <= 0:
                    return {"ok": False, "reason": "insufficient_crew"}
                crew = int(eq_spec.get("crew", 2))
                labor = min(piece.persons(), max(1, int(payload.get("operator_count") or crew)))
                if duration_hours is None:
                    dur = float(eq_spec.get("deploy_hours", 0.5))
            else:
                weapon = payload.get("weapon")
                eq_spec = toe.EQUIPMENT.get(weapon, {})
                if not eq_spec.get("deployable"):
                    return {"ok": False, "reason": "weapon_not_deployable"}
                have = sum(sq.equipment.get(weapon, 0) for sq in piece.squads())
                if have <= 0:
                    return {"ok": False, "reason": "no_weapon_in_stock"}
                unmanned = bool(payload.get("unmanned", False)) or (payload.get("operator_persons") == 0)
                crew = int(eq_spec.get("crew", 3))
                if not unmanned and piece.persons() < crew:
                    return {"ok": False, "reason": "insufficient_crew"}
                if unmanned and piece.persons() <= 0:
                    return {"ok": False, "reason": "insufficient_crew"}
                labor = min(piece.persons(), max(1, int(payload.get("operator_count") or crew)))
                if duration_hours is None:
                    dur = float(eq_spec.get("deploy_hours", 0.5))
        elif kind == "pack_heavy_unit":
            tgt = world.pieces.get(payload.get("target_unit_id"))
            if tgt is None or not tgt.is_heavy_emplacement or math.hypot(tgt.pos[0] - piece.pos[0], tgt.pos[1] - piece.pos[1]) > 1.5:
                return {"ok": False, "reason": "target_emplacement_not_here"}
            labor = min(piece.persons(), max(1, tgt.persons() or 2))
        elif kind == "operate_heavy_unit":
            tgt = world.pieces.get(payload.get("target_unit_id"))
            if tgt is None or tgt.unit_state != "deployed" or math.hypot(tgt.pos[0] - piece.pos[0], tgt.pos[1] - piece.pos[1]) > 1.5 or tgt.team != piece.team:
                return {"ok": False, "reason": "target_emplacement_not_here"}
            weapon = tgt.deployed_weapon
            if piece.persons() <= 0:
                return {"ok": False, "reason": "insufficient_crew"}
            sqs = [s for s in piece.squads() if s.persons > 0]
            if not sqs:
                return {"ok": False, "reason": "no_operator_squad"}
            op_sq = max(sqs, key=lambda s: toe.heavy_operator_efficiency(s, weapon))
            payload["operator_eff"] = toe.heavy_operator_efficiency(op_sq, weapon)
            sel_sids = list(payload.get("operator_soldier_ids") or [])
            op_cnt = int(payload.get("operator_count") or len(sel_sids) or min(piece.persons(), int(toe.EQUIPMENT.get(weapon, {}).get("crew", 2))))
            tgt.operated_by = piece.id
            tgt.meta["operator_soldier_ids"] = sel_sids
            tgt.meta["operator_persons"] = max(1, min(piece.persons(), op_cnt))
            labor = max(1, min(piece.persons(), op_cnt))
        elif kind == "operate_vehicle":
            tgt = world.pieces.get(payload.get("target_unit_id"))
            if tgt is None or not tgt.vehicle_model or tgt.px != piece.px or tgt.team != piece.team:
                return {"ok": False, "reason": "target_vehicle_not_here"}
            if piece.persons() < int(toe.VEHICLE_SPECS[tgt.vehicle_model]["min_crew"]):
                return {"ok": False, "reason": "insufficient_crew"}
            tgt.operated_by = piece.id
            labor = min(piece.persons(), int(toe.VEHICLE_SPECS[tgt.vehicle_model]["full_crew"]))
        elif kind == "repair_debuff":
            tgt = world.pieces.get(payload.get("target_unit_id", piece.id))
            if tgt is None or tgt.px != piece.px:
                return {"ok": False, "reason": "target_not_here"}
            labor = min(piece.persons(), int(payload.get("labor_persons", 2)))
        elif kind == "cargo_transfer":
            labor = min(piece.persons(), int(payload.get("labor_persons", 2)))
        elif kind == "fire_weapon":
            if mods.get("forbid_fire"):
                return {"ok": False, "reason": "forbid_fire_locked"}
            weapon = payload.get("weapon")
            have = sum(sq.equipment.get(weapon, 0) for sq in piece.squads()) if weapon else 0
            if not weapon or have <= 0:
                return {"ok": False, "reason": "weapon_missing"}
            if "target_px" not in payload:
                return {"ok": False, "reason": "target_px_required"}
            labor = min(piece.persons(), int(toe.EQUIPMENT.get(weapon, {}).get("crew", 1)))
        elif kind == "register_fire":
            if "target_px" not in payload:
                return {"ok": False, "reason": "target_px_required"}
            labor = min(piece.persons(), int(payload.get("labor_persons", 2)))
        elif kind in ("engineering", "fortify"):
            eng_op = payload.get("op", "dig_trench")
            tgt_px = payload.get("target_px", piece.px)
            if math.hypot(float(tgt_px[0]) - piece.pos[0], float(tgt_px[1]) - piece.pos[1]) > 1.8:
                return {"ok": False, "reason": "target_too_far"}
            if kind == "engineering" and eng_op in ("build_bridge", "repair_airfield", "repair_terrain", "lay_mines", "clear_mines"):
                is_eng = any(
                    sq.has_buff(k) for sq in piece.squads()
                    for k in ("bridge_building", "mine_clearing", "airfield_repair", "salvage")
                ) or any(
                    sq.equipment.get("mine_detector", 0) > 0 or sq.equipment.get("demo_charge", 0) > 0
                    for sq in piece.squads()
                )
                if not is_eng:
                    return {"ok": False, "reason": "requires_engineer_capability"}
            labor = min(piece.persons(), int(payload.get("labor_persons", 4)))
        elif kind == "radio_decoy":
            has_rad = any(sq.equipment.get("radio", 0) > 0 or sq.equipment.get("hq_radio", 0) > 0 for sq in piece.squads())
            if not has_rad and not payload.get("force"):
                return {"ok": False, "reason": "no_radio"}
            labor = min(piece.persons(), 2)
        elif kind == "demolition":
            tgt_px = payload.get("target_px", piece.px)
            if math.hypot(float(tgt_px[0]) - piece.pos[0], float(tgt_px[1]) - piece.pos[1]) > 1.8:
                return {"ok": False, "reason": "target_too_far"}
            has_demo_cap = any(
                sq.has_buff(k) for sq in piece.squads() for k in ("salvage", "bridge_building", "mine_clearing")
            )
            demo_sq = next((sq for sq in piece.squads() if int(sq.equipment.get("demo_charge", 0)) > 0), None)
            if not has_demo_cap and demo_sq is None:
                return {"ok": False, "reason": "requires_engineer_or_demo_charge"}
            if not payload.get("free"):
                if demo_sq is None:
                    return {"ok": False, "reason": "no_demo_charge"}
                demo_sq.equipment["demo_charge"] -= 1
            labor = min(piece.persons(), int(payload.get("labor_persons", 4)))
        elif kind == "resupply_pickup":
            labor = min(piece.persons(), int(payload.get("labor_persons", 4)))

        ev = ActiveEvent(self._next_id(kind), kind, actor_id, dur, payload=payload, labor_persons=labor)
        self.events[ev.id] = ev
        return {"ok": True, "event_id": ev.id, "kind": kind, "duration_h": dur, "labor_persons": labor,
                "payload": payload}

    def start_reorganize_echelon(self, world, commander_id, spec, duration_hours=None, commander_piece_id=None):
        """启动排以上编制整编定时事件（允许 cadre=0 空干部；锁定参编源部队；无前置门槛）。"""
        spec = dict(spec or {})
        cmd = world.commanders.get(commander_id) if hasattr(world, "commanders") else None
        if cmd is None or not cmd.alive:
            return {"ok": False, "reason": "commander_unavailable", "commander_id": commander_id}
        src_ids = [pid for pid in (spec.get("source_unit_ids") or []) if pid in world.pieces and world.pieces[pid].team == cmd.team]
        if not src_ids:
            return {"ok": False, "reason": "no_source_units"}
        # 确认最高指挥官实体单位（若调用方传入或 HQ 在同格）
        cmd_pid = commander_piece_id or spec.get("commander_piece_id") or spec.get("highest_commander_piece_id") or getattr(cmd, "hq_piece_id", None)
        if not cmd_pid or cmd_pid not in world.pieces:
            # 优先匹配 cmd.px 同格内指挥部/最高编制单位，避免误把远处排当指挥官单位
            at_cmd_px = [p for p in world.pieces.values() if p.team == cmd.team and tuple(p.px) == tuple(cmd.px)]
            if at_cmd_px:
                at_cmd_px.sort(key=lambda p: (
                    0 if ("hq" in p.id.lower() or "hq" in p.name.lower() or any(s.kind == "hq_squad" for s in p.squads())) else 1,
                    -p.persons(),
                ))
                cmd_pid = at_cmd_px[0].id
            else:
                cmd_pid = None

        target_echelon = str(spec.get("target_echelon", "platoon"))
        dur = float(duration_hours if duration_hours is not None else spec.get("duration_h", REORG_DURATION_BY_ECHELON.get(target_echelon, 1.0)))
        labor = sum(world.pieces[pid].persons() for pid in src_ids)
        payload = dict(spec)
        payload["commander_id"] = cmd.id
        payload["commander_piece_id"] = cmd_pid
        payload["target_echelon"] = target_echelon
        payload["source_unit_ids"] = list(src_ids)
        payload["target_specs"] = list(spec.get("target_specs") or [])

        actor = cmd_pid or cmd.id
        ev = ActiveEvent(self._next_id("reorganize_echelon"), "reorganize_echelon", actor, dur, payload=payload, labor_persons=labor)
        self.events[ev.id] = ev
        for pid in src_ids + ([cmd_pid] if cmd_pid and cmd_pid not in src_ids else []):
            p = world.pieces.get(pid)
            if p is not None:
                p.meta["reorganizing_event_id"] = ev.id
        return {
            "ok": True,
            "event_id": ev.id,
            "kind": "reorganize_echelon",
            "commander_id": cmd.id,
            "commander_piece_id": cmd_pid,
            "target_echelon": target_echelon,
            "duration_h": dur,
            "source_unit_ids": list(src_ids),
        }

    def _abort_reorganize_event(self, world, eid, reason="interrupted"):
        ev = self.events.pop(eid, None)
        if ev is None:
            return None
        for pid in list(ev.payload.get("source_unit_ids") or []) + [ev.payload.get("commander_piece_id")]:
            if pid and world is not None and pid in world.pieces:
                world.pieces[pid].meta.pop("reorganizing_event_id", None)
        return {"ok": True, "aborted_event": eid, "reason": reason}

    def notify_unit_attacked(self, world, attacked_piece_id=None, attacker_team=None, cell_rc=None):
        """整编中最高指挥官单位受到任何攻击直接打断整编，立即撤除锁定，保留原单位。"""
        interrupted = []
        for eid, ev in list(self.events.items()):
            if ev.kind != "reorganize_echelon":
                continue
            cmd_id = ev.payload.get("commander_id")
            cmd = world.commanders.get(cmd_id) if (world is not None and hasattr(world, "commanders")) else None
            if cmd is not None and attacker_team is not None and cmd.team == attacker_team:
                continue
            cmd_pid = ev.payload.get("commander_piece_id")
            hit = False
            if attacked_piece_id is not None and cmd_pid is not None and attacked_piece_id == cmd_pid:
                hit = True
            elif attacked_piece_id is not None and attacked_piece_id == cmd_id:
                hit = True
            elif cell_rc is not None and cmd is not None and cmd_pid is None and tuple(cmd.px) == (int(cell_rc[0]), int(cell_rc[1])):
                hit = True
            if hit:
                self._abort_reorganize_event(world, eid, reason="highest_commander_under_attack")
                interrupted.append({
                    "event_id": eid,
                    "commander_id": cmd_id,
                    "commander_piece_id": cmd_pid,
                    "reason": "highest_commander_under_attack",
                })
        return interrupted

    def cancel_hold_still_on_move(self, actor_id, world=None):
        dropped = []
        for eid, ev in list(self.events.items()):
            if ev.actor_id == actor_id and ev.spec["hold_still"]:
                if ev.kind == "operate_heavy_unit" and world is not None:
                    tgt = world.pieces.get(ev.payload.get("target_unit_id"))
                    if tgt is not None and tgt.operated_by == actor_id:
                        tgt.operated_by = None
                del self.events[eid]
                dropped.append(eid)
        return dropped

    def tick_events(self, world, dt_hours):
        """在时间切片上推进所有活动事件，应用持续 Debuff 并收口到期事件。"""
        dt = max(0.0, float(dt_hours))
        completed = []
        for eid, ev in list(self.events.items()):
            p = world.pieces.get(ev.actor_id)
            if ev.kind == "reorganize_echelon":
                cmd = world.commanders.get(ev.payload.get("commander_id")) if hasattr(world, "commanders") else None
                if cmd is None or not cmd.alive:
                    self._abort_reorganize_event(world, eid, reason="commander_lost")
                    continue
            elif p is None or p.persons() <= 0:
                del self.events[eid]
                continue
            sp = ev.spec
            if p is not None and sp["org_delta_per_h"] != 0.0 and dt > 0:
                for sq in p.squads():
                    sq.org = max(0.0, sq.org + sp["org_delta_per_h"] * dt)
            ev.remaining_hours -= dt
            if ev.remaining_hours <= 1e-9:
                res = self._complete_event(world, ev, p)
                completed.append({"event_id": eid, "kind": ev.kind, "actor": (p.id if p is not None else ev.actor_id), **res})
                self.events.pop(eid, None)
        return completed

    def _complete_event(self, world, ev, p):
        g = world.grid
        if ev.kind == "demolition":
            tgt_px = tuple(ev.payload.get("target_px", p.px))
            target_type = ev.payload.get("target_type", "bridge")
            g.add_exposure(tgt_px, ev.spec.get("burst_exposure", 12.0), radius_px=2.0)
            if target_type == "bridge":
                ok = g.destroy_bridge(tgt_px)
                g.cut_wire_at(tgt_px, radius_px=1.0)
                return {"demolished": "bridge", "px": list(tgt_px), "ok": ok}
            elif target_type == "terrain":
                ok = g.destroy_terrain(tgt_px)
                g.cut_wire_at(tgt_px, radius_px=1.0)
                return {"demolished": "terrain", "px": list(tgt_px), "ok": ok}
            elif target_type == "stash":
                g.cell_stash.pop(tgt_px, None)
                return {"demolished": "stash", "px": list(tgt_px), "ok": True}
            elif target_type == "depot":
                burned = []
                for dep in world.depots.values():
                    if tuple(dep.px) == tgt_px:
                        ammo_loss = float(dep.stock.get("ammo", 0.0))
                        burned.append({"depot": dep.id, "prev_stock": dict(dep.stock)})
                        for rk in list(dep.stock):
                            dep.stock[rk] = 0.0
                        dep.ammo_by_type.clear()
                        if ammo_loss > 1.0:
                            g.add_exposure(tgt_px, 15.0, radius_px=1.0)
                return {"demolished": "depot", "px": list(tgt_px), "burned": burned, "ok": bool(burned)}
            return {"demolished": target_type, "px": list(tgt_px)}

        if ev.kind == "deploy_heavy_unit":
            existing_id = ev.payload.get("target_unit_id")
            if existing_id and existing_id in world.pieces:
                tgt = world.pieces[existing_id]
                tgt.detach("deployed")
                tgt.pos = tuple(p.pos)
                tgt.is_heavy_emplacement = True
                if ev.payload.get("keep_manned", True):
                    tgt.operated_by = p.id
                    tgt.meta["operator_soldier_ids"] = list(ev.payload.get("operator_soldier_ids") or [])
                    tgt.meta["operator_persons"] = int(ev.payload.get("operator_count") or len(tgt.meta["operator_soldier_ids"]) or p.persons())
                return {"ok": True, "deployed_unit": tgt.id, "weapon": tgt.deployed_weapon}
            weapon = ev.payload["weapon"]
            crew = int(toe.EQUIPMENT[weapon]["crew"])
            src_sq = next((s for s in p.squads() if s.equipment.get(weapon, 0) > 0 and s.persons > 0), None)
            if src_sq is None:
                return {"ok": False, "reason": "weapon_missing"}
            unmanned = bool(ev.payload.get("unmanned", False)) or (ev.payload.get("operator_persons") == 0)
            new_pid = ev.payload.get("new_unit_id") or f"{p.id}-{weapon}-{self._seq}"
            if unmanned:
                det_sq = toe.split_squad(
                    src_sq, 0, take_cadre=0,
                    take_equipment={weapon: 1}, new_id=f"{new_pid}-sq",
                )
                op_by = p.id
            else:
                take_cad = 1 if src_sq.cadre > 0 and src_sq.persons > crew else 0
                take_enl = min(src_sq.enlisted, max(0, crew - take_cad))
                det_sq = toe.split_squad(
                    src_sq, take_enl, take_cadre=take_cad,
                    take_equipment={weapon: 1}, new_id=f"{new_pid}-sq",
                )
                op_by = p.id if ev.payload.get("keep_manned") else None
            root = toe.Node(new_pid, f"{toe.EQUIPMENT[weapon]['name']}阵地", "squad", squad=det_sq)
            emp = toe.MapPiece(
                new_pid, f"{toe.EQUIPMENT[weapon]['name']}阵地", p.team, p.px, root,
                level="squad", footprint_px=1, radio_channel=p.radio_channel,
                is_heavy_emplacement=True, deployed_weapon=weapon,
                pos=p.pos, unit_state="deployed", operated_by=op_by,
            )
            if unmanned:
                emp.meta["operator_soldier_ids"] = list(ev.payload.get("operator_soldier_ids") or [])
                emp.meta["operator_persons"] = int(ev.payload.get("operator_count") or len(emp.meta["operator_soldier_ids"]) or p.persons())
            emp.set_full_stock(0.2)
            world.add_piece(emp)
            if hasattr(world, "commanders"):
                for c in world.commanders.values():
                    if p.id in c.subordinate_ids and emp.id not in c.subordinate_ids:
                        c.subordinate_ids.append(emp.id)
            return {"ok": True, "created_unit": new_pid, "weapon": weapon, "crew": det_sq.persons, "unit_state": "deployed"}

        if ev.kind in ("operate_heavy_unit", "operate_vehicle"):
            tgt = world.pieces.get(ev.payload.get("target_unit_id"))
            if tgt is not None and tgt.operated_by == p.id:
                tgt.operated_by = None
            return {"ok": True, "released_unit": ev.payload.get("target_unit_id")}

        if ev.kind == "repair_debuff":
            tgt = world.pieces.get(ev.payload.get("target_unit_id", p.id))
            if tgt is None:
                return {"ok": False, "reason": "target_missing"}
            keys = ev.payload.get("debuff_keys")
            removed = []
            if keys:
                for k in keys:
                    if tgt.remove_debuff(k):
                        removed.append(k)
            else:
                removed = list(tgt.status_debuffs.keys())
                tgt.status_debuffs.clear()
            return {"ok": True, "repaired_unit": tgt.id, "removed_debuffs": removed}

        if ev.kind == "cargo_transfer":
            return {"ok": True, "unit": p.id}

        if ev.kind == "fire_weapon":
            from . import combat as cb
            weapon = ev.payload["weapon"]
            tgt_px = (int(round(ev.payload["target_px"][0])), int(round(ev.payload["target_px"][1])))
            radius_px = max(0.0, float(ev.payload.get("radius_px", 1.0)))
            bgroup = toe.weapon_ballistic_group(weapon)
            indirect = toe.is_indirect_weapon(weapon)
            acc_bonus = resolve_registration_bonus(world, p, tgt_px, bgroup)
            acc_mult = 1.0 + acc_bonus

            dist_m = g.distance_km(p.pos, tgt_px) * 1000.0
            has_los = True if indirect else (tf.line_visibility(g, p.px, tgt_px, night=world.night) > 1e-6)
            mods = self.piece_modifiers(p, world=world)
            fire_h = float(ev.payload.get("duration_h", ev.spec.get("default_hours", 5.0 / 60.0)))
            totals = {}
            op_info = p.get_operator_info(world.pieces) if hasattr(p, "get_operator_info") else {}
            op_sq = op_info.get("operator_squad") if (p.persons() <= 0 or p.operated_by) else None
            op_pers = op_info.get("operator_persons") if (p.persons() <= 0 or p.operated_by) else None
            for sq in p.squads():
                pwr = sq.power(
                    dist_m=dist_m, has_los=has_los, only_weapon=weapon, acc_mult=acc_mult,
                    operator_squad=op_sq, operator_persons=op_pers,
                )
                if pwr:
                    world.exposure_tracker.mark_fired(sq)
                for dt_, pts in pwr.items():
                    totals[dt_] = totals.get(dt_, 0.0) + pts * fire_h * mods["combat_power_mult"]

            lim = int(math.ceil(radius_px))
            cover_cells = []
            occ_cells = []
            depot_cells = {tuple(dep.px) for dep in (world.depots or {}).values()}
            piece_cells = {op.px for op in world.pieces.values() if op.persons() > 0 or getattr(op, "vehicle_model", None)}
            for dr in range(-lim, lim + 1):
                for dc in range(-lim, lim + 1):
                    if dr * dr + dc * dc <= radius_px * radius_px + 1e-9:
                        rr, cc = tgt_px[0] + dr, tgt_px[1] + dc
                        if 0 <= rr < g.shape[0] and 0 <= cc < g.shape[1]:
                            rc = (rr, cc)
                            cover_cells.append(rc)
                            if rc in piece_cells or rc in depot_cells:
                                occ_cells.append(rc)

            casualties_rec = {}
            strike_logs = []
            if sum(totals.values()) > 0.0 and occ_cells:
                share = 1.0 / len(occ_cells)
                if radius_px > 1.5 and len(cover_cells) > 0:
                    share = 1.0 / max(len(occ_cells), int(round(len(cover_cells) * 0.25)))
                sub_fire = {k: v * share for k, v in totals.items()}
                for rc in occ_cells:
                    sr = cb.resolve_cell_strike(
                        world,
                        rc,
                        p.team,
                        sub_fire,
                        dist_m=dist_m,
                        weapon_model=weapon,
                        acc_bonus=acc_bonus,
                        aspect_target_pos=p.pos,
                        seed_tag=f"{world.turn}|{ev.id}|{p.id}",
                        casualties_out=casualties_rec,
                    )
                    strike_logs.append(sr)

            reg_info = apply_registration_to_cells(
                world, p, tgt_px, radius_px, weapon, dist_m, fire_h * 60.0, long_term=False
            )
            supp_ttl = max(0.25, float(getattr(world, "hours", 0.25)))
            for rc in cover_cells:
                g.set_cell_modifier(rc, f"supp:{p.team}", {"speed_mult": 0.65, "suppressed": 1.0}, ttl_hours=supp_ttl)

            hits = [{"target": pid, **stats} for pid, stats in casualties_rec.items()]
            return {
                "ok": True, "weapon": weapon, "ballistic_group": bgroup,
                "acc_bonus_used": acc_bonus, "reg_info": reg_info,
                "has_los": has_los, "dist_m": round(dist_m, 1),
                "hits": hits, "cell_strikes": strike_logs,
            }

        if ev.kind == "register_fire":
            tgt_px = (int(round(ev.payload["target_px"][0])), int(round(ev.payload["target_px"][1])))
            radius_px = max(0.0, float(ev.payload.get("radius_px", 1.0)))
            weapon = ev.payload.get("weapon")
            if not weapon:
                for sq in p.squads():
                    for m, n in sq.equipment.items():
                        if n > 0 and toe.EQUIPMENT.get(m, {}).get("fire"):
                            weapon = m
                            break
                    if weapon:
                        break
                weapon = weapon or "rifle"
            dist_m = g.distance_km(p.pos, tgt_px) * 1000.0
            dur_h = float(ev.payload.get("duration_h", ev.spec.get("default_hours", 0.25)))
            reg_info = apply_registration_to_cells(
                world, p, tgt_px, radius_px, weapon, dist_m, dur_h * 60.0, long_term=True
            )
            return {"ok": True, "weapon": weapon, "target_px": list(tgt_px), "dist_m": round(dist_m, 1), **reg_info}

        if ev.kind in ("engineering", "fortify"):
            tgt_px = (int(round(ev.payload.get("target_px", p.px)[0])), int(round(ev.payload.get("target_px", p.px)[1])))
            op = ev.payload.get("op", "dig_trench" if ev.kind == "fortify" else "dig_trench")
            if op == "build_bridge":
                ok = g.repair_bridge(tgt_px)
                return {"ok": ok, "op": op, "px": list(tgt_px)}
            if op in ("repair_airfield", "repair_terrain"):
                ok = g.repair_terrain(tgt_px)
                return {"ok": ok, "op": op, "px": list(tgt_px)}
            if op == "lay_mines":
                g.set_cell_modifier(
                    tgt_px,
                    f"minefield:{p.team}",
                    {"mine_density": float(ev.payload.get("density", 1.0)), "speed_mult": 0.7},
                    ttl_hours=None,
                )
                return {"ok": True, "op": op, "px": list(tgt_px)}
            if op == "clear_mines":
                bucket = g.cell_modifiers.get(tgt_px, {})
                removed = [k for k in list(bucket) if k.startswith("minefield:")]
                for k in removed:
                    g.remove_cell_modifier(tgt_px, k)
                if hasattr(g, "feature_channels") and "minefield" in g.feature_channels:
                    mf_mask = g.feature_channels["minefield"]["mask"]
                    if 0 <= tgt_px[0] < mf_mask.shape[0] and 0 <= tgt_px[1] < mf_mask.shape[1] and bool(mf_mask[tgt_px]):
                        mf_mask[tgt_px] = False
                        removed.append("feature:minefield")
                g.add_exposure(tgt_px, 6.0, radius_px=1.0)
                return {"ok": bool(removed), "op": op, "cleared": removed, "px": list(tgt_px)}
            # 挖掘像素级战壕/野战工事（归属像素而非绑在棋子上）
            fort_val = float(ev.payload.get("fortification_bonus", 0.35))
            cov_val = float(ev.payload.get("cover_bonus", 0.20))
            g.set_cell_modifier(tgt_px, "fort:trench", {"fortification_bonus": fort_val, "cover_bonus": cov_val}, ttl_hours=None)
            return {"ok": True, "op": "dig_trench", "px": list(tgt_px), "fortification_bonus": fort_val, "cover_bonus": cov_val}

        if ev.kind == "radio_decoy":
            tgt_px = (int(round(ev.payload.get("target_px", p.px)[0])), int(round(ev.payload.get("target_px", p.px)[1])))
            ttl_h = float(ev.payload.get("signal_ttl_h", 6.0))
            g.set_cell_modifier(
                tgt_px,
                f"decoy_radio:{p.team}",
                {"decoy_signal": 1.0, "fake_echelon": ev.payload.get("fake_echelon", "battalion")},
                ttl_hours=ttl_h,
            )
            g.add_exposure(tgt_px, 4.0, radius_px=0.0)
            return {"ok": True, "op": "radio_decoy", "px": list(tgt_px), "ttl_hours": ttl_h}

        if ev.kind == "pack_heavy_unit":
            tgt = world.pieces.get(ev.payload.get("target_unit_id"))
            if tgt is None or math.hypot(tgt.pos[0] - p.pos[0], tgt.pos[1] - p.pos[1]) > 1.5:
                return {"ok": False, "reason": "target_missing"}
            tgt.operated_by = None
            tgt.meta.pop("operator_soldier_ids", None)
            tgt.meta.pop("operator_persons", None)
            tgt.is_heavy_emplacement = False
            if ev.payload.get("attach"):
                tgt.attach_to(p.id)
                return {"ok": True, "packed_unit": tgt.id, "unit_state": "carried", "attached_to": p.id}
            if ev.payload.get("detach_mobile"):
                tgt.detach("mobile")
                return {"ok": True, "packed_unit": tgt.id, "unit_state": "mobile"}
            dst_sq = next((s for s in p.squads() if s.persons > 0), None)
            if dst_sq is None:
                return {"ok": False, "reason": "no_receiver_squad"}
            for sq in tgt.squads():
                old_p = max(0, dst_sq.persons)
                add_p = max(0, sq.persons)
                tot_p = old_p + add_p
                if tot_p > 0:
                    dst_sq.org = (dst_sq.org * old_p + sq.org * add_p) / float(tot_p)
                dst_sq.enlisted += sq.enlisted
                dst_sq.cadre += sq.cadre
                dst_sq.enlisted_full += sq.enlisted_full
                dst_sq.cadre_full += sq.cadre_full
                for m, n in sq.equipment.items():
                    dst_sq.equipment[m] = dst_sq.equipment.get(m, 0) + n
                    dst_sq.equipment_full[m] = dst_sq.equipment_full.get(m, 0) + n
            dst_sq.clamp_org()
            for rk, rv in (tgt.stock or {}).items():
                p.stock[rk] = float(p.stock.get(rk, 0.0)) + float(rv)
            for ak, av in (tgt.ammo_by_type or {}).items():
                if float(av) > 1e-9:
                    p.ammo_by_type[ak] = float(p.ammo_by_type.get(ak, 0.0)) + float(av)
            p.refresh_stock_cap()
            world.pieces.pop(tgt.id, None)
            if hasattr(world, "commanders"):
                for c in world.commanders.values():
                    if tgt.id in c.subordinate_ids:
                        c.subordinate_ids.remove(tgt.id)
            return {"ok": True, "packed_unit": tgt.id}

        if ev.kind == "reorganize_echelon":
            return self._complete_reorganize_echelon(world, ev, p)

        return {"ok": True}

    def _complete_reorganize_echelon(self, world, ev, actor_piece):
        cmd_id = ev.payload.get("commander_id")
        cmd = world.commanders.get(cmd_id) if hasattr(world, "commanders") else None
        team = cmd.team if cmd is not None else (actor_piece.team if actor_piece is not None else "red")
        src_ids = [pid for pid in (ev.payload.get("source_unit_ids") or []) if pid in world.pieces and world.pieces[pid].team == team]
        if not src_ids:
            return {"ok": False, "reason": "no_surviving_sources"}

        cmd_pid = ev.payload.get("commander_piece_id")
        if cmd_pid and cmd_pid in world.pieces:
            anchor_px = world.pieces[cmd_pid].px
        elif cmd is not None:
            anchor_px = cmd.px
        elif actor_piece is not None:
            anchor_px = actor_piece.px
        else:
            anchor_px = world.pieces[src_ids[0]].px
        spawn_px = find_nearest_literal_ground_px(world.grid, anchor_px)

        pool_enl = 0
        pool_cad = 0
        pool_eq = {}
        pool_stock = {}
        pool_ammo = {}
        org_num = 0.0
        org_den = 0.0
        default_ch = getattr(cmd, "radio_channel", "A")
        for pid in src_ids:
            sp = world.pieces[pid]
            sp.meta.pop("reorganizing_event_id", None)
            default_ch = sp.radio_channel or default_ch
            for sq in sp.squads():
                pool_enl += int(sq.enlisted)
                pool_cad += int(sq.cadre)
                org_num += float(sq.org) * max(1, sq.persons)
                org_den += max(1, sq.persons)
                for m, n in sq.equipment.items():
                    if int(n) > 0:
                        pool_eq[m] = pool_eq.get(m, 0) + int(n)
            for rk, rv in sp.stock.items():
                pool_stock[rk] = pool_stock.get(rk, 0.0) + float(rv)
            for ak, av in sp.ammo_by_type.items():
                if float(av) > 1e-9:
                    pool_ammo[ak] = pool_ammo.get(ak, 0.0) + float(av)

        mean_org = org_num / max(1.0, org_den)
        if cmd_pid and cmd_pid in world.pieces:
            world.pieces[cmd_pid].meta.pop("reorganizing_event_id", None)

        # 清空旧实体单位
        for pid in src_ids:
            world.pieces.pop(pid, None)
            if hasattr(world, "commanders"):
                for c in world.commanders.values():
                    if pid in c.subordinate_ids:
                        c.subordinate_ids.remove(pid)

        target_specs = list(ev.payload.get("target_specs") or [])
        if not target_specs:
            # 默认生成目标编制单位（允许 cadre=0 空干部，按每 10 人切分班组）
            tgt_ech = str(ev.payload.get("target_echelon", "platoon"))
            n_sq = max(1, (pool_enl + pool_cad + 9) // 10)
            sq_specs = []
            rem_e, rem_c = pool_enl, pool_cad
            eq_keys = list(pool_eq.keys())
            for i in range(n_sq):
                take_e = min(rem_e, max(1, pool_enl // n_sq + (1 if i < (pool_enl % n_sq) else 0)))
                take_c = min(rem_c, 1 if i == 0 else (1 if rem_c > (n_sq - 1 - i) else 0))
                rem_e -= take_e
                rem_c -= take_c
                sq_eq = {}
                if i == 0:
                    sq_eq = dict(pool_eq)
                sq_specs.append({"kind": "mixed_squad", "enlisted": take_e, "cadre": take_c, "equipment": sq_eq})
            target_specs = [{
                "piece_id": f"{cmd_id or 'REORG'}-{tgt_ech}-{self._seq}",
                "name": ev.payload.get("name") or f"整编{tgt_ech}",
                "level": tgt_ech,
                "squads": sq_specs,
            }]

        rem_enl = pool_enl
        rem_cad = pool_cad
        rem_eq = dict(pool_eq)
        spawned_ids = []
        empty_cadre_ids = []
        created_pieces = []

        for idx, uspec in enumerate(target_specs):
            uspec = dict(uspec or {})
            pid = uspec.get("piece_id") or f"{cmd_id or 'REORG'}-U{self._seq}-{idx + 1}"
            lvl = str(uspec.get("level", ev.payload.get("target_echelon", "squad")))
            sq_specs = list(uspec.get("squads") or [])
            if not sq_specs:
                sq_specs = [{
                    "id": f"{pid}-sq1",
                    "kind": uspec.get("kind", "mixed_squad"),
                    "enlisted": int(uspec.get("enlisted", rem_enl)),
                    "cadre": int(uspec.get("cadre", 0)),
                    "equipment": dict(uspec.get("equipment") or {}),
                    "buffs": dict(uspec.get("buffs") or {}),
                }]
            sq_nodes = []
            for s_i, sspec in enumerate(sq_specs):
                want_e = int(sspec.get("enlisted", 0))
                want_c = int(sspec.get("cadre", 0))
                take_e = min(rem_enl, max(0, want_e))
                take_c = min(rem_cad, max(0, want_c))
                rem_enl -= take_e
                rem_cad -= take_c
                take_eq = {}
                for m, n in (sspec.get("equipment") or {}).items():
                    av = min(rem_eq.get(m, 0), max(0, int(n)))
                    if av > 0:
                        take_eq[m] = av
                        rem_eq[m] = rem_eq.get(m, 0) - av
                sq = toe.make_custom_squad(
                    sspec.get("id") or f"{pid}-sq{s_i + 1}",
                    enlisted=take_e,
                    cadre=take_c,
                    equipment=take_eq,
                    kind=sspec.get("kind", "mixed_squad"),
                    buffs=sspec.get("buffs"),
                    org=min(mean_org, toe.evaluate_squad_counter_and_caps(take_e, take_c, take_eq, sspec.get("buffs"))["org_cap_limit"]),
                )
                sq_nodes.append(toe.Node(sq.id, sq.id, "squad", squad=sq))

            if len(sq_nodes) == 1 and lvl == "squad":
                root = toe.Node(pid, uspec.get("name", pid), "squad", squad=sq_nodes[0].squad)
            else:
                root = toe.Node(pid, uspec.get("name", pid), lvl, children=sq_nodes)
            off_r = ((idx % 3) - 1) * 0.08
            off_c = ((idx // 3) - 1) * 0.08
            pos = (spawn_px[0] + off_r, spawn_px[1] + off_c)
            np_piece = toe.MapPiece(
                pid,
                uspec.get("name", pid),
                team,
                spawn_px,
                root,
                level=lvl,
                footprint_px=int(uspec.get("footprint_px", 1)),
                control_radius_km=float(uspec.get("control_radius_km", 1.5)),
                radio_channel=uspec.get("radio_channel", default_ch),
                vehicle_model=uspec.get("vehicle_model"),
                pos=pos,
            )
            np_piece.stock = {k: 0.0 for k in np_piece.stock}
            np_piece.ammo_by_type = {}
            created_pieces.append(np_piece)

        # 未分配的余量兵员/装备回填给首个编成单位（或最后单位），保持严格守恒；若目标 spec 全为 0 干部且池里有干部，则不强塞除非未写死 strict_specs
        if created_pieces and (rem_enl > 0 or sum(rem_eq.values()) > 0 or (rem_cad > 0 and not ev.payload.get("strict_cadre_allocation"))):
            tail_sq = created_pieces[-1].squads()[-1]
            tail_sq.enlisted += rem_enl
            tail_sq.enlisted_full = max(tail_sq.enlisted_full, tail_sq.enlisted)
            if not ev.payload.get("strict_cadre_allocation"):
                tail_sq.cadre += rem_cad
                tail_sq.cadre_full = max(tail_sq.cadre_full, tail_sq.cadre)
            for m, n in rem_eq.items():
                if n > 0:
                    tail_sq.equipment[m] = tail_sq.equipment.get(m, 0) + n
                    tail_sq.equipment_full[m] = tail_sq.equipment_full.get(m, 0) + n

        # 严格按人数比例分发补给三账本，并挂入 world 与指挥官直属下级
        tot_spawn_p = max(1, sum(p.persons() for p in created_pieces))
        for np_piece in created_pieces:
            frac = np_piece.persons() / float(tot_spawn_p)
            for rk, rv in pool_stock.items():
                np_piece.stock[rk] = rv * frac
            for ak, av in pool_ammo.items():
                if av * frac > 1e-9:
                    np_piece.ammo_by_type[ak] = av * frac
            np_piece.refresh_stock_cap()
            if hasattr(world, "add_piece"):
                world.add_piece(np_piece)
            else:
                world.pieces[np_piece.id] = np_piece
            if cmd is not None and np_piece.id not in cmd.subordinate_ids:
                cmd.subordinate_ids.append(np_piece.id)
            spawned_ids.append(np_piece.id)
            if sum(sq.cadre for sq in np_piece.squads()) <= 0:
                empty_cadre_ids.append(np_piece.id)

        return {
            "ok": True,
            "kind": "reorganize_echelon",
            "anchored_px": list(anchor_px),
            "spawn_px": list(spawn_px),
            "removed_sources": src_ids,
            "spawned_pieces": spawned_ids,
            "empty_cadre_pieces": empty_cadre_ids,
        }

    def to_dict(self):
        return {"seq": self._seq, "events": [e.to_dict() for e in self.events.values()]}

    @classmethod
    def from_dict(cls, d):
        if not d:
            return cls()
        return cls([ActiveEvent.from_dict(x) for x in d.get("events", [])], seq=d.get("seq", 0))


def update_war_councils(
    commanders,
    pieces,
    grid,
    dt_hours=6.0,
    radius_km=1.0,
    required_still_hours=12.0,
    required_still_turns=None,
    council_exposure=4.0,
    buff_hours=18.0,
    buff_ttl=None,
    squad_org_boost=15.0,
):
    """作战会议事件：站住不许动 + 连续指数暴露/会后高频发报暴露 + 班组破上限组织度 + AO 扩域。"""
    dt = max(0.0, float(dt_hours))
    req_hours = float(required_still_turns) * 6.0 if required_still_turns is not None else float(required_still_hours)
    eff_buff_hours = float(buff_ttl) * 6.0 if buff_ttl is not None else float(buff_hours)

    for c in commanders.values():
        if not c.alive:
            c.still_hours = 0.0
            continue
        if c.last_px == c.px:
            c.still_hours += dt
        else:
            c.still_hours = 0.0
            c.last_px = c.px

    active_zones = []
    completed = []
    boosted_squads = []
    boosted_commanders = []
    cmd_list = [c for c in commanders.values() if c.alive]
    min_prep_hours = min(6.0, req_hours * 0.5) if req_hours > 0 else 0.0

    for sup in cmd_list:
        if sup.still_hours < min_prep_hours - 1e-9:
            continue
        for sub in cmd_list:
            if sub.id == sup.id or sub.team != sup.team or sub.still_hours < min_prep_hours - 1e-9:
                continue
            is_pair = (sub.parent_commander_id == sup.id) or (
                toe.RANK_ORDER.index(sup.rank) < toe.RANK_ORDER.index(sub.rank)
                and sub.echelon != sup.echelon
            )
            if not is_pair:
                continue
            if grid.distance_km(sup.px, sub.px) <= radius_km:
                dist_px = math.hypot(sup.px[0] - sub.px[0], sup.px[1] - sub.px[1])
                center = (int(round((sup.px[0] + sub.px[0]) / 2.0)), int(round((sup.px[1] + sub.px[1]) / 2.0)))
                exp_radius_px = 2.0 + 1.5 * min(1.0, dist_px / 5.0)
                buff_radius_px = max(0.5, dist_px / 2.0)
                streak = max(sup.council_streak, sub.council_streak)
                step_scale = min(1.0, dt / 6.0) if dt > 0 else 1.0
                step_exp = council_exposure * (2.5 ** streak) * step_scale
                active_zones.append({
                    "superior": sup.id,
                    "subordinate": sub.id,
                    "team": sup.team,
                    "center": center,
                    "dist_px": round(dist_px, 2),
                    "radius_px": round(exp_radius_px, 2),
                    "buff_radius_px": round(buff_radius_px, 2),
                    "exposure": round(step_exp, 4),
                    "streak": streak,
                })
                grid.add_exposure(center, step_exp, radius_px=exp_radius_px)
                # 面对面开会实时挂正向 WAR_COUNCIL_SYNC_BUFF（保留双方原有 order_churn 而不删除），参会指挥官仍吃暴露
                for c_me, c_partner in ((sup, sub), (sub, sup)):
                    if not hasattr(c_me, "meta") or c_me.meta is None:
                        c_me.meta = {}
                    c_me.meta["council_partner_id"] = c_partner.id
                    c_me.meta["council_radius_km"] = float(radius_km)
                    c_me.war_council_sync_hours = max(float(getattr(c_me, "war_council_sync_hours", 0.0)), eff_buff_hours)
                    grid.add_exposure(c_me.px, step_exp * 0.5, radius_px=0)

                if sup.still_hours + 1e-9 >= req_hours and sub.still_hours + 1e-9 >= req_hours:
                    sup.still_hours = 0.0
                    sub.still_hours = 0.0
                    sup.council_streak += 1
                    sub.council_streak += 1
                    sup.council_cooldown_hours = 24.0
                    sub.council_cooldown_hours = 24.0
                    sup.meta["council_completed"] = True
                    sub.meta["council_completed"] = True
                    post_val = 8.0 * (1.8 ** (max(sup.council_streak, sub.council_streak) - 1))
                    for c_join in (sup, sub):
                        c_join.post_council_exposure_hours = max(c_join.post_council_exposure_hours, 12.0)
                        c_join.post_council_exposure_val = max(c_join.post_council_exposure_val, post_val)

                    sub.briefing_buff_hours = max(sub.briefing_buff_hours, eff_buff_hours)
                    sup.war_council_sync_hours = max(float(getattr(sup, "war_council_sync_hours", 0.0)), eff_buff_hours)
                    sub.war_council_sync_hours = max(float(getattr(sub, "war_council_sync_hours", 0.0)), eff_buff_hours)
                    completed.append({
                        "superior": sup.id,
                        "subordinate": sub.id,
                        "buff_hours": sub.briefing_buff_hours,
                        "streak": max(sup.council_streak, sub.council_streak),
                        "post_council_exposure": round(post_val, 2),
                    })
                    for p in pieces.values():
                        if p.team == sup.team and math.hypot(p.px[0] - center[0], p.px[1] - center[1]) <= buff_radius_px:
                            for sq in p.squads():
                                sq.boost_org_overcap(squad_org_boost, max_org=120.0)
                            boosted_squads.append(p.id)
                    for oc in cmd_list:
                        if oc.team == sup.team and math.hypot(oc.px[0] - center[0], oc.px[1] - center[1]) <= buff_radius_px:
                            oc.council_ao_hours = max(oc.council_ao_hours, eff_buff_hours)
                            oc.post_council_exposure_hours = max(oc.post_council_exposure_hours, 12.0)
                            oc.post_council_exposure_val = max(oc.post_council_exposure_val, post_val)
                            boosted_commanders.append(oc.id)

    piece_bonus = {}
    if active_zones:
        for p in pieces.values():
            for z in active_zones:
                if math.hypot(p.px[0] - z["center"][0], p.px[1] - z["center"][1]) <= z["radius_px"]:
                    piece_bonus[p.id] = piece_bonus.get(p.id, 0.0) + z["exposure"]
                    grid.add_exposure(p.px, z["exposure"], radius_px=0)

    return {
        "active_zones": active_zones,
        "completed": completed,
        "piece_bonus": piece_bonus,
        "boosted_squads": sorted(set(boosted_squads)),
        "boosted_commanders": sorted(set(boosted_commanders)),
    }


def evaluate_events(world, dt_hours):
    """事件总线统一入口：推进事件调度表、单位 Debuff TTL、超载 15min 随机故障与作战会议。"""
    for p in world.pieces.values():
        if hasattr(p, "tick_debuffs"):
            p.tick_debuffs(dt_hours)
    completed = world.event_manager.tick_events(world, dt_hours) if hasattr(world, "event_manager") else []
    overload_faults = mod_engine.tick_overload_faults(world, dt_hours)
    wc = update_war_councils(world.commanders, world.pieces, world.grid, dt_hours=dt_hours) if world.commanders else {
        "active_zones": [], "completed": [], "piece_bonus": {}, "boosted_squads": [], "boosted_commanders": [],
    }
    return {"completed_events": completed, "war_councils": wc, "overload_faults": overload_faults}
