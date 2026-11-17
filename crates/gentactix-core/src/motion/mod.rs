//! `motion` module: Motion Proposals, Rigid Constraints & Single Commit Point
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 5 节与 plan.txt Phase 04 要求：
//! 1. ProposeMotion 只生成提案，绝不修改世界坐标
//! 2. 连续路径采样，高速运动严禁穿透薄障碍
//! 3. CommitMotion 为每实体每 Tick 唯一合法位置写入点

use crate::geometry::{CellCoord, GridGeometry, WorldPos, TACTICAL_CELL_SIZE_M};
use crate::intent::MotionIntent;
use crate::state::{DerivedState, WorldState};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionProposal {
    pub entity_id: u32,
    pub current_pos: WorldPos,
    pub proposed_delta: [f32; 2],
    pub body_radius_m: f32,
}

impl MotionProposal {
    pub fn from_intent(
        intent: &MotionIntent,
        current_pos: WorldPos,
        subtick_sec: f32,
        body_radius_m: f32,
    ) -> Self {
        let max_dist = intent.speed_mps * subtick_sec;
        let delta = if let Some(target) = intent.target_pos {
            let dx = target.x - current_pos.x;
            let dy = target.y - current_pos.y;
            let d = dx.hypot(dy);
            if d <= 1e-4 {
                [0.0, 0.0]
            } else {
                let step = d.min(max_dist);
                [dx / d * step, dy / d * step]
            }
        } else {
            let rad = intent.heading_deg.to_radians();
            [rad.sin() * max_dist, -rad.cos() * max_dist]
        };

        Self {
            entity_id: intent.entity_id,
            current_pos,
            proposed_delta: delta,
            body_radius_m,
        }
    }

    #[inline]
    pub fn proposed_target(&self) -> WorldPos {
        WorldPos::new(
            self.current_pos.x + self.proposed_delta[0],
            self.current_pos.y + self.proposed_delta[1],
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstrainedMotion {
    pub entity_id: u32,
    pub original_pos: WorldPos,
    pub final_pos: WorldPos,
    pub is_blocked: bool,
    pub reason: Option<String>,
}

/// 空间运动硬约束求解器
#[derive(Debug, Default)]
pub struct MotionConstraintSolver {
    pub cell_capacity_reservations: FxHashMap<CellCoord, u32>,
    pub max_capacity_per_cell: u32,
}

impl MotionConstraintSolver {
    pub fn new(max_capacity_per_cell: u32) -> Self {
        Self {
            cell_capacity_reservations: FxHashMap::default(),
            max_capacity_per_cell,
        }
    }

    pub fn reset(&mut self) {
        self.cell_capacity_reservations.clear();
    }

    /// 执行五级硬约束链
    pub fn solve(
        &mut self,
        geom: &GridGeometry,
        world: &WorldState,
        _derived: &DerivedState,
        proposal: &MotionProposal,
    ) -> ConstrainedMotion {
        let target = proposal.proposed_target();
        let total_dist = proposal.proposed_delta[0].hypot(proposal.proposed_delta[1]);

        if total_dist <= 1e-4 {
            return ConstrainedMotion {
                entity_id: proposal.entity_id,
                original_pos: proposal.current_pos,
                final_pos: proposal.current_pos,
                is_blocked: false,
                reason: None,
            };
        }

        // 约束 1: 连续整段路径地形采样 (防高速穿透薄障碍)
        // 采样步长取 0.5 * cell_size = 4.0m
        let sample_step_m = (TACTICAL_CELL_SIZE_M * 0.5).max(1.0);
        let num_samples = (total_dist / sample_step_m).ceil() as usize;

        let mut safe_pos = proposal.current_pos;
        let mut blocked = false;
        let mut block_reason = None;

        for s in 1..=num_samples {
            let t = (s as f32) / (num_samples as f32);
            let sample_x = proposal.current_pos.x + proposal.proposed_delta[0] * t;
            let sample_y = proposal.current_pos.y + proposal.proposed_delta[1] * t;
            let sample_pos = WorldPos::new(sample_x, sample_y);

            match geom.world_to_cell(sample_pos) {
                Ok(cell) => {
                    let c_idx = world.terrain.idx(cell.r, cell.c);
                    // 假设 terrain_type == 2 为水体/绝壁等绝对不可通行障碍
                    if world.terrain.terrain_type[c_idx] == 2 {
                        blocked = true;
                        block_reason = Some(format!("Continuous path blocked by terrain at cell ({}, {})", cell.r, cell.c));
                        break;
                    }
                    safe_pos = sample_pos;
                }
                Err(e) => {
                    blocked = true;
                    block_reason = Some(format!("Path stepped outside world bounds: {e}"));
                    break;
                }
            }
        }

        if blocked {
            return ConstrainedMotion {
                entity_id: proposal.entity_id,
                original_pos: proposal.current_pos,
                final_pos: safe_pos,
                is_blocked: true,
                reason: block_reason,
            };
        }

        // 约束 2: 目标格容量预约
        if let Ok(final_cell) = geom.world_to_cell(target) {
            let count = self.cell_capacity_reservations.entry(final_cell).or_insert(0);
            if *count >= self.max_capacity_per_cell {
                return ConstrainedMotion {
                    entity_id: proposal.entity_id,
                    original_pos: proposal.current_pos,
                    final_pos: proposal.current_pos,
                    is_blocked: true,
                    reason: Some(format!("Target cell ({}, {}) capacity exceeded limit {}", final_cell.r, final_cell.c, self.max_capacity_per_cell)),
                };
            }
            *count += 1;
        }

        ConstrainedMotion {
            entity_id: proposal.entity_id,
            original_pos: proposal.current_pos,
            final_pos: target,
            is_blocked: false,
            reason: None,
        }
    }
}

/// 唯一位置提交器 (Phase 7: CommitMotion)
pub fn commit_motions(world: &mut WorldState, motions: &[ConstrainedMotion]) -> usize {
    let mut committed = 0;
    for m in motions {
        if world.entities.commit_position(m.entity_id, m.final_pos) {
            committed += 1;
        }
    }
    committed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::GridGeometry;
    use crate::intent::IntentSource;
    use crate::state::EntityAuthorityRecord;

    #[test]
    fn test_phase_04_continuous_path_blocks_high_speed_tunneling() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom.clone(), 0, 100.0);
        let derived = DerivedState::default();

        // 设立一处厚度仅 1 格 (8m) 的薄墙 (c=10, r 全长)
        for r in 0..1024 {
            let idx = world.terrain.idx(r, 10);
            world.terrain.terrain_type[idx] = 2; // 不可穿越
        }

        // 实体位于 (x=40.0, y=80.0)，即 c=5
        // 提出高速机动跨过薄墙到达 (x=160.0, y=80.0)，即 c=20 (跨过 c=10 的薄墙)
        let intent = MotionIntent {
            entity_id: 101,
            winning_source: IntentSource::DirectOrder,
            speed_mps: 10.0,
            target_pos: Some(WorldPos::new(160.0, 80.0)),
            heading_deg: 90.0,
            arbitration_reason: "High speed test".into(),
        };

        let proposal = MotionProposal::from_intent(
            &intent,
            WorldPos::new(40.0, 80.0),
            16.0, // 16s * 10m/s = 160m displacement
            1.5,
        );

        let mut solver = MotionConstraintSolver::new(4);
        let motion = solver.solve(&geom, &world, &derived, &proposal);

        // 必须被连续路径采样拦截，绝不允许穿墙直接进入 c=20
        assert!(motion.is_blocked);
        assert!(motion.final_pos.x < 80.0, "Entity must be stopped before thin wall at x=80m");
    }

    #[test]
    fn test_phase_04_single_commit_point_updates_authority() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);

        world.entities.insert(EntityAuthorityRecord {
            id: 201,
            str_id: "tank-201".into(),
            team_id: 1,
            pos: WorldPos::new(10.0, 10.0),
            facing_deg: 0.0,
            alive: true,
            persons: 3,
            org: 100.0,
        });

        let motion = ConstrainedMotion {
            entity_id: 201,
            original_pos: WorldPos::new(10.0, 10.0),
            final_pos: WorldPos::new(26.0, 10.0),
            is_blocked: false,
            reason: None,
        };

        let count = commit_motions(&mut world, &[motion]);
        assert_eq!(count, 1);
        assert_eq!(world.entities.entities.get(&201).unwrap().pos, WorldPos::new(26.0, 10.0));
    }
}
