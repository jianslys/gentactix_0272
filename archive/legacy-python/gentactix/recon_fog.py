# -*- coding: utf-8 -*-
"""信息与战争迷雾（V2）：地面侦察、无线电测向、空中照相侦察、兵力虚实。

输出两层视图：
- truth_state            全图客观真值（上帝视角存档）
- fog_view(team)         某一方按侦察结果过滤后的情报简报

情报等级：0 未知 / 1 接触（位置模糊、兵力只有区间）/ 2 识别（番号清楚）/ 3 精确（含识破佯动）
模糊量与区间由 (单位 id, 回合) 的哈希决定 —— 同局面同输出，可复现。
"""

from __future__ import annotations

import hashlib
import math

from . import autonomy_v3
from . import terrain_field as tf

BASE_OBSERVE_KM = 1.5
RECON_BONUS_KM = 2.0
DF_RANGE_KM = 30.0
SKY_EYE_EXPOSURE_THRESHOLD = 20.0
BLUR_KM = {1: 4.0, 2: 1.0, 3: 0.0}
STRENGTH_BAND = {1: (0.6, 1.6), 2: (0.8, 1.2), 3: (1.0, 1.0)}


def observe_radius_km(piece):
    """棋子的地面观察半径：基础视距 + 侦察单位加成（支持模板与自由混编 recon_radius buff）。"""
    n_recon = sum(1 for s in piece.squads() if s.kind == "recon_squad" or s.has_buff("recon_radius"))
    buff = 1.0
    for s in piece.squads():
        if s.kind == "recon_squad" or s.has_buff("recon_radius"):
            buff = max(buff, float(s.get_buff("recon_radius", 1.0)))
    return BASE_OBSERVE_KM + RECON_BONUS_KM * min(1.0, n_recon / 3.0) * buff


def _has_radio(piece):
    return any(s.equipment.get("radio", 0) > 0 for s in piece.squads())


def can_request_support(piece):
    """无线电静默 = 脱离上级指挥链：不能实时呼叫跨区炮火与航空支援。"""
    return not piece.radio_silence


def _put(intel, team, target, level, source, dist):
    cur = intel.setdefault(team, {}).get(target.id)
    if cur is None:
        intel[team][target.id] = {"level": level, "sources": {source}, "dist_km": dist}
    else:
        cur["sources"].add(source)
        if level > cur["level"]:
            cur["level"] = level
        if dist is not None and (cur["dist_km"] is None or dist < cur["dist_km"]):
            cur["dist_km"] = dist


def gather_intel(grid, pieces, photos=(), night=False, recon_mult_fn=None, intercepted_channels=None):
    """汇总本回合各方对敌方棋子的情报（含信道破译窃听与假电台欺骗）。"""
    intel = {}
    vis = tf.visibility_field(grid, night)
    pcs = list(pieces)
    teams = {p.team for p in pcs}
    ic_map = intercepted_channels or {}
    for obs in pcs:
        r_km = observe_radius_km(obs)
        rm = float(recon_mult_fn(obs.px, obs.team)) if recon_mult_fn else 1.0
        obs_ic = ic_map.get(obs.team) or {}
        for tgt in pcs:
            if tgt.team == obs.team:
                continue
            d = grid.distance_km(obs.px, tgt.px)
            if _has_radio(tgt) and not tgt.radio_silence and tgt.radio_channel in obs_ic:
                _put(intel, obs.team, tgt, 3, "channel_intercept", d)
            t_code = int(grid.terrain[tgt.px])
            cell_mods = grid.get_cell_modifiers(tgt.px) if hasattr(grid, "get_cell_modifiers") else {}
            primary_fid = tf.TERRAIN_CODE_TO_FEATURE.get(t_code)
            base_conc = 0.0 if (primary_fid and primary_fid in cell_mods.get("features", ())) else tf.CONCEALMENT.get(t_code, 0.0)
            conceal = min(0.95, base_conc + float(cell_mods.get("concealment_bonus", 0.0)) + 0.4 * tgt.fortification)
            px_exp = float(grid.exposure[tgt.px]) if hasattr(grid, "exposure") else 0.0
            if px_exp >= SKY_EYE_EXPOSURE_THRESHOLD:
                _put(intel, obs.team, tgt, 3, "sky_eye", d)
                continue
            exp_mult = 1.0 + 0.25 * max(0.0, px_exp)
            eff = r_km * rm * exp_mult * float(vis[tgt.px]) * (1.0 - conceal)
            if eff > 0 and d <= eff and tf.line_visibility(grid, obs.px, tgt.px, night=night) > 1e-6:
                ratio = d / eff
                level = 3 if ratio <= 0.25 else (2 if ratio <= 0.6 else 1)
                _put(intel, obs.team, tgt, level, "ground", d)
            # 无线电测向：只抓得到没静默、且确有电台的目标
            if _has_radio(obs) and _has_radio(tgt) and not tgt.radio_silence and d <= DF_RANGE_KM:
                _put(intel, obs.team, tgt, 1, "sigint", d)
        if _has_radio(obs) and hasattr(grid, "cell_modifiers"):
            for rc, bucket in grid.cell_modifiers.items():
                for k, entry in bucket.items():
                    if not k.startswith("decoy_radio:"):
                        continue
                    decoy_team = k.split(":", 1)[1]
                    if decoy_team == obs.team:
                        continue
                    d = grid.distance_km(obs.px, rc)
                    if d <= DF_RANGE_KM:
                        dkey = f"__decoy__:{rc[0]}:{rc[1]}:{decoy_team}"
                        intel.setdefault(obs.team, {})[dkey] = {
                            "level": 1,
                            "sources": {"sigint_decoy"},
                            "dist_km": d,
                            "decoy_px": rc,
                            "fake_echelon": (entry.get("modifiers") or {}).get("fake_echelon", "battalion"),
                        }
    for ph in photos:
        for tgt in pcs:
            if tgt.team == ph["team"]:
                continue
            if math.hypot(tgt.px[0] - ph["px"][0], tgt.px[1] - ph["px"][1]) <= ph["radius_px"]:
                q = ph["quality"]
                level = 3 if q >= 0.6 else (2 if q >= 0.3 else 1)
                _put(intel, ph["team"], tgt, level, "air_photo", grid.distance_km(ph["px"], tgt.px))
    for t in teams:
        intel.setdefault(t, {})
    return intel


def _noise(key, n=2):
    h = hashlib.sha1(key.encode("utf-8")).digest()
    return [(h[i] / 255.0) * 2.0 - 1.0 for i in range(n)]


def piece_strength(piece):
    if piece.decoy and piece.fake_strength is not None:
        return float(piece.fake_strength)
    return float(piece.persons())


def fog_view(team, pieces, intel, grid, turn=0):
    """某一方看到的世界：己方精确，敌方按情报等级模糊。"""
    seen = intel.get(team, {})
    own, contacts = [], []
    for p in pieces:
        if p.team == team:
            has_f = autonomy_v3.is_directional_hull_unit(p)
            own.append({"id": p.id, "name": p.name, "px": p.px,
                        "pos": [round(p.pos[0], 3), round(p.pos[1], 3)],
                        "has_facing": has_f,
                        "facing_deg": round(p.facing_deg, 1) if has_f else None,
                        "autonomy_state": dict((getattr(p, "meta", {}) or {}).get("autonomy_state") or {}),
                        "unit_state": p.unit_state,
                        "deployed_weapon": p.deployed_weapon,
                        "vehicle_model": p.vehicle_model,
                        "attached_to": p.attached_to,
                        "operated_by": p.operated_by,
                        "cargo": p.cargo,
                        "status_debuffs": sorted(p.status_debuffs.keys()),
                        "roe": getattr(p, "roe", "return_fire"),
                        "meta": dict(getattr(p, "meta", {}) or {}),
                        "counter": p.counter_profile(),
                        "persons": p.persons(),
                        "org": round(p.mean_org(), 1), "radio_silence": p.radio_silence})
            continue
        info = seen.get(p.id)
        if not info or info["level"] <= 0:
            continue
        lvl = info["level"]
        nx, ny, ns = _noise(f"{p.id}|{turn}|{lvl}", 3)
        blur_px = grid.km_to_px(BLUR_KM[lvl])
        raw_r = max(0.0, min(float(grid.shape[0] - 1), p.px[0] + ny * blur_px))
        raw_c = max(0.0, min(float(grid.shape[1] - 1), p.px[1] + nx * blur_px))
        px = (raw_r, raw_c)
        lo, hi = STRENGTH_BAND[lvl]
        true = piece_strength(p)
        center = 1.0 + 0.2 * ns * (1 if lvl == 1 else 0.5)
        entry = {"level": lvl, "sources": sorted(info["sources"]),
                 "px": (round(px[0], 2), round(px[1], 2)),
                 "persons_range": (int(true * lo * center), int(true * hi * center)) if lvl < 3
                 else (int(true), int(true))}
        if lvl == 1:
            entry["id"] = "contact-" + hashlib.sha1(p.id.encode()).hexdigest()[:6]
            entry["name"] = "未识别接触"
        else:
            has_f = autonomy_v3.is_directional_hull_unit(p)
            entry["id"] = p.id
            entry["name"] = p.name
            entry["has_facing"] = has_f
            entry["facing_deg"] = round(p.facing_deg, 1) if has_f else None
            entry["unit_state"] = p.unit_state
            entry["deployed_weapon"] = p.deployed_weapon
            entry["vehicle_model"] = p.vehicle_model
            entry["attached_to"] = p.attached_to
            entry["operated_by"] = p.operated_by
            entry["status_debuffs"] = sorted(p.status_debuffs.keys())
            cp = p.counter_profile()
            entry["counter"] = (
                cp if lvl == 3 else {
                    "primary_icon": cp["primary_icon"],
                    "armor_class": cp["armor_class"],
                    "mobility": cp["mobility"],
                    "level": cp["level"],
                }
            )
        if lvl == 3:
            entry["cargo"] = p.cargo
            entry["decoy"] = bool(p.decoy)
            entry["autonomy_state"] = dict((getattr(p, "meta", {}) or {}).get("autonomy_state") or {})
            if p.decoy:
                entry["persons_range"] = (0, 0)
                entry["note"] = "识破佯动：该目标为假目标"
        contacts.append(entry)
    for key, info in seen.items():
        if not key.startswith("__decoy__:"):
            continue
        dpx = info["decoy_px"]
        nx, ny, _ = _noise(f"{key}|{turn}|1", 3)
        blur_px = grid.km_to_px(BLUR_KM[1])
        contacts.append({
            "id": "contact-" + hashlib.sha1(key.encode()).hexdigest()[:6],
            "name": "未识别无线电集群",
            "level": 1,
            "sources": sorted(info["sources"]),
            "px": (round(dpx[0] + ny * blur_px, 2), round(dpx[1] + nx * blur_px, 2)),
            "persons_range": (120, 320),
        })
    return {"team": team, "turn": turn, "own": own, "contacts": contacts}


def truth_state(pieces):
    return [{"id": p.id, "name": p.name, "team": p.team, "px": p.px,
             "pos": [round(p.pos[0], 3), round(p.pos[1], 3)],
             "facing_deg": round(p.facing_deg, 1),
             "unit_state": p.unit_state,
             "deployed_weapon": p.deployed_weapon,
             "vehicle_model": p.vehicle_model,
             "attached_to": p.attached_to,
             "operated_by": p.operated_by,
             "cargo": p.cargo,
             "status_debuffs": sorted(p.status_debuffs.keys()),
             "roe": getattr(p, "roe", "return_fire"),
             "meta": dict(getattr(p, "meta", {}) or {}),
             "counter": p.counter_profile(),
             "persons": p.persons(),
             "decoy": p.decoy, "radio_silence": p.radio_silence, "org": round(p.mean_org(), 1)}
            for p in pieces]
