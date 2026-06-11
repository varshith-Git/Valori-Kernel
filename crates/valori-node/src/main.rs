use valori_node::config::NodeConfig;
use valori_node::server::{build_router, SharedEngine};
use valori_node::engine::Engine;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // Initialize Telemetry (Logs + Metrics)
    valori_node::telemetry::init_telemetry();

    let cfg = NodeConfig::default();

    tracing::info!("Initializing Valori Node with config: {:?}", cfg);

    let mut engine = Engine::new(&cfg);

    // ── Crash Recovery ────────────────────────────────────────────────────────
    // Priority order: event log (canonical truth) → snapshot → fresh start.
    // try_recover() never panics; on failure it logs and continues with the
    // next source. A corrupt snapshot no longer kills the process.
    let mode = engine.try_recover();
    match mode {
        valori_node::engine::RecoveryMode::EventLog(n) =>
            tracing::info!("Recovered {} events from event log", n),
        valori_node::engine::RecoveryMode::Snapshot =>
            tracing::info!("Recovered from snapshot"),
        valori_node::engine::RecoveryMode::Fresh =>
            tracing::info!("Starting fresh (no prior state found)"),
    }

    let shared_state: SharedEngine = Arc::new(Mutex::new(engine));

    // ── Auto-snapshot task ────────────────────────────────────────────────────
    if let (Some(path), Some(secs)) = (cfg.snapshot_path.clone(), cfg.auto_snapshot_interval_secs) {
        let state_clone = shared_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;

                tracing::debug!("Auto-snapshotting...");
                let engine = state_clone.lock().await;
                match engine.save_snapshot(Some(&path)) {
                    Ok(_) => tracing::info!("Snapshot saved to {:?}", path),
                    Err(e) => tracing::error!("Snapshot failed: {:?}", e),
                }
            }
        });
    }

    let app = build_router(shared_state.clone(), cfg.auth_token.clone());

    let addr = cfg.bind_addr;
    tracing::info!("Listening on {}", addr);

    // ── Replication mode ──────────────────────────────────────────────────────
    if let valori_node::config::NodeMode::Follower { leader_url } = cfg.mode {
        tracing::info!("Node starting in FOLLOWER mode. Leader: {}", leader_url);
        let state_clone = shared_state.clone();
        tokio::spawn(async move {
            valori_node::replication::run_follower_loop(state_clone, leader_url).await;
        });
    } else {
        tracing::info!("Node starting in LEADER mode.");
    }

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
