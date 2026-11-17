# -*- coding: utf-8 -*-
"""V2 战术图渲染：地形底图 / 坡度 / 道路 / 烟幕能见度 / 火力封锁 / 迷雾遮罩 + 军标图层。

纯 numpy 自带 PNG 编码（不依赖 PIL）；folium 作为唯一第三方渲染库，不 import streamlit。
view_team=None → 上帝视角（真值）；view_team='red'/'blue' → 该方迷雾视角（己方精确 + 敌方模糊接触）。
"""

from __future__ import annotations

import base64
import math
import struct
import zlib

import folium
import numpy as np

from . import recon_fog
from . import terrain_field as tf

_TERRAIN_RGB = {
    tf.PLAIN: (196, 206, 150), tf.FOREST: (84, 128, 78), tf.MOUNTAIN: (150, 130, 110),
    tf.SWAMP: (120, 150, 130), tf.WATER: (70, 120, 190), tf.URBAN: (170, 160, 160),
    tf.DIRT_ROAD: (176, 140, 90), tf.HIGHWAY: (235, 200, 80), tf.RAIL: (60, 60, 60),
    tf.BRIDGE: (255, 120, 40), tf.AIRFIELD: (210, 210, 235),
}


def _png_bytes(rgba):
    rgba = np.ascontiguousarray(rgba.astype(np.uint8))
    h, w, _ = rgba.shape
    raw = b"".join(b"\x00" + rgba[y].tobytes() for y in range(h))

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def _data_uri(rgba):
    return "data:image/png;base64," + base64.b64encode(_png_bytes(rgba)).decode("ascii")


def terrain_rgba(grid, hillshade=True):
    h, w = grid.shape
    img = np.zeros((h, w, 4), np.uint8)
    img[..., 3] = 255
    for code, rgb in _TERRAIN_RGB.items():
        img[grid.terrain == code, :3] = rgb
    if hillshade:
        shade = np.clip(0.75 + 2.2 * (grid.grad_x + grid.grad_y), 0.35, 1.15)
        img[..., :3] = np.clip(img[..., :3] * shade[..., None], 0, 255)
    return img


def slope_rgba(grid):
    s = np.clip(grid.slope_deg / 35.0, 0, 1)
    img = np.zeros(grid.shape + (4,), np.uint8)
    img[..., 0] = 255
    img[..., 1] = (200 * (1 - s)).astype(np.uint8)
    img[..., 3] = (s * 170).astype(np.uint8)
    return img


def smoke_rgba(grid):
    dark = np.clip(1.0 - grid.visibility_mod, 0, 1)
    img = np.zeros(grid.shape + (4,), np.uint8)
    img[..., :3] = 235
    img[..., 3] = (dark * 200).astype(np.uint8)
    return img


def hazard_rgba(field):
    img = np.zeros(field.shape + (4,), np.uint8)
    img[..., 0] = 255
    img[..., 1] = 40
    img[..., 3] = (np.clip(field, 0, 1) * 190).astype(np.uint8)
    return img


def fog_rgba(world, team):
    """己方视野之外压暗。"""
    g = world.grid
    seen = np.zeros(g.shape, bool)
    vis = tf.visibility_field(g, world.night)
    for p in world.pieces.values():
        if p.team == team:
            r_px = g.km_to_px(recon_fog.observe_radius_km(p))
            seen |= tf.disk_mask(g.shape, p.px, max(2.0, r_px * float(vis[p.px])))
    img = np.zeros(g.shape + (4,), np.uint8)
    img[..., 3] = np.where(seen, 0, 150)
    return img


def _bounds(grid):
    top_left = grid.pixel_to_latlon((-0.5, -0.5))
    bottom_right = grid.pixel_to_latlon((grid.shape[0] - 0.5, grid.shape[1] - 0.5))
    return [[bottom_right[0], top_left[1]], [top_left[0], bottom_right[1]]]


def _unit_html(name, symbol, team, label=None, faded=False):
    if team == "red":
        frame = ("width:22px;height:22px;background:rgba(255,180,180,.88);border:2px solid #cc0000;"
                 "transform:rotate(45deg);")
        inner = f"<div style='position:absolute;top:0;left:0;width:30px;height:30px;display:flex;align-items:center;justify-content:center;font-weight:bold;font-size:18px'>{symbol}</div>"
        box = f"<div style='position:relative;width:30px;height:30px'><div style='position:absolute;top:3px;left:3px;{frame}'></div>{inner}</div>"
    else:
        box = (f"<div style='width:36px;height:22px;background:rgba(180,210,255,.88);border:2px solid #0044cc;"
               f"display:flex;align-items:center;justify-content:center;font-weight:bold;font-size:18px'>{symbol}</div>")
    op = "0.55" if faded else "1"
    return (f"<div style='display:flex;flex-direction:column;align-items:center;width:130px;margin-left:-65px;"
            f"margin-top:-15px;opacity:{op}'>{box}<div style='background:rgba(0,0,0,.72);color:#fff;font-size:11px;"
            f"padding:2px 5px;border-radius:3px;margin-top:3px;white-space:nowrap'>{label or name}</div></div>")


def _piece_symbol(piece):
    eq = piece.totals()["equipment"]
    if eq.get("tank_medium", 0) + eq.get("tank_heavy", 0) > 5:
        return "<div style='width:14px;height:6px;border:2px solid black;border-radius:50%'></div>"
    if eq.get("howitzer_105", 0) + eq.get("howitzer_152", 0) > 5 and eq.get("rifle", 0) < 30:
        return "●"
    return "X"


def build_map_v2(world, view_team=None, show_slope=False, report=None):
    g = world.grid
    cr, cc = g.shape[0] / 2.0, g.shape[1] / 2.0
    m = folium.Map(location=list(g.pixel_to_latlon((cr, cc))), zoom_start=11, tiles=None,
                   attr="GenTactix V2")
    folium.TileLayer(tiles="https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}",
                     attr="Esri World Imagery", name="🛰️ 卫星（真实地球原型时可用）", overlay=False, show=False).add_to(m)
    bounds = _bounds(g)
    folium.raster_layers.ImageOverlay(_data_uri(terrain_rgba(g)), bounds, name="🗺️ 像素地形（含山体阴影/道路/桥梁/机场）",
                                      opacity=1.0, interactive=False, show=True).add_to(m)
    if show_slope:
        folium.raster_layers.ImageOverlay(_data_uri(slope_rgba(g)), bounds, name="⛰️ 微积分坡度热力",
                                          opacity=0.8, show=True).add_to(m)
    if float(g.visibility_mod.min()) < 1.0:
        folium.raster_layers.ImageOverlay(_data_uri(smoke_rgba(g)), bounds, name="💨 烟幕（能见度）", opacity=1.0).add_to(m)
    ff = getattr(world, "last_fire_fields", None)
    if ff is not None:
        for team, field in sorted(ff.hazard.items()):
            if float(field.max()) > 0:
                folium.raster_layers.ImageOverlay(_data_uri(hazard_rgba(field)), bounds,
                                                  name=f"🔥 火力封锁区（对{team}）", opacity=1.0).add_to(m)
    if view_team:
        folium.raster_layers.ImageOverlay(_data_uri(fog_rgba(world, view_team)), bounds,
                                          name=f"🌫️ {view_team} 方战争迷雾", opacity=1.0).add_to(m)

    units = folium.FeatureGroup(name="🪖 作战群", show=True)
    if view_team and report and view_team in report.get("fog", {}):
        view = report["fog"][view_team]
        for p in world.pieces.values():
            if p.team == view_team:
                _marker(g, units, p.px, _unit_html(p.name, _piece_symbol(p), p.team,
                                                   f"{p.name} 兵{p.persons()} 组{p.mean_org():.0f}"),
                        f"{p.name}<br>兵力 {p.persons()}｜平均组织度 {p.mean_org():.0f}｜静默 {p.radio_silence}")
        for c in view["contacts"]:
            enemy = "blue" if view_team == "red" else "red"
            lo, hi = c["persons_range"]
            _marker(g, units, c["px"], _unit_html(c["name"], "?", enemy,
                                                  f"{c['name']} ≈{lo}-{hi}", faded=c["level"] < 3),
                    f"情报等级 {c['level']}｜来源 {','.join(c['sources'])}" + ("｜<b>假目标</b>" if c.get("decoy") else ""))
    else:
        for p in world.pieces.values():
            tag = "（佯动）" if p.decoy else ""
            _marker(g, units, p.px, _unit_html(p.name, _piece_symbol(p), p.team,
                                               f"{p.name}{tag} 兵{p.persons()} 组{p.mean_org():.0f}"),
                    f"{p.name}<br>兵力 {p.persons()}｜平均组织度 {p.mean_org():.0f}｜骨干 "
                    f"{p.totals()['cadre']}/{p.totals()['cadre_full']}｜静默 {p.radio_silence}")
    units.add_to(m)

    support = folium.FeatureGroup(name="📦 炮兵/航空/补给/遗留装备", show=True)
    for b in world.batteries.values():
        if view_team and b.team != view_team:
            continue
        folium.CircleMarker(g.pixel_to_latlon(b.px), radius=6, color="#000", fill=True,
                            fill_color="#ff8800" if b.team == "red" else "#0088ff",
                            tooltip=f"炮兵连 {b.id}｜{b.tubes} 门｜射程 {b.range_km} km").add_to(support)
        folium.Circle(g.pixel_to_latlon(b.px), radius=b.range_km * 1000, color="#888", weight=1,
                      fill=False, dash_array="4 6").add_to(support)
    for w in world.wings.values():
        if view_team and w.team != view_team:
            continue
        folium.Marker(g.pixel_to_latlon(w.base_px), icon=folium.Icon(color="red" if w.team == "red" else "blue",
                                                                      icon="plane", prefix="fa"),
                      tooltip=f"{w.id}｜{w.role}｜{w.aircraft}/{w.aircraft_full} 架｜整备 {w.ready_in_h:.0f}h").add_to(support)
    for d in world.depots.values():
        if view_team and d.team != view_team:
            continue
        folium.Marker(g.pixel_to_latlon(d.px), icon=folium.Icon(color="green", icon="archive", prefix="fa"),
                      tooltip=f"补给点 {d.id}｜粮{d.stock.get('food', 0):.0f}t 弹{d.stock.get('ammo', 0):.0f}t "
                              f"油{d.stock.get('fuel', 0):.0f}t").add_to(support)
    for px, stash in world.grid.cell_stash.items():
        if not stash.get("equipment") and not stash.get("resources"):
            continue
        owner = stash.get("owner_team") or "无主"
        folium.Marker(g.pixel_to_latlon(px), icon=folium.Icon(color="gray", icon="wrench", prefix="fa"),
                      tooltip=f"像素仓库（{owner}）：装备 {stash.get('equipment')}｜物资 {stash.get('resources')}").add_to(support)
    support.add_to(m)

    from folium import plugins
    plugins.Fullscreen(position="topleft").add_to(m)
    folium.LayerControl(position="topright", collapsed=False).add_to(m)
    south_west, north_east = bounds
    m.fit_bounds([south_west, north_east])
    return m


def _marker(grid, group, px, html, tooltip):
    folium.Marker(grid.pixel_to_latlon(px), icon=folium.DivIcon(html=html), tooltip=tooltip).add_to(group)
