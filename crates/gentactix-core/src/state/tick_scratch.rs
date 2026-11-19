use crate::clock::TransactionPhase;
use crate::domain::ecs::{ActiveEvent, UnitId};
use crate::intent::IntentArbitrator;
use crate::math_ops::AuctionBid;
use rustc_hash::{FxHashMap, FxHashSet};

/// 引擎级 TickScratch：单 16s Tick 瞬态工作区，Stage 12 出大门前物理清空
#[derive(Debug, Default)]
pub struct SimTickScratch {
    pub tick_id: u64,
    pub delivered_orders: usize,
    pub saved_direct_positions: FxHashMap<usize, ([f32; 2], (i32, i32), f32)>,
    pub auction_bids: Vec<AuctionBid>,
    pub auction_capacities: FxHashMap<usize, usize>,
    pub auto_moved_count: usize,
    pub auto_attack_count: usize,
    pub direct_moved_count: usize,
    pub exploded_points: Vec<[f32; 2]>,
    pub combat_pairs: Vec<(UnitId, UnitId, String, f32)>,
    pub due_events: Vec<ActiveEvent>,
    pub completed_events: usize,
    pub air_sorties: usize,
    pub logi_conservation_error: f32,
    pub arbitrator: IntentArbitrator,
    pub direct_controlled_uids: FxHashSet<usize>,
    pub executed_phases: Vec<TransactionPhase>,
}

impl SimTickScratch {
    pub fn begin_tick(&mut self, tick_id: u64) {
        self.clear();
        self.tick_id = tick_id;
    }

    pub fn clear(&mut self) {
        self.delivered_orders = 0;
        self.saved_direct_positions.clear();
        self.auction_bids.clear();
        self.auction_capacities.clear();
        self.auto_moved_count = 0;
        self.auto_attack_count = 0;
        self.direct_moved_count = 0;
        self.exploded_points.clear();
        self.combat_pairs.clear();
        self.due_events.clear();
        self.completed_events = 0;
        self.air_sorties = 0;
        self.logi_conservation_error = 0.0;
        self.arbitrator.candidates.clear();
        self.direct_controlled_uids.clear();
        self.executed_phases.clear();
    }

    pub fn is_clean(&self) -> bool {
        self.saved_direct_positions.is_empty()
            && self.auction_bids.is_empty()
            && self.auction_capacities.is_empty()
            && self.exploded_points.is_empty()
            && self.combat_pairs.is_empty()
            && self.due_events.is_empty()
            && self.arbitrator.candidates.is_empty()
            && self.direct_controlled_uids.is_empty()
            && self.executed_phases.is_empty()
    }
}
