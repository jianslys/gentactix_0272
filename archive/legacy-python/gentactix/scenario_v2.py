# -*- coding: utf-8 -*-
"""场景规格 → World。让对话里的推演者只需写一份 JSON 就能开局。

规格示例见 `scenario_demo()`。坐标可写像素 px=[row,col]，也可写 latlon=[lat,lon]。
"""

from __future__ import annotations

from . import artillery as ar
from . import command_v2
from . import logistics as lg
from . import terrain_field as tf
from . import toe, ww2_air as air
from .adjudicator_v2 import World


def _pos(grid, spec):
    if "px" in spec:
        return (int(spec["px"][0]), int(spec["px"][1]))
    r, c = grid.latlon_to_pixel(spec["latlon"][0], spec["latlon"][1])
    return grid.clamp((r, c))


def build_grid(spec):
    for fid, fspec in (spec.get("register_terrain_features") or {}).items():
        tf.register_terrain_feature(fid, fspec)
    used_raw = False
    if "npz" in spec:
        g = tf.Grid.load_npz(spec["npz"])
    elif "raw_elevation" in spec or "from_raw" in spec:
        raw_in = spec.get("from_raw") if "from_raw" in spec else spec["raw_elevation"]
        raw_cfg = {"source": raw_in} if not isinstance(raw_in, dict) else dict(raw_in)
        elev_src = raw_cfg.pop("source", raw_cfg.pop("elevation_source", None))
        if elev_src is None:
            raise ValueError("raw_elevation / from_raw 需要 source 或 elevation_source")
        t_ch = raw_cfg.pop("terrain_channel", spec.get("terrain_channel"))
        t_cmap = raw_cfg.pop("terrain_code_map", spec.get("terrain_code_map"))
        f_chs = raw_cfg.pop("feature_channels", spec.get("feature_channels"))
        g = tf.Grid.from_raw_channels(
            elev_src,
            terrain_channel=t_ch,
            terrain_code_map=t_cmap,
            feature_channels=f_chs,
            **raw_cfg,
        )
        used_raw = True
    elif "synthetic" in spec:
        g = tf.synthetic_grid(**spec["synthetic"])
    else:
        raise ValueError("grid 规格需要 npz、raw_elevation/from_raw 或 synthetic")

    if not used_raw and spec.get("feature_channels"):
        priority = {
            "plain": 0, "water": 1, "swamp": 2, "forest": 3, "mountain": 4,
            "urban": 5, "airfield": 6, "dirt_road": 7, "rail": 8, "highway": 9, "bridge": 10,
        }
        ordered = sorted(spec["feature_channels"].items(), key=lambda kv: priority.get(str(kv[0]), 20))
        for fid, item in ordered:
            if isinstance(item, dict):
                src = item["source"] if "source" in item else (item["data"] if "data" in item else item["mask"])
                g.apply_feature_channel(
                    fid, src,
                    threshold=item.get("threshold", 0.5),
                    equals=item.get("equals"),
                    modifiers=item.get("modifiers"),
                    sync_terrain=item.get("sync_terrain", True),
                    terrain_code=item.get("terrain_code"),
                    bit_depth=item.get("bit_depth"),
                    flip_y=item.get("flip_y", False),
                )
            else:
                g.apply_feature_channel(fid, item)

    for tf_item in spec.get("terrain_features") or []:
        g.add_terrain_feature(
            _pos(g, tf_item),
            tf_item["feature"],
            modifiers=tf_item.get("modifiers"),
            ttl_hours=tf_item.get("ttl_hours"),
            sync_terrain=tf_item.get("sync_terrain", True),
            terrain_code=tf_item.get("terrain_code"),
        )
    return g


def build_piece(grid, d):
    px = _pos(grid, d)
    pid = d["id"]
    if "vehicle_model" in d:
        piece = toe.make_vehicle_piece(
            pid, d.get("name", pid), d["team"], px, d["vehicle_model"],
            crew_enlisted=d.get("crew_enlisted", 0), crew_cadre=d.get("crew_cadre", 0),
            org=d.get("org", 100.0), radio_silence=d.get("radio_silence", False),
            radio_channel=d.get("radio_channel", "A"), cargo=d.get("cargo"),
            facing_deg=d.get("facing_deg", 0.0), pos=d.get("pos"),
            attached_to=d.get("attached_to"), unit_state=d.get("unit_state"),
            operated_by=d.get("operated_by"),
            roe=d.get("roe", "return_fire"), meta=d.get("meta"),
        )
        piece.set_full_stock(d.get("stock_fraction", 1.0))
        return piece
    if "division" in d:
        dv = d["division"]
        root = toe.build_division(pid, dv.get("kind", "infantry"), dv.get("strength", 1.0), dv.get("org", 100.0))
    else:
        strength, org = d.get("strength", 1.0), d.get("org", 100.0)
        sq_specs = d.get("custom_squads") if "custom_squads" in d else d.get("squads", [])
        nodes = []
        for i, item in enumerate(sq_specs):
            sid = f"{pid}-{i + 1}"
            if isinstance(item, str):
                sq = toe.make_squad(item, sid, strength, org)
            elif isinstance(item, dict):
                sid = item.get("id", sid)
                tpl = item.get("template") or (
                    item.get("kind") if item.get("kind") in toe.SQUAD_TEMPLATES and "equipment" not in item and "enlisted" not in item else None
                )
                if tpl:
                    sq = toe.make_squad(
                        tpl, sid,
                        strength=item.get("strength", strength),
                        org=item.get("org", org),
                        equipment=item.get("equipment"),
                        buffs=item.get("buffs"),
                        enlisted=item.get("enlisted"),
                        cadre=item.get("cadre"),
                    )
                else:
                    sq = toe.make_custom_squad(
                        sid,
                        enlisted=item.get("enlisted", 8),
                        cadre=item.get("cadre", 1),
                        equipment=item.get("equipment"),
                        buffs=item.get("buffs"),
                        kind=item.get("kind", "custom_squad"),
                        org=item.get("org", org),
                        enlisted_full=item.get("enlisted_full"),
                        cadre_full=item.get("cadre_full"),
                        equipment_full=item.get("equipment_full"),
                    )
            else:
                raise ValueError(f"不支持的 squad 配置项: {item}")
            nodes.append(toe.Node(sid, sid, "squad", squad=sq))
        root = toe.Node(pid, d.get("name", pid), d.get("level", "company"), nodes)
    piece = toe.MapPiece(pid, d.get("name", pid), d["team"], px, root, d.get("level", "division"),
                         d.get("radio_silence", False), d.get("decoy", False), d.get("fortification", 0.0),
                         d.get("footprint_px", 16), d.get("control_radius_km", 3.0), d.get("fake_strength"),
                         radio_channel=d.get("radio_channel", "A"),
                         is_heavy_emplacement=d.get("is_heavy_emplacement", False),
                         deployed_weapon=d.get("deployed_weapon"),
                         pos=d.get("pos"), attached_to=d.get("attached_to"),
                         unit_state=d.get("unit_state"), operated_by=d.get("operated_by"),
                         facing_deg=d.get("facing_deg", 0.0),
                         roe=d.get("roe", "return_fire"), meta=d.get("meta"))
    piece.set_full_stock(d.get("stock_fraction", 1.0))
    return piece


def build_world(spec):
    for fid, fspec in (spec.get("register_terrain_features") or {}).items():
        tf.register_terrain_feature(fid, fspec)
    for model, eq_spec in (spec.get("register_equipment") or {}).items():
        toe.register_equipment(model, eq_spec)
    for kind, tpl_spec in (spec.get("register_squad_templates") or {}).items():
        toe.register_squad_template(kind, tpl_spec)
    grid = build_grid(spec["grid"])
    w = World(grid, hours=spec.get("hours", 6.0), night=spec.get("night", False))
    if "weather" in spec:
        grid.set_weather(getattr(tf, spec["weather"].upper()))
    for d in spec.get("pieces", []):
        w.add_piece(build_piece(grid, d))
    for c in spec.get("commanders", []):
        cmd = toe.Commander(
            c["id"], c.get("name", c["id"]), c["team"], _pos(grid, c),
            rank=c.get("rank", "company_cmd"), echelon=c.get("echelon"),
            unit_id=c.get("unit_id"), subordinate_ids=c.get("subordinate_ids"),
            has_radio=c.get("has_radio", True), radio_silence=c.get("radio_silence", False),
            radio_channel=c.get("radio_channel", "A"), cadre_piece_id=c.get("cadre_piece_id"),
            parent_commander_id=c.get("parent_commander_id"),
            attached_to=c.get("attached_to"), meta=c.get("meta"),
        )
        w.add_commander(cmd)
    for z in spec.get("ao_zones", []):
        cmd = w.commanders.get(z["owner_id"])
        if cmd is not None:
            cells = command_v2.build_ao_cells(grid, z["geometry"], center_px=cmd.px)
            command_v2.register_ao_zone(w.ao_zones, cmd, cells, z["id"], current_turn=w.turn, grid=grid)
    for st in spec.get("cell_stash", []):
        grid.stash_put(
            _pos(grid, st),
            resources=st.get("resources"),
            equipment=st.get("equipment"),
            owner_team=st.get("owner_team"),
            radio_channel=st.get("radio_channel"),
        )
    for we in spec.get("wire_edges", []):
        grid.add_wire_link(we["team"], tuple(we["a"]), tuple(we["b"]))
    for d in spec.get("depots", []):
        w.add_depot(lg.Depot(d["id"], _pos(grid, d), d["team"], d.get("stock"), ammo_by_type=d.get("ammo_by_type")))
    for d in spec.get("batteries", []):
        w.add_battery(ar.Battery(d["id"], d["team"], _pos(grid, d), d["tubes"], d.get("caliber", "medium"),
                                 d.get("range_km", 11.0), d.get("rate_per_tube_h", 30.0), d.get("ammo"),
                                 attached_to=d.get("attached_to")))
    for d in spec.get("wings", []):
        over = {k: d[k] for k in ("range_km", "loiter_h", "turnaround_h", "payload") if k in d}
        w.add_wing(air.AirWing(d["id"], d["team"], _pos(grid, {"px": d["base_px"]} if "base_px" in d else d),
                               d["role"], d["aircraft"], d.get("quality", 1.0), **over))
    return w


def scenario_demo():
    """演示场景：合成地形上一个红方装甲群对蓝方步兵师。"""
    return {
        "grid": {"synthetic": {"h": 120, "w": 160}},
        "hours": 6,
        "pieces": [
            {"id": "R-ARM", "team": "red", "px": [84, 20], "level": "brigade",
             "squads": ["tank_squad"] * 4 + ["mech_squad"] * 2 + ["transport_squad", "porter_squad", "porter_squad",
                                                                "hq_squad"]},
            {"id": "B-INF", "team": "blue", "px": [84, 70], "level": "division",
             "squads": ["rifle_squad"] * 6 + ["mg_squad"] * 2 + ["at_squad", "gun_squad", "gun_squad", "hq_squad",
                                                                "recon_squad"]},
        ],
        "depots": [{"id": "D-R", "team": "red", "px": [84, 3],
                    "stock": {"food": 200, "ammo": 200, "fuel": 200, "replacements": 500}},
                   {"id": "D-B", "team": "blue", "px": [84, 85], "stock": {"food": 200, "ammo": 200, "fuel": 200}}],
        "batteries": [{"id": "R-ART", "team": "red", "px": [84, 22], "tubes": 12,
                       "ammo": {"he": 1000, "smoke": 200, "ap": 100}}],
        "wings": [{"id": "R-CAS", "team": "red", "base_px": [82, 149], "role": "attack", "aircraft": 20}],
    }
