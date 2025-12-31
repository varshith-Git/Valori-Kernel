// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_node::config::NodeConfig;
use valori_node::server::{build_router, ConcreteEngine, SharedEngine};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // Initialize Telemetry (Logs + Metrics)
    valori_node::telemetry::init_telemetry();

    let cfg = NodeConfig::default();
    
    tracing::info!("Initializing Valori Node with config: {:?}", cfg);
    
    let mut engine = ConcreteEngine::new(&cfg);
    
    // Load Snapshot if present
    if let Some(path) = &cfg.snapshot_path {
        if path.exists() {
            tracing::info!("Found snapshot at {:?}. Loading...", path);
            match tokio::fs::read(path).await {
                Ok(data) => {
                    if let Err(e) = engine.restore(&data) {
                        tracing::error!("Failed to restore snapshot: {:?}", e);
                        // panic or continue? Continue empty for robustness, or fail?
                        // Fail is safer for DB.
                        panic!("Failed to restore snapshot");
                    } else {
                         tracing::info!("Snapshot restored successfully.");
                    }
                },
                Err(e) => tracing::error!("Failed to read snapshot file: {:?}", e),
            }
        }
    }

    // Use consts from server or duplicates? 
    // Since ConcreteEngine is imported from server, it uses defaults.
    // We should match usage.
    // ConcreteEngine is basically Engine<1024, 16, 1024, 2048>.
    
    // Explicit type to match generic server signature
    use valori_node::server::{MAX_RECORDS, D, MAX_NODES, MAX_EDGES};
    let shared_state: SharedEngine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> = Arc::new(Mutex::new(engine));
    
    // Spawn Persistence Task
    if let (Some(path), Some(secs)) = (cfg.snapshot_path.clone(), cfg.auto_snapshot_interval_secs) {
        let state_clone = shared_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(secs));
            loop {
                interval.tick().await; // First tick is immediate? No, typically waits.
                // We want to skip first immediate potentially? 
                // interval.tick().await; 
                
                tracing::debug!("Auto-snapshotting...");
                tracing::debug!("Auto-snapshotting...");
                let mut engine = state_clone.lock().await; 
                // Using save_snapshot which handles formatting and atomic rename
                match engine.save_snapshot(Some(&path)) {
                     Ok(_) => {
                         tracing::info!("Snapshot saved to {:?}", path);
                     },
                     Err(e) => tracing::error!("Snapshot failed: {:?}", e),
                }
                // Lock released here
            }
        });
    }
    
    let app = build_router(shared_state.clone(), cfg.auth_token.clone());
    
    let addr = cfg.bind_addr;
    tracing::info!("Listening on {}", addr);
    
    // Check Mode
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
