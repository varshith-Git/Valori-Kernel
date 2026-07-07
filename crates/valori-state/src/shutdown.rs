// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Graceful shutdown — writes a final snapshot before the process exits.
use std::path::Path;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::encode::encode_state;
use crate::error::StateResult;

/// Encode `state` and write it to `path`, creating or overwriting the file.
///
/// Called from the graceful shutdown path (`SIGTERM`, project close) so the
/// next startup can skip WAL replay and load directly from the snapshot.
///
/// This function is synchronous — the caller must ensure it runs to completion
/// before the process exits. In the Axum server this is done inside
/// `with_graceful_shutdown(shutdown_signal())`.
pub fn shutdown_snapshot(state: &KernelState, path: &Path) -> StateResult<()> {
    tracing::info!("Shutdown snapshot: writing to {:?}", path);

    let mut buf: Vec<u8> = Vec::with_capacity(1 << 20); // 1 MiB initial capacity
    encode_state(state, &mut buf)
        .map_err(|e| crate::error::StateError::InvalidInput(
            format!("Snapshot encode failed: {:?}", e)
        ))?;

    std::fs::write(path, &buf)?;

    let size_kb = buf.len() / 1024;
    tracing::info!("Shutdown snapshot written: {} KiB to {:?}", size_kb, path);
    metrics::counter!("valori_shutdown_snapshots_total", 1);

    Ok(())
}
