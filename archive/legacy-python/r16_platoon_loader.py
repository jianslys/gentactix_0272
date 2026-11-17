# -*- coding: utf-8 -*-
"""Gentactix V3 实景 .r16 + diffuse + river 地形自动分类与单排战区加载器（严格 1:1 严禁降采样，200 m/px）。"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np
from PIL import Image

from gentactix import adjudicator_v2, command_v2
from gentactix import terrain_field as tf
from gentactix import toe

DEFAULT_R16_DIR = r"E:\商单1100\New folder"


def classify_terrain_from_r16_diffuse_river(r16_raw, diffuse_rgb, river_flow, flow_threshold=100):
    """基于 1:1 .r16 原始高程、diffuse 地表 RGB 与 river 汇流流量判定像素地形编码。

    判定优先级（后写覆盖前写，水文河道绝对优先）：
      0 = PLAIN (平原默认基底)
      1 = FOREST (diffuse 植被绿度显著: G-R>=7 & G>=95 & B<85)
      3 = SWAMP (低海拔高湿暗青漫滩: B>=68 & G>=R & r16<4300 & river<=thr)
      2 = MOUNTAIN (高海拔山脊 r16>=5200 或深暗裸岩 R+G+B<255)
      4 = WATER (river 汇流通道 > thr 或弥漫水体蓝 B>R+8 & B>G-10)
    返回 (terrain_uint8, feature_masks_dict)。
    """
    r16 = np.asarray(r16_raw)
    rgb = np.asarray(diffuse_rgb)
    riv = np.asarray(river_flow)
    if r16.shape != riv.shape or rgb.shape[:2] != r16.shape:
        raise ValueError(
            f"通道尺寸不一致（严禁降采样）：r16={r16.shape}, diffuse={rgb.shape[:2]}, river={riv.shape}"
        )

    R = rgb[:, :, 0].astype(np.int16)
    G = rgb[:, :, 1].astype(np.int16)
    B = rgb[:, :, 2].astype(np.int16)
    thr = int(flow_threshold)

    terr = np.full(r16.shape, tf.PLAIN, dtype=np.uint8)
    is_forest = (G - R >= 7) & (G >= 95) & (B < 85)
    terr[is_forest] = tf.FOREST

    is_swamp = (B >= 68) & (G >= R) & (r16 < 4300) & (riv <= thr)
    terr[is_swamp] = tf.SWAMP

    is_mtn = (r16 >= 5200) | ((R + G + B) < 255)
    terr[is_mtn] = tf.MOUNTAIN

    is_water = (riv > thr) | ((B > R + 8) & (B > G - 10))
    terr[is_water] = tf.WATER

    feature_masks = {
        "forest": (terr == tf.FOREST),
        "swamp": (terr == tf.SWAMP),
        "mountain": (terr == tf.MOUNTAIN),
        "water": (terr == tf.WATER),
    }
    return terr, feature_masks


def _find_passable_cell(terrain, preferred_rc, forbidden=None):
    H, W = terrain.shape
    pr, pc = int(preferred_rc[0]), int(preferred_rc[1])
    pr = min(max(0, pr), H - 1)
    pc = min(max(0, pc), W - 1)
    forb = forbidden or set()
    if int(terrain[pr, pc]) in (tf.PLAIN, tf.FOREST) and (pr, pc) not in forb:
        return (pr, pc)
    best = None
    best_d2 = 10**12
    for rad in range(1, 25):
        for r in range(max(0, pr - rad), min(H, pr + rad + 1)):
            for c in range(max(0, pc - rad), min(W, pc + rad + 1)):
                if int(terrain[r, c]) in (tf.PLAIN, tf.FOREST) and (r, c) not in forb:
                    d2 = (r - pr) ** 2 + (c - pc) ** 2
                    if d2 < best_d2:
                        best_d2 = d2
                        best = (r, c)
        if best is not None:
            return best
    return (pr, pc)


def build_r16_single_platoon_world(
    folder_path=DEFAULT_R16_DIR,
    tile_xy=(1, 1),
    flow_threshold=100,
):
    """加载 1 张真实 4096x4096 .r16 + diffuse + river（200 m/px 无降采样），部署 1 个步兵排。"""
    base = Path(folder_path)
    tx, ty = int(tile_xy[0]), int(tile_xy[1])
    r16_path = base / f"1100_x{tx}_y{ty}.r16"
    diff_path = base / f"diffuse_x0_y0_x{tx}_y{ty}.png"
    riv_path = base / f"river_x{tx}_y{ty}.png"

    if not r16_path.exists():
        raise FileNotFoundError(f"找不到 .r16 高程文件: {r16_path}")
    if not diff_path.exists():
        raise FileNotFoundError(f"找不到 diffuse 色彩图: {diff_path}")
    if not riv_path.exists():
        raise FileNotFoundError(f"找不到 river 流量图: {riv_path}")

    with Image.open(diff_path) as im_diff:
        diff_rgb = np.asarray(im_diff.convert("RGB"), dtype=np.uint8)
    H, W = diff_rgb.shape[:2]

    with Image.open(riv_path) as im_riv:
        riv_flow = np.asarray(im_riv)
    if riv_flow.ndim == 3:
        riv_flow = riv_flow[:, :, 0]

    raw_bytes = r16_path.read_bytes()
    if len(raw_bytes) != H * W * 2:
        raise ValueError(f".r16 字节长度 {len(raw_bytes)} 与 {H}x{W} 不匹配")
    r16_raw = np.frombuffer(raw_bytes, dtype=np.dtype("<u2")).reshape((H, W))

    terrain, feat_masks = classify_terrain_from_r16_diffuse_river(
        r16_raw, diff_rgb, riv_flow, flow_threshold=flow_threshold
    )
    elev = tf.map_elevation_channel(r16_raw.astype(np.float32), low_meter=120.0, high_meter=1480.0)

    g = tf.Grid(elev, cell_m=200.0, origin_latlon=(34.25, 108.95), terrain=terrain)
    for fid, mask in feat_masks.items():
        spec = tf.TERRAIN_FEATURE_SPECS.get(fid, {})
        g.feature_channels[fid] = {
            "mask": mask,
            "modifiers": None,
            "terrain_code": spec.get("terrain_code"),
        }
    g.refresh_attrition()

    # 检查并选择兼具河流/森林/平原/山地的局部战区锚点（默认 x1_y1 为 696, 2700）
    anchor = _find_passable_cell(terrain, (696, 2700))
    ar, ac = anchor

    pos_cmd = anchor
    pos_sq1 = _find_passable_cell(terrain, (ar, ac - 1))
    pos_sq2 = _find_passable_cell(terrain, (ar, ac + 1))
    pos_sq3 = _find_passable_cell(terrain, (ar + 1, ac - 1))
    pos_mg = anchor
    pos_stash = _find_passable_cell(terrain, (ar - 1, ac - 2))
    pos_blue = _find_passable_cell(terrain, (ar - 1, ac + 4))

    g.stash_put(
        pos_stash,
        resources={"ammo": 2.5, "food": 1.8},
        equipment={"radio": 1},
        owner_team="red",
        radio_channel="A",
    )

    world = adjudicator_v2.World(g, hours=0.25, role_echelon="platoon")

    def _make_rifle_squad_piece(pid, name, px):
        sq = toe.make_squad("rifle_squad", f"{pid}-sq", enlisted=10, cadre=1)
        node = toe.Node(pid, name, "squad", squad=sq)
        p = toe.MapPiece(pid, name, "red", px, node, level="squad", footprint_px=1, radio_channel="A")
        p.set_full_stock(1.0)
        return p

    p_sq1 = _make_rifle_squad_piece("red-sq-1", "红1排1班(步兵)", pos_sq1)
    p_sq2 = _make_rifle_squad_piece("red-sq-2", "红1排2班(步兵)", pos_sq2)
    p_sq3 = _make_rifle_squad_piece("red-sq-3", "红1排3班(步兵)", pos_sq3)

    sq_mg = toe.make_squad(
        "mg_squad",
        "red-sq-mg-sq",
        enlisted=7,
        cadre=1,
        equipment={"hmg": 1, "mortar_81": 1, "rifle": 4, "radio": 1},
    )
    p_mg = toe.MapPiece(
        "red-sq-mg",
        "红1排机炮火力班",
        "red",
        pos_mg,
        toe.Node("red-sq-mg", "红1排机炮火力班", "squad", squad=sq_mg),
        level="squad",
        footprint_px=1,
        radio_channel="A",
    )
    p_mg.set_full_stock(1.0)

    p_blue = toe.make_squad_piece(
        "blue-recon-1", "蓝方前出警戒班", "blue", pos_blue, "recon_squad", radio_channel="B"
    )
    p_blue.set_full_stock(1.0)

    for p in (p_sq1, p_sq2, p_sq3, p_mg, p_blue):
        world.add_piece(p)

    cmd_pl = toe.Commander(
        "cmd-pl-1",
        "红1排排长",
        "red",
        pos_cmd,
        rank="platoon_cmd",
        echelon="platoon",
        radio_channel="A",
        attached_to="red-sq-mg",
        subordinate_ids=["red-sq-1", "red-sq-2", "red-sq-3", "red-sq-mg"],
    )
    world.add_commander(cmd_pl)

    # 排级 AO 限额 max_area_px=100；围绕 anchor 构建 9x10=90 格 AO 区域
    ao_cells = [
        (r, c)
        for r in range(max(0, ar - 4), min(H, ar + 5))
        for c in range(max(0, ac - 5), min(W, ac + 5))
    ]
    ao_pl = command_v2.OperationalZone(
        "ao-pl1-init",
        "cmd-pl-1",
        "red",
        "platoon",
        ao_cells,
        activate_turn=0,
    )
    command_v2.register_zone(world.ao_zones, ao_pl, world.commanders, current_turn=0, pieces=world.pieces, grid=g)

    g.write_typed_frontline(
        "fl-pl1-deploy",
        "red",
        [[ar - 1, ac + 1], [ar + 1, ac + 1]],
        unit_type_mask=tf.LINE_MASK_INFANTRY | tf.LINE_MASK_HEAVY_AT,
        attraction_weight=1.15,
        front_normal=[0.0, 1.0],
        commander_id="cmd-pl-1",
        zone_id="ao-pl1-init",
        line_role=tf.LINE_ROLE_DEPLOY,
    )
    g.write_typed_frontline(
        "fl-pl1-global",
        "red",
        [[ar, ac - 2], [ar, ac + 4]],
        unit_type_mask=tf.LINE_MASK_ALL,
        attraction_weight=1.0,
        front_normal=[0.0, 1.0],
        commander_id="cmd-pl-1",
        zone_id="ao-pl1-init",
        line_role=tf.LINE_ROLE_GLOBAL_ACTION,
    )

    vals, cnts = np.unique(terrain, return_counts=True)
    stats = {
        tf.TERRAIN_NAMES.get(int(k), str(k)): {
            "code": int(k),
            "pixels": int(v),
            "pct": round(float(v) / float(terrain.size) * 100.0, 2),
        }
        for k, v in zip(vals, cnts)
    }
    world.meta["r16_source"] = {
        "folder": str(base),
        "tile_xy": [tx, ty],
        "shape": [int(H), int(W)],
        "cell_m": 200.0,
        "anchor_px": list(anchor),
        "terrain_stats": stats,
    }
    return world


if __name__ == "__main__":
    w = build_r16_single_platoon_world()
    info = w.meta["r16_source"]
    print(f"Loaded tile x{info['tile_xy'][0]}_y{info['tile_xy'][1]} shape={info['shape']} cell_m={info['cell_m']}")
    print("Terrain classification breakdown:")
    for name, s in info["terrain_stats"].items():
        print(f"  {name} (code={s['code']}): {s['pixels']} px ({s['pct']}%)")
    print(f"Anchor px: {info['anchor_px']}")
    print("Commanders:", [(c.id, c.name, c.px, c.subordinate_ids) for c in w.commanders.values()])
    print("Pieces:", [(p.id, p.name, p.team, p.px, p.persons()) for p in w.pieces.values()])
