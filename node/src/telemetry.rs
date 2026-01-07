// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::sync::OnceLock;

static PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initialize telemetry (logs + metrics)
pub fn init_telemetry() {
    // 1. Initialize Tracing (Logs)
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "valori_node=debug,tower_http=debug".into()),
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
    
    // Default metrics to 0
    metrics::describe_counter!("valori_events_committed_total", "Total number of events committed");
    metrics::describe_histogram!("valori_event_commit_duration_seconds", "Time taken to commit an event");
    metrics::describe_gauge!("valori_snapshot_size_bytes", "Size of the last saved snapshot in bytes");
    metrics::describe_counter!("valori_proofs_generated_total", "Total number of cryptographic proofs generated");
    metrics::describe_histogram!("valori_replay_duration_seconds", "Time taken to replay WAL/Event Log");

    // Ensure at least one metric exists on startup
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
