# -*- coding: utf-8 -*-
"""Gentactix V3 全屏 Web 服务与战术沙盘后端 API（无第三方依赖，标准库 ThreadingHTTPServer）。"""

from __future__ import annotations

import argparse
import base64
import json
import math
import os
import threading
import webbrowser
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import numpy as np

from gentactix import adjudicator_v2, baguwen_v3, command_v2, events, recon_fog
from gentactix import terrain_field as tf
from gentactix import toe
import r16_platoon_loader

WEB_DIR = Path(__file__).resolve().parent / "web"
LARGE_GRID_CELL_THRESHOLD = 262144


def build_default_demo_world():
    """构建 64x64 (200m/px) V3 默认演示战区，含高山黑脊、桥梁/河道、三层编制与跨营近距单位。"""
    H, W = 64, 64
    rr, cc = np.mgrid[:H, :W].astype(np.float64)
    elev = 160.0 + 30.0 * np.sin(cc / 11.0) * np.cos(rr / 13.0)
    # 北部东西向高山脊（>380m 高程，前端以深黑呈现高差与阴影）
    elev += 340.0 * np.exp(-((rr - 12.0) ** 2) / (2.0 * 3.8 ** 2))
    terrain = np.full((H, W), tf.PLAIN, dtype=np.uint8)
    terrain[elev > 370.0] = tf.MOUNTAIN
    terrain[(rr >= 28) & (rr <= 36) & (cc >= 16) & (cc <= 24)] = tf.FOREST
    terrain[(rr >= 22) & (rr <= 26) & (cc >= 42) & (cc <= 46)] = tf.URBAN

    # 河道与桥梁（col = 32）
    terrain[:, 32] = tf.WATER
    terrain[24, :] = np.where(cc[24, :] == 32, tf.BRIDGE, tf.HIGHWAY)
    terrain[40, 32] = tf.BRIDGE
    terrain[40, 20:46] = tf.HIGHWAY

    g = tf.Grid(elev.astype(np.float32), cell_m=200.0, terrain=terrain)
    g.stash_put((24, 18), resources={"ammo": 4.0, "food": 3.0, "fuel": 2.0}, equipment={"hq_radio": 1}, owner_team="red", radio_channel="A")
    g.stash_put((24, 46), resources={"ammo": 4.0, "food": 3.0}, owner_team="blue")
    g.lay_wire([(24, c) for c in range(18, 25)], "red")

    world = adjudicator_v2.World(g, role_echelon="company")

    # 红方指挥体系（营 -> 连1/连2 -> 分队 + 跨营近距友军排长）
    p_hq_bn = toe.make_squad_piece("red-bn-hq", "第1营营部直属排", "red", (24, 18), "hq_squad", radio_channel="A")
    p_hq_bn.level = "platoon"
    p_1a = toe.make_squad_piece("red-1co-sq1", "1连1班", "red", (24, 24), "rifle_squad", radio_channel="A")
    p_1b_tank = toe.make_vehicle_piece("red-1co-tk1", "1连突击装甲车", "red", (25, 25), "tank_medium", crew_enlisted=3, crew_cadre=1, radio_channel="A")
    p_1c_arty = toe.make_squad_piece("red-1co-mor", "1连迫击炮班", "red", (23, 23), "mortar_squad", radio_channel="A")
    p_1c_arty.is_heavy_emplacement = True
    p_1c_arty.deployed_weapon = "mortar_82"
    p_2a = toe.make_squad_piece("red-2co-sq1", "2连2班", "red", (38, 22), "rifle_squad", radio_channel="A")
    p_other_pl = toe.make_squad_piece("red-2bn-recon", "邻营侦察排(400m近邻)", "red", (24, 19), "recon_squad", radio_channel="C")

    # 蓝方单位
    p_blue_1 = toe.make_squad_piece("blue-inf-1", "蓝方守备1班", "blue", (24, 44), "rifle_squad", radio_channel="A")
    p_blue_tk = toe.make_vehicle_piece("blue-tk-1", "蓝方装甲哨", "blue", (26, 40), "tank_medium", crew_enlisted=3, crew_cadre=1, radio_channel="A")

    for p in (p_hq_bn, p_1a, p_1b_tank, p_1c_arty, p_2a, p_other_pl, p_blue_1, p_blue_tk):
        world.add_piece(p)

    # 指挥官：营长 -> 连长1、连长2（另设跨营排长紧邻营长）
    cmd_bn = toe.Commander(
        "cmd-bn-red", "红方1营长", "red", (24, 18), rank="battalion_cmd",
        radio_channel="A", subordinate_ids=["cmd-co1-red", "cmd-co2-red", "red-bn-hq"],
    )
    cmd_co1 = toe.Commander(
        "cmd-co1-red", "红方1连长", "red", (24, 24), rank="company_cmd",
        radio_channel="A", subordinate_ids=["red-1co-sq1", "red-1co-tk1", "red-1co-mor"],
    )
    cmd_co2 = toe.Commander(
        "cmd-co2-red", "红方2连长", "red", (38, 22), rank="company_cmd",
        radio_channel="A", subordinate_ids=["red-2co-sq1"],
    )
    cmd_other_pl = toe.Commander(
        "cmd-pl-other", "邻营侦察排长", "red", (24, 19), rank="platoon_cmd",
        radio_channel="C", subordinate_ids=["red-2bn-recon"],
    )
    cmd_blue = toe.Commander(
        "cmd-co-blue", "蓝方连长", "blue", (24, 44), rank="company_cmd",
        radio_channel="A", subordinate_ids=["blue-inf-1", "blue-tk-1"],
    )
    for c in (cmd_bn, cmd_co1, cmd_co2, cmd_other_pl, cmd_blue):
        world.add_commander(c)

    # 初始已确认 AO（连长1防区 + V3.1 默认分兵种战线势场）
    ao1 = command_v2.OperationalZone(
        "ao-co1-init", "cmd-co1-red", "red", "company",
        command_v2.rasterize_circle((24, 24), 1.4, cell_m=g.cell_m, shape=g.shape),
        activate_turn=0,
        frontlines=[
            {
                "line_id": "fl-co1-assault",
                "team": "red",
                "commander_id": "cmd-co1-red",
                "points_px": [[20, 27], [28, 27]],
                "unit_type_mask": tf.LINE_MASK_INFANTRY | tf.LINE_MASK_ARMOR,
                "attraction_weight": 1.2,
                "front_normal": [0.0, 1.0],
            },
            {
                "line_id": "fl-co1-heavy",
                "team": "red",
                "commander_id": "cmd-co1-red",
                "points_px": [[21, 24], [27, 24]],
                "unit_type_mask": tf.LINE_MASK_HEAVY_AT,
                "attraction_weight": 1.05,
                "front_normal": [0.0, 1.0],
            },
        ],
    )
    command_v2.register_zone(world.ao_zones, ao1, world.commanders, current_turn=0, pieces=world.pieces, grid=g)
    return world


def _unit_weapons(piece):
    weapons = []
    for sq in piece.squads():
        for m, n in (sq.equipment or {}).items():
            if int(n) > 0 and m in toe.EQUIPMENT:
                spec = toe.EQUIPMENT[m]
                if spec.get("range_m", 0) > 0:
                    weapons.append({
                        "model": m,
                        "name": spec.get("name", m),
                        "count": int(n),
                        "range_m": float(spec.get("range_m", 400.0)),
                        "min_range_m": float(spec.get("min_range_m", 0.0)),
                        "indirect": bool(spec.get("indirect", False)),
                    })
    if piece.deployed_weapon and piece.deployed_weapon in toe.EQUIPMENT:
        m = piece.deployed_weapon
        if not any(w["model"] == m for w in weapons):
            spec = toe.EQUIPMENT[m]
            weapons.append({
                "model": m, "name": spec.get("name", m), "count": 1,
                "range_m": float(spec.get("range_m", 800.0)),
                "min_range_m": float(spec.get("min_range_m", 0.0)),
                "indirect": bool(spec.get("indirect", False)),
            })
    return weapons


class GentactixV3App:
    """线程安全的 V3 沙盘状态控制器，可供 HTTP 服务与自动化测试直接调用。"""

    def __init__(self, world=None):
        self.lock = threading.RLock()
        self.world = world or build_default_demo_world()
        self.pre_game_editable = True
        self.last_turn_summary = None
        self.terrain_revision = 1
        self._terrain_bin_cache = None

    def _bump_terrain_rev(self):
        self.terrain_revision += 1
        self._terrain_bin_cache = None

    def get_terrain_raw_bin(self):
        with self.lock:
            if self._terrain_bin_cache is not None and self._terrain_bin_cache[0] == self.terrain_revision:
                return self._terrain_bin_cache[1]
            g = self.world.grid
            t_bytes = g.terrain.astype("<u1", copy=False).tobytes()
            e_bytes = np.round(g.elevation, 0).clip(0, 65535).astype("<u2").tobytes()
            blob = t_bytes + e_bytes
            self._terrain_bin_cache = (self.terrain_revision, blob)
            return blob

    def serialize_state(self, commander_id=None, observer_team=None):
        with self.lock:
            w = self.world
            g = w.grid
            if not commander_id or commander_id not in w.commanders:
                commander_id = next((cid for cid, c in w.commanders.items() if c.alive), None)
            active_cmd = w.commanders.get(commander_id) if commander_id else None
            team = observer_team or (active_cmd.team if active_cmd else "red")

            intel = recon_fog.gather_intel(g, list(w.pieces.values()), night=w.night)
            fog = recon_fog.fog_view(team, list(w.pieces.values()), intel, g, turn=w.turn)
            scope = baguwen_v3.get_commander_readable_scope(w, commander_id, proximity_km=0.4) if commander_id else {
                "own_commander_id": None,
                "readable_commander_ids": set(),
                "readable_piece_ids": set(),
                "proximity_commander_ids": set(),
                "proximity_piece_ids": set(),
                "connected_direct_subordinates": [],
                "disconnected_direct_subordinates": [],
            }
            uplink = baguwen_v3.collect_commander_uplink_baguwen(w, commander_id) if commander_id else {
                "commander_id": None, "reports": [], "omitted_disconnected_ids": [], "text": ""
            }

            readable_all = scope["readable_commander_ids"] | scope["readable_piece_ids"]
            own_pieces = []
            for p in sorted(w.pieces.values(), key=lambda x: x.id):
                if p.team != team:
                    continue
                if p.persons() <= 0 and not getattr(p, "vehicle_model", None) and not getattr(p, "is_heavy_emplacement", False) and not getattr(p, "deployed_weapon", None):
                    continue
                cp = p.counter_profile()
                has_f = bool(cp.get("display_tier_bonus", 0) > 0 or getattr(p, "vehicle_model", None) or getattr(p, "is_heavy_emplacement", False))
                route = (p.meta or {}).get("planned_route") or {}
                ao = (p.meta or {}).get("active_order") or {}
                auto_st = (p.meta or {}).get("autonomy_state") or {}
                direct_c = command_v2.direct_commander_for(p, w.commanders)
                op_info = p.get_operator_info(w.pieces) if hasattr(p, "get_operator_info") else {}
                soldiers_out = self._ensure_piece_soldiers(p)
                caps_out = self._piece_capabilities(p)
                is_hq = any(getattr(c, "hq_piece_id", None) == p.id for c in w.commanders.values())
                own_pieces.append({
                    "id": p.id,
                    "name": p.name,
                    "team": p.team,
                    "px": list(p.px),
                    "pos": [round(p.pos[0], 3), round(p.pos[1], 3)],
                    "level": p.level,
                    "persons": p.persons(),
                    "org": round(p.mean_org(), 1),
                    "org_cap": round(cp["org_cap_limit"], 1),
                    "has_facing": has_f,
                    "facing_deg": round(p.facing_deg, 1) if has_f else None,
                    "tactical_face_vec": (p.meta or {}).get("tactical_face_vec"),
                    "radio_channel": p.radio_channel,
                    "radio_silence": p.radio_silence,
                    "is_disconnected": bool((p.meta or {}).get("is_disconnected", False)),
                    "exclamation_buff": (p.meta or {}).get("exclamation_buff"),
                    "k_C": round(float((p.meta or {}).get("k_C", 1.0)), 3),
                    "atomic_action": auto_st.get("atomic_action", "HOLD"),
                    "vehicle_model": p.vehicle_model,
                    "deployed_weapon": p.deployed_weapon,
                    "is_heavy_emplacement": p.is_heavy_emplacement,
                    "unit_state": p.unit_state,
                    "attached_to": p.attached_to,
                    "operated_by": p.operated_by,
                    "operator_persons": int(op_info.get("operator_persons", 0)),
                    "operator_soldier_ids": list(op_info.get("operator_soldier_ids") or []),
                    "operator_buff_mult": round(float(op_info.get("total_mult", 0.0)), 3),
                    "crew_buff_mult": round(float(op_info.get("crew_buff_mult", 0.0)), 3),
                    "base_speed_kmh": p.base_speed_kmh(w.pieces) if hasattr(p, "base_speed_kmh") else 4.5,
                    "is_hq_piece": is_hq,
                    "capabilities": caps_out,
                    "soldiers": soldiers_out,
                    "carried_bodies": list((p.meta or {}).get("carried_bodies") or []),
                    "direct_commander_id": direct_c.id if direct_c else None,
                    "readable_line": p.id in readable_all,
                    "counter": cp,
                    "weapons": _unit_weapons(p),
                    "autonomy_state": auto_st,
                    "planned_route": route if p.id in readable_all else None,
                    "active_order": ao if p.id in readable_all else None,
                    "task_queue": (p.meta or {}).get("task_queue", []),
                    "direct_order": {
                        "action": (p.meta.get("task_queue", [{}])[0].get("action", "move")) if p.meta.get("task_queue") else "stop",
                        "waypoints": [s["goal"] for s in p.meta.get("task_queue", []) if "goal" in s],
                        "target_pos": (p.meta.get("task_queue", [{}])[-1].get("goal")) if p.meta.get("task_queue") else None,
                        "move_mode": (p.meta.get("task_queue", [{}])[0].get("move_mode", "normal")) if p.meta.get("task_queue") else "normal",
                        "fire_mode": (p.meta.get("task_queue", [{}])[0].get("fire_mode", "stop_to_fire")) if p.meta.get("task_queue") else "stop_to_fire",
                        "segments": p.meta.get("task_queue", []),
                    } if (p.meta or {}).get("task_queue") else None,
                    "retreat_recovery_hours": float((p.meta or {}).get("retreat_recovery_hours", 0.0)),
                    "stock": {k: round(v, 3) for k, v in p.stock.items()},
                    "ammo_by_type": {k: round(v, 3) for k, v in p.ammo_by_type.items()},
                    "debuffs": sorted(p.status_debuffs.keys()),
                })

            commanders_out = []
            for cid, c in sorted(w.commanders.items()):
                if c.team != team:
                    continue
                r_km = command_v2.effective_commander_radius_km(c, w.pieces, g)
                hq_p = w.pieces.get(getattr(c, "hq_piece_id", None))
                hq_soldiers = self._ensure_piece_soldiers(hq_p) if hq_p is not None else []
                hq_carried = list((hq_p.meta or {}).get("carried_bodies") or []) if hq_p is not None else []
                commanders_out.append({
                    "id": c.id,
                    "name": c.name,
                    "team": c.team,
                    "echelon": c.echelon,
                    "rank": c.rank,
                    "px": list(c.px),
                    "alive": c.alive,
                    "radio_channel": c.radio_channel,
                    "radio_silence": c.radio_silence,
                    "radius_km": round(r_km, 3),
                    "max_area_px": command_v2.effective_commander_max_area_px(c, w.pieces, g),
                    "subordinate_ids": list(c.subordinate_ids),
                    "hq_piece_id": getattr(c, "hq_piece_id", None),
                    "hq_soldiers": hq_soldiers,
                    "hq_carried_bodies": hq_carried,
                    "hq_ammo_t": round(float(hq_p.stock.get("ammo", 0.0)), 3) if hq_p else 0.0,
                    "hq_food_t": round(float(hq_p.stock.get("food", 0.0)), 3) if hq_p else 0.0,
                    "hq_fuel_t": round(float(hq_p.stock.get("fuel", 0.0)), 3) if hq_p else 0.0,
                    "is_self": c.id == commander_id,
                    "readable": c.id in scope["readable_commander_ids"],
                    "proximity_readable": c.id in scope["proximity_commander_ids"],
                    "task_queue": (w.pieces[c.hq_piece_id].meta or {}).get("task_queue", []) if getattr(c, "hq_piece_id", None) in w.pieces else [],
                    "direct_order": {
                        "action": (w.pieces[c.hq_piece_id].meta.get("task_queue", [{}])[0].get("action", "move")) if w.pieces[c.hq_piece_id].meta.get("task_queue") else "stop",
                        "waypoints": [s["goal"] for s in w.pieces[c.hq_piece_id].meta.get("task_queue", []) if "goal" in s],
                        "target_pos": (w.pieces[c.hq_piece_id].meta.get("task_queue", [{}])[-1].get("goal")) if w.pieces[c.hq_piece_id].meta.get("task_queue") else None,
                        "move_mode": (w.pieces[c.hq_piece_id].meta.get("task_queue", [{}])[0].get("move_mode", "normal")) if w.pieces[c.hq_piece_id].meta.get("task_queue") else "normal",
                        "fire_mode": (w.pieces[c.hq_piece_id].meta.get("task_queue", [{}])[0].get("fire_mode", "stop_to_fire")) if w.pieces[c.hq_piece_id].meta.get("task_queue") else "stop_to_fire",
                        "segments": w.pieces[c.hq_piece_id].meta.get("task_queue", []),
                    } if (getattr(c, "hq_piece_id", None) in w.pieces and (w.pieces[c.hq_piece_id].meta or {}).get("task_queue")) else None,
                })

            # Layer 1 已确认通联连线（严格过滤在 readable_scope 内）
            layer1_links = []
            if active_cmd is not None:
                for sid in active_cmd.subordinate_ids:
                    if sid in w.pieces and sid in scope["readable_piece_ids"]:
                        sp = w.pieces[sid]
                        ok, via = command_v2.validate_command_link(active_cmd, sp, g, pieces=w.pieces)
                        if ok:
                            layer1_links.append({"from": list(active_cmd.px), "to": list(sp.px), "target_id": sid, "via": via, "kind": "subordinate"})
                    elif sid in w.commanders and sid in scope["readable_commander_ids"]:
                        sc = w.commanders[sid]
                        ok, via = command_v2.validate_commander_link(active_cmd, sc, g, pieces=w.pieces)
                        if ok:
                            layer1_links.append({"from": list(active_cmd.px), "to": list(sc.px), "target_id": sid, "via": via, "kind": "subordinate_cmdr"})
                for px_cid in scope["proximity_commander_ids"]:
                    oc = w.commanders[px_cid]
                    layer1_links.append({"from": list(active_cmd.px), "to": list(oc.px), "target_id": px_cid, "via": "proximity_400m", "kind": "proximity"})
                for px_pid in scope["proximity_piece_ids"]:
                    op = w.pieces[px_pid]
                    layer1_links.append({"from": list(active_cmd.px), "to": list(op.px), "target_id": px_pid, "via": "proximity_400m", "kind": "proximity"})

            ao_out = []
            for z in w.ao_zones.values():
                if z.team == team:
                    ao_out.append({
                        "zone_id": z.zone_id,
                        "commander_id": z.commander_id,
                        "echelon": z.echelon,
                        "cells": [list(rc) for rc in sorted(z.cells)],
                        "area_px": z.area_px,
                        "activate_turn": z.activate_turn,
                        "frontlines": list(getattr(z, "frontlines", []) or []),
                        "writable": (z.commander_id == commander_id),
                    })

            typed_frontlines_out = [
                {
                    "line_id": str(fl.get("line_id", "")),
                    "team": fl.get("team"),
                    "commander_id": fl.get("commander_id"),
                    "zone_id": fl.get("zone_id"),
                    "unit_type_mask": int(fl.get("unit_type_mask", tf.LINE_MASK_ALL)),
                    "attraction_weight": float(fl.get("attraction_weight", 1.0)),
                    "front_normal": list(fl.get("front_normal") or [0.0, 1.0]),
                    "points_px": [list(pt) for pt in (fl.get("points_px") or [])],
                    "writable": (fl.get("commander_id") == commander_id),
                }
                for fl in sorted(getattr(g, "typed_frontlines", {}).values(), key=lambda x: str(x.get("line_id", "")))
                if fl.get("team") == team
            ]

            stashes = []
            for rc, st in g.cell_stash.items():
                if st.get("owner_team") in (None, team):
                    stashes.append({"px": [int(rc[0]), int(rc[1])], "resources": st.get("resources", {}), "equipment": st.get("equipment", {}), "radio_channel": st.get("radio_channel")})

            H, W = int(g.shape[0]), int(g.shape[1])
            large_grid = (H * W) > LARGE_GRID_CELL_THRESHOLD
            focus_px = (
                list(active_cmd.px)
                if active_cmd is not None
                else [int(H // 2), int(W // 2)]
            )

            feat_overlays = []
            for fid, ch in g.feature_channels.items():
                mask = ch["mask"]
                cnt = int(np.count_nonzero(mask))
                if large_grid:
                    feat_overlays.append({"feature_id": fid, "cells": [], "count": cnt})
                else:
                    coords = np.argwhere(mask)[:4096].tolist()
                    feat_overlays.append({"feature_id": fid, "cells": coords, "count": cnt})
            cell_mods_out = [
                {"px": [int(r), int(c)], "features": g.get_terrain_features((r, c))}
                for (r, c) in sorted(g.cell_modifiers.keys())
            ]

            wires = []
            for a, b in sorted(g.wire_edges.get(team, set())):
                wires.append([list(a), list(b)])

            grid_payload = {
                "rows": H,
                "cols": W,
                "cell_m": float(g.cell_m),
                "elev_min": float(np.min(g.elevation)),
                "elev_max": float(np.max(g.elevation)),
                "large_grid": bool(large_grid),
                "terrain_revision": int(self.terrain_revision),
                "focus_px": focus_px,
                "r16_source": w.meta.get("r16_source"),
                "feature_overlays": feat_overlays,
                "cell_features": cell_mods_out,
                "stashes": stashes,
                "wires": wires,
            }
            if not large_grid:
                grid_payload["elevation"] = np.round(g.elevation, 1).tolist()
                grid_payload["terrain"] = g.terrain.tolist()

            return {
                "ok": True,
                "turn": w.turn,
                "clock_hours": round(w.clock_hours, 3),
                "hours_per_turn": round(w.hours, 3),
                "role_echelon": w.role_echelon,
                "night": w.night,
                "pre_game_editable": bool(self.pre_game_editable and w.turn == 0),
                "active_commander_id": commander_id,
                "team": team,
                "grid": grid_payload,
                "feature_catalog": {
                    k: {"name": v.get("name", k), "color": v.get("color"), "modifiers": v.get("modifiers", {})}
                    for k, v in tf.TERRAIN_FEATURE_SPECS.items()
                },
                "own_pieces": own_pieces,
                "enemy_contacts": fog.get("contacts", fog.get("enemies", [])),
                "commanders": commanders_out,
                "ao_zones": ao_out,
                "typed_frontlines": typed_frontlines_out,
                "layer1_links": layer1_links,
                "readable_scope": {
                    "own_commander_id": scope["own_commander_id"],
                    "readable_commander_ids": sorted(scope["readable_commander_ids"]),
                    "readable_piece_ids": sorted(scope["readable_piece_ids"]),
                    "proximity_commander_ids": sorted(scope["proximity_commander_ids"]),
                    "proximity_piece_ids": sorted(scope["proximity_piece_ids"]),
                    "disconnected_direct": scope["disconnected_direct_subordinates"],
                },
                "uplink_inbox": uplink,
                "active_events": [
                    e.to_dict() for e in w.event_manager.events.values()
                    if (
                        (e.actor_id in w.pieces and w.pieces[e.actor_id].team == team)
                        or (e.actor_id in w.commanders and w.commanders[e.actor_id].team == team)
                    )
                ],
                "ground_bodies": [
                    gb for gb in (w.meta.get("ground_bodies") or [])
                    if gb.get("team") in (None, team) or not gb.get("team")
                ],
                "last_turn_summary": self.last_turn_summary,
            }

    def handle_import_r16(self, body):
        with self.lock:
            w = self.world
            if not self.pre_game_editable or w.turn > 0:
                return 400, {"ok": False, "reason": "map_editing_locked_after_game_start"}
            width = body.get("width")
            height = body.get("height")
            bit_depth = int(body.get("bit_depth", 16))
            flip_y = bool(body.get("flip_y", False))
            elev_low = float(body.get("elev_low_m", 0.0))
            elev_high = float(body.get("elev_high_m", 1000.0))

            if body.get("raw_b64"):
                raw_source = base64.b64decode(body["raw_b64"])
            elif body.get("path"):
                raw_source = str(body["path"])
            else:
                return 400, {"ok": False, "reason": "missing_r16_raw_or_path"}

            feat_channels = {}
            for ch in (body.get("feature_channels") or []):
                fid = ch["feature_id"]
                src = base64.b64decode(ch["raw_b64"]) if ch.get("raw_b64") else ch["path"]
                feat_channels[fid] = {
                    "source": src,
                    "bit_depth": int(ch.get("bit_depth", 8)),
                    "threshold": float(ch.get("threshold", 0.5)),
                    "equals": ch.get("equals"),
                    "modifiers": ch.get("modifiers"),
                    "flip_y": bool(ch.get("flip_y", flip_y)),
                }

            terrain_ch = None
            if body.get("terrain_raw_b64") or body.get("terrain_path"):
                terrain_ch = {
                    "source": base64.b64decode(body["terrain_raw_b64"]) if body.get("terrain_raw_b64") else body["terrain_path"],
                    "bit_depth": int(body.get("terrain_bit_depth", 8)),
                    "code_map": body.get("terrain_code_map"),
                    "flip_y": flip_y,
                }

            try:
                new_grid = tf.Grid.from_raw_channels(
                    raw_source,
                    width=width,
                    height=height,
                    bit_depth=bit_depth,
                    cell_m=200.0,
                    elev_low_m=elev_low,
                    elev_high_m=elev_high,
                    flip_y=flip_y,
                    terrain_channel=terrain_ch,
                    feature_channels=feat_channels or None,
                )
            except Exception as exc:
                return 400, {"ok": False, "reason": f"r16_import_error:{exc}"}

            # 自动标注高山地形（若未传入专门地形通道，默认高程 > 75% 分位标为山地深黑特征或保持真实高程）
            if terrain_ch is None and body.get("auto_mountain_above_m") is not None:
                thr = float(body["auto_mountain_above_m"])
                new_grid.terrain[new_grid.elevation >= thr] = tf.MOUNTAIN
                new_grid.refresh_attrition()

            w.grid = new_grid
            H, W = new_grid.shape
            for p in w.pieces.values():
                p.pos = (min(max(0.0, p.pos[0]), H - 1.0), min(max(0.0, p.pos[1]), W - 1.0))
            for c in w.commanders.values():
                c.px = new_grid.clamp(c.px)
            w.ao_zones.clear()
            self._bump_terrain_rev()
            return 200, {"ok": True, "rows": int(H), "cols": int(W), "cell_m": float(new_grid.cell_m)}

    def handle_edit_pixel(self, body):
        with self.lock:
            w = self.world
            if not self.pre_game_editable or w.turn > 0:
                return 400, {"ok": False, "reason": "map_editing_locked_after_game_start"}
            g = w.grid
            cells = body.get("cells") or ([body["px"]] if "px" in body else [])
            if not cells:
                return 400, {"ok": False, "reason": "no_cells_specified"}
            updated = 0
            elev_changed = False
            for rc in cells:
                r, c = int(rc[0]), int(rc[1])
                if not g.in_bounds((r, c)):
                    continue
                updated += 1
                if body.get("remove_feature"):
                    g.remove_terrain_feature((r, c), body["remove_feature"], fallback_terrain=body.get("terrain_code"))
                if "feature_id" in body and body["feature_id"]:
                    g.add_terrain_feature(
                        (r, c),
                        body["feature_id"],
                        modifiers=body.get("modifiers"),
                        sync_terrain=bool(body.get("sync_terrain", True)),
                        terrain_code=body.get("terrain_code"),
                    )
                elif "terrain_code" in body and body["terrain_code"] is not None:
                    g.terrain[r, c] = int(body["terrain_code"])
                if "elevation" in body and body["elevation"] is not None:
                    g.elevation[r, c] = float(body["elevation"])
                    elev_changed = True
                if isinstance(body.get("stash"), dict):
                    st = body["stash"]
                    g.stash_put((r, c), resources=st.get("resources"), equipment=st.get("equipment"), owner_team=st.get("owner_team"))
            if elev_changed:
                g.recompute_slope()
            g.refresh_attrition()
            self._bump_terrain_rev()
            return 200, {"ok": True, "updated_cells": updated}

    def handle_validate_ao(self, body):
        with self.lock:
            w = self.world
            player_cid = body.get("player_commander_id")
            target_cid = body.get("commander_id") or player_cid
            if player_cid and not baguwen_v3.can_commander_write_line(player_cid, target_cid):
                return 200, {"ok": False, "reason": "forbidden_write_other_commander_line"}
            cmdr = w.commanders.get(target_cid)
            if cmdr is None or not cmdr.alive:
                return 200, {"ok": False, "reason": "commander_unavailable"}
            zspec = dict(body.get("ao_spec") or {})
            zid = zspec.get("id") or zspec.get("zone_id") or f"draft-{target_cid}"
            cells = command_v2.build_ao_cells(zspec, w.grid, w.pieces)
            zone = command_v2.OperationalZone(
                zid, target_cid, cmdr.team, zspec.get("echelon") or cmdr.echelon, cells, activate_turn=w.turn + 1
            )
            dry_zones = dict(w.ao_zones)
            ok, reason = command_v2.register_zone(
                dry_zones, zone, w.commanders, current_turn=w.turn, pieces=w.pieces, grid=w.grid
            )
            return 200, {
                "ok": bool(ok),
                "reason": reason,
                "area_px": zone.area_px,
                "max_area_px": command_v2.effective_commander_max_area_px(cmdr, w.pieces, w.grid),
                "cells": [list(rc) for rc in sorted(zone.cells)],
            }

    def handle_weapon_los(self, body):
        with self.lock:
            w = self.world
            g = w.grid
            pid = body.get("piece_id")
            p = w.pieces.get(pid)
            if p is None:
                return 404, {"ok": False, "reason": "piece_not_found"}
            weapons = _unit_weapons(p)
            w_model = body.get("weapon_model") or (weapons[0]["model"] if weapons else "rifle")
            spec = toe.EQUIPMENT.get(w_model, {"range_m": 400.0, "min_range_m": 0.0, "indirect": False})
            range_m = float(spec.get("range_m", 400.0))
            min_range_m = float(spec.get("min_range_m", 0.0))
            indirect = bool(spec.get("indirect", False))
            arc = tf.compute_fire_arc(
                g, p.pos, range_m, min_range_m=min_range_m, indirect=indirect, night=w.night, max_scan_m=max(range_m * 1.4, 2400.0)
            )
            arc_map = {(c[0], c[1]): c[2] for c in arc}
            targetable_enemies = []
            for ep in w.pieces.values():
                if ep.team == p.team:
                    continue
                if ep.persons() <= 0 and not getattr(ep, "vehicle_model", None) and not getattr(ep, "is_heavy_emplacement", False) and not getattr(ep, "deployed_weapon", None):
                    continue
                if ep.px in arc_map:
                    targetable_enemies.append({"id": ep.id, "name": ep.name, "px": list(ep.px), "eff": arc_map[ep.px]})
            return 200, {
                "ok": True,
                "piece_id": pid,
                "weapon_model": w_model,
                "range_m": range_m,
                "indirect": indirect,
                "cells": arc,
                "targetable_enemies": targetable_enemies,
            }

    @staticmethod
    def _commander_owns_piece(w, cmd, pid):
        if cmd is None or not pid:
            return True
        p = w.pieces.get(pid)
        if p is None:
            return True
        if p.team != cmd.team:
            return False
        allowed = set()
        if getattr(cmd, "hq_piece_id", None):
            allowed.add(cmd.hq_piece_id)
        if getattr(cmd, "cadre_piece_id", None):
            allowed.add(cmd.cadre_piece_id)
        queue = [cmd.id]
        visited_c = set()
        while queue:
            cid = queue.pop(0)
            if cid in visited_c:
                continue
            visited_c.add(cid)
            cobj = w.commanders.get(cid)
            if not cobj or cobj.team != cmd.team:
                continue
            if getattr(cobj, "hq_piece_id", None):
                allowed.add(cobj.hq_piece_id)
            if getattr(cobj, "cadre_piece_id", None):
                allowed.add(cobj.cadre_piece_id)
            for sid in (cobj.subordinate_ids or []):
                if sid in w.commanders:
                    queue.append(sid)
                else:
                    allowed.add(sid)
        if pid in allowed:
            return True
        direct_c = command_v2.direct_commander_for(p, w.commanders)
        return direct_c is None

    @staticmethod
    def _piece_capabilities(p):
        caps = set()
        for sq in p.squads():
            caps.update(sq.derive_capabilities().keys())
            caps.update(sq.arm_tags())
        return sorted(caps)

    @staticmethod
    def _ensure_piece_soldiers(p):
        n_pers = max(0, int(p.persons()))
        existing = list((p.meta or {}).get("soldiers") or [])
        if len(existing) != n_pers:
            soldiers = []
            idx = 0
            for sq in p.squads():
                eq_pool = []
                for m, cnt in sq.equipment.items():
                    if m not in toe.VEHICLE_SPECS:
                        for _ in range(int(cnt)):
                            eq_pool.append(m)
                for c_i in range(int(sq.cadre)):
                    if idx < len(existing):
                        soldiers.append(existing[idx])
                    else:
                        wpn = eq_pool[c_i] if c_i < len(eq_pool) else "rifle"
                        spec = toe.EQUIPMENT.get(wpn, {})
                        soldiers.append({
                            "id": f"{p.id}-sol-{idx + 1}",
                            "name": f"班干#{idx + 1}",
                            "hp": 100.0,
                            "status": "healthy",
                            "is_cadre": True,
                            "weapon": wpn,
                            "cannot_fire": False,
                            "pack_cols": 8,
                            "pack_rows": 6,
                            "max_hand_weight_kg": 18.0,
                            "max_pack_weight_kg": 28.0,
                            "items": [
                                {
                                    "name": spec.get("name", wpn),
                                    "kind": "weapon",
                                    "model": wpn,
                                    "weight_kg": float(spec.get("weight_kg", 4.0)),
                                    "count": 1,
                                    "slot": "hand",
                                    "grid_w": 3,
                                    "grid_h": 1,
                                    "grid_x": 0,
                                    "grid_y": 0,
                                },
                                {
                                    "name": "随身弹药包",
                                    "kind": "ammo",
                                    "weight_kg": 2.5,
                                    "ammo_t": 0.005,
                                    "count": 1,
                                    "slot": "pack",
                                    "grid_w": 2,
                                    "grid_h": 2,
                                    "grid_x": 0,
                                    "grid_y": 0,
                                },
                            ],
                        })
                    idx += 1
                for e_i in range(int(sq.enlisted)):
                    if idx < len(existing):
                        soldiers.append(existing[idx])
                    else:
                        eq_i = int(sq.cadre) + e_i
                        wpn = eq_pool[eq_i] if eq_i < len(eq_pool) else "rifle"
                        spec = toe.EQUIPMENT.get(wpn, {})
                        kind_it = "radio" if "radio" in wpn else "weapon"
                        soldiers.append({
                            "id": f"{p.id}-sol-{idx + 1}",
                            "name": f"战士#{idx + 1}",
                            "hp": 100.0,
                            "status": "healthy",
                            "is_cadre": False,
                            "weapon": wpn,
                            "cannot_fire": False,
                            "pack_cols": 8,
                            "pack_rows": 6,
                            "max_hand_weight_kg": 18.0,
                            "max_pack_weight_kg": 28.0,
                            "items": [
                                {
                                    "name": spec.get("name", wpn),
                                    "kind": kind_it,
                                    "model": wpn,
                                    "weight_kg": float(spec.get("weight_kg", 4.0)),
                                    "count": 1,
                                    "slot": "hand",
                                    "grid_w": 3,
                                    "grid_h": 1,
                                    "grid_x": 0,
                                    "grid_y": 0,
                                },
                                {
                                    "name": "口粮干粮",
                                    "kind": "ration",
                                    "weight_kg": 1.2,
                                    "ammo_t": 0.002,
                                    "count": 1,
                                    "slot": "pack",
                                    "grid_w": 2,
                                    "grid_h": 1,
                                    "grid_x": 0,
                                    "grid_y": 0,
                                },
                            ],
                        })
                    idx += 1
            p.meta["soldiers"] = soldiers[:n_pers]
        for s in (p.meta.get("soldiers") or []):
            hand_cnt = sum(1 for it in (s.get("items") or []) if it.get("slot") == "hand")
            s["cannot_fire"] = (hand_cnt >= 2)
        return list(p.meta.get("soldiers") or [])

    def handle_unit_op(self, body):
        with self.lock:
            w = self.world
            op = str(body.get("op", ""))
            player_cid = body.get("player_commander_id") or body.get("commander_id")
            player_cmd = w.commanders.get(body.get("player_commander_id")) if body.get("player_commander_id") else None
            if body.get("player_commander_id") and player_cmd is None:
                return 403, {"ok": False, "reason": "unknown_player_commander"}
            if player_cmd is not None and op not in ("reset_demo",):
                check_pids = [
                    body.get("piece_id"), body.get("unit_id"), body.get("target_id"),
                    body.get("actor_id"), body.get("from_piece_id"), body.get("to_piece_id"),
                    body.get("from_unit_id"), body.get("to_unit_id"),
                ]
                for spid in (body.get("source_pieces") or []):
                    check_pids.append(spid)
                for cpid in check_pids:
                    if not cpid or cpid not in w.pieces:
                        continue
                    if w.pieces[cpid].team != player_cmd.team:
                        return 403, {"ok": False, "reason": "forbidden_cross_team"}
                    if not self._commander_owns_piece(w, player_cmd, cpid):
                        return 403, {"ok": False, "reason": "forbidden_command_chain"}
                chk_cid = body.get("commander_id")
                if chk_cid and chk_cid in w.commanders and w.commanders[chk_cid].team != player_cmd.team:
                    return 403, {"ok": False, "reason": "forbidden_cross_team"}
            if op == "split_squad":
                res = toe.commander_reorganize(w, player_cid, {"op": "split_squad", **body}, enforce_link=False)
                return (200 if res.get("ok") else 400), res
            if op == "merge_squad_pieces":
                res = toe.commander_reorganize(w, player_cid, {"op": "merge_squad_pieces", **body}, enforce_link=False)
                return (200 if res.get("ok") else 400), res
            if op == "start_reorganize_echelon":
                res = w.event_manager.start_reorganize_echelon(w, player_cid, body)
                return (200 if res.get("ok") else 400), res
            if op == "set_counter_skin":
                p = w.pieces.get(body.get("piece_id"))
                if p is None:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                try:
                    skin = p.set_counter_skin(
                        color=body.get("color"),
                        badge_style=body.get("badge_style"),
                        custom_label=body.get("custom_label"),
                        attempt_level=body.get("attempt_level"),
                    )
                    return 200, {"ok": True, "piece_id": p.id, "skin": skin, "level": p.level}
                except ValueError as exc:
                    return 400, {"ok": False, "reason": str(exc)}
            if op in ("issue_baguwen_order", "issue_combat_log_order"):
                target_cid = body.get("target_commander_id") or player_cid
                res = baguwen_v3.issue_downlink_baguwen_order(
                    w, target_cid, body.get("order") or body, author_commander_id=player_cid
                )
                return (200 if res.get("ok") else 403), res
            if op == "write_typed_frontline":
                target_cid = body.get("commander_id") or player_cid
                if player_cid and not baguwen_v3.can_commander_write_line(player_cid, target_cid):
                    return 403, {"ok": False, "reason": "forbidden_write_other_commander_line"}
                cmdr = w.commanders.get(target_cid) if target_cid else None
                pts = body.get("points_px") or []
                if len(pts) < 2:
                    return 400, {"ok": False, "reason": "frontline_needs_at_least_2_points"}
                lid = str(body.get("line_id") or f"fl-{target_cid or 'ui'}-{w.turn}-{len(w.grid.typed_frontlines)}")
                fl_rec = w.grid.write_typed_frontline(
                    lid,
                    body.get("team") or (cmdr.team if cmdr else "red"),
                    pts,
                    unit_type_mask=int(body.get("unit_type_mask", tf.LINE_MASK_ALL)),
                    attraction_weight=float(body.get("attraction_weight", 1.0)),
                    front_normal=body.get("front_normal"),
                    commander_id=target_cid,
                    zone_id=body.get("zone_id"),
                    spread_radius_px=int(body.get("spread_radius_px", 4)),
                )
                return 200, {"ok": True, "line_id": lid, "frontline": {k: v for k, v in fl_rec.items() if k != "mask_cells"}}
            if op == "set_radio":
                eid = body.get("entity_id") or body.get("piece_id")
                ch = body.get("radio_channel")
                sil = body.get("radio_silence")
                if eid in w.pieces:
                    if ch in toe.RADIO_CHANNELS:
                        w.pieces[eid].radio_channel = ch
                    if sil is not None:
                        w.pieces[eid].radio_silence = bool(sil)
                elif eid in w.commanders:
                    if ch in toe.RADIO_CHANNELS:
                        w.commanders[eid].radio_channel = ch
                    if sil is not None:
                        w.commanders[eid].radio_silence = bool(sil)
                    hq_pid = getattr(w.commanders[eid], "hq_piece_id", None)
                    if hq_pid and hq_pid in w.pieces:
                        if ch in toe.RADIO_CHANNELS:
                            w.pieces[hq_pid].radio_channel = ch
                        if sil is not None:
                            w.pieces[hq_pid].radio_silence = bool(sil)
                else:
                    return 404, {"ok": False, "reason": "entity_not_found"}
                return 200, {"ok": True, "entity_id": eid, "state": self.serialize_state(commander_id=player_cid)}
            if op == "load_r16_platoon":
                folder = body.get("folder") or r16_platoon_loader.DEFAULT_R16_DIR
                tile_xy = tuple(body.get("tile_xy") or (1, 1))
                thr = int(body.get("flow_threshold", 100))
                try:
                    self.world = r16_platoon_loader.build_r16_single_platoon_world(
                        folder_path=folder, tile_xy=tile_xy, flow_threshold=thr
                    )
                except Exception as exc:
                    return 400, {"ok": False, "reason": f"load_r16_platoon_failed:{exc}"}
                self.pre_game_editable = True
                self.last_turn_summary = None
                self._bump_terrain_rev()
                return 200, {"ok": True, "r16_source": self.world.meta.get("r16_source")}
            if op == "direct_action":
                target_kind = body.get("target_kind", "unit")
                target_id = body.get("target_id")
                action = str(body.get("action", "move"))
                goal = body.get("goal")
                append = bool(body.get("append", False))
                move_mode = str(body.get("move_mode", "normal"))
                fire_mode = str(body.get("fire_mode", "stop_to_fire"))
                follow_target = bool(body.get("follow_target", True))
                target_unit_id = body.get("target_unit_id")

                p = w.pieces.get(target_id)
                if p is None and target_kind == "commander":
                    c = w.commanders.get(target_id)
                    if c and getattr(c, "hq_piece_id", None):
                        p = w.pieces.get(c.hq_piece_id)

                if p is None:
                    return 404, {"ok": False, "reason": "target_entity_not_found"}

                # 撤退恢复期中不允许人工下令
                if float((p.meta or {}).get("retreat_recovery_hours", 0.0)) > 0.0:
                    return 409, {"ok": False, "reason": "retreat_recovery_uncontrollable"}

                if action == "retreat":
                    p.meta["task_queue"] = []
                    p.meta["retreat_recovery_hours"] = 1.0
                    return 200, {"ok": True, "action": "retreat", "state": self.serialize_state(commander_id=player_cid)}

                if action == "stop":
                    p.meta["task_queue"] = []
                    return 200, {"ok": True, "action": "stop", "state": self.serialize_state(commander_id=player_cid)}

                step = {
                    "action": action,
                    "goal": list(goal) if goal else list(p.pos),
                    "move_mode": move_mode,
                    "fire_mode": fire_mode,
                    "follow_target": follow_target,
                    "target_unit_id": target_unit_id,
                }

                if not append:
                    p.meta["task_queue"] = [step]
                else:
                    p.meta.setdefault("task_queue", []).append(step)

                return 200, {"ok": True, "task_queue": p.meta["task_queue"], "state": self.serialize_state(commander_id=player_cid)}

            if op == "reorder_task_queue":
                target_id = body.get("target_id") or body.get("piece_id")
                from_idx = int(body.get("from_index") if "from_index" in body else body.get("from_idx", 0))
                to_idx = int(body.get("to_index") if "to_index" in body else body.get("to_idx", 0))
                p = w.pieces.get(target_id)
                if not p:
                    cmd = w.commanders.get(target_id)
                    if cmd and getattr(cmd, "hq_piece_id", None) in w.pieces:
                        p = w.pieces[cmd.hq_piece_id]
                if not p:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                q = p.meta.setdefault("task_queue", [])
                if 0 <= from_idx < len(q) and 0 <= to_idx < len(q):
                    item = q.pop(from_idx)
                    q.insert(to_idx, item)
                    return 200, {"ok": True, "task_queue": q, "state": self.serialize_state(commander_id=player_cid)}
                return 400, {"ok": False, "reason": "index_out_of_range"}

            if op == "delete_task_step":
                target_id = body.get("target_id") or body.get("piece_id")
                step_idx = int(body.get("step_index") if "step_index" in body else body.get("step_idx", 0))
                p = w.pieces.get(target_id)
                if not p:
                    cmd = w.commanders.get(target_id)
                    if cmd and getattr(cmd, "hq_piece_id", None) in w.pieces:
                        p = w.pieces[cmd.hq_piece_id]
                if not p:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                q = p.meta.setdefault("task_queue", [])
                if 0 <= step_idx < len(q):
                    q.pop(step_idx)
                    return 200, {"ok": True, "task_queue": q, "state": self.serialize_state(commander_id=player_cid)}
                return 400, {"ok": False, "reason": "index_out_of_range"}

            if op == "update_queued_segment":
                pid = body.get("piece_id") or body.get("target_id")
                p = w.pieces.get(pid)
                if not p:
                    cmd = w.commanders.get(pid)
                    if cmd and getattr(cmd, "hq_piece_id", None) in w.pieces:
                        p = w.pieces[cmd.hq_piece_id]
                if not p:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                idx = int(body.get("segment_index") if "segment_index" in body else body.get("step_index", 0))
                q = p.meta.setdefault("task_queue", [])
                if 0 <= idx < len(q):
                    if "action" in body and body["action"]:
                        q[idx]["action"] = str(body["action"])
                    if "move_mode" in body and body["move_mode"]:
                        q[idx]["move_mode"] = str(body["move_mode"])
                    if "fire_mode" in body and body["fire_mode"]:
                        q[idx]["fire_mode"] = str(body["fire_mode"])
                    if "follow_target" in body:
                        q[idx]["follow_target"] = bool(body["follow_target"])
                    if "goal" in body and isinstance(body["goal"], (list, tuple)) and len(body["goal"]) >= 2:
                        q[idx]["goal"] = [float(body["goal"][0]), float(body["goal"][1])]
                    return 200, {"ok": True, "task_queue": q, "state": self.serialize_state(commander_id=player_cid)}
                return 400, {"ok": False, "reason": "index_out_of_range"}

            if op == "operate_heavy_unit":
                actor_id = body.get("piece_id") or body.get("actor_id")
                tgt_id = body.get("target_unit_id") or body.get("emplacement_id")
                payload = {
                    "target_unit_id": tgt_id,
                    "operator_soldier_ids": list(body.get("operator_soldier_ids") or []),
                    "operator_count": body.get("operator_count"),
                }
                dur = body.get("duration_h")
                res = w.event_manager.start_event(w, "operate_heavy_unit", actor_id, payload=payload, duration_hours=dur)
                if not res.get("ok"):
                    return 400, res
                return 200, {**res, "state": self.serialize_state(commander_id=player_cid)}

            if op == "deploy_heavy_unit":
                actor_id = body.get("piece_id") or body.get("actor_id")
                payload = {
                    "weapon": body.get("weapon"),
                    "target_unit_id": body.get("target_unit_id"),
                    "new_unit_id": body.get("new_unit_id"),
                    "unmanned": bool(body.get("unmanned", True)),
                    "keep_manned": bool(body.get("keep_manned", True)),
                    "operator_soldier_ids": list(body.get("operator_soldier_ids") or []),
                    "operator_count": body.get("operator_count"),
                }
                dur = body.get("duration_h")
                res = w.event_manager.start_event(w, "deploy_heavy_unit", actor_id, payload=payload, duration_hours=dur)
                if not res.get("ok"):
                    return 400, res
                if body.get("immediate") or w.turn == 0:
                    ev = w.event_manager.events.pop(res["event_id"], None)
                    if ev is not None:
                        comp = w.event_manager._complete_event(w, ev, w.pieces.get(actor_id))
                        res["completed"] = comp
                return 200, {**res, "state": self.serialize_state(commander_id=player_cid)}

            if op == "pack_heavy_unit":
                actor_id = body.get("piece_id") or body.get("actor_id")
                payload = {
                    "target_unit_id": body.get("target_unit_id"),
                    "attach": bool(body.get("attach", False)),
                    "detach_mobile": bool(body.get("detach_mobile", False)),
                }
                dur = body.get("duration_h")
                res = w.event_manager.start_event(w, "pack_heavy_unit", actor_id, payload=payload, duration_hours=dur)
                if not res.get("ok"):
                    return 400, res
                if body.get("immediate") or w.turn == 0:
                    ev = w.event_manager.events.pop(res["event_id"], None)
                    if ev is not None:
                        comp = w.event_manager._complete_event(w, ev, w.pieces.get(actor_id))
                        res["completed"] = comp
                return 200, {**res, "state": self.serialize_state(commander_id=player_cid)}

            if op == "demolition":
                actor_id = body.get("piece_id") or body.get("actor_id")
                p = w.pieces.get(actor_id)
                tgt_px = body.get("target_px") or (list(p.px) if p else [0, 0])
                payload = {
                    "target_px": [int(round(tgt_px[0])), int(round(tgt_px[1]))],
                    "target_type": str(body.get("target_type", "bridge")),
                    "labor_persons": int(body.get("labor_persons", 4)),
                }
                dur = body.get("duration_h")
                res = w.event_manager.start_event(w, "demolition", actor_id, payload=payload, duration_hours=dur)
                if not res.get("ok"):
                    return 400, res
                return 200, {**res, "state": self.serialize_state(commander_id=player_cid)}

            if op == "engineering":
                actor_id = body.get("piece_id") or body.get("actor_id")
                p = w.pieces.get(actor_id)
                tgt_px = body.get("target_px") or (list(p.px) if p else [0, 0])
                eng_op = str(body.get("eng_op") or body.get("sub_op") or "dig_trench")
                kind = "fortify" if eng_op == "dig_trench" and body.get("kind") == "fortify" else "engineering"
                payload = {
                    "op": eng_op,
                    "target_px": [int(round(tgt_px[0])), int(round(tgt_px[1]))],
                    "labor_persons": int(body.get("labor_persons", 4)),
                    "density": float(body.get("density", 1.0)),
                }
                dur = body.get("duration_h")
                res = w.event_manager.start_event(w, kind, actor_id, payload=payload, duration_hours=dur)
                if not res.get("ok"):
                    return 400, res
                return 200, {**res, "state": self.serialize_state(commander_id=player_cid)}

            if op == "lay_wire":
                actor_id = body.get("piece_id") or body.get("actor_id")
                p = w.pieces.get(actor_id)
                if p is None:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                tgt_px = body.get("target_px")
                if not tgt_px or len(tgt_px) < 2:
                    return 400, {"ok": False, "reason": "target_px_required"}
                r0, c0 = p.px
                r1, c1 = int(round(tgt_px[0])), int(round(tgt_px[1]))
                steps_n = max(1, max(abs(r1 - r0), abs(c1 - c0)))
                pts = []
                for si in range(steps_n + 1):
                    t = si / float(steps_n)
                    rc = (int(round(r0 + (r1 - r0) * t)), int(round(c0 + (c1 - c0) * t)))
                    if not pts or pts[-1] != rc:
                        pts.append(rc)
                w.grid.lay_wire(pts, p.team)
                self._bump_terrain_rev()
                return 200, {"ok": True, "op": "lay_wire", "cells": [list(pt) for pt in pts], "state": self.serialize_state(commander_id=player_cid)}

            if op == "cancel_event":
                eid = body.get("event_id")
                ev = w.event_manager.events.get(eid)
                if ev is None:
                    return 404, {"ok": False, "reason": "event_not_found"}
                actor_p = w.pieces.get(ev.actor_id)
                if player_cmd is not None and actor_p is not None and actor_p.team != player_cmd.team:
                    return 403, {"ok": False, "reason": "forbidden_cross_team"}
                w.event_manager.events.pop(eid, None)
                if ev.kind in ("operate_heavy_unit", "operate_vehicle"):
                    tgt_u = w.pieces.get(ev.payload.get("target_unit_id"))
                    if tgt_u is not None and tgt_u.operated_by == ev.actor_id:
                        tgt_u.operated_by = None
                return 200, {"ok": True, "cancelled_event_id": eid, "state": self.serialize_state(commander_id=player_cid)}

            if op == "move_soldier_item":
                from_kind = body.get("from_kind")
                to_kind = body.get("to_kind")
                idx = int(body.get("item_index", -1))
                bodies = w.meta.setdefault("ground_bodies", [])

                def _resolve_sol(uk, uid, sid):
                    u_p = w.pieces.get(uid)
                    if uk == "commander":
                        c_o = w.commanders.get(uid)
                        if c_o and getattr(c_o, "hq_piece_id", None) in w.pieces:
                            u_p = w.pieces[c_o.hq_piece_id]
                    if u_p is None:
                        return None, None
                    sols = self._ensure_piece_soldiers(u_p)
                    sol = next((s for s in sols if s.get("id") == sid), None)
                    return u_p, sol

                item_obj = None
                src_pos = None
                if from_kind == "soldier":
                    src_p, src_sol = _resolve_sol(body.get("from_unit_kind"), body.get("from_unit_id"), body.get("from_soldier_id"))
                    if src_p is None or src_sol is None or not (0 <= idx < len(src_sol.get("items", []))):
                        return 404, {"ok": False, "reason": "source_item_not_found"}
                    src_pos = src_p.pos
                    if to_kind == "ground":
                        item_obj = src_sol["items"].pop(idx)
                        bodies.append({
                            "id": f"gb-{len(bodies) + 1}-{src_p.id}",
                            "kind": "weapon" if item_obj.get("kind") in ("weapon", "radio") else "loot",
                            "team": src_p.team,
                            "pos": [round(float(src_p.pos[0]), 3), round(float(src_p.pos[1]), 3)],
                            "origin_unit_id": src_p.id,
                            "origin_unit_name": src_p.name,
                            "soldier": {"name": item_obj.get("name", "地面物资"), "items": [item_obj]},
                        })
                        self._ensure_piece_soldiers(src_p)
                        return 200, {"ok": True, "state": self.serialize_state(commander_id=player_cid)}
                    dst_p, dst_sol = _resolve_sol(body.get("to_unit_kind"), body.get("to_unit_id"), body.get("to_soldier_id"))
                    if dst_p is None or dst_sol is None:
                        return 404, {"ok": False, "reason": "target_soldier_not_found"}
                    if math.hypot(src_p.pos[0] - dst_p.pos[0], src_p.pos[1] - dst_p.pos[1]) > 1.35:
                        return 400, {"ok": False, "reason": "distance_too_far"}
                    item_obj = src_sol["items"].pop(idx)
                elif from_kind == "ground":
                    bid = body.get("from_body_id")
                    gb = next((b for b in bodies if b.get("id") == bid), None)
                    if gb is None or not (0 <= idx < len((gb.get("soldier") or {}).get("items", []))):
                        return 404, {"ok": False, "reason": "ground_item_not_found"}
                    src_pos = gb.get("pos") or [0, 0]
                    dst_p, dst_sol = _resolve_sol(body.get("to_unit_kind"), body.get("to_unit_id"), body.get("to_soldier_id"))
                    if dst_p is None or dst_sol is None:
                        return 404, {"ok": False, "reason": "target_soldier_not_found"}
                    if math.hypot(src_pos[0] - dst_p.pos[0], src_pos[1] - dst_p.pos[1]) > 1.35:
                        return 400, {"ok": False, "reason": "distance_too_far"}
                    item_obj = gb["soldier"]["items"].pop(idx)
                    if gb.get("kind") == "weapon" and not gb["soldier"]["items"]:
                        bodies.remove(gb)
                else:
                    return 400, {"ok": False, "reason": "invalid_from_kind"}

                if item_obj is not None and to_kind == "soldier":
                    item_obj["slot"] = str(body.get("to_slot") or "pack")
                    if "to_grid_x" in body and body["to_grid_x"] is not None:
                        item_obj["grid_x"] = max(0, int(body["to_grid_x"]))
                    if "to_grid_y" in body and body["to_grid_y"] is not None:
                        item_obj["grid_y"] = max(0, int(body["to_grid_y"]))
                    dst_sol.setdefault("items", []).append(item_obj)
                    self._ensure_piece_soldiers(dst_p)
                    if from_kind == "soldier":
                        self._ensure_piece_soldiers(src_p)
                    return 200, {"ok": True, "state": self.serialize_state(commander_id=player_cid)}
                return 400, {"ok": False, "reason": "invalid_transfer"}

            if op in ("pickup_body", "drop_body"):
                tgt_kind = body.get("target_kind")
                tgt_id = body.get("target_id")
                bid = body.get("body_id")
                p = w.pieces.get(tgt_id)
                if tgt_kind == "commander":
                    c_o = w.commanders.get(tgt_id)
                    if c_o and getattr(c_o, "hq_piece_id", None) in w.pieces:
                        p = w.pieces[c_o.hq_piece_id]
                if p is None:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                bodies = w.meta.setdefault("ground_bodies", [])
                carried = p.meta.setdefault("carried_bodies", [])
                if op == "pickup_body":
                    gb = next((b for b in bodies if b.get("id") == bid), None)
                    if gb is None:
                        return 404, {"ok": False, "reason": "body_not_found"}
                    bp = gb.get("pos") or [0, 0]
                    if math.hypot(bp[0] - p.pos[0], bp[1] - p.pos[1]) > 1.35:
                        return 400, {"ok": False, "reason": "distance_too_far"}
                    bodies.remove(gb)
                    carried.append(gb)
                else:
                    cb = next((b for b in carried if b.get("id") == bid), None)
                    if cb is None:
                        return 404, {"ok": False, "reason": "carried_body_not_found"}
                    carried.remove(cb)
                    cb["pos"] = [round(float(p.pos[0]), 3), round(float(p.pos[1]), 3)]
                    bodies.append(cb)
                return 200, {"ok": True, "state": self.serialize_state(commander_id=player_cid)}

            if op in ("radio_decoy", "cargo_transfer", "fortify", "resupply_pickup", "radio_silence"):
                actor_id = body.get("piece_id") or body.get("actor_id")
                p = w.pieces.get(actor_id)
                if p is None and body.get("entity_id") in w.commanders:
                    hq_pid = getattr(w.commanders[body["entity_id"]], "hq_piece_id", None)
                    if hq_pid:
                        actor_id = hq_pid
                        p = w.pieces.get(actor_id)
                if p is None:
                    return 404, {"ok": False, "reason": "piece_not_found"}
                tgt_px = body.get("target_px") or list(p.px)
                payload = {
                    "target_px": [int(round(tgt_px[0])), int(round(tgt_px[1]))],
                    "target_piece_id": body.get("target_piece_id"),
                    "ammo_t": float(body.get("ammo_t", 0.0)),
                    "food_t": float(body.get("food_t", 0.0)),
                    "fuel_t": float(body.get("fuel_t", 0.0)),
                    "ttl_hours": float(body.get("ttl_hours", 2.0)),
                }
                if op == "radio_silence":
                    sil_val = bool(body.get("radio_silence", not getattr(p, "radio_silence", False)))
                    p.radio_silence = sil_val
                    if p.direct_commander_id and p.direct_commander_id in w.commanders:
                        w.commanders[p.direct_commander_id].radio_silence = sil_val
                dur = body.get("duration_h")
                res = w.event_manager.start_event(w, op, actor_id, payload=payload, duration_hours=dur)
                if not res.get("ok"):
                    return 400, res
                return 200, {**res, "state": self.serialize_state(commander_id=player_cid)}

            if op == "clear_frontlines":
                target_cid = body.get("commander_id") or player_cid
                if player_cid and target_cid and not baguwen_v3.can_commander_write_line(player_cid, target_cid):
                    return 403, {"ok": False, "reason": "forbidden_write_other_commander_line"}
                line_ids = set(body.get("line_ids") or [])
                if line_ids:
                    w.grid.typed_frontlines = [
                        fl for fl in (w.grid.typed_frontlines or [])
                        if not (fl.get("line_id") in line_ids and (not player_cid or baguwen_v3.can_commander_write_line(player_cid, fl.get("commander_id"))))
                    ]
                elif target_cid:
                    w.grid.typed_frontlines = [
                        fl for fl in (w.grid.typed_frontlines or [])
                        if fl.get("commander_id") != target_cid
                    ]
                return 200, {"ok": True, "state": self.serialize_state(commander_id=player_cid)}

            if op == "estimate_plan_time":
                return self.handle_estimate_plan_time(body)

            if op == "reset_demo":
                self.world = build_default_demo_world()
                self.pre_game_editable = True
                self.last_turn_summary = None
                self._bump_terrain_rev()
                return 200, {"ok": True}
            return 400, {"ok": False, "reason": f"unknown_op:{op}"}

    def handle_commit_turn(self, body):
        with self.lock:
            w = self.world
            player_cid = body.get("player_commander_id")
            player_cmd = w.commanders.get(player_cid) if player_cid else None
            if player_cid and player_cmd is None:
                return 403, {"ok": False, "reason": "unknown_player_commander"}
            if player_cmd is not None:
                for mpid in list((body.get("moves") or {}).keys()):
                    mp = w.pieces.get(mpid)
                    if mp is not None and (mp.team != player_cmd.team or not self._commander_owns_piece(w, player_cmd, mpid)):
                        return 403, {"ok": False, "reason": f"forbidden_move_unit:{mpid}"}
                for bo in (body.get("baguwen_orders") or []):
                    bcid = bo.get("commander_id") or bo.get("commander")
                    bc = w.commanders.get(bcid) if bcid else None
                    if bc is not None and bc.team != player_cmd.team:
                        return 403, {"ok": False, "reason": f"forbidden_commander:{bcid}"}
                for qo in (body.get("lay_wire") or []) + (body.get("cargo_transfer") or []):
                    qpid = qo.get("piece_id")
                    qp = w.pieces.get(qpid) if qpid else None
                    if qp is not None and (qp.team != player_cmd.team or not self._commander_owns_piece(w, player_cmd, qpid)):
                        return 403, {"ok": False, "reason": f"forbidden_op_unit:{qpid}"}
            player_cid = body.get("player_commander_id")
            ao_list = []
            for zspec in (body.get("ao_zones") or []):
                tgt_cid = zspec.get("commander") or zspec.get("commander_id") or player_cid
                if player_cid and not baguwen_v3.can_commander_write_line(player_cid, tgt_cid):
                    return 403, {"ok": False, "reason": "forbidden_write_other_commander_line"}
                ao_list.append({"commander": tgt_cid, **zspec})

            b_orders = []
            for bo in (list(body.get("baguwen_orders") or []) + list(body.get("combat_log_orders") or [])):
                tgt_cid = bo.get("commander_id") or player_cid
                if player_cid and not baguwen_v3.can_commander_write_line(player_cid, tgt_cid):
                    return 403, {"ok": False, "reason": "forbidden_write_other_commander_line"}
                b_orders.append({"author_commander_id": player_cid, "commander_id": tgt_cid, **bo})

            fl_list = []
            for fl in (body.get("typed_frontlines") or []):
                tgt_cid = fl.get("commander_id") or fl.get("commander") or player_cid
                if player_cid and not baguwen_v3.can_commander_write_line(player_cid, tgt_cid):
                    return 403, {"ok": False, "reason": "forbidden_write_other_commander_line"}
                fl_list.append({"commander_id": tgt_cid, **fl})

            orders = {
                "ao_zones": ao_list,
                "typed_frontlines": fl_list,
                "combat_log_orders": b_orders,
                "events": body.get("events") or [],
                "fire": body.get("fire") or [],
            }
            if body.get("role"):
                orders["role"] = body["role"]
            if body.get("hours") is not None:
                orders["hours"] = float(body["hours"])

            self.pre_game_editable = False
            terr_sig_before = (len(w.grid.destroyed_bridges), len(w.grid.damaged_terrain))
            rep = adjudicator_v2.run_turn_v3(w, orders=orders)
            terr_sig_after = (len(w.grid.destroyed_bridges), len(w.grid.damaged_terrain))
            if terr_sig_after != terr_sig_before:
                self._bump_terrain_rev()
            self.last_turn_summary = {
                "turn": rep.get("turn"),
                "clock_hours": rep.get("clock_hours"),
                "subtick_count": rep.get("v3_subtick_count"),
                "total_hours": rep.get("v3_total_hours"),
                "warnings": rep.get("warnings", []),
            }
            state = self.serialize_state(commander_id=player_cid)
            return 200, {"ok": True, "report": self.last_turn_summary, "state": state}

    def handle_estimate_plan_time(self, body):
        with self.lock:
            w = self.world
            pid = body.get("piece_id") or body.get("target_id")
            p = w.pieces.get(pid)
            if p is None and pid in w.commanders:
                hq_pid = getattr(w.commanders[pid], "hq_piece_id", None)
                if hq_pid:
                    p = w.pieces.get(hq_pid)
            if not p:
                return 404, {"ok": False, "reason": "piece_not_found"}
            steps = body.get("steps") or (p.meta or {}).get("task_queue") or []
            cur_pos = list(p.pos)
            mob = p.mobility() if hasattr(p, "mobility") else "foot"
            base_kmh = float(p.base_speed_kmh(w.pieces)) if hasattr(p, "base_speed_kmh") else float(autonomy_v3.BASE_SPEED_KMH.get(mob, 4.5))
            cell_m = float(w.grid.cell_m)
            cur_clock = float(getattr(w, "clock_hours", 6.0))

            est_steps = []
            cum_minutes = 0.0
            for i, st in enumerate(steps):
                goal = st.get("goal") or cur_pos
                d_px = math.hypot(goal[0] - cur_pos[0], goal[1] - cur_pos[1])
                d_km_euclid = d_px * (cell_m / 1000.0)
                mm = str(st.get("move_mode", "normal"))
                eff_km = d_km_euclid
                if mm == "fast" and d_px > 0.5:
                    sp_px = (max(0, min(w.grid.shape[0] - 1, int(round(cur_pos[0])))), max(0, min(w.grid.shape[1] - 1, int(round(cur_pos[1])))))
                    gp_px = (max(0, min(w.grid.shape[0] - 1, int(round(goal[0])))), max(0, min(w.grid.shape[1] - 1, int(round(goal[1])))))
                    path = tf.shortest_path(w.grid, sp_px, gp_px, mob)
                    if path is not None and len(path[0]) > 1:
                        _, used_km, _ = tf.advance_along(*path, 1e6)
                        if used_km > 1e-4:
                            eff_km = float(used_km)
                elif d_px > 0.5:
                    # 沿着起止线段采样地面地形阻力，使 normal / slow 真实反映地形耗时
                    n_samp = max(2, min(48, int(math.ceil(d_px))))
                    t_costs = tf.TERRAIN_COST.get(mob, {})
                    cost_acc = 0.0
                    for si in range(n_samp):
                        frac = (si + 0.5) / float(n_samp)
                        sr = max(0, min(w.grid.shape[0] - 1, int(round(cur_pos[0] + (goal[0] - cur_pos[0]) * frac))))
                        sc = max(0, min(w.grid.shape[1] - 1, int(round(cur_pos[1] + (goal[1] - cur_pos[1]) * frac))))
                        tc = int(w.grid.terrain[sr, sc])
                        c_val = t_costs.get(tc, 2.5)
                        cost_acc += float(c_val) if math.isfinite(c_val) else 4.0
                    eff_km = d_km_euclid * (cost_acc / float(n_samp))

                mult = 1.20 if mm == "fast" else (0.55 if mm in ("stealth", "slow") else 1.0)
                eff_speed = max(0.3, base_kmh * mult)
                dur_h = eff_km / eff_speed if eff_km > 1e-4 else (5.0 / 60.0)
                dur_min = round(dur_h * 60.0, 2)
                cum_minutes += dur_min
                eta_h = cur_clock + (cum_minutes / 60.0)
                eta_hour = int(eta_h) % 24
                eta_min = int(round((eta_h - int(eta_h)) * 60.0)) % 60
                eta_str = f"{eta_hour:02d}:{eta_min:02d}"

                mode_cn = "快速" if mm == "fast" else ("慢速渗透" if mm in ("stealth", "slow") else "正常")
                fm = str(st.get("fire_mode", "stop_to_fire"))
                fire_cn = "移动射击" if fm == "fire_on_move" else "遇敌停下"
                act_cn = {"move": "移动", "assault": "战术进攻", "attack": "火力打击", "retreat": "撤退", "stop": "停止"}.get(st.get("action", "move"), "任务")

                est_steps.append({
                    "step_idx": i,
                    "action": st.get("action", "move"),
                    "action_cn": act_cn,
                    "goal": goal,
                    "move_mode": mm,
                    "mode_cn": mode_cn,
                    "fire_mode": fm,
                    "fire_cn": fire_cn,
                    "dist_px": round(d_px, 1),
                    "dist_km": round(eff_km, 2),
                    "duration_min": dur_min,
                    "eta": eta_str,
                })
                cur_pos = goal

            return 200, {
                "ok": True,
                "piece_id": pid,
                "total_steps": len(est_steps),
                "total_minutes": round(cum_minutes, 1),
                "steps": est_steps,
            }


def make_handler(app: GentactixV3App):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, format, *args):
            return

        def _send_json(self, status, obj):
            raw = json.dumps(obj, ensure_ascii=False).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(raw)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(raw)

        def _read_json(self):
            ln = int(self.headers.get("Content-Length", "0") or 0)
            if ln <= 0:
                return {}
            data = self.rfile.read(ln)
            return json.loads(data.decode("utf-8"))

        def do_GET(self):
            u = urlparse(self.path)
            if u.path in ("/", "/index.html"):
                html_path = WEB_DIR / "index.html"
                if not html_path.exists():
                    self.send_error(HTTPStatus.NOT_FOUND, "web/index.html missing")
                    return
                raw = html_path.read_bytes()
                self.send_response(HTTPStatus.OK)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
                return
            if u.path == "/api/state":
                qs = parse_qs(u.query)
                cid = (qs.get("commander_id") or [None])[0]
                team = (qs.get("team") or [None])[0]
                self._send_json(200, app.serialize_state(commander_id=cid, observer_team=team))
                return
            if u.path == "/api/terrain_raw.bin":
                raw = app.get_terrain_raw_bin()
                self.send_response(HTTPStatus.OK)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Length", str(len(raw)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(raw)
                return
            self.send_error(HTTPStatus.NOT_FOUND)

        def do_POST(self):
            u = urlparse(self.path)
            try:
                body = self._read_json()
            except Exception as exc:
                self._send_json(400, {"ok": False, "reason": f"invalid_json:{exc}"})
                return
            routes = {
                "/api/import_r16": app.handle_import_r16,
                "/api/edit_pixel": app.handle_edit_pixel,
                "/api/validate_ao": app.handle_validate_ao,
                "/api/weapon_los": app.handle_weapon_los,
                "/api/unit_op": app.handle_unit_op,
                "/api/commit_turn": app.handle_commit_turn,
                "/api/estimate_plan_time": app.handle_estimate_plan_time,
            }
            fn = routes.get(u.path)
            if fn is None:
                self.send_error(HTTPStatus.NOT_FOUND)
                return
            status, payload = fn(body)
            self._send_json(status, payload)

    return Handler


def start_server(host="127.0.0.1", port=8765, open_browser=False, app=None):
    app = app or GentactixV3App()
    srv = ThreadingHTTPServer((host, port), make_handler(app))
    if open_browser:
        threading.Timer(0.4, lambda: webbrowser.open(f"http://{host}:{srv.server_port}/")).start()
    return srv, app


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Gentactix V3 Tactical Sandbox Server")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--open", action="store_true")
    parser.add_argument("--demo", action="store_true", help="使用默认 64x64 演示地图")
    parser.add_argument("--r16-dir", default=r16_platoon_loader.DEFAULT_R16_DIR, help="实景 .r16/diffuse/river 数据目录")
    parser.add_argument("--tile-xy", default="1,1", help="选择加载的瓦片编号 x,y，默认 1,1")
    parser.add_argument("--flow-threshold", type=int, default=100, help="river flow 河道阈值")
    args = parser.parse_args()

    init_world = None
    if not args.demo and args.r16_dir and Path(args.r16_dir).is_dir():
        tx_s, ty_s = args.tile_xy.split(",")
        init_world = r16_platoon_loader.build_r16_single_platoon_world(
            folder_path=args.r16_dir,
            tile_xy=(int(tx_s), int(ty_s)),
            flow_threshold=int(args.flow_threshold),
        )
    app_inst = GentactixV3App(world=init_world)
    server, _ = start_server(args.host, args.port, open_browser=args.open, app=app_inst)
    print(f"Gentactix V3 running at http://{args.host}:{server.server_port}/")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.shutdown()
