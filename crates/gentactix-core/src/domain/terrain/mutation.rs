//! `operators` module: Terrain Mutation Transactions & Cascading Cache Invalidation
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 8 节与 plan.txt Phase 07 要求：
//! 1. 地形修改统一经过 TerrainMutation -> revision += 1 -> 标记 DirtyHalo -> 级联失效
//! 2. 杜绝旧 SDF / 威胁场在地形变异后被继续误判为有效

use crate::geometry::CellCoord;
use crate::state::{DerivedState, StateKind, WorldState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainMutation {
    pub center: CellCoord,
    pub radius_cells: i32,
    pub elev_delta: f32,
    pub new_terrain_type: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyHaloBox {
    pub min_r: i32,
    pub min_c: i32,
    pub max_r: i32,
    pub max_c: i32,
}

impl DirtyHaloBox {
    #[inline]
    pub fn contains(&self, r: i32, c: i32) -> bool {
        r >= self.min_r && r <= self.max_r && c >= self.min_c && c <= self.max_c
    }
}

pub struct TerrainTransaction;

impl TerrainTransaction {
    /// 执行地形修改事务：更新地形、递增修订号、计算 Halo 脏区并级联失效派生缓存
    pub fn apply(
        world: &mut WorldState,
        derived: &mut DerivedState,
        mutation: TerrainMutation,
        halo_cells: i32,
    ) -> (u64, DirtyHaloBox) {
        let r0 = mutation.center.r;
        let c0 = mutation.center.c;
        let rad = mutation.radius_cells;

        let mut min_r = r0 - rad;
        let mut max_r = r0 + rad;
        let mut min_c = c0 - rad;
        let mut max_c = c0 + rad;

        for dr in -rad..=rad {
            for dc in -rad..=rad {
                if dr * dr + dc * dc <= rad * rad {
                    let r = r0 + dr;
                    let c = c0 + dc;
                    if world.geometry.in_bounds(r, c) {
                        let idx = world.terrain.idx(r, c);
                        let cur_elev = world.terrain.elevation_m[idx];
                        world.terrain.mutate_cell(
                            r,
                            c,
                            cur_elev + mutation.elev_delta,
                            mutation.new_terrain_type,
                        );
                    }
                }
            }
        }

        // 计算带 Halo 的脏区边界
        min_r = (min_r - halo_cells).max(0);
        min_c = (min_c - halo_cells).max(0);
        max_r = (max_r + halo_cells).min(world.geometry.rows as i32 - 1);
        max_c = (max_c + halo_cells).min(world.geometry.cols as i32 - 1);

        let dirty_box = DirtyHaloBox {
            min_r,
            min_c,
            max_r,
            max_c,
        };

        // 级联失效依赖于地形的派生缓存 (SDF、威胁场等)
        derived.invalidate_by_mutation(StateKind::Terrain);

        (world.terrain.revision, dirty_box)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::GridGeometry;

    #[test]
    fn test_phase_07_terrain_mutation_invalidates_derived_caches() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);
        let mut derived = DerivedState::default();

        // 1. 初始化派生缓存
        derived.rebuild_sdf_if_needed(&world);
        assert!(derived.sdf.meta.as_ref().unwrap().is_up_to_date(&world));

        // 2. 触发一次地形破坏事务 (弹坑下陷 2.5m, 半径 2 格, halo 1 格)
        let initial_rev = world.terrain.revision;
        let (new_rev, dirty_box) = TerrainTransaction::apply(
            &mut world,
            &mut derived,
            TerrainMutation {
                center: CellCoord::new(20, 20),
                radius_cells: 2,
                elev_delta: -2.5,
                new_terrain_type: None,
            },
            1, // halo
        );

        assert!(new_rev > initial_rev, "Revision must increment on mutation");
        assert_eq!(dirty_box, DirtyHaloBox { min_r: 17, min_c: 17, max_r: 23, max_c: 23 });

        // 3. 验证此时 SDF 缓存必须立即可感知失效 (stale)
        assert!(!derived.sdf.meta.as_ref().unwrap().is_up_to_date(&world), "SDF cache must be invalid after terrain mutation");

        // 4. 重建后再次有效并绑定新版本号
        derived.rebuild_sdf_if_needed(&world);
        assert!(derived.sdf.meta.as_ref().unwrap().is_up_to_date(&world));
        assert_eq!(derived.sdf.meta.as_ref().unwrap().dependencies[0].1, new_rev);
    }
}
