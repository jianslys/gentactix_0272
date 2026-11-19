use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// 算子 A-1 · 九点各向同性保质量平滑核 K_iso (alpha = 0.25)
/// 中心权重 1 - alpha = 0.75，正交 4 格各 alpha/6 = 0.041667，对角 4 格各 alpha/12 = 0.020833
pub fn apply_k_iso_smoothing(grid: &mut [f32], rows: usize, cols: usize, alpha: f32) {
    if rows < 3 || cols < 3 || grid.len() < rows * cols {
        return;
    }
    let w_center = 1.0 - alpha;
    let w_ortho = alpha / 6.0;
    let w_diag = alpha / 12.0;

    let buf = grid.to_vec();
    for r in 1..rows - 1 {
        for c in 1..cols - 1 {
            let idx = r * cols + c;
            let center = buf[idx];
            let ortho_sum = buf[(r - 1) * cols + c]
                + buf[(r + 1) * cols + c]
                + buf[r * cols + (c - 1)]
                + buf[r * cols + (c + 1)];
            let diag_sum = buf[(r - 1) * cols + (c - 1)]
                + buf[(r - 1) * cols + (c + 1)]
                + buf[(r + 1) * cols + (c - 1)]
                + buf[(r + 1) * cols + (c + 1)];
            grid[idx] = w_center * center + w_ortho * ortho_sum + w_diag * diag_sum;
        }
    }
}

/// 算子 A-2 · 确定性 Tier-B 多容量最小堆拍卖 (Deterministic Multi-Capacity Min-Heap Auction)
/// 严格采用定点整数代价与 (cost_fixed, unit_id, cell_idx) 字典序打破平局，杜绝浮点 exp/log 与跨平台分叉
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuctionBid {
    pub cost_fixed: i64,
    pub unit_id: u32,
    pub cell_idx: usize,
}

impl Ord for AuctionBid {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 最小堆：代价小的优先；字典序解决平局
        other
            .cost_fixed
            .cmp(&self.cost_fixed)
            .then_with(|| other.unit_id.cmp(&self.unit_id))
            .then_with(|| other.cell_idx.cmp(&self.cell_idx))
    }
}

impl PartialOrd for AuctionBid {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 求解确定性拍卖分配：
/// 传入候选出价列表，每格容量限制 cell_capacities，单位最多认领 1 个目标
pub fn solve_deterministic_auction(
    bids: Vec<AuctionBid>,
    cell_capacities: &FxHashMap<usize, usize>,
) -> Vec<(u32, usize)> {
    use std::collections::BinaryHeap;
    let mut heap = BinaryHeap::from(bids);
    let mut assigned_units = rustc_hash::FxHashSet::default();
    let mut cell_occupancy: FxHashMap<usize, usize> = FxHashMap::default();
    let mut results = Vec::new();

    while let Some(bid) = heap.pop() {
        if assigned_units.contains(&bid.unit_id) {
            continue;
        }
        let cap = cell_capacities.get(&bid.cell_idx).copied().unwrap_or(1);
        let cur = cell_occupancy.get(&bid.cell_idx).copied().unwrap_or(0);
        if cur < cap {
            assigned_units.insert(bid.unit_id);
            cell_occupancy.insert(bid.cell_idx, cur + 1);
            results.push((bid.unit_id, bid.cell_idx));
        }
    }

    results.sort_by_key(|&(uid, cidx)| (uid, cidx));
    results
}

pub use solve_deterministic_auction as run_deterministic_multi_capacity_auction;


/// 铁律二 · G 场空间类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GFieldKind {
    /// 状态占坑型 (人在账消，人走/人亡账回)
    State,
    /// 积分做功型 (不可逆累积做功销账，人走绝不反弹)
    Work,
}

/// 铁律一 · G 场待分配残差计算：在途预扣 vs 到场实扣
/// G_alloc = max(0, G0 - sum(on_site) - sum(en_route))
#[inline]
pub fn compute_g_alloc(g0: f32, on_site: f32, en_route: f32) -> f32 {
    (g0 - on_site - en_route).max(0.0)
}

/// 铁律二 · 积分做功型 G^work 不可逆销账：G0(t+dt) = max(0, G0(t) - delta_w)
#[inline]
pub fn apply_g_work_consumption(g0: &mut f32, delta_w: f32) {
    *g0 = (*g0 - delta_w).max(0.0);
}

