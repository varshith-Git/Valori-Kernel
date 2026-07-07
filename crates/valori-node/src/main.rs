use valori_node::config::NodeConfig;
use valori_effect;
use valori_node::server::{build_router_with_keys, SharedEngine};
use valori_node::api_keys::KeyStore;
use valori_node::engine::Engine;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::TcpListener;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // Docker HEALTHCHECK probe — connect to own TCP port, exit 0/1.
    // Distroless images have no curl; the binary is its own health probe.
    if std::env::args().any(|a| a == "--health-check") {
        let bind = std::env::var("VALORI_BIND").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let port = bind.rsplit(':').next().unwrap_or("3000");
        match std::net::TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(_) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    // Initialize Telemetry (Logs + Metrics)
    valori_node::telemetry::init_telemetry();

    // ── Boot-mode decision (Phase 2) ──────────────────────────────────────────
    // VALORI_CLUSTER_MEMBERS present → Raft cluster mode.
    // Absent → the standalone path below, unchanged.
    // A malformed topology is a hard stop — silently booting standalone on a
    // typo would be how you accidentally run two databases that each think
    // they're the real one.
    match valori_node::cluster::ClusterConfig::from_env() {
        Err(e) => {
            eprintln!("FATAL: cluster configuration invalid: {e}");
            std::process::exit(1);
        }
        Ok(Some(cluster_cfg)) => {
            run_cluster(cluster_cfg).await;
            return;
        }
        Ok(None) => { /* standalone — fall through */ }
    }

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

    let shared_state: SharedEngine = Arc::new(RwLock::new(engine));

    // ── Auto-snapshot task ────────────────────────────────────────────────────
    if let (Some(path), Some(secs)) = (cfg.snapshot_path.clone(), cfg.auto_snapshot_interval_secs) {
        let state_clone = shared_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;

                tracing::debug!("Auto-snapshotting...");
                let engine = state_clone.read().await;
                match engine.save_snapshot(Some(&path)) {
                    Ok(_) => tracing::info!("Snapshot saved to {:?}", path),
                    Err(e) => tracing::error!("Snapshot failed: {:?}", e),
                }
            }
        });
    }

    let key_store = Arc::new(KeyStore::new(cfg.keys_path.clone()));
    let receipt_store = Arc::new(valori_effect::ReceiptStore::new(256));
    let app = build_router_with_keys(shared_state.clone(), cfg.auth_token.clone(), cfg.cors_origin.clone(), key_store, receipt_store);

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

    let listener = TcpListener::bind(addr).await.unwrap_or_else(|e| {
        let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "Port {} is already in use — set VALORI_BIND to a free port (e.g. VALORI_BIND=0.0.0.0:3001)",
                addr
            )
        } else {
            format!("Cannot bind to {addr}: {e}")
        };
        eprintln!("FATAL: {msg}");
        std::process::exit(1);
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shared_state.clone(), cfg.snapshot_path.clone()))
        .await
        .unwrap();
}

/// Resolve on SIGTERM / Ctrl-C. Before returning (which lets axum drain and exit)
/// write a final snapshot if a snapshot path is configured. The WAL already
/// guarantees durability — this just keeps the next start instant. Snapshot-on-close.
async fn shutdown_signal(state: SharedEngine, snapshot_path: Option<std::path::PathBuf>) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => { s.recv().await; }
            Err(_)    => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c   => {}
        _ = terminate => {}
    }

    if let Some(path) = snapshot_path {
        tracing::info!("Shutdown signal received — saving final snapshot to {:?}", path);
        let engine = state.read().await;
        match engine.save_snapshot(Some(path.as_path())) {
            Ok(_)  => tracing::info!("Final snapshot saved"),
            Err(e) => tracing::error!("Final snapshot failed (WAL still durable): {:?}", e),
        }
    }
}

// ── Cluster mode (Phase 2) ────────────────────────────────────────────────────

async fn run_cluster(cluster_cfg: valori_node::cluster::ClusterConfig) {
    use valori_node::cluster::bootstrap_cluster;
    use valori_node::cluster_server::build_cluster_router;

    let node_cfg = NodeConfig::default();

    tracing::info!(
        node_id = cluster_cfg.node_id,
        members = cluster_cfg.members.len(),
        tls = cluster_cfg.tls.is_some(),
        persistent_raft_log = cluster_cfg.raft_log_path.is_some(),
        "Booting in CLUSTER mode"
    );

    if node_cfg.event_log_path.is_none() {
        tracing::warn!(
            "VALORI_EVENT_LOG_PATH is not set — cluster running WITHOUT an \
             audit log. Committed events are replicated but not chained to \
             disk on this node."
        );
    }

    // Phase S13: bootstrap_cluster builds a real per-shard audit sink for
    // every shard itself (quorum-committed events land in each shard's own
    // chained events.log, same format the standalone path and valori-verify
    // use) — this just passes the raw path + rotation config through instead
    // of pre-constructing a single EventLogWriter/EventLogAuditSink here.
    let rotation_bytes = match node_cfg.event_log_rotation_bytes {
        Some(0) => None,
        other => other,
    };
    let handle = bootstrap_cluster(
        &cluster_cfg,
        node_cfg.event_log_path.as_deref(),
        rotation_bytes,
        node_cfg.dim,
    )
    .await
    .unwrap_or_else(|e| {
        eprintln!("FATAL: cluster bootstrap failed: {e}");
        std::process::exit(1);
    });
    tracing::info!("Raft listening on {}", handle.raft_addr);

    let app = build_cluster_router(&handle, handle.event_log_writer.clone());
    let addr = node_cfg.bind_addr;
    tracing::info!("HTTP API listening on {addr}");

    let listener = TcpListener::bind(addr).await.unwrap_or_else(|e| {
        let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "Port {} is already in use — set VALORI_BIND to a free port (e.g. VALORI_BIND=0.0.0.0:3001)",
                addr
            )
        } else {
            format!("Cannot bind to {addr}: {e}")
        };
        eprintln!("FATAL: {msg}");
        std::process::exit(1);
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(cluster_shutdown_signal())
        .await
        .unwrap();
}

/// Resolve on SIGTERM / Ctrl-C so axum drains in-flight requests and the
/// process exits cleanly — Raft's redb log is the durable store in cluster
/// mode, and a clean exit lets redb release its file lock and flush.
async fn cluster_shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => { s.recv().await; }
            Err(_)    => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c   => {}
        _ = terminate => {}
    }
    tracing::info!("Shutdown signal received — draining and exiting (Raft log is durable)");
}
