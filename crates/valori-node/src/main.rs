use valori_node::config::NodeConfig;
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
    let app = build_router_with_keys(shared_state.clone(), cfg.auth_token.clone(), cfg.cors_origin.clone(), key_store);

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

// ── Cluster mode (Phase 2) ────────────────────────────────────────────────────

async fn run_cluster(cluster_cfg: valori_node::cluster::ClusterConfig) {
    use valori_node::cluster::bootstrap_cluster;
    use valori_node::cluster_server::build_cluster_router;
    use valori_node::commit::EventLogAuditSink;
    use valori_node::events::event_log::EventLogWriter;

    let node_cfg = NodeConfig::default();

    tracing::info!(
        node_id = cluster_cfg.node_id,
        members = cluster_cfg.members.len(),
        tls = cluster_cfg.tls.is_some(),
        persistent_raft_log = cluster_cfg.raft_log_path.is_some(),
        "Booting in CLUSTER mode"
    );

    // The audit sink: quorum-committed events land in the chained
    // events.log, same format the standalone path and valori-verify use.
    let (audit_sink, audit_writer): (
        Box<dyn valori_consensus::AuditSink>,
        Option<Arc<std::sync::Mutex<EventLogWriter>>>,
    ) = match &node_cfg.event_log_path {
        Some(path) => {
            let writer = EventLogWriter::open(path, Some(node_cfg.dim as u32))
                .unwrap_or_else(|e| {
                    eprintln!("FATAL: cannot open audit log {path:?}: {e}");
                    std::process::exit(1);
                });
            let mut sink = EventLogAuditSink::new(writer);
            if let Some(limit) = node_cfg.event_log_rotation_bytes {
                sink = sink.with_rotation_bytes(if limit == 0 { None } else { Some(limit) });
            }
            let handle = sink.writer();
            (Box::new(sink), Some(handle))
        }
        None => {
            tracing::warn!(
                "VALORI_EVENT_LOG_PATH is not set — cluster running WITHOUT an \
                 audit log. Committed events are replicated but not chained to \
                 disk on this node."
            );
            (Box::new(valori_consensus::NullAuditSink), None)
        }
    };

    let handle = bootstrap_cluster(&cluster_cfg, audit_sink)
        .await
        .unwrap_or_else(|e| {
            eprintln!("FATAL: cluster bootstrap failed: {e}");
            std::process::exit(1);
        });
    tracing::info!("Raft listening on {}", handle.raft_addr);

    let app = build_cluster_router(&handle, audit_writer);
    let addr = node_cfg.bind_addr;
    tracing::info!("HTTP API listening on {addr}");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
