# -*- coding: utf-8 -*-
"""炮兵多弹种（V2）：HE / AP / HEAT / 白磷 / 烟雾 / 照明。

- 毁伤弹（HE/AP/HEAT/WP）产出两样东西：strike（交给 combat 结算）与 barrage 场
  （hazard：搬运/行军的危险度；mobility_mult：穿过弹幕区的机动迟滞倍率）。
- 烟雾弹直接改写目标像素 visibility_mod，切断直瞄与空中俯冲瞄准线；
- 照明弹在夜间抵消能见度惩罚。
- 没有观测（observed=False）则散布增大、效果打折；射程外/无弹药/无炮管直接拒绝。
"""

from __future__ import annotations

import math

import numpy as np

from . import terrain_field as tf

# 每发有效火力点（按口径档位）
CALIBER_POINTS = {"light": 0.8, "medium": 1.2, "heavy": 2.4}
# 每发弹重（吨），用于补给折算
ROUND_MASS_T = {"light": 0.012, "medium": 0.02, "heavy": 0.045}

SHELLS = {
    "he":    dict(damage="he_blast", mult=1.0, barrage=True),
    "ap":    dict(damage="ap_kinetic", mult=1.0, barrage=False),
    "heat":  dict(damage="heat_shaped", mult=1.0, barrage=False),
    "wp":    dict(damage="incendiary", mult=0.8, barrage=True, smoke=0.6, smoke_ttl=1.0),
    "smoke": dict(damage=None, mult=0.0, barrage=False, smoke=0.92, smoke_ttl=2.0),
    "illum": dict(damage=None, mult=0.0, barrage=False, illum_ttl=1.0),
}

UNOBSERVED_FACTOR = 0.35
BARRAGE_DENSITY_K = 0.9         # hazard = 1 - exp(-K × 弹点密度)
BARRAGE_SLOW = 3.0              # mobility_mult = 1 + SLOW × hazard
BARRAGE_REFERENCE_PX = 8.0


class Battery:
    def __init__(self, bid, team, px, tubes, caliber="medium", range_km=11.0, rate_per_tube_h=30.0,
                 ammo=None, tubes_full=None, attached_to=None, trucks_t=0.0, porters=0, meta=None):
        self.id = bid
        self.team = team
        self.px = (int(px[0]), int(px[1]))
        self.tubes = int(tubes)
        self.tubes_full = int(tubes if tubes_full is None else tubes_full)
        self.caliber = caliber
        self.range_km = float(range_km)
        self.rate = float(rate_per_tube_h)
        self.ammo = {k: int(v) for k, v in (ammo or {}).items()}
        self.attached_to = attached_to
        self.trucks_t = float(trucks_t)
        self.porters = int(porters)
        self.base_exposure = 1.5
        self.firing_exposure_delta = {"light": 2.5, "medium": 4.0, "heavy": 6.0}.get(caliber, 4.0)
        self.firing_exposure_hours = 0.0
        self.firing_exposure_pending = False
        self.meta = dict(meta or {})

    def tick_exposure(self, dt_hours=6.0):
        dt = max(0.0, float(dt_hours))
        if self.firing_exposure_hours > 0:
            self.firing_exposure_hours = max(0.0, self.firing_exposure_hours - dt)
        if self.firing_exposure_pending:
            self.firing_exposure_hours = 12.0
            self.firing_exposure_pending = False

    def exposure(self):
        if self.tubes <= 0:
            return 0.0
        extra = self.firing_exposure_delta if self.firing_exposure_hours > 1e-9 else 0.0
        return self.base_exposure + extra

    def ammo_mass_t(self):
        return sum(self.ammo.values()) * ROUND_MASS_T[self.caliber]

    def to_dict(self):
        return dict(id=self.id, team=self.team, px=list(self.px), tubes=self.tubes,
                    tubes_full=self.tubes_full, caliber=self.caliber, range_km=self.range_km,
                    rate_per_tube_h=self.rate, ammo=dict(self.ammo), attached_to=self.attached_to,
                    trucks_t=self.trucks_t, porters=self.porters,
                    firing_exposure_hours=self.firing_exposure_hours,
                    firing_exposure_pending=self.firing_exposure_pending,
                    meta=dict(self.meta))

    @classmethod
    def from_dict(cls, d):
        b = cls(d["id"], d["team"], d["px"], d["tubes"], d["caliber"], d["range_km"],
                d["rate_per_tube_h"], d.get("ammo"), d.get("tubes_full"),
                d.get("attached_to"), d.get("trucks_t", 0.0), d.get("porters", 0),
                meta=d.get("meta"))
        if "firing_exposure_hours" in d:
            b.firing_exposure_hours = float(d["firing_exposure_hours"])
        else:
            b.firing_exposure_hours = float(d.get("firing_exposure_ttl", 0)) * 6.0
        b.firing_exposure_pending = bool(d.get("firing_exposure_pending", False))
        return b


def fire_mission(grid, battery, shell, target_px, radius_px, hours=1.0, observed=True,
                 night=False, aim_threshold=0.2, accuracy_mult=1.0):
    """执行一次炮击任务。原地扣弹、必要时改写栅格（烟雾/照明）。

    返回 dict：ok / reason / rounds / strike / barrage(可选)。
    """
    res = {"ok": False, "reason": None, "battery": battery.id, "shell": shell, "rounds": 0,
           "strike": None, "barrage": None}
    if shell not in SHELLS:
        res["reason"] = "unknown_shell"
        return res
    spec = SHELLS[shell]
    target_px = (int(target_px[0]), int(target_px[1]))
    if grid.distance_km(battery.px, target_px) > battery.range_km:
        res["reason"] = "out_of_range"
        return res
    if battery.tubes <= 0:
        res["reason"] = "no_tubes"
        return res
    have = battery.ammo.get(shell, 0)
    if have <= 0:
        res["reason"] = "no_ammo"
        return res
    carry_map = battery.meta.setdefault("_rounds_carry", {})
    raw_rounds = float(battery.tubes * battery.rate * hours) + float(carry_map.get(shell, 0.0))
    rounds = int(min(have, math.floor(raw_rounds + 1e-9)))
    carry_map[shell] = round(max(0.0, raw_rounds - rounds), 6)
    if rounds <= 0:
        res["reason"] = "no_fire_capacity"
        return res
    battery.ammo[shell] = have - rounds
    battery.firing_exposure_pending = True
    res["ok"], res["rounds"] = True, rounds

    factor = 1.0 if observed else UNOBSERVED_FACTOR
    # 目标处能见度太差（烟幕/浓雾/夜暗）也会降低效果
    vis = float(tf.visibility_field(grid, night)[target_px])
    if not observed:
        pass
    elif vis < aim_threshold:
        factor *= max(0.4, vis / aim_threshold)
    factor *= max(0.05, float(accuracy_mult))
    res["effectiveness"] = factor
    radius_px = max(1.0, float(radius_px))
    mask = tf.disk_mask(grid.shape, target_px, radius_px)

    if spec["damage"]:
        pts = rounds * CALIBER_POINTS[battery.caliber] * spec["mult"] * factor
        raw_dist_m = grid.distance_km(battery.px, target_px) * 1000.0
        eff_dist_m = min(raw_dist_m, 1500.0) if observed else raw_dist_m
        cal_weapon = {"light": "mortar_81", "medium": "howitzer_105", "heavy": "howitzer_152"}.get(battery.caliber, "howitzer_105")
        res["strike"] = {"team": battery.team, "damage_type": spec["damage"], "intensity": pts,
                         "px": target_px, "radius_px": radius_px, "source": battery.id,
                         "weapon_model": cal_weapon, "dist_m": eff_dist_m,
                         "acc_bonus": (0.35 if observed else 0.0) + max(0.0, float(accuracy_mult) - 1.0)}
        if spec["barrage"]:
            area = max(1.0, float(mask.sum()))
            hazard = 1.0 - math.exp(-BARRAGE_DENSITY_K * pts / area * BARRAGE_REFERENCE_PX)
            res["barrage"] = {"team": battery.team, "px": target_px, "radius_px": radius_px,
                              "hazard": hazard, "mult": 1.0 + BARRAGE_SLOW * hazard}
    if "smoke" in spec:
        res["smoke_pixels"] = tf.apply_smoke(grid, target_px, radius_px, spec["smoke"], spec["smoke_ttl"])
    if "illum_ttl" in spec:
        res["illum_pixels"] = tf.apply_illumination(grid, target_px, radius_px, spec["illum_ttl"])
    return res


def rolling_barrage_mission(grid, battery, shell, start_px, end_px, steps=3, radius_px=3.0,
                            hours=1.0, observed=True, night=False, accuracy_mult=1.0):
    """徐进弹幕（Rolling Barrage）：沿起止路径把炮火分段前推，压制敌军护送步兵跃进。"""
    n_steps = max(2, int(steps))
    s_px = (int(start_px[0]), int(start_px[1]))
    e_px = (int(end_px[0]), int(end_px[1]))
    step_h = max(1e-6, float(hours) / n_steps)
    strikes, barrages, step_pts = [], [], []
    total_rounds = 0
    for i in range(n_steps):
        t = i / float(n_steps - 1)
        px = (int(round(s_px[0] + t * (e_px[0] - s_px[0]))), int(round(s_px[1] + t * (e_px[1] - s_px[1]))))
        sub = fire_mission(grid, battery, shell, px, radius_px, hours=step_h, observed=observed,
                           night=night, accuracy_mult=accuracy_mult)
        if not sub["ok"]:
            if i == 0:
                return {"ok": False, "reason": sub["reason"], "battery": battery.id,
                        "rounds": 0, "strikes": [], "barrages": [], "steps_px": []}
            break
        total_rounds += sub["rounds"]
        step_pts.append(px)
        if sub.get("strike"):
            st = dict(sub["strike"])
            st["rolling_step"] = i
            strikes.append(st)
        if sub.get("barrage"):
            bg = dict(sub["barrage"])
            bg["rolling_step"] = i
            barrages.append(bg)
    return {
        "ok": True,
        "reason": None,
        "battery": battery.id,
        "shell": shell,
        "rolling_barrage": True,
        "rounds": total_rounds,
        "steps_px": step_pts,
        "strikes": strikes,
        "barrages": barrages,
    }


class FireFields:
    """一回合内累计的弹幕/扫射场：按「受害方」分层，供行军与后勤使用。"""

    def __init__(self, shape):
        self.shape = shape
        self.hazard = {}        # victim_team -> (H,W) 0..1
        self.mult = {}          # victim_team -> (H,W) >=1

    def add(self, victim_team, mask, hazard, mult):
        h = self.hazard.setdefault(victim_team, np.zeros(self.shape))
        m = self.mult.setdefault(victim_team, np.ones(self.shape))
        h[mask] = np.maximum(h[mask], hazard)
        m[mask] = np.maximum(m[mask], mult)

    def add_barrage(self, grid, barrage, victim_team):
        mask = tf.disk_mask(grid.shape, barrage["px"], barrage["radius_px"])
        self.add(victim_team, mask, barrage["hazard"], barrage["mult"])

    def hazard_for(self, team):
        return self.hazard.get(team)

    def mult_for(self, team):
        return self.mult.get(team)
