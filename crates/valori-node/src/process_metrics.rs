// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Per-node process metrics — memory (RSS/virtual) and CPU for THIS
//! process, exported alongside the engine's own state gauges on
//! `/metrics`.
//!
//! Kept in `valori-node`, not `valori-engine`, deliberately: "how much RSS
//! is this OS process using" is a property of the running node, not of the
//! deterministic engine state, and `valori-kernel`/`valori-engine` must
//! stay free of OS-level dependencies (the kernel is `no_std` — see
//! CLAUDE.md's invariant 7).
//!
//! ## Why a background task, not the `/health` handler
//!
//! CPU percentage is only meaningful as a delta between two samples taken
//! some interval apart — `sysinfo` explicitly requires refreshing the same
//! process twice, at least `MINIMUM_CPU_UPDATE_INTERVAL` apart, before
//! `cpu_usage()` returns anything but 0. Sampling on a request handler
//! would give a number that depends on how recently someone happened to
//! call `/health`, which is worse than no number at all. A fixed-interval
//! task gives a consistent, honest reading.

use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// How often the sampler refreshes. Comfortably above sysinfo's
/// `MINIMUM_CPU_UPDATE_INTERVAL` (200ms) so every CPU reading is a real
/// delta, and cheap enough at this cadence to be irrelevant to node load.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(10);

/// Spawns the sampler. Returns immediately; the task runs until the
/// process exits. Safe to call once per process — calling it twice would
/// just double-write the same gauges, not corrupt anything.
pub fn spawn_process_metrics_task() {
    tokio::spawn(async move {
        let pid = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        let refresh = ProcessRefreshKind::nothing().with_cpu().with_memory();

        // First refresh establishes the baseline the first CPU delta is
        // measured against — its own cpu_usage() is always 0, so it's
        // taken before the loop rather than published.
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, refresh);

        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        interval.tick().await; // the first tick completes immediately

        loop {
            interval.tick().await;
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, refresh);

            let Some(proc) = sys.process(pid) else {
                // Can't happen for our own pid in practice; log rather than
                // silently stop publishing if it somehow does.
                tracing::warn!("process metrics: own pid not found in refresh, skipping sample");
                continue;
            };

            // sysinfo reports bytes for memory. `memory()` is RSS.
            metrics::gauge!("valori_process_memory_rss_bytes", proc.memory() as f64);
            metrics::gauge!(
                "valori_process_memory_virtual_bytes",
                proc.virtual_memory() as f64
            );
            // Percent of ONE core — >100 on a multi-threaded process under
            // load is expected, not a bug.
            metrics::gauge!("valori_process_cpu_percent", proc.cpu_usage() as f64);
        }
    });
}
