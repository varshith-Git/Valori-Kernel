use valori_node::config::NodeConfig;
use valori_node::server::{build_router, ConcreteEngine, SharedEngine};
use std::sync::Arc;
use tokio::sync::Mutex;
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
    
    tracing::info!("Initializing Valori Node with config: max_records={}, dim={}", cfg.max_records, cfg.dim);
    
    let engine = ConcreteEngine::new(&cfg);
    let shared_state: SharedEngine = Arc::new(Mutex::new(engine));
    
    let app = build_router(shared_state);
    
    let addr = cfg.bind_addr;
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
