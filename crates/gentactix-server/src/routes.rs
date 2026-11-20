use crate::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use gentactix_core::commands::TacticalCommand;
use gentactix_core::engine::PureEngine;
use gentactix_core::math_ops::{WorldPos, TACTICAL_CELL_SIZE_M};
use gentactix_core::ruleset::{ScenarioSpec, SimRuleset};
use gentactix_core::types::{EngineerWorkKind, MoveStance, Team, UnitId, TICK_SECONDS};
use serde::Deserialize;
use serde_json::{json, Value};

type ApiErr = (StatusCode, Json<Value>);
type ApiRes = Result<Json<Value>, ApiErr>;

fn err_resp(status: StatusCode, msg: impl Into<String>) -> ApiErr {
    (status, Json(json!({ "ok": false, "error": msg.into() })))
}
#[derive(Deserialize)]
struct TeamQuery {
    team: Option<Team>,
}
#[derive(Deserialize)]
struct CreateWorldReq {
    rows: usize,
    cols: usize,
    #[serde(default)]
    base_elevation_m: f32,
    #[serde(default)]
    ruleset: Option<SimRuleset>,
}
#[derive(Deserialize)]
struct PlanQueryReq {
    unit_id: u32,
    goal: WorldPos,
    #[serde(default)]
    stance: MoveStance,
}
#[derive(Deserialize)]
struct LosQueryReq {
    from: WorldPos,
    to: WorldPos,
    #[serde(default = "default_vision_m")]
    max_vision_m: f32,
}
#[derive(Deserialize)]
struct ArcQueryReq {
    unit_id: u32,
    center_angle_deg: f32,
    half_arc_deg: f32,
    #[serde(default = "default_ray_count")]
    ray_count: usize,
    #[serde(default = "default_vision_m")]
    max_dist_m: f32,
}
#[derive(Deserialize)]
struct WorkQueryReq {
    unit_id: u32,
    work: EngineerWorkKind,
    control_points: Vec<WorldPos>,
}
#[derive(Deserialize)]
struct StepReq {
    #[serde(default = "default_ticks")]
    ticks: u32,
    #[serde(default)]
    observer_team: Option<Team>,
}
const fn default_vision_m() -> f32 { 600.0 }
const fn default_ray_count() -> usize { 16 }
const fn default_ticks() -> u32 { 1 }

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/world/create", post(world_create))
        .route("/api/world/scenario", post(world_scenario))
        .route("/api/world/save", get(world_save))
        .route("/api/world/load", post(world_load))
        .route("/api/ruleset", get(get_ruleset).put(put_ruleset))
        .route("/api/schema", get(get_schema))
        .route("/api/catalog", get(get_catalog))
        .route("/api/command", post(post_command))
        .route("/api/state", get(get_state))
        .route("/api/units/:id", get(get_unit_by_id))
        .route("/api/terrain.bin", get(get_terrain_bin))
        .route("/api/fields/:kind", get(get_field))
        .route("/api/query/plan", post(query_plan))
        .route("/api/query/los", post(query_los))
        .route("/api/query/arc", post(query_arc))
        .route("/api/query/work", post(query_work))
        .route("/api/clock/step", post(clock_step))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": "2.0.0", "cell_m": TACTICAL_CELL_SIZE_M, "tick_s": TICK_SECONDS }))
}
async fn world_create(State(st): State<AppState>, Json(req): Json<CreateWorldReq>) -> ApiRes {
    if req.rows == 0 || req.cols == 0 || req.rows > 4096 || req.cols > 4096 {
        return Err(err_resp(StatusCode::BAD_REQUEST, "rows/cols must be in 1..=4096"));
    }
    let eng = PureEngine::new_blank(req.rows, req.cols, req.base_elevation_m, req.ruleset.unwrap_or_default())
        .map_err(|e| err_resp(StatusCode::BAD_REQUEST, e))?;
    let sha = eng.world.authoritative_sha256();
    *st.engine.write().await = eng;
    Ok(Json(json!({ "ok": true, "rows": req.rows, "cols": req.cols, "cell_m": TACTICAL_CELL_SIZE_M, "authoritative_sha256": sha })))
}
async fn world_scenario(State(st): State<AppState>, Json(spec): Json<ScenarioSpec>) -> ApiRes {
    let eng = PureEngine::from_scenario(&spec).map_err(|e| err_resp(StatusCode::BAD_REQUEST, e))?;
    let sha = eng.world.authoritative_sha256();
    let unit_count = eng.world.units.len();
    let platoon_count = eng.world.platoons.len();
    *st.engine.write().await = eng;
    Ok(Json(json!({ "ok": true, "name": spec.name, "units": unit_count, "platoons": platoon_count, "authoritative_sha256": sha })))
}
async fn world_save(State(st): State<AppState>) -> Result<impl IntoResponse, ApiErr> {
    let saved = st.engine.read().await.save_json().map_err(|e| err_resp(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(([(header::CONTENT_TYPE, "application/json")], saved))
}
async fn world_load(State(st): State<AppState>, body: String) -> ApiRes {
    let mut g = st.engine.write().await;
    g.load_json(&body).map_err(|e| err_resp(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true, "tick": g.world.tick, "authoritative_sha256": g.world.authoritative_sha256() })))
}
async fn get_ruleset(State(st): State<AppState>) -> Json<SimRuleset> {
    Json(st.engine.read().await.world.ruleset.clone())
}
async fn put_ruleset(State(st): State<AppState>, Json(ruleset): Json<SimRuleset>) -> Json<Value> {
    let mut g = st.engine.write().await;
    let out = g.submit_command(TacticalCommand::SetRuleset { ruleset }, 1);
    Json(json!(out))
}
async fn get_schema() -> Json<Value> {
    Json(json!({
        "engine": "gentactix-v2-pure-platoon",
        "version": "2.0.0",
        "cell_m": TACTICAL_CELL_SIZE_M,
        "tick_s": TICK_SECONDS,
        "command_kinds": [
            "move_unit", "attack_pos", "attack_unit", "hold_unit", "set_fire_discipline",
            "pop_smoke", "build_work", "arm_detonator", "trigger_detonator",
            "spawn_platoon", "spawn_unit", "despawn_unit", "edit_terrain", "set_ruleset", "set_night",
            "schedule_timed_charge", "cancel_step_event", "issue_commander_wave"
        ],
        "ruleset_presets": {
            "hardcore_platoon": SimRuleset::hardcore_platoon(),
            "tactical_standard": SimRuleset::tactical_standard(),
            "arcade_sandbox": SimRuleset::arcade_sandbox(),
        },
        "endpoints": [
            "GET /health", "POST /api/world/create", "POST /api/world/scenario",
            "GET /api/world/save", "POST /api/world/load", "GET/PUT /api/ruleset",
            "GET /api/schema", "GET /api/catalog", "POST /api/command",
            "GET /api/state", "GET /api/units/:id", "GET /api/terrain.bin",
            "GET /api/fields/:kind", "POST /api/query/plan", "POST /api/query/los",
            "POST /api/query/arc", "POST /api/query/work", "POST /api/clock/step"
        ]
    }))
}
async fn get_catalog(State(st): State<AppState>) -> Json<Value> {
    let g = st.engine.read().await;
    Json(json!({ "version": g.catalog.version, "schema": g.catalog.schema, "count": g.catalog.len(), "items": g.catalog.to_raw_items() }))
}
async fn post_command(State(st): State<AppState>, Json(mut payload): Json<Value>) -> ApiRes {
    let sec = payload.as_object_mut().and_then(|m| m.remove("cmd_sec_in_tick")).and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    let cmd: TacticalCommand = serde_json::from_value(payload).map_err(|e| err_resp(StatusCode::BAD_REQUEST, format!("invalid TacticalCommand: {e}")))?;
    let mut g = st.engine.write().await;
    Ok(Json(json!(g.submit_command(cmd, sec))))
}
async fn get_state(State(st): State<AppState>, Query(q): Query<TeamQuery>) -> Json<Value> {
    let obs = st.engine.read().await.observe(q.team.unwrap_or(Team::Red));
    Json(json!(obs))
}
async fn get_unit_by_id(State(st): State<AppState>, AxumPath(id): AxumPath<u32>, Query(q): Query<TeamQuery>) -> ApiRes {
    st.engine.read().await.observe_unit_detail(q.team.unwrap_or(Team::Red), UnitId(id))
        .map(|u| Json(json!(u)))
        .ok_or_else(|| err_resp(StatusCode::NOT_FOUND, format!("unit {id} not visible or missing")))
}
async fn get_terrain_bin(State(st): State<AppState>) -> impl IntoResponse {
    let bytes = st.engine.read().await.world.terrain.to_binary_stream();
    ([(header::CONTENT_TYPE, "application/octet-stream")], bytes)
}
async fn get_field(State(st): State<AppState>, AxumPath(kind): AxumPath<String>, Query(q): Query<TeamQuery>) -> ApiRes {
    let g = st.engine.read().await;
    let (rows, cols) = (g.world.terrain.rows, g.world.terrain.cols);
    let obs = q.team.unwrap_or(Team::Red);
    let ti = obs.index().unwrap_or(0);
    match kind.as_str() {
        "i" => Ok(Json(json!({
            "kind": "i", "observer_team": obs, "rows": rows, "cols": cols, "cell_m": TACTICAL_CELL_SIZE_M,
            "values": g.derived.team_i_field_threat[ti],
            "i_intel": g.world.team_i_intel[ti],
            "i_vis_tier": g.world.team_i_vis_tier[ti],
            "h_blast": g.world.cell_h_blast,
            "i_threat": g.derived.team_i_field_threat[ti],
        }))),
        "g" => Ok(Json(json!({ "kind": "g", "rows": rows, "cols": cols, "cell_m": TACTICAL_CELL_SIZE_M, "cells": g.derived.g_field_cells }))),
        _ => Err(err_resp(StatusCode::BAD_REQUEST, "field kind must be 'i' or 'g'")),
    }
}
async fn query_plan(State(st): State<AppState>, Json(req): Json<PlanQueryReq>) -> ApiRes {
    st.engine.read().await.preview_move(UnitId(req.unit_id), req.goal, req.stance)
        .map(|r| Json(json!(r))).map_err(|e| err_resp(StatusCode::BAD_REQUEST, e))
}
async fn query_los(State(st): State<AppState>, Json(req): Json<LosQueryReq>) -> Json<Value> {
    Json(json!(st.engine.read().await.preview_los(req.from, req.to, req.max_vision_m)))
}
async fn query_arc(State(st): State<AppState>, Json(req): Json<ArcQueryReq>) -> ApiRes {
    st.engine.read().await.preview_fire_arc(UnitId(req.unit_id), req.center_angle_deg, req.half_arc_deg, req.ray_count, req.max_dist_m)
        .map(|r| Json(json!(r))).map_err(|e| err_resp(StatusCode::BAD_REQUEST, e))
}
async fn query_work(State(st): State<AppState>, Json(req): Json<WorkQueryReq>) -> ApiRes {
    st.engine.read().await.preview_engineer_work(UnitId(req.unit_id), req.work, &req.control_points)
        .map(|r| Json(json!(r))).map_err(|e| err_resp(StatusCode::BAD_REQUEST, e))
}
async fn clock_step(State(st): State<AppState>, Json(req): Json<StepReq>) -> Json<Value> {
    let mut g = st.engine.write().await;
    let steps = g.step_n_ticks(req.ticks.clamp(1, 64), req.observer_team.unwrap_or(Team::Red));
    Json(json!({ "tick": g.world.tick, "clock_seconds": g.world.clock_seconds, "steps": steps }))
}
