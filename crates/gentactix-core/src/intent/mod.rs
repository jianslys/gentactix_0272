//! `intent` module: Motion Intent Candidates & Deterministic Arbitration
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 5 节与 plan.txt Phase 04 要求：
//! 1. 直控与自主均只能产生 IntentCandidate
//! 2. 仲裁器确保每实体每 Tick 至多输出一个 MotionIntent
//! 3. 严格禁止任何子系统直接改写实体坐标

use crate::geometry::WorldPos;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IntentSource {
    DirectOrder = 0,    // 玩家/指挥官直接作战军令 (最高优先级)
    AutonomousAi = 1,   // 作战区/战线自主维持警戒 (次优先级)
    EmergencyRout = 2,  // 溃退紧急避险
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentCandidate {
    pub entity_id: u32,
    pub source: IntentSource,
    pub desired_speed_mps: f32,
    pub target_pos: Option<WorldPos>,
    pub heading_deg: f32,
    pub timestamp_sec: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionIntent {
    pub entity_id: u32,
    pub winning_source: IntentSource,
    pub speed_mps: f32,
    pub target_pos: Option<WorldPos>,
    pub heading_deg: f32,
    pub arbitration_reason: String,
}

/// 确定性意图仲裁器
#[derive(Debug, Default)]
pub struct IntentArbitrator {
    pub candidates: Vec<IntentCandidate>,
}

impl IntentArbitrator {
    pub fn push_candidate(&mut self, candidate: IntentCandidate) {
        self.candidates.push(candidate);
    }

    /// 执行仲裁：每实体至多保留一个最高优先级的意图
    pub fn arbitrate(&mut self) -> Vec<MotionIntent> {
        // 按 (entity_id, source) 稳定排序，确保 DirectOrder 优先于 AutonomousAi
        self.candidates.sort_by(|a, b| {
            a.entity_id
                .cmp(&b.entity_id)
                .then_with(|| (a.source as u8).cmp(&(b.source as u8)))
                .then_with(|| a.timestamp_sec.total_cmp(&b.timestamp_sec))
        });

        let mut results = Vec::new();
        let mut last_entity_id: Option<u32> = None;

        for cand in &self.candidates {
            if Some(cand.entity_id) == last_entity_id {
                // 该实体已有更高优先级意图被选中，丢弃次级意图
                continue;
            }

            last_entity_id = Some(cand.entity_id);
            let reason = match cand.source {
                IntentSource::DirectOrder => "Direct order took precedence over autonomy".to_string(),
                IntentSource::AutonomousAi => "Autonomous movement accepted (no direct order)".to_string(),
                IntentSource::EmergencyRout => "Emergency rout took override".to_string(),
            };

            results.push(MotionIntent {
                entity_id: cand.entity_id,
                winning_source: cand.source,
                speed_mps: cand.desired_speed_mps,
                target_pos: cand.target_pos,
                heading_deg: cand.heading_deg,
                arbitration_reason: reason,
            });
        }

        self.candidates.clear();
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_arbitration_direct_precedence() {
        let mut arb = IntentArbitrator::default();

        // 实体 101 同时收到自主警戒移动和玩家直控推进
        arb.push_candidate(IntentCandidate {
            entity_id: 101,
            source: IntentSource::AutonomousAi,
            desired_speed_mps: 1.2,
            target_pos: Some(WorldPos::new(20.0, 20.0)),
            heading_deg: 0.0,
            timestamp_sec: 10.0,
        });

        arb.push_candidate(IntentCandidate {
            entity_id: 101,
            source: IntentSource::DirectOrder,
            desired_speed_mps: 3.5,
            target_pos: Some(WorldPos::new(50.0, 50.0)),
            heading_deg: 45.0,
            timestamp_sec: 10.0,
        });

        // 实体 102 仅收到自主巡逻
        arb.push_candidate(IntentCandidate {
            entity_id: 102,
            source: IntentSource::AutonomousAi,
            desired_speed_mps: 1.0,
            target_pos: Some(WorldPos::new(30.0, 30.0)),
            heading_deg: 90.0,
            timestamp_sec: 10.0,
        });

        let intents = arb.arbitrate();
        assert_eq!(intents.len(), 2, "Each entity must produce at most one intent");

        // 实体 101 胜出者必须为 DirectOrder
        let i101 = intents.iter().find(|i| i.entity_id == 101).unwrap();
        assert_eq!(i101.winning_source, IntentSource::DirectOrder);
        assert_eq!(i101.speed_mps, 3.5);

        // 实体 102 胜出者为 AutonomousAi
        let i102 = intents.iter().find(|i| i.entity_id == 102).unwrap();
        assert_eq!(i102.winning_source, IntentSource::AutonomousAi);
    }
}
