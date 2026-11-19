use axum::{routing::get, Router};
use clap::Parser;
use gentactix_core::catalog::ItemCatalog;
use gentactix_core::ruleset::SimRuleset;
use gentactix_core::state::WorldState;

#[derive(Parser, Debug)]
#[command(name = "gentactix-server", version)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8767")]
    listen: String,
}

#[allow(dead_code)]
fn build_placeholder_router() -> Router {
    Router::new().route("/health", get(|| async { "ok" }))
}

fn main() {
    let args = Args::parse();
    let catalog = ItemCatalog::embedded().expect("embedded items catalog must validate");
    let world = WorldState::new_blank(64, 64, 0.0, SimRuleset::hardcore_platoon());
    let _router = build_placeholder_router();
    println!(
        "GenTactix V2 Pure Engine Foundation ready (listen target {}): catalog={} items, grid={}x{} @ {}m",
        args.listen,
        catalog.len(),
        world.terrain.rows,
        world.terrain.cols,
        world.terrain.geom.cell_size_m(),
    );
}
