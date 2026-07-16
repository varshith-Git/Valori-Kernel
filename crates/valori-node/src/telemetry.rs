// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

static PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initialize telemetry (logs + metrics)
pub fn init_telemetry() {
    // 1. Initialize Tracing (Logs)
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "valori_node=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 2. Initialize Metrics (Prometheus)
    let builder = PrometheusBuilder::new();
    let handle = builder
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    // Store handle for /metrics endpoint
    if PROM_HANDLE.set(handle).is_err() {
        tracing::warn!("Prometheus handle already set. Telemetry re-initialized?");
    }

    // ── Event / WAL metrics ───────────────────────────────────────────────────
    metrics::describe_counter!(
        "valori_events_committed_total",
        "Total number of events committed to the event log"
    );
    metrics::describe_histogram!(
        "valori_event_commit_duration_seconds",
        "Time taken to commit a single event"
    );
    metrics::describe_gauge!(
        "valori_snapshot_size_bytes",
        "Size of the last written snapshot in bytes"
    );
    metrics::describe_counter!(
        "valori_proofs_generated_total",
        "Total number of cryptographic proof queries served"
    );
    metrics::describe_histogram!(
        "valori_replay_duration_seconds",
        "Time spent replaying the WAL or event log on startup"
    );

    // ── Raft cluster gauges (Phase 2.10c; updated by the metrics watcher) ─────
    metrics::describe_gauge!("valori_raft_term", "Current Raft term on this node");
    metrics::describe_gauge!(
        "valori_raft_current_leader",
        "Node id of the leader this node currently sees (0 = none)"
    );
    metrics::describe_gauge!(
        "valori_raft_is_leader",
        "1 when this node is the leader, else 0"
    );
    metrics::describe_gauge!(
        "valori_raft_last_log_index",
        "Highest Raft log index appended on this node"
    );
    metrics::describe_gauge!(
        "valori_raft_last_applied_index",
        "Highest Raft log index applied to the kernel; the gap to last_log_index is apply lag"
    );
    metrics::describe_gauge!(
        "valori_raft_snapshot_index",
        "Log index covered by the most recent Raft snapshot"
    );
    metrics::describe_gauge!(
        "valori_raft_purged_index",
        "Highest Raft log index removed by compaction"
    );

    // ── KernelState capacity gauges (updated on /health and /metrics) ─────────
    metrics::describe_gauge!(
        "valori_records_live",
        "Number of live (non-deleted) records in the store"
    );
    metrics::describe_gauge!(
        "valori_records_capacity",
        "Maximum records allowed (VALORI_MAX_RECORDS)"
    );
    metrics::describe_gauge!(
        "valori_record_fill_ratio",
        "Live records divided by capacity (0.0–1.0); alert above 0.9"
    );
    metrics::describe_gauge!("valori_nodes_live", "Number of live graph nodes");
    metrics::describe_gauge!(
        "valori_nodes_capacity",
        "Maximum nodes allowed (VALORI_MAX_NODES)"
    );
    metrics::describe_gauge!(
        "valori_node_fill_ratio",
        "Live nodes divided by capacity (0.0–1.0)"
    );
    metrics::describe_gauge!("valori_edges_live", "Number of live graph edges");
    metrics::describe_gauge!(
        "valori_edges_capacity",
        "Maximum edges allowed (VALORI_MAX_EDGES)"
    );
    metrics::describe_gauge!(
        "valori_edge_fill_ratio",
        "Live edges divided by capacity (0.0–1.0)"
    );
    metrics::describe_gauge!("valori_dim", "Configured vector dimension (VALORI_DIM)");
    metrics::describe_gauge!(
        "valori_event_log_height",
        "Number of committed events in the event journal"
    );

    // ── Cross-replica state-hash agreement ────────────────────────────────────
    metrics::describe_gauge!(
        "valori_raft_state_hash_match",
        "1 when all reachable peers agree on this node's BLAKE3 state hash, 0 on divergence"
    );
    metrics::describe_counter!(
        "valori_raft_divergence_detections_total",
        "Number of times this node detected a state-hash mismatch with any peer"
    );

    // ── Liveness sentinel ─────────────────────────────────────────────────────
    // Ensure at least one gauge exists at startup before any request arrives.
    metrics::gauge!("valori_node_up", 1.0);
}

/// Get the Prometheus handle to render metrics
pub fn get_metrics() -> String {
    if let Some(handle) = PROM_HANDLE.get() {
        handle.render()
    } else {
        "# metrics not initialized".to_string()
    }
}
