# -*- coding: utf-8 -*-
"""V2 总裁决器：把地形/后勤/编制/战斗/航空/炮兵/迷雾串成确定性的回合流水线。

一回合的顺序（全部确定性，无随机、无网络、无 LLM）：
  1 无线电静默切换 → 2 航空 1 小时 Sub-tick → 3 炮兵任务（烟/照明改写栅格、弹幕场）
  → 4 行军（受弹幕/扫射迟滞、油料、地形坡度）→ 5 后勤配送（士兵去取，守恒）
  → 6 打击落地（炮/空 → 各班账本）→ 7 地面交战 → 8 非战斗损耗与消耗
  → 9 整补复原（有装无人原地补兵，骨干决定速度）→ 10 总部混编 → 11 环境衰减
  → 12 迷雾视图 + 客观压力面板 + 指纹

**压力面板只给客观量**（组织度、骨干率、补给比、火力封锁度、制空状态、退路封锁度、
被迫撤收的预期遗留率）。是否撤退、是否死战由指挥与推演层决定——内核不替人性写剧本。
"""

from __future__ import annotations

import copy
import hashlib
import json
import math
import os

import numpy as np

from . import artillery as ar
from . import autonomy_v3, baguwen_v3, combat, command_v2, events, recon_fog
from . import logistics as lg
from . import terrain_field as tf
from . import toe, ww2_air as air

MIN_TICK_HOURS = 5.0 / 60.0
ROLE_DEFAULT_HOURS = {
    "squad": 5.0 / 60.0,
    "platoon": 0.25,
    "company": 0.5,
    "battalion": 1.0,
    "regiment": 2.0,
    "brigade": 4.0,
    "division": 6.0,
}

BASE_SPEED_KMH = {"foot": 3.5, "wheeled": 12.0, "tracked": 12.0}
LONG_MARCH_HOURS = 8.0
LONG_MARCH_FACTOR = {"foot": 0.35, "wheeled": 0.30, "tracked": 0.30}
FOOD_T_PER_PERSON_DAY = 0.002
FUEL_T_PER_VEHICLE_KM = 0.004
FUEL_T_PER_VEHICLE_IDLE_H = 0.0005
AMMO_T_PER_POINT = 0.01
ENGAGE_RANGE_KM = 3.0
OCCUPY_RADIUS_PX = 2
AA_RADIUS_KM = 8.0
VEHICLE_MODELS = ("truck", "tank_medium", "tank_heavy", "halftrack", "howitzer_105", "howitzer_152",
                  "at_gun_50", "flak_88")
DEPLETED_RATIO = 0.6
RAY_COUNT = 16
RAY_KM = 8.0


class World:
    def __init__(self, grid, turn=0, hours=6.0, night=False, clock_hours=0.0, role_echelon="division", meta=None):
        self.grid = grid
        self.turn = int(turn)
        self.hours = max(MIN_TICK_HOURS, float(hours))
        self.clock_hours = float(clock_hours)
        self.role_echelon = role_echelon
        self.night = bool(night)
        self.pieces = {}
        self.depots = {}
        self.batteries = {}
        self.wings = {}
        self.commanders = {}
        self.ao_zones = {}
        self.exposure_tracker = combat.ExposureTracker()
        self.event_manager = events.EventManager()
        self.last_fire_fields = None
        self.intercepted_channels = {}
        self.meta = dict(meta or {})

    def set_role(self, role_echelon, custom_hours=None):
        """切换当前扮演的指挥官身份，决定每次看清沙盘/重下令的回合物理跨度（最小 15 分钟）。"""
        self.role_echelon = role_echelon
        h = custom_hours if custom_hours is not None else ROLE_DEFAULT_HOURS.get(role_echelon, 6.0)
        self.hours = max(MIN_TICK_HOURS, float(h))
        return self.hours

    # ---- 便捷 ----
    def teams(self):
        return sorted({p.team for p in self.pieces.values()})

    def enemies_of(self, team):
        return [t for t in self.teams() if t != team]

    def add_piece(self, piece):
        self.pieces[piece.id] = piece
        return piece

    def add_depot(self, depot):
        self.depots[depot.id] = depot
        return depot

    def add_battery(self, battery):
        self.batteries[battery.id] = battery
        return battery

    def add_wing(self, wing):
        self.wings[wing.id] = wing
        return wing

    def add_commander(self, cmdr):
        self.commanders[cmdr.id] = cmdr
        return cmdr

    def add_zone(self, zone):
        ok, reason = command_v2.register_zone(self.ao_zones, zone, self.commanders)
        return ok, reason

    # ---- 持久化 ----
    def save(self, directory):
        os.makedirs(directory, exist_ok=True)
        self.grid.save_npz(os.path.join(directory, "grid.npz"))
        data = {
            "turn": self.turn,
            "hours": self.hours,
            "clock_hours": self.clock_hours,
            "role_echelon": self.role_echelon,
            "night": self.night,
            "pieces": {k: v.to_dict() for k, v in self.pieces.items()},
            "depots": {k: v.to_dict() for k, v in self.depots.items()},
            "batteries": {k: v.to_dict() for k, v in self.batteries.items()},
            "wings": {k: v.to_dict() for k, v in self.wings.items()},
            "commanders": {k: v.to_dict() for k, v in self.commanders.items()},
            "ao_zones": {k: v.to_dict() for k, v in self.ao_zones.items()},
            "exposure_tracker": self.exposure_tracker.to_dict(),
            "event_manager": self.event_manager.to_dict(),
            "intercepted_channels": self.intercepted_channels,
            "meta": dict(self.meta),
        }
        with open(os.path.join(directory, "world.json"), "w", encoding="utf-8") as fh:
            json.dump(data, fh, ensure_ascii=False, indent=1)

    @classmethod
    def load(cls, directory):
        grid = tf.Grid.load_npz(os.path.join(directory, "grid.npz"))
        with open(os.path.join(directory, "world.json"), "r", encoding="utf-8-sig") as fh:
            d = json.load(fh)
        w = cls(
            grid,
            d["turn"],
            d["hours"],
            d["night"],
            clock_hours=d.get("clock_hours", 0.0),
            role_echelon=d.get("role_echelon", "division"),
            meta=d.get("meta"),
        )
        for k, v in d["pieces"].items():
            w.pieces[k] = toe.MapPiece.from_dict(v)
        for k, v in d["depots"].items():
            w.depots[k] = lg.Depot.from_dict(v)
        for k, v in d["batteries"].items():
            w.batteries[k] = ar.Battery.from_dict(v)
        for k, v in d["wings"].items():
            w.wings[k] = air.AirWing.from_dict(v)
        for k, v in (d.get("commanders") or {}).items():
            w.commanders[k] = toe.Commander.from_dict(v)
        for k, v in (d.get("ao_zones") or {}).items():
            w.ao_zones[k] = command_v2.OperationalZone.from_dict(v)
        if "exposure_tracker" in d:
            w.exposure_tracker = combat.ExposureTracker.from_dict(d["exposure_tracker"])
        if "event_manager" in d:
            w.event_manager = events.EventManager.from_dict(d["event_manager"])
        w.intercepted_channels = {
            str(t): {str(ch): float(ttl) for ch, ttl in (chs or {}).items()}
            for t, chs in (d.get("intercepted_channels") or {}).items()
        }
        return w


# ==========================================
# 🧰 内部工具
# ==========================================
def _px(v):
    return (int(round(v[0])), int(round(v[1])))


HIT_POINTS = combat.HIT_POINTS


def _distribute(squads, amount, key=""):
    return combat.distribute_squads(squads, amount, key)


def _vehicle_count(piece):
    return sum(n for s in piece.squads() for m, n in s.equipment.items() if m in VEHICLE_MODELS)


def _piece_is_armored(piece):
    return (piece.vehicle_model in ("tank_medium", "tank_heavy", "halftrack")) or any(
        s.equipment.get(m, 0) > 0 for s in piece.squads() for m in ("tank_medium", "tank_heavy", "halftrack")
    )


def _primary_weapon_of(piece):
    if getattr(piece, "deployed_weapon", None):
        return piece.deployed_weapon
    best_m, best_f = None, -1.0
    for sq in piece.squads():
        for m, n in sq.equipment.items():
            if n > 0 and m in toe.EQUIPMENT and toe.EQUIPMENT[m].get("fire"):
                f_tot = sum(toe.EQUIPMENT[m]["fire"].values()) * n
                if f_tot > best_f:
                    best_f = f_tot
                    best_m = m
    return best_m


def _displace_piece(world, piece, from_pos=None, flank=False):
    """强制击退/退格（Displacement）：将战败或被碾压棋子向背离敌方的最近可通行像素退开 1 格，打断事件。"""
    g = world.grid
    world.event_manager.cancel_hold_still_on_move(piece.id, world=world)
    piece.operated_by = None
    cur = piece.px
    cost_l = tf.cost_layer(g, piece.mobility())
    H, W = g.shape
    away_r, away_c = 0.0, 0.0
    if from_pos is not None:
        away_r = float(cur[0]) - float(from_pos[0])
        away_c = float(cur[1]) - float(from_pos[1])
    norm = math.hypot(away_r, away_c)
    if norm <= 1e-6:
        away_r, away_c = (-1.0 if piece.team == "red" else 1.0), 0.0
    else:
        away_r /= norm
        away_c /= norm

    enemy_cells = {
        ep.px for ep in world.pieces.values()
        if ep.team != piece.team and (ep.persons() > 0 or ep.vehicle_model)
    }
    candidates = []
    for dr in (-1, 0, 1):
        for dc in (-1, 0, 1):
            if dr == 0 and dc == 0:
                continue
            nr, nc = cur[0] + dr, cur[1] + dc
            if not (0 <= nr < H and 0 <= nc < W):
                continue
            if not math.isfinite(float(cost_l[nr, nc])):
                continue
            d_norm = math.hypot(dr, dc)
            dot = (dr * away_r + dc * away_c) / d_norm
            no_enemy = 1 if (nr, nc) not in enemy_cells else 0
            dir_score = -abs(dot - 0.35) if flank else dot
            candidates.append((no_enemy, dir_score, (nr, nc)))
    if not candidates:
        return list(cur)
    candidates.sort(key=lambda x: (-x[0], -x[1]))
    nxt = candidates[0][2]
    if piece.is_heavy_emplacement:
        piece.is_heavy_emplacement = False
        piece.unit_state = "mobile"
    piece.pos = (float(nxt[0]), float(nxt[1]))
    piece.moved_last_turn = True
    for ch in world.pieces.values():
        if ch.attached_to == piece.id:
            ch.pos = tuple(piece.pos)
    for c in world.commanders.values():
        if c.attached_to == piece.id or (c.cadre_piece_id == piece.id and not c.attached_to):
            c.px = nxt
    return list(nxt)


def _capture_hq_channel(world, winner_team, loser_piece, reason):
    """缴获/打垮敌方指挥或通信电台单位时破译其正在使用的无线电信道（持续 12 小时）。"""
    has_sig = any(
        s.kind in ("hq_squad", "signal_squad")
        or s.has_buff("command")
        or s.has_buff("signal")
        or s.equipment.get("hq_radio", 0) > 0
        for s in loser_piece.squads()
    )
    if not has_sig:
        return None
    ch = getattr(loser_piece, "radio_channel", "A")
    world.intercepted_channels.setdefault(winner_team, {})[ch] = 12.0
    return {"winner_team": winner_team, "from_piece": loser_piece.id, "channel": ch, "ttl_hours": 12.0, "reason": reason}


def _control_mask(world, victim_team):
    """敌方对 victim_team 形成的控制区（敌军控制半径圈的并集）。"""
    g = world.grid
    mask = np.zeros(g.shape, bool)
    for p in world.pieces.values():
        if p.team != victim_team:
            mask |= tf.disk_mask(g.shape, p.px, g.km_to_px(p.control_radius_km))
    return mask


def _occupation_mask(world, team, mover=None):
    """敌军占领与 ZOC 阻挡掩膜：受压制/溃退敌军丧失 ZOC，优势装甲单位可直接碾穿残兵。"""
    g = world.grid
    mask = np.zeros(g.shape, bool)
    mover_has_armor = mover is not None and _piece_is_armored(mover)
    mover_power = sum(sum(s.power().values()) for s in mover.squads()) if mover is not None else 0.0
    for p in world.pieces.values():
        if p.team == team or (p.persons() <= 0 and not p.vehicle_model):
            continue
        suppressed = any(k.startswith("supp:") for k in g.cell_modifiers.get(p.px, {}))
        routed = p.mean_org() < 25.0
        if suppressed or routed:
            if mover_has_armor:
                continue
            mask[p.px] = True
            continue
        if mover_has_armor:
            ep_power = sum(sum(s.power().values()) for s in p.squads())
            if mover_power >= 2.5 * max(1.0, ep_power) or p.persons() <= 15:
                continue
        mask |= tf.disk_mask(g.shape, p.px, OCCUPY_RADIUS_PX)
    return mask


def _aa_sites(world, busy_aa_ids=None):
    """防空火力点扫描：88 炮被拖拽行军、上回合刚移动、受开火锁定或正忙于地面交战时不能防空。"""
    sites = []
    busy = set(busy_aa_ids or ())
    for p in world.pieces.values():
        if p.id in busy or p.unit_state == "carried" or p.moved_last_turn:
            continue
        ev_mods = world.event_manager.piece_modifiers(p, world=world)
        if ev_mods["forbid_fire"]:
            continue
        if any(e.kind == "fire_weapon" and e.payload.get("weapon") == "flak_88" for e in world.event_manager.active_for(p.id)):
            continue
        if p.deployed_weapon == "flak_88":
            if p.unit_state != "deployed":
                continue
            has_op = any(
                world.event_manager.piece_modifiers(op)["operating_target"] == p.id
                for op in world.pieces.values()
            ) or (p.operated_by and p.operated_by in world.pieces and world.pieces[p.operated_by].px == p.px)
            if not has_op and p.persons() <= 0:
                continue
        strength = sum(s.equipment.get("flak_88", 0) * (0.3 + 0.7 * s.org / 100.0) for s in p.squads())
        if strength > 0:
            sites.append({"team": p.team, "px": p.px, "radius_px": world.grid.km_to_px(AA_RADIUS_KM),
                          "strength": strength, "piece": p.id})
    return sites


def _engineer_assist(piece):
    return combat.engineer_assist_of(piece.squads())


def _fuel_factor(piece):
    if piece.mobility() == "foot":
        return 1.0
    cap = piece.stock_cap.get("fuel", 0.0)
    if cap <= 0:
        return 1.0
    return max(0.0, min(1.0, (piece.stock.get("fuel", 0.0) / cap) / 0.1))


def _observed(world, team, target_px, photos):
    g = world.grid
    tpx = (int(target_px[0]), int(target_px[1]))
    vis = float(tf.WEATHER_VIS[g.weather[tpx]] * g.visibility_mod[tpx])
    if world.night:
        vis *= (tf.NIGHT_BASE_VIS + (1.0 - tf.NIGHT_BASE_VIS) * float(g.illumination[tpx]))
    for p in world.pieces.values():
        if p.team == team and g.distance_km(p.px, target_px) <= recon_fog.observe_radius_km(p) * vis:
            return True
    for ph in photos:
        if ph["team"] == team and math.hypot(ph["px"][0] - target_px[0], ph["px"][1] - target_px[1]) <= ph["radius_px"]:
            return True
    return False


def _apply_strike_to_pieces(world, strike, report, strike_logs=None):
    """将打击包按覆盖区域分摊至像素容器，由容器统一结算无差别误伤、破片落点与 Depot 殉爆。"""
    g = world.grid
    center = _px(strike["px"])
    radius = max(1.0, float(strike["radius_px"]))
    area = math.pi * radius * radius
    target_cells = set()
    for p in world.pieces.values():
        if (p.persons() > 0 or p.vehicle_model) and math.hypot(p.px[0] - center[0], p.px[1] - center[1]) <= radius:
            target_cells.add(p.px)
    for dep in world.depots.values():
        if math.hypot(dep.px[0] - center[0], dep.px[1] - center[1]) <= radius:
            target_cells.add(dep.px)
    for rc in sorted(target_cells):
        pcs_here = [p for p in world.pieces.values() if p.px == rc]
        fp = max([p.footprint_px for p in pcs_here], default=4)
        share = min(1.0, fp / max(1.0, area))
        pts = float(strike["intensity"]) * share
        sr = combat.resolve_cell_strike(
            world,
            rc,
            strike["team"],
            {strike["damage_type"]: pts},
            dist_m=float(strike.get("dist_m", 1000.0)),
            weapon_model=strike.get("weapon_model"),
            acc_bonus=float(strike.get("acc_bonus", 0.0)),
            fac_guided=bool(strike.get("fac_guided", False)),
            aspect_target_pos=None,
            seed_tag=f"{world.turn}|{strike.get('source')}|{strike['damage_type']}|{center}",
            casualties_out=report,
        )
        if strike_logs is not None:
            strike_logs.append(sr)


# ==========================================
# 🔁 回合流水线
# ==========================================
def run_turn(world, orders=None):
    orders = orders or {}
    g = world.grid
    warnings = []
    if "role" in orders:
        world.set_role(orders["role"], orders.get("hours"))
    elif "hours" in orders:
        world.hours = max(MIN_TICK_HOURS, float(orders["hours"]))
    if "night" in orders:
        world.night = bool(orders["night"])
    hours = max(MIN_TICK_HOURS, float(world.hours))
    report = {
        "turn": world.turn + 1,
        "hours": hours,
        "clock_hours": round(world.clock_hours + hours, 3),
        "role": world.role_echelon,
        "night": world.night,
        "warnings": warnings,
    }

    # 0 推进开火暴露残留（物理小时制，不随身份 turn_hours 缩放）
    world.exposure_tracker.tick(hours)
    for b in world.batteries.values():
        b.tick_exposure(hours)

    # 1 无线电静默与 ABCDEFG 信道切换
    for eid, flag in (orders.get("radio_silence") or {}).items():
        if eid in world.pieces:
            world.pieces[eid].radio_silence = bool(flag)
        if eid in world.commanders:
            world.commanders[eid].radio_silence = bool(flag)
    for eid, ch in (orders.get("radio_channel") or {}).items():
        if ch not in toe.RADIO_CHANNELS:
            continue
        if eid in world.pieces:
            world.pieces[eid].radio_channel = ch
        if eid in world.commanders:
            world.commanders[eid].radio_channel = ch

    # 1.5 指挥链顺位继承、纯 AO 区域注册（含地上/移动电台与静默缩水校验）、铺设电缆与像素仓库转移
    succession = toe.resolve_succession(world.commanders, world.pieces) if world.commanders else []
    trimmed_zones = (
        command_v2.enforce_zone_quotas(world.ao_zones, world.commanders, pieces=world.pieces, grid=g)
        if world.ao_zones
        else []
    )
    ao_receipts = []
    for zspec in orders.get("ao_zones") or []:
        cmdr = world.commanders.get(zspec["commander"])
        cells = command_v2.build_ao_cells(zspec, g, world.pieces)
        z = command_v2.OperationalZone(
            zspec["id"], zspec["commander"],
            zspec.get("team") or (cmdr.team if cmdr else "red"),
            zspec.get("echelon") or (cmdr.echelon if cmdr else "company"),
            cells,
            frontlines=zspec.get("frontlines"),
        )
        ok, reason = command_v2.register_zone(
            world.ao_zones, z, world.commanders, current_turn=report["turn"], pieces=world.pieces, grid=g
        )
        ao_receipts.append({"zone_id": z.zone_id, "accepted": ok, "reason": reason,
                            "activate_turn": z.activate_turn, "area_px": z.area_px})

    for idx_fl, fl in enumerate(orders.get("typed_frontlines") or []):
        if not isinstance(fl, dict) or not fl.get("points_px"):
            continue
        cid_fl = fl.get("commander_id") or fl.get("commander")
        cmd_fl = world.commanders.get(cid_fl) if cid_fl else None
        team_fl = fl.get("team") or (cmd_fl.team if cmd_fl else "red")
        lid_fl = str(fl.get("line_id") or f"fl-{report['turn']}-{idx_fl}")
        g.write_typed_frontline(
            lid_fl, team_fl, fl["points_px"],
            unit_type_mask=int(fl.get("unit_type_mask", tf.LINE_MASK_ALL)),
            attraction_weight=float(fl.get("attraction_weight", 1.0)),
            front_normal=fl.get("front_normal"),
            commander_id=cid_fl,
            zone_id=fl.get("zone_id"),
            spread_radius_px=int(fl.get("spread_radius_px", 4)),
        )

    wire_events = []
    for lw in orders.get("lay_wire") or []:
        p = world.pieces.get(lw.get("piece"))
        if p is None:
            continue
        pts = [_px(pt) for pt in (lw.get("path") or [])]
        if len(pts) < 2:
            continue
        avail_spools = sum(sq.equipment.get("field_wire", 0) for sq in p.squads())
        if avail_spools <= 0:
            wire_events.append({"piece": p.id, "ok": False, "reason": "no_field_wire"})
            continue
        max_edges = int(avail_spools * (2000.0 / g.cell_m))
        added = g.lay_wire(pts[:max_edges + 1], p.team)
        wire_events.append({"piece": p.id, "ok": True, "added_edges": added})

    stash_events = []
    for st in orders.get("stash_transfers") or []:
        p = world.pieces.get(st["piece"])
        if p is None:
            continue
        if st.get("op") == "take":
            got = g.stash_take(p.px, resources=st.get("resources"), equipment=st.get("equipment"))
            for rk, rv in got["resources"].items():
                if rk in p.stock:
                    p.stock[rk] = min(p.stock_cap.get(rk, 1e9), p.stock[rk] + rv)
            if got["equipment"]:
                sqs = p.squads()
                if sqs:
                    target_sq = next((s for s in sqs if s.persons > 0), sqs[0])
                    for eq, n in got["equipment"].items():
                        target_sq.equipment[eq] = target_sq.equipment.get(eq, 0) + n
            stash_events.append({"piece": p.id, "op": "take", "px": list(p.px), "got": got})
        elif st.get("op") == "put":
            res_put = {}
            for rk, rv in (st.get("resources") or {}).items():
                avail = p.stock.get(rk, 0.0)
                give = min(avail, max(0.0, float(rv)))
                if give > 0:
                    p.stock[rk] -= give
                    res_put[rk] = give
            g.stash_put(
                p.px,
                resources=res_put,
                equipment=st.get("equipment"),
                owner_team=p.team,
                radio_channel=st.get("radio_channel"),
            )
            stash_events.append({"piece": p.id, "op": "put", "px": list(p.px), "put": res_put})

    report["command_v2"] = {
        "succession": succession,
        "trimmed_zones": trimmed_zones,
        "ao_receipts": ao_receipts,
        "wire_events": wire_events,
        "stash_events": stash_events,
    }

    # 1.6 像素容器通用 Buff、单位挂靠/解挂、战地拆班与统一事件下达
    for cm in orders.get("cell_modifiers") or []:
        op = cm.get("op", "set")
        if op == "remove":
            g.remove_cell_modifier(_px(cm["px"]), cm["key"])
        else:
            g.set_cell_modifier(_px(cm["px"]), cm["key"], cm.get("modifiers", {}), ttl_hours=cm.get("ttl_hours"))

    for tfo in orders.get("terrain_features") or []:
        op = tfo.get("op", "add")
        if op == "remove":
            g.remove_terrain_feature(_px(tfo["px"]), tfo["feature"], fallback_terrain=tfo.get("fallback_terrain"))
        else:
            g.add_terrain_feature(
                _px(tfo["px"]), tfo["feature"],
                modifiers=tfo.get("modifiers"),
                ttl_hours=tfo.get("ttl_hours"),
                sync_terrain=bool(tfo.get("sync_terrain", True)),
                terrain_code=tfo.get("terrain_code"),
            )

    for att in orders.get("attach") or []:
        c_id = att.get("child")
        parent = world.pieces.get(att.get("parent"))
        if c_id in world.commanders and parent is not None:
            cmd = world.commanders[c_id]
            if cmd.team == parent.team:
                cmd.attached_to = parent.id
                cmd.px = parent.px
            continue
        child = world.pieces.get(c_id)
        if child is not None and parent is not None and child.id != parent.id and child.team == parent.team:
            child.attach_to(parent.id)
            child.pos = tuple(parent.pos)
    for pid_det in orders.get("detach") or []:
        if pid_det in world.commanders:
            world.commanders[pid_det].attached_to = None
            continue
        child = world.pieces.get(pid_det)
        if child is not None and child.attached_to is not None:
            child.detach("mobile")

    for pid_r, r_val in (orders.get("roe") or {}).items():
        if pid_r in world.pieces and r_val in toe.VALID_ROE:
            world.pieces[pid_r].roe = r_val

    for db in orders.get("debuffs") or []:
        tgt = world.pieces.get(db.get("piece"))
        if tgt is None:
            continue
        if db.get("op", "add") == "remove":
            tgt.remove_debuff(db["key"])
        else:
            tgt.add_debuff(db["key"], db.get("spec", {}))

    for ct in orders.get("cargo_transfer") or []:
        vp = world.pieces.get(ct.get("vehicle"))
        if vp is None:
            continue
        direction = ct.get("direction", "load")  # "load": cell_stash -> cargo; "unload": cargo -> cell_stash
        res_req = ct.get("resources") or {}
        eq_req = ct.get("equipment") or {}
        if direction == "load":
            taken = g.stash_take(vp.px, resources=res_req, equipment=eq_req)
            for k, v in taken["resources"].items():
                vp.cargo["resources"][k] = vp.cargo["resources"].get(k, 0.0) + v
            for k, v in taken["equipment"].items():
                vp.cargo["equipment"][k] = vp.cargo["equipment"].get(k, 0) + v
        elif direction == "unload":
            put_res, put_eq = {}, {}
            for k, v in res_req.items():
                avail = float(vp.cargo["resources"].get(k, 0.0))
                take = min(avail, max(0.0, float(v)))
                if take > 1e-9:
                    vp.cargo["resources"][k] = avail - take
                    if vp.cargo["resources"][k] <= 1e-9:
                        vp.cargo["resources"].pop(k, None)
                    put_res[k] = take
            for k, v in eq_req.items():
                avail = int(vp.cargo["equipment"].get(k, 0))
                take = min(avail, max(0, int(v)))
                if take > 0:
                    vp.cargo["equipment"][k] = avail - take
                    if vp.cargo["equipment"][k] <= 0:
                        vp.cargo["equipment"].pop(k, None)
                    put_eq[k] = take
            if put_res or put_eq:
                g.stash_put(vp.px, resources=put_res, equipment=put_eq, owner_team=vp.team)

    explicit_facings = orders.get("facing") or {}
    for pid_f, deg in explicit_facings.items():
        pf = world.pieces.get(pid_f)
        if pf is None:
            continue
        if world.event_manager.piece_modifiers(pf, world=world)["forbid_move"]:
            warnings.append(f"{pid_f} 处于禁止移动锁定，无法改变朝向")
            continue
        pf.set_facing(deg)

    split_receipts = []
    for sp in orders.get("split_squad") or []:
        src_p = world.pieces.get(sp.get("piece"))
        if src_p is None:
            continue
        sq = next((s for s in src_p.squads() if s.id == sp.get("squad_id")), None)
        if sq is None and src_p.squads():
            sq = src_p.squads()[0]
        if sq is None:
            continue
        new_pid = sp.get("new_piece_id") or f"{src_p.id}-det"
        det_piece = src_p.split_squad_to_piece(
            sq.id,
            sp.get("enlisted", max(1, sq.enlisted // 2)),
            take_cadre=sp.get("cadre", 0),
            take_equipment=sp.get("equipment"),
            new_piece_id=new_pid,
            new_squad_id=sp.get("new_squad_id") or f"{new_pid}-sq",
            new_name=sp.get("new_name", new_pid),
            offset_px=tuple(sp.get("offset_px", (0.0, 0.0))),
        )
        world.add_piece(det_piece)
        for cmd in world.commanders.values():
            if cmd.team == src_p.team and src_p.id in cmd.subordinate_ids and det_piece.id not in cmd.subordinate_ids:
                cmd.subordinate_ids.append(det_piece.id)
        if toe._piece_is_empty(src_p):
            toe._prune_piece_from_world(world, src_p.id)
        split_receipts.append({"ok": True, "from_piece": src_p.id, "new_piece": det_piece.id, "persons": det_piece.persons()})
    report["splits"] = split_receipts

    reorg_receipts = []
    for r_spec in orders.get("reorganize") or []:
        res = toe.commander_reorganize(
            world,
            r_spec.get("commander"),
            r_spec,
            enforce_link=bool(r_spec.get("enforce_link", False)),
        )
        reorg_receipts.append(res)
    report["reorganizations"] = reorg_receipts

    event_receipts = []
    for ev_spec in orders.get("events") or []:
        res = world.event_manager.start_event(
            world,
            ev_spec["kind"],
            ev_spec["actor"],
            payload=ev_spec.get("payload"),
            duration_hours=ev_spec.get("duration_h"),
            modifiers=ev_spec.get("modifiers"),
        )
        event_receipts.append({"kind": ev_spec["kind"], "actor": ev_spec["actor"], **res})
    report["event_receipts"] = event_receipts

    def blocked_by_silence(req):
        p = world.pieces.get(req) if req else None
        return p is not None and not recon_fog.can_request_support(p)

    # 2 航空
    air_orders = []
    air_order_receipts = []
    for o in orders.get("air") or []:
        if blocked_by_silence(o.get("requested_by")):
            warnings.append(f"空中任务 {o['wing']} 被拒：请求方 {o['requested_by']} 处于无线电静默")
            continue
        ao = {"wing": o["wing"], "mission": o["mission"], "target": _px(o["target"]),
              "radius_px": float(o.get("radius_px", 8))}
        if "resources" in o:
            ao["resources"] = o["resources"]
        if "piece_id" in o:
            ao["piece_id"] = o["piece_id"]
        air_orders.append(ao)
    for m in orders.get("air_missions") or []:
        w = world.wings.get(m["wing"])
        if w is None:
            air_order_receipts.append({"accepted": False, "reason": "unknown_wing", "wing": m["wing"]})
            continue
        rec = air.assign_air_mission(g, w, {"px": _px(m["target"]), "radius_km": m.get("radius_km", 8.0),
                                            "mission": m.get("mission", "cas"),
                                            "loiter_h": m.get("loiter_h", w.loiter_h)}, list(world.pieces.values()))
        air_order_receipts.append(rec)
        if rec["accepted"]:
            ao = {"wing": w.id, "mission": m.get("mission", "cas"), "target": _px(m["target"]),
                  "radius_px": float(m.get("radius_px", 8))}
            if "resources" in m:
                ao["resources"] = m["resources"]
            if "piece_id" in m:
                ao["piece_id"] = m["piece_id"]
            air_orders.append(ao)
    report["air_order_receipts"] = air_order_receipts
    def _air_mult(px, team):
        if not world.ao_zones:
            return 1.0
        return command_v2.compute_operational_ao_buff(px, team, world.ao_zones, world.commanders, current_turn=report["turn"])["air_mult"]
    busy_aa_ids = {
        e_id
        for e in (orders.get("engagements") or [])
        for e_id in (e.get("attacker"), e.get("defender"))
        if e_id
    }
    air_res = air.run_air(g, list(world.wings.values()), air_orders, _aa_sites(world, busy_aa_ids=busy_aa_ids),
                          hours, world.night, air_mult_fn=_air_mult if world.ao_zones else None,
                          pieces=list(world.pieces.values()))
    for drop in air_res.airdrops:
        g.stash_put(drop["px"], resources=drop["resources"], owner_team=drop["team"])
    paradrop_events = []
    for pd_op in air_res.paradrops:
        para_p = world.pieces.get(pd_op.get("piece_id"))
        if para_p is not None and para_p.team == pd_op["team"]:
            para_p.detach("mobile")
            para_p.pos = (float(pd_op["px"][0]), float(pd_op["px"][1]))
            if pd_op["eff_ratio"] < 0.99:
                for sq in para_p.squads():
                    sq.org = max(10.0, sq.org * (0.65 + 0.35 * pd_op["eff_ratio"]))
            for et in world.enemies_of(para_p.team):
                g.cut_wire_at(para_p.px, radius_px=0, team=et)
            paradrop_events.append({"piece": para_p.id, "px": list(para_p.px), "wing": pd_op["wing"],
                                    "eff_ratio": pd_op["eff_ratio"]})
    report["air"] = {"log": air_res.log, "losses": air_res.losses, "events": air_res.events,
                     "photos": air_res.photos, "airdrops": air_res.airdrops, "paradrops": paradrop_events}
    ff = ar.FireFields(g.shape)
    for team in air_res.fields.hazard:
        mask = air_res.fields.hazard[team] > 0
        ff.add(team, mask, air_res.fields.hazard[team][mask], air_res.fields.mult[team][mask])
    strikes = list(air_res.strikes)

    # 3 炮兵（受营级 AO 炮兵精度 Buff 加成 + 支持徐进弹幕）
    arty_report = []
    for o in orders.get("artillery") or []:
        bat = world.batteries.get(o["battery"])
        if bat is None:
            warnings.append(f"未知炮兵连 {o['battery']}")
            continue
        if blocked_by_silence(o.get("requested_by")):
            warnings.append(f"炮击 {bat.id} 被拒：请求方 {o['requested_by']} 处于无线电静默")
            continue
        tgt = _px(o.get("target") or o.get("end_px") or bat.px)
        observed = o.get("observed")
        if observed is None:
            observed = _observed(world, bat.team, tgt, air_res.photos)
        acc_mult = (command_v2.compute_operational_ao_buff(tgt, bat.team, world.ao_zones, world.commanders, current_turn=report["turn"])["artillery_mult"]
                    if world.ao_zones else 1.0)
        if o.get("rolling_barrage"):
            s_px = _px(o.get("start_px", tgt))
            e_px = _px(o.get("end_px", tgt))
            res = ar.rolling_barrage_mission(
                g, bat, o.get("shell", "he"), s_px, e_px,
                steps=int(o.get("steps", 3)),
                radius_px=float(o.get("radius_px", 3)),
                hours=float(o.get("hours", hours)),
                observed=observed, night=world.night, accuracy_mult=acc_mult,
            )
            arty_report.append({k: v for k, v in res.items() if k not in ("strikes", "barrages")}
                               | {"observed": observed})
            strikes.extend(res.get("strikes") or [])
            for bg in res.get("barrages") or []:
                for victim in world.enemies_of(bat.team):
                    ff.add_barrage(g, bg, victim)
        else:
            res = ar.fire_mission(g, bat, o.get("shell", "he"), tgt, float(o.get("radius_px", 6)),
                                  float(o.get("hours", hours)), observed, world.night, accuracy_mult=acc_mult)
            arty_report.append({k: v for k, v in res.items() if k not in ("strike", "barrage")}
                               | {"observed": observed})
            if res.get("strike"):
                strikes.append(res["strike"])
            if res.get("barrage"):
                for victim in world.enemies_of(bat.team):
                    ff.add_barrage(g, res["barrage"], victim)
    report["artillery"] = arty_report
    world.last_fire_fields = ff

    # 火力掩护先行压制：炮兵打击点与活跃 fire_weapon 事件先行打上压制标签，瘫痪守方 ZOC 与伏击
    supp_ttl = max(0.25, hours)
    for st in strikes:
        if st.get("intensity", 0.0) > 0:
            c_px = _px(st["px"])
            rad_s = max(1, int(math.ceil(float(st.get("radius_px", 2)))))
            for dr in range(-rad_s, rad_s + 1):
                for dc in range(-rad_s, rad_s + 1):
                    if dr * dr + dc * dc <= rad_s * rad_s + 1:
                        rr, cc = c_px[0] + dr, c_px[1] + dc
                        if 0 <= rr < g.shape[0] and 0 <= cc < g.shape[1]:
                            g.set_cell_modifier((rr, cc), f"supp:{st['team']}", {"speed_mult": 0.65, "suppressed": 1.0}, ttl_hours=supp_ttl)
    for ev in world.event_manager.events.values():
        if ev.kind == "fire_weapon":
            actor = world.pieces.get(ev.actor_id)
            if actor is not None and "target_px" in ev.payload:
                c_px = _px(ev.payload["target_px"])
                rad_s = max(0, int(math.ceil(float(ev.payload.get("radius_px", 1.0)))))
                for dr in range(-rad_s, rad_s + 1):
                    for dc in range(-rad_s, rad_s + 1):
                        if dr * dr + dc * dc <= rad_s * rad_s + 1e-9:
                            rr, cc = c_px[0] + dr, c_px[1] + dc
                            if 0 <= rr < g.shape[0] and 0 <= cc < g.shape[1]:
                                g.set_cell_modifier((rr, cc), f"supp:{actor.team}", {"speed_mult": 0.65, "suppressed": 1.0}, ttl_hours=supp_ttl)

    casualties = {}
    channel_intercepts = []
    ambush_events = []
    mine_events = []
    overrun_events = []
    assault_pairs = []
    ambush_fired = set()
    ambush_arc_cache = {}

    # 3.9 指挥官独立机动（commander_moves，或直接混写在 moves 里的指挥官 ID）
    cmd_move_specs = dict(orders.get("commander_moves") or {})
    for pid_m, spec_m in (orders.get("moves") or {}).items():
        if pid_m in world.commanders and pid_m not in world.pieces and pid_m not in cmd_move_specs:
            cmd_move_specs[pid_m] = spec_m

    cmd_moves_report = {}
    for cid, spec in cmd_move_specs.items():
        cmd = world.commanders.get(cid)
        if cmd is None:
            warnings.append(f"未知指挥官 {cid}")
            continue
        if not cmd.alive:
            cmd_moves_report[cid] = {"from": cmd.px, "to": cmd.px, "status": "dead", "arrived": False, "moved_km": 0.0}
            continue
        if cmd.attached_to is not None:
            cmd_moves_report[cid] = {"from": cmd.px, "to": cmd.px, "status": "attached_follows_parent", "arrived": False, "moved_km": 0.0}
            continue
        segs = (
            list(spec["waypoints"])
            if isinstance(spec, dict) and "waypoints" in spec
            else ([spec] if isinstance(spec, dict) else [{"to": spec}])
        )
        start_c = cmd.px
        rem_h_c = hours * LONG_MARCH_FACTOR["foot"] if hours > LONG_MARCH_HOURS else hours
        total_km_c = 0.0
        arrived_c = False
        status_c = "en_route"
        for seg in segs:
            if rem_h_c <= 1e-9:
                break
            tgt_c = _px(seg["to"] if isinstance(seg, dict) else seg)
            seg_h_c = min(rem_h_c, max(MIN_TICK_HOURS, float(seg.get("duration_h", rem_h_c)))) if isinstance(seg, dict) else rem_h_c
            mob_c = seg.get("mobility", "foot") if isinstance(seg, dict) else "foot"
            if mob_c not in BASE_SPEED_KMH:
                mob_c = "foot"
            spd_c = BASE_SPEED_KMH[mob_c] * g.get_cell_modifiers(cmd.px, mobility=mob_c)["non_feature_speed_mult"]
            budget_c = spd_c * seg_h_c
            path_c = tf.shortest_path(g, cmd.px, tgt_c, mob_c, extra_mult=ff.mult_for(cmd.team),
                                      blocked=_occupation_mask(world, cmd.team))
            if path_c is None:
                status_c = "unreachable"
                warnings.append(f"指挥官 {cid} 到 {tgt_c} 不可达")
                break
            end_c, used_c, arrived_c = tf.advance_along(*path_c, budget_c)
            idx_c = path_c[0].index(end_c)
            km_c = sum(math.hypot(a[0] - b[0], a[1] - b[1]) for a, b in zip(path_c[0][:idx_c], path_c[0][1:idx_c + 1])) * g.cell_m / 1000.0
            cmd.px = tgt_c if arrived_c else end_c
            total_km_c += km_c
            spent_c = min(seg_h_c, used_c / spd_c) if (arrived_c and spd_c > 1e-9) else seg_h_c
            rem_h_c = max(0.0, rem_h_c - spent_c)
            status_c = "arrived" if arrived_c else "en_route"
            if not arrived_c:
                break
        cmd_moves_report[cid] = {"from": start_c, "to": cmd.px, "status": status_c,
                                 "arrived": arrived_c, "moved_km": round(total_km_c, 2)}
    report["commander_moves"] = cmd_moves_report

    # 4 行军（连续浮点坐标 pos 推进 + 像素容器 Buff + 雷区/装甲碾压/ROE伏击 + 牵引/人力拖载重惩罚 + 事件行动锁）
    moves_report = {}
    for pid, spec in (orders.get("moves") or {}).items():
        if pid in world.commanders and pid not in world.pieces:
            continue
        p = world.pieces.get(pid)
        if p is None:
            warnings.append(f"未知棋子 {pid}")
            continue
        if p.unit_state == "deployed" or p.is_heavy_emplacement:
            moves_report[pid] = {"from": p.px, "pos_from": list(p.pos), "to": p.px, "pos_to": list(p.pos),
                               "status": "locked_heavy_emplacement", "arrived": False, "moved_km": 0.0}
            warnings.append(f"{pid} 处于部署态，必须先撤收才能机动")
            continue
        if p.attached_to is not None:
            moves_report[pid] = {"from": p.px, "pos_from": list(p.pos), "to": p.px, "pos_to": list(p.pos),
                               "status": "attached_follows_parent", "arrived": False, "moved_km": 0.0}
            continue
        ev_mods = world.event_manager.piece_modifiers(p, world=world)
        if ev_mods["forbid_move"]:
            if ev_mods["hold_still"] and isinstance(spec, dict) and spec.get("force_move"):
                world.event_manager.cancel_hold_still_on_move(p.id, world=world)
                ev_mods = world.event_manager.piece_modifiers(p, world=world)
            if ev_mods["forbid_move"]:
                status_tag = "locked_hold_still" if ev_mods["hold_still"] else "locked_forbid_move"
                moves_report[pid] = {"from": p.px, "pos_from": list(p.pos), "to": p.px, "pos_to": list(p.pos),
                                   "status": status_tag, "arrived": False, "moved_km": 0.0}
                warnings.append(f"{pid} 处于禁止移动锁定（{status_tag}）")
                continue

        attached_children = [c for c in world.pieces.values() if c.attached_to == p.id]
        if attached_children and ev_mods["forbid_tow"]:
            moves_report[pid] = {"from": p.px, "pos_from": list(p.pos), "to": p.px, "pos_to": list(p.pos),
                               "status": "locked_forbid_tow", "arrived": False, "moved_km": 0.0}
            warnings.append(f"{pid} 牵引装置受损，禁止拖带随行单位")
            continue
        tow_speed_mult = 1.0
        tow_org_cost_per_h = 0.0
        immovable_reason = None
        for ch in attached_children:
            tp = toe.tow_profile(p, ch)
            if not tp["can_move"]:
                immovable_reason = ch.id
                break
            tow_speed_mult = min(tow_speed_mult, tp["speed_mult"])
            tow_org_cost_per_h = max(tow_org_cost_per_h, tp["org_cost_per_h"])
        if immovable_reason is not None:
            moves_report[pid] = {"from": p.px, "pos_from": list(p.pos), "to": p.px, "pos_to": list(p.pos),
                               "status": "locked_towing_overload", "arrived": False, "moved_km": 0.0}
            warnings.append(f"{pid} 人力/载具不足，无法拖动重武器 {immovable_reason}")
            continue
        # 若自身就是独立重武器单位（非随行、未部署，纯粹自己人力推着走）
        if p.deployed_weapon and p.unit_state == "mobile":
            tp_self = toe.tow_profile(p, p)
            if not tp_self["can_move"]:
                moves_report[pid] = {"from": p.px, "pos_from": list(p.pos), "to": p.px, "pos_to": list(p.pos),
                                   "status": "locked_insufficient_crew", "arrived": False, "moved_km": 0.0}
                continue
            tow_speed_mult = min(tow_speed_mult, tp_self["speed_mult"])
            tow_org_cost_per_h = max(tow_org_cost_per_h, tp_self["org_cost_per_h"])

        if isinstance(spec, dict) and "waypoints" in spec:
            segments = list(spec["waypoints"])
        elif isinstance(spec, dict):
            segments = [spec]
        elif (
            isinstance(spec, (list, tuple))
            and spec
            and (isinstance(spec[0], dict) or isinstance(spec[0], (list, tuple)))
        ):
            segments = list(spec)
        else:
            segments = [{"to": spec, "mode": "advance"}]

        mob = p.mobility()
        eff_hours = hours * LONG_MARCH_FACTOR[mob] if hours > LONG_MARCH_HOURS else hours
        rem_hours = eff_hours
        start_overall = p.px
        start_pos = tuple(p.pos)
        total_real_km = 0.0
        total_used_km = 0.0
        seg_logs = []
        last_status = "en_route"
        final_arrived = False
        final_target = start_overall

        for seg in segments:
            if rem_hours <= 1e-9:
                break
            if isinstance(seg, dict):
                target_f = (float(seg["to"][0]), float(seg["to"][1]))
                target = _px(seg["to"])
                mode = seg.get("mode", "advance")
                seg_h = min(rem_hours, max(MIN_TICK_HOURS, float(seg.get("duration_h", rem_hours))))
            else:
                target_f = (float(seg[0]), float(seg[1]))
                target, mode, seg_h = _px(seg), "advance", rem_hours
            final_target = target
            cell_speed_mult = g.get_cell_modifiers(p.px, mobility=mob)["non_feature_speed_mult"]
            base_kmh = p.base_speed_kmh(world.pieces) if hasattr(p, "base_speed_kmh") else BASE_SPEED_KMH[mob]
            speed_eff = base_kmh * _fuel_factor(p) * ev_mods["speed_mult"] * cell_speed_mult * tow_speed_mult
            budget = speed_eff * seg_h
            cur_start = p.px
            blocked_mask = _occupation_mask(world, p.team, mover=p)
            if mode == "assault":
                blocked_mask = blocked_mask.copy()
                blocked_mask &= ~tf.disk_mask(g.shape, target, OCCUPY_RADIUS_PX + 1)
                blocked_mask[cur_start] = False
            path = tf.shortest_path(g, cur_start, target, mob, extra_mult=ff.mult_for(p.team),
                                    blocked=blocked_mask)
            if path is None:
                last_status = "unreachable"
                seg_logs.append({"from": cur_start, "target": target, "status": "unreachable"})
                warnings.append(f"{pid} 到 {target} 不可达（地形/水域/被占）")
                break
            end, used, arrived = tf.advance_along(*path, budget)
            idx = path[0].index(end)
            real_km = 0.0
            used_actual = 0.0
            stopped_reason = None
            halted_by_event = False
            last_walked_idx = 0

            if idx == 0 and not arrived and budget > 1e-9:
                # 亚像素连续推进（例如 15 分钟纯人力拖重武器只挪 0.25 像素，不丢零头）
                nxt_px = path[0][1] if len(path[0]) > 1 else target_f
                dr = float(nxt_px[0]) - p.pos[0]
                dc = float(nxt_px[1]) - p.pos[1]
                d_px = math.hypot(dr, dc)
                if d_px > 1e-9:
                    step_px = min(d_px, budget / (g.cell_m / 1000.0))
                    p.pos = (p.pos[0] + dr * (step_px / d_px), p.pos[1] + dc * (step_px / d_px))
                    real_km += step_px * g.cell_m / 1000.0
                    used_actual = real_km
            else:
                for step_i in range(1, idx + 1):
                    prev_px = path[0][step_i - 1]
                    step_px = path[0][step_i]
                    step_km = math.hypot(step_px[0] - prev_px[0], step_px[1] - prev_px[1]) * g.cell_m / 1000.0
                    real_km += step_km
                    used_actual = path[1][step_i]
                    last_walked_idx = step_i
                    if arrived and step_i == idx:
                        p.pos = target_f
                    else:
                        p.pos = (float(step_px[0]), float(step_px[1]))
                    for ch in attached_children:
                        ch.pos = tuple(p.pos)
                    for cmd in world.commanders.values():
                        if cmd.attached_to == p.id or (cmd.cadre_piece_id == p.id and not cmd.attached_to and cmd.id not in cmd_moves_report):
                            cmd.px = p.px
                    for et in world.enemies_of(p.team):
                        g.cut_wire_at(step_px, radius_px=0, team=et)

                    # 4.1 像素雷区判定（工兵班明确排雷机动时豁免并安全清除）
                    cm_step = g.cell_modifiers.get(step_px, {})
                    mine_keys = [k for k in list(cm_step) if k.startswith("minefield:") and k != f"minefield:{p.team}"]
                    step_mods = g.get_cell_modifiers(step_px, mobility=mob)
                    has_feat_mine = step_mods.get("mine_hazard", 0.0) > 0.0
                    if mine_keys or has_feat_mine:
                        is_clearing = mode == "clear" and any(
                            sq.kind == "engineer_squad" or sq.has_buff("mine_clearing") for sq in p.squads()
                        )
                        cleared_keys = list(mine_keys)
                        for mk in mine_keys:
                            g.remove_cell_modifier(step_px, mk)
                        if has_feat_mine:
                            for fid in list(step_mods.get("features") or ()):
                                spec_m = tf.TERRAIN_FEATURE_SPECS.get(fid, {}).get("modifiers", {})
                                ch_m = (g.feature_channels.get(fid, {}).get("modifiers") or {}) if hasattr(g, "feature_channels") else {}
                                if fid == "minefield" or spec_m.get("mine_hazard", 0) or ch_m.get("mine_hazard", 0):
                                    g.remove_terrain_feature(step_px, fid)
                                    cleared_keys.append(f"feat:{fid}")
                        if is_clearing:
                            g.add_exposure(step_px, 4.0, radius_px=1.0)
                            mine_events.append({"piece": p.id, "px": list(step_px), "action": "cleared", "keys": cleared_keys})
                        else:
                            if _piece_is_armored(p) or _vehicle_count(p) > 0:
                                p.add_debuff("mine_damaged", {"forbid_move": True, "source": "minefield"})
                                combat._apply_damage_to_piece(
                                    p, "heat_shaped", 35.0, 0.0, 0.0, f"{world.turn}|mine|{p.id}|{step_px}", casualties
                                )
                            else:
                                combat._apply_damage_to_piece(
                                    p, "he_blast", 30.0, 0.0, 0.0, f"{world.turn}|mine|{p.id}|{step_px}", casualties
                                )
                                for sq in p.squads():
                                    sq.org = max(0.0, sq.org - 20.0)
                            mine_events.append({"piece": p.id, "px": list(step_px), "action": "detonated", "keys": cleared_keys})
                            arrived = False
                            stopped_reason = "minefield"
                            halted_by_event = True
                            break

                    # 4.2 装甲行军碾压（Overrun）与突击接敌暂停
                    enemies_here = [
                        ep for ep in world.pieces.values()
                        if ep.team != p.team and ep.px == step_px and (ep.persons() > 0 or ep.vehicle_model)
                    ]
                    if enemies_here:
                        if mode == "assault":
                            for ep in enemies_here:
                                assault_pairs.append((p.id, ep.id))
                            arrived = (step_px == target)
                            stopped_reason = "assault_contact"
                            halted_by_event = True
                            break
                        elif _piece_is_armored(p):
                            p_pwr = sum(sum(s.power().values()) for s in p.squads())
                            blocked_here = False
                            for ep in list(enemies_here):
                                ep_suppressed = any(k.startswith("supp:") for k in g.cell_modifiers.get(step_px, {}))
                                ep_pwr = sum(sum(s.power().values()) for s in ep.squads())
                                if ep_suppressed or ep.mean_org() < 25.0 or p_pwr >= 2.5 * max(1.0, ep_pwr) or ep.persons() <= 15:
                                    combat._apply_damage_to_piece(
                                        ep, "he_blast", min(45.0, max(12.0, 0.25 * p_pwr)), 0.0, 0.0,
                                        f"{world.turn}|overrun|{p.id}|{ep.id}", casualties
                                    )
                                    for sq in ep.squads():
                                        sq.org = max(0.0, sq.org - 20.0)
                                    displaced_to = _displace_piece(world, ep, from_pos=prev_px, flank=True)
                                    ic_ov = _capture_hq_channel(world, p.team, ep, "overrun")
                                    if ic_ov is not None:
                                        channel_intercepts.append(ic_ov)
                                    overrun_events.append({
                                        "attacker": p.id, "defender": ep.id,
                                        "at": list(step_px), "displaced_to": displaced_to,
                                    })
                                else:
                                    blocked_here = True
                            if blocked_here:
                                arrived = False
                                stopped_reason = "enemy_contact"
                                halted_by_event = True
                                break
                        else:
                            arrived = (step_px == target)
                            stopped_reason = "enemy_contact"
                            halted_by_event = True
                            break

                    # 4.3 交战姿态与行进间触发伏击（ROE Overwatch / Ambush）
                    for ep in sorted(world.pieces.values(), key=lambda x: x.id):
                        if ep.team == p.team or ep.id in ambush_fired:
                            continue
                        if ep.persons() <= 0 and not ep.deployed_weapon and not ep.vehicle_model:
                            continue
                        if getattr(ep, "roe", "return_fire") not in ("ambush", "fire_at_will"):
                            continue
                        if ep.unit_state == "carried":
                            continue
                        if any(k.startswith("supp:") and k != f"supp:{ep.team}" for k in g.cell_modifiers.get(ep.px, {})):
                            continue
                        ep_ev = world.event_manager.piece_modifiers(ep, world=world)
                        if ep_ev["forbid_fire"] or (ep_ev["single_crew"] and ep.moved_last_turn):
                            continue
                        ep_op_mult = 1.0
                        ep_op_sq = None
                        ep_op_pers = None
                        if ep.deployed_weapon:
                            if ep.unit_state != "deployed":
                                continue
                            op_piece_amb = next(
                                (op for op in world.pieces.values()
                                 if world.event_manager.piece_modifiers(op)["operating_target"] == ep.id),
                                None,
                            )
                            if op_piece_amb is not None:
                                op_sqs = [s for s in op_piece_amb.squads() if s.persons > 0]
                                if not op_sqs:
                                    continue
                                ep_op_sq = max(op_sqs, key=lambda s: toe.heavy_operator_efficiency(s, ep.deployed_weapon))
                                sel_ids = list((ep.meta or {}).get("operator_soldier_ids") or [])
                                ep_op_pers = int((ep.meta or {}).get("operator_persons") or (len(sel_ids) if sel_ids else op_piece_amb.persons()))
                            elif ep.operated_by and ep.operated_by in world.pieces:
                                op_info = ep.get_operator_info(world.pieces)
                                ep_op_sq = op_info.get("operator_squad")
                                ep_op_pers = op_info.get("operator_persons", 0)
                            else:
                                continue
                            if ep_op_sq is None or (ep_op_pers or 0) <= 0:
                                continue
                        dist_km_ep = g.distance_km(ep.pos, p.pos)
                        if dist_km_ep > 2.0 + 1e-6:
                            continue
                        wm_ep = _primary_weapon_of(ep)
                        spec_ep = toe.EQUIPMENT.get(wm_ep, {}) if wm_ep else {}
                        r_ep = float(spec_ep.get("range_m", 400.0))
                        min_r_ep = float(spec_ep.get("min_range_m", 0.0))
                        cache_key = (ep.id, ep.px)
                        arc_map = ambush_arc_cache.get(cache_key)
                        if arc_map is None:
                            arc_cells = tf.compute_fire_arc(
                                g, ep.pos, r_ep, min_range_m=min_r_ep,
                                indirect=bool(spec_ep.get("indirect", False)),
                                night=world.night, max_scan_m=2000.0,
                            )
                            arc_map = {(c_rc[0], c_rc[1]): c_rc[2] for c_rc in arc_cells}
                            ambush_arc_cache[cache_key] = arc_map
                        if step_px not in arc_map:
                            continue
                        # 踩入通视射界，触发一次自动拦截射击！
                        ambush_fired.add(ep.id)
                        burst_h = min(0.35, max(0.15, hours * 0.25))
                        ep_detail = 1.0
                        ep_cell_mult = g.get_cell_modifiers(ep.px)["combat_power_mult"]
                        ep_ammo_ratio = ep.stock.get("ammo", 0.0) / max(1e-9, ep.stock_cap.get("ammo", 1.0))
                        ep_sup = 0.2 + 0.8 * min(1.0, ep_ammo_ratio / 0.2)
                        ep_ev_mult = ep_ev["combat_power_mult"] * ep_cell_mult * ep_op_mult
                        ep_move_mult = 0.55 if ep.moved_last_turn else 1.0
                        bg_ep = toe.weapon_ballistic_group(wm_ep) if wm_ep else "direct"
                        acc_b_ep = events.resolve_registration_bonus(world, ep, step_px, bg_ep)
                        dist_m_ep = dist_km_ep * 1000.0
                        amb_fire = {}
                        for sq in ep.squads():
                            pwr = sq.power(
                                dist_m=dist_m_ep, has_los=True, acc_mult=1.0 + acc_b_ep,
                                operator_squad=ep_op_sq, operator_persons=ep_op_pers,
                            )
                            if pwr:
                                world.exposure_tracker.mark_fired(sq)
                            for dt_, pts in pwr.items():
                                amb_fire[dt_] = amb_fire.get(dt_, 0.0) + pts * burst_h * ep_sup * ep_detail * ep_ev_mult * ep_move_mult
                        ep.stock["ammo"] = max(0.0, ep.stock.get("ammo", 0.0) - sum(amb_fire.values()) * AMMO_T_PER_POINT)
                        pre_cas = dict(casualties.get(p.id) or {})
                        pre_en = pre_cas.get("enlisted", 0)
                        pre_ca = pre_cas.get("cadre", 0)
                        pre_eq = dict(pre_cas.get("equipment") or {})
                        combat.resolve_cell_strike(
                            world, step_px, ep.team, amb_fire,
                            dist_m=dist_m_ep, weapon_model=wm_ep, acc_bonus=acc_b_ep,
                            aspect_target_pos=ep.pos,
                            seed_tag=f"{world.turn}|ambush|{ep.id}|{p.id}|{step_px}",
                            casualties_out=casualties,
                        )
                        post_cas = casualties.get(p.id) or {}
                        took_hit = (
                            post_cas.get("enlisted", 0) > pre_en
                            or post_cas.get("cadre", 0) > pre_ca
                            or post_cas.get("equipment") != pre_eq
                        )
                        p_armored = _piece_is_armored(p)
                        anti_armor_pts = (
                            amb_fire.get("ap_kinetic", 0.0)
                            + amb_fire.get("heat_shaped", 0.0)
                            + amb_fire.get("he_blast", 0.0)
                        )
                        org_drop = 10.0 if (p_armored and anti_armor_pts < 5.0) else 22.0
                        for sq in p.squads():
                            sq.org = max(0.0, sq.org - org_drop)
                        interrupted = (
                            took_hit
                            or p.mean_org() < 80.0
                            or (not p_armored and sum(amb_fire.values()) >= 4.0)
                            or (p_armored and anti_armor_pts >= 8.0)
                        )
                        ambush_events.append({
                            "ambusher": ep.id, "target": p.id, "at": list(step_px),
                            "dist_km": round(dist_km_ep, 2), "weapon": wm_ep,
                            "interrupted": interrupted, "casualties": post_cas,
                        })
                        if interrupted:
                            arrived = False
                            stopped_reason = "ambush_intercept"
                            halted_by_event = True
                            break
                    if halted_by_event:
                        break

            total_real_km += real_km
            total_used_km += used_actual
            veh = _vehicle_count(p)
            p.stock["fuel"] = max(0.0, p.stock["fuel"] - veh * FUEL_T_PER_VEHICLE_KM * real_km * ev_mods["fuel_mult"])
            if tow_org_cost_per_h > 0.0 and real_km > 1e-9:
                for sq in p.squads():
                    sq.org = max(0.0, sq.org - tow_org_cost_per_h * seg_h)
            spent_h = min(seg_h, used_actual / speed_eff) if (arrived and speed_eff > 1e-9) else seg_h
            rem_hours = max(0.0, rem_hours - spent_h)
            final_arrived = arrived
            last_status = f"halted_{stopped_reason}" if stopped_reason else ("arrived" if arrived else "en_route")
            seg_entry = {"from": cur_start, "to": p.px, "pos_to": [round(p.pos[0], 4), round(p.pos[1], 4)],
                         "target": target, "mode": mode,
                         "arrived": arrived, "moved_km": round(real_km, 2), "spent_h": round(spent_h, 2)}
            if stopped_reason:
                seg_entry["stopped_reason"] = stopped_reason
            if mode == "withdraw" and p.px != cur_start:
                assist = _engineer_assist(p)
                left_total = {}
                for sq in p.squads():
                    for m, n in combat.abandon_equipment(sq, assist).items():
                        left_total[m] = left_total.get(m, 0) + n
                if left_total:
                    g.stash_put(cur_start, equipment=left_total, owner_team=p.team)
                    seg_entry["abandoned"] = left_total
            seg_logs.append(seg_entry)
            if not arrived or halted_by_event:
                break

        for ch in attached_children:
            ch.pos = tuple(p.pos)
        for cmd in world.commanders.values():
            if cmd.attached_to == p.id or (cmd.cadre_piece_id == p.id and not cmd.attached_to and cmd.id not in cmd_moves_report):
                cmd.px = p.px

        p.moved_last_turn = math.hypot(p.pos[0] - start_pos[0], p.pos[1] - start_pos[1]) > 1e-6
        if p.moved_last_turn and p.id not in explicit_facings and autonomy_v3.is_directional_hull_unit(p):
            p.set_facing(toe.bearing_deg(start_pos, p.pos))
        entry = {
            "from": start_overall,
            "pos_from": [round(start_pos[0], 4), round(start_pos[1], 4)],
            "to": p.px,
            "pos_to": [round(p.pos[0], 4), round(p.pos[1], 4)],
            "facing_deg": round(p.facing_deg, 1),
            "sub_hex": tf.sub_hex(p.pos)[1],
            "target": final_target,
            "mode": seg_logs[-1].get("mode", "advance") if seg_logs else "advance",
            "mobility": mob,
            "budget_km": round(BASE_SPEED_KMH[mob] * eff_hours, 2),
            "arrived": final_arrived,
            "status": last_status,
            "moved_km": round(total_real_km, 2),
            "cost_km": round(total_used_km, 2),
            "segments": seg_logs,
        }
        if any("abandoned" in s for s in seg_logs):
            agg_ab = {}
            for s in seg_logs:
                for m, n in (s.get("abandoned") or {}).items():
                    agg_ab[m] = agg_ab.get(m, 0) + n
            entry["abandoned"] = agg_ab
        moves_report[pid] = entry
    for p in world.pieces.values():
        if p.id not in moves_report:
            p.moved_last_turn = False
    report["moves"] = moves_report
    report["ambushes"] = ambush_events
    report["mines"] = mine_events
    report["overruns"] = overrun_events

    # 缴获：到达敌方遗留装备所在像素附近的棋子直接从 cell_stash 清点并回收（SSOT 单轨）
    captures = {}
    for key in list(g.cell_stash):
        stash = g.cell_stash.get(key)
        if not stash or not stash.get("equipment"):
            continue
        owner_team = stash.get("owner_team")
        for p in world.pieces.values():
            if owner_team is not None and p.team == owner_team:
                continue
            if math.hypot(p.px[0] - key[0], p.px[1] - key[1]) <= 3:
                eq_snap = dict(stash["equipment"])
                usable, wrecked = combat.capture_equipment(eq_snap, _engineer_assist(p))
                g.stash_take(key, equipment=eq_snap)
                if usable:
                    sqs = p.squads()
                    if sqs:
                        target_sq = next((s for s in sqs if s.persons > 0), sqs[0])
                        for m, n in usable.items():
                            target_sq.equipment[m] = target_sq.equipment.get(m, 0) + n
                if (eq_snap.get("hq_radio", 0) > 0 or eq_snap.get("radio", 0) > 0) and stash.get("radio_channel") and owner_team != p.team:
                    rc_ch = stash["radio_channel"]
                    world.intercepted_channels.setdefault(p.team, {})[rc_ch] = 12.0
                    channel_intercepts.append({
                        "winner_team": p.team, "from_stash": list(key), "channel": rc_ch,
                        "ttl_hours": 12.0, "reason": "capture_stash",
                    })
                captures[p.id] = {"at": key, "usable": usable, "wrecked": wrecked}
                break
    report["captures"] = captures

    # 5 后勤（按队分别配送；合并有库存的像素仓库 cell_stash + 团/旅/师级 AO 运力减损加成）
    logi_report = {}
    borrowed = {}
    def _logi_mult(px, team):
        if not world.ao_zones:
            return 1.0
        return command_v2.compute_operational_ao_buff(px, team, world.ao_zones, world.commanders, current_turn=report["turn"])["logistics_mult"]
    for team in world.teams():
        depots = [d for d in world.depots.values() if d.team == team]
        enemy_block = _control_mask(world, team)
        stash_proxies = []
        for s_px, s_val in list(g.cell_stash.items()):
            res_map = s_val.get("resources") or {}
            if any(v > 1e-9 for v in res_map.values()) and not enemy_block[s_px]:
                if any(d.px == s_px for d in depots):
                    continue
                proxy = lg.Depot(f"stash:{s_px[0]},{s_px[1]}", s_px, team, res_map)
                stash_proxies.append((proxy, s_px, dict(res_map)))
                depots.append(proxy)
        if not depots:
            continue
        consumers = [lg.consumer_from_piece(p, g) for p in world.pieces.values() if p.team == team]
        for c in consumers:
            p = world.pieces[c["id"]]
            c["demand"]["replacements"] = float(sum(max(0, s.enlisted_full - s.enlisted) for s in p.squads()))
            c["required_ammo_types"] = sorted(p.required_ammo_types())
        init = {d.id: dict(d.stock) for d in depots}
        rep = lg.dispatch(g, depots, consumers, hours, hazard=ff.hazard_for(team),
                          enemy_block=enemy_block,
                          logistics_mult_fn=_logi_mult if world.ao_zones else None)
        ok, problems = lg.check_conservation(rep, init, depots)
        if not ok:
            warnings.append(f"后勤守恒校验失败: {problems[:2]}")
        # 将虚拟像素仓库消耗的物资守恒回写进 grid.cell_stash
        for proxy, s_px, orig_res in stash_proxies:
            spent = {rk: max(0.0, orig_res.get(rk, 0.0) - proxy.stock.get(rk, 0.0)) for rk in orig_res}
            if any(v > 1e-9 for v in spent.values()):
                g.stash_take(s_px, resources=spent)
        for pid, r in rep.items():
            p = world.pieces[pid]
            for res in lg.TONNAGE:
                p.stock[res] = min(p.stock_cap.get(res, 1e18), p.stock.get(res, 0.0) + r["delivered"][res])
            for cal, amt in (r.get("ammo_by_type_delivered") or {}).items():
                p.ammo_by_type[cal] = p.ammo_by_type.get(cal, 0.0) + amt
            p.stock["replacements"] = p.stock.get("replacements", 0.0) + r["delivered"]["replacements"]
            borrowed[pid] = r["borrowed_soldiers"]
            cas = r["porter_casualties"]
            if cas:
                for sq in sorted([s for s in p.squads() if s.kind in ("porter_squad", "transport_squad")],
                                 key=lambda s: s.id):
                    k = min(cas, sq.enlisted)
                    sq.enlisted -= k
                    cas -= k
                    if cas <= 0:
                        break
            logi_report[pid] = {k: (round(v, 3) if isinstance(v, float) else v)
                                for k, v in r.items() if k not in ("delivered", "drawn", "losses", "unmet")}
            logi_report[pid]["delivered"] = {k: round(v, 3) for k, v in r["delivered"].items()}
            logi_report[pid]["unmet"] = {k: round(v, 3) for k, v in r["unmet"].items()}
    report["logistics"] = logi_report

    # 6 打击落地（高爆/燃烧同时物理切断落点半径内的野战电缆，统一像素容器结算无差别误伤、破片落点与 Depot 殉爆）
    strike_logs = []
    for s in strikes:
        _apply_strike_to_pieces(world, s, casualties, strike_logs=strike_logs)
        if s.get("damage_type") in ("he_blast", "incendiary") and s.get("intensity", 0) > 0:
            g.cut_wire_at(s["px"], radius_px=float(s.get("radius_px", 1)))
    report["strike_logs"] = strike_logs

    # 7 地面交战（同时结算 + 射界诸元标定 + ROE 静默控制 + 像素容器结算 + 短兵相接击退夺占）
    engaged = set()
    eng_report = []
    cqb_displacements = []
    eng_list = list(orders.get("engagements") or [])
    existing_pairs = {(e.get("attacker"), e.get("defender")) for e in eng_list} | {
        (e.get("defender"), e.get("attacker")) for e in eng_list
    }
    for aid, did in assault_pairs:
        if (aid, did) not in existing_pairs:
            eng_list.append({"attacker": aid, "defender": did, "hours": hours, "auto_assault": True})
            existing_pairs.add((aid, did))
            existing_pairs.add((did, aid))

    for e in eng_list:
        a, d = world.pieces.get(e["attacker"]), world.pieces.get(e["defender"])
        if a is None or d is None or a.team == d.team:
            warnings.append(f"无效交战 {e}")
            continue
        if (a.persons() <= 0 and not a.vehicle_model) or (d.persons() <= 0 and not d.vehicle_model):
            continue
        dist = g.distance_km(a.px, d.px)
        if dist > ENGAGE_RANGE_KM:
            warnings.append(f"{a.id}→{d.id} 距离 {dist:.1f} km 超过接触线 {ENGAGE_RANGE_KM} km，未交战")
            continue
        h = float(e.get("hours", hours))
        fire = {}
        for piece in (a, d):
            piece_roe = getattr(piece, "roe", "return_fire")
            if piece_roe == "hold_fire" and not (piece is a and e.get("force_fire")):
                fire[piece.id] = {}
                continue
            fighters = sum(s.enlisted for s in piece.squads() if s.kind not in ("porter_squad", "transport_squad")) or 1
            detail = 1.0 - min(0.9, borrowed.get(piece.id, 0) / fighters)
            ev_mods = world.event_manager.piece_modifiers(piece, world=world)
            cell_mods = g.get_cell_modifiers(piece.px)
            ev_power_mult = ev_mods["combat_power_mult"] * cell_mods["combat_power_mult"]
            if ev_mods["forbid_fire"] or (ev_mods["single_crew"] and piece.moved_last_turn):
                ev_power_mult = 0.0
            # 三态重武器单位火力法则：随行(carried)不开火；部署无人(deployed且无operated_by)不开火；有人操作按士官效率发挥
            op_sq = None
            op_pers = None
            if piece.deployed_weapon:
                if piece.unit_state == "carried":
                    ev_power_mult = 0.0
                elif piece.unit_state == "deployed":
                    op_piece_eng = next(
                        (op for op in world.pieces.values()
                         if world.event_manager.piece_modifiers(op)["operating_target"] == piece.id),
                        None,
                    )
                    if op_piece_eng is not None:
                        op_sqs = [s for s in op_piece_eng.squads() if s.persons > 0]
                        if op_sqs:
                            op_sq = max(op_sqs, key=lambda s: toe.heavy_operator_efficiency(s, piece.deployed_weapon))
                            sel_ids = list((piece.meta or {}).get("operator_soldier_ids") or [])
                            op_pers = int((piece.meta or {}).get("operator_persons") or (len(sel_ids) if sel_ids else op_piece_eng.persons()))
                        else:
                            ev_power_mult = 0.0
                    elif piece.operated_by and piece.operated_by in world.pieces:
                        op_info = piece.get_operator_info(world.pieces)
                        op_sq = op_info.get("operator_squad")
                        op_pers = op_info.get("operator_persons", 0)
                        if op_sq is None or (op_pers or 0) <= 0:
                            ev_power_mult = 0.0
                    else:
                        ev_power_mult = 0.0
            ammo_ratio = piece.stock.get("ammo", 0.0) / max(1e-9, piece.stock_cap.get("ammo", 1.0))
            sup = 0.2 + 0.8 * min(1.0, ammo_ratio / 0.2)
            other = d if piece is a else a
            dist_m = g.distance_km(piece.pos, other.pos) * 1000.0
            has_los = tf.line_visibility(g, piece.px, other.px, night=world.night) > 1e-6
            move_fire_mult = 0.55 if piece.moved_last_turn else 1.0
            wm_p = _primary_weapon_of(piece)
            bg_p = toe.weapon_ballistic_group(wm_p) if wm_p else "direct"
            reg_b = events.resolve_registration_bonus(world, piece, other.px, bg_p)
            totals = {}
            for sq in piece.squads():
                pwr = sq.power(
                    dist_m=dist_m, has_los=has_los, acc_mult=1.0 + reg_b,
                    operator_squad=op_sq, operator_persons=op_pers,
                )
                if pwr:
                    world.exposure_tracker.mark_fired(sq)
                for dt_, pts in pwr.items():
                    totals[dt_] = totals.get(dt_, 0.0) + pts * h * sup * detail * ev_power_mult * move_fire_mult
            fire[piece.id] = totals
            used = sum(totals.values()) * AMMO_T_PER_POINT
            piece.stock["ammo"] = max(0.0, piece.stock.get("ammo", 0.0) - used)

        eng_strikes = {}
        for src, dst in ((a, d), (d, a)):
            if not fire[src.id]:
                continue
            wm_src = _primary_weapon_of(src)
            bg_src = toe.weapon_ballistic_group(wm_src) if wm_src else "direct"
            acc_b = events.resolve_registration_bonus(world, src, dst.px, bg_src)
            sr = combat.resolve_cell_strike(
                world,
                dst.px,
                src.team,
                fire[src.id],
                dist_m=g.distance_km(src.pos, dst.pos) * 1000.0,
                weapon_model=wm_src,
                acc_bonus=acc_b,
                aspect_target_pos=src.pos,
                seed_tag=f"{world.turn}|{src.id}|{dst.id}",
                casualties_out=casualties,
            )
            eng_strikes[f"{src.id}->{dst.id}"] = sr

        # 短兵相接（同格交战必决胜负，败者被击退至邻近格并遗留部分重装备，防 200 米同格室友穿帮）
        if a.px == d.px:
            a_alive = a.persons() > 0 or bool(a.vehicle_model)
            d_alive = d.persons() > 0 or bool(d.vehicle_model)
            if a_alive and not d_alive:
                ic_c = _capture_hq_channel(world, a.team, d, "cqb_wiped")
                if ic_c is not None:
                    channel_intercepts.append(ic_c)
            elif d_alive and not a_alive:
                ic_c = _capture_hq_channel(world, d.team, a, "cqb_wiped")
                if ic_c is not None:
                    channel_intercepts.append(ic_c)
            elif a_alive and d_alive:
                a_cqb = sum(sum(s.power(dist_m=0.0, has_los=True).values()) for s in a.squads()) * (0.4 + 0.6 * a.mean_org() / 100.0)
                d_cqb = sum(sum(s.power(dist_m=0.0, has_los=True).values()) for s in d.squads()) * (0.4 + 0.6 * d.mean_org() / 100.0)
                if (a.mean_org() < 40.0) != (d.mean_org() < 40.0):
                    loser = a if a.mean_org() < 40.0 else d
                    winner = d if loser is a else a
                else:
                    winner = a if a_cqb >= d_cqb else d
                    loser = d if winner is a else a
                old_px = loser.px
                new_px = _displace_piece(world, loser, from_pos=winner.pos)
                left_cqb = {}
                if tuple(new_px) != old_px:
                    assist_l = _engineer_assist(loser)
                    for sq in loser.squads():
                        for m, n in combat.abandon_equipment(sq, assist_l).items():
                            left_cqb[m] = left_cqb.get(m, 0) + n
                    if left_cqb:
                        g.stash_put(old_px, equipment=left_cqb, owner_team=loser.team, radio_channel=loser.radio_channel)
                ic_c = _capture_hq_channel(world, winner.team, loser, "cqb_displaced")
                if ic_c is not None:
                    channel_intercepts.append(ic_c)
                cqb_displacements.append({
                    "winner": winner.id, "loser": loser.id, "from": list(old_px),
                    "to": new_px, "abandoned": left_cqb,
                })

        engaged.update({a.id, d.id})
        eng_report.append({
            "attacker": a.id, "defender": d.id, "hours": h, "dist_km": round(dist, 2),
            "fire_points": {k: round(sum(v.values()), 1) for k, v in fire.items()},
            "strikes": eng_strikes,
        })
    report["engagements"] = eng_report
    report["cqb_displacements"] = cqb_displacements
    report["casualties"] = casualties

    # 8 非战斗损耗与消耗
    attrition = {}
    for p in world.pieces.values():
        rate = float(g.terrain_attrition[p.px]) * hours / 24.0
        lost = 0
        if rate > 0:
            for sq in p.squads():
                k = min(sq.enlisted, int(sq.carry.get("attr", 0.0) + rate * sq.enlisted))
                sq.carry["attr"] = sq.carry.get("attr", 0.0) + rate * sq.enlisted - k
                sq.enlisted -= k
                lost += k
        attrition[p.id] = lost
        persons = p.persons()
        p.stock["food"] = max(0.0, p.stock["food"] - persons * FOOD_T_PER_PERSON_DAY * hours / 24.0)
        if not p.moved_last_turn:
            p.stock["fuel"] = max(0.0, p.stock["fuel"] - _vehicle_count(p) * FUEL_T_PER_VEHICLE_IDLE_H * hours)
    report["attrition"] = attrition

    # 炮兵连补弹：从所属棋子的弹药库存里领
    for o in orders.get("battery_resupply") or []:
        bat, src = world.batteries.get(o["battery"]), world.pieces.get(o.get("from_piece"))
        if bat is None or src is None:
            warnings.append(f"炮兵补弹指令无效 {o}")
            continue
        mass = ar.ROUND_MASS_T[bat.caliber]
        rounds = int(min(o["rounds"], src.stock.get("ammo", 0.0) / mass))
        src.stock["ammo"] -= rounds * mass
        bat.ammo[o["shell"]] = bat.ammo.get(o["shell"], 0) + rounds

    # 9 整补复原：没有交战/受打击的班组才能休整；替补兵员先补缺额最大的班（AO Buff 加速组织度恢复）
    ao_buffs = (
        {pid: command_v2.compute_ao_buff(p, world.ao_zones, world.commanders, g, current_turn=report["turn"], pieces=world.pieces)
         for pid, p in world.pieces.items()}
        if world.ao_zones
        else {}
    )
    recovery = {}
    for p in world.pieces.values():
        pool = int(p.stock.get("replacements", 0.0))
        hit = p.id in engaged or p.id in casualties
        forbid_rest = world.event_manager.piece_modifiers(p)["forbid_rest"]
        gained = 0
        eff_h = 0.0 if (hit or forbid_rest) else hours * (1.0 + ao_buffs.get(p.id, {}).get("total", 0.0))
        if p.shock_ttl > 0:
            eff_h *= 0.5
        for sq in sorted(p.squads(), key=lambda s: -(s.enlisted_full - s.enlisted)):
            take = sq.recover(eff_h, replacements=pool)
            pool -= take
            gained += take
        p.stock["replacements"] = float(pool)
        recovery[p.id] = gained
    report["recovery"] = recovery
    report["ao_buffs"] = ao_buffs
    report["operational_ao_buffs"] = (
        {
            "commanders": {cid: command_v2.compute_operational_ao_buff(c.px, c.team, world.ao_zones, world.commanders, current_turn=report["turn"])
                           for cid, c in world.commanders.items()},
            "depots": {did: command_v2.compute_operational_ao_buff(d.px, d.team, world.ao_zones, world.commanders, current_turn=report["turn"])
                       for did, d in world.depots.items()},
            "wings": {wid: command_v2.compute_operational_ao_buff(w.base_px, w.team, world.ao_zones, world.commanders, current_turn=report["turn"])
                      for wid, w in world.wings.items()},
        }
        if world.ao_zones
        else {}
    )

    # 10 总部混编
    merges = []
    for m in orders.get("merge") or []:
        merges.append(form_kampfgruppe(world, m["hq"], float(m.get("radius_px", 10)), m["new_id"],
                                       m.get("filter", "depleted"), m.get("new_name")))
    report["merges"] = merges

    # 11 环境衰减与物理时间推进（震荡/Buff/超限组织度回落一律走物理小时，不受身份 turn_hours 缩放）
    tf.tick_environment(g, hours)
    for p in world.pieces.values():
        if p.shock_hours > 0:
            p.shock_hours = max(0.0, p.shock_hours - hours)
        for sq in p.squads():
            sq.decay_overcap_org(hours)
    for c in world.commanders.values():
        c.tick_hours(hours)
    for team_ic, ch_map in list(world.intercepted_channels.items()):
        for ch_name in list(ch_map):
            ch_map[ch_name] -= hours
            if ch_map[ch_name] <= 1e-9:
                del ch_map[ch_name]
        if not ch_map:
            del world.intercepted_channels[team_ic]
    report["channel_intercepts"] = channel_intercepts
    world.clock_hours += hours
    world.turn += 1
    report["turn"] = world.turn
    report["clock_hours"] = round(world.clock_hours, 3)

    # 11.5 汇总全图暴露度加法场 + 地上实体电台暴露 + 事件引擎（活动事件锁定/Buff推进 + 作战会议 + 超载故障）
    for p in world.pieces.values():
        if p.vehicle_model and p.status_debuffs.get("destroyed") and (p.cargo.get("resources") or p.cargo.get("equipment")):
            drop_res = {k: round(v * 0.5, 4) for k, v in p.cargo["resources"].items() if v > 1e-6}
            drop_eq = {k: max(0, int(v // 2)) for k, v in p.cargo["equipment"].items() if v > 0}
            if drop_res or drop_eq:
                g.stash_put(p.px, resources=drop_res, equipment=drop_eq, owner_team=p.team)
            p.cargo = {"resources": {}, "equipment": {}}

    g.reset_exposure()
    for p in world.pieces.values():
        ev_exp = world.event_manager.piece_modifiers(p, world=world)["exposure_delta"]
        cell_exp_mult = g.get_cell_modifiers(p.px)["exposure_mult"]
        g.add_exposure(p.px, (world.exposure_tracker.piece_exposure(p) + ev_exp) * cell_exp_mult)
    for c in world.commanders.values():
        if c.alive:
            g.add_exposure(c.px, world.exposure_tracker.commander_exposure(c))
    for b in world.batteries.values():
        g.add_exposure(b.px, b.exposure())
    for s_px, st in g.cell_stash.items():
        eq = st.get("equipment") or {}
        if eq.get("hq_radio", 0) > 0:
            g.add_exposure(s_px, 1.8 * eq["hq_radio"])
        if eq.get("radio", 0) > 0:
            g.add_exposure(s_px, 0.6 * eq["radio"])
    g.tick_cell_modifiers(hours)
    ev_out = events.evaluate_events(world, hours)
    report["completed_events"] = ev_out["completed_events"]
    report["war_councils"] = ev_out["war_councils"]
    report["overload_faults"] = ev_out["overload_faults"]

    # 12 迷雾 + 面板 + 指纹（受营级 AO 侦察迷雾 Buff 加成）
    pieces = list(world.pieces.values())
    def _recon_mult(px, team):
        if not world.ao_zones:
            return 1.0
        return command_v2.compute_operational_ao_buff(px, team, world.ao_zones, world.commanders, current_turn=report["turn"])["recon_mult"]
    intel = recon_fog.gather_intel(g, pieces, air_res.photos, world.night,
                                   recon_mult_fn=_recon_mult if world.ao_zones else None,
                                   intercepted_channels=world.intercepted_channels)
    report["fog"] = {t: recon_fog.fog_view(t, pieces, intel, g, world.turn) for t in world.teams()}
    for t, fv in report["fog"].items():
        fv["commanders"] = [
            {"id": c.id, "name": c.name, "px": list(c.px), "rank": c.rank, "echelon": c.echelon,
             "alive": c.alive, "radio_channel": c.radio_channel, "radio_silence": c.radio_silence,
             "attached_to": c.attached_to, "meta": dict(getattr(c, "meta", {}) or {})}
            for c in world.commanders.values() if c.team == t
        ]
        fv["intercepted_channels"] = {
            ch: round(ttl, 2) for ch, ttl in world.intercepted_channels.get(t, {}).items()
        }
        fv["ao_zones"] = [
            {"id": z.zone_id, "echelon": z.echelon, "commander_id": z.commander_id, "active": z.activate_turn <= report["turn"],
             "cells": [list(rc) for rc in sorted(z.cells)]}
            for z in world.ao_zones.values() if z.team == t
        ]
        fv["wire_edges"] = [[list(a), list(b)] for a, b in sorted(g.wire_edges.get(t, ()))]
        fv["cell_stash"] = [
            {"px": [r, c], "resources": dict(st.get("resources") or {}), "equipment": dict(st.get("equipment") or {})}
            for (r, c), st in sorted(g.cell_stash.items())
            if st.get("owner_team") in (None, t)
        ]
    report["truth"] = recon_fog.truth_state(pieces)
    report["panel"] = pressure_panel(world, air_res, ff, logi_report, borrowed, casualties)
    report["fingerprint"] = fingerprint(report["panel"], world)
    return report


# ==========================================
# 🧩 总部混编
# ==========================================
def form_kampfgruppe(world, hq_id, radius_px, new_id, flt="depleted", new_name=None):
    """连以上总部在回合末把周边残部/散落装备整合成一个新的混编战斗群。"""
    hq = world.pieces.get(hq_id)
    if hq is None:
        return {"ok": False, "reason": "unknown_hq"}
    picked, sources = [], []
    for p in sorted(world.pieces.values(), key=lambda x: x.id):
        if p.team != hq.team:
            continue
        if math.hypot(p.px[0] - hq.px[0], p.px[1] - hq.px[1]) > radius_px:
            continue
        for sq in p.squads():
            if sq.kind == "hq_squad":
                continue
            if flt == "depleted" and sq.enlisted_ratio >= DEPLETED_RATIO and sq.equipment_ratio >= DEPLETED_RATIO:
                continue
            picked.append(sq)
            sources.append(p)
    loose = {}
    for key in list(world.grid.cell_stash):
        stash = world.grid.cell_stash.get(key)
        if not stash or not stash.get("equipment"):
            continue
        owner = stash.get("owner_team")
        if (owner is None or owner == hq.team) and math.hypot(key[0] - hq.px[0], key[1] - hq.px[1]) <= radius_px:
            eq_take = dict(stash["equipment"])
            world.grid.stash_take(key, equipment=eq_take)
            for m, n in eq_take.items():
                loose[m] = loose.get(m, 0) + n
    if not picked and not loose:
        return {"ok": False, "reason": "nothing_to_merge"}
    for sq, src in zip(picked, sources):
        src.root.remove_squad(sq.id)
    merged = toe.merge_squads(picked, f"{new_id}-MIX", loose_equipment=loose)
    root = toe.Node(new_id, new_name or new_id, "battalion",
                    [toe.Node(merged.id, merged.id, "squad", squad=merged)])
    kg = toe.MapPiece(new_id, new_name or new_id, hq.team, hq.px, root, level="battalion")
    kg.set_full_stock(0.0)
    world.add_piece(kg)
    return {"ok": True, "new_piece": new_id, "squads_merged": [s.id for s in picked],
            "loose_equipment": loose, "enlisted": merged.enlisted, "cadre": merged.cadre,
            "org": round(merged.org, 1), "equipment": dict(merged.equipment)}


# ==========================================
# 📊 客观压力面板 + 指纹
# ==========================================
def _retreat_closure(world, piece, cost_cache, control_cache):
    g = world.grid
    mob = piece.mobility()
    n = max(2, int(g.km_to_px(RAY_KM)))
    H, W = g.shape
    if H * W > 65536:
        r0 = max(0, piece.px[0] - n - 1)
        r1 = min(H, piece.px[0] + n + 2)
        c0 = max(0, piece.px[1] - n - 1)
        c1 = min(W, piece.px[1] + n + 2)
        sub = tf._SubGridWindow(g, r0, r1, c0, c1)
        sub_inf = np.isinf(tf.cost_layer(sub, mob))
        enemies = [
            (ep.px, float(g.km_to_px(ep.control_radius_km)) ** 2)
            for ep in world.pieces.values()
            if ep.team != piece.team
        ]
        blocked = 0
        for i in range(RAY_COUNT):
            ang = 2 * math.pi * i / RAY_COUNT
            hit = False
            for s in range(2, n + 1):
                r = int(round(piece.px[0] + math.sin(ang) * s))
                c = int(round(piece.px[1] + math.cos(ang) * s))
                if not (0 <= r < H and 0 <= c < W):
                    break
                if bool(sub_inf[r - r0, c - c0]) or any(
                    (r - epx[0]) ** 2 + (c - epx[1]) ** 2 <= erad2 for epx, erad2 in enemies
                ):
                    hit = True
                    break
            blocked += hit
        return blocked / RAY_COUNT
    if mob not in cost_cache:
        cost_cache[mob] = np.isinf(tf.cost_layer(g, mob))
    if piece.team not in control_cache:
        control_cache[piece.team] = _control_mask(world, piece.team)
    bad = cost_cache[mob] | control_cache[piece.team]
    blocked = 0
    for i in range(RAY_COUNT):
        ang = 2 * math.pi * i / RAY_COUNT
        hit = False
        for s in range(2, n + 1):
            r = int(round(piece.px[0] + math.sin(ang) * s))
            c = int(round(piece.px[1] + math.cos(ang) * s))
            if not (0 <= r < H and 0 <= c < W):
                break
            if bad[r, c]:
                hit = True
                break
        blocked += hit
    return blocked / RAY_COUNT


def pressure_panel(world, air_res, ff, logi_report, borrowed, casualties):
    cost_cache, control_cache = {}, {}
    panel = {}
    for p in sorted(world.pieces.values(), key=lambda x: x.id):
        sq = p.squads()
        tot = p.totals()
        orgs = [s.org for s in sq] or [0.0]
        haz = ff.hazard_for(p.team)
        cap = p.stock_cap
        risks = []
        assist = _engineer_assist(p)
        for s in sq:
            for m, v in combat.abandonment_risk(s, assist).items():
                if toe.EQUIPMENT.get(m, {}).get("weight") == "heavy":
                    risks.append(v)
        panel[p.id] = {
            "team": p.team, "px": list(p.px), "mobility": p.mobility(),
            "persons": p.persons(), "enlisted": tot["enlisted"], "cadre": tot["cadre"],
            "cadre_ratio": round(tot["cadre"] / tot["cadre_full"], 3) if tot["cadre_full"] else 1.0,
            "org_mean": round(p.mean_org(), 1), "org_min": round(min(orgs), 1),
            "org_cap_mean": round(sum(s.org_cap() * s.persons for s in sq) / max(1, p.persons()), 1),
            "equipment_ratio": round(sum(tot["equipment"].values()) / max(1, sum(tot["equipment_full"].values())), 3),
            "stock_ratio": {k: round(p.stock.get(k, 0.0) / cap[k], 3) if cap.get(k) else 0.0
                            for k in ("food", "ammo", "fuel")},
            "fire_hazard_here": round(float(haz[p.px]), 3) if haz is not None else 0.0,
            "air_status": air_res.status_at(p.px, p.team),
            "retreat_closure": round(_retreat_closure(world, p, cost_cache, control_cache), 3),
            "forced_withdraw_heavy_loss_risk": round(sum(risks) / len(risks), 3) if risks else 0.0,
            "radio_silence": p.radio_silence,
            "borrowed_soldiers": borrowed.get(p.id, 0),
            "supply_status": (logi_report.get(p.id) or {}).get("status", "n/a"),
            "casualties_this_turn": {k: casualties[p.id][k] for k in ("enlisted", "cadre")} if p.id in casualties
            else {"enlisted": 0, "cadre": 0},
        }
    return panel


def fingerprint(panel, world):
    payload = {"turn": world.turn, "hours": world.hours, "panel": panel}
    raw = json.dumps(payload, ensure_ascii=False, sort_keys=True, default=str)
    return hashlib.sha1(raw.encode("utf-8")).hexdigest()[:12]


def run_turn_v3(world, orders=None, subtick_hours=MIN_TICK_HOURS, total_hours=None):
    """V3 主推演入口：外部回合按指挥官身份耗时伸缩，内部严格按 5 分钟(1/12 h)确定性 Sub-tick 循环推进。"""
    orders = dict(orders or {})
    if total_hours is not None:
        turn_hours = max(MIN_TICK_HOURS, float(total_hours))
    elif "hours" in orders:
        turn_hours = max(MIN_TICK_HOURS, float(orders["hours"]))
    elif "role" in orders:
        turn_hours = float(world.set_role(orders["role"]))
    else:
        turn_hours = max(MIN_TICK_HOURS, float(world.hours))

    saved_hours = float(world.hours)
    sub_dt = max(MIN_TICK_HOURS, float(subtick_hours))
    n_ticks = max(1, int(round(turn_hours / sub_dt)))
    tick_dt = turn_hours / float(n_ticks)

    # 先写入显式提交的分兵种战线势场到地形容器
    for idx_fl, fl in enumerate(orders.get("typed_frontlines") or []):
        if not isinstance(fl, dict) or not fl.get("points_px"):
            continue
        cid_fl = fl.get("commander_id") or fl.get("commander")
        cmd_fl = world.commanders.get(cid_fl) if cid_fl else None
        team_fl = fl.get("team") or (cmd_fl.team if cmd_fl else "red")
        lid_fl = str(fl.get("line_id") or f"fl-v3-{world.turn}-{idx_fl}")
        world.grid.write_typed_frontline(
            lid_fl, team_fl, fl["points_px"],
            unit_type_mask=int(fl.get("unit_type_mask", tf.LINE_MASK_ALL)),
            attraction_weight=float(fl.get("attraction_weight", 1.0)),
            front_normal=fl.get("front_normal"),
            commander_id=cid_fl,
            zone_id=fl.get("zone_id"),
            spread_radius_px=int(fl.get("spread_radius_px", 4)),
            line_role=str(fl.get("line_role", tf.LINE_ROLE_DEPLOY)),
            source_deploy_line_id=fl.get("source_deploy_line_id"),
            target_deploy_line_id=fl.get("target_deploy_line_id"),
        )
        if cmd_fl is not None and fl.get("count_churn", True):
            baguwen_v3.record_commander_line_churn(world, cmd_fl, amount=float(fl.get("churn_amount", 1.0)))

    # 下达本回合的作战日志下行命令（一次性基于 200 m/px 地形动态代价规划路线）
    baguwen_receipts = []
    combined_log_orders = list(orders.get("baguwen_orders") or []) + list(orders.get("combat_log_orders") or [])
    for b_ord in combined_log_orders:
        cid = b_ord.get("commander_id") or b_ord.get("commander")
        author = b_ord.get("author_commander_id", cid)
        res = baguwen_v3.issue_downlink_baguwen_order(world, cid, b_ord, author_commander_id=author)
        baguwen_receipts.append(res)

    active_manual_moves = copy.deepcopy(orders.get("moves") or {})
    subtick_logs = []
    all_mid_tier_actions = []
    last_report = None
    base_turn_tick = int(getattr(world, "_v3_total_ticks", int(getattr(world, "turn", 0))))

    for tick_i in range(n_ticks):
        # 0. 重建 O(1) 格网空间桶索引（彻底禁止 O(N^2) 全图遍历）
        autonomy_v3.build_spatial_index(world, bucket_px=2)

        # 1. 记录 5 分钟前快照（供 5m Delta 与三矢量判定器使用）
        snap = {}
        for pid, p in world.pieces.items():
            snap[pid] = (int(p.persons()), float(p.mean_org()))
            p.meta["prev_5m_snapshot"] = list(snap[pid])

        # 2. 断连检测（触发红/黄! k_C 漂移）+ 400m 友军局部情报汲取（不代报）
        for pid in sorted(world.pieces.keys()):
            p = world.pieces[pid]
            if p.persons() <= 0 and not getattr(p, "vehicle_model", None):
                continue
            baguwen_v3.harvest_nearby_friendly_intel(world, p, radius_km=0.4)

        # 2.5 先派发到达传达延迟 τ 的直属下行队列，再驱动全层级自治指挥官脚本
        baguwen_v3.tick_pending_downlink_orders(world, tick_dt)
        mid_tier_res = baguwen_v3.step_mid_tier_commanders(
            world, tick_index=base_turn_tick + tick_i, subtick_hours=tick_dt
        )
        if mid_tier_res:
            all_mid_tier_actions.append({"subtick": tick_i + 1, "actions": mid_tier_res})

        # 3. 5 分钟自治判定 + 载具软超载检定 + 自主线性执行（显式 moves 单位交由物理机动推进，自治不强改坐标）
        auto_decisions = autonomy_v3.step_world_autonomy_5m(
            world, dt_hours=tick_dt, execute_move=True, skip_move_pids=set(active_manual_moves.keys())
        )
        hq_moves = baguwen_v3.step_hq_accompaniment(world, dt_hours=tick_dt)

        # 4. 组装子回合指令运行物理/后勤/交战/事件结算
        if tick_i == 0:
            sub_orders = dict(orders)
            sub_orders.pop("baguwen_orders", None)
            sub_orders.pop("combat_log_orders", None)
            sub_orders.pop("typed_frontlines", None)
            if active_manual_moves:
                sub_orders["moves"] = active_manual_moves
        else:
            sub_orders = {}
            if active_manual_moves:
                sub_orders["moves"] = active_manual_moves
        sub_orders["hours"] = tick_dt
        if "night" in orders:
            sub_orders["night"] = orders["night"]

        rep = run_turn(world, sub_orders)
        baguwen_v3.tick_pending_downlink_orders(world, 0.0)
        last_report = rep

        # 4.5 消费已到达的航路点，防止后续 5 分钟子步重复推进与速度倍增
        if rep and "moves" in rep:
            for m_pid, m_rep in rep["moves"].items():
                if m_rep.get("arrived", False) or m_rep.get("status") in {"arrived", "unreachable", "locked_insufficient_crew"}:
                    spec = active_manual_moves.get(m_pid)
                    if isinstance(spec, dict) and "waypoints" in spec and spec["waypoints"]:
                        spec["waypoints"].pop(0)
                        if not spec["waypoints"]:
                            active_manual_moves.pop(m_pid, None)
                    else:
                        active_manual_moves.pop(m_pid, None)

        # 5. 更新近 5 分钟人员与组织度 Delta
        for pid, p in world.pieces.items():
            old_p, old_org = snap.get(pid, (p.persons(), p.mean_org()))
            p.meta["delta_persons_5m"] = int(p.persons()) - int(old_p)
            p.meta["delta_org_5m"] = round(float(p.mean_org()) - float(old_org), 2)

        subtick_logs.append({
            "subtick": tick_i + 1,
            "dt_hours": round(tick_dt, 5),
            "clock_hours": rep.get("clock_hours"),
            "actions": {pid: dec.get("action") for pid, dec in auto_decisions.items()},
            "atomic_actions": {pid: dec.get("atomic_action") for pid, dec in auto_decisions.items()},
            "mid_tier_actions": mid_tier_res,
            "hq_moves": hq_moves,
        })

    if "hours" not in orders and total_hours is None and "role" not in orders:
        world.hours = saved_hours
    world._v3_total_ticks = base_turn_tick + n_ticks

    # 6. 回合末收集各指挥官确定性上行作战日志（断连直接没有东西）
    uplink_by_commander = {}
    for cid in sorted(world.commanders.keys()):
        if world.commanders[cid].alive:
            uplink_by_commander[cid] = baguwen_v3.collect_commander_uplink_baguwen(world, cid)

    out = dict(last_report or {})
    out["v3_subtick_count"] = n_ticks
    out["v3_total_hours"] = round(turn_hours, 4)
    out["v3_subticks"] = subtick_logs
    out["baguwen_receipts"] = baguwen_receipts
    out["combat_log_receipts"] = baguwen_receipts
    out["baguwen_uplink"] = uplink_by_commander
    out["combat_log_uplink"] = uplink_by_commander
    out["mid_tier_actions"] = all_mid_tier_actions
    return out

