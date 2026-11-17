# -*- coding: utf-8 -*-
"""统一像素地形张量（V2 · 第一根大梁）。

整张战区是一个多通道二维栅格，默认 200 m/px。每个像素自带：
高程、微积分坡度梯度、地貌（道路/铁路/桥梁/机场也是地貌的一种）、天气、
能见度修正（烟雾）、照明、非战斗损耗率。

约定
----
- 行号 row 向南递增，列号 col 向东递增；像素坐标一律写作 (row, col)。
- grad_x = ∂z/∂col（每米），grad_y = ∂z/∂row（每米），均由 3×3 Horn 算子求得。
- 路径代价单位为「等效公里」：真实距离 × 地形/天气/坡度/火力封锁倍率。
  部队预算 = 基准速度 × 小时数，因此道路（倍率 < 1）真的更快。
- 本模块只依赖 numpy 与标准库，不 import streamlit / folium。
"""

from __future__ import annotations

import heapq
import math
from pathlib import Path

import numpy as np

# ==========================================
# 🏷️ 编码表
# ==========================================
PLAIN, FOREST, MOUNTAIN, SWAMP, WATER, URBAN = 0, 1, 2, 3, 4, 5
DIRT_ROAD, HIGHWAY, RAIL, BRIDGE, AIRFIELD = 10, 11, 12, 13, 14
ROAD_TYPES = (DIRT_ROAD, HIGHWAY, RAIL, BRIDGE)

CLEAR, RAIN, FOG, SNOW, MUD, ICE = 0, 1, 2, 3, 4, 5

MOBILITY_CLASSES = ("foot", "wheeled", "tracked")

TERRAIN_NAMES = {
    PLAIN: "平原", FOREST: "森林", MOUNTAIN: "山地", SWAMP: "沼泽", WATER: "水域",
    URBAN: "城市", DIRT_ROAD: "土路", HIGHWAY: "干线公路", RAIL: "铁路",
    BRIDGE: "桥梁", AIRFIELD: "野战机场",
}
WEATHER_NAMES = {CLEAR: "晴", RAIN: "雨", FOG: "大雾", SNOW: "雪", MUD: "泥泞", ICE: "冰封"}

# 各机动类别在各地貌上的代价倍率（inf = 不可通行；< 1 = 比基准更快）
TERRAIN_COST = {
    "foot": {PLAIN: 1.0, FOREST: 1.4, MOUNTAIN: 2.0, SWAMP: 2.5, URBAN: 1.3,
             DIRT_ROAD: 0.85, HIGHWAY: 0.8, RAIL: 0.9, BRIDGE: 0.85, AIRFIELD: 0.9},
    "wheeled": {PLAIN: 2.0, FOREST: 4.0, MOUNTAIN: 6.0, SWAMP: 8.0, URBAN: 2.0,
                DIRT_ROAD: 0.7, HIGHWAY: 0.35, RAIL: 4.0, BRIDGE: 0.45, AIRFIELD: 0.8},
    "tracked": {PLAIN: 1.2, FOREST: 2.2, MOUNTAIN: 3.5, SWAMP: 5.0, URBAN: 1.8,
                DIRT_ROAD: 0.8, HIGHWAY: 0.5, RAIL: 3.0, BRIDGE: 0.6, AIRFIELD: 0.8},
}
# 非道路像素坡度超过该角度（度）即禁行；道路像素覆盖此限制
SLOPE_LIMIT_DEG = {"foot": 40.0, "wheeled": 20.0, "tracked": 28.0}
# 上坡代价：factor = 1 + k × (上升/水平距离)
UPHILL_K = {"foot": 4.0, "wheeled": 9.0, "tracked": 6.0}
DOWNHILL_BRAKE = 0.5
DOWNHILL_FLOOR = 0.85

# 天气：(越野倍率, 道路倍率)；索引即天气编码
_WEATHER_OFFROAD = np.array([1.0, 1.3, 1.1, 1.6, 2.2, 1.8])
_WEATHER_ROAD = np.array([1.0, 1.1, 1.1, 1.3, 1.4, 1.5])
WEATHER_VIS = np.array([1.0, 0.7, 0.35, 0.5, 0.9, 0.9])
NIGHT_BASE_VIS = 0.4

# 地貌对隐蔽（侦察）与掩护（抗打击）的作用
CONCEALMENT = {PLAIN: 0.0, FOREST: 0.5, MOUNTAIN: 0.3, SWAMP: 0.15, WATER: 0.0, URBAN: 0.6,
               DIRT_ROAD: 0.0, HIGHWAY: 0.0, RAIL: 0.0, BRIDGE: 0.0, AIRFIELD: 0.0}
COVER = {PLAIN: 0.0, FOREST: 0.35, MOUNTAIN: 0.4, SWAMP: 0.1, WATER: 0.0, URBAN: 0.5,
         DIRT_ROAD: 0.0, HIGHWAY: 0.0, RAIL: 0.0, BRIDGE: 0.0, AIRFIELD: 0.0}

# 非战斗损耗（每天占兵力比例）：地貌 + 天气两项相加
_TERRAIN_ATTRITION_DAY = {SWAMP: 0.004, MOUNTAIN: 0.002, FOREST: 0.001}
_WEATHER_ATTRITION_DAY = {SNOW: 0.006, ICE: 0.008, MUD: 0.003, RAIN: 0.001}

# ==========================================
# 🎯 V3.1 AO 与分兵种战线势场掩码常量
# ==========================================
LINE_MASK_INFANTRY = 1 << 0   # 步兵战线
LINE_MASK_ARMOR = 1 << 1      # 装甲/坦克战线
LINE_MASK_HEAVY_AT = 1 << 2   # 重武器/反坦克战线
LINE_MASK_RECON = 1 << 3      # 侦察警戒战线
LINE_MASK_ARTY_LOG = 1 << 4   # 炮兵/后勤支援战线
LINE_MASK_ALL = (1 << 5) - 1

LINE_ROLE_DEPLOY = "DEPLOY_LINE"
LINE_ROLE_MOVE = "MOVE_LINE"
LINE_ROLE_ATTACK = "ATTACK_LINE"
LINE_ROLE_GLOBAL_ACTION = "GLOBAL_ACTION_LINE"

UNIT_TYPE_TO_LINE_MASK = {
    "infantry": LINE_MASK_INFANTRY,
    "armor": LINE_MASK_ARMOR,
    "heavy": LINE_MASK_HEAVY_AT,
    "heavy_at": LINE_MASK_HEAVY_AT,
    "at": LINE_MASK_HEAVY_AT,
    "recon": LINE_MASK_RECON,
    "artillery": LINE_MASK_ARTY_LOG,
    "logistics": LINE_MASK_ARTY_LOG,
    "arty_log": LINE_MASK_ARTY_LOG,
}


def unit_line_affinity(unit_type_or_mask, line_type_mask):
    """计算单位兵种与战线兵种标签之间的亲和度系数 w ∈ (0, 1]。"""
    if isinstance(unit_type_or_mask, str):
        u_mask = UNIT_TYPE_TO_LINE_MASK.get(unit_type_or_mask.lower(), LINE_MASK_INFANTRY)
    else:
        u_mask = int(unit_type_or_mask or LINE_MASK_INFANTRY)
    l_mask = int(line_type_mask or LINE_MASK_ALL)
    if u_mask & l_mask:
        return 1.0
    # 次级战术协同兼容（例如装甲可轻微辅助步兵突破，步兵可轻微辅助重武器/侦察）
    if (u_mask & LINE_MASK_ARMOR) and (l_mask & LINE_MASK_INFANTRY):
        return 0.40
    if (u_mask & LINE_MASK_INFANTRY) and (l_mask & (LINE_MASK_ARMOR | LINE_MASK_HEAVY_AT)):
        return 0.30
    if (u_mask & LINE_MASK_RECON) and (l_mask & LINE_MASK_INFANTRY):
        return 0.35
    return 0.05


def rasterize_polyline(points_px, shape=None):
    """Bresenham 光栅化多段折线为去重有序像素列表，返回 (cells, default_normal_rc)。"""
    pts = [(int(round(float(p[0]))), int(round(float(p[1])))) for p in (points_px or ())]
    if not pts:
        return [], (0.0, 1.0)
    if len(pts) == 1:
        r0, c0 = pts[0]
        if shape is None or (0 <= r0 < shape[0] and 0 <= c0 < shape[1]):
            return [(r0, c0)], (0.0, 1.0)
        return [], (0.0, 1.0)

    cells = []
    seen = set()
    for (r0, c0), (r1, c1) in zip(pts[:-1], pts[1:]):
        steps = max(abs(r1 - r0), abs(c1 - c0))
        if steps == 0:
            cand = [(r0, c0)]
        else:
            cand = [
                (int(round(r0 + (r1 - r0) * (i / steps))), int(round(c0 + (c1 - c0) * (i / steps))))
                for i in range(steps + 1)
            ]
        for rr, cc in cand:
            if shape is not None and not (0 <= rr < shape[0] and 0 <= cc < shape[1]):
                continue
            if (rr, cc) not in seen:
                seen.add((rr, cc))
                cells.append((rr, cc))

    dr = float(pts[-1][0] - pts[0][0])
    dc = float(pts[-1][1] - pts[0][1])
    norm = math.hypot(dr, dc)
    if norm <= 1e-9:
        default_normal = (0.0, 1.0)
    else:
        # 顺时针旋转 90 度作为默认正面法向 N_line
        default_normal = (dc / norm, -dr / norm)
    return cells, default_normal


TERRAIN_CODE_TO_FEATURE = {
    PLAIN: "plain",
    FOREST: "forest",
    MOUNTAIN: "mountain",
    SWAMP: "swamp",
    WATER: "water",
    URBAN: "urban",
    DIRT_ROAD: "dirt_road",
    HIGHWAY: "highway",
    RAIL: "rail",
    BRIDGE: "bridge",
    AIRFIELD: "airfield",
}

TERRAIN_FEATURE_SPECS = {
    "plain": {
        "name": "平原",
        "terrain_code": PLAIN,
        "modifiers": {
            "speed_mult": 1.0, "wheeled_speed_mult": 1.0, "tracked_speed_mult": 1.0,
            "cover_bonus": 0.0, "fortification_bonus": 0.0, "concealment_bonus": 0.0,
            "los_block_m": 0.0, "attrition_day": 0.0,
        },
    },
    "forest": {
        "name": "森林",
        "terrain_code": FOREST,
        "modifiers": {
            "speed_mult": 0.72, "wheeled_speed_mult": 0.5, "tracked_speed_mult": 0.55,
            "cover_bonus": 0.35, "fortification_bonus": 0.0, "concealment_bonus": 0.5,
            "los_block_m": 8.0, "attrition_day": 0.001, "cqb_ambush": True,
        },
    },
    "mountain": {
        "name": "山地",
        "terrain_code": MOUNTAIN,
        "modifiers": {
            "speed_mult": 0.5, "wheeled_speed_mult": 0.33, "tracked_speed_mult": 0.35,
            "cover_bonus": 0.4, "fortification_bonus": 0.1, "concealment_bonus": 0.3,
            "los_block_m": 0.0, "attrition_day": 0.002,
        },
    },
    "swamp": {
        "name": "沼泽",
        "terrain_code": SWAMP,
        "modifiers": {
            "speed_mult": 0.4, "wheeled_speed_mult": 0.25, "tracked_speed_mult": 0.25,
            "cover_bonus": 0.1, "fortification_bonus": 0.0, "concealment_bonus": 0.15,
            "los_block_m": 0.0, "attrition_day": 0.004,
        },
    },
    "water": {
        "name": "水域",
        "terrain_code": WATER,
        "modifiers": {
            "impassable": True, "speed_mult": 0.0, "cover_bonus": 0.0, "concealment_bonus": 0.0,
        },
    },
    "urban": {
        "name": "城市街区",
        "terrain_code": URBAN,
        "modifiers": {
            "speed_mult": 0.77, "wheeled_speed_mult": 1.0, "tracked_speed_mult": 0.67,
            "cover_bonus": 0.5, "fortification_bonus": 0.25, "concealment_bonus": 0.6,
            "los_block_m": 12.0, "cqb_ambush": True,
        },
    },
    "dirt_road": {
        "name": "土路",
        "terrain_code": DIRT_ROAD,
        "modifiers": {
            "speed_mult": 1.18, "wheeled_speed_mult": 2.85, "tracked_speed_mult": 1.5,
            "ignore_slope_limit": True, "is_road": True,
        },
    },
    "highway": {
        "name": "干线公路",
        "terrain_code": HIGHWAY,
        "modifiers": {
            "speed_mult": 1.25, "wheeled_speed_mult": 5.7, "tracked_speed_mult": 2.4,
            "ignore_slope_limit": True, "is_road": True,
        },
    },
    "rail": {
        "name": "铁路",
        "terrain_code": RAIL,
        "modifiers": {
            "speed_mult": 1.1, "wheeled_speed_mult": 0.5, "tracked_speed_mult": 0.4,
            "ignore_slope_limit": True, "is_road": True,
        },
    },
    "bridge": {
        "name": "桥梁",
        "terrain_code": BRIDGE,
        "modifiers": {
            "speed_mult": 1.18, "wheeled_speed_mult": 4.4, "tracked_speed_mult": 2.0,
            "ignore_slope_limit": True, "is_road": True, "bridge_over_water": True,
        },
    },
    "airfield": {
        "name": "野战机场",
        "terrain_code": AIRFIELD,
        "modifiers": {
            "speed_mult": 1.1, "wheeled_speed_mult": 2.5, "tracked_speed_mult": 1.5,
            "is_airfield": True,
        },
    },
    "trench": {
        "name": "战壕工事",
        "terrain_code": None,
        "modifiers": {
            "speed_mult": 0.85, "cover_bonus": 0.30, "fortification_bonus": 0.45,
            "concealment_bonus": 0.25,
        },
    },
    "bunker": {
        "name": "永备暗堡",
        "terrain_code": None,
        "modifiers": {
            "speed_mult": 0.8, "cover_bonus": 0.45, "fortification_bonus": 0.70,
            "concealment_bonus": 0.35,
        },
    },
    "minefield": {
        "name": "雷区",
        "terrain_code": None,
        "modifiers": {
            "speed_mult": 0.65, "mine_hazard": 1.0,
        },
    },
}


def register_terrain_feature(feature_id, spec):
    """动态注册/覆写自定义像素地形特征与其携带的 Buff 修饰器包。"""
    fid = str(feature_id)
    if "modifiers" in spec or "terrain_code" in spec or "name" in spec:
        entry = {
            "name": str(spec.get("name", fid)),
            "terrain_code": spec.get("terrain_code"),
            "modifiers": dict(spec.get("modifiers") or {}),
        }
    else:
        entry = {"name": fid, "terrain_code": None, "modifiers": dict(spec)}
    TERRAIN_FEATURE_SPECS[fid] = entry
    return entry


def read_raw_channel(source, width=None, height=None, bit_depth=None, flip_y=False):
    """1:1 无降采样读取 .r8 / .r16 / .r32 / bytes / ndarray 为二维 float32 矩阵。"""
    if isinstance(source, np.ndarray):
        arr = np.asarray(source, dtype=np.float32).copy()
        if arr.ndim != 2:
            raise ValueError("RAW 通道数组必须是二维矩阵")
        return np.flipud(arr).copy() if flip_y else arr
    if isinstance(source, (list, tuple)):
        arr = np.asarray(source, dtype=np.float32)
        if arr.ndim != 2:
            raise ValueError("RAW 通道数组必须是二维矩阵")
        return np.flipud(arr).copy() if flip_y else arr

    if isinstance(source, (bytes, bytearray, memoryview)):
        raw_bytes = bytes(source)
        ext = ""
    else:
        p = Path(source)
        ext = p.suffix.lower()
        raw_bytes = p.read_bytes()

    if bit_depth is None:
        if ext == ".r8":
            bit_depth = 8
        elif ext == ".r32":
            bit_depth = 32
        else:
            bit_depth = 16
    bd = int(bit_depth)
    if bd == 8:
        dt = np.dtype("<u1")
    elif bd == 16:
        dt = np.dtype("<u2")
    elif bd == 32:
        dt = np.dtype("<f4")
    else:
        raise ValueError(f"不支持的 RAW 位深: {bit_depth}")

    bpp = bd // 8
    if len(raw_bytes) % bpp != 0:
        raise ValueError(f"RAW 字节长度 {len(raw_bytes)} 无法整除每像素字节数 {bpp}")
    total_px = len(raw_bytes) // bpp
    if width is None and height is None:
        side = math.isqrt(total_px)
        if side * side != total_px:
            raise ValueError(f"非方形 RAW 图像（像素总数 {total_px}），请显式传入 width/height")
        width, height = side, side
    elif width is not None and height is None:
        width = int(width)
        height = total_px // width
    elif height is not None and width is None:
        height = int(height)
        width = total_px // height
    else:
        width, height = int(width), int(height)
    if width * height != total_px:
        raise ValueError(f"尺寸 {height}x{width} 与 RAW 像素总数 {total_px} 不一致")

    arr = np.frombuffer(raw_bytes, dtype=dt).reshape((height, width)).astype(np.float32).copy()
    return np.flipud(arr).copy() if flip_y else arr


def map_elevation_channel(
    raw_arr,
    low_meter=0.0,
    high_meter=1000.0,
    raw_low=None,
    raw_high=None,
    ignore_u16_spike=True,
    invert=False,
):
    """将 1:1 原始高程张量线性映射到物理海拔（米），保留 65535 噪点免疫与平坦图保护。"""
    arr = np.asarray(raw_arr, dtype=np.float64)
    sample = arr
    if ignore_u16_spike and float(np.max(arr)) >= 65535.0:
        valid = arr[arr < 65535.0]
        if valid.size > 0:
            sample = valid
    actual_min = float(np.min(sample)) if raw_low is None else float(raw_low)
    actual_max = float(np.max(sample)) if raw_high is None else float(raw_high)
    if actual_max <= actual_min:
        actual_max = actual_min + 1.0
    norm = np.clip((arr - actual_min) / (actual_max - actual_min), 0.0, 1.0)
    if invert:
        norm = 1.0 - norm
    return (float(low_meter) + norm * (float(high_meter) - float(low_meter))).astype(np.float32)

_KM_PER_DEG_LAT = 110.57
_KM_PER_DEG_LNG = 111.32

_NEIGHBORS = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)]


# ==========================================
# 🧮 微积分坡度
# ==========================================
def _box_smooth(arr, radius_px):
    """大尺度可分离均值滤波，用于开局前跨像素尺度烘焙宏观坡度。"""
    r = int(radius_px)
    if r <= 0:
        return arr
    pad = np.pad(arr, ((r, r), (r, r)), mode="edge")
    # 先沿行再沿列做滑动和
    k = 2 * r + 1
    cs = np.cumsum(pad, axis=0, dtype=np.float64)
    cs0 = np.vstack([np.zeros((1, pad.shape[1]), dtype=np.float64), cs])
    row_sum = (cs0[k:, :] - cs0[:-k, :]) / k
    cs1 = np.hstack([np.zeros((row_sum.shape[0], 1), dtype=np.float64), np.cumsum(row_sum, axis=1, dtype=np.float64)])
    col_sum = (cs1[:, k:] - cs1[:, :-k]) / k
    return col_sum


def compute_gradient(elevation, cell_m, window_px=1):
    """3×3 Horn 算子（支持 window_px>1 大尺度预平滑烘焙）。返回 (grad_x, grad_y, slope_deg)。"""
    elev = np.asarray(elevation, dtype=np.float64)
    if int(window_px) > 1:
        elev = _box_smooth(elev, int(window_px))
    z = np.pad(elev, 1, mode="edge")
    # 邻域命名：a b c / d _ f / g h i
    a, b, c = z[:-2, :-2], z[:-2, 1:-1], z[:-2, 2:]
    d, f = z[1:-1, :-2], z[1:-1, 2:]
    g, h, i = z[2:, :-2], z[2:, 1:-1], z[2:, 2:]
    gx = ((c + 2 * f + i) - (a + 2 * d + g)) / (8.0 * cell_m)
    gy = ((g + 2 * h + i) - (a + 2 * b + c)) / (8.0 * cell_m)
    slope = np.degrees(np.arctan(np.hypot(gx, gy)))
    return gx.astype(np.float32), gy.astype(np.float32), slope.astype(np.float32)


# ==========================================
# 🗺️ 栅格
# ==========================================
class Grid:
    """多通道地形栅格。所有通道同形 (H, W)。"""

    def __init__(self, elevation, cell_m=200.0, origin_latlon=(0.0, 0.0),
                 terrain=None, weather=None, slope_window_px=1):
        elev = np.asarray(elevation, dtype=np.float32)
        if elev.ndim != 2:
            raise ValueError("elevation 必须是二维矩阵")
        self.elevation = elev
        self.shape = elev.shape
        self.cell_m = float(cell_m)
        self.origin_latlon = (float(origin_latlon[0]), float(origin_latlon[1]))
        self.slope_window_px = max(1, int(slope_window_px))
        self.terrain = (np.zeros(self.shape, np.uint8) if terrain is None
                        else np.asarray(terrain, dtype=np.uint8).copy())
        self.weather = (np.zeros(self.shape, np.uint8) if weather is None
                        else np.asarray(weather, dtype=np.uint8).copy())
        self.visibility_mod = np.ones(self.shape, np.float32)
        self.smoke_ttl = np.zeros(self.shape, np.float32)
        self.illumination = np.zeros(self.shape, np.float32)
        self.illum_ttl = np.zeros(self.shape, np.float32)
        self.exposure = np.zeros(self.shape, np.float32)
        self.terrain_attrition = np.zeros(self.shape, np.float32)
        self.destroyed_bridges = set()
        self.damaged_terrain = {}
        self.cell_stash = {}      # (r, c) -> {"resources": {...}, "equipment": {...}, "owner_team": ..., "radio_channel": ...}
        self.cell_modifiers = {}  # (r, c) -> {key: {"modifiers": {...}, "ttl_hours": float|None}}
        self.feature_channels = {}  # feature_id -> {"mask": bool(H,W), "modifiers": dict|None, "terrain_code": int|None}
        self.wire_edges = {}      # team -> set[tuple[tuple[int,int], tuple[int,int]]]
        # V3.1 地形容器 AO 通道与分兵种战线势场通道
        self.ao_channel = {}      # team -> {"zones": {zid: {"cells": set, "centroid": [r, c], "commander_id": cid}}, "cell_zones": {(r,c): set(zid)}}
        self.typed_frontlines = {}  # line_id -> {...}
        self.frontline_channel = {}  # team -> {(r, c): list[dict]}
        self.recompute_slope()
        self.refresh_attrition()

    # ---- 派生通道 ----
    def recompute_slope(self):
        self.grad_x, self.grad_y, self.slope_deg = compute_gradient(
            self.elevation, self.cell_m, window_px=self.slope_window_px
        )

    def refresh_attrition(self):
        """按当前地貌 + 天气 + 像素特征/修饰器重算每日非战斗损耗率（避免主地貌重复叠加）。"""
        att = np.zeros(self.shape, np.float32)
        for code, v in _TERRAIN_ATTRITION_DAY.items():
            att[self.terrain == code] += v
        for code, v in _WEATHER_ATTRITION_DAY.items():
            att[self.weather == code] += v
        for fid, ch in self.feature_channels.items():
            spec = TERRAIN_FEATURE_SPECS.get(fid, {})
            mods = ch.get("modifiers") if ch.get("modifiers") is not None else spec.get("modifiers", {})
            a = float(mods.get("attrition_day", 0.0) or 0.0)
            if a == 0.0:
                continue
            tc = ch.get("terrain_code") if ch.get("terrain_code") is not None else spec.get("terrain_code")
            mask = np.asarray(ch["mask"], dtype=bool)
            if tc is not None:
                mask = mask & (self.terrain != int(tc))
            att[mask] += a
        for rc, bucket in self.cell_modifiers.items():
            r, c = rc
            if not (0 <= r < self.shape[0] and 0 <= c < self.shape[1]):
                continue
            primary_fid = TERRAIN_CODE_TO_FEATURE.get(int(self.terrain[r, c]))
            for key, entry in bucket.items():
                if key.startswith("feat:") and key[5:] == primary_fid:
                    continue
                mods = entry.get("modifiers", {})
                if "attrition_day" in mods:
                    att[r, c] += float(mods["attrition_day"])
        self.terrain_attrition = att

    # ---- 坐标 ----
    def in_bounds(self, px):
        return 0 <= px[0] < self.shape[0] and 0 <= px[1] < self.shape[1]

    def clamp(self, px):
        return (min(max(int(round(px[0])), 0), self.shape[0] - 1),
                min(max(int(round(px[1])), 0), self.shape[1] - 1))

    def pixel_to_latlon(self, px):
        lat0, lon0 = self.origin_latlon
        km = self.cell_m / 1000.0
        lat = lat0 - px[0] * km / _KM_PER_DEG_LAT
        lon = lon0 + px[1] * km / (_KM_PER_DEG_LNG * max(0.1, math.cos(math.radians(lat0))))
        return lat, lon

    def latlon_to_pixel(self, lat, lon):
        lat0, lon0 = self.origin_latlon
        km = self.cell_m / 1000.0
        row = (lat0 - lat) * _KM_PER_DEG_LAT / km
        col = (lon - lon0) * _KM_PER_DEG_LNG * max(0.1, math.cos(math.radians(lat0))) / km
        return row, col

    def distance_km(self, a, b):
        return math.hypot(a[0] - b[0], a[1] - b[1]) * self.cell_m / 1000.0

    def km_to_px(self, km):
        return km * 1000.0 / self.cell_m

    # ---- 编辑 ----
    def set_terrain(self, code, mask=None, px=None):
        if px is not None:
            self.terrain[px[0], px[1]] = code
        else:
            self.terrain[mask] = code
        self.refresh_attrition()

    def set_weather(self, code, mask=None):
        if mask is None:
            self.weather[:] = code
        else:
            self.weather[mask] = code
        self.refresh_attrition()

    def destroy_bridge(self, px):
        """炸毁桥梁：退化为水域障碍，记录原位置以便工兵抢修。"""
        px = (int(px[0]), int(px[1]))
        if self.terrain[px] != BRIDGE:
            return False
        self.terrain[px] = WATER
        self.destroyed_bridges.add(px)
        if "bridge" in self.feature_channels:
            self.feature_channels["bridge"]["mask"][px] = False
        self.remove_cell_modifier(px, "feat:bridge")
        return True

    def repair_bridge(self, px):
        px = (int(px[0]), int(px[1]))
        if px not in self.destroyed_bridges:
            return False
        self.terrain[px] = BRIDGE
        self.destroyed_bridges.discard(px)
        return True

    def destroy_terrain(self, px, new_terrain=None):
        """战斗破坏触发局部改写静态地形（桥梁变水域、公路变土路、城市/机场变平原等）。"""
        px = (int(px[0]), int(px[1]))
        cur = int(self.terrain[px])
        if cur == BRIDGE:
            return self.destroy_bridge(px)
        if new_terrain is None:
            fallback = {HIGHWAY: DIRT_ROAD, RAIL: PLAIN, AIRFIELD: PLAIN, URBAN: PLAIN, FOREST: PLAIN}
            new_terrain = fallback.get(cur, cur)
        if int(new_terrain) == cur:
            return False
        self.damaged_terrain.setdefault(px, cur)
        self.terrain[px] = int(new_terrain)
        self.refresh_attrition()
        return True

    def repair_terrain(self, px):
        """工程抢修恢复被破坏的静态地形（桥梁/野战机场/公路/城市等）。"""
        px = (int(px[0]), int(px[1]))
        if px in self.destroyed_bridges:
            return self.repair_bridge(px)
        if px in self.damaged_terrain:
            self.terrain[px] = int(self.damaged_terrain.pop(px))
            self.refresh_attrition()
            return True
        return False

    def reset_exposure(self):
        self.exposure.fill(0.0)

    def add_exposure(self, px, amount, radius_px=0):
        """在指定像素（或圆域）线性加法叠加暴露度。"""
        px = self.clamp(px)
        val = float(amount)
        if val <= 0:
            return
        if radius_px <= 0:
            self.exposure[px] += val
        else:
            mask = disk_mask(self.shape, px, radius_px)
            self.exposure[mask] += val

    # ---- 稀疏像素物资/装备容器（唯一真源 SSOT） ----
    def stash_at(self, px):
        px = (int(px[0]), int(px[1]))
        entry = self.cell_stash.get(px)
        if entry is None:
            return {"resources": {}, "equipment": {}, "owner_team": None, "radio_channel": "A"}
        return {
            "resources": dict(entry["resources"]),
            "equipment": dict(entry["equipment"]),
            "owner_team": entry.get("owner_team"),
            "radio_channel": entry.get("radio_channel", "A"),
        }

    def stash_put(self, px, resources=None, equipment=None, owner_team=None, radio_channel=None):
        px = (int(px[0]), int(px[1]))
        entry = self.cell_stash.setdefault(
            px, {"resources": {}, "equipment": {}, "owner_team": owner_team, "radio_channel": radio_channel or "A"}
        )
        if radio_channel is not None:
            entry["radio_channel"] = radio_channel
        if owner_team is not None and entry.get("owner_team") is None:
            entry["owner_team"] = owner_team
        elif owner_team is not None and entry.get("owner_team") != owner_team:
            # 多阵营混杂在同一像素时降级为无阵营混合堆放
            entry["owner_team"] = None
        for k, v in (resources or {}).items():
            if v > 0:
                entry["resources"][k] = entry["resources"].get(k, 0.0) + float(v)
        for k, v in (equipment or {}).items():
            if v > 0:
                entry["equipment"][k] = entry["equipment"].get(k, 0) + int(v)
        if not entry["resources"] and not entry["equipment"]:
            self.cell_stash.pop(px, None)

    def stash_take(self, px, resources=None, equipment=None):
        """从像素仓库守恒提取，返回实际取到的 {resources, equipment}。"""
        px = (int(px[0]), int(px[1]))
        taken = {"resources": {}, "equipment": {}}
        entry = self.cell_stash.get(px)
        if entry is None:
            return taken
        for k, req in (resources or {}).items():
            avail = entry["resources"].get(k, 0.0)
            got = min(avail, max(0.0, float(req)))
            if got > 0:
                taken["resources"][k] = got
                rem = avail - got
                if rem > 1e-9:
                    entry["resources"][k] = rem
                else:
                    entry["resources"].pop(k, None)
        for k, req in (equipment or {}).items():
            avail = entry["equipment"].get(k, 0)
            got = min(avail, max(0, int(req)))
            if got > 0:
                taken["equipment"][k] = got
                rem = avail - got
                if rem > 0:
                    entry["equipment"][k] = rem
                else:
                    entry["equipment"].pop(k, None)
        if not entry["resources"] and not entry["equipment"]:
            self.cell_stash.pop(px, None)
        return taken

    # ---- 野战有线电缆物理拓扑 ----
    def lay_wire(self, path_pixels, team):
        """把连续像素路径铺设为己方野战有线电缆边，返回新增边数。"""
        pts = [(int(r), int(c)) for r, c in path_pixels]
        if len(pts) < 2:
            return 0
        edges = self.wire_edges.setdefault(team, set())
        added = 0
        for a, b in zip(pts, pts[1:]):
            if a == b:
                continue
            edge = (a, b) if a < b else (b, a)
            if edge not in edges:
                edges.add(edge)
                added += 1
        return added

    def cut_wire_at(self, px, radius_px=0.0, team=None):
        """在指定像素或半径内切断野战电缆，返回被切断的边数。"""
        cr, cc = int(px[0]), int(px[1])
        r2 = float(radius_px) ** 2 + 1e-9
        teams = [team] if team is not None else list(self.wire_edges)
        removed = 0
        for t in teams:
            edges = self.wire_edges.get(t)
            if not edges:
                continue
            to_drop = set()
            for a, b in edges:
                if ((a[0] - cr) ** 2 + (a[1] - cc) ** 2 <= r2) or ((b[0] - cr) ** 2 + (b[1] - cc) ** 2 <= r2):
                    to_drop.add((a, b))
            if to_drop:
                edges -= to_drop
                removed += len(to_drop)
        return removed

    def has_wire_at(self, px, team):
        px = (int(px[0]), int(px[1]))
        edges = self.wire_edges.get(team)
        if not edges:
            return False
        return any(a == px or b == px for a, b in edges)

    def wire_connected(self, px_a, px_b, team):
        """BFS 检查两像素之间是否存在完好连通的己方有线电缆。"""
        pa = (int(px_a[0]), int(px_a[1]))
        pb = (int(px_b[0]), int(px_b[1]))
        if pa == pb:
            return True
        edges = self.wire_edges.get(team)
        if not edges:
            return False
        adj = {}
        for a, b in edges:
            adj.setdefault(a, []).append(b)
            adj.setdefault(b, []).append(a)
        if pa not in adj or pb not in adj:
            return False
        seen = {pa}
        q = [pa]
        while q:
            cur = q.pop()
            if cur == pb:
                return True
            for nxt in adj.get(cur, ()):
                if nxt not in seen:
                    seen.add(nxt)
                    q.append(nxt)
        return False

    # ---- 像素容器通用 Buff/Debuff 修饰器与地形特征 ----
    def _resolve_feature_spec(self, feature_id, modifiers=None, terrain_code=None):
        fid = str(feature_id)
        spec = TERRAIN_FEATURE_SPECS.get(fid, {})
        mods = dict(spec.get("modifiers") or {})
        if modifiers:
            mods.update(modifiers)
        tc = terrain_code if terrain_code is not None else spec.get("terrain_code")
        return mods, tc

    def add_terrain_feature(self, px, feature_id, modifiers=None, ttl_hours=None, sync_terrain=True, terrain_code=None):
        """在指定像素容器写入地形特征与携带的 Buff 修饰器。"""
        rc = (int(round(px[0])), int(round(px[1])))
        fid = str(feature_id)
        mods, tc = self._resolve_feature_spec(fid, modifiers=modifiers, terrain_code=terrain_code)
        self.set_cell_modifier(rc, f"feat:{fid}", mods, ttl_hours=ttl_hours)
        if sync_terrain and tc is not None and self.in_bounds(rc):
            self.terrain[rc[0], rc[1]] = int(tc)
        if "attrition_day" in mods or sync_terrain:
            self.refresh_attrition()
        return rc

    def remove_terrain_feature(self, px, feature_id, fallback_terrain=None):
        """移除指定像素容器上的地形特征与通道标记。"""
        rc = (int(round(px[0])), int(round(px[1])))
        fid = str(feature_id)
        changed = self.remove_cell_modifier(rc, f"feat:{fid}")
        ch = self.feature_channels.get(fid)
        if ch is not None and self.in_bounds(rc) and bool(ch["mask"][rc[0], rc[1]]):
            ch["mask"][rc[0], rc[1]] = False
            changed = True
        if changed and fallback_terrain is not None and self.in_bounds(rc):
            self.terrain[rc[0], rc[1]] = int(fallback_terrain)
        if changed:
            self.refresh_attrition()
        return changed

    def get_terrain_features(self, px):
        """查询像素容器当前激活的全部地形特征 ID（含基础地貌特征与叠加特征）。"""
        rc = (int(round(px[0])), int(round(px[1])))
        mods = self.get_cell_modifiers(rc)
        feats = list(mods.get("features", []))
        if self.in_bounds(rc):
            primary_fid = TERRAIN_CODE_TO_FEATURE.get(int(self.terrain[rc[0], rc[1]]))
            if primary_fid and primary_fid not in feats:
                feats.insert(0, primary_fid)
        return feats

    def apply_feature_channel(self, feature_id, channel_data, threshold=0.5, equals=None,
                              modifiers=None, sync_terrain=True, terrain_code=None,
                              bit_depth=None, flip_y=False):
        """按 1:1 通道矩阵/RAW 文件把指定地形特征写入像素容器（严禁降采样）。"""
        H, W = self.shape
        fid = str(feature_id)
        if isinstance(channel_data, np.ndarray):
            arr = np.asarray(channel_data, dtype=np.float32)
            if flip_y:
                arr = np.flipud(arr)
        else:
            arr = read_raw_channel(channel_data, width=W, height=H, bit_depth=bit_depth, flip_y=flip_y)
        if arr.shape != self.shape:
            raise ValueError(f"特征通道 {fid} 尺寸 {arr.shape} 与地图 {self.shape} 不匹配（禁止自动降采样/重采样）")
        if equals is not None:
            mask = np.rint(arr).astype(np.int64) == int(equals)
        else:
            mask = arr >= float(threshold)
        spec = TERRAIN_FEATURE_SPECS.get(fid, {})
        tc = terrain_code if terrain_code is not None else spec.get("terrain_code")
        stored_mods = None
        if modifiers is not None:
            stored_mods = dict(spec.get("modifiers") or {})
            stored_mods.update(modifiers)
        elif fid not in TERRAIN_FEATURE_SPECS:
            stored_mods = {}
        self.feature_channels[fid] = {
            "mask": mask.astype(bool),
            "modifiers": stored_mods,
            "terrain_code": tc,
        }
        if sync_terrain and tc is not None:
            self.terrain[mask] = int(tc)
        self.refresh_attrition()
        return int(mask.sum())

    @classmethod
    def from_raw_channels(cls, elevation_source, width=None, height=None, bit_depth=None,
                          cell_m=200.0, origin_latlon=(0.0, 0.0),
                          elev_low_m=0.0, elev_high_m=1000.0,
                          raw_low=None, raw_high=None, ignore_u16_spike=True,
                          invert_elev=False, flip_y=False,
                          terrain_channel=None, terrain_code_map=None,
                          feature_channels=None, weather=None, slope_window_px=1):
        """从 1:1 RAW (.r8/.r16/.r32/array) 高程图与数据通道直接构造多通道 Grid。"""
        raw_elev = read_raw_channel(
            elevation_source, width=width, height=height, bit_depth=bit_depth, flip_y=flip_y
        )
        if elev_low_m is None or elev_high_m is None:
            elev_m = np.asarray(raw_elev, dtype=np.float32)
        else:
            elev_m = map_elevation_channel(
                raw_elev, low_meter=elev_low_m, high_meter=elev_high_m,
                raw_low=raw_low, raw_high=raw_high,
                ignore_u16_spike=ignore_u16_spike, invert=invert_elev,
            )
        H, W = elev_m.shape
        g = cls(elev_m, cell_m=cell_m, origin_latlon=origin_latlon,
                weather=weather, slope_window_px=slope_window_px)

        if terrain_channel is not None:
            if isinstance(terrain_channel, dict):
                t_arr = read_raw_channel(
                    terrain_channel["source"], width=W, height=H,
                    bit_depth=terrain_channel.get("bit_depth"),
                    flip_y=terrain_channel.get("flip_y", flip_y),
                )
                cmap = terrain_channel.get("code_map", terrain_code_map)
            else:
                t_arr = read_raw_channel(terrain_channel, width=W, height=H, flip_y=flip_y)
                cmap = terrain_code_map
            if t_arr.shape != g.shape:
                raise ValueError(f"地形通道尺寸 {t_arr.shape} 与高程图 {g.shape} 不匹配")
            int_arr = np.rint(t_arr).astype(np.int64)
            if cmap:
                for raw_v, target in cmap.items():
                    mask = int_arr == int(raw_v)
                    if not np.any(mask):
                        continue
                    if isinstance(target, str):
                        g.apply_feature_channel(target, mask.astype(np.float32), threshold=0.5, sync_terrain=True)
                    else:
                        g.terrain[mask] = int(target)
                g.refresh_attrition()
            else:
                g.terrain = np.clip(int_arr, 0, 255).astype(np.uint8)
                g.refresh_attrition()

        if feature_channels:
            priority = {
                "plain": 0, "water": 1, "swamp": 2, "forest": 3, "mountain": 4,
                "urban": 5, "airfield": 6, "dirt_road": 7, "rail": 8, "highway": 9, "bridge": 10,
            }
            ordered = sorted(feature_channels.items(), key=lambda kv: priority.get(str(kv[0]), 20))
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
                        flip_y=item.get("flip_y", flip_y),
                    )
                else:
                    g.apply_feature_channel(fid, item, flip_y=flip_y)
        return g

    def set_cell_modifier(self, px, key, modifiers, ttl_hours=None):
        """给指定像素容器挂接或更新命名修饰器（如炮兵掩体、雷区阻力、临时路基）。"""
        rc = (int(round(px[0])), int(round(px[1])))
        bucket = self.cell_modifiers.setdefault(rc, {})
        bucket[str(key)] = {
            "modifiers": {
                str(k): (float(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else v)
                for k, v in (modifiers or {}).items()
            },
            "ttl_hours": (None if ttl_hours is None else max(0.0, float(ttl_hours))),
        }

    def remove_cell_modifier(self, px, key):
        rc = (int(round(px[0])), int(round(px[1])))
        bucket = self.cell_modifiers.get(rc)
        if not bucket or str(key) not in bucket:
            return False
        del bucket[str(key)]
        if not bucket:
            del self.cell_modifiers[rc]
        return True

    def get_cell_modifiers(self, px, mobility=None):
        """汇总像素容器当前生效的合并修饰器（含地形特征通道与动态 cell_modifiers）。"""
        rc = (int(round(px[0])), int(round(px[1])))
        out = {
            "speed_mult": 1.0,
            "non_feature_speed_mult": 1.0,
            "feature_speed_mult": 1.0,
            "combat_power_mult": 1.0,
            "cover_bonus": 0.0,
            "fortification_bonus": 0.0,
            "concealment_bonus": 0.0,
            "exposure_mult": 1.0,
            "los_block_m": 0.0,
            "attrition_day": 0.0,
            "mine_hazard": 0.0,
            "cqb_ambush": False,
            "ignore_slope_limit": False,
            "is_road": False,
            "is_airfield": False,
            "bridge_over_water": False,
            "impassable": False,
            "features": [],
            "keys": [],
        }
        r, c = rc
        in_b = 0 <= r < self.shape[0] and 0 <= c < self.shape[1]
        mob_key = ("wheeled_speed_mult" if mobility == "wheeled"
                   else ("tracked_speed_mult" if mobility == "tracked" else "speed_mult"))
        seen_fids = set()

        def _merge(mods, is_feature=False):
            sm = mods.get(mob_key, mods.get("speed_mult"))
            if sm is not None:
                sm_f = float(sm)
                out["speed_mult"] *= sm_f
                if is_feature:
                    out["feature_speed_mult"] *= sm_f
                else:
                    out["non_feature_speed_mult"] *= sm_f
            if "combat_power_mult" in mods:
                out["combat_power_mult"] *= float(mods["combat_power_mult"])
            if "cover_bonus" in mods:
                out["cover_bonus"] += float(mods["cover_bonus"])
            if "fortification_bonus" in mods:
                out["fortification_bonus"] += float(mods["fortification_bonus"])
            if "concealment_bonus" in mods:
                out["concealment_bonus"] += float(mods["concealment_bonus"])
            if "exposure_mult" in mods:
                out["exposure_mult"] *= float(mods["exposure_mult"])
            if "los_block_m" in mods:
                out["los_block_m"] = max(out["los_block_m"], float(mods["los_block_m"]))
            if "attrition_day" in mods:
                out["attrition_day"] += float(mods["attrition_day"])
            if "mine_hazard" in mods:
                mh = mods["mine_hazard"]
                out["mine_hazard"] = max(out["mine_hazard"], 1.0 if isinstance(mh, bool) and mh else float(mh))
            for bkey in ("cqb_ambush", "ignore_slope_limit", "is_road", "is_airfield", "bridge_over_water", "impassable"):
                if mods.get(bkey):
                    out[bkey] = True

        bucket = self.cell_modifiers.get(rc)
        if bucket:
            for key, entry in bucket.items():
                out["keys"].append(key)
                mods = entry.get("modifiers", {})
                if key.startswith("feat:"):
                    fid = key[5:]
                    if fid not in seen_fids:
                        seen_fids.add(fid)
                        out["features"].append(fid)
                    _merge(mods, is_feature=True)
                else:
                    _merge(mods, is_feature=False)

        if in_b and self.feature_channels:
            for fid, ch in self.feature_channels.items():
                if not bool(ch["mask"][r, c]):
                    continue
                if fid in seen_fids:
                    continue
                seen_fids.add(fid)
                out["features"].append(fid)
                spec = TERRAIN_FEATURE_SPECS.get(fid, {})
                mods = ch.get("modifiers") if ch.get("modifiers") is not None else spec.get("modifiers", {})
                _merge(mods, is_feature=True)

        return out

    def tick_cell_modifiers(self, dt_hours):
        dt = max(0.0, float(dt_hours))
        if dt <= 0.0:
            return
        dirty_attrition = False
        for rc in list(self.cell_modifiers.keys()):
            bucket = self.cell_modifiers[rc]
            for key in list(bucket.keys()):
                ttl = bucket[key].get("ttl_hours")
                if ttl is not None:
                    ttl -= dt
                    if ttl <= 1e-9:
                        if "attrition_day" in bucket[key].get("modifiers", {}):
                            dirty_attrition = True
                        del bucket[key]
                    else:
                        bucket[key]["ttl_hours"] = ttl
            if not bucket:
                del self.cell_modifiers[rc]
        if dirty_attrition:
            self.refresh_attrition()

    # ---- V3.1 地形容器：AO 边界约束通道与分兵种战线势场通道 ----
    def effective_cover_at(self, px):
        """快速获取指定像素的综合掩蔽度 cover(x) ∈ [0, 0.95]（供 9x9 扫描与动态寻路代价使用）。"""
        r, c = int(round(float(px[0]))), int(round(float(px[1])))
        if not (0 <= r < self.shape[0] and 0 <= c < self.shape[1]):
            return 0.0
        t_code = int(self.terrain[r, c])
        primary_fid = TERRAIN_CODE_TO_FEATURE.get(t_code)
        mods = self.get_cell_modifiers((r, c))
        raw_cov = 0.0 if (primary_fid and primary_fid in mods.get("features", ())) else COVER.get(t_code, 0.0)
        cov = raw_cov + float(mods.get("cover_bonus", 0.0)) + 0.5 * float(mods.get("fortification_bonus", 0.0))
        return min(0.95, max(0.0, cov))

    def write_ao_to_container(self, zone_id, team, cells, commander_id=None):
        """将 AO 归属边界直接光栅化写入地形容器，并缓存质心供出界拉回引力计算。"""
        t_chan = self.ao_channel.setdefault(team, {"zones": {}, "cell_zones": {}})
        # 若此前存在同 zone_id 先清除旧像素索引
        self.remove_ao_from_container(zone_id, team=team)
        norm_cells = {(int(r), int(c)) for r, c in (cells or ()) if 0 <= int(r) < self.shape[0] and 0 <= int(c) < self.shape[1]}
        if not norm_cells:
            return
        sr = sum(rc[0] for rc in norm_cells) / float(len(norm_cells))
        sc = sum(rc[1] for rc in norm_cells) / float(len(norm_cells))
        t_chan["zones"][zone_id] = {
            "cells": norm_cells,
            "centroid": (sr, sc),
            "commander_id": commander_id,
        }
        cell_zones = t_chan["cell_zones"]
        for rc in norm_cells:
            cell_zones.setdefault(rc, set()).add(zone_id)

    def remove_ao_from_container(self, zone_id, team=None):
        teams = [team] if team is not None else list(self.ao_channel.keys())
        for t in teams:
            t_chan = self.ao_channel.get(t)
            if not t_chan:
                continue
            z_entry = t_chan["zones"].pop(zone_id, None)
            if z_entry:
                cell_zones = t_chan["cell_zones"]
                for rc in z_entry["cells"]:
                    s = cell_zones.get(rc)
                    if s:
                        s.discard(zone_id)
                        if not s:
                            cell_zones.pop(rc, None)

    def _rebuild_frontline_channel(self, team):
        field = {}
        H, W = self.shape
        for lid, fl in self.typed_frontlines.items():
            if fl["team"] != team:
                continue
            rad = int(fl.get("spread_radius_px", 4))
            att = float(fl.get("attraction_weight", 1.0))
            nr, nc = fl["front_normal"]
            mask = int(fl["unit_type_mask"])
            zid = fl.get("zone_id")
            lrole = fl.get("line_role", LINE_ROLE_DEPLOY)
            src_dl = fl.get("source_deploy_line_id")
            tgt_dl = fl.get("target_deploy_line_id")
            end_px = fl.get("end_px")
            axis_vec = fl.get("axis_vec") or [0.0, 0.0]
            rc_cells = fl["raster_cells"]
            # 使用每个线段像元扩展邻域并保留每个像素对该 line_id 的最大势场值
            best_for_line = {}
            for lr, lc in rc_cells:
                r0, r1 = max(0, lr - rad), min(H - 1, lr + rad)
                c0, c1 = max(0, lc - rad), min(W - 1, lc + rad)
                for r in range(r0, r1 + 1):
                    for c in range(c0, c1 + 1):
                        d = math.hypot(r - lr, c - lc)
                        if d > rad + 1e-6:
                            continue
                        strength = att * math.exp(-d / 2.5)
                        prev = best_for_line.get((r, c))
                        if prev is None or strength > prev["line_strength"]:
                            best_for_line[(r, c)] = {
                                "line_id": lid,
                                "zone_id": zid,
                                "commander_id": fl.get("commander_id"),
                                "unit_type_mask": mask,
                                "line_role": lrole,
                                "source_deploy_line_id": src_dl,
                                "target_deploy_line_id": tgt_dl,
                                "end_px": end_px,
                                "axis_vec": axis_vec,
                                "attraction_weight": att,
                                "line_strength": strength,
                                "dist_px": d,
                                "nearest_r": lr,
                                "nearest_c": lc,
                                "normal_r": nr,
                                "normal_c": nc,
                            }
            for rc, entry in best_for_line.items():
                field.setdefault(rc, []).append(entry)
        self.frontline_channel[team] = field

    def write_typed_frontline(
        self,
        line_id,
        team,
        points_px,
        unit_type_mask=LINE_MASK_INFANTRY,
        attraction_weight=1.0,
        front_normal=None,
        commander_id=None,
        zone_id=None,
        spread_radius_px=4,
        line_role=LINE_ROLE_DEPLOY,
        source_deploy_line_id=None,
        target_deploy_line_id=None,
    ):
        """把带兵种标签与角色类型（DEPLOY/MOVE/ATTACK/GLOBAL_ACTION）的战线折线直接光栅化写入地形容器势场通道。"""
        cells, default_n = rasterize_polyline(points_px, shape=self.shape)
        if not cells:
            return None
        if front_normal is not None:
            nr, nc = float(front_normal[0]), float(front_normal[1])
            nm = math.hypot(nr, nc)
            norm_vec = (nr / nm, nc / nm) if nm > 1e-9 else default_n
        else:
            norm_vec = default_n

        pts_int = [[int(round(float(p[0]))), int(round(float(p[1])))] for p in points_px]
        start_px = list(pts_int[0])
        end_px = list(pts_int[-1])
        ad_r = float(end_px[0] - start_px[0])
        ad_c = float(end_px[1] - start_px[1])
        ad_m = math.hypot(ad_r, ad_c)
        axis_vec = [round(ad_r / ad_m, 5), round(ad_c / ad_m, 5)] if ad_m > 1e-9 else [round(norm_vec[0], 5), round(norm_vec[1], 5)]

        entry = {
            "line_id": str(line_id),
            "team": str(team),
            "commander_id": commander_id,
            "zone_id": zone_id,
            "unit_type_mask": int(unit_type_mask or LINE_MASK_ALL),
            "line_role": str(line_role or LINE_ROLE_DEPLOY),
            "source_deploy_line_id": str(source_deploy_line_id) if source_deploy_line_id is not None else None,
            "target_deploy_line_id": str(target_deploy_line_id) if target_deploy_line_id is not None else None,
            "start_px": start_px,
            "end_px": end_px,
            "axis_vec": axis_vec,
            "attraction_weight": float(attraction_weight),
            "front_normal": [round(norm_vec[0], 5), round(norm_vec[1], 5)],
            "points_px": pts_int,
            "raster_cells": [list(rc) for rc in cells],
            "spread_radius_px": int(max(1, spread_radius_px)),
        }
        self.typed_frontlines[str(line_id)] = entry
        self._rebuild_frontline_channel(str(team))
        return entry

    def remove_typed_frontline(self, line_id):
        fl = self.typed_frontlines.pop(str(line_id), None)
        if fl is not None:
            self._rebuild_frontline_channel(fl["team"])
            return True
        return False

    def clear_team_frontlines(self, team, commander_id=None):
        to_del = [
            lid for lid, fl in self.typed_frontlines.items()
            if fl["team"] == team and (commander_id is None or fl.get("commander_id") == commander_id)
        ]
        for lid in to_del:
            self.typed_frontlines.pop(lid, None)
        if to_del:
            self._rebuild_frontline_channel(team)
        return len(to_del)

    def sample_ao_and_frontline_field(self, px, team, unit_type_mask, zone_id=None, commander_id=None, active_deploy_line_id=None):
        """从地形容器读取当前像素的 AO 边界约束、双线分工+溯源势场与本级全局行动线方向。"""
        r, c = int(round(float(px[0]))), int(round(float(px[1])))
        in_ao = True
        ao_pull_vec = [0.0, 0.0]
        t_chan = self.ao_channel.get(team)
        if t_chan and t_chan["zones"]:
            if zone_id is not None and zone_id in t_chan["zones"]:
                z_info = t_chan["zones"][zone_id]
                in_ao = (r, c) in z_info["cells"]
                if not in_ao:
                    cr, cc = z_info["centroid"]
                    dr, dc = cr - float(px[0]), cc - float(px[1])
                    d = math.hypot(dr, dc)
                    if d > 1e-6:
                        ao_pull_vec = [dr / d, dc / d]
            elif commander_id is not None:
                cmd_zones = {zid: zi for zid, zi in t_chan["zones"].items() if zi.get("commander_id") == commander_id}
                if cmd_zones:
                    in_ao = any((r, c) in zi["cells"] for zi in cmd_zones.values())
                    if not in_ao:
                        best_d = math.inf
                        best_vec = [0.0, 0.0]
                        for zi in cmd_zones.values():
                            cr, cc = zi["centroid"]
                            dr, dc = cr - float(px[0]), cc - float(px[1])
                            d = math.hypot(dr, dc)
                            if d < best_d and d > 1e-6:
                                best_d = d
                                best_vec = [dr / d, dc / d]
                        ao_pull_vec = best_vec
            else:
                active_zids = t_chan["cell_zones"].get((r, c))
                in_ao = bool(active_zids)
                if not in_ao:
                    best_d = math.inf
                    best_vec = [0.0, 0.0]
                    for z_info in t_chan["zones"].values():
                        cr, cc = z_info["centroid"]
                        dr, dc = cr - float(px[0]), cc - float(px[1])
                        d = math.hypot(dr, dc)
                        if d < best_d and d > 1e-6:
                            best_d = d
                            best_vec = [dr / d, dc / d]
                    ao_pull_vec = best_vec

        # 1) 提取直属本级全局行动线引导矢量 G_action
        ga_r, ga_c = 0.0, 0.0
        for fl in self.typed_frontlines.values():
            if fl["team"] != team or fl.get("line_role") != LINE_ROLE_GLOBAL_ACTION:
                continue
            fl_cid = fl.get("commander_id")
            if commander_id is not None and fl_cid is not None and fl_cid != commander_id:
                continue
            ax = fl.get("axis_vec") or [0.0, 0.0]
            w_ga = 1.5 if (commander_id is not None and fl_cid == commander_id) else 1.0
            ga_r += float(ax[0]) * w_ga
            ga_c += float(ax[1]) * w_ga
        ga_m = math.hypot(ga_r, ga_c)
        global_action_vec = [round(ga_r / ga_m, 5), round(ga_c / ga_m, 5)] if ga_m > 1e-6 else [0.0, 0.0]

        def _fl_allowed(ent_or_fl):
            fl_cid = ent_or_fl.get("commander_id")
            if commander_id is not None and fl_cid is not None and fl_cid != commander_id:
                return False
            role = ent_or_fl.get("line_role", LINE_ROLE_DEPLOY)
            if role == LINE_ROLE_GLOBAL_ACTION:
                return False
            if role in (LINE_ROLE_MOVE, LINE_ROLE_ATTACK):
                src_dl = ent_or_fl.get("source_deploy_line_id")
                if src_dl is not None and src_dl != active_deploy_line_id:
                    return False
            return True

        # 2) 检测是否踩入部署线（领标 active_deploy_line_id）或完成 MOVE/ATTACK 抵达终点
        claimed_deploy_line_id = None
        best_claim_w = -1.0
        f_entries = (self.frontline_channel.get(team) or {}).get((r, c), ())
        for ent in f_entries:
            if not _fl_allowed(ent):
                continue
            if ent.get("line_role", LINE_ROLE_DEPLOY) == LINE_ROLE_DEPLOY and float(ent.get("dist_px", 99.0)) <= 1.5:
                aff = unit_line_affinity(unit_type_mask, ent["unit_type_mask"])
                wc = float(ent["line_strength"]) * aff
                if aff >= 0.25 and wc > best_claim_w:
                    best_claim_w = wc
                    claimed_deploy_line_id = ent["line_id"]

        # 3) 检查是否存在匹配的激活移动/进攻线（优先级高于静态驻防部署线，到达终点<=1.5px则转入目标部署线）
        active_mv_id = None
        best_mv_score = -1.0
        best_mv_pull = [0.0, 0.0]
        best_mv_norm = [0.0, 0.0]
        best_mv_dist = math.inf
        best_mv_aff = 0.0
        best_mv_raw = 0.0
        best_mv_role = None

        for lid, fl in self.typed_frontlines.items():
            if fl["team"] != team or not _fl_allowed(fl):
                continue
            role = fl.get("line_role", LINE_ROLE_DEPLOY)
            if role not in (LINE_ROLE_MOVE, LINE_ROLE_ATTACK):
                continue
            end_px = fl.get("end_px") or fl["raster_cells"][-1]
            d_end = math.hypot(float(end_px[0]) - float(px[0]), float(end_px[1]) - float(px[1]))
            tgt_dl = fl.get("target_deploy_line_id")
            if d_end <= 1.5:
                if tgt_dl and tgt_dl in self.typed_frontlines:
                    claimed_deploy_line_id = tgt_dl
                continue
            aff = unit_line_affinity(unit_type_mask, fl["unit_type_mask"])
            att = float(fl.get("attraction_weight", 1.0))
            if aff * att <= 1e-4:
                continue
            rc_cells = fl["raster_cells"]
            step = max(1, len(rc_cells) // 16)
            min_cell = min(rc_cells[::step], key=lambda pt: (pt[0] - px[0]) ** 2 + (pt[1] - px[1]) ** 2)
            d_near = math.hypot(float(min_cell[0]) - float(px[0]), float(min_cell[1]) - float(px[1]))
            # 若离移动线较远则先拉向线路，接近后直接沿终末目标端前进
            if d_near > 1.6:
                dr = float(min_cell[0]) - float(px[0]) * 0.7 + float(end_px[0]) * 0.0
                dr = float(min_cell[0]) - float(px[0])
                dc = float(min_cell[1]) - float(px[1])
            else:
                dr = float(end_px[0]) - float(px[0])
                dc = float(end_px[1]) - float(px[1])
            dm = math.hypot(dr, dc)
            sc = att * aff * (1.25 if fl.get("source_deploy_line_id") == active_deploy_line_id else 1.0)
            if sc > best_mv_score:
                best_mv_score = sc
                active_mv_id = lid
                best_mv_aff = aff
                best_mv_raw = att
                best_mv_dist = d_near
                best_mv_norm = list(fl["front_normal"])
                best_mv_role = role
                best_mv_pull = [dr / dm, dc / dm] if dm > 1e-6 else [0.0, 0.0]

        if active_mv_id is not None:
            return {
                "in_ao": bool(in_ao),
                "ao_pull_vec": ao_pull_vec,
                "line_strength": round(best_mv_score, 5),
                "raw_line_strength": round(best_mv_raw, 5),
                "affinity": round(best_mv_aff, 4),
                "front_normal": best_mv_norm,
                "line_pull_vec": best_mv_pull,
                "best_line_id": active_mv_id,
                "best_line_role": best_mv_role,
                "dist_to_line_px": round(best_mv_dist, 3) if math.isfinite(best_mv_dist) else None,
                "claimed_deploy_line_id": claimed_deploy_line_id,
                "active_move_attack_line_id": active_mv_id,
                "global_action_vec": global_action_vec,
            }

        # 4) 分兵种部署线势场读取（局部直接命中的条目 + 未命中时退化检索直属匹配战线最近点）
        best_w = 0.0
        best_raw = 0.0
        best_aff = 0.0
        best_norm = [0.0, 0.0]
        best_pull = [0.0, 0.0]
        best_id = None
        best_role = LINE_ROLE_DEPLOY
        best_dist = math.inf
        for ent in f_entries:
            if not _fl_allowed(ent):
                continue
            aff = unit_line_affinity(unit_type_mask, ent["unit_type_mask"])
            w_eff = float(ent["line_strength"]) * aff
            if w_eff > best_w:
                best_w = w_eff
                best_raw = float(ent["line_strength"])
                best_aff = aff
                best_norm = [float(ent["normal_r"]), float(ent["normal_c"])]
                best_id = ent["line_id"]
                best_role = ent.get("line_role", LINE_ROLE_DEPLOY)
                best_dist = float(ent["dist_px"])
                dr = float(ent["nearest_r"]) - float(px[0])
                dc = float(ent["nearest_c"]) - float(px[1])
                dm = math.hypot(dr, dc)
                best_pull = [dr / dm, dc / dm] if dm > 1e-6 else [0.0, 0.0]

        if best_id is None and self.typed_frontlines:
            best_score = -1.0
            for lid, fl in self.typed_frontlines.items():
                if fl["team"] != team or not _fl_allowed(fl):
                    continue
                if zone_id is not None and fl.get("zone_id") and fl.get("zone_id") != zone_id:
                    continue
                aff = unit_line_affinity(unit_type_mask, fl["unit_type_mask"])
                att = float(fl.get("attraction_weight", 1.0))
                if aff * att <= 1e-4:
                    continue
                rc_cells = fl["raster_cells"]
                step = max(1, len(rc_cells) // 16)
                min_cell = min(rc_cells[::step], key=lambda pt: (pt[0] - px[0]) ** 2 + (pt[1] - px[1]) ** 2)
                dr = float(min_cell[0]) - float(px[0])
                dc = float(min_cell[1]) - float(px[1])
                dm = math.hypot(dr, dc)
                sc = (att * aff) / (1.0 + dm * 0.25)
                if sc > best_score:
                    best_score = sc
                    best_w = sc
                    best_raw = att * math.exp(-dm / 6.0)
                    best_aff = aff
                    best_norm = list(fl["front_normal"])
                    best_id = lid
                    best_role = fl.get("line_role", LINE_ROLE_DEPLOY)
                    best_dist = dm
                    best_pull = [dr / dm, dc / dm] if dm > 1e-6 else [0.0, 0.0]

        return {
            "in_ao": bool(in_ao),
            "ao_pull_vec": ao_pull_vec,
            "line_strength": round(best_w, 5),
            "raw_line_strength": round(best_raw, 5),
            "affinity": round(best_aff, 4),
            "front_normal": best_norm,
            "line_pull_vec": best_pull,
            "best_line_id": best_id,
            "best_line_role": best_role if best_id is not None else None,
            "dist_to_line_px": round(best_dist, 3) if math.isfinite(best_dist) else None,
            "claimed_deploy_line_id": claimed_deploy_line_id,
            "active_move_attack_line_id": None,
            "global_action_vec": global_action_vec,
        }

    # ---- 持久化 ----
    def save_npz(self, path):
        import json as _json
        stash_payload = {f"{r},{c}": v for (r, c), v in self.cell_stash.items()}
        mods_payload = {f"{r},{c}": v for (r, c), v in self.cell_modifiers.items()}
        damaged_payload = {f"{r},{c}": int(t) for (r, c), t in self.damaged_terrain.items()}
        wire_payload = {t: [[list(a), list(b)] for a, b in sorted(edges)] for t, edges in self.wire_edges.items()}
        ao_payload = {
            t: {
                zid: {
                    "cells": [list(rc) for rc in sorted(zinfo["cells"])],
                    "commander_id": zinfo.get("commander_id"),
                }
                for zid, zinfo in ch["zones"].items()
            }
            for t, ch in self.ao_channel.items()
        }
        frontlines_payload = list(self.typed_frontlines.values())
        feat_meta = [
            {"id": fid, "modifiers": ch.get("modifiers"), "terrain_code": ch.get("terrain_code")}
            for fid, ch in self.feature_channels.items()
        ]
        feat_stack = (
            np.stack([np.asarray(ch["mask"], dtype=np.uint8) for ch in self.feature_channels.values()], axis=0)
            if self.feature_channels
            else np.zeros((0, *self.shape), dtype=np.uint8)
        )
        np.savez_compressed(
            path, elevation=self.elevation, terrain=self.terrain, weather=self.weather,
            visibility_mod=self.visibility_mod, smoke_ttl=self.smoke_ttl,
            illumination=self.illumination, illum_ttl=self.illum_ttl,
            exposure=self.exposure,
            meta=np.array([self.cell_m, *self.origin_latlon, float(self.slope_window_px)], dtype=np.float64),
            bridges=np.array(sorted(self.destroyed_bridges), dtype=np.int32).reshape(-1, 2),
            stash_json=np.array(_json.dumps(stash_payload, ensure_ascii=False)),
            cell_mods_json=np.array(_json.dumps(mods_payload, ensure_ascii=False)),
            damaged_json=np.array(_json.dumps(damaged_payload, ensure_ascii=False)),
            wire_json=np.array(_json.dumps(wire_payload, ensure_ascii=False)),
            ao_channel_json=np.array(_json.dumps(ao_payload, ensure_ascii=False)),
            frontlines_json=np.array(_json.dumps(frontlines_payload, ensure_ascii=False)),
            feat_meta_json=np.array(_json.dumps(feat_meta, ensure_ascii=False)),
            feat_stack=feat_stack,
        )

    @classmethod
    def load_npz(cls, path):
        import json as _json
        data = np.load(path)
        meta = [float(v) for v in data["meta"]]
        cell_m, lat0, lon0 = meta[:3]
        win = int(meta[3]) if len(meta) >= 4 else 1
        g = cls(data["elevation"], cell_m, (lat0, lon0), data["terrain"], data["weather"], slope_window_px=win)
        g.visibility_mod = data["visibility_mod"].astype(np.float32)
        g.smoke_ttl = data["smoke_ttl"].astype(np.float32)
        g.illumination = data["illumination"].astype(np.float32)
        g.illum_ttl = data["illum_ttl"].astype(np.float32)
        if "exposure" in data:
            g.exposure = data["exposure"].astype(np.float32)
        g.destroyed_bridges = {(int(r), int(c)) for r, c in data["bridges"]}
        if "stash_json" in data:
            raw = _json.loads(str(data["stash_json"].item()))
            for rc_s, val in raw.items():
                r, c = [int(x) for x in rc_s.split(",")]
                g.cell_stash[(r, c)] = {
                    "resources": {k: float(v) for k, v in val.get("resources", {}).items()},
                    "equipment": {k: int(v) for k, v in val.get("equipment", {}).items()},
                    "owner_team": val.get("owner_team"),
                    "radio_channel": val.get("radio_channel", "A"),
                }
        if "cell_mods_json" in data:
            raw_m = _json.loads(str(data["cell_mods_json"].item()))
            for rc_s, bucket in raw_m.items():
                r, c = [int(x) for x in rc_s.split(",")]
                g.cell_modifiers[(r, c)] = {
                    k: {"modifiers": dict(v.get("modifiers", {})), "ttl_hours": v.get("ttl_hours")}
                    for k, v in bucket.items()
                }
        if "damaged_json" in data:
            raw_d = _json.loads(str(data["damaged_json"].item()))
            for rc_s, val in raw_d.items():
                r, c = [int(x) for x in rc_s.split(",")]
                g.damaged_terrain[(r, c)] = int(val)
        if "wire_json" in data:
            raw_w = _json.loads(str(data["wire_json"].item()))
            for t, pairs in raw_w.items():
                g.wire_edges[t] = {((int(a[0]), int(a[1])), (int(b[0]), int(b[1]))) for a, b in pairs}
        if "feat_meta_json" in data and "feat_stack" in data:
            feat_meta = _json.loads(str(data["feat_meta_json"].item()))
            feat_stack = data["feat_stack"]
            for i, fm in enumerate(feat_meta):
                if i < feat_stack.shape[0]:
                    g.feature_channels[str(fm["id"])] = {
                        "mask": feat_stack[i].astype(bool),
                        "modifiers": fm.get("modifiers"),
                        "terrain_code": fm.get("terrain_code"),
                    }
        if "ao_channel_json" in data:
            raw_ao = _json.loads(str(data["ao_channel_json"].item()))
            for t, zmap in raw_ao.items():
                for zid, zinfo in zmap.items():
                    g.write_ao_to_container(zid, t, zinfo.get("cells", ()), commander_id=zinfo.get("commander_id"))
        if "frontlines_json" in data:
            raw_fl = _json.loads(str(data["frontlines_json"].item()))
            for fl in raw_fl:
                g.write_typed_frontline(
                    fl["line_id"], fl["team"], fl["points_px"],
                    unit_type_mask=fl.get("unit_type_mask", LINE_MASK_INFANTRY),
                    attraction_weight=fl.get("attraction_weight", 1.0),
                    front_normal=fl.get("front_normal"),
                    commander_id=fl.get("commander_id"),
                    zone_id=fl.get("zone_id"),
                    spread_radius_px=fl.get("spread_radius_px", 4),
                )
        g.refresh_attrition()
        return g


def pos_to_px(pos):
    """连续浮点坐标 (fx, fy) 实时投影到所属像素容器索引 (r, c)。"""
    return (int(round(float(pos[0]))), int(round(float(pos[1]))))


def sub_hex(pos):
    """O(1) 像素内六边形微槽解析映射（零全图建表开销）：中心为 0，外环 6 个方向扇区为 1..6。"""
    import math as _math
    px = pos_to_px(pos)
    dr = float(pos[0]) - px[0]
    dc = float(pos[1]) - px[1]
    if dr * dr + dc * dc < 0.15 * 0.15:
        return (px, 0)
    ang = (_math.degrees(_math.atan2(dr, dc)) + 360.0) % 360.0
    sector = int((ang + 30.0) // 60.0) % 6 + 1
    return (px, sector)


def disk_mask(shape, center, radius_px):
    rr, cc = np.ogrid[:shape[0], :shape[1]]
    return (rr - center[0]) ** 2 + (cc - center[1]) ** 2 <= float(radius_px) ** 2


def is_road_array(terrain):
    return np.isin(terrain, ROAD_TYPES)


# ==========================================
# 🚶 通行代价
# ==========================================
def cost_layer(grid, mobility):
    """逐像素基础代价倍率（含天气与像素特征 Buff/Debuff）；inf 表示该机动类别不可进入。"""
    if mobility not in TERRAIN_COST:
        raise ValueError(f"未知机动类别: {mobility}")
    t, w = grid.terrain, grid.weather
    base = np.full(grid.shape, np.inf, dtype=np.float64)
    for code, mult in TERRAIN_COST[mobility].items():
        base[t == code] = mult
    road = is_road_array(t).copy()
    ignore_slope = road.copy()
    speed_scale = np.ones(grid.shape, dtype=np.float64)
    impassable = np.zeros(grid.shape, dtype=bool)
    bridge_mask = np.zeros(grid.shape, dtype=bool)
    mob_key = ("wheeled_speed_mult" if mobility == "wheeled"
               else ("tracked_speed_mult" if mobility == "tracked" else "speed_mult"))

    for fid, ch in getattr(grid, "feature_channels", {}).items():
        mask = np.asarray(ch["mask"], dtype=bool)
        if not np.any(mask):
            continue
        spec = TERRAIN_FEATURE_SPECS.get(fid, {})
        mods = ch.get("modifiers") if ch.get("modifiers") is not None else spec.get("modifiers", {})
        tc = ch.get("terrain_code") if ch.get("terrain_code") is not None else spec.get("terrain_code")
        if mods.get("impassable"):
            impassable |= mask
        if mods.get("is_road"):
            road |= mask
        if mods.get("ignore_slope_limit") or mods.get("is_road"):
            ignore_slope |= mask
        if mods.get("bridge_over_water"):
            bridge_mask |= mask
            road |= mask
            ignore_slope |= mask
            continue
        if mods.get("is_road") and tc in TERRAIN_COST[mobility]:
            r_cost = TERRAIN_COST[mobility][tc]
            rmask = mask & (t != tc) & (t != WATER) & (t != BRIDGE)
            base[rmask] = np.minimum(base[rmask], r_cost)
            continue
        sm = mods.get(mob_key, mods.get("speed_mult"))
        if sm is not None:
            sm_f = float(sm)
            if sm_f <= 0.0:
                impassable |= mask
            else:
                apply_m = mask if tc is None else (mask & (t != int(tc)) & (~road))
                speed_scale[apply_m] *= sm_f

    for rc, bucket in getattr(grid, "cell_modifiers", {}).items():
        r, c = rc
        if not (0 <= r < grid.shape[0] and 0 <= c < grid.shape[1]):
            continue
        t_code = int(t[r, c])
        primary_fid = TERRAIN_CODE_TO_FEATURE.get(t_code)
        for key, entry in bucket.items():
            if not key.startswith("feat:"):
                continue
            mods = entry.get("modifiers", {})
            fid = key[5:]
            spec = TERRAIN_FEATURE_SPECS.get(fid, {})
            tc = spec.get("terrain_code")
            if mods.get("impassable"):
                impassable[r, c] = True
            if mods.get("is_road"):
                road[r, c] = True
                ignore_slope[r, c] = True
            if mods.get("ignore_slope_limit"):
                ignore_slope[r, c] = True
            if mods.get("bridge_over_water"):
                bridge_mask[r, c] = True
                road[r, c] = True
                ignore_slope[r, c] = True
                continue
            if mods.get("is_road") and tc in TERRAIN_COST[mobility] and t_code != tc and t_code not in (WATER, BRIDGE):
                base[r, c] = min(base[r, c], TERRAIN_COST[mobility][tc])
                continue
            if fid == primary_fid:
                continue
            if tc is not None and road[r, c]:
                continue
            sm = mods.get(mob_key, mods.get("speed_mult"))
            if sm is not None:
                sm_f = float(sm)
                if sm_f <= 0.0:
                    impassable[r, c] = True
                else:
                    speed_scale[r, c] *= sm_f

    if np.any(bridge_mask):
        impassable[bridge_mask] = False
        b_fill = bridge_mask & ((t == WATER) | np.isinf(base))
        base[b_fill] = TERRAIN_COST[mobility][BRIDGE]
    wmult = np.where(road, _WEATHER_ROAD[w], _WEATHER_OFFROAD[w])
    cost = base * wmult
    with np.errstate(divide="ignore", invalid="ignore"):
        cost = np.where(speed_scale > 0.0, cost / speed_scale, np.inf)
    cost[impassable] = np.inf
    cost[(~ignore_slope) & (grid.slope_deg > SLOPE_LIMIT_DEG[mobility])] = np.inf
    return cost


def directional_slope_factor(slope_along, mobility):
    """slope_along = 沿行进方向的上升/水平距离（上坡为正）。"""
    if slope_along >= 0:
        return 1.0 + UPHILL_K[mobility] * slope_along
    return max(DOWNHILL_FLOOR, 1.0 + DOWNHILL_BRAKE * slope_along)


def dynamic_path_cost_layer(grid, mobility, R=0.0, alpha_stealth=0.05, beta1=0.65, beta2=0.55, air_exposure=0.0):
    """V3.1 动态寻路代价矩阵：
    cost(x) = move_time(x) + (alpha_stealth + beta1 * R + beta2 * air_exposure) * (1 - cover(x))
    """
    move_time = cost_layer(grid, mobility)
    t = grid.terrain
    cov = np.zeros(grid.shape, dtype=np.float64)
    for code, v in COVER.items():
        if v > 0.0:
            cov[t == code] = v
    for fid, ch in getattr(grid, "feature_channels", {}).items():
        mask = np.asarray(ch["mask"], dtype=bool)
        if not np.any(mask):
            continue
        spec = TERRAIN_FEATURE_SPECS.get(fid, {})
        mods = ch.get("modifiers") if ch.get("modifiers") is not None else spec.get("modifiers", {})
        cb = float(mods.get("cover_bonus", 0.0) or 0.0)
        if cb > 0.0:
            cov[mask] = np.maximum(cov[mask], cb)
    for rc, bucket in getattr(grid, "cell_modifiers", {}).items():
        r, c = rc
        if 0 <= r < grid.shape[0] and 0 <= c < grid.shape[1]:
            cov[r, c] = max(cov[r, c], grid.effective_cover_at((r, c)))
    cov = np.clip(cov, 0.0, 0.95)
    grid_air = np.minimum(2.0, np.maximum(0.0, grid.exposure.astype(np.float64) / 25.0))
    tot_air = float(air_exposure) + grid_air
    penalty = (float(alpha_stealth) + float(beta1) * float(R) + float(beta2) * tot_air) * (1.0 - cov)
    return np.where(np.isfinite(move_time), move_time + penalty, np.inf)


def dijkstra(grid, start, mobility, extra_mult=None, blocked=None,
             max_cost=math.inf, goal=None, want_prev=False, dynamic_cost_params=None):
    """从 start 出发的最小等效公里代价场。

    extra_mult : (H,W) 额外倍率（弹幕 / 低空扫射造成的迟滞），None 表示无。
    blocked    : (H,W) 布尔，True 的像素不可进入（如敌控区）。
    goal       : 给定则到达即提前结束。
    dynamic_cost_params : dict，提供 V3.1 动态寻路代价参数 {R, alpha_stealth, beta1, beta2, air_exposure}。
    """
    H, W = grid.shape
    start = (int(start[0]), int(start[1]))
    if dynamic_cost_params is not None:
        layer = dynamic_path_cost_layer(
            grid,
            mobility,
            R=dynamic_cost_params.get("R", 0.0),
            alpha_stealth=dynamic_cost_params.get("alpha_stealth", 0.05),
            beta1=dynamic_cost_params.get("beta1", 0.65),
            beta2=dynamic_cost_params.get("beta2", 0.55),
            air_exposure=dynamic_cost_params.get("air_exposure", 0.0),
        )
    else:
        layer = cost_layer(grid, mobility)
    if blocked is not None:
        layer = np.where(blocked, np.inf, layer)
    cost = layer.tolist()
    extra = extra_mult.tolist() if extra_mult is not None else None
    gx, gy = grid.grad_x.tolist(), grid.grad_y.tolist()
    step_km = grid.cell_m / 1000.0
    kk = UPHILL_K[mobility]

    dist = np.full((H, W), np.inf)
    prev = np.full((H, W), -1, dtype=np.int64) if want_prev else None
    dist_l = [[math.inf] * W for _ in range(H)]
    prev_l = [[-1] * W for _ in range(H)] if want_prev else None
    if not math.isfinite(cost[start[0]][start[1]]):
        # 起点本身不可通行（例如停在桥被炸后的水边）：仍允许从这里走出去
        cost[start[0]][start[1]] = 1.0
    dist_l[start[0]][start[1]] = 0.0
    heap = [(0.0, start[0], start[1])]
    while heap:
        d, r, c = heapq.heappop(heap)
        if d > dist_l[r][c]:
            continue
        if goal is not None and (r, c) == goal:
            break
        cf = cost[r][c]
        for dr, dc in _NEIGHBORS:
            nr, nc = r + dr, c + dc
            if nr < 0 or nc < 0 or nr >= H or nc >= W:
                continue
            ct = cost[nr][nc]
            if ct == math.inf:
                continue
            norm = math.sqrt(dr * dr + dc * dc)
            mgx = 0.5 * (gx[r][c] + gx[nr][nc])
            mgy = 0.5 * (gy[r][c] + gy[nr][nc])
            s_along = (mgy * dr + mgx * dc) / norm
            if s_along >= 0:
                sf = 1.0 + kk * s_along
            else:
                sf = max(DOWNHILL_FLOOR, 1.0 + DOWNHILL_BRAKE * s_along)
            nd = d + step_km * norm * 0.5 * (cf + ct) * sf * (extra[nr][nc] if extra else 1.0)
            if nd > max_cost or nd >= dist_l[nr][nc]:
                continue
            dist_l[nr][nc] = nd
            if want_prev:
                prev_l[nr][nc] = r * W + c
            heapq.heappush(heap, (nd, nr, nc))
    dist = np.array(dist_l, dtype=np.float64)
    if want_prev:
        prev = np.array(prev_l, dtype=np.int64)
    return dist, prev


def reach_map(grid, start, budget_km, mobility, extra_mult=None, blocked=None, dynamic_cost_params=None):
    """预算内可达的像素代价场（超预算为 inf）。"""
    dist, _ = dijkstra(grid, start, mobility, extra_mult, blocked, max_cost=budget_km, dynamic_cost_params=dynamic_cost_params)
    return dist


class _SubGridWindow:
    """局部只读栅格视窗，避免大图上为短距离寻路/射线检测拷贝全图矩阵。"""

    def __init__(self, grid, r0, r1, c0, c1):
        self._parent = grid
        self.r0 = int(r0)
        self.c0 = int(c0)
        self.shape = (int(r1) - int(r0), int(c1) - int(c0))
        self.cell_m = grid.cell_m
        self.terrain = grid.terrain[r0:r1, c0:c1]
        self.weather = grid.weather[r0:r1, c0:c1]
        self.grad_x = grid.grad_x[r0:r1, c0:c1]
        self.grad_y = grid.grad_y[r0:r1, c0:c1]
        self.slope_deg = grid.slope_deg[r0:r1, c0:c1]
        self.exposure = grid.exposure[r0:r1, c0:c1]
        self.feature_channels = {
            fid: {
                "mask": ch["mask"][r0:r1, c0:c1],
                "modifiers": ch.get("modifiers"),
                "terrain_code": ch.get("terrain_code"),
            }
            for fid, ch in getattr(grid, "feature_channels", {}).items()
        }
        self.cell_modifiers = {
            (r - r0, c - c0): bucket
            for (r, c), bucket in getattr(grid, "cell_modifiers", {}).items()
            if r0 <= r < r1 and c0 <= c < c1
        }

    def effective_cover_at(self, px):
        return self._parent.effective_cover_at((px[0] + self.r0, px[1] + self.c0))


def _shortest_path_windowed(grid, start, goal, mobility, extra_mult=None, blocked=None,
                            max_cost=math.inf, dynamic_cost_params=None):
    """大图短距寻路局部子窗口加速：先在起止包络窗口内精确求路，失败再回退全图。"""
    H, W = grid.shape
    span = max(abs(goal[0] - start[0]), abs(goal[1] - start[1]))
    margin = max(48, span // 2 + 24)
    r0 = max(0, min(start[0], goal[0]) - margin)
    r1 = min(H, max(start[0], goal[0]) + margin + 1)
    c0 = max(0, min(start[1], goal[1]) - margin)
    c1 = min(W, max(start[1], goal[1]) + margin + 1)
    sub = _SubGridWindow(grid, r0, r1, c0, c1)
    extra_sub = extra_mult[r0:r1, c0:c1] if extra_mult is not None else None
    blocked_sub = blocked[r0:r1, c0:c1] if blocked is not None else None
    s_loc = (start[0] - r0, start[1] - c0)
    g_loc = (goal[0] - r0, goal[1] - c0)
    dist, prev = dijkstra(
        sub, s_loc, mobility, extra_sub, blocked_sub,
        max_cost=max_cost, goal=g_loc, want_prev=True, dynamic_cost_params=dynamic_cost_params,
    )
    if not math.isfinite(dist[g_loc]):
        return None
    sub_w = sub.shape[1]
    path, cur = [], g_loc[0] * sub_w + g_loc[1]
    costs = []
    while cur != -1:
        lr, lc = cur // sub_w, cur % sub_w
        path.append((lr + r0, lc + c0))
        costs.append(float(dist[lr, lc]))
        cur = int(prev[lr, lc])
    path.reverse()
    costs.reverse()
    return path, costs


def shortest_path(grid, start, goal, mobility, extra_mult=None, blocked=None, max_cost=math.inf, dynamic_cost_params=None):
    """返回 (路径像素列表, 对应累计代价列表)；不可达返回 None。"""
    start = (int(start[0]), int(start[1]))
    goal = (int(goal[0]), int(goal[1]))
    H, W = grid.shape
    if H * W > 65536 and max(abs(goal[0] - start[0]), abs(goal[1] - start[1])) <= 256:
        res = _shortest_path_windowed(
            grid, start, goal, mobility, extra_mult=extra_mult, blocked=blocked,
            max_cost=max_cost, dynamic_cost_params=dynamic_cost_params,
        )
        if res is not None:
            return res
    dist, prev = dijkstra(grid, start, mobility, extra_mult, blocked, max_cost, goal, True, dynamic_cost_params=dynamic_cost_params)
    if not math.isfinite(dist[goal]):
        return None
    W = grid.shape[1]
    path, cur = [], goal[0] * W + goal[1]
    while cur != -1:
        path.append((cur // W, cur % W))
        cur = int(prev[cur // W, cur % W])
    path.reverse()
    return path, [float(dist[p]) for p in path]


def advance_along(path, costs, budget_km):
    """沿路径前进到预算允许的最远像素。返回 (像素, 已用代价, 是否到达终点)。"""
    last = 0
    for idx, cst in enumerate(costs):
        if cst <= budget_km + 1e-9:
            last = idx
        else:
            break
    return path[last], costs[last], last == len(path) - 1


# ==========================================
# 👁️ 能见度 / 烟雾 / 照明
# ==========================================
def visibility_field(grid, night=False):
    """逐像素有效能见度（0—1）：天气 × 烟雾，夜间再乘照明修正。"""
    vis = WEATHER_VIS[grid.weather] * grid.visibility_mod
    if night:
        vis = vis * (NIGHT_BASE_VIS + (1.0 - NIGHT_BASE_VIS) * grid.illumination)
    return vis


def apply_smoke(grid, center, radius_px, strength=0.9, ttl_h=2.0):
    """烟幕：直接改写像素 visibility_mod，并记录持续时间。"""
    mask = disk_mask(grid.shape, center, radius_px)
    grid.visibility_mod[mask] = np.minimum(grid.visibility_mod[mask], max(0.02, 1.0 - strength))
    grid.smoke_ttl[mask] = np.maximum(grid.smoke_ttl[mask], ttl_h)
    return int(mask.sum())


def apply_illumination(grid, center, radius_px, ttl_h=1.0):
    mask = disk_mask(grid.shape, center, radius_px)
    grid.illumination[mask] = 1.0
    grid.illum_ttl[mask] = np.maximum(grid.illum_ttl[mask], ttl_h)
    return int(mask.sum())


def tick_environment(grid, hours):
    """时间流逝：烟幕与照明衰减，到期恢复。"""
    grid.smoke_ttl = np.maximum(0.0, grid.smoke_ttl - hours)
    grid.visibility_mod[grid.smoke_ttl <= 0] = 1.0
    grid.illum_ttl = np.maximum(0.0, grid.illum_ttl - hours)
    grid.illumination[grid.illum_ttl <= 0] = 0.0


def line_visibility(grid, a, b, night=False, check_elevation=True):
    """a→b 视线沿途最低能见度（0—1）。烟幕/浓雾压低能见度；山脊高程或特征遮蔽物（树冠/街区/自定义遮蔽）阻断视线。"""
    n = int(max(abs(b[0] - a[0]), abs(b[1] - a[1]))) + 1
    rr = np.clip(np.rint(np.linspace(a[0], b[0], n)).astype(int), 0, grid.shape[0] - 1)
    cc = np.clip(np.rint(np.linspace(a[1], b[1], n)).astype(int), 0, grid.shape[1] - 1)
    if check_elevation and n > 2:
        h_a = float(grid.elevation[rr[0], cc[0]]) + 2.0
        h_b = float(grid.elevation[rr[-1], cc[-1]]) + 2.0
        los_h = np.linspace(h_a, h_b, n)[1:-1]
        mid_r, mid_c = rr[1:-1], cc[1:-1]
        los_block = np.where(grid.terrain[mid_r, mid_c] == FOREST, 8.0, 0.0)
        los_block = np.where(grid.terrain[mid_r, mid_c] == URBAN, np.maximum(los_block, 12.0), los_block)
        for fid, ch in getattr(grid, "feature_channels", {}).items():
            spec = TERRAIN_FEATURE_SPECS.get(fid, {})
            mods = ch.get("modifiers") if ch.get("modifiers") is not None else spec.get("modifiers", {})
            lb = float(mods.get("los_block_m", 0.0) or 0.0)
            if lb > 0.0:
                los_block = np.where(ch["mask"][mid_r, mid_c], np.maximum(los_block, lb), los_block)
        if getattr(grid, "cell_modifiers", None):
            for idx, (mr, mc) in enumerate(zip(mid_r.tolist(), mid_c.tolist())):
                bucket = grid.cell_modifiers.get((mr, mc))
                if not bucket:
                    continue
                for entry in bucket.values():
                    lb = float(entry.get("modifiers", {}).get("los_block_m", 0.0) or 0.0)
                    if lb > los_block[idx]:
                        los_block[idx] = lb
        mid_elev = grid.elevation[mid_r, mid_c].astype(np.float64) + los_block
        if np.any(mid_elev > los_h + 1e-6):
            return 0.0
    vis_ray = WEATHER_VIS[grid.weather[rr, cc]] * grid.visibility_mod[rr, cc]
    if night:
        vis_ray = vis_ray * (NIGHT_BASE_VIS + (1.0 - NIGHT_BASE_VIS) * grid.illumination[rr, cc])
    return float(vis_ray.min())


def can_aim(grid, shooter_px, target_px, night=False, threshold=0.25):
    """直瞄火力 / 空中俯冲瞄准是否被遮蔽或地形反斜面切断。"""
    return line_visibility(grid, shooter_px, target_px, night) >= threshold


def compute_fire_arc(grid, pos, range_m, min_range_m=0.0, indirect=False, night=False, max_scan_m=3000.0):
    """只读射界热力计算：取消硬射程上限，按基准有效射程 R_eff 输出连续精度热力 [[r, c, eff], ...]。"""
    max_r = max(0.0, float(range_m))
    min_r = max(0.0, float(min_range_m))
    if max_r <= 0.0:
        return []
    scan_r = max_r * 1.5 if indirect else max(max_r, float(max_scan_m))
    r0, c0 = float(pos[0]), float(pos[1])
    px0 = pos_to_px(pos)
    lim = int(math.ceil(scan_r / grid.cell_m)) + 1
    r_lo = max(0, px0[0] - lim)
    r_hi = min(grid.shape[0] - 1, px0[0] + lim)
    c_lo = max(0, px0[1] - lim)
    c_hi = min(grid.shape[1] - 1, px0[1] + lim)
    cells = []
    for r in range(r_lo, r_hi + 1):
        for c in range(c_lo, c_hi + 1):
            dist_m = math.hypot(r - r0, c - c0) * grid.cell_m
            if dist_m < min_r - 1e-6 or dist_m > scan_r + 1e-6:
                continue
            if not indirect and line_visibility(grid, px0, (r, c), night=night) <= 1e-6:
                continue
            eff = 1.0 / (1.0 + (dist_m / max(1.0, max_r)) ** 2)
            if eff >= 0.005:
                cells.append([r, c, round(eff, 4)])
    return cells


# ==========================================
# 🏔️ 合成地形（测试与演示）
# ==========================================
def synthetic_grid(h=120, w=160, cell_m=200.0, seed=7, origin_latlon=(35.90, 112.70)):
    """确定性合成战区：东西向山脊 + 南北向河流 + 跨河桥梁 + 公路/铁路 + 野战机场。"""
    rng = np.random.default_rng(seed)
    rr, cc = np.mgrid[:h, :w].astype(np.float64)
    elev = 200.0 + 40.0 * np.sin(cc / 17.0) * np.cos(rr / 23.0)
    elev += 220.0 * np.exp(-((rr - h * 0.35) ** 2) / (2 * 6.0 ** 2))          # 山脊
    elev += rng.normal(0, 1.5, (h, w))
    terrain = np.zeros((h, w), np.uint8)
    terrain[elev > 380] = MOUNTAIN
    forest = (rng.random((h, w)) < 0.08) & (elev < 300)
    terrain[forest] = FOREST
    river_col = int(w * 0.55)
    terrain[:, river_col - 1:river_col + 1] = WATER
    elev[:, river_col - 1:river_col + 1] -= 25.0
    road_row = int(h * 0.7)
    terrain[road_row, :] = HIGHWAY
    terrain[road_row, river_col - 1:river_col + 1] = BRIDGE
    rail_row = int(h * 0.85)
    terrain[rail_row, :] = RAIL
    terrain[rail_row, river_col - 1:river_col + 1] = BRIDGE
    terrain[road_row:rail_row + 1, 6] = DIRT_ROAD
    terrain[road_row - 3:road_row, w - 12:w - 8] = AIRFIELD
    g = Grid(elev, cell_m, origin_latlon, terrain)
    g.synthetic_meta = {"river_col": river_col, "road_row": road_row, "rail_row": rail_row,
                        "ridge_row": int(h * 0.35)}
    return g
