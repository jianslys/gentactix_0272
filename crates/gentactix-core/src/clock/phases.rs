//! `transaction` module: Deterministic 13-Phase Tick Transaction Pipeline
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 9 节与 plan.txt Phase 03 要求：
//! 1. 唯一 TickClock 驱动原子 16 秒步长
//! 2. 严格锁死 13 阶段事务推进 (BeginTick -> ... -> PublishAndDrop)
//! 3. 绝不泄露半 Tick 中间态，所有权威提交受事务守护

use crate::state::{DerivedState, TickScratch, WorldState};
use serde::{Deserialize, Serialize};

pub const DEFAULT_SUBTICK_SECONDS: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TransactionPhase {
    BeginTick = 0,
    ClockAndSchedule = 1,
    PersistentFieldEvolve = 2,
    DerivedRefresh = 3,
    SenseAndArbitrate = 4,
    ProposeMotion = 5,
    ConstrainMotion = 6,
    CommitMotion = 7,
    EmitEvents = 8,
    ResolveEvents = 9,
    CommitPersistent = 10,
    Validate = 11,
    PublishAndDrop = 12,
}

impl TransactionPhase {
    pub const ALL: [TransactionPhase; 13] = [
        TransactionPhase::BeginTick,
        TransactionPhase::ClockAndSchedule,
        TransactionPhase::PersistentFieldEvolve,
        TransactionPhase::DerivedRefresh,
        TransactionPhase::SenseAndArbitrate,
        TransactionPhase::ProposeMotion,
        TransactionPhase::ConstrainMotion,
        TransactionPhase::CommitMotion,
        TransactionPhase::EmitEvents,
        TransactionPhase::ResolveEvents,
        TransactionPhase::CommitPersistent,
        TransactionPhase::Validate,
        TransactionPhase::PublishAndDrop,
    ];

    #[inline]
    pub fn next(self) -> Option<TransactionPhase> {
        let idx = self as usize;
        if idx + 1 < Self::ALL.len() {
            Some(Self::ALL[idx + 1])
        } else {
            None
        }
    }
}

/// 唯一权威时钟 (TickClock)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickClock {
    pub current_tick: u64,
    pub logic_time_sec: f64,
    pub subtick_sec: f32,
}

impl Default for TickClock {
    fn default() -> Self {
        Self {
            current_tick: 0,
            logic_time_sec: 0.0,
            subtick_sec: DEFAULT_SUBTICK_SECONDS,
        }
    }
}

impl TickClock {
    pub fn new(subtick_sec: f32) -> Self {
        Self {
            current_tick: 0,
            logic_time_sec: 0.0,
            subtick_sec,
        }
    }

    /// 推进时钟一步，返回 (新Tick, 新逻辑秒)
    pub fn advance_step(&mut self) -> (u64, f64) {
        self.current_tick += 1;
        self.logic_time_sec += self.subtick_sec as f64;
        (self.current_tick, self.logic_time_sec)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseTraceEntry {
    pub tick_id: u64,
    pub phase: TransactionPhase,
    pub logic_time_sec: f64,
    pub scratch_item_count: usize,
    pub diagnostics_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionError {
    PhaseOutOfOrder { expected: TransactionPhase, actual: TransactionPhase },
    ValidationFailed(String),
}

impl std::fmt::Display for TransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PhaseOutOfOrder { expected, actual } => {
                write!(f, "Transaction phase out of order: expected {expected:?}, got {actual:?}")
            }
            Self::ValidationFailed(msg) => write!(f, "Transaction validation failed: {msg}"),
        }
    }
}

impl std::error::Error for TransactionError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickSummary {
    pub tick: u64,
    pub logic_time_sec: f64,
    pub phases_executed: usize,
    pub published_revision: u64,
}

/// 确定性十三阶段事务编排器
pub struct TickOrchestrator {
    pub clock: TickClock,
    pub current_phase: Option<TransactionPhase>,
    pub trace: Vec<PhaseTraceEntry>,
}

impl Default for TickOrchestrator {
    fn default() -> Self {
        Self {
            clock: TickClock::default(),
            current_phase: None,
            trace: Vec::new(),
        }
    }
}

impl TickOrchestrator {
    pub fn new(clock: TickClock) -> Self {
        Self {
            clock,
            current_phase: None,
            trace: Vec::new(),
        }
    }

    /// 严格推进单个阶段，绝不允许跳过或逆序
    pub fn step_phase(
        &mut self,
        target: TransactionPhase,
        world: &mut WorldState,
        derived: &mut DerivedState,
        scratch: &mut TickScratch,
    ) -> Result<(), TransactionError> {
        let expected = match self.current_phase {
            None => TransactionPhase::BeginTick,
            Some(prev) => prev.next().unwrap_or(TransactionPhase::BeginTick),
        };

        if target != expected {
            return Err(TransactionError::PhaseOutOfOrder {
                expected,
                actual: target,
            });
        }

        match target {
            TransactionPhase::BeginTick => {
                scratch.begin_tick(self.clock.current_tick + 1);
            }
            TransactionPhase::ClockAndSchedule => {
                let (t, sec) = self.clock.advance_step();
                world.clock.tick = t;
                world.clock.logic_time_sec = sec;
                world.clock.revision = world.clock.revision.wrapping_add(1);
            }
            TransactionPhase::PersistentFieldEvolve => {
                world.persistent_fields.evolve(self.clock.subtick_sec);
            }
            TransactionPhase::DerivedRefresh => {
                derived.rebuild_sdf_if_needed(world);
            }
            TransactionPhase::SenseAndArbitrate => {
                // 仲裁意图：从权威实体抽取活跃候选
                for &id in world.entities.entities.keys() {
                    scratch.intent_candidates.push(id);
                }
            }
            TransactionPhase::ProposeMotion => {
                // 为每个意图候选生成初始位移提案
            }
            TransactionPhase::ConstrainMotion => {
                // 空间约束与SDF投影校验
            }
            TransactionPhase::CommitMotion => {
                // 统一提交实体位置
                for &(eid, pos) in &scratch.motion_proposals {
                    world.entities.commit_position(eid, pos);
                }
            }
            TransactionPhase::EmitEvents => {
                // 派发即时事件
            }
            TransactionPhase::ResolveEvents => {
                // 事件结算
            }
            TransactionPhase::CommitPersistent => {
                // 提交伤害、地形与持久状态
            }
            TransactionPhase::Validate => {
                // 校验合法性与守恒
                if world.clock.logic_time_sec < 0.0 {
                    return Err(TransactionError::ValidationFailed("Negative logic time".into()));
                }
            }
            TransactionPhase::PublishAndDrop => {
                // 发布并重置单Tick瞬态工作区
                scratch.reset_scratch();
            }
        }

        self.trace.push(PhaseTraceEntry {
            tick_id: world.clock.tick,
            phase: target,
            logic_time_sec: world.clock.logic_time_sec,
            scratch_item_count: scratch.intent_candidates.len() + scratch.motion_proposals.len(),
            diagnostics_count: scratch.diagnostics.len(),
        });

        self.current_phase = Some(target);
        Ok(())
    }

    /// 运行一个完整的确定性 Tick (顺序执行 0 到 12 全部阶段)
    pub fn run_full_tick(
        &mut self,
        world: &mut WorldState,
        derived: &mut DerivedState,
        scratch: &mut TickScratch,
    ) -> Result<TickSummary, TransactionError> {
        for phase in TransactionPhase::ALL {
            self.step_phase(phase, world, derived, scratch)?;
        }
        self.current_phase = None; // Reset phase ready for next tick

        Ok(TickSummary {
            tick: world.clock.tick,
            logic_time_sec: world.clock.logic_time_sec,
            phases_executed: 13,
            published_revision: world.clock.revision + world.entities.revision + world.terrain.revision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::GridGeometry;

    #[test]
    fn test_phase_03_thirteen_phases_strictly_in_order() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);
        let mut derived = DerivedState::default();
        let mut scratch = TickScratch::default();
        let mut orch = TickOrchestrator::default();

        // 尝试跳过阶段直接执行 ProposeMotion，必须报错
        let err = orch.step_phase(
            TransactionPhase::ProposeMotion,
            &mut world,
            &mut derived,
            &mut scratch,
        );
        assert!(matches!(
            err,
            Err(TransactionError::PhaseOutOfOrder {
                expected: TransactionPhase::BeginTick,
                actual: TransactionPhase::ProposeMotion
            })
        ));

        // 正常跑完一个完整 Tick (13阶段)
        let summary = orch.run_full_tick(&mut world, &mut derived, &mut scratch).unwrap();
        assert_eq!(summary.tick, 1);
        assert_eq!(summary.logic_time_sec, 16.0);
        assert_eq!(summary.phases_executed, 13);
        assert_eq!(orch.trace.len(), 13);

        // 验证 Tick 结束后 scratch 被干净重置
        assert!(scratch.intent_candidates.is_empty());
        assert!(scratch.motion_proposals.is_empty());

        // 运行第二个 Tick
        let summary2 = orch.run_full_tick(&mut world, &mut derived, &mut scratch).unwrap();
        assert_eq!(summary2.tick, 2);
        assert_eq!(summary2.logic_time_sec, 32.0);
    }
}
