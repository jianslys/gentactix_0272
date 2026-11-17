# -*- coding: utf-8 -*-
"""TO&E 树状编制（V2）：最小到班、人/骨干/装备三账本、混编战斗群。

设计要点
--------
- 地图上移动的实体是 MapPiece（团/旅/师/军…），内部挂一棵 Node 树，叶子是 Squad（班）。
- 每个班三本独立账本：enlisted（普通士兵）、cadre（军官+士官）、equipment（按型号计数）。
- 战斗力由 (装备 × 乘员到位率 × 骨干 × 组织度) 动态算出，所以可以随意重组。
- 组织度（org，0—100）只是**客观压力度量**。本模块不写任何「低于 X 就逃跑」的行为脚本。
- 取整一律走 carry（累计小数）机制：确定性、不丢不重。
"""

from __future__ import annotations

import copy
import math

# ==========================================
# 🔫 装备目录（细分具体型号与弹药口径/燃料）
# ==========================================
# fire      : 每台每小时的有效火力点，按伤害类型分列（伤害类型见 combat.DAMAGE_MATRIX）
# armor     : soft / armored；weight: light / medium / heavy（决定撤收难度）
# crew      : 每台所需乘员；mobility: 该装备自己的机动类别
# ammo_type : 精确弹药口径/弹种（用于后勤弹药补给与缴获可用性判断；None 表示无弹药消耗）
# fuel_type : 燃料类型（gasoline / diesel / None）
EQUIPMENT = {
    "rifle":        dict(name="步枪", armor="soft", weight="light", crew=1, mobility="foot",
                         range_m=400, ammo_type="7.92x57mm", fuel_type=None, fire={"small_arms": 1.0}),
    "lmg":          dict(name="轻机枪", armor="soft", weight="light", crew=1, mobility="foot",
                         range_m=800, ammo_type="7.92x57mm", fuel_type=None, fire={"small_arms": 6.0}),
    "hmg":          dict(name="重机枪", armor="soft", weight="medium", crew=3, mobility="foot",
                         range_m=1800, ammo_type="7.92x57mm", fuel_type=None,
                         deployable=True, deploy_hours=0.25, fire={"small_arms": 16.0}),
    "mortar_81":    dict(name="81mm迫击炮", armor="soft", weight="medium", crew=3, mobility="foot",
                         range_m=4500, frag_radius_m=25.0, ammo_type="81mm_mortar", fuel_type=None,
                         deployable=True, deploy_hours=0.25, fire={"he_blast": 28.0}),
    "at_gun_50":    dict(name="50mm反坦克炮", armor="soft", weight="medium", crew=4, mobility="wheeled",
                         range_m=1500, frag_radius_m=18.0, ammo_type="50mm_pak38", fuel_type=None,
                         deployable=True, deploy_hours=0.5, fire={"ap_kinetic": 40.0}),
    "howitzer_105": dict(name="105mm榴弹炮", armor="soft", weight="heavy", crew=6, mobility="wheeled",
                         range_m=11000, frag_radius_m=60.0, ammo_type="105mm_lefh", fuel_type=None,
                         deployable=True, deploy_hours=0.75, fire={"he_blast": 60.0}),
    "howitzer_152": dict(name="152mm榴弹炮", armor="soft", weight="heavy", crew=8, mobility="wheeled",
                         range_m=17000, frag_radius_m=95.0, ammo_type="152mm_ml20", fuel_type=None,
                         deployable=True, deploy_hours=1.0, fire={"he_blast": 120.0}),
    "flak_88":      dict(name="88mm高射炮", armor="soft", weight="heavy", crew=8, mobility="wheeled",
                         range_m=8000, frag_radius_m=35.0, ammo_type="88mm_flak", fuel_type=None,
                         deployable=True, deploy_hours=0.75, fire={"ap_kinetic": 30.0, "he_blast": 20.0}, aa=1.0),
    "tank_medium":  dict(name="中型坦克(75mm)", armor="armored", weight="heavy", crew=5, mobility="tracked",
                         range_m=2000, frag_radius_m=25.0, ammo_type="75mm_kwk40", fuel_type="gasoline",
                         fire={"ap_kinetic": 28.0, "he_blast": 14.0, "small_arms": 8.0}),
    "tank_heavy":   dict(name="重型坦克(88mm)", armor="armored", weight="heavy", crew=5, mobility="tracked",
                         range_m=2500, frag_radius_m=35.0, ammo_type="88mm_kwk36", fuel_type="gasoline",
                         fire={"ap_kinetic": 55.0, "he_blast": 20.0, "small_arms": 8.0}),
    "halftrack":    dict(name="半履带车", armor="armored", weight="medium", crew=2, mobility="tracked",
                         range_m=800, ammo_type="7.92x57mm", fuel_type="gasoline",
                         fire={"small_arms": 10.0}, carry_t=1.5),
    "truck":        dict(name="卡车", armor="soft", weight="medium", crew=1, mobility="wheeled",
                         range_m=0, ammo_type=None, fuel_type="gasoline", fire={}, carry_t=3.0),
    "tractor":      dict(name="履带牵引车", armor="soft", weight="heavy", crew=2, mobility="tracked",
                         range_m=0, ammo_type=None, fuel_type="diesel", fire={}, carry_t=6.0),
    "radio":        dict(name="便携电台", armor="soft", weight="light", crew=1, mobility="foot",
                         range_m=2500, ammo_type=None, fuel_type=None, fire={}),
    "hq_radio":     dict(name="大功率指挥电台", armor="soft", weight="medium", crew=2, mobility="wheeled",
                         range_m=15000, ammo_type=None, fuel_type=None, fire={}),
    "field_wire":   dict(name="野战被覆线卷", armor="soft", weight="light", crew=1, mobility="foot",
                         range_m=2000, wire_km=2.0, ammo_type=None, fuel_type=None, fire={}),
    "fac_radio":    dict(name="空地协同电台", armor="soft", weight="light", crew=2, mobility="foot",
                         range_m=0, ammo_type=None, fuel_type=None, fire={}),
    "demo_charge":  dict(name="工程爆破筒", armor="soft", weight="light", crew=1, mobility="foot",
                         range_m=50, ammo_type=None, fuel_type=None,
                         fire={"soft_he": 14.0}, buffs={"salvage": 0.8}),
    "mine_detector": dict(name="探雷器与工兵锹", armor="soft", weight="light", crew=1, mobility="foot",
                          range_m=0, ammo_type=None, fuel_type=None, fire={},
                          buffs={"mine_clearing": 1.0, "airfield_repair": 1.0}),
    "pontoon_kit":  dict(name="架桥舟桥组件", armor="soft", weight="medium", crew=2, mobility="foot",
                         range_m=0, ammo_type=None, fuel_type=None, fire={},
                         buffs={"bridge_building": 1.0}),
}


def register_equipment(model, spec):
    """动态注册/扩展自定义武器与装备。"""
    EQUIPMENT[str(model)] = dict(spec)
    return EQUIPMENT[str(model)]

RADIO_CHANNELS = ("A", "B", "C", "D", "E", "F", "G")
PERSONAL_RADIUS_KM = 0.4

VEHICLE_SPECS = {
    "truck":       dict(name="军用卡车",     mobility="wheeled", min_crew=1, full_crew=2, rated_load_t=3.5, is_combat=False, base_speed_kmh=30.0),
    "halftrack":   dict(name="半履带装甲车", mobility="tracked", min_crew=1, full_crew=3, rated_load_t=2.5, is_combat=True,  base_speed_kmh=24.0),
    "tank_medium": dict(name="中型坦克",     mobility="tracked", min_crew=1, full_crew=4, rated_load_t=1.2, is_combat=True,  base_speed_kmh=20.0),
    "tank_heavy":  dict(name="重型坦克",     mobility="tracked", min_crew=1, full_crew=5, rated_load_t=1.5, is_combat=True,  base_speed_kmh=14.0),
    "tractor":     dict(name="履带牵引车",   mobility="tracked", min_crew=1, full_crew=2, rated_load_t=6.0, is_combat=False, base_speed_kmh=16.0),
}


def register_vehicle_spec(model, spec):
    """动态注册/覆写载具规格（解耦硬编码型号与速度表）。"""
    m = str(model)
    merged = {
        "name": str(spec.get("name", m)),
        "mobility": str(spec.get("mobility", "wheeled")),
        "min_crew": int(spec.get("min_crew", 1)),
        "full_crew": int(spec.get("full_crew", 2)),
        "rated_load_t": float(spec.get("rated_load_t", 2.0)),
        "is_combat": bool(spec.get("is_combat", False)),
        "base_speed_kmh": float(spec.get("base_speed_kmh", 20.0)),
    }
    VEHICLE_SPECS[m] = merged
    if m not in EQUIPMENT:
        EQUIPMENT[m] = {
            "name": merged["name"],
            "mobility": merged["mobility"],
            "crew": merged["min_crew"],
            "carry_t": float(spec.get("carry_t", merged["rated_load_t"])),
            "fire": dict(spec.get("fire") or {}),
            "range_m": float(spec.get("range_m", 0.0)),
        }
    return VEHICLE_SPECS[m]

INDIRECT_WEAPONS = ("mortar_81", "howitzer_105", "howitzer_152")


def is_indirect_weapon(model):
    return model in INDIRECT_WEAPONS


def weapon_min_range_m(model):
    if model == "mortar_81":
        return 100.0
    if model in ("howitzer_105", "howitzer_152"):
        return 400.0
    return 0.0


def weapon_ballistic_group(model):
    """弹道族分类：用于按 AO 作用域严格隔离步兵枪械、直瞄火炮、曲射炮兵的校射诸元。"""
    if is_indirect_weapon(model):
        return "indirect_arty"
    if model in ("at_gun_50", "flak_88", "tank_medium", "tank_heavy", "halftrack"):
        return "direct_cannon"
    return "small_arms"


def weapon_frag_radius_m(model):
    """返回装备的高爆破片杀伤半径（米）。"""
    return float(EQUIPMENT.get(model, {}).get("frag_radius_m", 0.0))


def weapon_accuracy_decay(model, dist_m):
    """连续精度衰减：取消最大射程空气墙，按基准有效射程 R_eff 平滑衰减 Acc(d) = 1 / (1 + (d / R_eff)^2)。"""
    r_eff = max(50.0, float(EQUIPMENT.get(model, {}).get("range_m", 400.0)))
    d = max(0.0, float(dist_m))
    return 1.0 / (1.0 + (d / r_eff) ** 2)


def weapon_damage_decay(model, dtype, dist_m):
    """连续伤害衰减与 50m 内 CQB 生态反转：动能弹衰减，高爆/聚能装药不衰减伤害。"""
    r_eff = max(50.0, float(EQUIPMENT.get(model, {}).get("range_m", 400.0)))
    d = max(0.0, float(dist_m))
    cqb_mult = 1.0
    if d <= 50.0:
        if dtype == "small_arms" and model in ("rifle", "lmg"):
            cqb_mult = 2.0
        elif model in ("at_gun_50", "flak_88"):
            cqb_mult = 0.15
    if dtype in ("he_blast", "heat_shaped", "incendiary"):
        return 1.0 * cqb_mult
    return math.exp(-0.65 * max(0.0, d - 50.0) / r_eff) * cqb_mult


def compute_registration_delta(model, dist_m, duration_min=5.0):
    """三锚点分段幂律校射增益（以步枪 R_eff=400m 为基准等比缩放）：
    - 50m：一枪(5min)直接满精度 +0.60；
    - 300m：15min(3轮)满精度 +0.60；
    - 3000m：20h(240轮)仅 +0.01；
    同时返回受距离观测上限约束的 (delta_bonus, cap_bonus)。
    """
    r_eff = max(50.0, float(EQUIPMENT.get(model, {}).get("range_m", 400.0)))
    d_eq = max(1.0, float(dist_m) * (400.0 / r_eff))
    t_scale = max(0.0, float(duration_min) / 5.0)
    if d_eq <= 50.0:
        rate_5m = 0.60
    elif d_eq <= 300.0:
        alpha1 = math.log(3.0) / math.log(6.0)
        rate_5m = 0.60 * ((d_eq / 50.0) ** (-alpha1))
    else:
        alpha2 = math.log(4800.0) / math.log(10.0)
        rate_5m = 0.20 * ((d_eq / 300.0) ** (-alpha2))
    cap_bonus = 0.60 if d_eq <= 300.0 else min(0.60, 0.60 * ((300.0 / d_eq) ** 1.6))
    return rate_5m * t_scale, cap_bonus

# ==========================================
# 🧱 班组模板
# ==========================================
# buffs 约定：>=1 的数字是增益（合并取最大），<1 是减益（合并取最小）
SQUAD_TEMPLATES = {
    "rifle_squad": dict(enlisted=8, cadre=1, equipment={"rifle": 9, "lmg": 1}, buffs={}),
    "mg_squad": dict(enlisted=5, cadre=1, equipment={"hmg": 2, "rifle": 4},
                     buffs={"defense_firepower": 1.2, "mountain_mobility": 0.8}),
    "mortar_squad": dict(enlisted=5, cadre=1, equipment={"mortar_81": 2, "rifle": 4},
                         buffs={"mountain_mobility": 0.85}),
    "at_squad": dict(enlisted=6, cadre=1, equipment={"at_gun_50": 1, "rifle": 5},
                     buffs={"anti_armor": 1.3}),
    "recon_squad": dict(enlisted=5, cadre=1, equipment={"rifle": 4, "radio": 1, "lmg": 1},
                        buffs={"recon_radius": 1.6, "concealment_use": 1.3}),
    "engineer_squad": dict(enlisted=8, cadre=1,
                           equipment={"rifle": 8, "radio": 1, "demo_charge": 2, "mine_detector": 2},
                           buffs={"bridge_building": 1.0, "mine_clearing": 1.0,
                                  "salvage": 0.8, "airfield_repair": 1.0}),
    "transport_squad": dict(enlisted=6, cadre=1, equipment={"truck": 3, "rifle": 4}, buffs={}),
    "porter_squad": dict(enlisted=10, cadre=1, equipment={"rifle": 4}, buffs={}),
    "tank_squad": dict(enlisted=12, cadre=2, equipment={"tank_medium": 3, "radio": 1}, buffs={}),
    "heavy_tank_squad": dict(enlisted=10, cadre=2, equipment={"tank_heavy": 2, "radio": 1}, buffs={}),
    "gun_squad": dict(enlisted=10, cadre=2, equipment={"howitzer_105": 2, "truck": 1, "radio": 1},
                      buffs={"mountain_mobility": 0.7}),
    "fac_squad": dict(enlisted=4, cadre=1, equipment={"rifle": 4, "radio": 1, "fac_radio": 1},
                      buffs={"air_liaison": 1.0}),
    "heavy_gun_squad": dict(enlisted=14, cadre=2, equipment={"howitzer_152": 2, "truck": 2, "radio": 1},
                            buffs={"mountain_mobility": 0.6}),
    "aa_squad": dict(enlisted=14, cadre=2, equipment={"flak_88": 1, "truck": 1, "rifle": 6},
                     buffs={"anti_air": 1.0}),
    "mech_squad": dict(enlisted=8, cadre=1, equipment={"halftrack": 1, "rifle": 7, "lmg": 1},
                       buffs={"road_mobility": 1.2}),
    "hq_squad": dict(enlisted=6, cadre=4, equipment={"radio": 2, "hq_radio": 1, "field_wire": 2, "rifle": 8, "truck": 1},
                     buffs={"command": 1.0}),
    "signal_squad": dict(enlisted=6, cadre=1, equipment={"rifle": 4, "radio": 1, "hq_radio": 1, "field_wire": 3},
                         buffs={"signal": 1.0}),
}


def register_squad_template(kind, spec):
    """动态注册/覆写班组模板。"""
    SQUAD_TEMPLATES[str(kind)] = {
        "enlisted": int(spec.get("enlisted", 0)),
        "cadre": int(spec.get("cadre", 0)),
        "equipment": dict(spec.get("equipment") or {}),
        "buffs": dict(spec.get("buffs") or {}),
    }
    return SQUAD_TEMPLATES[str(kind)]

LEVELS = ("squad", "platoon", "company", "battalion", "regiment", "brigade",
          "division", "corps", "army", "group")

# 组织度：骨干保留率 → 天花板（≥70% 满值；≤30% 锁在低位）
ORG_CAP_FLOOR = 25.0
CADRE_FULL_RATIO = 0.70
CADRE_LOCK_RATIO = 0.30
ORG_RECOVER_PER_HOUR = 2.5
MIX_COHESION_PENALTY = 0.85   # 临时混编的磨合折扣（可配置）
DIRECT_MERGE_MAX_PERSONS = 15  # 班组直接合编人力硬上限；排以上整编必须走耗时事件


def evaluate_squad_counter_and_caps(
    enlisted,
    cadre,
    equipment,
    buffs=None,
    is_heavy_emplacement=False,
    deployed_weapon=None,
    vehicle_model=None,
):
    """按实有人力、骨干与装备确定性判定班组兵牌图标、显示层级加权与能力限制级。"""
    enl = max(0, int(enlisted))
    cad = max(0, int(cadre))
    persons = enl + cad
    eq = {str(m): max(0, int(n)) for m, n in (equipment or {}).items() if int(n) > 0}
    bmap = dict(buffs or {})

    crew_need = sum(
        int(EQUIPMENT.get(m, {}).get("crew", 1)) * n
        for m, n in eq.items()
        if m in EQUIPMENT and EQUIPMENT[m].get("fire")
    )
    crew_restricted = bool(crew_need > 0 and persons < crew_need)

    if persons <= 0:
        restriction_tier = "unmanned"
        org_cap_limit = 0.0
        heavy_op_cap = 0.0
        can_operate_radio = False
        badge_label = "空壳/遗弃"
    elif cad <= 0:
        restriction_tier = "restricted_no_cadre"
        org_cap_limit = ORG_CAP_FLOOR
        heavy_op_cap = 0.5
        can_operate_radio = False
        badge_label = "受限:无骨干散班"
    elif persons <= 4:
        restriction_tier = "restricted_fireteam"
        org_cap_limit = round(35.0 + 8.75 * persons, 1)
        heavy_op_cap = 0.85
        can_operate_radio = True
        badge_label = "受限:微型班组"
    else:
        restriction_tier = "standard_squad"
        org_cap_limit = 100.0
        heavy_op_cap = 1.0
        can_operate_radio = True
        badge_label = "标准建制班"

    # 按严格优先级决定主兵牌与 +1 显示层级加权
    display_tier_bonus = 0
    if vehicle_model in ("tank_medium", "tank_heavy") or eq.get("tank_medium", 0) > 0 or eq.get("tank_heavy", 0) > 0:
        primary_icon = "armor"
        display_tier_bonus = 1
    elif vehicle_model == "halftrack" or eq.get("halftrack", 0) > 0:
        primary_icon = "mechanized"
        display_tier_bonus = 1
    elif deployed_weapon in ("howitzer_105", "howitzer_152") or eq.get("howitzer_105", 0) > 0 or eq.get("howitzer_152", 0) > 0:
        primary_icon = "artillery"
        display_tier_bonus = 1
    elif deployed_weapon == "flak_88" or eq.get("flak_88", 0) > 0:
        primary_icon = "aa"
        display_tier_bonus = 1
    elif deployed_weapon == "at_gun_50" or eq.get("at_gun_50", 0) > 0:
        primary_icon = "at"
        display_tier_bonus = 1
    elif deployed_weapon == "mortar_81" or eq.get("mortar_81", 0) > 0:
        primary_icon = "mortar"
        display_tier_bonus = 1
    elif deployed_weapon == "hmg" or eq.get("hmg", 0) > 0:
        primary_icon = "mg"
        display_tier_bonus = 1
    elif is_heavy_emplacement:
        primary_icon = "emplacement"
        display_tier_bonus = 1
    elif eq.get("hq_radio", 0) > 0 and cad >= 2:
        primary_icon = "hq"
    elif eq.get("fac_radio", 0) > 0 and cad >= 1:
        primary_icon = "fac"
    elif eq.get("hq_radio", 0) > 0 or eq.get("field_wire", 0) > 0:
        primary_icon = "signal"
    elif any(eq.get(k, 0) > 0 for k in ("mine_detector", "demo_charge", "pontoon_kit")) or any(
        bmap.get(k) for k in ("mine_clearing", "bridge_building", "airfield_repair", "salvage")
    ):
        primary_icon = "engineer"
    elif vehicle_model or any(eq.get(k, 0) > 0 for k in ("truck", "tractor")):
        primary_icon = "logistics" if (eq.get("truck", 0) > 0 or eq.get("tractor", 0) > 0 or vehicle_model in ("truck", "tractor")) else "vehicle"
    elif bmap.get("recon_radius") or (eq.get("radio", 0) > 0 and persons <= 6):
        primary_icon = "recon"
    else:
        primary_icon = "infantry"

    return {
        "primary_icon": primary_icon,
        "display_tier_bonus": display_tier_bonus,
        "restriction_tier": restriction_tier,
        "org_cap_limit": org_cap_limit,
        "heavy_op_cap": heavy_op_cap,
        "can_operate_radio": can_operate_radio,
        "crew_need": crew_need,
        "crew_restricted": crew_restricted,
        "badge_label": badge_label,
    }


def _smooth(x):
    x = min(1.0, max(0.0, x))
    return x * x * (3 - 2 * x)


# ==========================================
# 🪖 班
# ==========================================
class Squad:
    def __init__(self, sid, kind, enlisted, cadre, equipment, enlisted_full=None, cadre_full=None,
                 equipment_full=None, buffs=None, org=100.0):
        self.id = sid
        self.kind = kind
        self.enlisted = int(enlisted)
        self.cadre = int(cadre)
        self.equipment = {k: int(v) for k, v in equipment.items() if int(v) > 0}
        self.enlisted_full = int(enlisted if enlisted_full is None else enlisted_full)
        self.cadre_full = int(cadre if cadre_full is None else cadre_full)
        self.equipment_full = dict(equipment if equipment_full is None else equipment_full)
        self.buffs = dict(buffs or {})
        self.org = float(org)
        self.carry = {}            # 取整累计小数

    def required_ammo_types(self):
        """返回本班所有现役具体装备型号对应的弹药口径集合。"""
        out = set()
        for mdl, n in self.equipment.items():
            if n > 0:
                ammo_t = EQUIPMENT.get(mdl, {}).get("ammo_type")
                if ammo_t:
                    out.add(ammo_t)
        return out

    def required_fuel_types(self):
        """返回本班所有现役装备所需燃料类型集合。"""
        out = set()
        for mdl, n in self.equipment.items():
            if n > 0:
                ft = EQUIPMENT.get(mdl, {}).get("fuel_type")
                if ft:
                    out.add(ft)
        return out

    # ---- 账本 ----
    @property
    def persons(self):
        return self.enlisted + self.cadre

    @property
    def cadre_ratio(self):
        if self.cadre <= 0:
            return 0.0
        return self.cadre / self.cadre_full if self.cadre_full > 0 else 1.0

    @property
    def equipment_ratio(self):
        full = sum(self.equipment_full.values())
        return sum(self.equipment.values()) / full if full > 0 else 1.0

    @property
    def enlisted_ratio(self):
        return self.enlisted / self.enlisted_full if self.enlisted_full > 0 else 1.0

    def org_cap(self):
        if self.persons <= 0:
            return 0.0
        if self.cadre <= 0:
            return ORG_CAP_FLOOR
        base_cap = ORG_CAP_FLOOR + (100.0 - ORG_CAP_FLOOR) * _smooth(
            (self.cadre_ratio - CADRE_LOCK_RATIO) / (CADRE_FULL_RATIO - CADRE_LOCK_RATIO))
        eval_cap = evaluate_squad_counter_and_caps(
            self.enlisted, self.cadre, self.equipment, self.buffs
        )["org_cap_limit"]
        return min(base_cap, eval_cap)

    def clamp_org(self):
        self.org = max(0.0, min(self.org, self.org_cap()))

    # ---- 战斗力 ----
    def crew_factor(self):
        need = sum(EQUIPMENT[m]["crew"] * n for m, n in self.equipment.items() if m in EQUIPMENT
                   and EQUIPMENT[m]["fire"])
        return 1.0 if need <= 0 else min(1.0, self.persons / need)

    def power(self, dist_m=None, has_los=True, only_weapon=None, acc_mult=1.0,
              operator_squad=None, operator_persons=None):
        """各伤害类型的每小时有效火力点（含乘员、外挂操炮班组与多人加成、骨干、组织度、buff、连续精度/动能双衰减与通视过滤）。"""
        has_ext_op = (operator_squad is not None) or (operator_persons is not None)
        ext_pers = int(operator_persons) if operator_persons is not None else (
            int(operator_squad.persons) if operator_squad is not None else 0
        )
        eff_persons = self.persons + ext_pers if has_ext_op else self.persons
        need = sum(EQUIPMENT[m]["crew"] * n for m, n in self.equipment.items() if m in EQUIPMENT
                   and EQUIPMENT[m]["fire"])
        crew = (1.0 if need <= 0 else min(1.0, eff_persons / need)) if has_ext_op else self.crew_factor()
        if self.persons <= 0 and operator_squad is not None:
            cadre_f = 0.55 + 0.45 * min(1.0, operator_squad.cadre_ratio / CADRE_FULL_RATIO)
            org_f = 0.3 + 0.7 * operator_squad.org / 100.0
        else:
            cadre_f = 0.55 + 0.45 * min(1.0, self.cadre_ratio / CADRE_FULL_RATIO)
            org_f = 0.3 + 0.7 * self.org / 100.0
        gen_buff = float(self.buffs.get("defense_firepower", 1.0)) * float(self.buffs.get("firepower", 1.0))
        aa_buff = float(self.buffs.get("anti_armor", 1.0))
        out = {}
        for model, n in self.equipment.items():
            if int(n) <= 0:
                continue
            if only_weapon is not None and model != only_weapon:
                continue
            eq = EQUIPMENT.get(model, {})
            if not eq.get("fire"):
                continue
            acc_f = max(0.1, float(acc_mult))
            if dist_m is not None:
                max_r = float(eq.get("range_m", 0.0))
                min_r = weapon_min_range_m(model)
                if max_r <= 0.0 or dist_m < min_r - 1e-6:
                    continue
                if not is_indirect_weapon(model) and not has_los:
                    continue
                acc_f = min(1.6, weapon_accuracy_decay(model, dist_m) * max(0.1, float(acc_mult)))
            op_mult = 1.0
            if has_ext_op:
                if eff_persons <= 0:
                    op_mult = 0.0
                else:
                    eff_skill = heavy_operator_efficiency(operator_squad, model) if operator_squad is not None else 1.0
                    # 多人操炮加成 Buff：1人吃装填/压制折扣，2人基线，3人协同加速，4人+满编压制
                    if eff_persons == 1:
                        crew_buff = 0.70
                    elif eff_persons == 2:
                        crew_buff = 1.00
                    elif eff_persons == 3:
                        crew_buff = 1.18
                    else:
                        crew_buff = 1.32
                    op_mult = eff_skill * crew_buff
            for dtype, pts in eq["fire"].items():
                mult = gen_buff * (aa_buff if dtype in ("ap_kinetic", "heat_shaped") else 1.0) * op_mult
                dmg_f = weapon_damage_decay(model, dtype, dist_m) if dist_m is not None else 1.0
                out[dtype] = out.get(dtype, 0.0) + pts * n * mult * acc_f * dmg_f
        scale = crew * cadre_f * org_f
        return {k: v * scale for k, v in out.items()}

    def carry_capacity_t(self, porter_load_t=0.025, truck_ok=True):
        """搬运能力（吨/趟）：每个士兵肩挑 + 各型号载具按各自额定载重累计。返回 (人力吨, 载具吨, 士兵数)。"""
        truck_t = 0.0
        if truck_ok:
            for m, n in self.equipment.items():
                if int(n) > 0 and m in VEHICLE_SPECS:
                    cap_each = float(EQUIPMENT.get(m, {}).get("carry_t", VEHICLE_SPECS[m].get("rated_load_t", 0.0)))
                    truck_t += int(n) * cap_each
        return self.enlisted * porter_load_t, truck_t, self.enlisted

    def derive_capabilities(self):
        """由当前携带装备、骨干数量自动派生职能与专业能力（解耦硬编码 kind 兵种名）。"""
        caps = {}
        eq = self.equipment
        for m, n in eq.items():
            if int(n) <= 0:
                continue
            spec = EQUIPMENT.get(m, {})
            for bk, bv in (spec.get("buffs") or {}).items():
                val = float(bv)
                if bk not in caps or val > caps[bk]:
                    caps[bk] = val
        if eq.get("hq_radio", 0) > 0:
            caps.setdefault("signal", 1.0)
            if self.cadre >= 3:
                caps.setdefault("command", 1.0)
        if eq.get("field_wire", 0) > 0:
            caps.setdefault("signal", 1.0)
        if eq.get("fac_radio", 0) > 0:
            caps.setdefault("air_liaison", 1.0)
        if eq.get("flak_88", 0) > 0:
            caps.setdefault("anti_air", 1.0)
        if eq.get("at_gun_50", 0) > 0:
            caps.setdefault("anti_armor", 1.0)
        return caps

    def has_buff(self, key):
        return (key in self.buffs) or (key in self.derive_capabilities())

    def get_buff(self, key, default=None):
        if key in self.buffs:
            return self.buffs[key]
        caps = self.derive_capabilities()
        if key in caps:
            return caps[key]
        return default

    def arm_tags(self):
        """由当前实有装备、技能与人员自动派生兵种/兵牌标签集合。"""
        tags = set()
        eq = self.equipment
        if any(eq.get(m, 0) > 0 for m in ("tank_medium", "tank_heavy", "halftrack")) or self.kind in (
            "tank_squad", "heavy_tank_squad", "mech_squad"
        ):
            tags.add("armor")
        if any(eq.get(m, 0) > 0 for m in ("mortar_81", "howitzer_105", "howitzer_152")):
            tags.add("artillery")
        if eq.get("flak_88", 0) > 0 or self.has_buff("anti_air"):
            tags.add("aa")
        if eq.get("at_gun_50", 0) > 0 or self.has_buff("anti_armor"):
            tags.add("at")
        if any(self.has_buff(k) for k in ("bridge_building", "mine_clearing", "airfield_repair", "salvage")):
            tags.add("engineer")
        if self.has_buff("recon_radius") or self.kind == "recon_squad":
            tags.add("recon")
        if self.has_buff("command") or self.kind == "hq_squad":
            tags.add("hq")
        if self.has_buff("signal") or self.kind == "signal_squad":
            tags.add("signal")
        if self.has_buff("air_liaison") or self.kind == "fac_squad":
            tags.add("fac")
        if any(eq.get(m, 0) > 0 for m in ("truck", "tractor")) or self.kind in ("transport_squad", "porter_squad"):
            tags.add("logistics")
        if self.enlisted > 0 and (
            any(eq.get(m, 0) > 0 for m in ("rifle", "lmg", "hmg", "mortar_81")) or not (tags & {"armor", "artillery", "logistics"})
        ):
            tags.add("infantry")
        return tags

    # ---- 复原 ----
    def recover(self, hours, replacements=0):
        """补兵 + 组织度恢复。返回实际吸收的新兵数。
        有装无人 → 只要补兵员就能原地成建制；组织度上限/速率由骨干保留率锁死。"""
        need = max(0, self.enlisted_full - self.enlisted)
        take = min(need, int(replacements))
        self.enlisted += take
        cap = self.org_cap()
        rate = ORG_RECOVER_PER_HOUR * (0.25 + 0.75 * min(1.0, self.cadre_ratio / CADRE_FULL_RATIO))
        # 缺编越严重，恢复越慢（新兵占比高）
        rate *= (0.4 + 0.6 * self.enlisted_ratio)
        if self.org < cap:
            self.org = min(cap, self.org + rate * hours)
        return take

    def boost_org_overcap(self, delta, max_org=120.0):
        """作战会议现场督战：强行直接增加组织度，允许突破骨干上限 org_cap()，但受饱和天花板限制。"""
        if self.persons <= 0:
            return
        self.org = min(float(max_org), self.org + max(0.0, float(delta)))

    def decay_overcap_org(self, dt_hours, rate_per_h=1.0):
        """战前动员超限组织度随物理时间缓缓回落到骨干上限 org_cap()。"""
        cap = self.org_cap()
        if self.org > cap and dt_hours > 0:
            self.org = max(cap, self.org - float(rate_per_h) * float(dt_hours))

    def train_cadre(self, days):
        """整训：从老兵里缓慢提拔骨干（每天约 3% 的现有士兵可晋升，受缺口限制）。"""
        deficit = self.cadre_full - self.cadre
        if deficit <= 0 or self.enlisted <= 0:
            return 0
        self.carry["promote"] = self.carry.get("promote", 0.0) + 0.03 * self.enlisted * days
        n = min(deficit, int(self.carry["promote"]), self.enlisted)
        self.carry["promote"] -= n
        self.enlisted -= n
        self.cadre += n
        return n

    # ---- 序列化 ----
    def to_dict(self):
        return dict(id=self.id, kind=self.kind, enlisted=self.enlisted, cadre=self.cadre,
                    equipment=dict(self.equipment), enlisted_full=self.enlisted_full,
                    cadre_full=self.cadre_full, equipment_full=dict(self.equipment_full),
                    buffs=dict(self.buffs), org=self.org, carry=dict(self.carry))

    @classmethod
    def from_dict(cls, d):
        s = cls(d["id"], d["kind"], d["enlisted"], d["cadre"], d["equipment"], d["enlisted_full"],
                d["cadre_full"], d["equipment_full"], d.get("buffs"), d.get("org", 100.0))
        s.carry = dict(d.get("carry", {}))
        return s


def make_squad(kind, sid, strength=1.0, org=100.0, equipment=None, buffs=None, enlisted=None, cadre=None):
    """按模板生成班（支持自由覆写人数、装备与 Buff）。strength<1 表示残编（人/骨干/装备同比例缩减）。"""
    t = SQUAD_TEMPLATES[kind]
    enl_full = int(t["enlisted"] if enlisted is None else enlisted)
    cad_full = int(t["cadre"] if cadre is None else cadre)
    eq_full = dict(t["equipment"] if equipment is None else equipment)
    b = dict(t["buffs"])
    if buffs:
        b.update(buffs)
    sq = Squad(sid, kind, round(enl_full * strength), max(0, round(cad_full * strength)),
               {m: round(n * strength) for m, n in eq_full.items()},
               enl_full, cad_full, eq_full, b, org)
    sq.clamp_org()
    return sq


def make_custom_squad(sid, enlisted=8, cadre=1, equipment=None, buffs=None,
                      kind="custom_squad", org=100.0,
                      enlisted_full=None, cadre_full=None, equipment_full=None):
    """完全自由混编班组构造器：不依赖固定模板，任意组合人员与武器装备。"""
    enl = max(0, int(enlisted))
    cad = max(0, int(cadre))
    eq = {str(m): max(0, int(n)) for m, n in (equipment or {}).items() if int(n) > 0}
    ef = enl if enlisted_full is None else max(enl, int(enlisted_full))
    cf = cad if cadre_full is None else max(cad, int(cadre_full))
    eqf = dict(eq) if equipment_full is None else {str(m): int(n) for m, n in equipment_full.items()}
    sq = Squad(str(sid), str(kind), enl, cad, eq, ef, cf, eqf, dict(buffs or {}), org)
    sq.clamp_org()
    return sq


# ==========================================
# 🌳 编制树
# ==========================================
class Node:
    def __init__(self, nid, name, level, children=None, squad=None):
        self.id = nid
        self.name = name
        self.level = level
        self.children = list(children or [])
        self.squad = squad

    def squads(self):
        if self.squad is not None:
            yield self.squad
        for ch in self.children:
            yield from ch.squads()

    def find_node(self, nid):
        if self.id == nid:
            return self
        for ch in self.children:
            hit = ch.find_node(nid)
            if hit:
                return hit
        return None

    def remove_squad(self, sid):
        """从树中摘掉某个班，返回被摘掉的 Squad（找不到返回 None）。"""
        for i, ch in enumerate(self.children):
            if ch.squad is not None and ch.squad.id == sid:
                self.children.pop(i)
                return ch.squad
            hit = ch.remove_squad(sid)
            if hit is not None:
                return hit
        return None

    def totals(self):
        sq = list(self.squads())
        return {
            "squads": len(sq),
            "enlisted": sum(s.enlisted for s in sq),
            "cadre": sum(s.cadre for s in sq),
            "enlisted_full": sum(s.enlisted_full for s in sq),
            "cadre_full": sum(s.cadre_full for s in sq),
            "equipment": _sum_dicts(s.equipment for s in sq),
            "equipment_full": _sum_dicts(s.equipment_full for s in sq),
        }

    def to_dict(self):
        return dict(id=self.id, name=self.name, level=self.level,
                    squad=self.squad.to_dict() if self.squad else None,
                    children=[c.to_dict() for c in self.children])

    @classmethod
    def from_dict(cls, d):
        return cls(d["id"], d["name"], d["level"],
                   [cls.from_dict(c) for c in d.get("children", [])],
                   Squad.from_dict(d["squad"]) if d.get("squad") else None)


def _sum_dicts(dicts):
    out = {}
    for d in dicts:
        for k, v in d.items():
            out[k] = out.get(k, 0) + v
    return out


# ==========================================
# 🧭 地图棋子
# ==========================================
DEFAULT_STOCK_CAP_PER_PERSON_T = {"food": 0.006, "ammo": 0.012, "fuel": 0.01}


VALID_ROE = ("hold_fire", "return_fire", "ambush", "fire_at_will")


class MapPiece:
    """地图上的一个作战实体。"""

    def __init__(self, pid, name, team, px, root, level="division",
                 radio_silence=False, decoy=False, fortification=0.0,
                 footprint_px=16, control_radius_km=3.0, fake_strength=None,
                 radio_channel="A", is_heavy_emplacement=False, deployed_weapon=None,
                 pos=None, attached_to=None, unit_state=None, operated_by=None,
                 vehicle_model=None, cargo=None, status_debuffs=None, facing_deg=0.0,
                 roe="return_fire", meta=None):
        self.id = pid
        self.name = name
        self.team = team
        self.pos = (float(pos[0]), float(pos[1])) if pos is not None else (float(px[0]), float(px[1]))
        self.facing_deg = float(facing_deg) % 360.0
        self.roe = roe if roe in VALID_ROE else "return_fire"
        self.meta = dict(meta or {})
        if isinstance(root, Squad):
            self.root = Node(pid, name, "squad", squad=root)
        elif isinstance(root, (list, tuple)):
            children = [
                c if isinstance(c, Node) else Node(c.id, c.id, "squad", squad=c)
                for c in root
            ]
            self.root = Node(pid, name, level, children=children)
        elif root is None:
            self.root = Node(pid, name, level, children=[])
        else:
            self.root = root
        self.level = level
        self.radio_silence = bool(radio_silence)
        self.radio_channel = radio_channel if radio_channel in RADIO_CHANNELS else "A"
        self._is_heavy_emplacement = bool(is_heavy_emplacement)
        self.deployed_weapon = deployed_weapon
        self.attached_to = attached_to
        self.unit_state = unit_state or (
            "deployed" if is_heavy_emplacement else ("carried" if attached_to else "mobile")
        )
        self.operated_by = operated_by
        self.vehicle_model = vehicle_model
        raw_cargo = cargo or {}
        self.cargo = {
            "resources": {k: float(v) for k, v in (raw_cargo.get("resources") or {}).items()},
            "equipment": {k: int(v) for k, v in (raw_cargo.get("equipment") or {}).items()},
        }
        self.status_debuffs = {str(k): dict(v) for k, v in (status_debuffs or {}).items()}
        self.decoy = bool(decoy)
        self.fortification = float(fortification)
        self.footprint_px = int(footprint_px)
        self.control_radius_km = float(control_radius_km)
        self.fake_strength = fake_strength
        self.stock = {"food": 0.0, "ammo": 0.0, "fuel": 0.0}
        self.ammo_by_type = {}     # 细分口径弹药明细 {ammo_type: tons}
        self.stock_cap = {}
        self.moved_last_turn = False
        self.shock_hours = 0.0
        self.set_full_stock()

    @property
    def px(self):
        return (int(round(self.pos[0])), int(round(self.pos[1])))

    @px.setter
    def px(self, val):
        self.pos = (float(val[0]), float(val[1]))

    @property
    def is_heavy_emplacement(self):
        return self._is_heavy_emplacement or (self.deployed_weapon is not None and self.unit_state == "deployed")

    @is_heavy_emplacement.setter
    def is_heavy_emplacement(self, val):
        self._is_heavy_emplacement = bool(val)
        if val:
            self.unit_state = "deployed"

    def attach_to(self, parent_id):
        self.attached_to = parent_id
        self.unit_state = "carried"
        self.operated_by = None

    def detach(self, state="mobile"):
        self.attached_to = None
        self.unit_state = state

    def set_facing(self, deg):
        self.facing_deg = float(deg) % 360.0

    # ---- 通用状态/战损/故障 Debuff 原语 ----
    def add_debuff(self, key, spec):
        self.status_debuffs[str(key)] = dict(spec or {})

    def remove_debuff(self, key):
        return self.status_debuffs.pop(str(key), None) is not None

    def tick_debuffs(self, dt_hours):
        dt = max(0.0, float(dt_hours))
        if dt <= 0.0:
            return
        for k in list(self.status_debuffs.keys()):
            ttl = self.status_debuffs[k].get("ttl_hours")
            if ttl is not None:
                ttl -= dt
                if ttl <= 1e-9:
                    del self.status_debuffs[k]
                else:
                    self.status_debuffs[k]["ttl_hours"] = ttl

    def aggregate_debuffs(self):
        out = {
            "forbid_move": False, "forbid_fire": False, "forbid_tow": False,
            "forbid_radio": False, "forbid_rest": False, "forbid_deploy": False,
            "speed_mult": 1.0, "fuel_mult": 1.0, "combat_power_mult": 1.0,
            "recon_mult": 1.0, "exposure_delta": 0.0,
        }
        for sp in self.status_debuffs.values():
            for lock in ("forbid_move", "forbid_fire", "forbid_tow", "forbid_radio", "forbid_rest", "forbid_deploy"):
                if sp.get(lock):
                    out[lock] = True
            for mk in ("speed_mult", "fuel_mult", "combat_power_mult", "recon_mult"):
                if mk in sp:
                    out[mk] *= float(sp[mk])
            if "exposure_delta" in sp:
                out["exposure_delta"] += float(sp["exposure_delta"])
        return out

    # ---- 载具容器、软上限超载与单人/残编车组评估 ----
    def cargo_load_tons(self, pieces=None):
        """汇总车载容器物资 + 搭载步兵 + 牵引重武器总吨位。"""
        res_t = sum(max(0.0, float(v)) for v in (self.cargo.get("resources") or {}).values())
        eq_w = {"light": 0.08, "medium": 0.5, "heavy": 3.5}
        eq_t = sum(
            max(0, int(n)) * eq_w.get(EQUIPMENT.get(m, {}).get("weight", "medium"), 0.5)
            for m, n in (self.cargo.get("equipment") or {}).items()
        )
        child_t = 0.0
        if pieces is not None:
            for ch in pieces.values():
                if ch.attached_to == self.id:
                    if ch.deployed_weapon:
                        w = EQUIPMENT.get(ch.deployed_weapon, {}).get("weight", "medium")
                        child_t += eq_w.get(w, 1.0)
                    elif (
                        self.vehicle_model in ("tank_medium", "tank_heavy")
                        and not ch.vehicle_model
                        and ch.mobility() == "foot"
                    ):
                        # Tank Desant（坦克跨乘步兵）：豁免超载按 0.02t/人计
                        child_t += ch.persons() * 0.02
                    else:
                        child_t += ch.persons() * 0.1
        return round(res_t + eq_t + child_t, 4)

    def overload_debuff(self, pieces=None):
        """软上限超载计算：超重不硬拦截，而是动态降速/高油耗，达到 >=2.0x 时物理压趴禁移动。"""
        if not self.vehicle_model or self.vehicle_model not in VEHICLE_SPECS:
            return {"load_ratio": 0.0, "forbid_move": False, "speed_mult": 1.0, "fuel_mult": 1.0}
        rated = max(0.1, float(VEHICLE_SPECS[self.vehicle_model]["rated_load_t"]))
        ratio = self.cargo_load_tons(pieces) / rated
        if ratio <= 1.0:
            return {"load_ratio": round(ratio, 3), "forbid_move": False, "speed_mult": 1.0, "fuel_mult": 1.0}
        if ratio >= 2.0:
            return {"load_ratio": round(ratio, 3), "forbid_move": True, "speed_mult": 0.0, "fuel_mult": 2.5}
        excess = ratio - 1.0  # (0, 1)
        speed_mult = max(0.05, 1.0 - 0.85 * excess)
        fuel_mult = 1.0 + 1.5 * excess
        return {"load_ratio": round(ratio, 3), "forbid_move": False,
                "speed_mult": round(speed_mult, 4), "fuel_mult": round(fuel_mult, 4)}

    def vehicle_crew_modifiers(self, pieces=None):
        """独立载具车组人数评估：min_crew=1 即可发动开走；缺编时按岗位冲突折算火力与视野。"""
        if not self.vehicle_model or self.vehicle_model not in VEHICLE_SPECS:
            return {"forbid_move": False, "forbid_fire": False, "speed_mult": 1.0,
                    "combat_power_mult": 1.0, "recon_mult": 1.0, "single_crew": False, "crew": self.persons()}
        spec = VEHICLE_SPECS[self.vehicle_model]
        crew = self.persons()
        eff = 1.0
        if self.operated_by and pieces and self.operated_by in pieces:
            op_p = pieces[self.operated_by]
            crew += op_p.persons()
            if op_p.squads():
                has_arm = any("armor" in s.arm_tags() for s in op_p.squads())
                eff = 1.0 if has_arm else 0.7
        if crew < int(spec["min_crew"]):
            return {"forbid_move": True, "forbid_fire": True, "speed_mult": 0.0,
                    "combat_power_mult": 0.0, "recon_mult": 0.2, "single_crew": False, "crew": crew}
        full = max(1, int(spec["full_crew"]))
        if crew == 1 and full > 1:
            return {"forbid_move": False, "forbid_fire": False, "speed_mult": 0.75 * eff,
                    "combat_power_mult": 0.3 * eff, "recon_mult": 0.5, "single_crew": True, "crew": crew}
        ratio = min(1.0, crew / float(full))
        return {"forbid_move": False, "forbid_fire": False, "speed_mult": (0.65 + 0.35 * ratio) * eff,
                "combat_power_mult": ratio * eff, "recon_mult": 0.6 + 0.4 * ratio,
                "single_crew": False, "crew": crew}

    @property
    def shock_ttl(self):
        return int(math.ceil(self.shock_hours / 6.0)) if self.shock_hours > 1e-9 else 0

    @shock_ttl.setter
    def shock_ttl(self, val):
        self.shock_hours = max(0.0, float(val) * 6.0)

    def required_ammo_types(self):
        out = set()
        for s in self.squads():
            out.update(s.required_ammo_types())
        return out

    def required_fuel_types(self):
        out = set()
        for s in self.squads():
            out.update(s.required_fuel_types())
        return out

    def squads(self):
        return list(self.root.squads())

    def get_squad(self, sid):
        for s in self.squads():
            if s.id == sid:
                return s
        return None

    def add_squad(self, squad):
        """将任意班组混编挂入本棋子，并刷新补给上限。"""
        if self.root.squad is not None:
            old = self.root.squad
            self.root.squad = None
            self.root.children.append(Node(old.id, old.id, "squad", squad=old))
        self.root.children.append(Node(squad.id, squad.id, "squad", squad=squad))
        if self.level == "squad" and len(self.squads()) > 1:
            self.level = "platoon"
        self.refresh_stock_cap()
        return squad

    def remove_squad(self, sid):
        """从本棋子移除并返回指定 ID 的班组，并刷新补给上限。"""
        if self.root.squad is not None and self.root.squad.id == sid:
            sq = self.root.squad
            self.root.squad = None
            self.refresh_stock_cap()
            return sq
        sq = self.root.remove_squad(sid)
        if sq is not None:
            self.refresh_stock_cap()
        return sq

    def split_squad_to_piece(self, sid, take_enlisted, take_cadre=0, take_equipment=None,
                             new_piece_id=None, new_squad_id=None, new_name=None,
                             offset_px=(0.15, 0.15)):
        """战地拆班并直接生成独立可机动的班组棋子，按兵员比例划转补给（三账本严格守恒）。"""
        sq = self.get_squad(sid) if sid is not None else (self.squads()[0] if len(self.squads()) == 1 else None)
        if sq is None:
            raise KeyError(f"棋子 {self.id} 中找不到班组 {sid}")
        old_persons = max(1, self.persons())
        det = split_squad(
            sq, take_enlisted, take_cadre=take_cadre,
            take_equipment=take_equipment, new_id=new_squad_id,
        )
        dpid = new_piece_id or f"{self.id}-{det.id}"
        dpos = (float(self.pos[0]) + float(offset_px[0]), float(self.pos[1]) + float(offset_px[1]))
        parent_profile = self.counter_profile()
        det_meta = {
            "inherited_parent_piece": self.id,
            "inherited_parent_squad": sq.id,
            "inherited_parent_icon": self.meta.get("inherited_parent_icon") or parent_profile.get("primary_icon"),
        }
        if self.meta.get("skin"):
            det_meta["skin"] = dict(self.meta.get("skin") or {})
        det_piece = MapPiece(
            dpid, new_name or f"{self.name}·{det.id}", self.team, dpos, det,
            level="squad", footprint_px=1, control_radius_km=0.8,
            radio_channel=self.radio_channel, roe=self.roe, pos=dpos,
            meta=det_meta,
        )
        frac = min(1.0, det.persons / float(old_persons))
        for rk in list(self.stock.keys()):
            trans = float(self.stock.get(rk, 0.0)) * frac
            self.stock[rk] = max(0.0, float(self.stock.get(rk, 0.0)) - trans)
            det_piece.stock[rk] = trans
        for ak in list(self.ammo_by_type.keys()):
            atrans = float(self.ammo_by_type.get(ak, 0.0)) * frac
            self.ammo_by_type[ak] = max(0.0, float(self.ammo_by_type.get(ak, 0.0)) - atrans)
            if atrans > 1e-9:
                det_piece.ammo_by_type[ak] = atrans
        if sq.persons <= 0 and sum(sq.equipment.values()) <= 0:
            self.remove_squad(sq.id)
        self.refresh_stock_cap()
        det_piece.refresh_stock_cap()
        return det_piece

    def set_counter_skin(self, color=None, badge_style=None, custom_label=None, attempt_level=None):
        """自定义兵牌颜色与外观皮肤；严禁篡改编制层级。"""
        if attempt_level is not None and str(attempt_level) != str(self.level):
            raise ValueError(f"严禁篡改编制层级: {self.level} -> {attempt_level}")
        skin = dict(self.meta.get("skin") or {})
        if color is not None:
            skin["color"] = str(color)
        if badge_style is not None:
            skin["badge_style"] = str(badge_style)
        if custom_label is not None:
            skin["custom_label"] = str(custom_label)
        self.meta["skin"] = skin
        return dict(skin)

    def counter_profile(self):
        """动态推演兵牌图标、能力限制级与编成画像（完全解耦固定兵种名称，供前端渲染开放混编单位）。"""
        sqs = self.squads()
        all_tags = set()
        eq_sum = {}
        enlisted_tot = 0
        cadre_tot = 0
        buff_list = []
        has_armored_eq = bool(self.vehicle_model and VEHICLE_SPECS.get(self.vehicle_model, {}).get("is_combat"))
        squad_rows = []
        for s in sqs:
            tags = s.arm_tags()
            all_tags.update(tags)
            enlisted_tot += s.enlisted
            cadre_tot += s.cadre
            if s.buffs:
                buff_list.append(s.buffs)
            for m, n in s.equipment.items():
                if int(n) > 0:
                    eq_sum[m] = eq_sum.get(m, 0) + int(n)
                    if EQUIPMENT.get(m, {}).get("armor") == "armored":
                        has_armored_eq = True
            sq_eval = evaluate_squad_counter_and_caps(s.enlisted, s.cadre, s.equipment, s.buffs)
            squad_rows.append({
                "id": s.id,
                "kind": s.kind,
                "enlisted": s.enlisted,
                "cadre": s.cadre,
                "persons": s.persons,
                "org": round(s.org, 1),
                "org_cap": round(s.org_cap(), 1),
                "equipment": dict(s.equipment),
                "arm_tags": sorted(tags),
                "capabilities": sorted(s.derive_capabilities().keys()),
                "restriction_tier": sq_eval["restriction_tier"],
                "primary_icon": sq_eval["primary_icon"],
                "badge_label": sq_eval["badge_label"],
            })
        merged_buffs = _merge_buffs(buff_list)
        eval_res = evaluate_squad_counter_and_caps(
            enlisted_tot,
            cadre_tot,
            eq_sum,
            buffs=merged_buffs,
            is_heavy_emplacement=self.is_heavy_emplacement,
            deployed_weapon=self.deployed_weapon,
            vehicle_model=self.vehicle_model,
        )
        return {
            "piece_id": self.id,
            "name": self.name,
            "team": self.team,
            "level": self.level,
            "unit_state": self.unit_state,
            "mobility": self.mobility(),
            "armor_class": "armored" if has_armored_eq else "soft",
            "primary_icon": eval_res["primary_icon"],
            "display_tier_bonus": eval_res["display_tier_bonus"],
            "restriction_tier": eval_res["restriction_tier"],
            "org_cap_limit": eval_res["org_cap_limit"],
            "heavy_op_cap": eval_res["heavy_op_cap"],
            "can_operate_radio": eval_res["can_operate_radio"],
            "crew_need": eval_res["crew_need"],
            "crew_restricted": eval_res["crew_restricted"],
            "badge_label": eval_res["badge_label"],
            "skin": dict(self.meta.get("skin") or {}),
            "arm_tags": sorted(all_tags),
            "squad_count": len(sqs),
            "persons": enlisted_tot + cadre_tot,
            "enlisted": enlisted_tot,
            "cadre": cadre_tot,
            "mean_org": round(self.mean_org(), 1),
            "equipment": eq_sum,
            "squads": squad_rows,
        }

    def persons(self):
        return sum(s.persons for s in self.squads())

    def totals(self):
        return self.root.totals()

    def refresh_stock_cap(self):
        n = max(1, self.persons())
        self.stock_cap = {k: v * n for k, v in DEFAULT_STOCK_CAP_PER_PERSON_T.items()}

    def set_full_stock(self, fraction=1.0):
        self.refresh_stock_cap()
        self.stock = {k: v * fraction for k, v in self.stock_cap.items()}

    def equipment_counts(self):
        """汇总当前棋子全部班组的实有装备计数。"""
        out = {}
        for sq in self.squads():
            for m, n in (sq.equipment or {}).items():
                if int(n) > 0:
                    out[m] = out.get(m, 0) + int(n)
        return out

    def mobility(self):
        """棋子机动类别 = 兵员占比 ≥20% 的班组里最慢的那一类；无人载具/装备回退按载具规格与实有装备判定。"""
        weight = {"foot": 0.0, "wheeled": 0.0, "tracked": 0.0}
        for s in self.squads():
            cls = _squad_mobility(s)
            weight[cls] += s.persons
        total = sum(weight.values())
        if total > 0:
            for cls in ("foot", "wheeled", "tracked"):    # 最慢优先
                if weight[cls] / total >= 0.2:
                    return cls
            return "foot"
        # 无人载具 / 无人重武器单位不能退化成步兵
        if self.vehicle_model and self.vehicle_model in VEHICLE_SPECS:
            return VEHICLE_SPECS[self.vehicle_model]["mobility"]
        eq = self.equipment_counts()
        veh_mobs = [VEHICLE_SPECS[m]["mobility"] for m, n in eq.items() if int(n) > 0 and m in VEHICLE_SPECS]
        if veh_mobs:
            for cls in ("wheeled", "tracked"):
                if cls in veh_mobs:
                    return cls
        return "foot"

    def base_speed_kmh(self, pieces=None):
        """动态基础机动力（km/h）：按具体载具型号/步兵编成 + 重武器拖累 + 车辆超载乘区实时计算。"""
        mob = self.mobility()
        eq = self.equipment_counts()
        heavy_towed = sum(int(eq.get(m, 0)) for m in ("at_gun_50", "howitzer_105", "howitzer_152", "flak_88"))
        heavy_man = sum(int(eq.get(m, 0)) for m in ("hmg", "mortar_81"))
        if mob == "foot":
            base = 4.5
            if heavy_towed > 0:
                base *= 0.65
            elif heavy_man > 0:
                base *= 0.85
        else:
            veh_speeds = []
            if self.vehicle_model and self.vehicle_model in VEHICLE_SPECS:
                veh_speeds.append(float(VEHICLE_SPECS[self.vehicle_model].get("base_speed_kmh", 20.0)))
            for m, n in eq.items():
                if int(n) > 0 and m in VEHICLE_SPECS:
                    veh_speeds.append(float(VEHICLE_SPECS[m].get("base_speed_kmh", 20.0)))
            base = min(veh_speeds) if veh_speeds else (28.0 if mob == "wheeled" else 18.0)
            if heavy_towed > 0:
                base *= 0.80
        # 载具超载折损
        ov = self.overload_debuff() if hasattr(self, "overload_debuff") else {}
        base *= float(ov.get("speed_mult", 1.0))
        # 若为部署态重武器阵地（未撤收且无人/无牵引），无法自行高速奔袭
        if self.is_heavy_emplacement and self.unit_state == "deployed":
            base = min(base, 0.5)
        return max(0.3, round(float(base), 3))

    def get_operator_info(self, pieces=None):
        """解析当前重武器/载具的有效操炮班组、操炮人数与多人加成 Buff。"""
        op_id = self.operated_by
        op_piece = pieces.get(op_id) if (pieces and op_id) else None
        sel_ids = list((self.meta or {}).get("operator_soldier_ids") or [])
        weapon = self.deployed_weapon
        if not weapon:
            for m, n in self.equipment_counts().items():
                if int(n) > 0 and EQUIPMENT.get(m, {}).get("fire"):
                    weapon = m
                    break
        op_sq = None
        op_persons = 0
        if op_piece is not None:
            sqs = op_piece.squads()
            if sqs:
                op_sq = max(sqs, key=lambda s: heavy_operator_efficiency(s, weapon or "rifle"))
            if sel_ids:
                sol_meta = (op_piece.meta or {}).get("soldiers")
                if isinstance(sol_meta, list) and sol_meta:
                    valid_ids = {
                        str(s.get("id")) for s in sol_meta
                        if float(s.get("hp", 100.0)) > 0 and not s.get("cannot_fire")
                    }
                    op_persons = sum(1 for sid in sel_ids if str(sid) in valid_ids)
                else:
                    op_persons = min(len(sel_ids), op_piece.persons())
            else:
                op_persons = int((self.meta or {}).get("operator_persons") or op_piece.persons())
        elif self.persons() > 0:
            sqs = self.squads()
            op_sq = sqs[0] if sqs else None
            op_persons = self.persons()
        eff = heavy_operator_efficiency(op_sq, weapon or "rifle") if op_sq is not None else 0.0
        if op_persons <= 0:
            crew_buff = 0.0
        elif op_persons == 1:
            crew_buff = 0.70
        elif op_persons == 2:
            crew_buff = 1.00
        elif op_persons == 3:
            crew_buff = 1.18
        else:
            crew_buff = 1.32
        return {
            "operator_piece": op_piece,
            "operator_squad": op_sq,
            "operator_persons": int(op_persons),
            "operator_soldier_ids": sel_ids,
            "operator_eff": float(eff),
            "crew_buff_mult": float(crew_buff),
            "total_mult": float(eff * crew_buff) if op_persons > 0 else 0.0,
        }

    def has_buff(self, key):
        return any(s.has_buff(key) for s in self.squads())

    def mean_org(self):
        sq = self.squads()
        w = sum(s.persons for s in sq)
        return sum(s.org * s.persons for s in sq) / w if w > 0 else 0.0

    def to_dict(self):
        return dict(id=self.id, name=self.name, team=self.team, px=list(self.px),
                    pos=[round(self.pos[0], 5), round(self.pos[1], 5)],
                    level=self.level, radio_silence=self.radio_silence, decoy=self.decoy,
                    fortification=self.fortification, footprint_px=self.footprint_px,
                    control_radius_km=self.control_radius_km, fake_strength=self.fake_strength,
                    stock=dict(self.stock), ammo_by_type=dict(self.ammo_by_type),
                    stock_cap=dict(self.stock_cap),
                    moved_last_turn=self.moved_last_turn,
                    shock_hours=self.shock_hours,
                    radio_channel=self.radio_channel,
                    is_heavy_emplacement=self.is_heavy_emplacement,
                    deployed_weapon=self.deployed_weapon,
                    attached_to=self.attached_to,
                    unit_state=self.unit_state,
                    operated_by=self.operated_by,
                    vehicle_model=self.vehicle_model,
                    cargo=self.cargo,
                    status_debuffs=self.status_debuffs,
                    facing_deg=round(self.facing_deg, 2),
                    roe=self.roe,
                    meta=dict(self.meta),
                    root=self.root.to_dict())

    @classmethod
    def from_dict(cls, d):
        p = cls(d["id"], d["name"], d["team"], d["px"], Node.from_dict(d["root"]), d.get("level", "division"),
                d.get("radio_silence", False), d.get("decoy", False), d.get("fortification", 0.0),
                d.get("footprint_px", 16), d.get("control_radius_km", 3.0), d.get("fake_strength"),
                radio_channel=d.get("radio_channel", "A"),
                is_heavy_emplacement=d.get("is_heavy_emplacement", False),
                deployed_weapon=d.get("deployed_weapon"),
                pos=d.get("pos"),
                attached_to=d.get("attached_to"),
                unit_state=d.get("unit_state"),
                operated_by=d.get("operated_by"),
                vehicle_model=d.get("vehicle_model"),
                cargo=d.get("cargo"),
                status_debuffs=d.get("status_debuffs"),
                facing_deg=d.get("facing_deg", 0.0),
                roe=d.get("roe", "return_fire"),
                meta=d.get("meta"))
        p.stock = dict(d.get("stock", p.stock))
        p.ammo_by_type = dict(d.get("ammo_by_type", {}))
        p.stock_cap = dict(d.get("stock_cap", p.stock_cap))
        p.moved_last_turn = bool(d.get("moved_last_turn", False))
        if "shock_hours" in d:
            p.shock_hours = float(d["shock_hours"])
        else:
            p.shock_ttl = int(d.get("shock_ttl", 0))
        return p


def make_vehicle_piece(pid, name, team, px, vehicle_model, crew_enlisted=0, crew_cadre=0, org=100.0, **kwargs):
    """构造独立载具单位（自带容器 cargo 与软超载/战损 Debuff，支持 0 人空车或 min_crew=1 单人驾驶）。"""
    spec = VEHICLE_SPECS[vehicle_model]
    sq = Squad(
        f"{pid}-veh", "vehicle_crew",
        enlisted=int(crew_enlisted), cadre=int(crew_cadre),
        equipment={vehicle_model: 1} if vehicle_model in EQUIPMENT else {},
        org=org,
        enlisted_full=max(int(crew_enlisted), int(spec["full_crew"])),
        cadre_full=max(int(crew_cadre), 1),
    )
    root = Node(pid, name, "squad", squad=sq)
    return MapPiece(pid, name, team, px, root, level="squad", footprint_px=1,
                    vehicle_model=vehicle_model, **kwargs)


def bearing_deg(src_pos, dst_pos):
    """计算从 src_pos 指向 dst_pos 的方位角 [0, 360)。"""
    dr = float(dst_pos[0]) - float(src_pos[0])
    dc = float(dst_pos[1]) - float(src_pos[1])
    if abs(dr) < 1e-9 and abs(dc) < 1e-9:
        return 0.0
    return math.degrees(math.atan2(dr, dc)) % 360.0


def facing_cos(shooter_pos, target_pos, target_facing_deg):
    """计算来袭方向（目标->射手）与目标朝向的余弦相似度 [-1, 1]：+1 为纯正面，0 为侧面，-1 为背面。"""
    incoming = bearing_deg(target_pos, shooter_pos)
    diff = math.radians(incoming - float(target_facing_deg))
    return math.cos(diff)


def threat_facing_deg(origin_pos, weighted_threats, fallback_deg=0.0):
    """纯数学工具：给定 [(pos, weight), ...]，计算总威胁合矢量朝向角，供上层脚本调用。"""
    vr, vc = 0.0, 0.0
    for pos, w in (weighted_threats or ()):
        dr = float(pos[0]) - float(origin_pos[0])
        dc = float(pos[1]) - float(origin_pos[1])
        d = math.hypot(dr, dc)
        if d > 1e-9 and float(w) > 0.0:
            vr += (dr / d) * float(w)
            vc += (dc / d) * float(w)
    if math.hypot(vr, vc) <= 1e-9:
        return float(fallback_deg) % 360.0
    return math.degrees(math.atan2(vr, vc)) % 360.0


def make_squad_piece(pid, name, team, px, squad_kind, strength=1.0, org=100.0, **kwargs):
    """便捷构造最小地图实体（单个班级 MapPiece）。"""
    sq = make_squad(squad_kind, f"{pid}-sq", strength=strength, org=org)
    root = Node(pid, name, "squad", squad=sq)
    return MapPiece(pid, name, team, px, root, level="squad", footprint_px=1, **kwargs)


def _squad_mobility(squad):
    """班的机动类别：取其装备里最慢（foot < wheeled < tracked 反向）的主力装备类别。
    有履带战车的班按 tracked；有卡车/牵引炮的按 wheeled；其余步行。"""
    classes = set()
    for m in squad.equipment:
        mob = None
        if m in VEHICLE_SPECS:
            mob = VEHICLE_SPECS[m].get("mobility")
        if not mob and m in EQUIPMENT:
            mob = EQUIPMENT[m].get("mobility")
        if mob and mob != "foot":
            classes.add(mob)
    if not classes:
        return "foot"
    # 牵引/车载班：只要有履带装备就按履带，否则轮式
    return "tracked" if "tracked" in classes else "wheeled"


# ==========================================
# 🏗️ 批量建编制
# ==========================================
def _squad_node(kind, nid, strength=1.0, org=100.0):
    return Node(nid, nid, "squad", squad=make_squad(kind, nid, strength, org))


def build_platoon(pid, kinds, strength=1.0, org=100.0):
    return Node(pid, pid, "platoon", [_squad_node(k, f"{pid}-{i + 1}", strength, org)
                                      for i, k in enumerate(kinds)])


def build_company(cid, platoons, strength=1.0, org=100.0, with_hq=True):
    """platoons: 每个排的班种列表的列表。"""
    ch = [build_platoon(f"{cid}-P{i + 1}", kinds, strength, org) for i, kinds in enumerate(platoons)]
    if with_hq:
        ch.append(Node(f"{cid}-HQ", f"{cid}连部", "platoon",
                       [_squad_node("hq_squad", f"{cid}-HQ-1", strength, org)]))
    return Node(cid, cid, "company", ch)


def build_infantry_battalion(bid, strength=1.0, org=100.0):
    inf_platoon = ["rifle_squad"] * 3
    comps = [build_company(f"{bid}-C{i + 1}", [inf_platoon] * 3, strength, org) for i in range(3)]
    support = build_company(f"{bid}-S", [["mg_squad"] * 2, ["mortar_squad"] * 2, ["at_squad"] * 2],
                            strength, org)
    return Node(bid, bid, "battalion", comps + [support])


def build_armor_battalion(bid, strength=1.0, org=100.0):
    comps = [build_company(f"{bid}-C{i + 1}", [["tank_squad"] * 3, ["tank_squad"] * 2 + ["mech_squad"]],
                           strength, org) for i in range(3)]
    return Node(bid, bid, "battalion", comps)


def build_division(did, kind="infantry", strength=1.0, org=100.0):
    """标准师模板：infantry(3 步兵团×3 营 + 炮兵/工兵/辎重/防空) 或 armor(2 装甲团 + 1 装甲步兵团 + 支援)。"""
    if kind == "infantry":
        regs = [Node(f"{did}-R{i + 1}", f"{did}-R{i + 1}", "regiment",
                     [build_infantry_battalion(f"{did}-R{i + 1}-B{j + 1}", strength, org) for j in range(3)])
                for i in range(3)]
    elif kind == "armor":
        regs = [Node(f"{did}-R{i + 1}", f"{did}-R{i + 1}", "regiment",
                     [build_armor_battalion(f"{did}-R{i + 1}-B{j + 1}", strength, org) for j in range(2)])
                for i in range(2)]
        regs.append(Node(f"{did}-R3", f"{did}-R3", "regiment",
                         [build_infantry_battalion(f"{did}-R3-B{j + 1}", strength, org) for j in range(2)]))
    else:
        raise ValueError(f"未知师模板: {kind}")
    artillery = Node(f"{did}-ART", f"{did}炮兵团", "regiment", [
        build_company(f"{did}-ART-C{i + 1}", [["gun_squad"] * 3, ["heavy_gun_squad"] * 2], strength, org)
        for i in range(2)])
    support = Node(f"{did}-SUP", f"{did}保障群", "regiment", [
        build_company(f"{did}-ENG", [["engineer_squad"] * 3], strength, org),
        build_company(f"{did}-TRN", [["transport_squad"] * 6, ["transport_squad"] * 6,
                                     ["porter_squad"] * 6, ["porter_squad"] * 6], strength, org),
        build_company(f"{did}-AA", [["aa_squad"] * 3], strength, org),
        build_company(f"{did}-REC", [["recon_squad"] * 3], strength, org),
    ])
    hq = Node(f"{did}-HQ", f"{did}师部", "platoon", [_squad_node("hq_squad", f"{did}-HQ-1", strength, org)])
    return Node(did, did, "division", regs + [artillery, support, hq])


# ==========================================
# 🧩 连以上总部：动态混编（Kampfgruppe）
# ==========================================
def _merge_buffs(buff_dicts):
    out = {}
    for b in buff_dicts:
        for k, v in b.items():
            if k not in out:
                out[k] = v
            elif v >= 1.0 and out[k] >= 1.0:
                out[k] = max(out[k], v)
            else:
                out[k] = min(out[k], v)
    return out


def merge_squads(squads, new_id, loose_equipment=None, cohesion_penalty=MIX_COHESION_PENALTY):
    """把若干班（及散落装备）整合成一个混编班。战斗力由新账本即时重算。

    - 人、骨干、装备三账本逐项相加；满编账本同样相加（缺口 = 合计缺口）；
    - 组织度 = 按兵员加权平均 × 磨合折扣，再被新骨干比率的天花板封顶；
    - buff：增益取最大、减益取最小。
    """
    squads = list(squads)
    if not squads and not loose_equipment:
        raise ValueError("没有可混编的对象")
    enlisted = sum(s.enlisted for s in squads)
    cadre = sum(s.cadre for s in squads)
    eq = _sum_dicts(s.equipment for s in squads)
    eq_full = _sum_dicts(s.equipment_full for s in squads)
    for m, n in (loose_equipment or {}).items():
        eq[m] = eq.get(m, 0) + int(n)
        eq_full[m] = eq_full.get(m, 0) + int(n)
    persons = sum(s.persons for s in squads)
    org = (sum(s.org * s.persons for s in squads) / persons) if persons > 0 else 0.0
    merged = Squad(new_id, "mixed", enlisted, cadre, eq,
                   sum(s.enlisted_full for s in squads), sum(s.cadre_full for s in squads),
                   eq_full, _merge_buffs(s.buffs for s in squads), org * cohesion_penalty)
    merged.clamp_org()
    return merged


def split_squad(squad, take_enlisted, take_cadre=0, take_equipment=None, new_id=None):
    """战地拆班（与 merge_squads 对称）：从原班划出人/骨干/装备建立独立分队，三账本严格守恒。"""
    te = max(0, min(squad.enlisted, int(take_enlisted)))
    tc = max(0, min(squad.cadre, int(take_cadre)))
    eq_take = {}
    for m, n in (take_equipment or {}).items():
        got = max(0, min(squad.equipment.get(m, 0), int(n)))
        if got > 0:
            eq_take[m] = got
    if te == 0 and tc == 0 and not eq_take:
        raise ValueError("拆班人数与装备不能全为 0")

    squad.enlisted -= te
    squad.cadre -= tc
    for m, n in eq_take.items():
        rem = squad.equipment[m] - n
        if rem > 0:
            squad.equipment[m] = rem
        else:
            squad.equipment.pop(m, None)

    # 满编参考值同步按拆分数量转移，保证总编制守恒
    ef_take = min(squad.enlisted_full, te)
    cf_take = min(squad.cadre_full, tc)
    squad.enlisted_full = max(squad.enlisted, squad.enlisted_full - ef_take)
    squad.cadre_full = max(squad.cadre, squad.cadre_full - cf_take)
    eq_full_take = {}
    for m, n in eq_take.items():
        ftake = min(squad.equipment_full.get(m, 0), n)
        eq_full_take[m] = max(n, ftake)
        rem_f = squad.equipment_full.get(m, 0) - ftake
        if rem_f > 0:
            squad.equipment_full[m] = rem_f
        else:
            squad.equipment_full.pop(m, None)

    det_id = new_id or f"{squad.id}-DET"
    det = Squad(det_id, squad.kind, te, tc, eq_take,
                max(1, ef_take) if te > 0 else 0, cf_take, eq_full_take,
                dict(squad.buffs), squad.org)
    return det


def heavy_operator_efficiency(squad, weapon_model):
    """计算班组操作独立重武器单位的效率倍率：取决于士官兵种属性（buffs）与骨干保留率。"""
    if squad.persons <= 0:
        return 0.0
    # 1. 士官/班组专业对口度（解耦硬编码兵种名，同时兼容装备与 Buff 派生能力）
    if weapon_model == "flak_88":
        specialty = 1.0 if (squad.has_buff("anti_air") or squad.kind == "aa_squad") else 0.5
    elif weapon_model in ("howitzer_105", "howitzer_152", "mortar_81"):
        has_arty = "artillery" in squad.arm_tags() or squad.kind in ("gun_squad", "heavy_gun_squad", "mortar_squad")
        specialty = 1.0 if has_arty else 0.55
    elif weapon_model == "at_gun_50":
        specialty = 1.0 if (squad.has_buff("anti_armor") or squad.kind == "at_squad") else 0.65
    elif weapon_model == "hmg":
        has_inf = (
            squad.equipment.get("hmg", 0) > 0
            or squad.equipment.get("lmg", 0) > 0
            or squad.equipment.get("rifle", 0) > 0
            or squad.kind in ("mg_squad", "rifle_squad", "mech_squad")
        )
        specialty = 1.0 if has_inf else 0.75
    else:
        specialty = 0.7
    # 2. 士官/骨干完整度（无骨干带队操作复杂重武器效率再折半）
    cadre_f = 0.5 if squad.cadre <= 0 else (0.7 + 0.3 * min(1.0, squad.cadre_ratio / CADRE_FULL_RATIO))
    return round(specialty * cadre_f, 4)


def tow_profile(carrier_piece, payload_piece):
    """通用牵引/携行评估原语：支持重武器牵引、坦克搭载步兵（Tank Desant）及牵引车抢救受损重坦。"""
    has_tank = carrier_piece.vehicle_model in ("tank_medium", "tank_heavy") or any(
        s.equipment.get(m, 0) > 0 for s in carrier_piece.squads() for m in ("tank_medium", "tank_heavy")
    )
    if has_tank and payload_piece.mobility() == "foot" and not payload_piece.deployed_weapon and not payload_piece.vehicle_model:
        return {"can_move": True, "speed_mult": 0.95, "org_cost_per_h": 0.0, "mode": "tank_desant"}

    payload_is_armor = bool(payload_piece.vehicle_model) or any(
        s.equipment.get(m, 0) > 0 for s in payload_piece.squads() for m in ("tank_medium", "tank_heavy", "halftrack")
    )
    if payload_is_armor:
        has_tractor = carrier_piece.vehicle_model == "tractor" or any(
            s.equipment.get("tractor", 0) > 0 for s in carrier_piece.squads()
        )
        if has_tractor:
            return {"can_move": True, "speed_mult": 0.65, "org_cost_per_h": 0.5, "mode": "armor_recovery"}
        return {"can_move": False, "speed_mult": 0.0, "org_cost_per_h": 0.0, "mode": "immovable"}

    weapon = payload_piece.deployed_weapon
    if not weapon:
        eqs = [m for s in payload_piece.squads() for m, n in s.equipment.items() if n > 0 and EQUIPMENT.get(m, {}).get("deployable")]
        weapon = eqs[0] if eqs else None
    spec = EQUIPMENT.get(weapon, {})
    weight = spec.get("weight", "medium")
    has_vehicle = bool(carrier_piece.vehicle_model) or any(
        m in VEHICLE_SPECS and n > 0
        for s in carrier_piece.squads()
        for m, n in s.equipment.items()
    ) or carrier_piece.mobility() in ("wheeled", "tracked")

    if has_vehicle:
        if weight == "heavy":
            return {"can_move": True, "speed_mult": 0.85, "org_cost_per_h": 0.0, "mode": "vehicle_tow"}
        return {"can_move": True, "speed_mult": 0.95, "org_cost_per_h": 0.0, "mode": "vehicle_tow"}

    # 纯步兵人力推拉/扛运
    crew_need = max(1, int(spec.get("crew", 3)))
    persons = carrier_piece.persons()
    if weight == "heavy":
        if persons < crew_need:
            return {"can_move": False, "speed_mult": 0.0, "org_cost_per_h": 0.0, "mode": "immovable"}
        return {"can_move": True, "speed_mult": 0.08, "org_cost_per_h": 4.0, "mode": "infantry_haul"}
    if weapon == "at_gun_50":
        if persons < crew_need:
            return {"can_move": False, "speed_mult": 0.0, "org_cost_per_h": 0.0, "mode": "immovable"}
        return {"can_move": True, "speed_mult": 0.18, "org_cost_per_h": 2.5, "mode": "infantry_haul"}
    return {"can_move": True, "speed_mult": 0.65, "org_cost_per_h": 1.0, "mode": "infantry_haul"}


def squads_within(squads_with_px, center_px, radius_px):
    cr, cc = center_px
    return [s for s, (r, c) in squads_with_px if math.hypot(r - cr, c - cc) <= radius_px]


# ==========================================
# 🎖️ 独立指挥官实体与顺位继承（从指挥层迁入单位模块）
# ==========================================
RANK_ORDER = (
    "division_cmd",
    "brigade_cmd",
    "regiment_cmd",
    "battalion_cmd",
    "company_cmd",
    "company_xo",
    "company_sgt",
    "platoon_cmd",
    "platoon_sgt",
    "squad_ldr",
)

RANK_SPECS = {
    "division_cmd":  dict(echelon="division",  radius_km=15.0, delay_turns=0, max_zones=8, max_area_px=3000, ref_area_px=400, max_buff=0.25),
    "brigade_cmd":   dict(echelon="brigade",   radius_km=10.0, delay_turns=0, max_zones=7, max_area_px=2000, ref_area_px=280, max_buff=0.22),
    "regiment_cmd":  dict(echelon="regiment",  radius_km=7.0,  delay_turns=0, max_zones=6, max_area_px=1400, ref_area_px=200, max_buff=0.20),
    "battalion_cmd": dict(echelon="battalion", radius_km=4.0,  delay_turns=0, max_zones=5, max_area_px=900,  ref_area_px=120, max_buff=0.20),
    "company_cmd":   dict(echelon="company",   radius_km=2.4,  delay_turns=0, max_zones=4, max_area_px=400,  ref_area_px=60,  max_buff=0.25),
    "company_xo":    dict(echelon="company",   radius_km=1.6,  delay_turns=1, max_zones=2, max_area_px=200,  ref_area_px=45,  max_buff=0.20),
    "company_sgt":   dict(echelon="company",   radius_km=1.0,  delay_turns=2, max_zones=1, max_area_px=80,   ref_area_px=30,  max_buff=0.15),
    "platoon_cmd":   dict(echelon="platoon",   radius_km=1.0,  delay_turns=0, max_zones=2, max_area_px=100,  ref_area_px=20,  max_buff=0.20),
    "platoon_sgt":   dict(echelon="platoon",   radius_km=0.6,  delay_turns=1, max_zones=1, max_area_px=40,   ref_area_px=15,  max_buff=0.12),
    "squad_ldr":     dict(echelon="squad",     radius_km=0.3,  delay_turns=1, max_zones=1, max_area_px=12,   ref_area_px=6,   max_buff=0.08),
}

DEMOTION_NEXT = {
    "division_cmd": "brigade_cmd",
    "brigade_cmd": "regiment_cmd",
    "regiment_cmd": "battalion_cmd",
    "battalion_cmd": "company_cmd",
    "company_cmd": "company_xo",
    "company_xo": "company_sgt",
    "company_sgt": "platoon_cmd",
    "platoon_cmd": "platoon_sgt",
    "platoon_sgt": "squad_ldr",
}

DEFAULT_SHOCK_HOURS = 12.0


class Commander:
    """地图上的独立指挥官实体。"""

    def __init__(
        self,
        cid,
        name,
        team,
        px,
        rank="company_cmd",
        echelon=None,
        unit_id=None,
        subordinate_ids=None,
        has_radio=True,
        radio_silence=False,
        alive=True,
        acting=False,
        cadre_piece_id=None,
        parent_commander_id=None,
        **overrides,
    ):
        if rank not in RANK_SPECS:
            raise ValueError(f"未知指挥官职级: {rank}")
        spec = dict(RANK_SPECS[rank])
        spec.update(overrides)
        self.id = cid
        self.name = name
        self.team = team
        self.px = (int(px[0]), int(px[1]))
        self.rank = rank
        self.echelon = echelon or spec["echelon"]
        self.unit_id = unit_id or cid
        self.subordinate_ids = list(subordinate_ids or [])
        self.has_radio = bool(has_radio)
        self.radio_silence = bool(radio_silence)
        ch = overrides.get("radio_channel", "A")
        self.radio_channel = ch if ch in RADIO_CHANNELS else "A"
        self.alive = bool(alive)
        self.acting = bool(acting)
        self.cadre_piece_id = cadre_piece_id
        self.parent_commander_id = parent_commander_id
        self.radius_km = float(spec["radius_km"])
        self.delay_turns = int(spec["delay_turns"])
        self.max_zones = int(spec["max_zones"])
        self.max_area_px = int(spec["max_area_px"])
        self.ref_area_px = int(spec["ref_area_px"])
        self.max_buff = float(spec["max_buff"])
        self.base_exposure = float(overrides.get("base_exposure", 0.8))
        self.last_px = tuple(overrides["last_px"]) if overrides.get("last_px") else self.px
        # 所有状态持续时间统一以物理小时计量，彻底消除与可变 turn_hours 的耦合
        self.still_hours = float(overrides.get("still_hours", overrides.get("still_turns", 0) * 6.0))
        self.briefing_buff_hours = float(overrides.get("briefing_buff_hours", overrides.get("briefing_buff_ttl", 0) * 6.0))
        self.council_ao_hours = float(overrides.get("council_ao_hours", overrides.get("council_ao_ttl", 0) * 6.0))
        self.shock_hours = float(overrides.get("shock_hours", overrides.get("shock_ttl", 0) * 6.0))
        self._shock_applied = bool(overrides.get("_shock_applied", False))
        self.council_streak = int(overrides.get("council_streak", 0))
        self.council_cooldown_hours = float(overrides.get("council_cooldown_hours", 0.0))
        self.post_council_exposure_hours = float(overrides.get("post_council_exposure_hours", 0.0))
        self.post_council_exposure_val = float(overrides.get("post_council_exposure_val", 0.0))
        self.order_churn = float(overrides.get("order_churn", 0.0))
        self.war_council_sync_hours = float(overrides.get("war_council_sync_hours", 0.0))
        self.attached_to = overrides.get("attached_to")
        self.meta = dict(overrides.get("meta") or {})

    # 向后兼容属性（以 6h=1 turn 换算）
    @property
    def still_turns(self):
        return int(math.floor(self.still_hours / 6.0 + 1e-9))

    @still_turns.setter
    def still_turns(self, val):
        self.still_hours = max(0.0, float(val) * 6.0)

    @property
    def briefing_buff_ttl(self):
        return int(math.ceil(self.briefing_buff_hours / 6.0)) if self.briefing_buff_hours > 1e-9 else 0

    @briefing_buff_ttl.setter
    def briefing_buff_ttl(self, val):
        self.briefing_buff_hours = max(0.0, float(val) * 6.0)

    @property
    def council_ao_ttl(self):
        return int(math.ceil(self.council_ao_hours / 6.0)) if self.council_ao_hours > 1e-9 else 0

    @council_ao_ttl.setter
    def council_ao_ttl(self, val):
        self.council_ao_hours = max(0.0, float(val) * 6.0)

    @property
    def shock_ttl(self):
        return int(math.ceil(self.shock_hours / 6.0)) if self.shock_hours > 1e-9 else 0

    @shock_ttl.setter
    def shock_ttl(self, val):
        self.shock_hours = max(0.0, float(val) * 6.0)

    def tick_hours(self, dt_hours):
        dt = max(0.0, float(dt_hours))
        if self.briefing_buff_hours > 0:
            self.briefing_buff_hours = max(0.0, self.briefing_buff_hours - dt)
        if self.council_ao_hours > 0:
            self.council_ao_hours = max(0.0, self.council_ao_hours - dt)
        if self.shock_hours > 0:
            self.shock_hours = max(0.0, self.shock_hours - dt)
        if self.post_council_exposure_hours > 0:
            self.post_council_exposure_hours = max(0.0, self.post_council_exposure_hours - dt)
            if self.post_council_exposure_hours <= 1e-9:
                self.post_council_exposure_val = 0.0
        if self.council_cooldown_hours > 0:
            self.council_cooldown_hours = max(0.0, self.council_cooldown_hours - dt)
            if self.council_cooldown_hours <= 1e-9:
                self.council_streak = 0
        if self.order_churn > 0:
            self.order_churn = max(0.0, self.order_churn - 0.5 * dt)
        if self.war_council_sync_hours > 0:
            self.war_council_sync_hours = max(0.0, self.war_council_sync_hours - dt)

    def effective_max_area_px(self):
        area = float(self.max_area_px)
        if self.council_ao_hours > 1e-9:
            area *= 1.5
        if self.radio_silence:
            area *= 0.5
        return max(10, int(round(area)))

    def effective_radius_km(self):
        r = self.radius_km
        if self.briefing_buff_hours > 1e-9:
            r *= 1.35
        if self.shock_hours > 1e-9:
            r *= 0.75
        if self.radio_silence:
            r = max(0.8, r * 0.35)
        return r

    def effective_delay_turns(self):
        d = self.delay_turns
        if self.briefing_buff_hours > 1e-9 and d > 0:
            d = max(0, d - 1)
        if self.shock_hours > 1e-9:
            d += 1
        return d

    def apply_rank(self, new_rank, acting=True):
        spec = RANK_SPECS[new_rank]
        self.rank = new_rank
        self.acting = bool(acting)
        self.radius_km = float(spec["radius_km"])
        self.delay_turns = int(spec["delay_turns"])
        self.max_zones = int(spec["max_zones"])
        self.max_area_px = int(spec["max_area_px"])
        self.ref_area_px = int(spec["ref_area_px"])
        self.max_buff = float(spec["max_buff"])

    def to_dict(self):
        return dict(
            id=self.id,
            name=self.name,
            team=self.team,
            px=list(self.px),
            rank=self.rank,
            echelon=self.echelon,
            unit_id=self.unit_id,
            subordinate_ids=list(self.subordinate_ids),
            has_radio=self.has_radio,
            radio_silence=self.radio_silence,
            radio_channel=self.radio_channel,
            alive=self.alive,
            acting=self.acting,
            cadre_piece_id=self.cadre_piece_id,
            parent_commander_id=self.parent_commander_id,
            radius_km=self.radius_km,
            delay_turns=self.delay_turns,
            max_zones=self.max_zones,
            max_area_px=self.max_area_px,
            ref_area_px=self.ref_area_px,
            max_buff=self.max_buff,
            base_exposure=self.base_exposure,
            still_hours=self.still_hours,
            last_px=list(self.last_px) if self.last_px else None,
            briefing_buff_hours=self.briefing_buff_hours,
            council_ao_hours=self.council_ao_hours,
            shock_hours=self.shock_hours,
            _shock_applied=self._shock_applied,
            council_streak=self.council_streak,
            council_cooldown_hours=self.council_cooldown_hours,
            post_council_exposure_hours=self.post_council_exposure_hours,
            post_council_exposure_val=self.post_council_exposure_val,
            order_churn=self.order_churn,
            war_council_sync_hours=self.war_council_sync_hours,
            attached_to=self.attached_to,
            meta=dict(self.meta),
        )

    @classmethod
    def from_dict(cls, d):
        kw = dict(
            rank=d["rank"],
            echelon=d.get("echelon"),
            unit_id=d.get("unit_id"),
            subordinate_ids=d.get("subordinate_ids"),
            has_radio=d.get("has_radio", True),
            radio_silence=d.get("radio_silence", False),
            radio_channel=d.get("radio_channel", "A"),
            alive=d.get("alive", True),
            acting=d.get("acting", False),
            cadre_piece_id=d.get("cadre_piece_id"),
            parent_commander_id=d.get("parent_commander_id"),
            radius_km=d.get("radius_km", RANK_SPECS[d["rank"]]["radius_km"]),
            delay_turns=d.get("delay_turns", RANK_SPECS[d["rank"]]["delay_turns"]),
            max_zones=d.get("max_zones", RANK_SPECS[d["rank"]]["max_zones"]),
            max_area_px=d.get("max_area_px", RANK_SPECS[d["rank"]]["max_area_px"]),
            ref_area_px=d.get("ref_area_px", RANK_SPECS[d["rank"]]["ref_area_px"]),
            max_buff=d.get("max_buff", RANK_SPECS[d["rank"]]["max_buff"]),
            base_exposure=d.get("base_exposure", 0.8),
            last_px=d.get("last_px"),
            _shock_applied=d.get("_shock_applied", False),
            council_streak=d.get("council_streak", 0),
            council_cooldown_hours=d.get("council_cooldown_hours", 0.0),
            post_council_exposure_hours=d.get("post_council_exposure_hours", 0.0),
            post_council_exposure_val=d.get("post_council_exposure_val", 0.0),
            order_churn=d.get("order_churn", 0.0),
            war_council_sync_hours=d.get("war_council_sync_hours", 0.0),
            attached_to=d.get("attached_to"),
            meta=d.get("meta"),
        )
        if "still_hours" in d:
            kw["still_hours"] = d["still_hours"]
        else:
            kw["still_turns"] = d.get("still_turns", 0)
        if "briefing_buff_hours" in d:
            kw["briefing_buff_hours"] = d["briefing_buff_hours"]
        else:
            kw["briefing_buff_ttl"] = d.get("briefing_buff_ttl", 0)
        if "council_ao_hours" in d:
            kw["council_ao_hours"] = d["council_ao_hours"]
        else:
            kw["council_ao_ttl"] = d.get("council_ao_ttl", 0)
        if "shock_hours" in d:
            kw["shock_hours"] = d["shock_hours"]
        else:
            kw["shock_ttl"] = d.get("shock_ttl", 0)
        return cls(d["id"], d["name"], d["team"], d["px"], **kw)


def _piece_has_radio(piece):
    return any(s.equipment.get("radio", 0) > 0 or s.equipment.get("hq_radio", 0) > 0 for s in piece.squads())


def _piece_cadre_count(piece):
    return sum(s.cadre for s in piece.squads())


def resolve_succession(commanders, pieces):
    """指挥链顺位继承：只要建制内仍有军官/士官（cadre > 0）存活，指挥权按职级递补。"""
    events = []
    for c in commanders.values():
        if c.alive:
            if c.attached_to and c.attached_to in pieces:
                c.px = pieces[c.attached_to].px
            elif c.attached_to and c.attached_to not in pieces:
                c.attached_to = None
            if c.cadre_piece_id and c.cadre_piece_id in pieces:
                p = pieces[c.cadre_piece_id]
                if not c.attached_to:
                    c.px = p.px
                if _piece_cadre_count(p) <= 0:
                    c.alive = False
                    events.append({"commander": c.id, "event": "cadre_wiped"})

    for c in commanders.values():
        if not c.alive and not c._shock_applied and c.echelon in ("battalion", "regiment", "brigade", "division"):
            c._shock_applied = True
            for sub_c in commanders.values():
                if sub_c.team == c.team and sub_c.parent_commander_id == c.id:
                    sub_c.briefing_buff_hours = 0.0
                    sub_c.shock_hours = max(sub_c.shock_hours, DEFAULT_SHOCK_HOURS)
            for sid in c.subordinate_ids:
                p = pieces.get(sid)
                if p is not None and p.team == c.team:
                    p.shock_hours = max(p.shock_hours, DEFAULT_SHOCK_HOURS)
            events.append({"commander": c.id, "event": "decapitation_shock", "shock_hours": DEFAULT_SHOCK_HOURS})

    by_unit = {}
    for c in commanders.values():
        by_unit.setdefault((c.team, c.unit_id), []).append(c)

    for (team, unit_id), group in by_unit.items():
        group.sort(key=lambda x: RANK_ORDER.index(x.rank))
        top = group[0]
        if top.alive:
            continue
        successor = next((c for c in group[1:] if c.alive), None)
        if successor is not None:
            merged_subs = sorted(set(successor.subordinate_ids) | set(top.subordinate_ids))
            successor.subordinate_ids = merged_subs
            successor.acting = True
            events.append({
                "unit_id": unit_id,
                "lost": top.id,
                "successor": successor.id,
                "rank": successor.rank,
                "radius_km": successor.radius_km,
                "delay_turns": successor.delay_turns,
            })
            continue
        for sid in top.subordinate_ids:
            p = pieces.get(sid)
            if p is not None and p.team == team and _piece_cadre_count(p) > 0:
                new_rank = DEMOTION_NEXT.get(top.rank, "company_sgt")
                top.alive = True
                top.px = p.px
                top.cadre_piece_id = p.id
                top.has_radio = _piece_has_radio(p)
                top.apply_rank(new_rank, acting=True)
                events.append({
                    "unit_id": unit_id,
                    "successor": top.id,
                    "promoted_from_piece": p.id,
                    "rank": new_rank,
                    "radius_km": top.radius_km,
                    "delay_turns": top.delay_turns,
                })
                break
    return events


def transfer_squad_assets(src_squad, dst_squad, take_enlisted=0, take_cadre=0, take_equipment=None):
    """在两个班组间守恒划转士兵、骨干、装备及满编编制配额，并按兵员加权合并组织度。"""
    te = max(0, min(src_squad.enlisted, int(take_enlisted)))
    tc = max(0, min(src_squad.cadre, int(take_cadre)))
    eq_take = {}
    for m, n in (take_equipment or {}).items():
        got = max(0, min(src_squad.equipment.get(m, 0), int(n)))
        if got > 0:
            eq_take[m] = got
    if te == 0 and tc == 0 and not eq_take:
        return {"enlisted": 0, "cadre": 0, "equipment": {}}

    src_squad.enlisted -= te
    src_squad.cadre -= tc
    for m, n in eq_take.items():
        rem = src_squad.equipment[m] - n
        if rem > 0:
            src_squad.equipment[m] = rem
        else:
            src_squad.equipment.pop(m, None)

    ef_take = min(src_squad.enlisted_full, te)
    cf_take = min(src_squad.cadre_full, tc)
    src_squad.enlisted_full = max(src_squad.enlisted, src_squad.enlisted_full - ef_take)
    src_squad.cadre_full = max(src_squad.cadre, src_squad.cadre_full - cf_take)
    eq_full_take = {}
    for m, n in eq_take.items():
        ftake = min(src_squad.equipment_full.get(m, 0), n)
        eq_full_take[m] = max(n, ftake)
        rem_f = src_squad.equipment_full.get(m, 0) - ftake
        if rem_f > 0:
            src_squad.equipment_full[m] = rem_f
        else:
            src_squad.equipment_full.pop(m, None)

    dst_old_p = dst_squad.persons
    move_p = te + tc
    if move_p > 0:
        if dst_old_p + move_p > 0:
            dst_squad.org = (dst_squad.org * dst_old_p + src_squad.org * move_p) / float(dst_old_p + move_p)
    dst_squad.enlisted += te
    dst_squad.cadre += tc
    dst_squad.enlisted_full += ef_take
    dst_squad.cadre_full += cf_take
    for m, n in eq_take.items():
        dst_squad.equipment[m] = dst_squad.equipment.get(m, 0) + n
        dst_squad.equipment_full[m] = dst_squad.equipment_full.get(m, 0) + eq_full_take.get(m, n)
    dst_squad.clamp_org()
    return {"enlisted": te, "cadre": tc, "equipment": eq_take}


def _piece_is_empty(piece):
    sqs = piece.squads()
    if not sqs:
        return True
    return all(s.persons <= 0 and sum(s.equipment.values()) <= 0 for s in sqs)


def _prune_piece_from_world(world, pid):
    world.pieces.pop(pid, None)
    for c in world.commanders.values():
        if pid in c.subordinate_ids:
            c.subordinate_ids = [x for x in c.subordinate_ids if x != pid]
        if c.attached_to == pid:
            c.attached_to = None


def _check_cmd_link(world, cmd, piece):
    if piece.id in cmd.subordinate_ids:
        return True
    dist_km = world.grid.distance_km(cmd.px, piece.px)
    return dist_km <= max(0.5, float(cmd.radius_km))


def form_kampfgruppe(world, new_commander_id, target_ids, clear_deploy_claim=True):
    """战斗群跨建制配属（单线直属强制唯一）：从原直属上级剥离并挂入新指挥官麾下。"""
    cmd = world.commanders.get(new_commander_id) if hasattr(world, "commanders") else None
    if cmd is None or not cmd.alive:
        return {"ok": False, "reason": "commander_unavailable", "commander_id": new_commander_id}
    if isinstance(target_ids, str):
        target_ids = [target_ids]
    attached = []
    for tid in list(target_ids or []):
        if tid == new_commander_id:
            continue
        if tid in world.pieces:
            for other_c in world.commanders.values():
                if other_c.id != new_commander_id and tid in other_c.subordinate_ids:
                    other_c.subordinate_ids = [x for x in other_c.subordinate_ids if x != tid]
            if tid not in cmd.subordinate_ids:
                cmd.subordinate_ids.append(tid)
            p = world.pieces[tid]
            if not hasattr(p, "meta") or p.meta is None:
                p.meta = {}
            p.meta["direct_commander_id"] = new_commander_id
            if clear_deploy_claim:
                p.meta.pop("active_deploy_line_id", None)
                ao = p.meta.get("active_order")
                if isinstance(ao, dict) and ao.get("issuer") and ao.get("issuer") != new_commander_id:
                    p.meta.pop("active_order", None)
            attached.append(tid)
        elif tid in world.commanders:
            sub_c = world.commanders[tid]
            for other_c in world.commanders.values():
                if other_c.id != new_commander_id and tid in other_c.subordinate_ids:
                    other_c.subordinate_ids = [x for x in other_c.subordinate_ids if x != tid]
            if tid not in cmd.subordinate_ids:
                cmd.subordinate_ids.append(tid)
            sub_c.parent_commander_id = new_commander_id
            if not hasattr(sub_c, "meta") or sub_c.meta is None:
                sub_c.meta = {}
            sub_c.meta["parent_new_order_arrived"] = True
            attached.append(tid)
    return {"ok": True, "op": "form_kampfgruppe", "commander_id": new_commander_id, "attached_ids": attached}


def commander_reorganize(world, commander_id, spec, enforce_link=False):
    """指挥官任意混编/拆分/调拨其麾下班组与武器人员资产（支持 compose / merge_squad_pieces / transfer_squads / split_squad / transfer_assets / form_kampfgruppe）。"""
    cmd = world.commanders.get(commander_id)
    if cmd is None or not cmd.alive:
        return {"ok": False, "reason": "commander_unavailable", "commander": commander_id}
    op = str((spec or {}).get("op", "")).strip()
    if op == "form_kampfgruppe":
        targets = spec.get("target_ids") or spec.get("targets") or ([] if spec.get("target_id") is None else [spec.get("target_id")])
        return form_kampfgruppe(
            world,
            spec.get("new_commander_id") or commander_id,
            targets,
            clear_deploy_claim=bool(spec.get("clear_deploy_claim", True)),
        )
    pieces = world.pieces
    turn_idx = int(getattr(world, "turn", getattr(world, "turn_index", 0)))

    if op == "compose":
        sources = list(spec.get("sources") or [])
        target_pid = spec.get("target_piece_id") or spec.get("new_piece_id") or f"{cmd.id}-KG-{turn_idx + 1}"
        merge_into_one = bool(spec.get("merge", False))
        new_sq_id = spec.get("merged_squad_id") or f"{target_pid}-sq"
        cohesion_penalty = float(spec.get("cohesion_penalty", MIX_COHESION_PENALTY))
        enforce_v3_cap = bool(spec.get("enforce_v3_merge_cap", False))

        dst_p = pieces.get(target_pid)
        created_new = dst_p is None
        moved_squads = []
        touched_sources = []
        moved_ids = []

        # Collect (src_piece, squad)
        picks = []
        for entry in sources:
            if isinstance(entry, str):
                sp = pieces.get(entry)
                if sp is None or sp.team != cmd.team:
                    return {"ok": False, "reason": f"invalid_source:{entry}"}
                if enforce_link and not _check_cmd_link(world, cmd, sp):
                    return {"ok": False, "reason": f"out_of_command_link:{entry}"}
                for s in list(sp.squads()):
                    picks.append((sp, s))
            elif isinstance(entry, dict):
                spid = entry.get("piece_id")
                sp = pieces.get(spid)
                if sp is None or sp.team != cmd.team:
                    return {"ok": False, "reason": f"invalid_source:{spid}"}
                if enforce_link and not _check_cmd_link(world, cmd, sp):
                    return {"ok": False, "reason": f"out_of_command_link:{spid}"}
                sids = entry.get("squad_ids") or ([entry["squad_id"]] if entry.get("squad_id") else None)
                if sids is None:
                    for s in list(sp.squads()):
                        picks.append((sp, s))
                else:
                    for sid in sids:
                        s = sp.get_squad(sid)
                        if s is None:
                            return {"ok": False, "reason": f"missing_squad:{spid}/{sid}"}
                        picks.append((sp, s))

        if not picks:
            return {"ok": False, "reason": "no_squads_selected"}

        if enforce_v3_cap:
            tgt_lvl = str(dst_p.level if dst_p is not None else spec.get("level", "platoon"))
            picked_ids = {id(s) for _, s in picks}
            existing_p = sum(s.persons for s in dst_p.squads() if id(s) not in picked_ids) if dst_p is not None else 0
            total_p = existing_p + sum(s.persons for _, s in picks)
            if tgt_lvl != "squad" or any(sp.level != "squad" for sp, _ in picks) or total_p > DIRECT_MERGE_MAX_PERSONS:
                return {
                    "ok": False,
                    "reason": "requires_reorganize_echelon_event_or_exceeds_squad_cap",
                    "target_level": tgt_lvl,
                    "total_persons": total_p,
                    "max_persons": DIRECT_MERGE_MAX_PERSONS,
                }

        if dst_p is not None:
            if dst_p.team != cmd.team:
                return {"ok": False, "reason": "cross_team_forbidden"}
            if enforce_link and not _check_cmd_link(world, cmd, dst_p):
                return {"ok": False, "reason": f"target_out_of_link:{target_pid}"}

        for sp, s in picks:
            if dst_p is not None and sp.id == dst_p.id:
                continue
            old_p = max(1, sp.persons())
            taken = sp.remove_squad(s.id)
            if taken is None:
                continue
            moved_squads.append((sp, taken, old_p))
            moved_ids.append(taken.id)
            if sp.id not in touched_sources:
                touched_sources.append(sp.id)

        if created_new:
            pos_ref = spec.get("px") or moved_squads[0][0].pos
            if merge_into_one:
                merged_sq = merge_squads(
                    [sq for _, sq, _ in moved_squads], new_sq_id, cohesion_penalty=cohesion_penalty
                )
                root = Node(target_pid, spec.get("name", target_pid), spec.get("level", "platoon"), squad=merged_sq)
            else:
                root = Node(
                    target_pid, spec.get("name", target_pid), spec.get("level", "platoon"),
                    children=[Node(sq.id, sq.id, "squad", squad=sq) for _, sq, _ in moved_squads],
                )
            dst_p = MapPiece(
                target_pid, spec.get("name", target_pid), cmd.team, pos_ref, root,
                level=spec.get("level", "platoon"),
                footprint_px=int(spec.get("footprint_px", 1)),
                control_radius_km=float(spec.get("control_radius_km", 1.5)),
                radio_channel=spec.get("radio_channel", cmd.radio_channel),
                pos=pos_ref,
            )
            dst_p.stock = {k: 0.0 for k in dst_p.stock}
            dst_p.ammo_by_type = {}
            pieces[target_pid] = dst_p
        else:
            if merge_into_one:
                existing = list(dst_p.squads())
                for sq in existing:
                    dst_p.remove_squad(sq.id)
                merged_sq = merge_squads(
                    existing + [sq for _, sq, _ in moved_squads], new_sq_id, cohesion_penalty=cohesion_penalty
                )
                dst_p.add_squad(merged_sq)
            else:
                for _, sq, _ in moved_squads:
                    dst_p.add_squad(sq)

        # 按原棋子人数比例同步划转补给
        for sp, sq, old_p in moved_squads:
            frac = min(1.0, sq.persons / float(old_p))
            for rk in list(sp.stock.keys()):
                tr = float(sp.stock.get(rk, 0.0)) * frac
                sp.stock[rk] = max(0.0, float(sp.stock.get(rk, 0.0)) - tr)
                dst_p.stock[rk] = float(dst_p.stock.get(rk, 0.0)) + tr
            for ak in list(sp.ammo_by_type.keys()):
                atr = float(sp.ammo_by_type.get(ak, 0.0)) * frac
                sp.ammo_by_type[ak] = max(0.0, float(sp.ammo_by_type.get(ak, 0.0)) - atr)
                if atr > 1e-9:
                    dst_p.ammo_by_type[ak] = float(dst_p.ammo_by_type.get(ak, 0.0)) + atr
        dst_p.refresh_stock_cap()
        if dst_p.id not in cmd.subordinate_ids:
            cmd.subordinate_ids.append(dst_p.id)

        removed_pieces = []
        for spid in touched_sources:
            sp = pieces.get(spid)
            if sp is not None and _piece_is_empty(sp):
                _prune_piece_from_world(world, spid)
                removed_pieces.append(spid)
        return {
            "ok": True, "op": "compose", "commander": cmd.id,
            "target_piece": dst_p.id, "moved_squads": moved_ids,
            "removed_pieces": removed_pieces, "merge": merge_into_one,
        }

    if op == "transfer_squads":
        from_pid = spec.get("from_piece")
        sp = pieces.get(from_pid)
        if sp is None or sp.team != cmd.team:
            return {"ok": False, "reason": f"invalid_source:{from_pid}"}
        if enforce_link and not _check_cmd_link(world, cmd, sp):
            return {"ok": False, "reason": f"out_of_command_link:{from_pid}"}
        sids = list(spec.get("squad_ids") or [s.id for s in sp.squads()])
        sub_spec = {
            "op": "compose",
            "sources": [{"piece_id": from_pid, "squad_ids": sids}],
            "target_piece_id": spec.get("to_piece") or spec.get("new_piece_id") or f"{cmd.id}-KG-{turn_idx + 1}",
            "name": spec.get("name"),
            "level": spec.get("level", "platoon"),
            "px": spec.get("px", sp.pos),
            "merge": bool(spec.get("merge", False)),
        }
        if not sub_spec["name"]:
            sub_spec["name"] = sub_spec["target_piece_id"]
        res = commander_reorganize(world, commander_id, sub_spec, enforce_link=enforce_link)
        if res.get("ok"):
            res["op"] = "transfer_squads"
        return res

    if op == "split_squad":
        pid = spec.get("piece_id")
        sp = pieces.get(pid)
        if sp is None or sp.team != cmd.team:
            return {"ok": False, "reason": f"invalid_source:{pid}"}
        if enforce_link and not _check_cmd_link(world, cmd, sp):
            return {"ok": False, "reason": f"out_of_command_link:{pid}"}
        sid = spec.get("squad_id")
        det_piece = sp.split_squad_to_piece(
            sid,
            spec.get("take_enlisted", 0),
            take_cadre=spec.get("take_cadre", 0),
            take_equipment=spec.get("take_equipment"),
            new_piece_id=spec.get("new_piece_id"),
            new_squad_id=spec.get("new_squad_id"),
            new_name=spec.get("new_name"),
            offset_px=tuple(spec.get("offset_px", (0.15, 0.15))),
        )
        to_pid = spec.get("to_piece")
        if to_pid and to_pid in pieces and pieces[to_pid].team == cmd.team:
            dst_p = pieces[to_pid]
            if enforce_link and not _check_cmd_link(world, cmd, dst_p):
                return {"ok": False, "reason": f"target_out_of_link:{to_pid}"}
            for det_sq in det_piece.squads():
                dst_p.add_squad(det_sq)
            for rk, rv in det_piece.stock.items():
                dst_p.stock[rk] = float(dst_p.stock.get(rk, 0.0)) + float(rv)
            for ak, av in det_piece.ammo_by_type.items():
                dst_p.ammo_by_type[ak] = float(dst_p.ammo_by_type.get(ak, 0.0)) + float(av)
            dst_p.refresh_stock_cap()
            target_out = dst_p.id
        else:
            pieces[det_piece.id] = det_piece
            if det_piece.id not in cmd.subordinate_ids:
                cmd.subordinate_ids.append(det_piece.id)
            target_out = det_piece.id
        removed = []
        if _piece_is_empty(sp):
            _prune_piece_from_world(world, sp.id)
            removed.append(sp.id)
        return {
            "ok": True, "op": "split_squad", "commander": cmd.id,
            "from_piece": pid, "target_piece": target_out, "removed_pieces": removed,
        }

    if op == "transfer_assets":
        from_pid = spec.get("from_piece")
        to_pid = spec.get("to_piece", from_pid)
        sp = pieces.get(from_pid)
        dp = pieces.get(to_pid)
        if sp is None or dp is None or sp.team != cmd.team or dp.team != cmd.team:
            return {"ok": False, "reason": "invalid_piece_or_team"}
        if enforce_link and (not _check_cmd_link(world, cmd, sp) or not _check_cmd_link(world, cmd, dp)):
            return {"ok": False, "reason": "out_of_command_link"}
        src_sq = sp.get_squad(spec.get("from_squad")) if spec.get("from_squad") else (sp.squads()[0] if len(sp.squads()) == 1 else None)
        if src_sq is None:
            return {"ok": False, "reason": "missing_source_squad"}
        dst_sid = spec.get("to_squad")
        dst_sq = dp.get_squad(dst_sid) if dst_sid else (dp.squads()[0] if len(dp.squads()) == 1 else None)
        if dst_sq is None:
            dst_sq = make_custom_squad(
                dst_sid or f"{dp.id}-mix-{len(dp.squads()) + 1}",
                enlisted=0, cadre=0, equipment={}, kind=spec.get("kind", "custom_squad"),
            )
            dp.add_squad(dst_sq)
        old_sp_p = max(1, sp.persons())
        moved = transfer_squad_assets(
            src_sq, dst_sq,
            take_enlisted=spec.get("enlisted", 0),
            take_cadre=spec.get("cadre", 0),
            take_equipment=spec.get("equipment"),
        )
        move_p = moved["enlisted"] + moved["cadre"]
        if sp.id != dp.id and move_p > 0:
            frac = min(1.0, move_p / float(old_sp_p))
            for rk in list(sp.stock.keys()):
                tr = float(sp.stock.get(rk, 0.0)) * frac
                sp.stock[rk] = max(0.0, float(sp.stock.get(rk, 0.0)) - tr)
                dp.stock[rk] = float(dp.stock.get(rk, 0.0)) + tr
        sp.refresh_stock_cap()
        dp.refresh_stock_cap()
        if src_sq.persons <= 0 and sum(src_sq.equipment.values()) <= 0:
            sp.remove_squad(src_sq.id)
        removed = []
        if _piece_is_empty(sp):
            _prune_piece_from_world(world, sp.id)
            removed.append(sp.id)
        return {
            "ok": True, "op": "transfer_assets", "commander": cmd.id,
            "from_piece": from_pid, "to_piece": to_pid,
            "from_squad": src_sq.id, "to_squad": dst_sq.id,
            "moved": moved, "removed_pieces": removed,
        }

    if op == "merge_squad_pieces":
        src_pids = list(spec.get("source_pieces") or [])
        if len(src_pids) < 2:
            return {"ok": False, "reason": "need_at_least_two_squad_pieces"}
        src_pieces = []
        total_p = 0
        for spid in src_pids:
            sp = pieces.get(spid)
            if sp is None or sp.team != cmd.team:
                return {"ok": False, "reason": f"invalid_source:{spid}"}
            if sp.level != "squad":
                return {
                    "ok": False,
                    "reason": "requires_reorganize_echelon_event_or_exceeds_squad_cap",
                    "source_piece": spid,
                    "source_level": sp.level,
                }
            if enforce_link and not _check_cmd_link(world, cmd, sp):
                return {"ok": False, "reason": f"out_of_command_link:{spid}"}
            src_pieces.append(sp)
            total_p += sp.persons()
        if total_p > DIRECT_MERGE_MAX_PERSONS:
            return {
                "ok": False,
                "reason": "requires_reorganize_echelon_event_or_exceeds_squad_cap",
                "total_persons": total_p,
                "max_persons": DIRECT_MERGE_MAX_PERSONS,
            }
        all_sqs = [sq for sp in src_pieces for sq in sp.squads()]
        if not all_sqs:
            return {"ok": False, "reason": "no_squads_selected"}
        target_pid = spec.get("target_piece_id") or f"{cmd.id}-SQM-{turn_idx + 1}"
        new_sq_id = spec.get("merged_squad_id") or f"{target_pid}-sq"
        cohesion_penalty = float(spec.get("cohesion_penalty", MIX_COHESION_PENALTY))
        merged_sq = merge_squads(all_sqs, new_sq_id, cohesion_penalty=cohesion_penalty)
        pos_ref = spec.get("px") or src_pieces[0].pos
        root = Node(target_pid, spec.get("name", target_pid), "squad", squad=merged_sq)
        dst_p = MapPiece(
            target_pid, spec.get("name", target_pid), cmd.team, pos_ref, root,
            level="squad", footprint_px=1, control_radius_km=0.8,
            radio_channel=spec.get("radio_channel", src_pieces[0].radio_channel),
            pos=pos_ref,
        )
        dst_p.stock = {k: 0.0 for k in dst_p.stock}
        dst_p.ammo_by_type = {}
        for sp in src_pieces:
            for rk, rv in sp.stock.items():
                dst_p.stock[rk] = float(dst_p.stock.get(rk, 0.0)) + float(rv)
            for ak, av in sp.ammo_by_type.items():
                if float(av) > 1e-9:
                    dst_p.ammo_by_type[ak] = float(dst_p.ammo_by_type.get(ak, 0.0)) + float(av)
            if sp.id != dst_p.id:
                _prune_piece_from_world(world, sp.id)
        dst_p.refresh_stock_cap()
        pieces[dst_p.id] = dst_p
        if dst_p.id not in cmd.subordinate_ids:
            cmd.subordinate_ids.append(dst_p.id)
        return {
            "ok": True,
            "op": "merge_squad_pieces",
            "commander": cmd.id,
            "target_piece": dst_p.id,
            "level": dst_p.level,
            "persons": dst_p.persons(),
            "removed_pieces": [sp.id for sp in src_pieces if sp.id != dst_p.id],
            "counter": dst_p.counter_profile(),
        }

    return {"ok": False, "reason": f"unknown_op:{op}"}
