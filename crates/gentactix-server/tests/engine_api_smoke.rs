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
