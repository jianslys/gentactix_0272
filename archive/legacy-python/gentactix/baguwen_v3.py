# -*- coding: utf-8 -*-
"""V3.1 双向“作战日志”体系、断连纯黑盒（断连直接没有东西）、中层指挥官节奏脚本与单指挥官权限内核。

核心原则（引擎内零 LLM）：
1. **单指挥官权限与读线规则**：
   - 任何玩家只能扮演一个指挥官；
   - 玩家只能画/写自己身份的线（`can_commander_write_line`）；
   - 远程只读**已连通的直属下级**的线（营长只读连长，不直接透视远程排长）；
   - 若指挥官物理跑到任意友军（包括其他营的排长/单位）**400m (0.4km / 2px)** 范围内，可直接现场读该友军的线。
2. **断连纯黑盒（断连直接没有东西）**：
   - 断连当步上级完全收不到该下级的任何上行报文（不积压队列、不延迟补发、不脑补填充）；
   - 断连瞬间触发一次红/黄感叹号 $k_C$ 漂移判定，断连下级延续最新收到的一条上级下行作战日志推演；
   - 若断连单位附近 400m 内有友军，可汲取其局部敌情/友军态势直喂 `autonomy_v3` 判定器（但不代为发报）。
3. **中层指挥官自治与司令部伴随（排 15m / 连 30m / 营 60m）**：
   - 按层级节拍汇总作战日志，聚拢敌情簇与战损，触发条件下向地形容器切分子 AO 与分兵种战线；
   - 指挥部实体严格随直属安全重心后方 1–3 格隐蔽伴随机动。
"""

from __future__ import annotations

import math

from . import autonomy_v3
from . import command_v2
from . import terrain_field as tf


def can_commander_write_line(player_commander_id, target_commander_id):
    """玩家指挥官严格只能画/写自己身份的线，严禁越权替上级或下级画线。"""
    return str(player_commander_id) == str(target_commander_id)


def get_commander_readable_scope(world, commander_id, proximity_km=0.4):
    """计算指定指挥官当前可见的全部线上权限范围：
    - 自己；
    - 通过指挥通联校验的直属下级（不跨级远程透视孙级）；
    - 指挥官物理位置 400m (0.4km) 内的一切同阵营友军指挥官与单位（哪怕来自别的营/团）。
    """
    cmd = world.commanders.get(commander_id) if hasattr(world, "commanders") else None
    if cmd is None or not cmd.alive:
        return {
            "own_commander_id": commander_id,
            "readable_commander_ids": set(),
            "readable_piece_ids": set(),
            "proximity_commander_ids": set(),
            "proximity_piece_ids": set(),
            "connected_direct_subordinates": [],
            "disconnected_direct_subordinates": [],
        }

    readable_cmdrs = {cmd.id}
    readable_pieces = set()
    prox_cmdrs = set()
    prox_pieces = set()
    connected_subs = []
    disconnected_subs = []
    g = world.grid

    for sid in (cmd.subordinate_ids or []):
        if sid in world.commanders:
            sub_c = world.commanders[sid]
            ok, reason = command_v2.validate_commander_link(cmd, sub_c, g, pieces=world.pieces)
            if ok:
                readable_cmdrs.add(sid)
                connected_subs.append({"id": sid, "kind": "commander", "via": reason})
            else:
                disconnected_subs.append({"id": sid, "kind": "commander", "reason": reason})
        elif sid in world.pieces:
            sub_p = world.pieces[sid]
            ok, reason = command_v2.validate_command_link(cmd, sub_p, g, pieces=world.pieces)
            if ok:
                readable_pieces.add(sid)
                connected_subs.append({"id": sid, "kind": "piece", "via": reason})
            else:
                disconnected_subs.append({"id": sid, "kind": "piece", "reason": reason})

    # 物理 400m 现场直读：即使跨营、跨团、即使无线电断连/不同信道，只要肉身走到附近即可直接读其线
    for other_c in world.commanders.values():
        if not other_c.alive or other_c.team != cmd.team or other_c.id == cmd.id:
            continue
        if g.distance_km(cmd.px, other_c.px) <= proximity_km + 1e-6:
            readable_cmdrs.add(other_c.id)
            prox_cmdrs.add(other_c.id)

    for other_p in world.pieces.values():
        if other_p.team != cmd.team or other_p.persons() <= 0:
            continue
        if g.distance_km(cmd.px, other_p.px) <= proximity_km + 1e-6:
            readable_pieces.add(other_p.id)
            prox_pieces.add(other_p.id)

    return {
        "own_commander_id": cmd.id,
        "readable_commander_ids": readable_cmdrs,
        "readable_piece_ids": readable_pieces,
        "proximity_commander_ids": prox_cmdrs,
        "proximity_piece_ids": prox_pieces,
        "connected_direct_subordinates": connected_subs,
        "disconnected_direct_subordinates": disconnected_subs,
    }


def filter_readable_lines(world, commander_id, lines, proximity_km=0.4):
    """过滤地图线路集合，仅保留玩家指挥官权限可读的线。"""
    scope = get_commander_readable_scope(world, commander_id, proximity_km=proximity_km)
    allowed = scope["readable_commander_ids"] | scope["readable_piece_ids"]
    out = []
    for ln in (lines or []):
        owner = ln.get("commander_id") or ln.get("owner_id") or ln.get("piece_id") or ln.get("unit_id")
        if owner in allowed:
            out.append(ln)
    return out


def is_piece_connected_to_direct_commander(world, piece):
    """判断棋子是否与其直属最低层指挥官连通（若无直属指挥官返回 True）。"""
    direct = command_v2.direct_commander_for(piece, world.commanders) if hasattr(world, "commanders") else None
    if direct is None:
        return True, "no_direct_commander"
    return command_v2.validate_command_link(direct, piece, world.grid, pieces=world.pieces)


def harvest_nearby_friendly_intel(world, piece, radius_km=0.4):
    """断连自治友军交互：触发 k_C 漂移判定；断连单位用空间桶从 400m 内友军汲取局部敌情与友军状态（不代报）。"""
    ok, _ = is_piece_connected_to_direct_commander(world, piece)
    piece.meta["is_disconnected"] = not ok
    disc_info = autonomy_v3.evaluate_disconnection_kc_drift(world, piece, not ok)
    if ok:
        return {
            "disconnected": False,
            "harvested_enemies": 0,
            "harvested_friendlies": 0,
            "k_C": disc_info["k_C"],
            "exclamation_buff": disc_info["exclamation_buff"],
        }

    g = world.grid
    sidx = autonomy_v3._resolve_spatial_index(world)
    enemies_map = {}
    friendlies_list = []
    cand_friendlies = (
        sidx.query_radius_km(piece.pos, radius_km, team=piece.team, exclude_id=piece.id)
        if sidx is not None
        else [
            (fp, g.distance_km(piece.pos, fp.pos))
            for fp in world.pieces.values()
            if fp.id != piece.id and fp.team == piece.team and fp.persons() > 0
        ]
    )
    rumor_count = 0
    for fp, d_km in cand_friendlies:
        if fp.persons() <= 0 or d_km > radius_km + 1e-6:
            continue
        fp_connected, _ = is_piece_connected_to_direct_commander(world, fp)
        fp_was_disc = bool((fp.meta or {}).get("is_disconnected", False)) or (not fp_connected)
        friendlies_list.append({
            "id": fp.id,
            "px": list(fp.px),
            "persons": fp.persons(),
            "action": ((fp.meta or {}).get("autonomy_state") or {}).get("action", "hold_position"),
            "disconnected": fp_was_disc,
        })
        # 仅吸收友军自身 400m 目视接触（不透传全战区无线电情报，保持物理真实）
        prev_disc = (fp.meta or {}).get("is_disconnected", False)
        fp.meta["is_disconnected"] = True
        try:
            fp_seen = autonomy_v3.gather_unit_known_enemies(world, fp, sight_km=radius_km, spatial_index=sidx)
        finally:
            fp.meta["is_disconnected"] = prev_disc
        for ct in fp_seen:
            eid = ct["id"]
            if eid not in enemies_map:
                ct_copy = dict(ct)
                ct_copy["source"] = f"friendly_400m:{fp.id}"
                if fp_was_disc:
                    # 真实战争摩擦：两条断连单位近距互通流言时产生情绪与规模放大（封顶 3x）
                    orig_s = float(ct_copy.get("base_strength", ct_copy.get("strength", 10.0)))
                    ct_copy["base_strength"] = orig_s
                    ct_copy["strength"] = round(min(3.0 * orig_s, float(ct_copy.get("strength", orig_s)) * 1.30), 3)
                    ct_copy["rumor_amplified"] = True
                    rumor_count += 1
                enemies_map[eid] = ct_copy

    piece.meta["local_shared_intel"] = {
        "enemies": enemies_map,
        "friendlies": friendlies_list,
        "rumor_amplified_count": rumor_count,
        "updated_clock_hours": round(float(getattr(world, "clock_hours", 0.0)), 4),
    }
    return {
        "disconnected": True,
        "harvested_enemies": len(enemies_map),
        "harvested_friendlies": len(friendlies_list),
        "rumor_amplified": rumor_count > 0,
        "rumor_amplified_count": rumor_count,
        "k_C": disc_info["k_C"],
        "exclamation_buff": disc_info["exclamation_buff"],
    }


def generate_squad_5m_baguwen(world, piece):
    """生成纯 Python 模板化的 5 分钟上行作战日志（无 LLM）。"""
    g = world.grid
    sidx = autonomy_v3._resolve_spatial_index(world)
    if sidx is not None:
        raw_e = sidx.query_radius_km(piece.pos, 0.4, exclude_team=piece.team, exclude_id=piece.id)
        enemies_400 = sorted(
            [
                {"id": ep.id, "px": list(ep.px), "persons": ep.persons(), "pos": [round(ep.pos[0], 3), round(ep.pos[1], 3)]}
                for ep, d_km in raw_e
                if ep.persons() > 0 and d_km <= 0.4 + 1e-6
            ],
            key=lambda x: x["id"],
        )
        raw_f = sidx.query_radius_km(piece.pos, 0.4, team=piece.team, exclude_id=piece.id)
        friendlies_400 = sorted(
            [
                {"id": fp.id, "px": list(fp.px), "persons": fp.persons()}
                for fp, d_km in raw_f
                if fp.persons() > 0 and d_km <= 0.4 + 1e-6
            ],
            key=lambda x: x["id"],
        )
    else:
        enemies_400 = [
            {"id": ep.id, "px": list(ep.px), "persons": ep.persons(), "pos": [round(ep.pos[0], 3), round(ep.pos[1], 3)]}
            for ep in sorted(world.pieces.values(), key=lambda x: x.id)
            if ep.team != piece.team and ep.persons() > 0 and g.distance_km(piece.pos, ep.pos) <= 0.4 + 1e-6
        ]
        friendlies_400 = [
            {"id": fp.id, "px": list(fp.px), "persons": fp.persons()}
            for fp in sorted(world.pieces.values(), key=lambda x: x.id)
            if fp.id != piece.id and fp.team == piece.team and fp.persons() > 0 and g.distance_km(piece.pos, fp.pos) <= 0.4 + 1e-6
        ]

    mods = world.event_manager.piece_modifiers(piece, world=world) if getattr(world, "event_manager", None) else {}
    active_locks = [k for k in ("hold_still", "forbid_move", "forbid_fire", "forbid_deploy", "forbid_rest") if mods.get(k)]
    auto_st = (piece.meta or {}).get("autonomy_state") or {}
    action_now = auto_st.get("action") or ("engaging" if enemies_400 else "hold_position")
    atomic_action = auto_st.get("atomic_action", "HOLD")
    k_c = round(float((piece.meta or {}).get("k_C", autonomy_v3.KC_BASE)), 3)
    excl = (piece.meta or {}).get("exclamation_buff")
    d5_p = int((piece.meta or {}).get("delta_persons_5m", 0))
    d5_org = round(float((piece.meta or {}).get("delta_org_5m", 0.0)), 1)
    ao = (piece.meta or {}).get("active_order") or {}
    next_intent = auto_st.get("action") or ao.get("doctrine", "hold_position")
    move_tgt = auto_st.get("move_target") or ao.get("target_px")
    cp = piece.counter_profile()

    text = (
        f"【5分上行作战日志】单位={piece.name}({piece.id}) 层级={piece.level} 限制={cp.get('restriction_tier')} "
        f"| 坐标=({piece.px[0]},{piece.px[1]}) "
        f"| 400m目视敌={len(enemies_400)} 友={len(friendlies_400)} "
        f"| 原子动作={atomic_action} 状态={action_now} 锁={','.join(active_locks) if active_locks else '无'} 姿态={piece.unit_state} "
        f"| 现存={piece.persons()}人/组织度={round(piece.mean_org(), 1)} 5分Delta=({d5_p:+d}人,{d5_org:+.1f}组织度) "
        + (f"| 断连={excl}(kC={k_c}) " if excl else "")
        + f"| 未来5分意图={next_intent}" + (f"->目标{tuple(move_tgt)}" if move_tgt else "")
    )
    return {
        "piece_id": piece.id,
        "name": piece.name,
        "level": piece.level,
        "coord": list(piece.px),
        "enemies_400m": enemies_400,
        "friendlies_400m": friendlies_400,
        "atomic_action": atomic_action,
        "state": action_now,
        "unit_state": piece.unit_state,
        "locks": active_locks,
        "k_C": k_c,
        "exclamation_buff": excl,
        "strength_persons": piece.persons(),
        "org": round(piece.mean_org(), 1),
        "delta_5m": {"persons": d5_p, "org": d5_org},
        "next_5m_intent": next_intent,
        "move_target": move_tgt,
        "text": text,
    }


generate_squad_5m_combat_log = generate_squad_5m_baguwen


def _cluster_enemy_contacts(contacts, cluster_radius_px=3.0):
    """将上行作战日志中的敌军目视接触按空间近邻聚合成敌情簇。"""
    dedup = {}
    for ct in contacts:
        eid = ct.get("id")
        if eid and eid not in dedup:
            dedup[eid] = ct
    clusters = []
    for eid in sorted(dedup.keys()):
        ct = dedup[eid]
        px = ct.get("px") or ct.get("coord")
        if not px or len(px) < 2:
            continue
        r, c = float(px[0]), float(px[1])
        st = max(1.0, float(ct.get("persons", 8.0)))
        placed = False
        for cl in clusters:
            cr, cc = cl["center_px"]
            if math.hypot(r - cr, c - cc) <= cluster_radius_px:
                tot = cl["total_strength"] + st
                cl["center_px"] = [
                    round((cr * cl["total_strength"] + r * st) / tot, 2),
                    round((cc * cl["total_strength"] + c * st) / tot, 2),
                ]
                cl["total_strength"] = round(tot, 1)
                cl["contact_ids"].append(eid)
                cl["count"] += 1
                placed = True
                break
        if not placed:
            clusters.append({
                "center_px": [round(r, 2), round(c, 2)],
                "total_strength": round(st, 1),
                "contact_ids": [eid],
                "count": 1,
            })
    clusters.sort(key=lambda x: (-x["total_strength"], x["contact_ids"][0]))
    return clusters


def collect_commander_uplink_baguwen(world, commander_id, _visited=None):
    """上级汇总下级上行作战日志。严格执行「断连直接没有东西」：
    断连下级当步完全不产生任何记录——不进队列、不延迟补发、不脑补估计。
    """
    if _visited is None:
        _visited = set()
    if commander_id in _visited:
        return {
            "commander_id": commander_id, "reports": [], "omitted_disconnected_ids": [],
            "enemy_clusters": [], "own_casualty_ratio_5m": 0.0, "text": "",
        }
    _visited.add(commander_id)

    cmd = world.commanders.get(commander_id)
    if cmd is None or not cmd.alive:
        return {
            "commander_id": commander_id, "reports": [], "omitted_disconnected_ids": [],
            "enemy_clusters": [], "own_casualty_ratio_5m": 0.0, "text": "",
        }

    reports = []
    omitted = []
    all_enemies = []
    total_persons = 0
    total_loss_5m = 0
    g = world.grid
    for sid in sorted(cmd.subordinate_ids or []):
        if sid in world.pieces:
            sub_p = world.pieces[sid]
            if sub_p.persons() <= 0 and not getattr(sub_p, "vehicle_model", None):
                continue
            ok, _reason = command_v2.validate_command_link(cmd, sub_p, g, pieces=world.pieces)
            if not ok:
                # 断连纯黑盒：直接没有东西
                omitted.append(sid)
                continue
            r_sq = generate_squad_5m_baguwen(world, sub_p)
            reports.append(r_sq)
            all_enemies.extend(r_sq.get("enemies_400m") or [])
            total_persons += int(r_sq.get("strength_persons", 0))
            dp = int((r_sq.get("delta_5m") or {}).get("persons", 0))
            if dp < 0:
                total_loss_5m += (-dp)
        elif sid in world.commanders:
            sub_c = world.commanders[sid]
            ok, _reason = command_v2.validate_commander_link(cmd, sub_c, g, pieces=world.pieces)
            if not ok:
                # 下级指挥官断连：直接没有东西
                omitted.append(sid)
                continue
            sub_pkg = collect_commander_uplink_baguwen(world, sid, _visited=_visited)
            omitted.extend(sub_pkg["omitted_disconnected_ids"])
            sub_cnt = len(sub_pkg["reports"])
            tot_p = sum(r.get("strength_persons", 0) for r in sub_pkg["reports"])
            total_persons += tot_p
            for cl in sub_pkg.get("enemy_clusters") or []:
                for eid in cl.get("contact_ids") or []:
                    all_enemies.append({"id": eid, "px": cl["center_px"], "persons": cl["total_strength"] / max(1, cl["count"])})
            sub_text = (
                f"【下级作战日志汇总】指挥官={sub_c.name}({sub_c.id}) 位置={tuple(sub_c.px)} "
                f"| 在连分队日志={sub_cnt}份 在位合计={tot_p}人\n" + sub_pkg["text"]
            ).strip()
            reports.append({
                "commander_id": sub_c.id,
                "name": sub_c.name,
                "coord": list(sub_c.px),
                "strength_persons": tot_p,
                "sub_reports": sub_pkg["reports"],
                "text": sub_text,
            })

    summary_lines = [r["text"] for r in reports]
    full_text = "\n".join(summary_lines)
    enemy_clusters = _cluster_enemy_contacts(all_enemies, cluster_radius_px=3.0)
    cas_ratio = float(total_loss_5m) / max(1.0, float(total_persons + total_loss_5m))
    pkg = {
        "commander_id": cmd.id,
        "commander_name": cmd.name,
        "echelon": cmd.echelon,
        "reports": reports,
        "omitted_disconnected_ids": omitted,
        "enemy_clusters": enemy_clusters,
        "own_casualty_ratio_5m": round(cas_ratio, 4),
        "text": full_text,
    }
    return pkg


collect_commander_uplink_combat_log = collect_commander_uplink_baguwen


def issue_downlink_baguwen_order(world, commander_id, order_spec, author_commander_id=None, _visited=None):
    """下达下行作战日志命令：强制检验单指挥官画线/写令权；支持同步写入 AO 与分兵种战线，连通下级规划动态代价路线；断连收不到。"""
    if author_commander_id is not None and not can_commander_write_line(author_commander_id, commander_id):
        return {
            "ok": False,
            "reason": "forbidden_write_other_commander_line",
            "author": author_commander_id,
            "target_commander": commander_id,
        }
    if _visited is None:
        _visited = set()
    if commander_id in _visited:
        return {"ok": False, "reason": "cycle_detected"}
    _visited.add(commander_id)

    cmd = world.commanders.get(commander_id)
    if cmd is None or not cmd.alive:
        return {"ok": False, "reason": "commander_unavailable"}

    if not hasattr(cmd, "meta") or cmd.meta is None:
        cmd.meta = {}
    spec = dict(order_spec or {})
    if author_commander_id is not None:
        cmd.meta["last_explicit_order_turn"] = int(getattr(world, "turn", 0))
        if spec.get("count_churn", True):
            record_commander_line_churn(world, cmd, amount=float(spec.get("churn_amount", 1.0)))

    targets = list(spec.get("targets") or cmd.subordinate_ids or [])
    target_px = spec.get("target_px") or spec.get("to")
    doctrine = str(spec.get("doctrine", spec.get("mode", "advance"))).lower()
    downlink_text = (
        f"【下行作战日志】发令={cmd.name}({cmd.id}) 准则={doctrine} "
        + (f"目标={tuple(target_px)} " if target_px is not None else "")
        + f"要旨={spec.get('intent', '按计划地形机动与接敌')}"
    )

    g = world.grid
    ao_receipt = None
    ao_spec = spec.get("ao_spec")
    zone_id = spec.get("zone_id")
    if isinstance(ao_spec, dict) and g is not None:
        cells = command_v2.build_ao_cells(ao_spec, g, world.pieces)
        zid = ao_spec.get("id") or ao_spec.get("zone_id") or zone_id or f"ao-{cmd.id}-log"
        zone_id = zid
        z_obj = command_v2.OperationalZone(
            zid, cmd.id, ao_spec.get("team") or cmd.team,
            ao_spec.get("echelon") or cmd.echelon, cells,
            activate_turn=int(ao_spec.get("activate_turn", getattr(world, "turn", 0))),
            frontlines=ao_spec.get("frontlines"),
        )
        ok_z, rsn_z = command_v2.register_zone(
            world.ao_zones, z_obj, world.commanders,
            current_turn=int(getattr(world, "turn", 0)),
            pieces=world.pieces, grid=g,
        )
        ao_receipt = {"zone_id": zid, "ok": ok_z, "reason": rsn_z}

    written_frontlines = []
    if g is not None:
        for idx, fl in enumerate(spec.get("typed_frontlines") or []):
            if not isinstance(fl, dict) or not fl.get("points_px"):
                continue
            lid = str(fl.get("line_id") or f"fl-{cmd.id}-{idx}")
            g.write_typed_frontline(
                lid, cmd.team, fl["points_px"],
                unit_type_mask=int(fl.get("unit_type_mask", tf.LINE_MASK_ALL)),
                attraction_weight=float(fl.get("attraction_weight", 1.0)),
                front_normal=fl.get("front_normal"),
                commander_id=cmd.id,
                zone_id=fl.get("zone_id") or zone_id,
                spread_radius_px=int(fl.get("spread_radius_px", 4)),
                line_role=str(fl.get("line_role", tf.LINE_ROLE_DEPLOY)),
                source_deploy_line_id=fl.get("source_deploy_line_id"),
                target_deploy_line_id=fl.get("target_deploy_line_id"),
            )
            written_frontlines.append(lid)

    delivered = []
    disconnected = []
    dyn_cost_params = spec.get("dynamic_cost_params")

    for tid in targets:
        if tid in world.pieces:
            p = world.pieces[tid]
            if p.team != cmd.team:
                continue
            ok, reason = command_v2.validate_command_link(cmd, p, g, pieces=world.pieces)
            if not ok:
                disconnected.append({"id": tid, "reason": reason})
                continue
            ord_payload = {
                "order_id": spec.get("order_id") or f"ord-{cmd.id}-{tid}",
                "issuer": cmd.id,
                "zone_id": zone_id,
                "target_px": target_px or list(p.px),
                "doctrine": doctrine,
                "doctrine_weight": spec.get("doctrine_weight"),
                "text": downlink_text,
            }
            route = autonomy_v3.plan_order_route(world, p, ord_payload, dynamic_cost_params=dyn_cost_params)
            delivered.append({
                "id": tid,
                "via": reason,
                "route_len": len(route.get("path") or []),
                "reachable": route.get("reachable", True),
            })
        elif tid in world.commanders:
            sub_c = world.commanders[tid]
            ok, reason = command_v2.validate_commander_link(cmd, sub_c, g, pieces=world.pieces)
            if not ok:
                disconnected.append({"id": tid, "reason": reason})
                continue
            sub_res = issue_downlink_baguwen_order(world, tid, spec, author_commander_id=None, _visited=_visited)
            delivered.extend(sub_res.get("delivered", []))
            disconnected.extend(sub_res.get("disconnected", []))

    return {
        "ok": True,
        "commander_id": cmd.id,
        "text": downlink_text,
        "ao_receipt": ao_receipt,
        "typed_frontlines": written_frontlines,
        "delivered": delivered,
        "disconnected": disconnected,
    }


issue_downlink_combat_log_order = issue_downlink_baguwen_order


# ==========================================
# 🧠 V3.2 全层级同构纯数学递归指挥官 + 传达延迟 τ + 三稳重闸门（Zero-LLM）
# ==========================================
MID_TIER_CADENCE_TICKS = {
    "platoon": 3,      # 15m (3 × 5m)
    "company": 6,      # 30m (6 × 5m)
    "battalion": 12,   # 60m (12 × 5m)
    "regiment": 36,    # 180m (36 × 5m)
    "brigade": 48,     # 240m (48 × 5m)
    "division": 72,    # 360m (72 × 5m)
}

PROPAGATION_DELAY_MINUTES = {
    "division": 30.0,
    "brigade": 25.0,
    "regiment": 20.0,
    "battalion": 10.0,
    "company": 5.0,
    "platoon": 2.0,
}

HARD_COOLDOWN_MINUTES = {
    "platoon": 20.0,
    "company": 45.0,
    "battalion": 90.0,
    "regiment": 180.0,
    "brigade": 240.0,
    "division": 360.0,
}


def record_commander_line_churn(world, cmd, amount=1.0):
    """玩家频繁改线触发 ORDER_CHURN 累积与电台暴露度暴涨（不直接封锁画线操作，代之以自然惩罚）。"""
    if cmd is None:
        return 0.0
    amt = max(0.0, float(amount))
    cmd.order_churn = round(float(getattr(cmd, "order_churn", 0.0)) + amt, 4)
    if world is not None and getattr(world, "grid", None) is not None and cmd.order_churn > 1.0:
        world.grid.add_exposure(cmd.px, 2.5 * (cmd.order_churn - 1.0), radius_px=1.0)
    return cmd.order_churn


def compute_commander_propagation_delay_min(world, cmd):
    """计算直属下级传达延迟 τ = τ_0 × τ_mult（受朝令夕改 ORDER_CHURN_DEBUFF 膨胀与作战会议同步 buff 抵消）。"""
    base_min = float(PROPAGATION_DELAY_MINUTES.get(getattr(cmd, "echelon", "company"), 10.0))
    churn = float(getattr(cmd, "order_churn", 0.0))
    sync_h = float(getattr(cmd, "war_council_sync_hours", 0.0))
    excess = max(0.0, churn - 1.0)
    tau_mult = 1.0 + 0.35 * excess
    if sync_h > 1e-9:
        tau_mult = max(0.85, tau_mult - 0.35 * min(1.5, max(1.0, excess)))
    return round(base_min * tau_mult, 3)


def _polyline_midpoint(points_px):
    if not points_px:
        return [0.0, 0.0]
    sr = sum(float(p[0]) for p in points_px) / len(points_px)
    sc = sum(float(p[1]) for p in points_px) / len(points_px)
    return [sr, sc]


def _slice_polyline_by_fraction(points_px, frac0, frac1):
    """按弧长比例 [frac0, frac1] 从父战线切出连续子折线段落。"""
    pts = [[float(p[0]), float(p[1])] for p in (points_px or [])]
    if len(pts) <= 1:
        return [[int(round(p[0])), int(round(p[1]))] for p in pts]
    cum = [0.0]
    for i in range(1, len(pts)):
        seg = math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1])
        cum.append(cum[-1] + seg)
    total = cum[-1]
    if total <= 1e-6:
        return [[int(round(pts[0][0])), int(round(pts[0][1]))], [int(round(pts[-1][0])), int(round(pts[-1][1]))]]

    def _interp(s):
        s = max(0.0, min(total, s))
        for i in range(1, len(cum)):
            if s <= cum[i] + 1e-9:
                span = cum[i] - cum[i - 1]
                t = (s - cum[i - 1]) / span if span > 1e-9 else 0.0
                r = pts[i - 1][0] + t * (pts[i][0] - pts[i - 1][0])
                c = pts[i - 1][1] + t * (pts[i][1] - pts[i - 1][1])
                return [int(round(r)), int(round(c))]
        return [int(round(pts[-1][0])), int(round(pts[-1][1]))]

    s0 = max(0.0, min(1.0, float(frac0))) * total
    s1 = max(float(frac0), min(1.0, float(frac1))) * total
    out = [_interp(s0)]
    for i in range(1, len(pts) - 1):
        if s0 + 1e-4 < cum[i] < s1 - 1e-4:
            out.append([int(round(pts[i][0])), int(round(pts[i][1]))])
    p_end = _interp(s1)
    if p_end != out[-1] or len(out) == 1:
        out.append(p_end)
    return out


def _commander_combat_power(world, cmd):
    """递归汇总指挥官名下存活连通作战战力权重。"""
    seen_pieces = set()

    def _walk(c_id, depth=0):
        if depth > 6 or c_id not in world.commanders:
            return 0.0
        c = world.commanders[c_id]
        if not c.alive:
            return 0.0
        total = 0.0
        for sid in (c.subordinate_ids or []):
            if sid in world.pieces and sid not in seen_pieces:
                seen_pieces.add(sid)
                sp = world.pieces[sid]
                if sp.team == c.team and sp.persons() > 0:
                    total += autonomy_v3._piece_strength_estimate(sp)
            elif sid in world.commanders:
                total += _walk(sid, depth + 1)
        return total

    return max(10.0, _walk(cmd.id))


def partition_parent_line_by_combat_power(world, cmd, parent_points_px):
    """全层级同构纯数学画线切分：按连通直属下级战力权重等比例切分母战线；断连留下的缺口由连通友军自然拉长补齐。"""
    if not parent_points_px or cmd is None:
        return {}
    g = getattr(world, "grid", None)
    conn_children = []
    for sid in (cmd.subordinate_ids or []):
        sc = world.commanders.get(sid)
        if sc is None or not sc.alive or sc.team != cmd.team:
            continue
        ok, _ = command_v2.validate_commander_link(cmd, sc, g, pieces=world.pieces)
        if ok:
            conn_children.append(sc)
    if not conn_children:
        return {}

    p0, p1 = parent_points_px[0], parent_points_px[-1]
    tr = float(p1[0]) - float(p0[0])
    tc = float(p1[1]) - float(p0[1])
    tm = math.hypot(tr, tc) or 1.0
    ur, uc = tr / tm, tc / tm

    def _proj(sc):
        pos = (sc.meta or {}).get("pos") or sc.px
        return float(pos[0]) * ur + float(pos[1]) * uc

    ordered = sorted(conn_children, key=lambda sc: (_proj(sc), sc.id))
    weights = [(_commander_combat_power(world, sc), sc) for sc in ordered]
    tot_w = sum(w for w, _ in weights) or float(len(weights))
    acc = 0.0
    result = {}
    for w, sc in weights:
        f0 = acc / tot_w
        acc += w
        f1 = acc / tot_w
        seg = _slice_polyline_by_fraction(parent_points_px, f0, f1)
        if not hasattr(sc, "meta") or sc.meta is None:
            sc.meta = {}
        sc.meta["assigned_parent_line_pts"] = seg
        result[sc.id] = seg
    return result


def create_bridging_move_line_if_needed(
    grid, cmd, old_points_px, new_points_px, new_deploy_line_id,
    old_deploy_line_id=None, unit_type_mask=tf.LINE_MASK_ALL, zone_id=None,
):
    """新部署线偏离旧部署线中心 > 3.0 px 时，自动生成带源部署线溯源与目标锚点的桥接 MOVE_LINE。"""
    if grid is None or not old_points_px or not new_points_px:
        return None
    m_old = _polyline_midpoint(old_points_px)
    m_new = _polyline_midpoint(new_points_px)
    shift_px = math.hypot(m_new[0] - m_old[0], m_new[1] - m_old[1])
    if shift_px <= 3.0:
        return None
    lid = f"auto-bridge-{cmd.id}"
    pts = [
        [int(round(m_old[0])), int(round(m_old[1]))],
        [int(round(m_new[0])), int(round(m_new[1]))],
    ]
    return grid.write_typed_frontline(
        lid, cmd.team, pts,
        unit_type_mask=int(unit_type_mask),
        attraction_weight=1.35,
        commander_id=cmd.id,
        zone_id=zone_id,
        spread_radius_px=3,
        line_role=tf.LINE_ROLE_MOVE,
        source_deploy_line_id=old_deploy_line_id,
        target_deploy_line_id=new_deploy_line_id,
    )


def queue_downlink_to_connected_subordinates(world, cmd, order_payload):
    """将上级指令按传达延迟 τ 排入下行延迟队列；仅物理连通直属下级进入队列，断连者直接略过。"""
    if world is None or cmd is None:
        return {"queued": [], "omitted_disconnected_ids": [], "delay_min": 0.0}
    g = getattr(world, "grid", None)
    delay_min = compute_commander_propagation_delay_min(world, cmd)
    now_h = float(getattr(world, "clock_hours", 0.0))
    deliver_at_h = now_h + delay_min / 60.0
    if not hasattr(world, "meta") or world.meta is None:
        world.meta = {}
    q = world.meta.setdefault("pending_downlink_queue", [])

    queued = []
    omitted = []
    payload_copy = dict(order_payload or {})
    for sid in (cmd.subordinate_ids or []):
        if sid in world.commanders:
            sc = world.commanders[sid]
            if not sc.alive or sc.team != cmd.team:
                continue
            ok, reason = command_v2.validate_commander_link(cmd, sc, g, pieces=world.pieces)
            if not ok:
                omitted.append({"id": sid, "reason": reason})
                continue
            item = {
                "target_type": "commander",
                "target_id": sid,
                "parent_id": cmd.id,
                "deliver_at_clock_hours": round(deliver_at_h, 6),
                "delay_min": delay_min,
                "payload": dict(payload_copy),
            }
            q.append(item)
            queued.append(sid)
        elif sid in world.pieces:
            sp = world.pieces[sid]
            if sp.team != cmd.team or sp.persons() <= 0:
                continue
            ok, reason = command_v2.validate_command_link(cmd, sp, g, pieces=world.pieces)
            if not ok:
                omitted.append({"id": sid, "reason": reason})
                continue
            item = {
                "target_type": "piece",
                "target_id": sid,
                "parent_id": cmd.id,
                "deliver_at_clock_hours": round(deliver_at_h, 6),
                "delay_min": delay_min,
                "payload": dict(payload_copy),
            }
            q.append(item)
            queued.append(sid)
    return {"queued": queued, "omitted_disconnected_ids": omitted, "delay_min": delay_min}


def tick_pending_downlink_orders(world, dt_hours=0.0, advance_clock=False):
    """推进并派发到达传达延迟 τ 的直属下行命令队列。"""
    if world is None:
        return []
    if advance_clock and dt_hours and float(dt_hours) > 0.0:
        world.clock_hours = float(getattr(world, "clock_hours", 0.0)) + float(dt_hours)
    meta = getattr(world, "meta", None) or {}
    q = meta.get("pending_downlink_queue") or []
    if not q:
        return []
    now_h = float(getattr(world, "clock_hours", 0.0))
    remaining = []
    delivered = []
    for item in q:
        if float(item.get("deliver_at_clock_hours", 0.0)) <= now_h + 1e-6:
            ttype = item.get("target_type")
            tid = item.get("target_id")
            pl = item.get("payload") or {}
            if ttype == "commander" and tid in world.commanders:
                sc = world.commanders[tid]
                if not hasattr(sc, "meta") or sc.meta is None:
                    sc.meta = {}
                sc.meta["parent_order_seq"] = int(sc.meta.get("parent_order_seq", 0)) + 1
                if pl.get("objective_px") is not None:
                    sc.meta["mid_tier_objective"] = list(pl["objective_px"])
                if pl.get("child_line_pts") and tid in pl["child_line_pts"]:
                    sc.meta["assigned_parent_line_pts"] = pl["child_line_pts"][tid]
            elif ttype == "piece" and tid in world.pieces:
                sp = world.pieces[tid]
                if pl.get("objective_px") is not None or pl.get("doctrine"):
                    autonomy_v3.plan_order_route(world, sp, {
                        "order_id": pl.get("order_id", f"dl-{item.get('parent_id')}-{tid}"),
                        "zone_id": pl.get("zone_id"),
                        "target_px": pl.get("objective_px"),
                        "doctrine": pl.get("doctrine", "advance"),
                    })
            delivered.append(item)
        else:
            remaining.append(item)
    meta["pending_downlink_queue"] = remaining
    return delivered


MID_TIER_CADENCE_TICKS = {
    "platoon": 3,      # 15m (3 × 5m)
    "company": 6,      # 30m (6 × 5m)
    "battalion": 12,   # 60m (12 × 5m)
    "regiment": 36,    # 180m (36 × 5m)
    "brigade": 48,     # 240m (48 × 5m)
    "division": 72,    # 360m (72 × 5m)
}


def _collect_connected_subordinate_centroid(world, cmd):
    """统计指定指挥官已连通直属下级的安全质心、成员与平均组织度。"""
    g = world.grid
    coords = []
    orgs = []
    connected_pieces = []
    connected_cmdrs = []
    for sid in (cmd.subordinate_ids or []):
        if sid in world.pieces:
            p = world.pieces[sid]
            if p.team != cmd.team or (p.persons() <= 0 and not getattr(p, "vehicle_model", None)):
                continue
            ok, _ = command_v2.validate_command_link(cmd, p, g, pieces=world.pieces)
            if ok:
                connected_pieces.append(p)
                coords.append((float(p.pos[0]), float(p.pos[1])))
                orgs.append(float(p.mean_org()))
        elif sid in world.commanders:
            sc = world.commanders[sid]
            if not sc.alive or sc.team != cmd.team:
                continue
            ok, _ = command_v2.validate_commander_link(cmd, sc, g, pieces=world.pieces)
            if ok:
                connected_cmdrs.append(sc)
                spos = (sc.meta or {}).get("pos") or sc.px
                coords.append((float(spos[0]), float(spos[1])))
    if coords:
        cr = sum(c[0] for c in coords) / len(coords)
        cc = sum(c[1] for c in coords) / len(coords)
    else:
        cr, cc = float(cmd.px[0]), float(cmd.px[1])
    mean_org = (sum(orgs) / len(orgs)) if orgs else 75.0
    return (cr, cc), connected_pieces, connected_cmdrs, mean_org


def _partition_cells_disjoint(cells, child_cmdrs, max_areas):
    """将父级区域单元无重叠切分给各直属下级指挥官（主轴分段 + 容量上限裁剪）。"""
    if not child_cmdrs or not cells:
        return {}
    sorted_children = sorted(child_cmdrs, key=lambda c: (c.px[0], c.px[1], c.id))
    rows = [rc[0] for rc in cells]
    cols = [rc[1] for rc in cells]
    span_r = (max(rows) - min(rows)) if rows else 0
    span_c = (max(cols) - min(cols)) if cols else 0
    axis = 0 if span_r >= span_c else 1
    ordered_children = sorted(sorted_children, key=lambda c: (c.px[axis], c.id))
    ordered_cells = sorted(cells, key=lambda rc: (rc[axis], rc[1 - axis]))

    n_ch = len(ordered_children)
    buckets = {c.id: set() for c in ordered_children}
    total = len(ordered_cells)
    for idx, rc in enumerate(ordered_cells):
        bi = min(n_ch - 1, (idx * n_ch) // max(1, total))
        cid = ordered_children[bi].id
        if len(buckets[cid]) < int(max_areas.get(cid, total)):
            buckets[cid].add(rc)
    return buckets


def evaluate_mid_tier_commander(world, commander_id, tick_index=0, force=False, subtick_hours=5.0 / 60.0):
    """全层级同构纯数学递归指挥官脚本（排/连/营/团/旅/师）：
    1) 按层级节拍 + 三稳重闸门（硬冷却/默认静默+触发门槛/几何死区）决策；
    2) 按连通下级战力切分母战线弧段与互不重叠子 AO（断连缺口由连通友军补位）；
    3) 自动生成 >3px 位移桥接 MOVE_LINE 与本级全局行动线 GLOBAL_ACTION_LINE，并按延迟 τ 下发连通队列。
    """
    cmd = world.commanders.get(commander_id)
    if cmd is None or not cmd.alive:
        return {"skipped": True, "reason": "commander_unavailable"}
    cadence = MID_TIER_CADENCE_TICKS.get(cmd.echelon)
    if cadence is None:
        return {"skipped": True, "reason": f"non_mid_tier:{cmd.echelon}"}
    if not force and (int(tick_index) % cadence != 0):
        return {"skipped": True, "reason": "off_cadence", "cadence": cadence}

    if not hasattr(cmd, "meta") or cmd.meta is None:
        cmd.meta = {}
    if not force and any(
        it.get("parent_id") == cmd.id
        for it in ((getattr(world, "meta", None) or {}).get("pending_downlink_queue") or [])
    ):
        return {"skipped": True, "reason": "downlink_in_flight"}

    # 闸门一：硬冷却时间（platoon>=20m / company>=45m / battalion>=90m / regiment>=180m / brigade>=240m / division>=360m）
    last_clock = cmd.meta.get("last_auto_order_clock_hours")
    last_tick = cmd.meta.get("last_auto_order_tick")
    cooldown_min = float(HARD_COOLDOWN_MINUTES.get(cmd.echelon, 45.0))
    if not force and last_clock is not None:
        now_clock = float(getattr(world, "clock_hours", 0.0))
        if now_clock > float(last_clock) + 1e-6:
            elapsed_min = (now_clock - float(last_clock)) * 60.0
        elif last_tick is not None and int(tick_index) > int(last_tick):
            elapsed_min = float(int(tick_index) - int(last_tick)) * (float(subtick_hours) * 60.0)
        else:
            elapsed_min = 0.0
        if elapsed_min < cooldown_min - 1e-6:
            return {
                "skipped": True,
                "reason": "hard_cooldown",
                "triggered": False,
                "commander_id": cmd.id,
                "echelon": cmd.echelon,
                "elapsed_min": round(elapsed_min, 2),
                "cooldown_min": cooldown_min,
            }

    uplink = collect_commander_uplink_baguwen(world, commander_id)
    clusters = uplink.get("enemy_clusters") or []
    omitted_now = list(uplink.get("omitted_disconnected_ids") or [])
    centroid, conn_pieces, conn_cmdrs, mean_org = _collect_connected_subordinate_centroid(world, cmd)

    # 目标探测（父级下发线 / 父级指令 / 最强敌情簇 / 最近子级前缘）
    g = world.grid
    H, W = g.shape
    parent_order = cmd.meta.get("mid_tier_objective")
    explicit_turn = cmd.meta.get("last_explicit_order_turn")
    explicit_this_turn = (explicit_turn is not None and int(explicit_turn) == int(getattr(world, "turn", 0)))
    parent_seq_now = int(cmd.meta.get("parent_order_seq", 0))
    parent_seq_prev = int(cmd.meta.get("last_handled_parent_seq", 0))
    parent_order_fresh = parent_seq_now > parent_seq_prev

    inferred_tgt = None
    for sp in conn_pieces:
        ao_ord = (sp.meta or {}).get("active_order") or {}
        if ao_ord.get("target_px"):
            inferred_tgt = tuple(ao_ord["target_px"])
            break
    enemies_vis = [ep.pos for ep in world.pieces.values() if ep.team != cmd.team and ep.persons() > 0]
    if parent_order is not None and (parent_order_fresh or not clusters):
        main_enemy = (float(parent_order[0]), float(parent_order[1]))
    elif clusters:
        main_enemy = tuple(clusters[0]["center_px"])
    elif inferred_tgt is not None:
        main_enemy = inferred_tgt
    elif cmd.meta.get("assigned_parent_line_pts"):
        mp = _polyline_midpoint(cmd.meta["assigned_parent_line_pts"])
        main_enemy = (float(mp[0]), float(mp[1]))
    else:
        if enemies_vis:
            nearest_e = min(enemies_vis, key=lambda ep: math.hypot(ep[0] - centroid[0], ep[1] - centroid[1]))
            main_enemy = (float(nearest_e[0]), float(nearest_e[1]))
        else:
            main_enemy = (centroid[0], min(float(W - 1), centroid[1] + 4.0))

    initialized = bool(cmd.meta.get("mid_tier_initialized", False))
    prev_omitted = int(cmd.meta.get("prev_omitted_count", 0))
    prev_obj = cmd.meta.get("prev_objective_px")
    obj_changed = (prev_obj is not None and math.hypot(main_enemy[0] - prev_obj[0], main_enemy[1] - prev_obj[1]) >= 4.0)
    obj_reached = (parent_order is not None and math.hypot(centroid[0] - parent_order[0], centroid[1] - parent_order[1]) <= 1.5)
    heavy_casualty = (uplink.get("own_casualty_ratio_5m", 0.0) >= 0.20) or (mean_org < 42.0 and bool(conn_pieces))
    flank_disconnected = len(omitted_now) > prev_omitted

    # 闸门二：默认保持静默 + 触发门槛
    has_initial_stimulus = (
        bool(enemies_vis)
        or (parent_order is not None)
        or explicit_this_turn
        or parent_order_fresh
        or bool(cmd.meta.get("assigned_parent_line_pts"))
    )
    trigger_reasons = []
    if not initialized and has_initial_stimulus:
        trigger_reasons.append("initial_deployment")
    if parent_order_fresh:
        trigger_reasons.append("parent_order_arrived")
    if explicit_this_turn and not initialized:
        trigger_reasons.append("explicit_order")
    if obj_changed or obj_reached:
        trigger_reasons.append("objective_reached_or_changed")
    if heavy_casualty:
        trigger_reasons.append("heavy_casualties")
    if flank_disconnected:
        trigger_reasons.append("flank_disconnected")

    if not trigger_reasons and not force:
        return {
            "skipped": True,
            "reason": "silence_gate",
            "triggered": False,
            "commander_id": cmd.id,
            "echelon": cmd.echelon,
        }

    # 1. 计算前向法向量 N_line 与侧翼切向量 T_perp
    dr = float(main_enemy[0]) - centroid[0]
    dc = float(main_enemy[1]) - centroid[1]
    dn = math.hypot(dr, dc)
    nr, nc = (dr / dn, dc / dn) if dn > 1e-6 else (0.0, 1.0)
    tr_v, tc_v = -nc, nr  # 正交左/右翼切向量

    lead_dist = min(2.5, max(0.8, dn * 0.4))
    lead_r = centroid[0] + nr * lead_dist
    lead_c = centroid[1] + nc * lead_dist
    if parent_order is not None and (parent_order_fresh or not clusters):
        lead_r, lead_c = float(parent_order[0]), float(parent_order[1])
    if heavy_casualty and "objective_reached_or_changed" not in trigger_reasons:
        lead_r = centroid[0] - nr * 1.0
        lead_c = centroid[1] - nc * 1.0

    # 闸门三：几何死区（偏移 < 2.0 px 且方向角变化 < 15° 不触发重画线）
    last_lead = cmd.meta.get("last_lead_pt")
    last_norm = cmd.meta.get("last_front_normal")
    if initialized and not force and last_lead is not None and last_norm is not None and not (heavy_casualty or flank_disconnected):
        shift_px = math.hypot(float(lead_r) - float(last_lead[0]), float(lead_c) - float(last_lead[1]))
        dot_n = max(-1.0, min(1.0, float(nr) * float(last_norm[0]) + float(nc) * float(last_norm[1])))
        ang_deg = math.degrees(math.acos(dot_n))
        if shift_px < 2.0 and ang_deg < 15.0:
            cmd.meta["last_handled_parent_seq"] = parent_seq_now
            cmd.meta["prev_omitted_count"] = len(omitted_now)
            return {
                "skipped": True,
                "reason": "geometric_deadzone",
                "triggered": False,
                "commander_id": cmd.id,
                "echelon": cmd.echelon,
                "shift_px": round(shift_px, 3),
                "angle_deg": round(ang_deg, 2),
            }

    cmd.meta["mid_tier_initialized"] = True
    cmd.meta["prev_omitted_count"] = len(omitted_now)
    cmd.meta["prev_objective_px"] = [round(main_enemy[0], 2), round(main_enemy[1], 2)]
    cmd.meta["last_uplink_clusters"] = clusters
    cmd.meta["last_handled_parent_seq"] = parent_seq_now
    cmd.meta["last_auto_order_clock_hours"] = round(float(getattr(world, "clock_hours", 0.0)), 6)
    cmd.meta["last_auto_order_tick"] = int(tick_index)
    cmd.meta["last_lead_pt"] = [round(lead_r, 3), round(lead_c, 3)]
    cmd.meta["last_front_normal"] = [round(nr, 4), round(nc, 4)]

    # 2. 若有直属下级指挥官，按父防区或指挥半径内活动带切分互不重叠的子 AO
    parent_cells = set()
    for z in world.ao_zones.values():
        if z.commander_id == cmd.id and z.activate_turn <= getattr(world, "turn", 0):
            parent_cells |= set(z.cells)
    if not parent_cells:
        base_r = max(2, min(5, int(round(command_v2.effective_commander_radius_km(cmd, world.pieces, g) / (g.cell_m / 1000.0)))))
        cr0, cc0 = int(round((centroid[0] + main_enemy[0]) * 0.5)), int(round((centroid[1] + main_enemy[1]) * 0.5))
        for rr in range(max(0, cr0 - base_r), min(H, cr0 + base_r + 1)):
            for cc_i in range(max(0, cc0 - base_r), min(W, cc0 + base_r + 1)):
                parent_cells.add((rr, cc_i))

    child_zones_written = []
    if conn_cmdrs and parent_cells:
        for zid_old in list(cmd.meta.get("auto_child_zone_ids") or []):
            old_z = world.ao_zones.pop(zid_old, None)
            if old_z is not None:
                g.remove_ao_from_container(zid_old, team=old_z.team)
        max_areas = {
            sc.id: command_v2.effective_commander_max_area_px(sc, world.pieces, g)
            for sc in conn_cmdrs
        }
        buckets = _partition_cells_disjoint(parent_cells, conn_cmdrs, max_areas)
        new_zids = []
        for sc in conn_cmdrs:
            cset = buckets.get(sc.id) or set()
            if not cset:
                continue
            zid = f"auto-ao-{sc.id}"
            z_ch = command_v2.OperationalZone(
                zid, sc.id, sc.team, sc.echelon, cset,
                activate_turn=int(getattr(world, "turn", 0)),
            )
            ok_z, _ = command_v2.register_zone(
                world.ao_zones, z_ch, world.commanders,
                current_turn=int(getattr(world, "turn", 0)),
                pieces=world.pieces, grid=g,
            )
            if ok_z:
                new_zids.append(zid)
                child_zones_written.append(zid)
        cmd.meta["auto_child_zone_ids"] = new_zids

    # 3. 写入四类分兵种部署线 + 本级全局行动线 GLOBAL_ACTION_LINE + (>3px 自动桥接 MOVE_LINE)
    def _clamp_pt(rr, cc_f):
        return [
            max(0, min(H - 1, int(round(rr)))),
            max(0, min(W - 1, int(round(cc_f)))),
        ]

    old_assault_pts = cmd.meta.get("last_assault_pts")
    old_assault_id = cmd.meta.get("last_assault_line_id")
    deploy_seq = int(cmd.meta.get("deploy_line_seq", 0)) + 1
    cmd.meta["deploy_line_seq"] = deploy_seq

    use_assigned_pts = False
    if cmd.meta.get("assigned_parent_line_pts") and not force and len(cmd.meta["assigned_parent_line_pts"]) >= 2:
        amp = _polyline_midpoint(cmd.meta["assigned_parent_line_pts"])
        if math.hypot(amp[0] - lead_r, amp[1] - lead_c) <= 4.0:
            use_assigned_pts = True
    if use_assigned_pts:
        assault_pts = [list(pt) for pt in cmd.meta["assigned_parent_line_pts"]]
    else:
        assault_pts = [_clamp_pt(lead_r - 3.0 * tr_v, lead_c - 3.0 * tc_v), _clamp_pt(lead_r + 3.0 * tr_v, lead_c + 3.0 * tc_v)]

    for old_lid in list(cmd.meta.get("auto_frontline_ids") or []):
        g.remove_typed_frontline(old_lid)
    if cmd.meta.get("auto_global_line_id"):
        g.remove_typed_frontline(cmd.meta["auto_global_line_id"])
    if cmd.meta.get("auto_bridge_line_id"):
        g.remove_typed_frontline(cmd.meta["auto_bridge_line_id"])

    assault_lid = f"auto-fl-{cmd.id}-assault" if deploy_seq == 1 else f"auto-fl-{cmd.id}-assault-v{deploy_seq}"
    line_specs = [
        (assault_lid, assault_pts, tf.LINE_MASK_INFANTRY | tf.LINE_MASK_ARMOR, 1.25),
        (
            f"auto-fl-{cmd.id}-support",
            [_clamp_pt(lead_r - 2.0 * nr - 2.5 * tr_v, lead_c - 2.0 * nc - 2.5 * tc_v), _clamp_pt(lead_r - 2.0 * nr + 2.5 * tr_v, lead_c - 2.0 * nc + 2.5 * tc_v)],
            tf.LINE_MASK_HEAVY_AT,
            1.10,
        ),
        (
            f"auto-fl-{cmd.id}-flank",
            [_clamp_pt(lead_r + 0.5 * nr + 3.0 * tr_v, lead_c + 0.5 * nc + 3.0 * tc_v), _clamp_pt(lead_r + 0.5 * nr + 5.0 * tr_v, lead_c + 0.5 * nc + 5.0 * tc_v)],
            tf.LINE_MASK_RECON,
            1.05,
        ),
        (
            f"auto-fl-{cmd.id}-rear",
            [_clamp_pt(lead_r - 3.0 * nr - 2.0 * tr_v, lead_c - 3.0 * nc - 2.0 * tc_v), _clamp_pt(lead_r - 3.0 * nr + 2.0 * tr_v, lead_c - 3.0 * nc + 2.0 * tc_v)],
            tf.LINE_MASK_ARTY_LOG,
            1.00,
        ),
    ]
    written_lines = []
    zone_ref = next(
        (z.zone_id for z in world.ao_zones.values() if z.commander_id == cmd.id and z.activate_turn <= getattr(world, "turn", 0)),
        None,
    )
    for lid, pts, mask, wgt in line_specs:
        g.write_typed_frontline(
            lid, cmd.team, pts,
            unit_type_mask=mask,
            attraction_weight=wgt,
            front_normal=[round(nr, 4), round(nc, 4)],
            commander_id=cmd.id,
            zone_id=zone_ref,
            spread_radius_px=4,
            line_role=tf.LINE_ROLE_DEPLOY,
        )
        written_lines.append(lid)
    cmd.meta["auto_frontline_ids"] = written_lines
    cmd.meta["last_assault_pts"] = [list(p) for p in assault_pts]
    cmd.meta["last_assault_line_id"] = assault_lid

    # 本级全局行动线（排/连/营以上均持有一条指向主攻推进方向之全局引导线）
    ga_lid = f"auto-fl-{cmd.id}-global"
    ga_pts = [_clamp_pt(centroid[0], centroid[1]), _clamp_pt(lead_r + nr * 2.0, lead_c + nc * 2.0)]
    g.write_typed_frontline(
        ga_lid, cmd.team, ga_pts,
        unit_type_mask=tf.LINE_MASK_ALL,
        attraction_weight=1.0,
        front_normal=[round(nr, 4), round(nc, 4)],
        commander_id=cmd.id,
        zone_id=zone_ref,
        spread_radius_px=2,
        line_role=tf.LINE_ROLE_GLOBAL_ACTION,
    )
    cmd.meta["auto_global_line_id"] = ga_lid

    # 跨防区 > 3.0 px 位移自动架设桥接 MOVE_LINE
    bridging_ent = None
    if old_assault_pts is not None:
        bridging_ent = create_bridging_move_line_if_needed(
            g, cmd, old_assault_pts, assault_pts, assault_lid,
            old_deploy_line_id=old_assault_id,
            unit_type_mask=tf.LINE_MASK_INFANTRY | tf.LINE_MASK_ARMOR,
            zone_id=zone_ref,
        )
    cmd.meta["auto_bridge_line_id"] = bridging_ent["line_id"] if bridging_ent else None

    # 纯数学递归切分母战线至连通下级，按 τ 排队传达
    child_line_slices = partition_parent_line_by_combat_power(world, cmd, assault_pts)
    downlink_queue_res = None
    if conn_cmdrs:
        downlink_queue_res = queue_downlink_to_connected_subordinates(world, cmd, {
            "order_id": f"auto-dl-{cmd.id}-{deploy_seq}",
            "zone_id": zone_ref,
            "objective_px": _clamp_pt(lead_r, lead_c),
            "child_line_pts": child_line_slices,
            "doctrine": "defend" if (heavy_casualty or flank_disconnected) else "attack",
        })

    # 4. 仅当非人类当回合手动覆写时，向下属补发或修正下行作战日志（重创/断连切换防御）
    order_receipt = None
    if not explicit_this_turn or heavy_casualty or flank_disconnected:
        doctrine = "defend" if (heavy_casualty or flank_disconnected) else "attack"
        sub_targets_need = [
            sp.id for sp in conn_pieces
            if (not (sp.meta or {}).get("active_order")) or heavy_casualty or obj_changed
        ]
        if sub_targets_need:
            order_receipt = issue_downlink_baguwen_order(
                world, cmd.id,
                {
                    "order_id": f"auto-{cmd.id}-t{getattr(world, 'turn', 0)}-{tick_index}",
                    "targets": sub_targets_need,
                    "zone_id": zone_ref,
                    "target_px": _clamp_pt(lead_r, lead_c),
                    "doctrine": doctrine,
                    "count_churn": False,
                    "intent": f"全层级{cmd.echelon}触发({','.join(trigger_reasons)})自动部署战线",
                },
                author_commander_id=None,
            )

    return {
        "skipped": False,
        "triggered": True,
        "commander_id": cmd.id,
        "echelon": cmd.echelon,
        "reasons": trigger_reasons,
        "child_zones": child_zones_written,
        "frontlines": written_lines,
        "global_action_line_id": ga_lid,
        "bridging_line_id": bridging_ent["line_id"] if bridging_ent else None,
        "partitioned_child_lines": child_line_slices,
        "downlink_queue": downlink_queue_res,
        "order_receipt": order_receipt,
    }


evaluate_echelon_commander = evaluate_mid_tier_commander


def step_mid_tier_commanders(world, tick_index=0, subtick_hours=5.0 / 60.0):
    """按全层级节拍（师->旅->团->营->连->排）自顶向下驱动纯数学递归指挥官脚本。"""
    order_rank = {"division": 0, "brigade": 1, "regiment": 2, "battalion": 3, "company": 4, "platoon": 5}
    player_cid = (getattr(world, "meta", None) or {}).get("player_commander_id")
    auto_assist = bool((getattr(world, "meta", None) or {}).get("player_auto_assist", False))
    results = {}
    cids = sorted(
        [
            cid for cid, c in world.commanders.items()
            if c.alive and c.echelon in MID_TIER_CADENCE_TICKS
        ],
        key=lambda cid: (order_rank.get(world.commanders[cid].echelon, 9), cid),
    )
    for cid in cids:
        if player_cid and cid == player_cid and not auto_assist:
            continue
        res = evaluate_mid_tier_commander(world, cid, tick_index=tick_index, force=False, subtick_hours=subtick_hours)
        if res.get("triggered"):
            results[cid] = res
    return results


step_all_echelon_commanders = step_mid_tier_commanders


def step_hq_accompaniment(world, dt_hours=5.0 / 60.0):
    """司令部/指挥官物理实体伴随机动：仅依据已连通直属下级的安全质心，保持在战线后方 1–3 格高掩蔽像素隐蔽伴随。"""
    g = world.grid
    H, W = g.shape
    px_km = g.cell_m / 1000.0
    max_step_px = max(0.05, (4.0 * float(dt_hours)) / max(1e-6, px_km))
    sidx = autonomy_v3._resolve_spatial_index(world)
    moved_hq = {}

    for cid in sorted(world.commanders.keys()):
        cmd = world.commanders[cid]
        if not cmd.alive or cmd.attached_to:
            continue
        centroid, conn_pieces, conn_cmdrs, _ = _collect_connected_subordinate_centroid(world, cmd)
        if not conn_pieces and not conn_cmdrs:
            continue

        # 计算面对威胁的平均方向 T_hat；若无接触敌军则用全局最近可见敌或东向默认
        tr, tc = 0.0, 0.0
        for sp in conn_pieces:
            st = (sp.meta or {}).get("autonomy_state") or {}
            td = st.get("threat_bearing_deg")
            t_vec = (st.get("T") if isinstance(st.get("T"), list) else None)
            if t_vec and math.hypot(t_vec[0], t_vec[1]) > 1e-6:
                tr += float(t_vec[0])
                tc += float(t_vec[1])
            elif td is not None and float(st.get("S", 0.0)) > 0.1:
                rad = math.radians(float(td))
                tr += math.sin(rad)
                tc += math.cos(rad)
        t_mag = math.hypot(tr, tc)
        if t_mag <= 1e-6:
            enemies_near = (
                sidx.query_radius_km(centroid, 3.0, exclude_team=cmd.team)
                if sidx is not None
                else [
                    (ep, g.distance_km(centroid, ep.pos))
                    for ep in world.pieces.values()
                    if ep.team != cmd.team and ep.persons() > 0
                ]
            )
            if enemies_near:
                nearest_e = min(enemies_near, key=lambda x: x[1])[0]
                tr = float(nearest_e.pos[0]) - centroid[0]
                tc = float(nearest_e.pos[1]) - centroid[1]
                t_mag = math.hypot(tr, tc)
        fr_hat, fc_hat = (tr / t_mag, tc / t_mag) if t_mag > 1e-6 else (0.0, 1.0)

        # 目标窗口：直属安全重心后方 (-T_hat) 1–3 格内寻最高掩蔽且非水/极陡像素
        if not hasattr(cmd, "meta") or cmd.meta is None:
            cmd.meta = {}
        cur_pos = cmd.meta.get("pos") or cmd.px
        cr_now, cc_now = float(cur_pos[0]), float(cur_pos[1])
        best_cell = None
        best_score = -1e9
        for back_d in (2.0, 1.5, 2.5, 1.0, 3.0):
            cand_center_r = centroid[0] - fr_hat * back_d
            cand_center_c = centroid[1] - fc_hat * back_d
            r0 = max(0, min(H - 1, int(round(cand_center_r))))
            c0 = max(0, min(W - 1, int(round(cand_center_c))))
            for dr in (-1, 0, 1):
                for dc in (-1, 0, 1):
                    rr, cc_i = r0 + dr, c0 + dc
                    if not (0 <= rr < H and 0 <= cc_i < W):
                        continue
                    tc_code = int(g.terrain[rr, cc_i])
                    cost = tf.TERRAIN_COST.get("foot", {}).get(tc_code, math.inf)
                    if not math.isfinite(cost):
                        continue
                    if tc_code not in tf.ROAD_TYPES and float(g.slope_deg[rr, cc_i]) > tf.SLOPE_LIMIT_DEG.get("foot", 30.0):
                        continue
                    dist_from_cent = math.hypot(rr - centroid[0], cc_i - centroid[1])
                    if not (0.8 <= dist_from_cent <= 3.6):
                        continue
                    cov = float(g.effective_cover_at((rr, cc_i)))
                    # 越偏向重心正后方(与 -T_hat 对齐)且掩蔽越高越好
                    dot_back = ((rr - centroid[0]) * (-fr_hat) + (cc_i - centroid[1]) * (-fc_hat)) / max(1e-6, dist_from_cent)
                    sc = cov * 1.4 + dot_back * 0.6 - abs(dist_from_cent - 2.0) * 0.25
                    if sc > best_score:
                        best_score = sc
                        best_cell = (rr, cc_i)

        if best_cell is None:
            continue
        dr_m = float(best_cell[0]) - cr_now
        dc_m = float(best_cell[1]) - cc_now
        d_m = math.hypot(dr_m, dc_m)
        if d_m > 1e-4:
            step = min(d_m, max_step_px)
            nr_p = max(0.0, min(float(H - 1), cr_now + (dr_m / d_m) * step))
            nc_p = max(0.0, min(float(W - 1), cc_now + (dc_m / d_m) * step))
            cmd.meta["pos"] = [round(nr_p, 4), round(nc_p, 4)]
            cmd.px = (int(round(nr_p)), int(round(nc_p)))
            # 若存在配套司令部实体 Piece，一并同步伴随位置
            hq_candidates = [cmd.cadre_piece_id, f"{cid}-hq", f"{cid.replace('cmd-', '')}-hq"]
            for hpid in hq_candidates:
                if hpid and hpid in world.pieces:
                    hp = world.pieces[hpid]
                    if hp.team == cmd.team and hp.persons() > 0:
                        hp.pos = (nr_p, nc_p)
                        hp.moved_last_turn = True
            moved_hq[cid] = {"to": list(cmd.px), "target_cover_cell": list(best_cell)}

    return moved_hq
