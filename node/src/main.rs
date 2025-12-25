// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_node::config::NodeConfig;
use valori_node::server::{build_router, ConcreteEngine, SharedEngine};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "valori_node=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

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

    let shared_state: SharedEngine = Arc::new(Mutex::new(engine));
    
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
    
    let app = build_router(shared_state, cfg.auth_token.clone());
    
    let addr = cfg.bind_addr;
    tracing::info!("Listening on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
