# -*- coding: utf-8 -*-
"""多维武器伤害矩阵与装备遗留（V2）。

内核只做物理：不同伤害类型对「人员 / 骨干 / 软装备 / 装甲装备」的杀伤系数、掩护削减、
组织度震荡，以及因极低组织度撤收失败而遗留的装备。**不写任何人性脚本**——
是否撤退、是否死战由指挥与推演层决定；本模块只回答「如果撤了，会丢多少」。

强度单位 = 有效火力点（见 toe.EQUIPMENT.fire）。杀伤比例 f = 1 - exp(-系数 × 强度 / 耐久池)。
所有取整经 carry 累计小数，确定性、不丢不重。
"""

from __future__ import annotations

import hashlib
import math

from . import autonomy_v3
from . import terrain_field as tf
from . import toe
from .toe import EQUIPMENT

# 每名人员 / 每件装备能吃多少火力点才倒下一半多
PERSON_DURABILITY = 15.0
SOFT_DURABILITY = 12.0
ARMOR_DURABILITY = 40.0
ORG_DURABILITY = 25.0
CADRE_LOSS_ORG_PENALTY = 40.0     # 骨干全灭时额外扣的组织度

# person/cadre: 对暴露人员；soft/armor: 对软装备/装甲装备；org: 组织度震荡；
# cover: 掩护/工事对该伤害的削减强度（0—1，1 = 掩护完全生效）
DAMAGE_MATRIX = {
    "small_arms": dict(person=1.0, soft=0.03, armor=0.0, org=0.6, cover=1.0, pixel=0.0),
    "he_blast":   dict(person=1.6, soft=0.9, armor=0.12, org=1.6, cover=0.6, pixel=0.5),
    "ap_kinetic": dict(person=0.05, soft=0.15, armor=1.0, org=0.2, cover=0.3, pixel=0.0),
    "heat_shaped": dict(person=0.3, soft=0.4, armor=1.0, org=0.4, cover=0.3, pixel=0.1),
    "incendiary": dict(person=1.0, soft=0.3, armor=0.1, org=1.8, cover=0.3, pixel=0.2),
    "suppression": dict(person=0.02, soft=0.0, armor=0.0, org=2.0, cover=0.5, pixel=0.0),
}

# 撤收失败：装备自重等级的基础遗留率
LEAVE_BASE = {"light": 0.05, "medium": 0.35, "heavy": 0.85}
DISORG_EXPONENT = 1.5
ENGINEER_TOW_EFFECT = 0.7
CAPTURE_BASE = 0.45
CAPTURE_ENGINEER_BONUS = 0.4


def _take(squad, key, expected, available):
    """把期望损失（浮点）经 carry 取整成整数，不超过 available。"""
    total = squad.carry.get(key, 0.0) + max(0.0, expected)
    n = int(total)
    n = min(n, available)
    squad.carry[key] = total - n if n < available else 0.0
    return n


def strike(squad, damage_type, intensity, cover=0.0, fortification=0.0, org_shock_mult=1.0):
    """对一个班结算一次打击。原地修改账本，返回战损报告。"""
    if damage_type not in DAMAGE_MATRIX:
        raise ValueError(f"未知伤害类型: {damage_type}")
    m = DAMAGE_MATRIX[damage_type]
    protection = min(0.95, max(0.0, (cover + fortification) * m["cover"]))
    eff = intensity * (1.0 - protection)
    rep = {"damage_type": damage_type, "intensity": intensity, "protection": round(protection, 3),
           "enlisted_lost": 0, "cadre_lost": 0, "equipment_lost": {}, "org_before": squad.org}
    if intensity <= 0 or (squad.persons <= 0 and not squad.equipment):
        rep["org_after"] = squad.org
        return rep

    # 人员：普通士兵与骨干按各自占比承伤
    pop = squad.persons
    if pop > 0:
        f = 1.0 - math.exp(-m["person"] * eff / (pop * PERSON_DURABILITY))
        lost_e = _take(squad, "enlisted", squad.enlisted * f, squad.enlisted)
        lost_c = _take(squad, "cadre", squad.cadre * f, squad.cadre)
        squad.enlisted -= lost_e
        squad.cadre -= lost_c
        rep["enlisted_lost"], rep["cadre_lost"] = lost_e, lost_c

    # 装备：软/装甲分别用各自系数与耐久
    n_soft = sum(n for mdl, n in squad.equipment.items() if EQUIPMENT.get(mdl, {}).get("armor") == "soft")
    n_arm = sum(n for mdl, n in squad.equipment.items() if EQUIPMENT.get(mdl, {}).get("armor") == "armored")
    f_soft = 1.0 - math.exp(-m["soft"] * eff / (max(1, n_soft) * SOFT_DURABILITY)) if m["soft"] > 0 else 0.0
    f_arm = 1.0 - math.exp(-m["armor"] * eff / (max(1, n_arm) * ARMOR_DURABILITY)) if m["armor"] > 0 else 0.0
    for mdl in sorted(squad.equipment):
        n = squad.equipment[mdl]
        f = f_arm if EQUIPMENT.get(mdl, {}).get("armor") == "armored" else f_soft
        lost = _take(squad, "eq:" + mdl, n * f, n)
        if lost:
            squad.equipment[mdl] = n - lost
            rep["equipment_lost"][mdl] = lost
    squad.equipment = {k: v for k, v in squad.equipment.items() if v > 0}

    # 组织度震荡：由火力强度相对人员规模决定；骨干折损再追加；侧后受击按 org_shock_mult 放大
    shock = 100.0 * (1.0 - math.exp(-m["org"] * eff / (max(1, pop) * ORG_DURABILITY)))
    shock *= max(0.0, float(org_shock_mult))
    cadre_hit = (rep["cadre_lost"] / squad.cadre_full) if squad.cadre_full > 0 else 0.0
    squad.org = max(0.0, squad.org - shock * (squad.org / 100.0 + 0.2) - CADRE_LOSS_ORG_PENALTY * cadre_hit)
    squad.clamp_org()
    rep["org_after"] = squad.org
    return rep


def abandonment_risk(squad, engineer_assist=0.0):
    """如果此刻被迫撤收，各装备的预期遗留比例（纯查询，不改账本）。"""
    disorg = max(0.0, 1.0 - squad.org / 100.0) ** DISORG_EXPONENT
    risk = {}
    for mdl in squad.equipment:
        w = EQUIPMENT.get(mdl, {}).get("weight", "medium")
        risk[mdl] = min(1.0, LEAVE_BASE[w] * disorg * (1.0 - ENGINEER_TOW_EFFECT * engineer_assist))
    return risk


def abandon_equipment(squad, engineer_assist=0.0):
    """被迫撤收时遗留装备。返回遗留的 {型号: 数量}（调用方负责写到像素格）。

    高组织度有序后撤 → disorg≈0 → 几乎全带走；极低组织度 → 重装备大比例遗留；
    engineer_assist（0—1，同队工兵/抢修班组的拖曳回收能力）按比例削减遗留。
    """
    left = {}
    risk = abandonment_risk(squad, engineer_assist)
    for mdl in sorted(squad.equipment):
        n = squad.equipment[mdl]
        k = _take(squad, "abandon:" + mdl, n * risk[mdl], n)
        if k:
            squad.equipment[mdl] = n - k
            left[mdl] = k
    squad.equipment = {k: v for k, v in squad.equipment.items() if v > 0}
    return left


def capture_equipment(abandoned, engineer_assist=0.0):
    """占领方清点遗留装备：只有一部分仍可用入编，其余视为损毁。返回 (可用, 损毁)。"""
    frac = min(0.95, CAPTURE_BASE + CAPTURE_ENGINEER_BONUS * engineer_assist)
    usable, wrecked = {}, {}
    for mdl, n in sorted(abandoned.items()):
        k = int(math.floor(n * frac + 0.5))
        if k:
            usable[mdl] = k
        if n - k:
            wrecked[mdl] = n - k
    return usable, wrecked


def engineer_assist_of(squads):
    """一批班组的拖曳/抢修能力（0—1）：salvage buff（模板或装备派生）加权的工兵占比，封顶 1。"""
    persons = sum(s.persons for s in squads) or 1
    cap = sum(float(s.get_buff("salvage", 0.0)) * s.persons for s in squads if s.has_buff("salvage"))
    return min(1.0, cap / persons * 6.0)


# ==========================================
# 👁️ 战时班组暴露度与开火残留判定（从 toe.py 迁入判定层）
# ==========================================
SQUAD_EXPOSURE = {
    "rifle_squad":      (0.4, 0.4),
    "mg_squad":         (0.6, 1.2),
    "mortar_squad":     (0.8, 1.8),
    "at_squad":         (0.7, 1.4),
    "recon_squad":      (0.15, 0.3),
    "engineer_squad":   (0.5, 0.4),
    "transport_squad":  (1.2, 0.2),
    "porter_squad":     (0.4, 0.2),
    "tank_squad":       (1.6, 2.5),
    "heavy_tank_squad": (2.2, 3.2),
    "gun_squad":        (1.5, 3.5),
    "fac_squad":        (0.5, 0.3),
    "heavy_gun_squad":  (2.0, 4.5),
    "aa_squad":         (1.4, 2.8),
    "mech_squad":       (1.1, 1.2),
    "hq_squad":         (1.0, 0.3),
}

DEFAULT_FIRING_EXPOSURE_HOURS = 12.0


class ExposureTracker:
    """独立维护班组开火暴露度残留（物理小时制），使 toe.py 保持纯粹的编制结构。"""

    def __init__(self, state=None):
        # squad_id -> {"hours": float, "pending": bool}
        self.state = {
            sid: {"hours": float(v.get("hours", 0.0)), "pending": bool(v.get("pending", False))}
            for sid, v in (state or {}).items()
        }

    def mark_fired(self, squad):
        if squad.persons > 0:
            st = self.state.setdefault(squad.id, {"hours": 0.0, "pending": False})
            st["pending"] = True

    def tick(self, dt_hours):
        dt = max(0.0, float(dt_hours))
        for sid in list(self.state):
            st = self.state[sid]
            if st["hours"] > 0.0:
                st["hours"] = max(0.0, st["hours"] - dt)
            if st["pending"]:
                st["hours"] = DEFAULT_FIRING_EXPOSURE_HOURS
                st["pending"] = False
            if st["hours"] <= 1e-9 and not st["pending"]:
                self.state.pop(sid, None)

    def squad_exposure(self, squad):
        if squad.persons <= 0 and not squad.equipment:
            return 0.0
        if squad.kind in SQUAD_EXPOSURE:
            base_exp, fire_exp = SQUAD_EXPOSURE[squad.kind]
        else:
            eq = squad.equipment
            if eq.get("tank_heavy", 0) > 0 or eq.get("howitzer_152", 0) > 0:
                base_exp, fire_exp = 2.0, 3.5
            elif eq.get("tank_medium", 0) > 0 or eq.get("howitzer_105", 0) > 0 or eq.get("flak_88", 0) > 0:
                base_exp, fire_exp = 1.5, 2.8
            elif eq.get("halftrack", 0) > 0 or eq.get("at_gun_50", 0) > 0 or eq.get("mortar_81", 0) > 0:
                base_exp, fire_exp = 0.8, 1.5
            elif eq.get("truck", 0) > 0 or eq.get("tractor", 0) > 0:
                base_exp, fire_exp = 1.2, 0.2
            elif squad.has_buff("recon_radius"):
                base_exp, fire_exp = 0.15, 0.3
            else:
                base_exp, fire_exp = 0.5, 0.5
        st = self.state.get(squad.id)
        extra = fire_exp if (st and st["hours"] > 1e-9) else 0.0
        return base_exp + extra

    def piece_exposure(self, piece):
        return sum(self.squad_exposure(s) for s in piece.squads())

    def commander_exposure(self, cmdr):
        if not cmdr.alive:
            return 0.0
        exp = float(getattr(cmdr, "base_exposure", 0.8))
        if not getattr(cmdr, "radio_silence", False) and getattr(cmdr, "post_council_exposure_hours", 0.0) > 1e-9:
            exp += float(getattr(cmdr, "post_council_exposure_val", 0.0))
        return exp

    def to_dict(self):
        return {sid: dict(v) for sid, v in self.state.items()}

    @classmethod
    def from_dict(cls, d):
        return cls(d)


# ==========================================
# 🎯 大一统像素容器伤害结算管线（无差别误伤 + 破片落点距离表 + 步坦协同 + Depot 殉爆）
# ==========================================
HIT_POINTS = 40.0


def distribute_squads(squads, amount, key=""):
    """把火力集中落在若干具体班组上（确定性 sha1 选班）。"""
    live = [s for s in squads if s.persons > 0 or s.equipment]
    if not live or amount <= 0:
        return []
    k = min(len(live), max(1, int(math.ceil(amount / HIT_POINTS))))
    order = sorted(live, key=lambda s: hashlib.sha1(f"{key}|{s.id}".encode("utf-8")).hexdigest())
    return [(s, amount / k) for s in order[:k]]


def _det_gauss_2d(seed_key, sigma_m):
    """确定性二维正态分布落点偏移采样（米）：同局面同参数 100% 可复现。"""
    s = max(0.0, float(sigma_m))
    if s <= 1e-9:
        return 0.0, 0.0
    digest = hashlib.sha256(seed_key.encode("utf-8")).digest()
    u1 = max(1e-9, min(1.0 - 1e-9, int.from_bytes(digest[:8], "big") / float(1 << 64)))
    u2 = int.from_bytes(digest[8:16], "big") / float(1 << 64)
    r = s * math.sqrt(-2.0 * math.log(u1))
    ang = 2.0 * math.pi * u2
    return r * math.cos(ang), r * math.sin(ang)


def _piece_tank_count(piece):
    c = 1 if getattr(piece, "vehicle_model", None) in ("tank_medium", "tank_heavy") else 0
    for sq in piece.squads():
        c += sq.equipment.get("tank_medium", 0) + sq.equipment.get("tank_heavy", 0)
    return c


def _piece_has_vehicle(piece):
    if getattr(piece, "vehicle_model", None):
        return True
    for sq in piece.squads():
        for m, n in sq.equipment.items():
            if n > 0 and m in toe.VEHICLE_SPECS:
                return True
    return False


def _piece_infantry_count(piece):
    if getattr(piece, "vehicle_model", None) in ("tank_medium", "tank_heavy", "tractor", "truck"):
        return 0
    total = 0
    for sq in piece.squads():
        if sq.kind in ("rifle_squad", "mg_squad", "at_squad", "recon_squad", "engineer_squad", "mech_squad", "porter_squad"):
            total += sq.persons
        else:
            tags = sq.arm_tags()
            has_small_arms = any(sq.equipment.get(m, 0) > 0 for m in ("rifle", "lmg", "hmg"))
            if ("infantry" in tags or has_small_arms) and "armor" not in tags and sq.kind not in ("tank_squad", "heavy_tank_squad", "vehicle_crew"):
                total += sq.persons
    return total


def compute_directional_cover_eff(piece, attacker_pos, world=None):
    """V3.1 方向命中投影有效掩体系数：cover_eff_i = max(0, cos theta_i)，返回 (cover_eff, cos_th)。
    - 载具与已部署重武器（具象车体朝向）：直接采用 piece.facing_deg；
    - 步兵（无物理车体朝向）：优先读取 meta['tactical_face_vec']（即 V3.1 的 d_face），
      否则回退 meta['autonomy_state']['threat_dir'] 或实时 compute_threat_vector。
      若尚无威胁/战术朝向向量，则视为全方位警戒（返回 1.0, 1.0）。
    """
    if piece is None or attacker_pos is None:
        return 1.0, 1.0
    if autonomy_v3.is_directional_hull_unit(piece):
        cos_th = float(toe.facing_cos(attacker_pos, piece.pos, getattr(piece, "facing_deg", 0.0)))
        cos_th = max(-1.0, min(1.0, cos_th))
        return max(0.0, cos_th), cos_th

    meta = getattr(piece, "meta", None) or {}
    fvec = meta.get("tactical_face_vec")
    if not (isinstance(fvec, (list, tuple)) and len(fvec) == 2 and math.hypot(float(fvec[0]), float(fvec[1])) > 1e-6):
        st = meta.get("autonomy_state")
        if isinstance(st, dict) and isinstance(st.get("threat_dir"), (list, tuple)) and len(st["threat_dir"]) == 2:
            fvec = st["threat_dir"]
        elif world is not None:
            st = autonomy_v3.compute_threat_vector(world, piece)
            fvec = st.get("threat_dir")
    if not (isinstance(fvec, (list, tuple)) and len(fvec) == 2):
        return 1.0, 1.0
    fr, fc = float(fvec[0]), float(fvec[1])
    fn = math.hypot(fr, fc)
    if fn <= 1e-6:
        return 1.0, 1.0
    fr, fc = fr / fn, fc / fn
    dr = float(attacker_pos[0]) - float(piece.pos[0])
    dc = float(attacker_pos[1]) - float(piece.pos[1])
    dn = math.hypot(dr, dc)
    if dn <= 1e-9:
        return 1.0, 1.0
    cos_th = max(-1.0, min(1.0, (dr / dn) * fr + (dc / dn) * fc))
    return max(0.0, cos_th), cos_th


def _apply_damage_to_piece(piece, dtype, pts, cover, fort, key, rec_bucket, org_shock_mult=1.0):
    if pts <= 0.0:
        return 0, 0
    squads = piece.squads()
    if not squads:
        return 0, 0
    rec = rec_bucket.setdefault(piece.id, {"enlisted": 0, "cadre": 0, "equipment": {}, "hits": 0})
    rec["hits"] += 1
    le, lc = 0, 0
    for sq, amount in distribute_squads(squads, pts, key):
        if amount <= 0.0:
            continue
        r = strike(sq, dtype, amount, cover=cover, fortification=fort, org_shock_mult=org_shock_mult)
        le += r["enlisted_lost"]
        lc += r["cadre_lost"]
        rec["enlisted"] += r["enlisted_lost"]
        rec["cadre"] += r["cadre_lost"]
        for m, n in r["equipment_lost"].items():
            rec["equipment"][m] = rec["equipment"].get(m, 0) + n
    return le, lc


def resolve_cell_strike(
    world,
    cell_rc,
    attacker_team,
    fire_dict,
    dist_m=200.0,
    weapon_model=None,
    acc_bonus=0.0,
    fac_guided=False,
    aspect_target_pos=None,
    seed_tag="",
    casualties_out=None,
):
    """大一统像素容器伤害结算：所有武器打在像素容器上，再由容器向容器内单位（不分敌我）与 Depot 结算。"""
    g = world.grid
    rc = (int(round(cell_rc[0])), int(round(cell_rc[1])))
    if casualties_out is None:
        casualties_out = {}

    # 1. 收集像素容器内全部存活单位；遇敌自动跳车解除 Tank Desant 挂载
    cell_pieces = [
        p for p in world.pieces.values()
        if p.px == rc and (p.persons() > 0 or getattr(p, "vehicle_model", None))
    ]
    dismounted = []
    seen_ids = {p.id for p in cell_pieces}
    for p in list(cell_pieces):
        if p.attached_to and p.attached_to in world.pieces:
            par = world.pieces[p.attached_to]
            if _piece_tank_count(par) > 0 and p.mobility() == "foot" and not p.deployed_weapon:
                p.detach("mobile")
                p.pos = tuple(par.pos)
                dismounted.append(p.id)
        for ch in list(world.pieces.values()):
            if ch.attached_to == p.id and _piece_tank_count(p) > 0 and ch.mobility() == "foot" and not ch.deployed_weapon:
                ch.detach("mobile")
                ch.pos = tuple(p.pos)
                dismounted.append(ch.id)
                if ch.id not in seen_ids and (ch.persons() > 0 or getattr(ch, "vehicle_model", None)):
                    cell_pieces.append(ch)
                    seen_ids.add(ch.id)

    terrain_code = int(g.terrain[rc]) if (0 <= rc[0] < g.shape[0] and 0 <= rc[1] < g.shape[1]) else tf.PLAIN
    cell_mods = g.get_cell_modifiers(rc)
    primary_fid = tf.TERRAIN_CODE_TO_FEATURE.get(terrain_code)
    raw_base_cov = 0.0 if (primary_fid and primary_fid in cell_mods.get("features", ())) else tf.COVER.get(terrain_code, 0.0)
    base_cover = raw_base_cov + cell_mods["cover_bonus"]
    cell_fort = cell_mods["fortification_bonus"]

    # 2. 步坦协同统计（按阵营统计坦克数量与伴随步兵掩护）
    tanks_by_team = {}
    inf_by_team = {}
    tank_pieces_by_team = {}
    for p in cell_pieces:
        tc = _piece_tank_count(p)
        ic = _piece_infantry_count(p)
        tanks_by_team[p.team] = tanks_by_team.get(p.team, 0) + tc
        inf_by_team[p.team] = inf_by_team.get(p.team, 0) + ic
        if tc > 0:
            tank_pieces_by_team.setdefault(p.team, []).append(p)
    has_screen = {
        t: (tanks_by_team.get(t, 0) == 0) or (inf_by_team.get(t, 0) >= 4 * tanks_by_team.get(t, 0))
        for t in set(tanks_by_team) | set(inf_by_team)
    }

    enemies = sorted([p for p in cell_pieces if p.team != attacker_team], key=lambda x: x.id)
    friendlies = sorted([p for p in cell_pieces if p.team == attacker_team], key=lambda x: x.id)

    def _effective_cover_and_fort(p):
        cov = base_cover
        if getattr(p, "moved_last_turn", False):
            cov *= 0.35
        if terrain_code == tf.PLAIN and tanks_by_team.get(p.team, 0) > 0 and _piece_tank_count(p) == 0:
            cov = min(0.85, cov + 0.30)
        directional = autonomy_v3.is_directional_hull_unit(p)
        if not directional:
            st = (getattr(p, "meta", {}) or {}).get("autonomy_state")
            if not isinstance(st, dict) or "c" not in st:
                st = autonomy_v3.compute_threat_vector(world, p)
            crossfire_cov = autonomy_v3.infantry_crossfire_cover_mult(st.get("S", 0.0), st.get("c", 1.0))
            if aspect_target_pos is not None:
                cover_eff, _ = compute_directional_cover_eff(p, aspect_target_pos, world=world)
                cov *= cover_eff * crossfire_cov
            else:
                cov *= crossfire_cov
        elif aspect_target_pos is not None:
            cover_eff, _ = compute_directional_cover_eff(p, aspect_target_pos, world=world)
            cov *= cover_eff
        return cov, float(p.fortification) + cell_fort

    def _aspect_mult(p):
        directional = autonomy_v3.is_directional_hull_unit(p)
        aspect_factor = 1.0
        if aspect_target_pos is not None:
            _, cos_th = compute_directional_cover_eff(p, aspect_target_pos, world=world)
            if cos_th < 0.7071:
                aspect_factor = 1.0 + 1.2 * ((0.7071 - cos_th) / 1.7071)
        if not directional:
            st = (getattr(p, "meta", {}) or {}).get("autonomy_state")
            if not isinstance(st, dict) or "c" not in st:
                st = autonomy_v3.compute_threat_vector(world, p)
            cf_mult = autonomy_v3.crossfire_casualty_mult(st.get("S", 0.0), st.get("c", 1.0))
            return max(aspect_factor, cf_mult)
        return aspect_factor

    def _org_shock_mult(p):
        if aspect_target_pos is None:
            return 1.0
        _, cos_th = compute_directional_cover_eff(p, aspect_target_pos, world=world)
        if cos_th <= 0.0:
            # 侧翼 (cos<=0 -> 1.5x) 至正后方 (cos=-1 -> 2.0x) 受击追加组织度震荡惩罚
            return 1.5 + 0.5 * min(1.0, -cos_th)
        return 1.0

    def _deliver_hit(p, dtype, pts, tag_suffix):
        if pts <= 0.0:
            return
        if getattr(world, "event_manager", None) is not None and p.team != attacker_team:
            world.event_manager.notify_unit_attacked(world, attacked_piece_id=p.id, attacker_team=attacker_team, cell_rc=rc)
        cov, fort = _effective_cover_and_fort(p)
        cross_mult = _aspect_mult(p) if (dtype in ("ap_kinetic", "heat_shaped") or not autonomy_v3.is_directional_hull_unit(p)) else 1.0
        shock_mult = _org_shock_mult(p)
        eff_pts = pts * cross_mult
        # 步坦协同 1：开阔地步兵受坦克车体掩护，35% 轻武器火力偏转至坦克装甲吸收
        if (
            dtype == "small_arms"
            and terrain_code == tf.PLAIN
            and tanks_by_team.get(p.team, 0) > 0
            and _piece_tank_count(p) == 0
        ):
            deflected = eff_pts * 0.35
            eff_pts *= 0.65
            shield_tank = tank_pieces_by_team[p.team][0]
            _apply_damage_to_piece(
                shield_tank, "small_arms", deflected, cov, fort,
                f"{seed_tag}|{tag_suffix}|shield|{shield_tank.id}", casualties_out,
                org_shock_mult=1.0,
            )
        # 步坦协同 2：城市/森林/近战伏击地形 200m 内无步兵护卫的裸奔坦克遭遇反坦克铁拳/燃烧瓶暴击惩罚
        if (
            dtype == "small_arms"
            and p.team != attacker_team
            and (terrain_code in (tf.URBAN, tf.FOREST) or cell_mods.get("cqb_ambush", False))
            and float(dist_m) <= 200.0
            and _piece_tank_count(p) > 0
            and not has_screen.get(p.team, True)
        ):
            crit_heat = eff_pts * 0.35 * 2.2
            eff_pts *= 0.65
            _apply_damage_to_piece(
                p, "heat_shaped", crit_heat, cov * 0.5, fort * 0.5,
                f"{seed_tag}|{tag_suffix}|cqb_at_crit|{p.id}", casualties_out,
                org_shock_mult=shock_mult,
            )
        _apply_damage_to_piece(
            p, dtype, eff_pts, cov, fort, f"{seed_tag}|{tag_suffix}|{p.id}", casualties_out,
            org_shock_mult=shock_mult,
        )

    # 3A. 直瞄/点杀伤类火力（步枪/机枪对友军误伤极小铁律）
    point_types = ("small_arms", "ap_kinetic", "heat_shaped", "suppression")
    for dtype in point_types:
        pts = float(fire_dict.get(dtype, 0.0))
        if pts <= 0.0:
            continue
        if enemies and friendlies:
            if dtype in ("small_arms", "ap_kinetic"):
                ff_ratio = 0.0 if acc_bonus >= 0.15 else 0.02
            elif dtype == "heat_shaped":
                ff_ratio = 0.02 if acc_bonus >= 0.15 else 0.05
            else:
                ff_ratio = 0.05
            enemy_pts = pts * (1.0 - ff_ratio)
            friendly_pts = pts * ff_ratio
            for ep in enemies:
                _deliver_hit(ep, dtype, enemy_pts / len(enemies), dtype)
            if friendly_pts > 1e-9:
                for fp in friendlies:
                    _deliver_hit(fp, dtype, friendly_pts / len(friendlies), f"{dtype}_ff")
        elif enemies:
            for ep in enemies:
                _deliver_hit(ep, dtype, pts / len(enemies), dtype)
        elif friendlies:
            # 误向纯友军格开火
            for fp in friendlies:
                _deliver_hit(fp, dtype, (pts * 0.15) / len(friendlies), f"{dtype}_stray_ff")

    # 3B. 火炮与高爆面杀伤（抽落点 -> 拉单位距离表 -> 查破片杀伤半径结算伤害）
    area_types = ("he_blast", "incendiary")
    he_total = sum(float(fire_dict.get(dt_, 0.0)) for dt_ in area_types)
    hit_pos = None
    sigma_m = 0.0
    frag_radius_m = 0.0
    dist_table = {}

    if he_total > 0.0 and cell_pieces:
        if getattr(world, "event_manager", None) is not None:
            for ep in enemies:
                world.event_manager.notify_unit_attacked(world, attacked_piece_id=ep.id, attacker_team=attacker_team, cell_rc=rc)
            world.event_manager.notify_unit_attacked(world, attacked_piece_id=None, attacker_team=attacker_team, cell_rc=rc)
        frag_radius_m = toe.weapon_frag_radius_m(weapon_model) if weapon_model else 0.0
        if frag_radius_m <= 0.0:
            frag_radius_m = 25.0 if he_total < 25.0 else (55.0 if he_total < 60.0 else 85.0)
        sigma_m = max(6.0, 12.0 + 0.025 * max(0.0, float(dist_m))) / (
            (1.0 + max(0.0, float(acc_bonus))) * (1.6 if fac_guided else 1.0)
        )
        hx, hy = _det_gauss_2d(f"{seed_tag}|{rc}|he", sigma_m)
        hit_pos = (round(hx, 2), round(hy, 2))

        local_coords = {}
        for idx, ep in enumerate(enemies):
            if idx == 0:
                local_coords[ep.id] = (0.0, 0.0)
            else:
                ang = (idx * 2.39996) % (2.0 * math.pi)
                rad = min(15.0, 5.0 * idx)
                local_coords[ep.id] = (rad * math.cos(ang), rad * math.sin(ang))
        for idx, fp in enumerate(friendlies):
            if enemies:
                # CQB 同格近战/贴脸交火：友军距离敌军中心约 18~25 米，落入迫击炮与大口径杀伤破片危及区
                ang = (idx * 2.09439) % (2.0 * math.pi)
                rad = 0.0 if idx == 0 else 6.0
                local_coords[fp.id] = (18.0 + rad * math.cos(ang), rad * math.sin(ang))
            else:
                ang = (idx * 2.39996) % (2.0 * math.pi)
                rad = min(15.0, 5.0 * idx)
                local_coords[fp.id] = (rad * math.cos(ang), rad * math.sin(ang))

        for p in cell_pieces:
            px_m, py_m = local_coords[p.id]
            r_p = math.hypot(hx - px_m, hy - py_m)
            dist_table[p.id] = round(r_p, 2)
            if r_p >= frag_radius_m:
                continue
            w_p = (1.0 - (r_p / frag_radius_m) ** 1.5) * (1.35 if fac_guided else 1.0)
            for dt_ in area_types:
                pts = float(fire_dict.get(dt_, 0.0))
                if pts > 0.0:
                    _deliver_hit(p, dt_, pts * w_p, f"{dt_}_frag")

    # 4. Depot 硬毁伤与高爆殉爆
    depot_damage = []
    if he_total > 10.0 and getattr(world, "depots", None):
        frac = min(0.60, max(0.15, (he_total - 10.0) / 150.0))
        for dep in world.depots.values():
            if tuple(dep.px) != rc:
                continue
            ammo_lost = float(dep.stock.get("ammo", 0.0)) * frac
            fuel_lost = float(dep.stock.get("fuel", 0.0)) * frac
            food_lost = float(dep.stock.get("food", 0.0)) * frac
            dep.stock["ammo"] = max(0.0, dep.stock.get("ammo", 0.0) - ammo_lost)
            dep.stock["fuel"] = max(0.0, dep.stock.get("fuel", 0.0) - fuel_lost)
            dep.stock["food"] = max(0.0, dep.stock.get("food", 0.0) - food_lost)
            for cal in list(dep.ammo_by_type):
                dep.ammo_by_type[cal] = max(0.0, dep.ammo_by_type[cal] * (1.0 - frac))
            cookoff = ammo_lost > 1.0
            if cookoff:
                g.add_exposure(rc, 15.0, radius_px=1.0)
            depot_damage.append({
                "depot": dep.id,
                "frac": round(frac, 3),
                "ammo_lost": round(ammo_lost, 3),
                "fuel_lost": round(fuel_lost, 3),
                "food_lost": round(food_lost, 3),
                "cookoff": cookoff,
            })

    return {
        "cell": list(rc),
        "hit_pos": hit_pos,
        "sigma_m": round(sigma_m, 2),
        "frag_radius_m": round(frag_radius_m, 1),
        "dist_table": dist_table,
        "dismounted": dismounted,
        "depot_damage": depot_damage,
    }
