use crate::clock::{check_los, find_path_world, generate_work_points, LosResult};
use crate::math_ops::{solve_boustrophedon_2opt, WorldPos, TACTICAL_CELL_SIZE_M};
use crate::state::WorldState;
use crate::types::{EngineerWorkKind, MoveStance, UnitId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewPathResult {
    pub reachable: bool,
    pub waypoints: Vec<WorldPos>,
    pub total_distance_m: f32,
    pub estimated_seconds: f32,
    pub estimated_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LosRaySample {
    pub angle_deg: f32,
    pub end_pos: WorldPos,
    pub visible_dist_m: f32,
    pub blocked_by_terrain: bool,
    pub blocked_by_smoke: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewLosArcResult {
    pub origin: WorldPos,
    pub max_range_m: f32,
    pub samples: Vec<LosRaySample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewLosLineResult {
    pub from: WorldPos,
    pub to: WorldPos,
    pub visible: bool,
    pub dist_m: f32,
    pub concealment: f32,
    pub blocked_by_terrain: bool,
    pub blocked_by_smoke: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewWorkResult {
    pub work: EngineerWorkKind,
    pub points: Vec<WorldPos>,
    pub estimated_work_person_sec: f32,
    pub estimated_ticks: u32,
}

/// Strictly read-only path & ETA preview (zero WorldState mutation).
pub fn preview_move_path(
    world: &WorldState,
    unit_id: UnitId,
    goal: WorldPos,
    stance: MoveStance,
) -> Result<PreviewPathResult, String> {
    let unit = world
        .units
        .get(&unit_id.0)
        .ok_or_else(|| format!("unknown unit {}", unit_id.0))?;
    if !unit.alive {
        return Err(format!("unit {} is destroyed", unit_id.0));
    }
    world
        .terrain
        .geom
        .world_to_cell(goal)
        .map_err(|e| format!("goal out of bounds: {e}"))?;

    let speed = (unit.base_speed_mps * stance.speed_mult()).max(0.1);
    match find_path_world(&world.terrain, unit.mobility, unit.pos, goal, 16_000) {
        Some(waypoints) => {
            let mut total_distance_m = 0.0f32;
            let mut prev = unit.pos;
            for &wp in &waypoints {
                total_distance_m += prev.distance_to(wp);
                prev = wp;
            }
            let estimated_seconds = total_distance_m / speed;
            let estimated_ticks = (estimated_seconds / 16.0).ceil() as u32;
            Ok(PreviewPathResult {
                reachable: true,
                waypoints,
                total_distance_m,
                estimated_seconds,
                estimated_ticks,
            })
        }
        None => Ok(PreviewPathResult {
            reachable: false,
            waypoints: Vec::new(),
            total_distance_m: 0.0,
            estimated_seconds: 0.0,
            estimated_ticks: 0,
        }),
    }
}

/// Strictly read-only DDA LOS fan arc preview (zero WorldState mutation).
pub fn preview_fire_arc(
    world: &WorldState,
    unit_id: UnitId,
    center_angle_deg: f32,
    half_arc_deg: f32,
    ray_count: usize,
    max_dist_m: f32,
) -> Result<PreviewLosArcResult, String> {
    let unit = world
        .units
        .get(&unit_id.0)
        .ok_or_else(|| format!("unknown unit {}", unit_id.0))?;
    let origin = unit.pos;
    let effective_limit = max_dist_m.max(TACTICAL_CELL_SIZE_M).min(unit.vision_m);
    let n = ray_count.clamp(1, 72);
    let half = half_arc_deg.clamp(0.0, 180.0);
    let step_m = TACTICAL_CELL_SIZE_M;

    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let angle_deg = if n == 1 {
            center_angle_deg
        } else {
            center_angle_deg - half + (2.0 * half) * (i as f32 / (n - 1) as f32)
        };
        let rad = angle_deg.to_radians();
        let (dx, dy) = (rad.cos(), rad.sin());

        let mut end_pos = origin;
        let mut visible_dist_m = 0.0f32;
        let mut blocked_by_terrain = false;
        let mut blocked_by_smoke = false;

        let max_steps = (effective_limit / step_m).ceil() as usize;
        for s in 1..=max_steps {
            let d = (s as f32 * step_m).min(effective_limit);
            let probe = WorldPos::new(origin.x + dx * d, origin.y + dy * d);
            if world.terrain.geom.world_to_cell(probe).is_err() {
                break;
            }
            let los = check_los(&world.terrain, origin, probe, effective_limit, world.night);
            if los.visible {
                end_pos = probe;
                visible_dist_m = los.dist_m;
            } else {
                blocked_by_terrain = los.blocked_by_terrain;
                blocked_by_smoke = los.blocked_by_smoke;
                break;
            }
        }

        samples.push(LosRaySample {
            angle_deg,
            end_pos,
            visible_dist_m,
            blocked_by_terrain,
            blocked_by_smoke,
        });
    }

    Ok(PreviewLosArcResult {
        origin,
        max_range_m: effective_limit,
        samples,
    })
}

/// Strictly read-only point-to-point LOS preview.
pub fn preview_los_between(
    world: &WorldState,
    from: WorldPos,
    to: WorldPos,
    max_vision_m: f32,
) -> PreviewLosLineResult {
    let los: LosResult = check_los(&world.terrain, from, to, max_vision_m, world.night);
    PreviewLosLineResult {
        from,
        to,
        visible: los.visible,
        dist_m: los.dist_m,
        concealment: los.concealment,
        blocked_by_terrain: los.blocked_by_terrain,
        blocked_by_smoke: los.blocked_by_smoke,
    }
}

/// Strictly read-only polygon / polyline engineer work layout & duration preview.
pub fn preview_engineer_work(
    world: &WorldState,
    unit_id: UnitId,
    work: EngineerWorkKind,
    control_points: &[WorldPos],
) -> Result<PreviewWorkResult, String> {
    let unit = world
        .units
        .get(&unit_id.0)
        .ok_or_else(|| format!("unknown unit {}", unit_id.0))?;
    if control_points.is_empty() {
        return Err("control_points cannot be empty".into());
    }
    for &cp in control_points {
        world
            .terrain
            .geom
            .world_to_cell(cp)
            .map_err(|e| format!("control point out of bounds: {e}"))?;
    }

    let mut points = generate_work_points(work, control_points);
    if work == EngineerWorkKind::Minefield && points.len() > 2 {
        let raw: Vec<[f32; 2]> = points.iter().map(|p| [p.x, p.y]).collect();
        points = solve_boustrophedon_2opt(&raw)
            .into_iter()
            .map(|p| WorldPos::new(p[0], p[1]))
            .collect();
    }

    let mut path_m = 0.0f32;
    let mut prev = unit.pos;
    for &pt in &points {
        path_m += prev.distance_to(pt);
        prev = pt;
    }
    let estimated_work_person_sec = (points.len() as f32) * 40.0;
    let work_time_s = estimated_work_person_sec / (unit.men.max(1) as f32);
    let move_time_s = path_m / unit.base_speed_mps.max(0.1);
    let estimated_ticks = ((work_time_s + move_time_s) / 16.0).ceil() as u32;

    Ok(PreviewWorkResult {
        work,
        points,
        estimated_work_person_sec,
        estimated_ticks,
    })
}
