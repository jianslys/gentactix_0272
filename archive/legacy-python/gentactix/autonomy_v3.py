# -*- coding: utf-8 -*-
"""V3.1 空间态势矢量内核、格网空间桶索引与 5 分钟纯数学状态器（零 LLM，战术完全涌现）。

核心原则
--------
1. **格网空间桶索引 ($O(1)$ 邻域查询)**：严禁 $O(N^2)$ 全图遍历，每个 5 分钟步长 $O(N)$ 重建一次 `SpatialBucketGrid`。
2. **AO + 分兵种战线势场 + 9×9 地形容器扫描**：读取 `Grid` 像素容器中的 AO 边界、分兵种战线（按 `affinity` 加权）、高程反斜面遮蔽、掩体 `cover` 与短距同兵种斥力 $\\vec{F}_{\\text{repel}}$。
3. **纯涌现统一矢量与五项原子动作打分**：废除硬编码交替掩护/撤退分支，统一计算 $\\vec{V}_{\\text{move}}$、断连瞬间 $k_C$ 漂移（`RED_EXCLAMATION` / `YELLOW_EXCLAMATION`），并通过硬锁过滤 + 滞回 $+H_{\\text{stick}}$ 对 `HOLD / MOVE / ATTACK / PICKUP / EMBARK` 打分（重武器先判 `PACK/DEPLOY`）。
4. **载体区分（步兵不带物理车体转向）**：载具/重武器维护物理 `facing_deg`（`forbid_move` 锁死）；普通步兵保持 `piece.facing_deg` 不变，仅在数学状态器内部生成战术迎敌向量 $\\vec{d}_{\\text{face}}$ 供受击几何投影使用。
"""

from __future__ import annotations

import hashlib
import math

from . import command_v2
from . import terrain_field as tf
from . import toe

BASE_SPEED_KMH = {"foot": 4.5, "wheeled": 28.0, "tracked": 18.0}
DEFAULT_SUBTICK_HOURS = 5.0 / 60.0

DOCTRINE_WEIGHTS = {
    "assault": 1.30,
    "attack": 1.15,
    "advance": 1.00,
    "recon": 0.90,
    "screen": 0.85,
    "defend": 0.75,
    "hold": 0.65,
    "withdraw": 1.10,
}

# V3.1 数学状态器常量
KC_BASE = 1.20
DELTA_KC = 0.55
KT_BASE = 1.30
KF_BASE = 0.90
K_TERRAIN_0 = 0.055
H_STICK = 0.18
H_DEPLOYED = 1.35
K_STEALTH_COMPROMISE = 1.65
THREAT_DECAY_KM = 0.60
D_MARCH_KM = 2.0


# ==========================================
# 🗂️ O(1) 格网空间桶索引（禁止 O(N^2) 遍历）
# ==========================================
class SpatialBucketGrid:
    """按 400m（默认 2 px）分桶的空间索引，每 5 分钟 Sub-tick 线性重建。"""

    def __init__(self, cell_m=200.0, bucket_px=2):
        self.cell_m = float(cell_m)
        self.bucket_px = max(1, int(bucket_px))
        self.buckets = {}  # (br, bc) -> list[piece]
        self.all_pieces = []

    def insert(self, piece):
        br = int(math.floor(float(piece.pos[0]))) // self.bucket_px
        bc = int(math.floor(float(piece.pos[1]))) // self.bucket_px
        self.buckets.setdefault((br, bc), []).append(piece)
        self.all_pieces.append(piece)

    def query_radius_px(self, pos, radius_px, team=None, exclude_team=None, exclude_id=None):
        pr, pc = float(pos[0]), float(pos[1])
        r_px = max(0.0, float(radius_px))
        br0 = int(math.floor(pr - r_px)) // self.bucket_px
        br1 = int(math.floor(pr + r_px)) // self.bucket_px
        bc0 = int(math.floor(pc - r_px)) // self.bucket_px
        bc1 = int(math.floor(pc + r_px)) // self.bucket_px
        r2 = r_px * r_px + 1e-6
        hits = []
        for br in range(br0, br1 + 1):
            for bc in range(bc0, bc1 + 1):
                for p in self.buckets.get((br, bc), ()):
                    if exclude_id is not None and p.id == exclude_id:
                        continue
                    if team is not None and p.team != team:
                        continue
                    if exclude_team is not None and p.team == exclude_team:
                        continue
                    dr = float(p.pos[0]) - pr
                    dc = float(p.pos[1]) - pc
                    d2 = dr * dr + dc * dc
                    if d2 <= r2:
                        hits.append((p, math.sqrt(d2)))
        return hits

    def query_radius_km(self, pos, radius_km, team=None, exclude_team=None, exclude_id=None):
        radius_px = (float(radius_km) * 1000.0) / max(1.0, self.cell_m)
        km_per_px = self.cell_m / 1000.0
        return [
            (p, d_px * km_per_px)
            for p, d_px in self.query_radius_px(
                pos, radius_px, team=team, exclude_team=exclude_team, exclude_id=exclude_id
            )
        ]


def _world_piece_sig(world):
    if world is None:
        return (0, 0.0)
    n = 0
    s = 0.0
    for p in world.pieces.values():
        if p.persons() > 0 or getattr(p, "vehicle_model", None) or getattr(p, "deployed_weapon", None):
            n += 1
            s += float(p.pos[0]) * 1.37 + float(p.pos[1]) * 3.19
    return (n, round(s, 3))


def build_spatial_index(world, bucket_px=2):
    """每 5 分钟 O(N) 构建格网空间桶索引并挂载在 world._spatial_index。"""
    g = getattr(world, "grid", None)
    cell_m = g.cell_m if g is not None else 200.0
    idx = SpatialBucketGrid(cell_m=cell_m, bucket_px=bucket_px)
    if world is not None:
        for p in world.pieces.values():
            if p.persons() > 0 or getattr(p, "vehicle_model", None) or getattr(p, "deployed_weapon", None):
                idx.insert(p)
        idx._sig = _world_piece_sig(world)
        world._spatial_index = idx
    return idx


def _resolve_spatial_index(world, spatial_index=None):
    if spatial_index is not None:
        return spatial_index
    if world is None:
        return None
    cached = getattr(world, "_spatial_index", None)
    if cached is not None and getattr(cached, "_sig", None) == _world_piece_sig(world):
        return cached
    return build_spatial_index(world)


# ==========================================
# 🧩 兵种分类与物理载体判定
# ==========================================
def is_directional_hull_unit(piece):
    """判定单位是否携带物理车体/炮盾朝向 `facing_deg`。
    普通步兵一律返回 False（步兵不带物理转向，仅在状态器内生成矢量朝向）。
    """
    if piece is None:
        return False
    if getattr(piece, "vehicle_model", None):
        return True
    if getattr(piece, "is_heavy_emplacement", False) or getattr(piece, "deployed_weapon", None):
        return True
    for sq in piece.squads():
        for m, n in (sq.equipment or {}).items():
            if int(n) <= 0:
                continue
            spec = toe.EQUIPMENT.get(m, {})
            if spec.get("armor") == "armored":
                return True
            if spec.get("mobility") in ("wheeled", "tracked"):
                return True
            if spec.get("deployable"):
                return True
    return False


def classify_piece_unit_mask(piece):
    """解析棋子的分兵种战线位掩码，用于读取地形容器中的 TypedFrontline 亲和引力。"""
    if piece is None:
        return tf.LINE_MASK_INFANTRY
    mask = 0
    vm = getattr(piece, "vehicle_model", None)
    if vm in ("tank_medium", "tank_heavy", "halftrack"):
        mask |= tf.LINE_MASK_ARMOR
    elif vm in ("truck", "tractor"):
        mask |= tf.LINE_MASK_ARTY_LOG

    if getattr(piece, "is_heavy_emplacement", False) or getattr(piece, "deployed_weapon", None):
        dw = getattr(piece, "deployed_weapon", "") or ""
        if dw in ("howitzer_105", "howitzer_152"):
            mask |= tf.LINE_MASK_ARTY_LOG | tf.LINE_MASK_HEAVY_AT
        else:
            mask |= tf.LINE_MASK_HEAVY_AT

    for sq in piece.squads():
        k = getattr(sq, "kind", "")
        if k in ("recon_squad", "fac_squad"):
            mask |= tf.LINE_MASK_RECON
        elif k in ("tank_squad", "heavy_tank_squad", "mech_squad"):
            mask |= tf.LINE_MASK_ARMOR
        elif k in ("mortar_squad", "at_squad", "mg_squad", "aa_squad"):
            mask |= tf.LINE_MASK_HEAVY_AT
        elif k in ("gun_squad", "heavy_gun_squad"):
            mask |= tf.LINE_MASK_ARTY_LOG | tf.LINE_MASK_HEAVY_AT
        elif k in ("transport_squad", "porter_squad", "hq_squad"):
            mask |= tf.LINE_MASK_ARTY_LOG
        elif k in ("rifle_squad", "engineer_squad"):
            mask |= tf.LINE_MASK_INFANTRY

        for m, n in (sq.equipment or {}).items():
            if int(n) <= 0:
                continue
            spec = toe.EQUIPMENT.get(m, {})
            if m in ("tank_medium", "tank_heavy", "halftrack"):
                mask |= tf.LINE_MASK_ARMOR
            elif m in ("howitzer_105", "howitzer_152"):
                mask |= tf.LINE_MASK_ARTY_LOG
            elif spec.get("deployable"):
                mask |= tf.LINE_MASK_HEAVY_AT
            elif m in ("truck", "tractor"):
                mask |= tf.LINE_MASK_ARTY_LOG

    return mask if mask != 0 else tf.LINE_MASK_INFANTRY


def _piece_strength_estimate(piece):
    p = max(0, piece.persons())
    veh = 1.0 if getattr(piece, "vehicle_model", None) else 0.0
    heavy = sum(
        int(n)
        for sq in piece.squads()
        for m, n in (sq.equipment or {}).items()
        if int(n) > 0 and toe.EQUIPMENT.get(m, {}).get("weight") == "heavy"
    )
    med = sum(
        int(n)
        for sq in piece.squads()
        for m, n in (sq.equipment or {}).items()
        if int(n) > 0 and toe.EQUIPMENT.get(m, {}).get("weight") == "medium"
    )
    return float(p) * 1.0 + veh * 18.0 + heavy * 12.0 + med * 4.0


def _piece_primary_weapon_range_m(piece):
    best_r = 400.0
    best_score = -1.0
    if getattr(piece, "deployed_weapon", None) and piece.deployed_weapon in toe.EQUIPMENT:
        return float(toe.EQUIPMENT[piece.deployed_weapon].get("range_m", 800.0))
    for sq in piece.squads():
        for m, n in (sq.equipment or {}).items():
            if int(n) <= 0 or m not in toe.EQUIPMENT:
                continue
            spec = toe.EQUIPMENT[m]
            f_sum = sum((spec.get("fire") or {}).values()) * int(n)
            if f_sum > best_score and spec.get("range_m", 0) > 0:
                best_score = f_sum
                best_r = float(spec["range_m"])
    return max(200.0, best_r)


# ==========================================
# 🎯 威胁矢量 T、压力比 R 与友军支撑 F
# ==========================================
def gather_unit_known_enemies(world, piece, extra_enemy_contacts=None, sight_km=0.4, spatial_index=None):
    """汇总单位当前全部已知敌情（自身 400m 目视 + 断连友军汲取 + 共享侦察），O(1) 桶查询。"""
    contacts = {}
    g = getattr(world, "grid", None) if world is not None else None
    night = bool(getattr(world, "night", False)) if world is not None else False
    if world is not None and g is not None:
        sidx = _resolve_spatial_index(world, spatial_index)
        cand = (
            sidx.query_radius_km(piece.pos, sight_km, exclude_team=piece.team, exclude_id=piece.id)
            if sidx is not None
            else [
                (ep, g.distance_km(piece.pos, ep.pos))
                for ep in world.pieces.values()
                if ep.team != piece.team and (ep.persons() > 0 or getattr(ep, "vehicle_model", None) or getattr(ep, "deployed_weapon", None))
            ]
        )
        for ep, d_km in cand:
            if d_km <= sight_km + 1e-6:
                d_px = math.hypot(piece.px[0] - ep.px[0], piece.px[1] - ep.px[1])
                vis = tf.line_visibility(g, piece.px, ep.px, night=night) if d_px > 1.2 else 1.0
                if vis > 0.05:
                    contacts[ep.id] = {
                        "id": ep.id,
                        "pos": [float(ep.pos[0]), float(ep.pos[1])],
                        "px": list(ep.px),
                        "strength": _piece_strength_estimate(ep),
                        "engaging": bool(getattr(ep, "roe", "return_fire") != "hold_fire" and d_km <= 1.2),
                        "source": "visual_400m",
                    }

    # 断连友军 400m 汲取的局部敌情
    local_intel = (piece.meta or {}).get("local_shared_intel") or {}
    for eid, rec in (local_intel.get("enemies") or {}).items():
        if eid not in contacts and isinstance(rec, dict) and "pos" in rec:
            contacts[eid] = dict(rec)

    # 未断连时消费共享侦察情报
    if world is not None and not (piece.meta or {}).get("is_disconnected"):
        team_intel = (getattr(world, "meta", {}) or {}).get("shared_intel", {}).get(piece.team, {})
        for eid, rec in (team_intel or {}).items():
            if eid not in contacts and isinstance(rec, dict) and "pos" in rec:
                contacts[eid] = dict(rec)

    for idx, ec in enumerate(extra_enemy_contacts or ()):
        if isinstance(ec, dict) and "pos" in ec:
            cid = str(ec.get("id", f"ext-{idx}"))
            contacts[cid] = {
                "id": cid,
                "pos": [float(ec["pos"][0]), float(ec["pos"][1])],
                "strength": float(ec.get("strength", 10.0)),
                "engaging": bool(ec.get("engaging", True)),
                "source": ec.get("source", "explicit"),
            }
    return list(contacts.values())


def compute_threat_vector(world, piece, extra_enemy_contacts=None, sight_km=0.4, spatial_index=None):
    """计算指数衰减威胁合矢量 T、总威胁 S 与合围集中度 c = |T| / max(S, eps) ∈ [0, 1]：
    w_i = P_{enemy, i} * exp(-d_i / d_0) * gamma_{firing, i}
    """
    contacts = gather_unit_known_enemies(
        world, piece, extra_enemy_contacts=extra_enemy_contacts, sight_km=sight_km, spatial_index=spatial_index
    )
    g = getattr(world, "grid", None) if world is not None else None
    px_km = (g.cell_m / 1000.0) if g is not None else 0.2
    pr, pc = float(piece.pos[0]), float(piece.pos[1])

    tr, tc = 0.0, 0.0
    total_s = 0.0
    nearest_enemy = None
    nearest_d_km = math.inf
    for ct in contacts:
        er, ec = float(ct["pos"][0]), float(ct["pos"][1])
        dr, dc = er - pr, ec - pc
        d_px = math.hypot(dr, dc)
        d_km = d_px * px_km
        if d_km < nearest_d_km:
            nearest_d_km = d_km
            nearest_enemy = ct
        ur, uc = (dr / d_px, dc / d_px) if d_px > 1e-9 else (1.0, 0.0)
        strength = max(1.0, float(ct.get("strength", 10.0)))
        gamma = 1.6 if ct.get("engaging") else 1.0
        w_i = strength * math.exp(-d_km / THREAT_DECAY_KM) * gamma
        tr += w_i * ur
        tc += w_i * uc
        total_s += w_i

    t_mag = math.hypot(tr, tc)
    if total_s > 1e-9:
        c_val = min(1.0, max(0.0, t_mag / max(total_s, 1e-9)))
    else:
        c_val = 1.0

    if t_mag > 1e-9:
        utr, utc = tr / t_mag, tc / t_mag
        t_deg = math.degrees(math.atan2(tr, tc)) % 360.0
    else:
        utr, utc = 0.0, 0.0
        t_deg = float(getattr(piece, "facing_deg", 0.0)) % 360.0

    is_crossfire = (total_s >= 10.0 and c_val <= 0.45) or (len(contacts) >= 2 and c_val <= 0.40)
    return {
        "T": [round(tr, 4), round(tc, 4)],
        "T_mag": round(t_mag, 4),
        "S": round(total_s, 4),
        "c": round(c_val, 4),
        "threat_dir": [round(utr, 4), round(utc, 4)],
        "threat_bearing_deg": round(t_deg, 2),
        "is_crossfire": bool(is_crossfire),
        "contact_count": len(contacts),
        "nearest_d_km": round(nearest_d_km, 4) if math.isfinite(nearest_d_km) else None,
        "nearest_enemy": nearest_enemy,
        "contacts": contacts,
    }


def infantry_crossfire_cover_mult(S, c):
    """步兵无物理车体转向时的合围交叉火力掩体衰减系数：c→0 且 S 大时最低跌至 0.25。"""
    s = float(S)
    c_val = max(0.0, min(1.0, float(c)))
    if s <= 1e-6 or c_val >= 0.70:
        return 1.0
    s_scale = min(1.0, s / 20.0)
    penalty = 0.75 * ((0.70 - c_val) / 0.70) * s_scale
    return max(0.25, 1.0 - penalty)


def crossfire_casualty_mult(S, c):
    """交叉火力下的额外伤亡与震慑乘数（最高 1.45x）。"""
    s = float(S)
    c_val = max(0.0, min(1.0, float(c)))
    if s <= 1e-6 or c_val >= 0.65:
        return 1.0
    s_scale = min(1.0, s / 20.0)
    return 1.0 + 0.45 * ((0.65 - c_val) / 0.65) * s_scale


def compute_friendly_vector(world, piece, radius_km=1.2, spatial_index=None):
    """利用空间桶计算友军态势矢量 F 与局部友军支援力 F_support。"""
    if world is None:
        return {
            "F": [0.0, 0.0], "F_mag": 0.0, "F_dir": [0.0, 0.0],
            "F_support": 0.0, "count": 0, "anchor_count": 0, "retreat_count": 0,
        }
    g = getattr(world, "grid", None)
    px_km = (g.cell_m / 1000.0) if g is not None else 0.2
    pr, pc = float(piece.pos[0]), float(piece.pos[1])
    sidx = _resolve_spatial_index(world, spatial_index)
    if sidx is not None:
        allies = sidx.query_radius_km(piece.pos, radius_km, team=piece.team, exclude_id=piece.id)
    else:
        allies = [
            (fp, math.hypot(float(fp.pos[0]) - pr, float(fp.pos[1]) - pc) * px_km)
            for fp in world.pieces.values()
            if fp.id != piece.id and fp.team == piece.team and fp.persons() > 0
        ]

    fr, fc = 0.0, 0.0
    count = 0
    anchor_count = 0
    retreat_count = 0
    f_support = 0.0
    for fp, d_km in allies:
        if d_km > radius_km + 1e-6:
            continue
        if fp.persons() <= 0:
            continue
        count += 1
        ast = (fp.meta or {}).get("autonomy_state") or {}
        st = ast.get("action", "hold_position")
        atomic = ast.get("atomic_action", "HOLD")
        org = fp.mean_org()
        f_pow = _piece_strength_estimate(fp)
        f_support += f_pow * math.exp(-d_km / 0.60)
        if st in ("alternating_withdraw", "withdraw") or (atomic == "MOVE" and org < 25.0) or org < 25.0:
            retreat_count += 1
            stance_w = -0.35
        else:
            anchor_count += 1
            stance_w = 1.0
        dr, dc = float(fp.pos[0]) - pr, float(fp.pos[1]) - pc
        d_px = math.hypot(dr, dc)
        ur, uc = (dr / d_px, dc / d_px) if d_px > 1e-9 else (0.0, 0.0)
        w = (f_pow / (d_km + 0.25)) * stance_w
        fr += w * ur
        fc += w * uc

    f_mag = math.hypot(fr, fc)
    ufr, ufc = (fr / f_mag, fc / f_mag) if f_mag > 1e-9 else (0.0, 0.0)
    return {
        "F": [round(fr, 4), round(fc, 4)],
        "F_mag": round(f_mag, 4),
        "F_dir": [round(ufr, 4), round(ufc, 4)],
        "F_support": round(f_support, 4),
        "count": count,
        "anchor_count": anchor_count,
        "retreat_count": retreat_count,
    }


# ==========================================
# 🚨 断连瞬间命令优先权重 k_C 漂移（红/黄感叹号 Buff）
# ==========================================
def evaluate_disconnection_kc_drift(world, piece, is_disconnected):
    """在断连发生的瞬间触发一次基于 cadre 与单位性格偏好的确定性判定：
    - 减 (RED_EXCLAMATION): k_C <- k_{C,0} - d_kC (倾向顺应现场态势自主行动)
    - 加 (YELLOW_EXCLAMATION): k_C <- k_{C,0} + d_kC (倾向死守最新命令执行到底)
    """
    meta = piece.meta
    if not is_disconnected:
        meta["was_disconnected"] = False
        meta["exclamation_buff"] = None
        meta["k_C"] = KC_BASE
        return {"k_C": KC_BASE, "exclamation_buff": None}

    was_disc = bool(meta.get("was_disconnected", False))
    if was_disc and meta.get("exclamation_buff") in ("RED_EXCLAMATION", "YELLOW_EXCLAMATION") and "k_C" in meta:
        return {"k_C": float(meta["k_C"]), "exclamation_buff": meta["exclamation_buff"]}

    tot = piece.totals()
    c_ratio = float(tot["cadre"]) / max(1.0, float(tot["cadre_full"])) if tot.get("cadre_full", 0) > 0 else (
        float(tot["cadre"]) / max(1.0, float(piece.persons()))
    )
    personality = str(meta.get("personality", "")).lower()
    if personality in ("stubborn", "disciplined", "fanatic", "obedient"):
        bias = 0.35
    elif personality in ("flexible", "initiative", "pragmatic", "cautious"):
        bias = -0.35
    else:
        bias = 0.0

    turn = getattr(world, "turn", 0) if world is not None else 0
    clk = round(float(getattr(world, "clock_hours", 0.0)) if world is not None else 0.0, 4)
    digest = hashlib.sha1(f"{turn}:{clk}:{piece.id}:disc_kc_v31".encode("utf-8")).hexdigest()
    roll = int(digest[:8], 16) / float(0xFFFFFFFF)
    # 干部越多越偏“黄!(死守命令)”可能性，空干部或灵活性格更易“红!(战场自主)”
    stick_threshold = min(0.95, max(0.05, 0.35 + 0.30 * c_ratio + bias))
    if roll < stick_threshold:
        buff = "YELLOW_EXCLAMATION"
        kc_val = KC_BASE + DELTA_KC
    else:
        buff = "RED_EXCLAMATION"
        kc_val = max(0.25, KC_BASE - DELTA_KC)

    meta["was_disconnected"] = True
    meta["exclamation_buff"] = buff
    meta["k_C"] = round(kc_val, 4)
    return {"k_C": float(meta["k_C"]), "exclamation_buff": buff}


# ==========================================
# 🧭 9×9 地形容器扫描与短距同兵种排斥力
# ==========================================
def scan_9x9_terrain_container(world, piece, threat_info, R, spatial_index=None):
    """~1ms 预算核心：仅扫描以当前像素为中心的 9×9 容器，联合读取：
    1) AO 边界与分兵种战线势场 -> C_hat, d_C, N_line
    2) 高程微视线遮挡（反斜面识别）+ 掩蔽 cover + 通行度 -> G_terrain
    3) 同兵种友军占领斥力 -> F_repel 与防扎堆铺开
    """
    g = getattr(world, "grid", None) if world is not None else None
    pr, pc = float(piece.pos[0]), float(piece.pos[1])
    px_r, px_c = int(round(pr)), int(round(pc))
    unit_mask = classify_piece_unit_mask(piece)
    mob = piece.mobility()
    px_km = (g.cell_m / 1000.0) if g is not None else 0.2

    ao = (piece.meta or {}).get("active_order") or {}
    zone_id = ao.get("zone_id")
    direct_cmdr = command_v2.direct_commander_for(piece, getattr(world, "commanders", None)) if world is not None else None
    filter_cid = ao.get("commander_id")
    if filter_cid is None and direct_cmdr is not None:
        filter_cid = direct_cmdr.id
    if zone_id is None and world is not None and direct_cmdr is not None:
        cur_turn = int(getattr(world, "turn", 0))
        for z in (getattr(world, "ao_zones", {}) or {}).values():
            if z.commander_id == direct_cmdr.id and z.activate_turn <= cur_turn and z.team == piece.team:
                zone_id = z.zone_id
                break
    active_deploy_id = (piece.meta or {}).get("active_deploy_line_id")

    # 1. 同兵种短距友军排斥力 F_repel（防止多班组挤死同一最优格）
    sidx = _resolve_spatial_index(world, spatial_index)
    repel_r, repel_c = 0.0, 0.0
    occ_counts = {}
    same_type_nearby = 0
    if world is not None:
        nearby = (
            sidx.query_radius_px(piece.pos, 5.7, team=piece.team, exclude_id=piece.id)
            if sidx is not None
            else [
                (fp, math.hypot(float(fp.pos[0]) - pr, float(fp.pos[1]) - pc))
                for fp in world.pieces.values()
                if fp.id != piece.id and fp.team == piece.team and fp.persons() > 0
            ]
        )
        for fp, d_px in nearby:
            if fp.persons() <= 0:
                continue
            f_mask = classify_piece_unit_mask(fp)
            if not (f_mask & unit_mask):
                continue
            same_type_nearby += 1
            f_px = fp.px
            occ_counts[f_px] = occ_counts.get(f_px, 0) + 1
            if d_px <= 2.2:
                dr = float(fp.pos[0]) - pr
                dc = float(fp.pos[1]) - pc
                if d_px <= 1e-4:
                    # 同格重叠时按确定性 ID 序向战线切线方向弹开
                    sign = 1.0 if piece.id > fp.id else -1.0
                    repel_r += sign * 0.95
                    repel_c += sign * 0.15
                else:
                    w_rep = (2.2 - d_px) / 2.2
                    # 从本单位指向友军的相反方向即斥力（累加指向友军，下面统一减或存为排斥矢量）
                    # 注意公式中为 - F_repel，所以如果 F_repel 存的是指向友军的吸引力方向，减去它即排斥；
                    # 这里我们让 vec_repel_push 为“被推开的方向”，公式写入 + 或严格保持符号一致：
                    # 令 F_repel 指向「邻近同兵种友军方向」，这样 - F_repel 严格等于推离友军！
                    repel_r += w_rep * (dr / d_px) * 1.15
                    repel_c += w_rep * (dc / d_px) * 1.15

    # 若无网格兜底返回
    if g is None:
        return {
            "unit_mask": unit_mask,
            "current_cover": 0.0,
            "G_terrain": [0.0, 0.0],
            "F_repel": [round(repel_r, 4), round(repel_c, 4)],
            "C_dir": [0.0, 0.0],
            "d_C_km": 0.0,
            "front_normal": [0.0, 0.0],
            "in_ao": True,
            "line_strength": 0.0,
            "line_role": None,
            "active_deploy_line_id": active_deploy_id,
            "active_move_attack_line_id": None,
            "global_action_vec": [0.0, 0.0],
            "on_optimal_cell": False,
            "reverse_slope_best": False,
            "same_type_nearby": same_type_nearby,
        }

    H, W = g.shape
    cur_sample = g.sample_ao_and_frontline_field(
        (px_r, px_c), piece.team, unit_mask,
        zone_id=zone_id, commander_id=filter_cid, active_deploy_line_id=active_deploy_id,
    )
    claimed_dl = cur_sample.get("claimed_deploy_line_id")
    if claimed_dl and claimed_dl != active_deploy_id:
        piece.meta["active_deploy_line_id"] = claimed_dl
        active_deploy_id = claimed_dl
        cur_sample = g.sample_ao_and_frontline_field(
            (px_r, px_c), piece.team, unit_mask,
            zone_id=zone_id, commander_id=filter_cid, active_deploy_line_id=active_deploy_id,
        )
    cur_cov = g.effective_cover_at((px_r, px_c))

    # 2. 遍历 9×9 容器寻找最优地形/战线/反斜面/林缘格
    utr, utc = threat_info.get("threat_dir") or threat_info.get("T_dir") or (0.0, 1.0)
    has_threat = float(threat_info.get("S", 0.0)) > 0.5 and float(threat_info.get("T_mag", 0.0)) > 1e-6
    stealth_mult = 0.4 + 0.8 * min(3.0, R) + 0.5 * min(2.0, float(g.exposure[px_r, px_c]) / 20.0) if (0 <= px_r < H and 0 <= px_c < W) else 0.4
    slope_limit = tf.SLOPE_LIMIT_DEG.get(mob, 30.0)
    armor_heavy = (mob in ("wheeled", "tracked"))

    best_score = -1e9
    best_cell = (px_r, px_c)
    best_rev = False
    cur_score = -1e9
    best_line_cell = None
    best_line_val = -1e9
    has_local_fl = bool((g.frontline_channel.get(piece.team) or {}))

    r_lo, r_hi = max(0, px_r - 4), min(H - 1, px_r + 4)
    c_lo, c_hi = max(0, px_c - 4), min(W - 1, px_c + 4)
    has_cell_mods = bool(g.cell_modifiers)
    has_feats = bool(g.feature_channels)
    fl_team_map = g.frontline_channel.get(piece.team) or {}
    t_ao = (g.ao_channel.get(piece.team) or {}).get("cell_zones") or {}
    t_costs = tf.TERRAIN_COST.get(mob, {})

    for nr in range(r_lo, r_hi + 1):
        for nc in range(c_lo, c_hi + 1):
            t_code = int(g.terrain[nr, nc])
            is_road = t_code in tf.ROAD_TYPES
            step_cost = t_costs.get(t_code, math.inf)
            if not math.isfinite(step_cost):
                continue
            if not is_road and float(g.slope_deg[nr, nc]) > slope_limit:
                continue

            # 快速掩蔽度
            cov = tf.COVER.get(t_code, 0.0)
            if has_feats:
                for fid, ch in g.feature_channels.items():
                    if bool(ch["mask"][nr, nc]):
                        mods_f = ch.get("modifiers") or tf.TERRAIN_FEATURE_SPECS.get(fid, {}).get("modifiers", {})
                        if mods_f.get("impassable"):
                            cov = -999.0
                            break
                        cov = max(cov, float(mods_f.get("cover_bonus", 0.0) or 0.0))
            if cov < -100.0:
                continue
            if has_cell_mods and (nr, nc) in g.cell_modifiers:
                cov = max(cov, g.effective_cover_at((nr, nc)))

            # 反斜面微视线遮挡检验：沿威胁方向前看 1~2 格地形高差
            rev_bonus = 0.0
            if has_threat:
                elev_here = float(g.elevation[nr, nc])
                for step_i in (1, 2):
                    fr = int(round(nr + utr * step_i))
                    fc = int(round(nc + utc * step_i))
                    if 0 <= fr < H and 0 <= fc < W:
                        crest = float(g.elevation[fr, fc])
                        if crest >= elev_here + 6.0:
                            rev_bonus = max(rev_bonus, 0.45)
                        elif int(g.terrain[fr, fc]) in (tf.FOREST, tf.URBAN) and t_code != tf.FOREST:
                            rev_bonus = max(rev_bonus, 0.25)

            # 分兵种战线吸引力（单线直属过滤 + 排除全局行动线 + 溯源门槛过滤）
            w_line_cell = 0.0
            if fl_team_map and (nr, nc) in fl_team_map:
                for ent in fl_team_map[(nr, nc)]:
                    fl_cid = ent.get("commander_id")
                    if filter_cid is not None and fl_cid is not None and fl_cid != filter_cid:
                        continue
                    ent_role = ent.get("line_role", tf.LINE_ROLE_DEPLOY)
                    if ent_role == tf.LINE_ROLE_GLOBAL_ACTION:
                        continue
                    if ent_role in (tf.LINE_ROLE_MOVE, tf.LINE_ROLE_ATTACK):
                        src_dl = ent.get("source_deploy_line_id")
                        if src_dl is not None and src_dl != active_deploy_id:
                            continue
                    w_cand = float(ent["line_strength"]) * tf.unit_line_affinity(unit_mask, ent["unit_type_mask"])
                    if w_cand > w_line_cell:
                        w_line_cell = w_cand

            occ = occ_counts.get((nr, nc), 0)
            dist_self = math.hypot(float(nr) - pr, float(nc) - pc)
            # 重甲因森林 move_time 极高自然贴林缘而非深钻林心
            mob_pen = (step_cost - 1.0) * (0.35 if armor_heavy else 0.14)
            ao_bonus = 0.20 if ((nr, nc) in t_ao) else (-0.25 if t_ao else 0.0)
            line_dist_pen = (0.025 * dist_self) if w_line_cell > 0.05 else 0.0
            score = cov * stealth_mult + rev_bonus + 1.15 * w_line_cell + ao_bonus - mob_pen - 0.65 * occ - line_dist_pen

            if (nr, nc) == (px_r, px_c):
                cur_score = score
            if score > best_score:
                best_score = score
                best_cell = (nr, nc)
                best_rev = (rev_bonus > 0.1)
            line_score_cell = w_line_cell - 0.025 * dist_self
            if w_line_cell > 0.05 and occ == 0 and line_score_cell > best_line_val:
                best_line_val = line_score_cell
                best_line_cell = (nr, nc)

    if best_cell != (px_r, px_c):
        gdr = float(best_cell[0]) - pr
        gdc = float(best_cell[1]) - pc
        gm = math.hypot(gdr, gdc)
        g_vec = [gdr / gm, gdc / gm] if gm > 1e-6 else [0.0, 0.0]
    else:
        g_vec = [0.0, 0.0]

    # 3. 合成命令导向矢量 C_hat（融合 planned_route 路点 + AO 边界拉回 + 分兵种战线引力）
    wp = _next_route_waypoint(piece)
    doctrine = str(ao.get("doctrine", "hold")).lower()
    has_wp_goal = (wp is not None) and (doctrine not in ("hold", "hold_position"))
    cr_sum, cc_sum = 0.0, 0.0
    d_c_km = 0.0
    if has_wp_goal:
        dr_w = float(wp[0]) - pr
        dc_w = float(wp[1]) - pc
        dw = math.hypot(dr_w, dc_w)
        d_c_km = max(d_c_km, dw * px_km)
        if dw > 0.25:
            cr_sum += (dr_w / dw) * 1.0
            cc_sum += (dc_w / dw) * 1.0
    if ao.get("target_px") is not None:
        tgt_px = ao["target_px"]
        d_c_km = max(d_c_km, math.hypot(float(tgt_px[0]) - pr, float(tgt_px[1]) - pc) * px_km)

    if not cur_sample["in_ao"]:
        ap = cur_sample["ao_pull_vec"]
        cr_sum += ap[0] * 1.4
        cc_sum += ap[1] * 1.4

    # 分兵种战线引力：若挂有激活移动/进攻线则优先沿突击方向推进至终点；否则围绕部署线微调寻优
    if cur_sample.get("active_move_attack_line_id") is not None:
        lp = cur_sample["line_pull_vec"]
        w_l = 0.95 + cur_sample["line_strength"]
        cr_sum += lp[0] * w_l
        cc_sum += lp[1] * w_l
        mv_fl = g.typed_frontlines.get(cur_sample["active_move_attack_line_id"])
        if mv_fl and mv_fl.get("end_px"):
            end_p = mv_fl["end_px"]
            d_c_km = max(d_c_km, math.hypot(float(end_p[0]) - pr, float(end_p[1]) - pc) * px_km)
        elif cur_sample["dist_to_line_px"] is not None:
            d_c_km = max(d_c_km, float(cur_sample["dist_to_line_px"]) * px_km)
    elif best_line_cell is not None and best_line_val > 0.05:
        dr_l = float(best_line_cell[0]) - pr
        dc_l = float(best_line_cell[1]) - pc
        dl = math.hypot(dr_l, dc_l)
        if dl > 0.15:
            w_l = 0.8 + best_line_val
            cr_sum += (dr_l / dl) * w_l
            cc_sum += (dc_l / dl) * w_l
        d_c_km = max(d_c_km, dl * px_km)
    elif cur_sample["best_line_id"] is not None:
        lp = cur_sample["line_pull_vec"]
        w_l = 0.65 + cur_sample["line_strength"]
        cr_sum += lp[0] * w_l
        cc_sum += lp[1] * w_l
        if cur_sample["dist_to_line_px"] is not None:
            d_c_km = max(d_c_km, float(cur_sample["dist_to_line_px"]) * px_km)

    cm = math.hypot(cr_sum, cc_sum)
    c_dir = [cr_sum / cm, cc_sum / cm] if cm > 1e-6 else [0.0, 0.0]

    # 是否已在战线 / AO 最优位（且未和同兵种扎堆；正在沿 MOVE/ATTACK_LINE 前出时不得误判驻防完成）
    cur_occ = occ_counts.get((px_r, px_c), 0)
    near_line = (cur_sample["dist_to_line_px"] is not None and cur_sample["dist_to_line_px"] <= 1.2 and cur_sample["affinity"] >= 0.35)
    at_wp_or_no_move = (not has_wp_goal) or (math.hypot(float(wp[0]) - pr, float(wp[1]) - pc) <= 0.35)
    has_applicable_fl = bool(best_line_cell is not None or cur_sample.get("best_line_id") is not None) if filter_cid is not None else has_local_fl
    on_optimal = (
        cur_sample["in_ao"]
        and cur_occ == 0
        and ((cur_score >= best_score - 0.12) or (near_line and cur_occ == 0))
        and (near_line or (has_applicable_fl is False and at_wp_or_no_move))
        and not cur_sample.get("active_move_attack_line_id")
    )

    return {
        "unit_mask": unit_mask,
        "current_cover": round(cur_cov, 4),
        "G_terrain": [round(g_vec[0], 4), round(g_vec[1], 4)],
        "best_terrain_cell": list(best_cell),
        "F_repel": [round(repel_r, 4), round(repel_c, 4)],
        "C_dir": [round(c_dir[0], 4), round(c_dir[1], 4)],
        "d_C_km": round(d_c_km, 4),
        "front_normal": cur_sample["front_normal"],
        "in_ao": cur_sample["in_ao"],
        "line_strength": cur_sample["line_strength"],
        "best_line_id": cur_sample["best_line_id"],
        "line_role": cur_sample.get("best_line_role"),
        "active_deploy_line_id": active_deploy_id,
        "active_move_attack_line_id": cur_sample.get("active_move_attack_line_id"),
        "global_action_vec": cur_sample.get("global_action_vec", [0.0, 0.0]),
        "on_optimal_cell": bool(on_optimal),
        "reverse_slope_best": bool(best_rev),
        "same_type_nearby": same_type_nearby,
    }


# ==========================================
# 📋 下行命令接收与动态路点预规划
# ==========================================
def plan_order_route(world, piece, order_dict, dynamic_cost_params=None):
    """收到上级作战日志命令时，一次性结合 200m/px 动态地形代价规划粗略路点（平时推演只扫 9×9）。"""
    order = dict(order_dict or {})
    target = order.get("target_px") or order.get("to")
    doctrine = str(order.get("doctrine", order.get("mode", "advance"))).lower()
    piece.meta["active_order"] = {
        "order_id": order.get("order_id") or order.get("id") or f"ord-{piece.id}",
        "issuer": order.get("issuer"),
        "zone_id": order.get("zone_id"),
        "target_px": [int(target[0]), int(target[1])] if target is not None else list(piece.px),
        "doctrine": doctrine,
        "doctrine_weight": float(order.get("doctrine_weight") if order.get("doctrine_weight") is not None else DOCTRINE_WEIGHTS.get(doctrine, 1.0)),
        "text": order.get("text", ""),
        "issued_hours": float(getattr(world, "clock_hours", 0.0)) if world is not None else 0.0,
    }
    if target is None or world is None or getattr(world, "grid", None) is None:
        piece.meta["planned_route"] = {
            "path": [list(piece.px)],
            "costs": [0.0],
            "cursor": 0,
            "target_px": list(piece.px),
            "doctrine": doctrine,
            "alternating_bounds": [],
        }
        return piece.meta["planned_route"]

    tgt_px = (int(round(target[0])), int(round(target[1])))
    dyn_p = dynamic_cost_params
    if dyn_p is None:
        prev_r = float(((piece.meta or {}).get("autonomy_state") or {}).get("R", 0.0))
        dyn_p = {"R": prev_r, "alpha_stealth": 0.05, "beta1": 0.65, "beta2": 0.55}
    res = tf.shortest_path(world.grid, piece.px, tgt_px, piece.mobility(), dynamic_cost_params=dyn_p)
    if res is None:
        path = [piece.px, tgt_px]
        costs = [0.0, world.grid.distance_km(piece.px, tgt_px)]
        reachable = False
    else:
        path, costs = res
        reachable = True

    bounds = [list(path[i]) for i in range(0, len(path), 3)]
    route = {
        "path": [[int(p[0]), int(p[1])] for p in path],
        "costs": [round(float(c), 4) for c in costs],
        "cursor": 0,
        "target_px": [tgt_px[0], tgt_px[1]],
        "doctrine": doctrine,
        "reachable": reachable,
        "alternating_bounds": bounds,
    }
    piece.meta["planned_route"] = route
    return route


def _next_route_waypoint(piece):
    route = (piece.meta or {}).get("planned_route")
    if not route or not route.get("path"):
        ao = (piece.meta or {}).get("active_order") or {}
        tgt = ao.get("target_px")
        return tuple(tgt) if tgt else None
    path = route["path"]
    cur = int(route.get("cursor", 0))
    while cur < len(path) - 1:
        wp = (int(path[cur][0]), int(path[cur][1]))
        if math.hypot(wp[0] - piece.pos[0], wp[1] - piece.pos[1]) < 0.35:
            cur += 1
        else:
            break
    route["cursor"] = min(cur, len(path) - 1)
    wp = path[route["cursor"]]
    return (int(wp[0]), int(wp[1]))


# ==========================================
# ⚙️ 5 分钟纯数学状态器 (Output = (Action, 方向))
# ==========================================
def _find_nearby_interact_targets(world, piece, spatial_index=None):
    """查找邻近 1 格内可用 CellStash 与可搭载空载具。"""
    g = getattr(world, "grid", None) if world is not None else None
    pr, pc = piece.px
    best_stash_val = 0.0
    best_stash_px = None
    if g is not None and getattr(g, "cell_stash", None):
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                rc = (pr + dr, pc + dc)
                st = g.cell_stash.get(rc)
                if not st:
                    continue
                owner = st.get("owner_team")
                if owner not in (None, piece.team):
                    continue
                res = st.get("resources") or {}
                val = float(res.get("ammo", 0.0)) * 0.5 + float(res.get("food", 0.0)) * 0.25 + len(st.get("equipment") or {}) * 0.35
                val = min(1.0, val)
                if val > best_stash_val:
                    best_stash_val = val
                    best_stash_px = rc

    veh_target = None
    if world is not None and not getattr(piece, "vehicle_model", None) and not piece.attached_to:
        sidx = _resolve_spatial_index(world, spatial_index)
        cands = (
            sidx.query_radius_px(piece.pos, 1.6, team=piece.team, exclude_id=piece.id)
            if sidx is not None
            else [(vp, math.hypot(vp.pos[0] - piece.pos[0], vp.pos[1] - piece.pos[1])) for vp in world.pieces.values() if vp.team == piece.team and vp.id != piece.id]
        )
        for vp, d_px in cands:
            if d_px <= 1.6 and getattr(vp, "vehicle_model", None) in ("truck", "halftrack", "tractor") and vp.persons() > 0:
                veh_target = vp
                break
    return best_stash_val, best_stash_px, veh_target


def decide_next_5min_action(world, piece, extra_enemy_contacts=None, spatial_index=None):
    """V3.1 5 分钟数学状态器（Output = (Action, 方向)，零硬编码剧本，全涌现判定）。"""
    sidx = _resolve_spatial_index(world, spatial_index)
    ao = (piece.meta or {}).get("active_order") or {}
    doctrine = str(ao.get("doctrine", "hold")).lower()
    w_cmd = float(ao.get("doctrine_weight") if ao.get("doctrine_weight") is not None else DOCTRINE_WEIGHTS.get(doctrine, 0.8))
    alpha_agg = min(0.92, max(0.15, 0.45 * w_cmd))

    # 1. 断连权重漂移 k_C
    is_disc = bool((piece.meta or {}).get("is_disconnected", False))
    disc_res = evaluate_disconnection_kc_drift(world, piece, is_disc)
    k_C = disc_res["k_C"]
    # 若存在主动下令，k_C 受条令基准微调；静止默认防御时不夸大强行位移
    if not ao.get("target_px") and doctrine in ("hold", "hold_position") and piece.meta.get("exclamation_buff") != "YELLOW_EXCLAMATION":
        k_C = k_C * 0.6

    # 2. 威胁矢量 T、压力比 R 与友军矢量 F
    threat = compute_threat_vector(world, piece, extra_enemy_contacts=extra_enemy_contacts, spatial_index=sidx)
    friendly = compute_friendly_vector(world, piece, spatial_index=sidx)
    S = threat["S"]
    c = threat["c"]
    utr, utc = threat["threat_dir"]
    ufr, ufc = friendly["F_dir"]

    mods = {"hold_still": False, "forbid_move": False, "forbid_fire": False, "forbid_deploy": False, "combat_power_mult": 1.0, "kc_mult": 1.0, "tau_mult": 1.0, "shock_m_add": 0.0}
    if world is not None and getattr(world, "event_manager", None) is not None:
        mods = world.event_manager.piece_modifiers(piece, world=world)
    held = bool(mods.get("hold_still") or mods.get("forbid_move"))
    forbid_fire = bool(mods.get("forbid_fire"))
    k_C = round(k_C * max(0.2, float(mods.get("kc_mult", 1.0))), 4)

    orgs = [s.org for s in piece.squads()] or [0.0]
    caps = [max(1.0, s.org_cap()) for s in piece.squads()] or [100.0]
    org_mean = sum(orgs) / len(orgs)
    org_cap = sum(caps) / len(caps)
    P = min(1.0, max(0.0, org_mean / max(1.0, org_cap)))
    d5_org = float((piece.meta or {}).get("delta_org_5m", 0.0))
    sup_flag = 0.25 if ((world is not None and getattr(world, "grid", None) is not None) and any(
        k.startswith("supp:") for k in (world.grid.cell_modifiers.get(piece.px) or {})
    )) else 0.0
    M = min(1.0, max(0.0, max(0.0, -d5_org) / 25.0 + (1.0 - P) * 0.55 + sup_flag + float(mods.get("shock_m_add", 0.0))))

    own_pow = _piece_strength_estimate(piece)
    p_eff = max(1.0, own_pow * max(0.15, P) * float(mods.get("combat_power_mult", 1.0)))
    R = (S / max(1.0, p_eff + 0.45 * friendly["F_support"])) if S > 1e-6 else 0.0
    ammo_cap = float(piece.stock_cap.get("ammo", 0.1)) or 0.1
    A = min(1.0, max(0.0, float(piece.stock.get("ammo", 0.0)) / ammo_cap))

    # 3. 9×9 地形容器扫描
    scan9 = scan_9x9_terrain_container(world, piece, threat, R, spatial_index=sidx)
    ucr, ucc = scan9["C_dir"]
    gr, gc = scan9["G_terrain"]
    rep_r, rep_c = scan9["F_repel"]
    cover = scan9["current_cover"]
    on_opt = scan9["on_optimal_cell"]
    dot_CT = (ucr * utr + ucc * utc) if ((abs(ucr) + abs(ucc)) > 1e-6 and threat["T_mag"] > 1e-6) else 1.0

    # 4. 统一行动方向公式 V_move（叠加直属本级全局行动线引导矢量 G_action）
    k_terrain = K_TERRAIN_0 * (S + M)
    threat_push = KT_BASE * (R * (1.0 - P) + M)
    crossfire_collapse = (P < 0.35 and (R > 1.2 or threat["is_crossfire"]) and S > 5.0 and piece.meta.get("exclamation_buff") != "YELLOW_EXCLAMATION")
    k_C_eff = k_C * max(0.35, P) if crossfire_collapse else k_C
    k_F_eff = KF_BASE if (R > 0.05 or M > 0.15) else (0.15 * KF_BASE)
    ga_vec = scan9.get("global_action_vec") or [0.0, 0.0]
    ga_r, ga_c = float(ga_vec[0]), float(ga_vec[1])
    ga_w = 0.45 * k_C_eff if (abs(ga_r) + abs(ga_c)) > 1e-6 else 0.0
    vm_r = k_C_eff * ucr + ga_w * ga_r - threat_push * utr + k_F_eff * ufr + k_terrain * gr - rep_r
    vm_c = k_C_eff * ucc + ga_w * ga_c - threat_push * utc + k_F_eff * ufc + k_terrain * gc - rep_c
    v_mag = math.hypot(vm_r, vm_c)
    if v_mag > 1e-6:
        dm_r, dm_c = vm_r / v_mag, vm_c / v_mag
    else:
        dm_r, dm_c = 0.0, 0.0

    # 交替掩护撤退合方向指标 -aT + bF（兼容历史与可观测输出）
    wr = -1.2 * utr + 0.8 * ufr
    wc = -1.2 * utc + 0.8 * ufc
    wm = math.hypot(wr, wc)
    if wm <= 1e-6:
        wr, wc = (dm_r, dm_c) if v_mag > 1e-6 else (-1.0, 0.0)
        wm = math.hypot(wr, wc) or 1.0
    withdraw_dir = [round(wr / wm, 4), round(wc / wm, 4)]

    # 5. 五项原子行动打分（先 locks 剔除，再加防抖滞回 +H_stick）
    stash_val, stash_px, veh_target = _find_nearby_interact_targets(world, piece, spatial_index=sidx)
    has_deployable = any(
        int(n) > 0 and toe.EQUIPMENT.get(m, {}).get("deployable")
        for sq in piece.squads()
        for m, n in (sq.equipment or {}).items()
    )
    need_pack_first = bool(getattr(piece, "is_heavy_emplacement", False))

    p_hit = 0.0
    if threat["contact_count"] > 0 and not forbid_fire:
        rng_km = _piece_primary_weapon_range_m(piece) / 1000.0
        nd_km = threat["nearest_d_km"] if threat["nearest_d_km"] is not None else 0.4
        p_hit = 1.0 / (1.0 + (nd_km / max(0.1, rng_km)) ** 2)

    if ao.get("alpha_stealth") is not None:
        alpha_stealth = float(ao["alpha_stealth"])
    elif (piece.meta or {}).get("alpha_stealth") is not None:
        alpha_stealth = float(piece.meta["alpha_stealth"])
    else:
        doc_s = 0.45 if doctrine in ("hold", "defend", "screen", "recon") else 0.15
        alpha_stealth = min(1.0, max(0.0, doc_s + 0.45 * cover + (0.20 if scan9.get("reverse_slope_best") else 0.0)))

    g_ref = getattr(world, "grid", None) if world is not None else None
    g_exp = float(g_ref.exposure[piece.px]) if (g_ref is not None and 0 <= piece.px[0] < g_ref.shape[0] and 0 <= piece.px[1] < g_ref.shape[1]) else 0.0
    is_exposed = bool(
        (piece.meta or {}).get("exposed")
        or g_exp >= 12.0
        or d5_org < -0.5
        or ((piece.meta or {}).get("autonomy_state") or {}).get("atomic_action") == "ATTACK"
    )

    is_deployed = bool(getattr(piece, "is_heavy_emplacement", False) or getattr(piece, "unit_state", None) == "deployed")
    has_tgt_far = bool(ao.get("target_px")) and math.hypot(float(ao["target_px"][0]) - piece.pos[0], float(ao["target_px"][1]) - piece.pos[1]) > 1.5
    allow_deployed_pack = bool(
        scan9.get("active_move_attack_line_id")
        or has_tgt_far
        or scan9["d_C_km"] > 0.60
        or R > 1.6
        or (P < 0.30 and S > 3.0)
        or (threat.get("is_crossfire") and R > 1.1)
    )

    directional = is_directional_hull_unit(piece)
    fn_r, fn_c = scan9["front_normal"]
    if held and directional:
        fr_d, fc_d = toe.bearing_to_vec(getattr(piece, "facing_deg", 0.0)) if hasattr(toe, "bearing_to_vec") else (
            math.sin(math.radians(getattr(piece, "facing_deg", 0.0))),
            math.cos(math.radians(getattr(piece, "facing_deg", 0.0))),
        )
    elif S > 0.5 and threat["T_mag"] > 1e-6:
        fr_d, fc_d = utr, utc
    elif (abs(fn_r) + abs(fn_c)) > 1e-6:
        fr_d, fc_d = fn_r, fn_c
    elif (abs(ucr) + abs(ucc)) > 1e-6:
        fr_d, fc_d = ucr, ucc
    else:
        fr_d, fc_d = (-1.0, 0.0)
    piece.meta["tactical_face_vec"] = [round(fr_d, 4), round(fc_d, 4)]

    # 0. 用户/指挥官主动作战队列 task_queue 与不可逆撤退状态机优先接管
    t_queue = (piece.meta or {}).get("task_queue") or []
    ret_rec = float((piece.meta or {}).get("retreat_recovery_hours", 0.0))
    task_override = False
    if ret_rec > 0.0:
        piece.meta["retreat_recovery_hours"] = max(0.0, ret_rec - (5.0 / 60.0))
        atomic_action = "RETREAT"
        mode = "withdraw"
        advance_scale = 1.45
        piece.meta["exposed"] = True
        alpha_stealth = 0.1
        wr, wc = withdraw_dir
        move_target = [piece.pos[0] + wr * 3.0, piece.pos[1] + wc * 3.0]
        action_label = "emergency_retreat"
        task_override = True
    elif t_queue:
        while t_queue:
            cur_step = t_queue[0]
            goal = cur_step.get("goal") or list(piece.pos)
            if cur_step.get("follow_target") and cur_step.get("target_unit_id") and world:
                tgt_p = world.pieces.get(cur_step["target_unit_id"])
                if tgt_p and (tgt_p.persons() > 0 or getattr(tgt_p, "vehicle_model", None) or getattr(tgt_p, "deployed_weapon", None)):
                    if _has_target_tracking(world, piece, tgt_p):
                        goal = list(tgt_p.pos)
                        cur_step["goal"] = list(goal)
            d_to_goal = math.hypot(goal[0] - piece.pos[0], goal[1] - piece.pos[1])
            if d_to_goal < 0.8:
                t_queue.pop(0)
                piece.meta["task_queue"] = t_queue
                continue
            break

        if t_queue:
            cur_step = t_queue[0]
            act = cur_step.get("action", "move")
            mm = cur_step.get("move_mode", "normal")
            fm = cur_step.get("fire_mode", "stop_to_fire")
            goal = cur_step.get("goal") or list(piece.pos)
            if cur_step.get("follow_target") and cur_step.get("target_unit_id") and world:
                tgt_p = world.pieces.get(cur_step["target_unit_id"])
                if tgt_p and (tgt_p.persons() > 0 or getattr(tgt_p, "vehicle_model", None) or getattr(tgt_p, "deployed_weapon", None)):
                    if _has_target_tracking(world, piece, tgt_p):
                        goal = list(tgt_p.pos)
                        cur_step["goal"] = list(goal)

            if act == "retreat":
                piece.meta["retreat_recovery_hours"] = 1.0
                piece.meta["task_queue"] = []
                atomic_action = "RETREAT"
                mode = "withdraw"
                advance_scale = 1.25
                piece.meta["exposed"] = True
                alpha_stealth = 0.1
                wr, wc = withdraw_dir
                move_target = [piece.pos[0] + wr * 3.0, piece.pos[1] + wc * 3.0]
                action_label = "emergency_retreat"
                task_override = True
            elif act == "stop":
                atomic_action = "HOLD"
                mode = "hold"
                advance_scale = 0.0
                move_target = None
                action_label = "hold_position"
                task_override = True
            elif act in ("move", "assault"):
                if mm == "fast":
                    advance_scale = 1.20
                    piece.meta["exposed"] = True
                    alpha_stealth = 0.1
                elif mm in ("stealth", "slow"):
                    advance_scale = 0.55
                    piece.meta["exposed"] = False
                    alpha_stealth = 0.95
                else:
                    advance_scale = 1.0
                    piece.meta["exposed"] = False

                if fm == "stop_to_fire" and S > 0.5 and not forbid_fire:
                    atomic_action = "ATTACK"
                    mode = "fire"
                    advance_scale = 0.0
                    action_label = "stop_to_fire"
                else:
                    atomic_action = "MOVE"
                    mode = "advance"
                    move_target = [goal[0], goal[1]]
                    action_label = f"{act}_{mm}"
                task_override = True
            elif act == "attack":
                atomic_action = "ATTACK"
                mode = "fire"
                advance_scale = 0.0
                action_label = "target_fire"
                task_override = True

    if task_override and move_target is not None:
        tr_d, tc_d = move_target[0] - piece.pos[0], move_target[1] - piece.pos[1]
        tm = math.hypot(tr_d, tc_d)
        if tm > 1e-6:
            dm_r, dm_c = tr_d / tm, tc_d / tm

    scores = {}
    d_interact = [0.0, 0.0]
    interact_target = None
    heavy_op = None
    if not task_override:
        # 1) HOLD
        scores["HOLD"] = 1.00 * cover * (1.0 - alpha_agg) + 0.85 * M + 0.78 * (1.0 if on_opt else 0.0)
        # 2) MOVE（被 hold_still / forbid_move 锁死则硬剔除；已部署重武器非脱线/突击/危局禁止1格微调抖动移位）
        if not held and v_mag > 0.05 and (not is_deployed or allow_deployed_pack):
            mv_boost = 0.65 if (is_deployed and allow_deployed_pack and (scan9.get("active_move_attack_line_id") or has_tgt_far or scan9["d_C_km"] > 0.60)) else 0.0
            scores["MOVE"] = 0.75 * v_mag * (1.0 - M) + 0.90 * max(0.0, R - 1.0) * (1.0 - cover) + mv_boost
        # 3) ATTACK（部署线开火权衡：高隐蔽且未暴露时低命中开火将承受泄密惩罚而倾向伏击潜伏）
        if threat["contact_count"] > 0 and not forbid_fire:
            stealth_pen = K_STEALTH_COMPROMISE * alpha_stealth * max(0.0, 1.0 - p_hit) if (not is_exposed and alpha_stealth > 0.30) else 0.0
            opt_fire_bonus = 0.65 * min(1.0, A * 2.0) * p_hit if on_opt else 0.0
            scores["ATTACK"] = 1.65 * A * p_hit * (1.0 - 0.5 * M) + 0.35 * alpha_agg * c + opt_fire_bonus - stealth_pen
        # 4) PICKUP
        if stash_px is not None and not held:
            scores["PICKUP"] = 1.60 * (1.0 - A) * stash_val * (1.0 - 0.5 * R)
        # 5) EMBARK
        if veh_target is not None and not held:
            evacuable = 1.0 if (P < 0.30 and R > 1.3) else 0.0
            march_need = 1.0 if (scan9["d_C_km"] > D_MARCH_KM and S <= 1e-6) else 0.0
            scores["EMBARK"] = 1.75 * march_need + 1.65 * evacuable

        if is_deployed and not allow_deployed_pack:
            scores["HOLD"] += H_DEPLOYED
            if "ATTACK" in scores:
                scores["ATTACK"] += H_DEPLOYED

        prev_atomic = (piece.meta or {}).get("autonomy_state", {}).get("atomic_action")
        if prev_atomic in scores:
            scores[prev_atomic] += H_STICK

        priority_tie = {"HOLD": 0, "ATTACK": 1, "MOVE": 2, "PICKUP": 3, "EMBARK": 4}
        atomic_action = max(scores.keys(), key=lambda k: (scores[k], -priority_tie.get(k, 9)))

        # 重武器先判 PACK / DEPLOY 状态要求（防刚架好又打包乱动）
        if is_deployed or has_deployable:
            if atomic_action in ("MOVE", "EMBARK") and need_pack_first and allow_deployed_pack:
                heavy_op = "PACK"
            elif atomic_action in ("HOLD", "ATTACK") and not is_deployed and not mods.get("forbid_deploy"):
                if on_opt or S > 2.0:
                    heavy_op = "DEPLOY"

        # 交互矢量 d_interact & 移动目标点
        if atomic_action == "PICKUP" and stash_px is not None:
            idr, idc = float(stash_px[0]) - piece.pos[0], float(stash_px[1]) - piece.pos[1]
            im = math.hypot(idr, idc)
            d_interact = [round(idr / im, 4), round(idc / im, 4)] if im > 1e-6 else [0.0, 0.0]
            interact_target = list(stash_px)
        elif atomic_action == "EMBARK" and veh_target is not None:
            idr, idc = float(veh_target.pos[0]) - piece.pos[0], float(veh_target.pos[1]) - piece.pos[1]
            im = math.hypot(idr, idc)
            d_interact = [round(idr / im, 4), round(idc / im, 4)] if im > 1e-6 else [0.0, 0.0]
            interact_target = list(veh_target.px)

        move_target = None
        advance_scale = 0.0
        mode = "hold"
        retreat_aligned = ((dm_r * utr + dm_c * utc) < -0.15) if (S > 0.5 and threat["T_mag"] > 1e-6) else False
        if crossfire_collapse and (retreat_aligned or (dm_r * ufr + dm_c * ufc) > 0.1 or (P < 0.25 and friendly["anchor_count"] >= 1)):
            retreat_aligned = True
        if atomic_action == "MOVE" and not held and heavy_op != "PACK":
            mode = "withdraw" if (retreat_aligned and P < 0.45) else "advance"
            advance_scale = 0.85 if mode == "withdraw" else min(1.0, max(0.35, 0.55 + 0.3 * w_cmd))
            # 若有计划路径且目标方向与其高度一致则跟路点，否则按统一方向矢量 d_move 推进 2~3 px
            g = getattr(world, "grid", None) if world is not None else None
            wp = _next_route_waypoint(piece)
            if wp is not None and not is_disc and not retreat_aligned and doctrine not in ("hold", "hold_position"):
                wdr, wdc = float(wp[0]) - piece.pos[0], float(wp[1]) - piece.pos[1]
                wdm = math.hypot(wdr, wdc)
                if wdm > 0.25 and ((wdr / wdm) * dm_r + (wdc / wdm) * dm_c) > 0.45 and abs(rep_r) + abs(rep_c) < 0.25:
                    move_target = [int(wp[0]), int(wp[1])]
            if move_target is None:
                tr_tgt = piece.pos[0] + dm_r * 2.5
                tc_tgt = piece.pos[1] + dm_c * 2.5
                if g is not None:
                    tr_tgt = max(0, min(g.shape[0] - 1, int(round(tr_tgt))))
                    tc_tgt = max(0, min(g.shape[1] - 1, int(round(tc_tgt))))
                else:
                    tr_tgt, tc_tgt = int(round(tr_tgt)), int(round(tc_tgt))
                move_target = [tr_tgt, tc_tgt]

        # 描述性行为标签（纯涌现映射，保持向后可读性）
        if held:
            action_label = "take_cover_suppress" if S > 0.5 else "hold_position"
        elif heavy_op == "PACK":
            action_label = "pack_heavy_weapon"
        elif atomic_action == "HOLD":
            action_label = "hold_position"
        elif atomic_action == "ATTACK":
            action_label = "take_cover_suppress"
        elif atomic_action == "PICKUP":
            action_label = "resupply_pickup"
        elif atomic_action == "EMBARK":
            action_label = "embark_vehicle"
        elif mode == "withdraw" and P < 0.35:
            action_label = "alternating_withdraw"
        elif S > 1.0:
            action_label = "suppress_and_advance"
        else:
            action_label = "advance_planned"

    # 载体朝向更新（严格只改 is_directional_hull_unit，步兵 facing_deg 纹丝不动）
    if directional and not held and not (piece.meta or {}).get("turned_this_5m"):
        if S > 0.5 and threat["T_mag"] > 1e-6:
            piece.set_facing(threat["threat_bearing_deg"])
            piece.meta["turned_this_5m"] = True
        elif atomic_action in ("HOLD", "ATTACK") and (abs(fr_d) + abs(fc_d)) > 1e-6:
            piece.set_facing(math.degrees(math.atan2(fr_d, fc_d)) % 360.0)
            piece.meta["turned_this_5m"] = True
        elif move_target is not None and math.hypot(move_target[0] - piece.pos[0], move_target[1] - piece.pos[1]) > 1e-6:
            piece.set_facing(toe.bearing_deg(piece.pos, move_target))
            piece.meta["turned_this_5m"] = True

    out_dir = [round(dm_r, 4), round(dm_c, 4)] if atomic_action == "MOVE" else (
        d_interact if atomic_action in ("PICKUP", "EMBARK") else [round(fr_d, 4), round(fc_d, 4)]
    )

    decision = {
        "atomic_action": atomic_action,
        "action": action_label,
        "mode": mode,
        "direction": out_dir,
        "d_move": [round(dm_r, 4), round(dm_c, 4)],
        "d_face": [round(fr_d, 4), round(fc_d, 4)],
        "d_interact": d_interact,
        "interact_target": interact_target,
        "heavy_op": heavy_op,
        "scores": {k: round(v, 4) for k, v in scores.items()},
        "advance_scale": round(advance_scale, 3),
        "move_target": move_target,
        "withdraw_dir": withdraw_dir,
        "dot_CT": round(dot_CT, 4),
        "k_C": round(k_C, 3),
        "exclamation_buff": disc_res["exclamation_buff"],
        "R": round(R, 4),
        "S": threat["S"],
        "c": threat["c"],
        "M": round(M, 3),
        "P": round(P, 3),
        "A": round(A, 3),
        "is_crossfire": threat["is_crossfire"],
        "threat_bearing_deg": threat["threat_bearing_deg"],
        "friendly_count": friendly["count"],
        "on_optimal_cell": scan9["on_optimal_cell"],
        "reverse_slope_best": scan9["reverse_slope_best"],
        "directional_hull": directional,
        "locked_hold": held,
        "alpha_stealth": round(alpha_stealth, 3),
        "is_exposed": is_exposed,
        "p_hit": round(p_hit, 4),
        "active_deploy_line_id": scan9.get("active_deploy_line_id"),
        "active_move_attack_line_id": scan9.get("active_move_attack_line_id"),
        "global_action_vec": scan9.get("global_action_vec", [0.0, 0.0]),
        "active_debuff_tags": mods.get("active_debuff_tags", []),
        "active_buff_tags": mods.get("active_buff_tags", []),
    }
    piece.meta["autonomy_state"] = decision
    return decision


def _apply_vehicle_overload_5m(world, piece, dt_hours):
    """载具软超载：实时附加动态降速/高油耗 Debuff，并按 5 分钟步长触发确定性故障检定。"""
    if not getattr(piece, "vehicle_model", None):
        return None
    ov = piece.overload_debuff(world.pieces if world is not None else None)
    if ov["load_ratio"] <= 1.0:
        return None
    piece.add_debuff("soft_overload", {
        "speed_mult": ov["speed_mult"],
        "fuel_mult": ov["fuel_mult"],
        "forbid_move": ov["forbid_move"],
        "ttl_hours": max(dt_hours * 1.5, DEFAULT_SUBTICK_HOURS),
        "label": f"软超载({ov['load_ratio']}x)",
    })
    prob = min(0.85, (ov["load_ratio"] - 1.0) * 0.08 * (dt_hours / DEFAULT_SUBTICK_HOURS))
    turn = getattr(world, "turn", 0) if world is not None else 0
    clk = round(float(getattr(world, "clock_hours", 0.0)) if world is not None else 0.0, 4)
    digest = hashlib.sha1(f"{turn}:{clk}:{piece.id}:overload_v3".encode("utf-8")).hexdigest()
    roll = int(digest[:8], 16) / float(0xFFFFFFFF)
    if roll < prob:
        piece.add_debuff("overload_breakdown", {
            "forbid_move": True,
            "speed_mult": 0.0,
            "ttl_hours": 0.5,
            "label": "超载机械故障",
        })
        return {"triggered_breakdown": True, "load_ratio": ov["load_ratio"]}
    return {"triggered_breakdown": False, "load_ratio": ov["load_ratio"]}


def _has_target_tracking(world, piece, tgt_p):
    """仅允许己方单位或已取得 Lv2+ 侦察/直视通视锁定的目标被实时追踪，禁止隔山隔雾 GPS 追踪。"""
    if tgt_p is None:
        return False
    if tgt_p.team == piece.team:
        return True
    rec = (getattr(world, "recon_intel", None) or {}).get(piece.team, {}).get(tgt_p.id)
    if rec and int(rec.get("level", 0)) >= 2:
        return True
    g = getattr(world, "grid", None)
    if g is not None:
        dist_m = g.distance_km(piece.pos, tgt_p.pos) * 1000.0
        if dist_m <= 1200.0 and tf.line_visibility(g, piece.px, tgt_p.px, night=getattr(world, "night", False)) > 1e-6:
            return True
    return False


def _fast_local_step(g, piece, target, budget_km, mob):
    """地形与有向坡度感知的多步机动推进（消耗完整 5 分钟里程预算，不再 1 格截断）。"""
    rem_budget = float(budget_km)
    if rem_budget <= 1e-9:
        return False

    H, W = g.shape
    px_km = g.cell_m / 1000.0
    slope_limit = tf.SLOPE_LIMIT_DEG.get(mob, 30.0)
    t_costs = tf.TERRAIN_COST.get(mob, {})
    uphill_k = tf.UPHILL_K.get(mob, 0.04) if hasattr(tf, "UPHILL_K") else 0.04
    moved_any = False

    for _hop in range(64):
        pr, pc = float(piece.pos[0]), float(piece.pos[1])
        tr, tc = float(target[0]), float(target[1])
        dr_t, dc_t = tr - pr, tc - pc
        d_total_px = math.hypot(dr_t, dc_t)
        if d_total_px <= 1e-3 or rem_budget <= 1e-9:
            break

        r0, c0 = int(round(pr)), int(round(pc))
        ur_t, uc_t = dr_t / d_total_px, dc_t / d_total_px
        best_n = None
        best_sc = -1e9
        best_cost = 1.0

        for dr, dc in tf._NEIGHBORS:
            nr, nc = r0 + dr, c0 + dc
            if not (0 <= nr < H and 0 <= nc < W):
                continue
            tc_code = int(g.terrain[nr, nc])
            base_c = t_costs.get(tc_code, math.inf)
            if not math.isfinite(base_c):
                continue
            cmod_n = g.get_cell_modifiers((nr, nc), mobility=mob)
            if cmod_n.get("impassable"):
                continue
            feat_sp = float(cmod_n.get("non_feature_speed_mult", 1.0)) * float(cmod_n.get("feature_speed_mult", 1.0))
            if feat_sp <= 1e-6:
                continue
            up_deg = 0.0
            if tc_code not in tf.ROAD_TYPES:
                dz = float(g.elevation[nr, nc]) - float(g.elevation[r0, c0])
                step_m = max(1.0, g.cell_m * math.hypot(dr, dc))
                up_deg = math.degrees(math.atan2(max(0.0, dz), step_m))
                if float(g.slope_deg[nr, nc]) > slope_limit and dz >= 0.0:
                    continue
                if up_deg > slope_limit:
                    continue
            align = (dr * ur_t + dc * uc_t) / math.hypot(dr, dc)
            if align <= -0.1:
                continue
            eff_cost = max(0.35, float(base_c) * (1.0 + uphill_k * up_deg) / max(0.15, feat_sp))
            sc = align * 2.0 - (eff_cost - 1.0) * 0.25
            if sc > best_sc:
                best_sc = sc
                best_n = (nr, nc)
                best_cost = eff_cost

        if best_n is None:
            break
        # 若目标已在本格邻近，直接朝最终目标坐标收敛，否则朝最佳邻格中心推进
        if d_total_px <= 1.45 and int(round(tr)) == best_n[0] and int(round(tc)) == best_n[1]:
            step_dr, step_dc = tr - pr, tc - pc
        else:
            step_dr, step_dc = float(best_n[0]) - pr, float(best_n[1]) - pc
        d_step_px = math.hypot(step_dr, step_dc)
        if d_step_px <= 1e-6:
            break

        max_step_px = min(d_step_px, (rem_budget / best_cost) / px_km)
        if max_step_px <= 1e-6:
            break
        piece.pos = (pr + step_dr * (max_step_px / d_step_px), pc + step_dc * (max_step_px / d_step_px))
        piece.moved_last_turn = True
        moved_any = True
        spent_km = max_step_px * px_km * best_cost
        rem_budget = max(0.0, rem_budget - spent_km)
        if max_step_px < d_step_px - 1e-6:
            break

    return moved_any


def _linear_step_piece(world, piece, decision, dt_hours):
    if decision["locked_hold"] or decision["advance_scale"] <= 0.0 or decision["move_target"] is None:
        return False
    if decision.get("heavy_op") == "PACK":
        return False
    g = world.grid
    mob = piece.mobility()
    cmods = g.get_cell_modifiers(piece.px, mobility=mob)
    speed_mult = float(cmods.get("non_feature_speed_mult", 1.0)) * float(cmods.get("feature_speed_mult", 1.0))
    ev_mods = world.event_manager.piece_modifiers(piece, world=world) if getattr(world, "event_manager", None) else {"speed_mult": 1.0}
    base_kmh = piece.base_speed_kmh(world.pieces) if hasattr(piece, "base_speed_kmh") else BASE_SPEED_KMH.get(mob, 4.5)
    eff_speed = base_kmh * speed_mult * float(ev_mods.get("speed_mult", 1.0)) * float(decision["advance_scale"])
    budget_km = max(0.0, eff_speed * float(dt_hours))
    if budget_km <= 1e-9:
        return False

    tgt = (
        max(0, min(g.shape[0] - 1, int(round(decision["move_target"][0])))),
        max(0, min(g.shape[1] - 1, int(round(decision["move_target"][1])))),
    )
    t_queue = (piece.meta or {}).get("task_queue") or []
    move_mode = t_queue[0].get("move_mode", "normal") if t_queue else "normal"
    if move_mode == "fast" and decision.get("mode") != "withdraw":
        path = tf.shortest_path(g, piece.px, tgt, mob)
        if path is not None and len(path[0]) > 1:
            end, used, arrived = tf.advance_along(*path, budget_km)
            moved = False
            if end != piece.px:
                piece.pos = (float(end[0]), float(end[1]))
                piece.moved_last_turn = True
                moved = True
            rem_km = max(0.0, budget_km - float(used))
            if not arrived and rem_km > 1e-4:
                cells = path[0]
                try:
                    idx = cells.index(end)
                except ValueError:
                    idx = 0
                if idx + 1 < len(cells):
                    if _fast_local_step(g, piece, cells[idx + 1], rem_km, mob):
                        moved = True
            return moved

    route = (piece.meta or {}).get("planned_route")
    if route and route.get("path") and len(route["path"]) > 1 and decision.get("mode") == "advance":
        wp = _next_route_waypoint(piece)
        if wp is not None:
            return _fast_local_step(g, piece, wp, budget_km, mob)
    return _fast_local_step(g, piece, tgt, budget_km, mob)


def _execute_atomic_interaction(world, piece, dec, dt_hours):
    """执行原子动作中的拾取 / 搭载 / 重武器 PACK/DEPLOY 状态流转。"""
    act = dec["atomic_action"]
    if act == "PICKUP" and dec.get("interact_target"):
        rc = tuple(dec["interact_target"])
        if math.hypot(rc[0] - piece.pos[0], rc[1] - piece.pos[1]) <= 1.5:
            got = world.grid.stash_take(rc, resources={"ammo": 0.2, "food": 0.1})
            for rk, rv in got["resources"].items():
                if rk in piece.stock:
                    piece.stock[rk] = min(piece.stock_cap.get(rk, 1e9), piece.stock[rk] + rv)
    elif act == "EMBARK" and dec.get("interact_target") and not piece.attached_to:
        rc = tuple(dec["interact_target"])
        for vp in world.pieces.values():
            if vp.id != piece.id and vp.team == piece.team and vp.px == rc and getattr(vp, "vehicle_model", None):
                piece.attach_to(vp.id)
                piece.pos = tuple(vp.pos)
                break
    elif dec.get("heavy_op") == "PACK" and getattr(piece, "is_heavy_emplacement", False):
        if getattr(world, "event_manager", None) is not None and not world.event_manager.active_for(piece.id):
            world.event_manager.start_event(world, "pack_heavy_unit", piece.id, payload={"target_unit_id": piece.id, "detach_mobile": True}, duration_hours=max(dt_hours, 0.25))
    elif dec.get("heavy_op") == "DEPLOY" and not getattr(piece, "is_heavy_emplacement", False):
        if getattr(world, "event_manager", None) is not None and not world.event_manager.active_for(piece.id):
            auto_wpn = getattr(piece, "deployed_weapon", None)
            if not auto_wpn:
                for m, n in piece.equipment_counts().items():
                    if int(n) > 0 and m in ("hmg", "mortar_81", "at_gun_50", "howitzer_105", "howitzer_152", "flak_88"):
                        auto_wpn = m
                        break
            if auto_wpn:
                world.event_manager.start_event(
                    world, "deploy_heavy_unit", piece.id,
                    payload={"target_unit_id": piece.id, "weapon": auto_wpn},
                    duration_hours=max(dt_hours, 0.25),
                )
        elif getattr(world, "event_manager", None) is None:
            piece.unit_state = "deployed"
            piece.is_heavy_emplacement = True


def step_world_autonomy_5m(world, dt_hours=DEFAULT_SUBTICK_HOURS, execute_move=True, extra_contacts_by_piece=None, skip_move_pids=None):
    """在 5 分钟 Sub-tick 上重建空间桶索引并驱动全军涌现数学判定、载具超载检定与行动执行。"""
    sidx = build_spatial_index(world, bucket_px=2)
    results = {}
    extra_map = extra_contacts_by_piece or {}
    skip_set = set(skip_move_pids or ())
    for pid in sorted(world.pieces.keys()):
        p = world.pieces[pid]
        if p.persons() <= 0 and not getattr(p, "vehicle_model", None) and not getattr(p, "deployed_weapon", None):
            continue
        p.meta["turned_this_5m"] = False
        ov_res = _apply_vehicle_overload_5m(world, p, dt_hours)
        dec = decide_next_5min_action(world, p, extra_enemy_contacts=extra_map.get(pid), spatial_index=sidx)
        p_exec = execute_move and (pid not in skip_set)
        moved = _linear_step_piece(world, p, dec, dt_hours) if p_exec else False
        if p_exec:
            _execute_atomic_interaction(world, p, dec, dt_hours)
        dec["executed_move"] = moved
        if ov_res is not None:
            dec["overload"] = ov_res
        results[pid] = dec
    return results
