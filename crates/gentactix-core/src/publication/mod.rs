//! `publication` module: Complete Deterministic Authority Fingerprint & Save/Replay
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 第 10 节与 plan.txt Phase 08 要求：
//! 1. 完整权威指纹覆盖 WorldState 全量权威数据
//! 2. 派生缓存重建绝不改变权威指纹
//! 3. 序列化存档 -> 反序列化加载 -> 权威指纹逐位一致

use crate::state::WorldState;
use serde_json;
use sha2::{Digest, Sha256};

/// 计算全量权威世界状态的确定性 SHA-256 指纹
pub fn compute_authority_fingerprint(world: &WorldState) -> String {
    let mut hasher = Sha256::new();

    // 1. 时钟
    hasher.update(world.clock.tick.to_le_bytes());
    hasher.update(world.clock.logic_time_sec.to_bits().to_le_bytes());
    hasher.update(world.clock.revision.to_le_bytes());

    // 2. 地形修订与关键几何
    hasher.update(world.geometry.rows.to_le_bytes());
    hasher.update(world.geometry.cols.to_le_bytes());
    hasher.update(world.terrain.revision.to_le_bytes());
    // 采样高程哈希
    for elev in &world.terrain.elevation_m {
        hasher.update(elev.to_bits().to_le_bytes());
    }

    // 3. 实体权威状态 (按 ID 稳定升序排序)
    let mut sorted_entity_ids: Vec<u32> = world.entities.entities.keys().copied().collect();
    sorted_entity_ids.sort_unstable();

    for id in sorted_entity_ids {
        let e = &world.entities.entities[&id];
        hasher.update(e.id.to_le_bytes());
        hasher.update(e.pos.x.to_bits().to_le_bytes());
        hasher.update(e.pos.y.to_bits().to_le_bytes());
        hasher.update(e.facing_deg.to_bits().to_le_bytes());
        hasher.update([if e.alive { 1u8 } else { 0u8 }]);
        hasher.update(e.persons.to_le_bytes());
        hasher.update(e.org.to_bits().to_le_bytes());
    }

    // 4. 库存账本 (按 entity_id 稳定升序排序)
    let mut sorted_inv_ids: Vec<u32> = world.inventory.entity_stocks.keys().copied().collect();
    sorted_inv_ids.sort_unstable();

    for id in sorted_inv_ids {
        hasher.update(id.to_le_bytes());
        let stock_map = &world.inventory.entity_stocks[&id];
        let mut sorted_items: Vec<&String> = stock_map.keys().collect();
        sorted_items.sort();
        for item in sorted_items {
            hasher.update(item.as_bytes());
            hasher.update(stock_map[item].to_le_bytes());
        }
    }

    // 5. 持久环境场 (按 cell_idx 排序)
    let mut sorted_smoke_cells: Vec<usize> = world.persistent_fields.smoke.keys().copied().collect();
    sorted_smoke_cells.sort_unstable();

    for idx in sorted_smoke_cells {
        hasher.update((idx as u64).to_le_bytes());
        let (dens, ttl) = world.persistent_fields.smoke[&idx];
        hasher.update(dens.to_bits().to_le_bytes());
        hasher.update(ttl.to_bits().to_le_bytes());
    }

    // 6. 计划事件
    hasher.update((world.scheduled_events.tasks.len() as u64).to_le_bytes());
    for t in &world.scheduled_events.tasks {
        hasher.update(t.task_id.to_le_bytes());
        hasher.update(t.entity_id.to_le_bytes());
        hasher.update(t.due_logic_sec.to_bits().to_le_bytes());
    }

    // 7. 随机状态
    hasher.update(world.random.seed.to_le_bytes());
    hasher.update(world.random.counter.to_le_bytes());

    format!("{:x}", hasher.finalize())
}

/// 存档导出为 JSON 字符串
pub fn save_world_state_to_json(world: &WorldState) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(world)
}

/// 从 JSON 字符串加载世界存档
pub fn load_world_state_from_json(json: &str) -> Result<WorldState, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{GridGeometry, WorldPos};
    use crate::state::{DerivedState, EntityAuthorityRecord};
    use rustc_hash::FxHashMap;

    #[test]
    fn test_phase_08_fingerprint_determinism_and_save_load_fidelity() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        let mut world = WorldState::new(geom, 0, 100.0);
        let mut derived = DerivedState::default();

        // 插入实体与库存
        world.entities.insert(EntityAuthorityRecord {
            id: 1,
            str_id: "squad-1".into(),
            team_id: 1,
            pos: WorldPos::new(100.0, 100.0),
            facing_deg: 45.0,
            alive: true,
            persons: 10,
            org: 95.0,
        });

        let mut inv = FxHashMap::default();
        inv.insert("rifle".into(), 10);
        world.inventory.entity_stocks.insert(1, inv);

        // 1. 同状态重复计算指纹完全相同
        let fp1 = compute_authority_fingerprint(&world);
        let fp2 = compute_authority_fingerprint(&world);
        assert_eq!(fp1, fp2);

        // 2. 派生缓存初始化与重建绝不改变权威指纹
        derived.rebuild_sdf_if_needed(&world);
        let fp_after_derived = compute_authority_fingerprint(&world);
        assert_eq!(fp1, fp_after_derived, "Rebuilding derived caches must NOT alter authority fingerprint!");

        // 3. 修改任意权威状态 (如实体轻微位移)，指纹必须改变
        world.entities.commit_position(1, WorldPos::new(100.5, 100.0));
        let fp_modified = compute_authority_fingerprint(&world);
        assert_ne!(fp1, fp_modified, "Fingerprint must change when an entity position moves!");

        // 4. 存档序列化 -> 加载 -> 指纹一致
        let json = save_world_state_to_json(&world).unwrap();
        let loaded_world = load_world_state_from_json(&json).unwrap();
        let fp_loaded = compute_authority_fingerprint(&loaded_world);
        assert_eq!(fp_modified, fp_loaded, "Saved and loaded world must produce identical authority fingerprint!");
    }
}
