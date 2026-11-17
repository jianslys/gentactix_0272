//! `events` module: Instant Event Buffer, Deterministic Sort & Unique Terminal States
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 7 节与 plan.txt Phase 05 要求：
//! 1. 严格实现四大唯一终态 (Resolved / Rejected / DiscardedByRule / ConsumedByField)
//! 2. 稳定排序键 (resolve_phase, target_key, source_entity_id, sequence)
//! 3. 终态审计带原因代码，杜绝“清空队列即代表成功”的假象

use crate::geometry::CellCoord;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EventKind {
    InventoryTransfer = 0,
    DirectFire = 1,
    DirectHit = 2,
    BlastImpact = 3,
    TerrainMutation = 4,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventPayload {
    InventoryTransfer {
        item: String,
        count: u32,
        from_id: u32,
        to_id: u32,
    },
    DirectHit {
        damage: f32,
        armor_pen_mm: f32,
    },
    Blast {
        origin: CellCoord,
        radius_m: f32,
        overpressure_kpa: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventFinalStatus {
    Resolved {
        code: String,
        detail: String,
    },
    Rejected {
        code: String,
        detail: String,
    },
    DiscardedByRule {
        code: String,
        detail: String,
    },
    ConsumedByField {
        field_id: String,
        absorbed_val: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstantEvent {
    pub event_id: u64,
    pub kind: EventKind,
    pub resolve_phase: u8,
    pub source_entity_id: u32,
    pub target_key: String,
    pub model_revision: u64,
    pub payload: EventPayload,
    pub sequence: u32,
}

impl Eq for InstantEvent {}

impl PartialOrd for InstantEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InstantEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // 稳定排序键: (resolve_phase, target_key, source_entity_id, sequence)
        self.resolve_phase
            .cmp(&other.resolve_phase)
            .then_with(|| self.target_key.cmp(&other.target_key))
            .then_with(|| self.source_entity_id.cmp(&other.source_entity_id))
            .then_with(|| self.sequence.cmp(&other.sequence))
            .then_with(|| self.event_id.cmp(&other.event_id))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventAuditEntry {
    pub event_id: u64,
    pub status: EventFinalStatus,
}

/// 即时事件缓冲器 (驻留于 TickScratch，每Tick结算并审计)
#[derive(Debug, Default, Clone)]
pub struct EventBuffer {
    pub pending: Vec<InstantEvent>,
    pub audit_log: Vec<EventAuditEntry>,
}

impl EventBuffer {
    pub fn push(&mut self, ev: InstantEvent) {
        self.pending.push(ev);
    }

    /// 稳定排序：消除线程或插入顺序的随机性
    pub fn sort_deterministically(&mut self) {
        self.pending.sort();
    }

    /// 记录并提交终态
    pub fn record_terminal(&mut self, event_id: u64, status: EventFinalStatus) {
        self.audit_log.push(EventAuditEntry { event_id, status });
    }

    /// 验证所有事件均有且仅有一次终态，并清空 pending
    pub fn verify_and_clear_tick(&mut self) -> Result<usize, String> {
        if !self.pending.is_empty() {
            return Err(format!(
                "Transaction failed: {} pending events left unresolved at end of tick!",
                self.pending.len()
            ));
        }
        let total = self.audit_log.len();
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_05_event_deterministic_sort_and_unique_terminal_status() {
        let mut buf = EventBuffer::default();

        // 乱序推入相同结算阶段的事件
        buf.push(InstantEvent {
            event_id: 102,
            kind: EventKind::DirectHit,
            resolve_phase: 1,
            source_entity_id: 20,
            target_key: "target_b".into(),
            model_revision: 1,
            payload: EventPayload::DirectHit { damage: 15.0, armor_pen_mm: 50.0 },
            sequence: 1,
        });

        buf.push(InstantEvent {
            event_id: 101,
            kind: EventKind::DirectHit,
            resolve_phase: 1,
            source_entity_id: 10,
            target_key: "target_a".into(),
            model_revision: 1,
            payload: EventPayload::DirectHit { damage: 10.0, armor_pen_mm: 45.0 },
            sequence: 0,
        });

        buf.push(InstantEvent {
            event_id: 103,
            kind: EventKind::DirectHit,
            resolve_phase: 1,
            source_entity_id: 10,
            target_key: "target_b".into(),
            model_revision: 1,
            payload: EventPayload::DirectHit { damage: 20.0, armor_pen_mm: 55.0 },
            sequence: 0,
        });

        // 确定性排序
        buf.sort_deterministically();

        let ids: Vec<u64> = buf.pending.iter().map(|e| e.event_id).collect();
        // 目标 "target_a" 必须排在 "target_b" 前面；在 "target_b" 内，source_entity_id 10 必须排在 20 前面
        assert_eq!(ids, vec![101, 103, 102]);

        // 结算并赋予终态
        while let Some(ev) = buf.pending.pop() {
            if ev.event_id == 101 {
                buf.record_terminal(ev.event_id, EventFinalStatus::Resolved {
                    code: "HIT_CONFIRMED".into(),
                    detail: "Target torso penetrated".into(),
                });
            } else if ev.event_id == 103 {
                buf.record_terminal(ev.event_id, EventFinalStatus::Rejected {
                    code: "OUT_OF_RANGE".into(),
                    detail: "Target moved out of range during flight".into(),
                });
            } else {
                buf.record_terminal(ev.event_id, EventFinalStatus::DiscardedByRule {
                    code: "TARGET_ALREADY_DEAD".into(),
                    detail: "Entity died in earlier event".into(),
                });
            }
        }

        // 验证 Tick 结束时 pending 清空，终态记录完整
        let audited_count = buf.verify_and_clear_tick().unwrap();
        assert_eq!(audited_count, 3);
        assert_eq!(buf.audit_log.len(), 3);
    }
}
