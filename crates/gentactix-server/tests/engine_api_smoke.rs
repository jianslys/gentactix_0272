use gentactix_core::ruleset::{ScenarioSpec, SimRuleset, TerrainSourceSpec};
use gentactix_core::terrain::BINARY_TERRAIN_MAGIC;
use gentactix_server::{build_router, AppState};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcpStream};
use tokio::net::TcpListener;

async fn http_req(
    addr: SocketAddr,
    method: &str,
    path: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> (u16, Vec<u8>) {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(ct) = content_type {
        head.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    head.push_str("\r\n");
    let body_owned = body.to_vec();

    let raw = tokio::task::spawn_blocking(move || {
        let mut stream = StdTcpStream::connect(addr).expect("connect test server");
        stream.write_all(head.as_bytes()).unwrap();
        if !body_owned.is_empty() {
            stream.write_all(&body_owned).unwrap();
        }
        stream.flush().unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        buf
    })
    .await
    .unwrap();
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or_else(|| {
            panic!(
                "invalid HTTP response ({} bytes): {:?}",
                raw.len(),
                String::from_utf8_lossy(&raw)
            )
        });
    let header_str = String::from_utf8_lossy(&raw[..sep]);
    let status: u16 = header_str
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();

    let raw_body = &raw[sep + 4..];
    let is_chunked = header_str
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    let body_out = if is_chunked {
        decode_chunked(raw_body)
    } else {
        raw_body.to_vec()
    };
    (status, body_out)
}

fn decode_chunked(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(pos) = data.windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let len_line = std::str::from_utf8(&data[..pos]).unwrap().trim();
        let hex = len_line.split(';').next().unwrap();
        let size = usize::from_str_radix(hex, 16).unwrap_or(0);
        data = &data[pos + 2..];
        if size == 0 {
            break;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
    out
}

async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let (st, bytes) = http_req(addr, "GET", path, None, &[]).await;
    (st, serde_json::from_slice(&bytes).unwrap())
}

async fn post_json(addr: SocketAddr, method: &str, path: &str, v: &Value) -> (u16, Value) {
    let body = serde_json::to_vec(v).unwrap();
    let (st, bytes) = http_req(addr, method, path, Some("application/json"), &body).await;
    (st, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_v2_p4_headless_http_api_full_smoke() {
    let state = AppState::new_blank(16, 16, 0.0, SimRuleset::hardcore_platoon()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = build_router(state);
    let srv = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::task::yield_now().await;

    // 1. Health, schema, catalog
    let (st, health) = get_json(addr, "/health").await;
    assert_eq!(st, 200);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["cell_m"], 8.0);
    assert_eq!(health["tick_s"], 16);

    let (st, schema) = get_json(addr, "/api/schema").await;
    assert_eq!(st, 200);
    assert_eq!(schema["cell_m"], 8.0);

    let (st, cat) = get_json(addr, "/api/catalog").await;
    assert_eq!(st, 200);
    assert!(cat["count"].as_u64().unwrap() >= 250);

    // 2. Scenario import then create blank 64x64 8m world + binary terrain check
    let scen = ScenarioSpec {
        name: "Smoke Scenario".into(),
        epoch_seconds: 23400,
        night: false,
        ruleset: SimRuleset::tactical_standard(),
        terrain: TerrainSourceSpec::Flat {
            rows: 32,
            cols: 32,
            base_elevation_m: 15.0,
        },
        platoons: vec![],
        units: vec![],
        objectives: vec![],
    };
    let (st, sc_resp) = post_json(addr, "POST", "/api/world/scenario", &serde_json::to_value(scen).unwrap()).await;
    assert_eq!(st, 200);
    assert_eq!(sc_resp["ok"], true);

    let (st, cr) = post_json(
        addr,
        "POST",
        "/api/world/create",
        &json!({ "rows": 64, "cols": 64, "base_elevation_m": 20.0 }),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(cr["ok"], true);

    let (st, bin) = http_req(addr, "GET", "/api/terrain.bin", None, &[]).await;
    assert_eq!(st, 200);
    assert_eq!(&bin[0..8], BINARY_TERRAIN_MAGIC);
    assert_eq!(bin.len(), 24 + 64 * 64 * 7);

    // 3. Ruleset read/update
    let (st, mut rs): (u16, SimRuleset) = {
        let (s, v) = get_json(addr, "/api/ruleset").await;
        (s, serde_json::from_value(v).unwrap())
    };
    assert_eq!(st, 200);
    rs.allow_runtime_map_edit = true;
    rs.enable_time_machine_deferral = false;
    rs.enable_g_field_autonomy = false;
    rs.enforce_fog_of_war = true;
    let (st, put_res) = post_json(addr, "PUT", "/api/ruleset", &serde_json::to_value(&rs).unwrap()).await;
    assert_eq!(st, 200);
    assert_eq!(put_res["accepted"], true);
    let (_, rs_check) = get_json(addr, "/api/ruleset").await;
    assert_eq!(rs_check["allow_runtime_map_edit"], true);

    // 4. Spawn Red platoon & 3 squads via unified /api/command
    let (_, pl_res) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({ "kind": "spawn_platoon", "name": "Red-1", "team": "red" }),
    )
    .await;
    assert_eq!(pl_res["accepted"], true);
    let pid = pl_res["created_id"].as_u64().unwrap();

    let spawn_req = |callsign: &str, team: &str, pl: Option<u64>, x: f32, y: f32, wpn: &str, ammo: &str, mounts: Vec<&str>, hp: f32| {
        json!({
            "kind": "spawn_unit",
            "spec": {
                "callsign": callsign, "team": team, "platoon_id": pl, "role": "infantry_squad",
                "mobility": "foot", "pos": { "x": x, "y": y }, "facing_deg": 90.0,
                "men": 9, "hp": hp, "armor_mm": 0.0, "weapon_id": wpn, "ammo_id": ammo,
                "mount_ids": mounts, "ammo_rounds": 1200, "smoke_grenades": 2,
                "base_speed_mps": 1.6, "vision_m": 200.0, "base_radio_range_m": 800.0
            }
        })
    };
    let (_, r1) = post_json(addr, "POST", "/api/command", &spawn_req("Red-MG1", "red", Some(pid), 100.0, 200.0, "wpn_mg42", "ammo_792x57_ss", vec!["mount_lafette_42"], 100.0)).await;
    let red_id = r1["created_id"].as_u64().unwrap();
    let (_, b_near) = post_json(addr, "POST", "/api/command", &spawn_req("Blue-Near", "blue", None, 200.0, 200.0, "wpn_luger_p08", "ammo_9x19_m08_ball", vec![], 250.0)).await;
    let near_id = b_near["created_id"].as_u64().unwrap();
    let (_, b_far) = post_json(addr, "POST", "/api/command", &spawn_req("Blue-Far", "blue", None, 460.0, 200.0, "wpn_luger_p08", "ammo_9x19_m08_ball", vec![], 100.0)).await;
    let far_id = b_far["created_id"].as_u64().unwrap();
    post_json(addr, "POST", "/api/command", &json!({ "kind": "set_fire_discipline", "unit_id": far_id, "discipline": "hold_fire" })).await;

    // 5. Read-only preview sandbox (must keep authoritative_sha256 untouched)
    let (_, state_before) = get_json(addr, "/api/state?team=red").await;
    let sha_before = state_before["authoritative_sha256"].as_str().unwrap().to_string();

    let (st, plan_res) = post_json(addr, "POST", "/api/query/plan", &json!({ "unit_id": red_id, "goal": { "x": 160.0, "y": 200.0 } })).await;
    assert_eq!(st, 200);
    assert_eq!(plan_res["reachable"], true);
    let (st, los_res) = post_json(addr, "POST", "/api/query/los", &json!({ "from": { "x": 100.0, "y": 200.0 }, "to": { "x": 200.0, "y": 200.0 }, "max_vision_m": 250.0 })).await;
    assert_eq!(st, 200);
    assert_eq!(los_res["visible"], true);
    let (st, arc_res) = post_json(addr, "POST", "/api/query/arc", &json!({ "unit_id": red_id, "center_angle_deg": 0.0, "half_arc_deg": 30.0, "ray_count": 5, "max_dist_m": 200.0 })).await;
    assert_eq!(st, 200);
    assert_eq!(arc_res["samples"].as_array().unwrap().len(), 5);
    let (st, work_res) = post_json(addr, "POST", "/api/query/work", &json!({ "unit_id": red_id, "work": "zigzag_trench", "control_points": [{ "x": 100.0, "y": 220.0 }, { "x": 132.0, "y": 220.0 }] })).await;
    assert_eq!(st, 200);
    assert!(!work_res["points"].as_array().unwrap().is_empty());

    let (_, state_after) = get_json(addr, "/api/state?team=red").await;
    assert_eq!(state_after["authoritative_sha256"], sha_before);

    // 6. Command move_unit + step 3 ticks + inspect playback & I/G fields
    let (_, mv_res) = post_json(addr, "POST", "/api/command", &json!({ "kind": "move_unit", "unit_id": red_id, "waypoints": [{ "x": 140.0, "y": 200.0 }] })).await;
    assert_eq!(mv_res["accepted"], true);

    let (st, step_res) = post_json(addr, "POST", "/api/clock/step", &json!({ "ticks": 3, "observer_team": "red" })).await;
    assert_eq!(st, 200);
    assert_eq!(step_res["tick"], 3);
    let steps = step_res["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["report"]["stages_executed"].as_array().unwrap().len(), 13);
    assert!(!steps[0]["playback"]["particle_tracks"].as_array().unwrap().is_empty());
    assert!(steps.iter().any(|s| !s["playback"]["playback_events"].as_array().unwrap().is_empty()));

    let (st, i_field) = get_json(addr, "/api/fields/i").await;
    assert_eq!(st, 200);
    assert_eq!(i_field["values"].as_array().unwrap().len(), 64 * 64);
    let (st, g_field) = get_json(addr, "/api/fields/g").await;
    assert_eq!(st, 200);
    assert_eq!(g_field["cell_m"], 8.0);
    let (st_bad, _) = get_json(addr, "/api/fields/invalid").await;
    assert_eq!(st_bad, 400);

    // 7. Fog-of-war HTTP enforcement (near unit visible w/ redaction, distant unit -> 404)
    let (st, near_view) = get_json(addr, &format!("/api/units/{near_id}?team=red")).await;
    assert_eq!(st, 200);
    assert!(near_view["ammo_rounds"].is_null() && near_view["order"].is_null());
    let (st_far, _) = get_json(addr, &format!("/api/units/{far_id}?team=red")).await;
    assert_eq!(st_far, 404);

    // 8. Save -> step 2 ticks -> load saved -> step 2 ticks => identical SHA-256!
    let (st, saved_bytes) = http_req(addr, "GET", "/api/world/save", None, &[]).await;
    assert_eq!(st, 200);
    let (_, run_a) = post_json(addr, "POST", "/api/clock/step", &json!({ "ticks": 2 })).await;
    let sha_a = run_a["steps"][1]["report"]["authoritative_sha256"].as_str().unwrap().to_string();

    let (st_load, _) = http_req(addr, "POST", "/api/world/load", Some("application/json"), &saved_bytes).await;
    assert_eq!(st_load, 200);
    let (_, run_b) = post_json(addr, "POST", "/api/clock/step", &json!({ "ticks": 2 })).await;
    let sha_b = run_b["steps"][1]["report"]["authoritative_sha256"].as_str().unwrap().to_string();
    assert_eq!(sha_a, sha_b, "save -> step 2 -> load -> step 2 must be bit-identical");

    srv.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_v2_b4_http_i_triplet_step_cache_keyframes_and_stop_exact_ms() {
    // Hard gate: gentactix-server/src/*.rs total lines MUST stay strictly < 400
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut total_src_lines = 0usize;
    for entry in std::fs::read_dir(&src_dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            let content = std::fs::read_to_string(&p).unwrap();
            total_src_lines += content.lines().count();
        }
    }
    assert!(
        total_src_lines < 400,
        "gentactix-server/src must stay thin (< 400 lines), got {total_src_lines}"
    );

    let state = AppState::new_blank(48, 48, 0.0, SimRuleset::hardcore_platoon()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = build_router(state);
    let srv = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::task::yield_now().await;

    // Disable time machine deferral & autonomy so commands execute cleanly on exact step assertions,
    // while keeping fog-of-war ON and allowing runtime map edits for detour obstacle.
    let (_, mut rs) = get_json(addr, "/api/ruleset").await;
    rs["allow_runtime_map_edit"] = json!(true);
    rs["enable_time_machine_deferral"] = json!(false);
    rs["enable_g_field_autonomy"] = json!(false);
    rs["enforce_fog_of_war"] = json!(true);
    let (st, put_rs) = post_json(addr, "PUT", "/api/ruleset", &rs).await;
    assert_eq!(st, 200);
    assert_eq!(put_rs["accepted"], true);

    // Spawn Red MG shooter, Red tank (for 8-subsystem check), Red maneuvering squad (river detour),
    // and fragile Blue squad right in front of Red MG so it dies mid-tick!
    let (_, red_mg) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "spawn_unit",
            "spec": {
                "callsign": "Red-MG-B4", "team": "red", "role": "heavy_weapon_team",
                "mobility": "foot", "pos": { "x": 100.0, "y": 200.0 }, "facing_deg": 0.0,
                "men": 5, "hp": 100.0, "armor_mm": 0.0, "weapon_id": "wpn_mg42",
                "ammo_id": "ammo_792x57_ss", "mount_ids": ["mount_lafette_42"],
                "ammo_rounds": 1500, "smoke_grenades": 2, "base_speed_mps": 1.6,
                "vision_m": 250.0, "base_radio_range_m": 800.0
            }
        }),
    )
    .await;
    let mg_id = red_mg["created_id"].as_u64().unwrap();

    let (_, red_tank) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "spawn_unit",
            "spec": {
                "callsign": "Red-Tank-B4", "team": "red", "role": "armored_vehicle",
                "mobility": "tracked", "pos": { "x": 80.0, "y": 140.0 }, "facing_deg": 0.0,
                "men": 4, "hp": 300.0, "armor_mm": 45.0, "weapon_id": "wpn_luger_p08",
                "ammo_id": "ammo_9x19_m08_ball", "mount_ids": [], "ammo_rounds": 500,
                "smoke_grenades": 2, "base_speed_mps": 4.0, "vision_m": 250.0,
                "base_radio_range_m": 800.0
            }
        }),
    )
    .await;
    let tank_id = red_tank["created_id"].as_u64().unwrap();

    let (_, red_runner) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "spawn_unit",
            "spec": {
                "callsign": "Red-Detour", "team": "red", "role": "infantry_squad",
                "mobility": "foot", "pos": { "x": 60.0, "y": 60.0 }, "facing_deg": 0.0,
                "men": 9, "hp": 100.0, "armor_mm": 0.0, "weapon_id": "wpn_luger_p08",
                "ammo_id": "ammo_9x19_m08_ball", "mount_ids": [], "ammo_rounds": 400,
                "smoke_grenades": 1, "base_speed_mps": 2.4, "vision_m": 200.0,
                "base_radio_range_m": 800.0
            }
        }),
    )
    .await;
    let runner_id = red_runner["created_id"].as_u64().unwrap();

    // Paint water obstacle right across Red-Detour's direct path at col=9..=10, row=6..=9
    for r in 6..=9 {
        for c in 9..=10 {
            let (st_ed, ed_res) = post_json(
                addr,
                "POST",
                "/api/command",
                &json!({
                    "kind": "edit_terrain",
                    "cell": { "r": r, "c": c },
                    "terrain_kind": "water"
                }),
            )
            .await;
            assert_eq!(st_ed, 200);
            assert_eq!(ed_res["accepted"], true);
        }
    }
    post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "move_unit",
            "unit_id": runner_id,
            "waypoints": [{ "x": 100.0, "y": 60.0 }]
        }),
    )
    .await;

    // Fragile visible Blue target at x=140, y=200, moving West (so it gets killed mid-tick and clamps stop_exact_ms!)
    let (_, blue_fragile) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "spawn_unit",
            "spec": {
                "callsign": "Blue-Fragile", "team": "blue", "role": "infantry_squad",
                "mobility": "foot", "pos": { "x": 140.0, "y": 200.0 }, "facing_deg": 180.0,
                "men": 1, "hp": 6.0, "armor_mm": 0.0, "weapon_id": "wpn_luger_p08",
                "ammo_id": "ammo_9x19_m08_ball", "mount_ids": [], "ammo_rounds": 100,
                "smoke_grenades": 0, "base_speed_mps": 2.0, "vision_m": 200.0,
                "base_radio_range_m": 800.0
            }
        }),
    )
    .await;
    let fragile_id = blue_fragile["created_id"].as_u64().unwrap();
    post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "set_fire_discipline",
            "unit_id": fragile_id,
            "discipline": "hold_fire"
        }),
    )
    .await;
    post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "move_unit",
            "unit_id": fragile_id,
            "waypoints": [{ "x": 120.0, "y": 200.0 }]
        }),
    )
    .await;

    // Schedule a timed charge by Red for tick=3 -> visible in Red step_cache_queue & visible_instances, hidden from distant Blue!
    let (_, blue_far) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "spawn_unit",
            "spec": {
                "callsign": "Blue-ObserverFar", "team": "blue", "role": "infantry_squad",
                "mobility": "foot", "pos": { "x": 360.0, "y": 360.0 }, "facing_deg": 0.0,
                "men": 6, "hp": 100.0, "armor_mm": 0.0, "weapon_id": "wpn_luger_p08",
                "ammo_id": "ammo_9x19_m08_ball", "mount_ids": [], "ammo_rounds": 100,
                "smoke_grenades": 0, "base_speed_mps": 1.6, "vision_m": 150.0,
                "base_radio_range_m": 800.0
            }
        }),
    )
    .await;
    let _far_obs = blue_far["created_id"].as_u64().unwrap();

    let (st_chg, chg_out) = post_json(
        addr,
        "POST",
        "/api/command",
        &json!({
            "kind": "schedule_timed_charge",
            "unit_id": mg_id,
            "pos": { "x": 108.0, "y": 200.0 },
            "delay_ticks": 3,
            "blast_radius_m": 24.0,
            "blast_kpa": 140.0
        }),
    )
    .await;
    assert_eq!(st_chg, 200);
    assert_eq!(chg_out["accepted"], true);

    // 1) Check ObservedStateView step_cache_queue & visible_instances + fog isolation
    let (_, red_obs) = get_json(addr, "/api/state?team=red").await;
    assert_eq!(red_obs["visible_instances"].as_array().unwrap().len(), 1);
    assert_eq!(red_obs["step_cache_queue"].as_array().unwrap().len(), 1);
    assert_eq!(red_obs["step_cache_queue"][0]["kind"], "detonation");
    assert_eq!(red_obs["step_cache_queue"][0]["target_tick"], 3);

    let (_, blue_obs) = get_json(addr, "/api/state?team=blue").await;
    assert!(
        blue_obs["step_cache_queue"].as_array().unwrap().is_empty(),
        "Blue must NOT see Red's scheduled step_cache_queue under fog-of-war"
    );

    // 2) Check GET /api/fields/i?team=red orthogonal triplet (i_intel, i_vis_tier, h_blast, i_threat)
    let (st, i_triplet) = get_json(addr, "/api/fields/i?team=red").await;
    assert_eq!(st, 200);
    assert_eq!(i_triplet["observer_team"], "red");
    assert_eq!(i_triplet["i_intel"].as_array().unwrap().len(), 48 * 48);
    assert_eq!(i_triplet["i_vis_tier"].as_array().unwrap().len(), 48 * 48);
    assert_eq!(i_triplet["h_blast"].as_array().unwrap().len(), 48 * 48);
    assert_eq!(i_triplet["i_threat"].as_array().unwrap().len(), 48 * 48);

    // 3) Check GET /api/units/:id fog-trimmed 4-region soldier & 8-subsystem tank details
    let (st, mg_detail) = get_json(addr, &format!("/api/units/{mg_id}?team=red")).await;
    assert_eq!(st, 200);
    assert_eq!(mg_detail["is_vehicle"], false);
    assert_eq!(mg_detail["soldiers"].as_array().unwrap().len(), 5);
    assert_eq!(mg_detail["soldiers"][0]["regions"].as_array().unwrap().len(), 4);
    assert!(mg_detail["soldiers"][0]["regions"][0]["integrity"].is_number());
    assert!(mg_detail["vehicle_damage"].is_null());

    let (st, tank_detail) = get_json(addr, &format!("/api/units/{tank_id}?team=red")).await;
    assert_eq!(st, 200);
    assert_eq!(tank_detail["is_vehicle"], true);
    assert_eq!(
        tank_detail["vehicle_damage"]["subsystems"].as_array().unwrap().len(),
        8
    );
    assert!(tank_detail["soldiers"].is_null());

    // Step 1 tick: Red-MG kills Blue-Fragile mid-tick, Red-Detour detours river with segmented keyframes
    let (st, step_res) = post_json(
        addr,
        "POST",
        "/api/clock/step",
        &json!({ "ticks": 1, "observer_team": "red" }),
    )
    .await;
    assert_eq!(st, 200);
    let pb = &step_res["steps"][0]["playback"];

    // t=0s baselines exist before end-of-tick HP/ammo mutation (anti-spoiler Section 3.1)
    let baselines = pb["unit_baselines"].as_array().unwrap();
    let mg_base = baselines
        .iter()
        .find(|b| b["piece_id"] == mg_id.to_string())
        .expect("mg baseline missing");
    assert_eq!(mg_base["ammo_start"], 1500);
    assert_eq!(mg_base["hp_start"], 100.0);

    // Dead fragile unit MUST appear in particle_tracks with stop_exact_ms in 1..=16000!
    let tracks = pb["particle_tracks"].as_array().unwrap();
    let dead_track = tracks
        .iter()
        .find(|t| t["piece_id"] == fragile_id.to_string())
        .expect("mid-tick killed unit track missing");
    let stop_ms = dead_track["stop_exact_ms"]
        .as_u64()
        .expect("killed unit must carry stop_exact_ms");
    assert!((1..=16000).contains(&stop_ms));
    let dead_kfs = dead_track["keyframes"].as_array().unwrap();
    assert!(dead_kfs.last().unwrap()[0].as_u64().unwrap() <= stop_ms);

    // Detouring runner emits >= 2 keyframes ordered in 0..=16000
    let runner_track = tracks
        .iter()
        .find(|t| t["piece_id"] == runner_id.to_string())
        .expect("runner track missing");
    let kfs = runner_track["keyframes"].as_array().unwrap();
    assert!(kfs.len() >= 2);
    assert_eq!(kfs[0][0], 0);
    assert_eq!(kfs.last().unwrap()[0], 16000);

    // Rich PlaybackEvent includes ammo_spent and hp_delta
    let events = pb["playback_events"].as_array().unwrap();
    let fire_ev = events
        .iter()
        .find(|e| e["source_piece_id"] == mg_id.to_string() && e["event_kind"] == "fire")
        .expect("mg fire playback event missing");
    assert!(fire_ev["ammo_spent"].as_u64().unwrap() > 0);
    assert!(fire_ev["hp_delta"].as_f64().unwrap() > 0.0);

    srv.abort();
}
