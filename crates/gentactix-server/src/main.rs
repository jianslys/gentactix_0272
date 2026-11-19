use clap::Parser;
use gentactix_core::ruleset::SimRuleset;
use gentactix_server::{build_router, AppState};
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(name = "gentactix-server", version)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8767")]
    listen: String,
    #[arg(long, default_value_t = 64)]
    rows: usize,
    #[arg(long, default_value_t = 64)]
    cols: usize,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let state = AppState::new_blank(args.rows, args.cols, 0.0, SimRuleset::hardcore_platoon())
        .expect("failed to initialize blank V2 world");
    let listener = TcpListener::bind(&args.listen)
        .await
        .expect("failed to bind TCP listener");
    println!(
        "GenTactix V2 headless server listening on http://{} ({}x{} @ 8.0m)",
        args.listen, args.rows, args.cols
    );
    axum::serve(listener, build_router(state))
        .await
        .expect("server exited unexpectedly");
}
