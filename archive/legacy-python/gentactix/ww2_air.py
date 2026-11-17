# -*- coding: utf-8 -*-
"""航空兵（V2）：1 小时 Sub-tick、野战机场、战斗机扫荡/低空扫射、CAS、照相侦察。

地面主回合（6 小时 / 1 天…）内部嵌套以 1 小时为步长的空中子循环：
  起飞（需野战机场像素完好、天气可飞、目标在作战半径内）→ 巡逻/打击 → 返航 → 整备冷却。
航空兵免疫地面地形减速；受限于机场作战半径与整备冷却。

制空权按「目标空域内各队战斗机有效兵力」裁出：superior / contested / inferior / unopposed。
- 战斗机扫荡：夺权；夺权后富余战斗机可转低空扫射（压低敌方机动、截杀搬运队）；
- CAS（强击机/轰炸机）：有制空全效，争夺中减半，无制空强行出动则效果锐减、损失倍增；
- 烟雾/浓雾降低目标处能见度 → 俯冲/轰炸瞄准效果下降；
- 防空班组（flak）覆盖的像素：低空飞机吃硬战损，扫射危险度被压低。
"""

from __future__ import annotations

import math

import numpy as np

from . import terrain_field as tf
from .artillery import FireFields

ROLE_DEFAULTS = {
    "fighter":   dict(range_km=250.0, loiter_h=2.0, turnaround_h=3.0, payload={"small_arms": 8.0}),
    "attack":    dict(range_km=200.0, loiter_h=1.0, turnaround_h=3.0, payload={"he_blast": 18.0, "ap_kinetic": 10.0}),
    "bomber":    dict(range_km=400.0, loiter_h=1.0, turnaround_h=5.0, payload={"he_blast": 45.0}),
    "recon":     dict(range_km=350.0, loiter_h=1.0, turnaround_h=3.0, payload={}),
    "transport": dict(range_km=450.0, loiter_h=1.0, turnaround_h=4.0, payload={}, capacity_t=4.0),
}
GROUNDING_WEATHER = (tf.FOG, tf.SNOW, tf.ICE)

SUPERIORITY_RATIO = 1.5
SWEEP_WEIGHT = 1.0
STRAFE_WEIGHT = 0.4
DOGFIGHT_LOSS_RATE = 0.10          # 每小时、对等兵力下的基准损失率
AA_LOSS_PER_FLAK_H = 0.01
INTERCEPT_LOSS_BASE = 0.015        # 无敌机拦截时的基础损失（事故/零星高炮）
INTERCEPT_LOSS_CONTESTED = 0.04
INTERCEPT_LOSS_INFERIOR = 0.08
STRAFE_K = 0.02
STRAFE_REFERENCE_PX = 5.0
STRAFE_SLOW = 1.5
EFFECT_BY_STATUS = {"superior": 1.0, "unopposed": 1.0, "contested": 0.5, "inferior": 0.25}
PHOTO_BY_STATUS = {"superior": 1.0, "unopposed": 1.0, "contested": 0.6, "inferior": 0.2}


class AirWing:
    def __init__(self, wid, team, base_px, role, aircraft, quality=1.0, aircraft_full=None, **over):
        d = dict(ROLE_DEFAULTS[role])
        d.update(over)
        self.id = wid
        self.team = team
        self.base_px = (int(base_px[0]), int(base_px[1]))
        self.role = role
        self.aircraft = int(aircraft)
        self.aircraft_full = int(aircraft if aircraft_full is None else aircraft_full)
        self.quality = float(quality)
        self.range_km = float(d["range_km"])
        self.loiter_h = float(d["loiter_h"])
        self.turnaround_h = float(d["turnaround_h"])
        self.payload = dict(d["payload"])
        self.ready_in_h = 0.0
        self.airborne_left = 0.0
        self.carry = 0.0
        self.state = "ground"            # ground / on_station / returning / turnaround
        self.assigned_zone = None        # {"px": [r, c], "radius_km": float, "mission": str}
        self.air_radio_radius_km = float(d.get("air_radio_radius_km", 8.0))
        self.meta = dict(over.get("meta") or {})

    def eff(self):
        return self.aircraft * self.quality

    def to_dict(self):
        return dict(id=self.id, team=self.team, base_px=list(self.base_px), role=self.role,
                    aircraft=self.aircraft, aircraft_full=self.aircraft_full, quality=self.quality,
                    range_km=self.range_km, loiter_h=self.loiter_h, turnaround_h=self.turnaround_h,
                    payload=dict(self.payload), ready_in_h=self.ready_in_h,
                    airborne_left=self.airborne_left, carry=self.carry,
                    state=self.state, assigned_zone=self.assigned_zone,
                    air_radio_radius_km=self.air_radio_radius_km,
                    meta=dict(self.meta))

    @classmethod
    def from_dict(cls, d):
        w = cls(d["id"], d["team"], d["base_px"], d["role"], d["aircraft"], d["quality"], d["aircraft_full"],
                range_km=d["range_km"], loiter_h=d["loiter_h"], turnaround_h=d["turnaround_h"],
                payload=d["payload"], meta=d.get("meta"))
        w.ready_in_h = d.get("ready_in_h", 0.0)
        w.airborne_left = d.get("airborne_left", 0.0)
        w.carry = d.get("carry", 0.0)
        w.state = d.get("state", "ground")
        w.assigned_zone = d.get("assigned_zone")
        w.air_radio_radius_km = float(d.get("air_radio_radius_km", 8.0))
        return w


def has_air_ground_liaison(grid, team, center_px, radius_km, pieces):
    """检查目标空域内是否存在未无线电静默且具备空地协同编制/电台的友军。"""
    for p in pieces:
        if p.team != team or p.radio_silence:
            continue
        if grid.distance_km(p.px, center_px) > radius_km:
            continue
        if p.has_buff("air_liaison") or any(s.equipment.get("fac_radio", 0) > 0 for s in p.squads()):
            return True
    return False


def can_receive_air_order(grid, wing, pieces):
    """航空受令锁：地面仅在机场/机场无线电圈可受令；空域中仅靠空地协同部队方可中途改令。"""
    if wing.aircraft <= 0:
        return False, "no_aircraft"
    if wing.state in ("turnaround", "returning") or (wing.state == "ground" and wing.ready_in_h > 0):
        return False, "wing_not_ready"
    if wing.state == "ground":
        if int(grid.terrain[wing.base_px]) == tf.AIRFIELD:
            return True, "at_airfield"
        # 检查是否在邻近完好野战机场的无线电覆盖半径内
        rad_px = int(math.ceil(grid.km_to_px(wing.air_radio_radius_km)))
        H, W = grid.shape
        r0, r1 = max(0, wing.base_px[0] - rad_px), min(H, wing.base_px[0] + rad_px + 1)
        c0, c1 = max(0, wing.base_px[1] - rad_px), min(W, wing.base_px[1] + rad_px + 1)
        sub = grid.terrain[r0:r1, c0:c1]
        ys, xs = np.where(sub == tf.AIRFIELD)
        for dy, dx in zip(ys, xs):
            if grid.distance_km(wing.base_px, (r0 + int(dy), c0 + int(dx))) <= wing.air_radio_radius_km:
                return True, "airfield_radio"
        return False, "outside_airfield_radio"
    if wing.state == "on_station":
        az = wing.assigned_zone or {}
        center = tuple(az.get("px", wing.base_px))
        rad_km = float(az.get("radius_km", 8.0))
        if has_air_ground_liaison(grid, wing.team, center, rad_km, pieces):
            return True, "air_ground_liaison"
        return False, "no_air_ground_liaison"
    return False, "invalid_state"


def assign_air_mission(grid, wing, mission_spec, pieces):
    """按航空受令锁指派或中途改派空域任务。"""
    ok, reason = can_receive_air_order(grid, wing, pieces)
    if not ok:
        return {"accepted": False, "reason": reason, "wing": wing.id}
    px = tuple(mission_spec["px"])
    if grid.distance_km(wing.base_px, px) > wing.range_km:
        return {"accepted": False, "reason": "out_of_range", "wing": wing.id}
    loiter_h = float(mission_spec.get("loiter_h", wing.loiter_h))
    wing.state = "on_station"
    wing.assigned_zone = {
        "px": [int(px[0]), int(px[1])],
        "radius_km": float(mission_spec.get("radius_km", 8.0)),
        "mission": mission_spec.get("mission", "cas"),
    }
    if wing.airborne_left <= 0:
        wing.airborne_left = loiter_h
    return {"accepted": True, "reason": reason, "wing": wing.id, "assigned_zone": wing.assigned_zone}


class AirResult:
    def __init__(self, shape):
        self.strikes = []
        self.fields = FireFields(shape)
        self.log = []                 # 每小时每个空域的制空裁决
        self.losses = {}              # wing_id -> 损失架数
        self.photos = []
        self.airdrops = []
        self.paradrops = []
        self.events = []
        self.zones_last = []          # 最后一小时的战斗机空域快照

    def lose(self, wing, n):
        if n:
            self.losses[wing.id] = self.losses.get(wing.id, 0) + n

    def status_at(self, px, team):
        """从 team 视角看某像素的制空状态（用最后一小时的战斗机空域）。"""
        eff = {}
        for z in self.zones_last:
            if math.hypot(z["px"][0] - px[0], z["px"][1] - px[1]) <= z["radius_px"]:
                eff[z["team"]] = eff.get(z["team"], 0.0) + z["eff"]
        return _status(eff.get(team, 0.0), sum(v for t, v in eff.items() if t != team))


def _status(own, enemy):
    if own <= 0 and enemy <= 0:
        return "unopposed"
    if enemy <= 0:
        return "superior"
    if own <= 0:
        return "inferior"
    if own / enemy >= SUPERIORITY_RATIO:
        return "superior"
    if enemy / own >= SUPERIORITY_RATIO:
        return "inferior"
    return "contested"


def _take_aircraft(wing, expected):
    total = wing.carry + max(0.0, expected)
    n = min(int(total), wing.aircraft)
    wing.carry = total - n if n < wing.aircraft else 0.0
    wing.aircraft -= n
    return n


def _aa_at(aa_sites, team, px):
    """敌方（对 team 而言）防空在 px 处的总强度。"""
    s = 0.0
    for a in aa_sites:
        if a["team"] != team and math.hypot(a["px"][0] - px[0], a["px"][1] - px[1]) <= a["radius_px"]:
            s += a["strength"]
    return s


def run_air(grid, wings, orders, aa_sites, hours, night=False, air_mult_fn=None, pieces=()):
    """跑一个地面回合内的全部空中子回合。原地修改 wings 状态，返回 AirResult。

    orders: [{wing, mission: sweep|strafe|cas|recon|airdrop_supply|paradrop, target:(r,c), radius_px}]
    aa_sites: [{team, px, radius_px, strength}]
    air_mult_fn: 可选 (px, team) -> float，由团/旅/师级 AO 提供的航空协同/整备加成
    """
    res = AirResult(grid.shape)
    by_id = {w.id: w for w in wings}
    order_of = {}
    for o in orders:
        if o["wing"] in by_id:
            order_of[o["wing"]] = o
    # 跨回合续飞：处于 on_station 且仍有留空时间的联队自动接续原空域任务
    for w in wings:
        if w.state == "on_station" and w.airborne_left > 1e-9 and w.assigned_zone and w.id not in order_of:
            az = w.assigned_zone
            order_of[w.id] = {
                "wing": w.id,
                "mission": az.get("mission", "cas"),
                "target": tuple(az["px"]),
                "radius_px": float(grid.km_to_px(az.get("radius_km", 8.0))),
            }
        elif w.airborne_left > 0 and w.id not in order_of:
            w.airborne_left = 0.0
            w.state = "turnaround"
            w.assigned_zone = None
            w.ready_in_h = max(w.ready_in_h, w.turnaround_h)

    steps = max(1, int(math.ceil(hours - 1e-9)))
    vis_field = tf.visibility_field(grid, night)
    for step in range(steps):
        dt = min(1.0, hours - step)
        # ---- 起飞 ----
        for w in wings:
            o = order_of.get(w.id)
            if o is None or w.aircraft <= 0 or w.airborne_left > 0 or w.ready_in_h > 1e-9:
                continue
            tgt = (int(o["target"][0]), int(o["target"][1]))
            if grid.terrain[w.base_px] != tf.AIRFIELD:
                res.events.append(f"h{step}: {w.id} 基地像素不是完好的野战机场，无法起飞")
                continue
            if int(grid.weather[w.base_px]) in GROUNDING_WEATHER:
                res.events.append(f"h{step}: {w.id} 机场天气不适航，停飞")
                continue
            if grid.distance_km(w.base_px, tgt) > w.range_km:
                res.events.append(f"h{step}: {w.id} 目标超出作战半径 {w.range_km:.0f} km")
                continue
            w.airborne_left = w.loiter_h
            w.state = "on_station"
            w.assigned_zone = {
                "px": [tgt[0], tgt[1]],
                "radius_km": float(o.get("radius_px", 8)) * grid.cell_m / 1000.0,
                "mission": o["mission"],
            }
        active = [w for w in wings if w.airborne_left > 1e-9 and w.aircraft > 0 and w.id in order_of]

        # ---- 空域快照：战斗机的存在 ----
        zones = []
        for w in active:
            o = order_of[w.id]
            if w.role == "fighter" and o["mission"] in ("sweep", "strafe"):
                weight = SWEEP_WEIGHT if o["mission"] == "sweep" else STRAFE_WEIGHT
                zones.append({"team": w.team, "wing": w.id, "px": tuple(o["target"]),
                              "radius_px": float(o.get("radius_px", 10)), "eff": w.eff() * weight})
        res.zones_last = zones

        def eff_at(px):
            e = {}
            for z in zones:
                if math.hypot(z["px"][0] - px[0], z["px"][1] - px[1]) <= z["radius_px"]:
                    e[z["team"]] = e.get(z["team"], 0.0) + z["eff"]
            return e

        # ---- 战斗机对战斗机：空战损耗 ----
        for w in active:
            o = order_of[w.id]
            if w.role != "fighter" or o["mission"] not in ("sweep", "strafe"):
                continue
            own = [z for z in zones if z["wing"] == w.id][0]
            enemy_eff = sum(z["eff"] for z in zones if z["team"] != w.team and
                            math.hypot(z["px"][0] - own["px"][0], z["px"][1] - own["px"][1])
                            <= z["radius_px"] + own["radius_px"])
            if enemy_eff > 0:
                frac = DOGFIGHT_LOSS_RATE * dt * enemy_eff / (own["eff"] + enemy_eff)
                res.lose(w, _take_aircraft(w, w.aircraft * frac))
        for w in active:
            if w.aircraft <= 0:
                continue
            o = order_of[w.id]
            tgt = (int(o["target"][0]), int(o["target"][1]))
            radius = float(o.get("radius_px", 6))
            e = eff_at(tgt)
            status = _status(e.get(w.team, 0.0), sum(v for t, v in e.items() if t != w.team))
            aa = _aa_at(aa_sites, w.team, tgt)
            vis = float(vis_field[tgt])
            vis_factor = min(1.0, max(0.1, (vis - 0.15) / 0.55))

            if o["mission"] == "sweep":
                res.log.append({"hour": step, "wing": w.id, "target": tgt, "status": status})
                continue

            # 低空任务的拦截/高炮损失
            if w.role == "fighter":
                intercept = 0.0   # 战斗机之间的损失已在上面空战里结算
            else:
                intercept = {"superior": INTERCEPT_LOSS_BASE, "unopposed": INTERCEPT_LOSS_BASE,
                             "contested": INTERCEPT_LOSS_CONTESTED, "inferior": INTERCEPT_LOSS_INFERIOR}[status]
            flak = min(0.5, AA_LOSS_PER_FLAK_H * aa)
            res.lose(w, _take_aircraft(w, w.aircraft * dt * (intercept + flak)))
            if w.aircraft <= 0:
                continue
            res.log.append({"hour": step, "wing": w.id, "target": tgt, "status": status, "aa": aa,
                            "vis_factor": round(vis_factor, 3)})

            victim = None
            teams = {x.team for x in wings}
            if pieces:
                teams |= {p.team for p in pieces.values()}
            for t in sorted(teams):
                if t != w.team:
                    victim = t
                    break
            tgt_mult = float(air_mult_fn(tgt, w.team)) if air_mult_fn else 1.0
            effect = EFFECT_BY_STATUS[status] * tgt_mult / (1.0 + 0.2 * aa)
            fac_guided = has_air_ground_liaison(grid, w.team, tgt, 8.0, pieces) if pieces else False
            guide_mult = 1.35 if fac_guided else 0.75
            air_dist_m = 300.0 if fac_guided else 1000.0

            if o["mission"] == "strafe" and w.role == "fighter":
                hazard = min(0.9, w.eff() * STRAFE_K * effect * guide_mult *
                             (STRAFE_REFERENCE_PX / max(1.0, radius)) ** 2 / (1.0 + 0.3 * aa))
                if victim is not None and hazard > 0:
                    mask = tf.disk_mask(grid.shape, tgt, radius)
                    res.fields.add(victim, mask, hazard, 1.0 + STRAFE_SLOW * hazard)
                for dtype, pts in w.payload.items():
                    res.strikes.append({"team": w.team, "damage_type": dtype, "source": w.id,
                                        "intensity": pts * w.eff() * effect * guide_mult * dt * vis_factor,
                                        "px": tgt, "radius_px": radius,
                                        "fac_guided": fac_guided, "dist_m": air_dist_m})
            elif o["mission"] == "cas" and w.role in ("attack", "bomber"):
                for dtype, pts in w.payload.items():
                    res.strikes.append({"team": w.team, "damage_type": dtype, "source": w.id,
                                        "intensity": pts * w.eff() * effect * guide_mult * dt * vis_factor,
                                        "px": tgt, "radius_px": radius,
                                        "fac_guided": fac_guided, "dist_m": air_dist_m})
            elif o["mission"] == "recon":
                q = min(1.0, PHOTO_BY_STATUS[status] * vis_factor * tgt_mult)
                res.photos.append({"team": w.team, "px": tgt, "radius_px": radius, "quality": q,
                                   "hour": step, "wing": w.id})
            elif o["mission"] == "airdrop_supply" and w.role == "transport":
                base_res = o.get("resources") or {"food": 0.5 * w.eff(), "ammo": 1.5 * w.eff(), "fuel": 0.5 * w.eff()}
                scale = max(0.2, effect * dt)
                delivered = {k: round(float(v) * scale, 3) for k, v in base_res.items() if float(v) > 0}
                res.airdrops.append({"team": w.team, "px": tgt, "wing": w.id, "resources": delivered})
            elif o["mission"] == "paradrop" and w.role == "transport":
                res.paradrops.append({"team": w.team, "px": tgt, "wing": w.id,
                                      "piece_id": o.get("piece_id"),
                                      "eff_ratio": round(max(0.2, min(1.0, effect)), 3)})
            else:
                res.events.append(f"h{step}: {w.id}（{w.role}）无法执行 {o['mission']}")

        # ---- 滞空时间/整备 ----
        for w in wings:
            if w.airborne_left > 1e-9 and w.id in order_of:
                w.airborne_left -= dt
                if w.airborne_left <= 1e-9:
                    w.airborne_left = 0.0
                    w.state = "turnaround"
                    w.assigned_zone = None
                    w.ready_in_h = w.turnaround_h
            elif w.ready_in_h > 0:
                base_mult = float(air_mult_fn(w.base_px, w.team)) if air_mult_fn else 1.0
                w.ready_in_h = max(0.0, w.ready_in_h - dt * base_mult)
                if w.ready_in_h <= 1e-9:
                    w.ready_in_h = 0.0
                    w.state = "ground"
    return res
