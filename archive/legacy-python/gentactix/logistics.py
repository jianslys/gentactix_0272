# -*- coding: utf-8 -*-
"""实体化后勤（V2）：道路像素链干线接力 + 尽头圆域士兵亲自去取 + 源头总量守恒。

模型
----
段 1  干线：从补给点出发沿连通的道路属性像素（公路/铁路/土路/桥梁）推进，
      只计极低的里程损耗；路被敌军占领（enemy_block）或桥被炸毁即断。
段 2  末端：在道路尽头像素为起点、半径 R 的圆域内，**前线必须派人去取**：
      运力 = 编内搬运人力（士兵肩挑）+ 卡车；运力不足时从战斗班组抽调士兵（borrowed_soldiers）。
      往返耗时与损耗由路径上每个像素的地形/坡度/泥泞决定（重新起算衰减）；
      路径穿过火力封锁区（hazard）时搬运队减员、物资送达率下降。
守恒  所有补给只来自补给点库存：  Σ 送达 + Σ 途损 == Σ 出库 ≤ 初始库存。
      `check_conservation()` 是算法比对用的不变量断言，不是业务开局填表要求。

资源单位：吨（food / ammo / fuel）；replacements 以人计，自己走路，无需搬运运力。
"""

from __future__ import annotations

import heapq
import math

import numpy as np

from . import terrain_field as tf

RESOURCES = ("food", "ammo", "fuel", "replacements")
TONNAGE = ("food", "ammo", "fuel")

TRUNK_LOSS_PER_100KM = 0.01        # 干线每百公里损耗
LASTMILE_LOSS_PER_KM = 0.02        # 末端每等效公里损耗（散落/遗失）
PORTER_LOAD_T = 0.025              # 每名士兵每趟肩挑 25 kg
PORTER_SPEED_KMH = 4.0
LOAD_HOURS = 0.5                   # 每趟装卸耗时
HAZARD_PORTER_CASUALTY = 0.25      # 路径平均 hazard=1 时每趟搬运减员比例
HAZARD_LOSS = 0.8                  # hazard=1 时物资送达率损失
DEFAULT_RADIUS_KM = 12.0
DEFAULT_PRIORITY = ("food", "ammo", "fuel")
# 选道路尽头时：干线靠火车/卡车，比步行快约 10 倍，所以干线里程只按 0.1 权重计入选址代价
TRUNK_SELECT_WEIGHT = 0.1


class Depot:
    def __init__(self, did, px, team, stock=None, ammo_by_type=None, meta=None):
        self.id = did
        self.px = (int(px[0]), int(px[1]))
        self.team = team
        self.stock = {r: 0.0 for r in RESOURCES}
        self.stock.update(stock or {})
        self.ammo_by_type = {k: float(v) for k, v in (ammo_by_type or {}).items()}
        self.meta = dict(meta or {})

    def has_ammo_for(self, ammo_type):
        if not self.ammo_by_type:
            return self.stock.get("ammo", 0.0) > 0.0
        return self.ammo_by_type.get(ammo_type, 0.0) > 0.0

    def to_dict(self):
        return dict(
            id=self.id,
            px=list(self.px),
            team=self.team,
            stock=dict(self.stock),
            ammo_by_type=dict(self.ammo_by_type),
            meta=dict(self.meta),
        )

    @classmethod
    def from_dict(cls, d):
        return cls(d["id"], d["px"], d["team"], d.get("stock"), d.get("ammo_by_type"), meta=d.get("meta"))


# ==========================================
# 🛣️ 道路网
# ==========================================
def road_network(grid, seed_px, blocked=None, snap_px=3):
    """从 seed 出发沿道路像素连通的区域。返回 (km 距离场, 种子像素)；无道路返回 (None, None)。"""
    H, W = grid.shape
    road = tf.is_road_array(grid.terrain)
    if blocked is not None:
        road = road & ~blocked
    seed = (int(seed_px[0]), int(seed_px[1]))
    if not road[seed]:
        best, bd = None, 1e18
        for dr in range(-snap_px, snap_px + 1):
            for dc in range(-snap_px, snap_px + 1):
                r, c = seed[0] + dr, seed[1] + dc
                if 0 <= r < H and 0 <= c < W and road[r, c] and dr * dr + dc * dc < bd:
                    best, bd = (r, c), dr * dr + dc * dc
        if best is None:
            return None, None
        seed = best
    step = grid.cell_m / 1000.0
    dist = np.full((H, W), np.inf)
    dist[seed] = 0.0
    heap = [(0.0, seed[0], seed[1])]
    road_l = road.tolist()
    dist_l = dist.tolist()
    while heap:
        d, r, c = heapq.heappop(heap)
        if d > dist_l[r][c]:
            continue
        for dr, dc in tf._NEIGHBORS:
            nr, nc = r + dr, c + dc
            if 0 <= nr < H and 0 <= nc < W and road_l[nr][nc]:
                nd = d + step * (1.4142 if dr and dc else 1.0)
                if nd < dist_l[nr][nc]:
                    dist_l[nr][nc] = nd
                    heapq.heappush(heap, (nd, nr, nc))
    return np.array(dist_l), seed


def _path_hazard(path, hazard):
    if hazard is None or not path:
        return 0.0
    rr = [p[0] for p in path]
    cc = [p[1] for p in path]
    return float(np.mean(hazard[rr, cc]))


# ==========================================
# 📦 配送
# ==========================================
def dispatch(grid, depots, consumers, hours, hazard=None, enemy_block=None,
             radius_km=DEFAULT_RADIUS_KM, logistics_mult_fn=None):
    """对一批消费者（部队/炮兵连）执行一次配送。会**原地扣减** depot.stock。

    consumer 字段：
      id, px, team, demand{res:吨/人}, porters(编内搬运士兵数), trucks_t(卡车每趟吨位),
      borrowable(可抽调的战斗士兵上限), priority(可选, 吨位类资源的分配顺序)
    返回 {consumer_id: 报告}。消费者按传入顺序依次处理，共享补给点库存按先到先得扣减。
    """
    nets = {}
    for d in depots:
        dist, seed = road_network(grid, d.px, enemy_block)
        nets[d.id] = (dist, seed)

    report = {}
    for con in consumers:
        cid, cpx = con["id"], (int(con["px"][0]), int(con["px"][1]))
        demand = {r: float(con.get("demand", {}).get(r, 0.0)) for r in RESOURCES}
        rep = {"consumer": cid, "status": "ok", "depot": None, "roadhead": None,
               "trunk_km": None, "lastmile_cost_km": None, "trips": 0, "capacity_t": 0.0,
               "porters_used": 0, "borrowed_soldiers": 0, "porter_casualties": 0, "hazard": 0.0,
               "delivered": {r: 0.0 for r in RESOURCES}, "drawn": {r: 0.0 for r in RESOURCES},
               "losses": {r: 0.0 for r in RESOURCES}, "unmet": dict(demand)}
        report[cid] = rep
        if sum(demand.values()) <= 0:
            rep["status"] = "no_demand"
            continue

        foot, _ = tf.dijkstra(grid, cpx, "foot", max_cost=radius_km)
        wheel, _ = tf.dijkstra(grid, cpx, "wheeled", max_cost=radius_km)

        # 选取总代价最小的 (补给点, 道路尽头) 组合；库存为空的补给点排在后面
        cands = []
        for d in depots:
            if d.team != con.get("team", d.team):
                continue
            dist, _seed = nets[d.id]
            if dist is None:
                continue
            reach = np.isfinite(dist) & np.isfinite(foot)
            if not reach.any():
                continue
            total = np.where(reach, dist * TRUNK_SELECT_WEIGHT + foot, np.inf)
            idx = int(np.argmin(total))
            rh = (idx // grid.shape[1], idx % grid.shape[1])
            has_stock = any(d.stock.get(r, 0.0) > 1e-9 for r in RESOURCES if demand[r] > 0)
            cands.append((0 if has_stock else 1, float(total[rh]), d.id, d, rh, float(dist[rh])))
        if not cands:
            rep["status"] = "no_road_link"
            continue
        cands.sort(key=lambda t: (t[0], t[1], str(t[2])))
        _, _, _, depot, roadhead, trunk_km = cands[0]
        rep.update(depot=depot.id, roadhead=roadhead, trunk_km=trunk_km,
                   lastmile_cost_km=float(foot[roadhead]))

        pth = tf.shortest_path(grid, cpx, roadhead, "foot", max_cost=radius_km)
        h = _path_hazard(pth[0] if pth else [], hazard)
        rep["hazard"] = h
        l_mult = max(1.0, float(logistics_mult_fn(cpx, con.get("team")))) if logistics_mult_fn else 1.0
        trunk_arr = (1.0 - TRUNK_LOSS_PER_100KM) ** (trunk_km / 100.0)
        lm_cost = float(foot[roadhead])
        lm_loss = min(0.5, (LASTMILE_LOSS_PER_KM / l_mult) * lm_cost)
        haz_loss = min(0.95, HAZARD_LOSS * h)
        end_to_end = (1.0 - lm_loss) * (1.0 - haz_loss)

        rt_foot = 2.0 * lm_cost / PORTER_SPEED_KMH + LOAD_HOURS
        trips = int(math.floor(hours / rt_foot + 1e-9))
        rep["trips"] = trips
        porters = int(con.get("porters", 0))
        truck_t = float(con.get("trucks_t", 0.0))
        truck_cap = 0.0
        if truck_t > 0 and math.isfinite(wheel[roadhead]):
            rt_truck = 2.0 * float(wheel[roadhead]) / 12.0 + LOAD_HOURS
            truck_cap = truck_t * math.floor(hours / rt_truck + 1e-9)
        porter_cap = porters * PORTER_LOAD_T * trips
        capacity = (porter_cap + truck_cap) * l_mult

        # 拟运量：先看库存能给多少（若双方配置了细分弹药口径，则按对口口径可用量限额）
        order = [r for r in con.get("priority", DEFAULT_PRIORITY) if r in TONNAGE]
        req_cals = list(con.get("required_ammo_types") or [])
        use_caliber = bool(req_cals and getattr(depot, "ammo_by_type", None))
        avail_ammo = (
            sum(depot.ammo_by_type.get(c, 0.0) for c in req_cals)
            if use_caliber else depot.stock.get("ammo", 0.0)
        )
        want = {
            r: min(demand[r], (avail_ammo if r == "ammo" else depot.stock.get(r, 0.0)) * trunk_arr)
            for r in order
        }
        total_want = sum(want.values())
        borrowed = 0
        if total_want > capacity + 1e-12 and trips > 0:
            shortfall = total_want - capacity
            need = int(math.ceil(shortfall / (PORTER_LOAD_T * trips)))
            borrowed = min(need, int(con.get("borrowable", 0)))
            capacity += borrowed * PORTER_LOAD_T * trips
        rep["capacity_t"] = capacity
        rep["borrowed_soldiers"] = borrowed
        used_porters = 0
        remaining = capacity
        for r in order:
            carried = min(want[r], remaining)
            remaining -= carried
            if carried <= 1e-12:
                continue
            drawn = carried / trunk_arr
            cap_r = avail_ammo if (r == "ammo" and use_caliber) else depot.stock.get(r, 0.0)
            drawn = min(drawn, cap_r)
            carried = drawn * trunk_arr
            depot.stock[r] -= drawn
            if r == "ammo" and use_caliber:
                rem_d = drawn
                cal_deliv = {}
                for c in req_cals:
                    if rem_d <= 1e-12:
                        break
                    have_c = depot.ammo_by_type.get(c, 0.0)
                    take_c = min(have_c, rem_d)
                    if take_c > 1e-12:
                        depot.ammo_by_type[c] = have_c - take_c
                        rem_d -= take_c
                        cal_deliv[c] = round(take_c * trunk_arr * end_to_end, 4)
                rep["ammo_by_type_delivered"] = cal_deliv
            delivered = carried * end_to_end
            rep["drawn"][r] += drawn
            rep["delivered"][r] += delivered
            rep["losses"][r] += drawn - delivered
        carried_total = capacity - remaining
        if porter_cap + borrowed * PORTER_LOAD_T * trips > 0 and trips > 0:
            frac = min(1.0, carried_total / max(1e-12, capacity)) if capacity > 0 else 0.0
            rep["porters_used"] = int(round((porters + borrowed) * frac))
        rep["porter_casualties"] = int(math.floor(rep["porters_used"] * HAZARD_PORTER_CASUALTY * h))

        # 替补兵员：自己走路，不占运力，只受路径火力影响
        if demand["replacements"] > 0:
            ok = min(demand["replacements"], depot.stock.get("replacements", 0.0))
            take = int(math.floor(ok))
            depot.stock["replacements"] -= take
            arrived = int(math.floor(take * (1.0 - min(0.95, HAZARD_PORTER_CASUALTY * h))))
            rep["drawn"]["replacements"] = float(take)
            rep["delivered"]["replacements"] = float(arrived)
            rep["losses"]["replacements"] = float(take - arrived)

        for r in RESOURCES:
            rep["unmet"][r] = max(0.0, demand[r] - rep["delivered"][r])
        if trips == 0:
            rep["status"] = "too_far_for_turn"
        elif sum(rep["delivered"][r] for r in TONNAGE) + rep["delivered"]["replacements"] <= 1e-12:
            rep["status"] = "nothing_delivered"
    return report


def check_conservation(report, initial_stocks, depots, tol=1e-9):
    """比对验证不变量：不凭空造物资。返回 (是否通过, 明细)。

    1) 每个补给点：初始库存 − 当前库存 == 该点所有出库之和（不多不少）
    2) 每份报告：送达 + 途损 == 出库
    3) 任何资源总送达 ≤ 初始总库存
    """
    problems = []
    for d in depots:
        for r in RESOURCES:
            drawn = sum(rep["drawn"][r] for rep in report.values() if rep["depot"] == d.id)
            lost_from_stock = initial_stocks[d.id][r] - d.stock[r]
            if abs(drawn - lost_from_stock) > tol * max(1.0, initial_stocks[d.id][r]):
                problems.append(f"{d.id}/{r}: 出库{drawn:.6f} ≠ 库存减少{lost_from_stock:.6f}")
            if d.stock[r] < -tol:
                problems.append(f"{d.id}/{r}: 库存为负 {d.stock[r]}")
    for cid, rep in report.items():
        for r in RESOURCES:
            if abs(rep["delivered"][r] + rep["losses"][r] - rep["drawn"][r]) > tol * max(1.0, rep["drawn"][r]):
                problems.append(f"{cid}/{r}: 送达+途损 ≠ 出库")
    for r in RESOURCES:
        total_init = sum(s[r] for s in initial_stocks.values())
        total_deliv = sum(rep["delivered"][r] for rep in report.values())
        if total_deliv > total_init + tol:
            problems.append(f"{r}: 总送达 {total_deliv} 超过总库存 {total_init}")
    return (not problems), problems


def check_ammo_compatibility(piece, available_ammo_types=None):
    """按具体装备型号校验弹药口径覆盖情况：返回 (全覆盖否, 所需口径集合, 缺失口径集合)。"""
    needed = piece.required_ammo_types() if hasattr(piece, "required_ammo_types") else set()
    if not available_ammo_types:
        return True, needed, set()
    avail = {k for k, v in available_ammo_types.items() if v > 0} if isinstance(available_ammo_types, dict) else set(available_ammo_types)
    missing = needed - avail
    return (not missing), needed, missing


def consumer_from_piece(piece, grid, demand=None, borrow_ratio=0.15):
    """把 MapPiece 转成配送消费者：搬运人力取自编内辎重/搬运班，可抽调战斗兵。"""
    squads = piece.squads()
    porters = sum(s.enlisted for s in squads if s.kind in ("porter_squad", "transport_squad"))
    trucks_t = sum(s.carry_capacity_t()[1] for s in squads if s.kind == "transport_squad")
    fighters = sum(s.enlisted for s in squads if s.kind not in ("porter_squad", "transport_squad"))
    if demand is None:
        demand = {r: max(0.0, piece.stock_cap.get(r, 0.0) - piece.stock.get(r, 0.0)) for r in TONNAGE}
    return {"id": piece.id, "px": piece.px, "team": piece.team, "demand": demand,
            "porters": porters, "trucks_t": trucks_t, "borrowable": int(fighters * borrow_ratio),
            "required_ammo_types": sorted(piece.required_ammo_types()) if hasattr(piece, "required_ammo_types") else []}
