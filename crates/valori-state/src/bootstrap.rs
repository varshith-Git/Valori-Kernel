// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! State bootstrap — crash recovery via event log replay, WAL replay, or
//! snapshot restore.
//!
//! This module is the correct home for recovery orchestration. It was
//! previously in `valori-storage::recovery` (Phase A2), which was the wrong
//! crate — raw byte movement belongs in `valori-storage`; state lifecycle
//! orchestration belongs here.
//!
//! # Priority order
//!
//! 1. **Event log** — canonical truth. If the event log exists and contains
//!    committed events, replay from scratch to rebuild `KernelState`.
//! 2. **Snapshot** — fast-path cache. Loaded only when the event log is absent
//!    or empty.
//! 3. **WAL** — legacy fallback. Replayed on top of an existing state when the
//!    event log is not present.
//! 4. **Fresh start** — no durable state found.

use std::path::Path;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::decode::decode_state;
use valori_storage::wal_reader::WalReader;
use valori_storage::events::event_replay::{recover_from_event_log, verify_snapshot_consistency};
use valori_storage::events::EventJournal;
use crate::error::{StateError, StateResult};

/// Outcome of a bootstrap attempt.
#[derive(Debug)]
pub enum BootstrapMode {
    /// Recovered by replaying `n` events from the event log.
    EventLog(u64),
    /// Recovered by loading a snapshot.
    Snapshot,
    /// Recovered by replaying `n` WAL commands.
    Wal(usize),
    /// No durable state found; started fresh.
    Fresh,
}

// ── Event log ────────────────────────────────────────────────────────────────

/// Replay the event log at `path` and return the recovered `KernelState`,
/// `EventJournal`, and the number of events applied.
///
/// Returns `StateError::InvalidInput` if the log is malformed.
/// Returns `Ok((fresh_state, empty_journal, 0))` if the log exists but is empty.
pub fn recover_from_events(
    event_log_path: &Path,
) -> StateResult<(KernelState, EventJournal, u64)> {
    tracing::info!("Recovering from event log: {:?}", event_log_path);

    recover_from_event_log(event_log_path)
        .map_err(|e| StateError::InvalidInput(format!("Event log replay failed: {:?}", e)))
}

/// Returns `true` when the event log at `path` exists and contains at least
/// the minimum header bytes (16 B) needed to be parseable.
pub(crate) fn has_event_log(event_log_path: &Path) -> bool {
    event_log_path.exists()
        && std::fs::metadata(event_log_path)
            .map(|m| m.len() >= 16)
            .unwrap_or(false)
}

// ── WAL ───────────────────────────────────────────────────────────────────────

/// Replay WAL entries from `wal_path` on top of `state`.
///
/// Returns `(commands_applied, running_blake3_hasher)`.
/// The hasher is initialised with the WAL header bytes so callers can verify
/// chain continuity across WAL segments if needed.
///
/// Called by `Engine::try_recover` (valori-engine) as the legacy fallback
/// when the deployment uses `Persistence::Wal` instead of the canonical
/// event log — see that function's doc comment for the full priority order.
pub fn replay_wal(
    state: &mut KernelState,
    wal_path: &Path,
) -> StateResult<(usize, blake3::Hasher)> {
    let dim = state.dim.map(|d| d as u32);
    let reader = WalReader::open(wal_path, dim)
        .map_err(|e| StateError::InvalidInput(format!("Failed to open WAL: {}", e)))?;

    let start = std::time::Instant::now();
    let mut commands_applied = 0;
    let mut hasher = blake3::Hasher::new();

    // Seed the hasher with the 16-byte WAL header fields.
    hasher.update(&1u32.to_le_bytes()); // header_ver
    hasher.update(&0u32.to_le_bytes()); // enc_ver
    hasher.update(&dim.unwrap_or(0).to_le_bytes());
    hasher.update(&0u32.to_le_bytes()); // crc_len placeholder

    for result in reader {
        let (evt, ns) = result
            .map_err(|e| StateError::InvalidInput(format!("WAL read error: {}", e)))?;

        state.apply_event_ns(&evt, ns).map_err(StateError::Kernel)?;

        let entry_bytes = bincode::serde::encode_to_vec(&(&evt, ns), bincode::config::standard())
            .map_err(|e| StateError::InvalidInput(format!("Hash serialization failed: {}", e)))?;
        hasher.update(&entry_bytes);

        commands_applied += 1;
    }

    metrics::histogram!(
        "valori_wal_replay_duration_seconds",
        start.elapsed().as_secs_f64()
    );
    Ok((commands_applied, hasher))
}

/// Returns `true` when the WAL at `path` exists and contains at least the
/// 16-byte header.
pub(crate) fn has_wal(wal_path: &Path) -> bool {
    wal_path.exists()
        && std::fs::metadata(wal_path)
            .map(|m| m.len() >= 16)
            .unwrap_or(false)
}

// ── Snapshot ──────────────────────────────────────────────────────────────────

/// Decode the snapshot at `snapshot_path` into a `KernelState`.
pub(crate) fn load_snapshot(snapshot_path: &Path) -> StateResult<KernelState> {
    let data = std::fs::read(snapshot_path)?;
    decode_state(&data)
        .map_err(|e| StateError::InvalidInput(format!("Snapshot decode failed: {:?}", e)))
}

/// Verify that a previously-loaded snapshot is consistent with a replayed state.
/// Returns `true` when the state hashes agree, `false` on mismatch.
/// A missing snapshot is treated as consistent (nothing to check).
pub(crate) fn validate_snapshot(
    snapshot_path: &Path,
    replayed_state: &KernelState,
) -> StateResult<bool> {
    if !snapshot_path.exists() {
        return Ok(true);
    }

    tracing::info!("Validating snapshot: {:?}", snapshot_path);

    let snapshot_data = std::fs::read(snapshot_path)?;
    let snapshot_state = decode_state(&snapshot_data)
        .map_err(|e| StateError::InvalidInput(format!("Snapshot decode failed: {:?}", e)))?;

    let ok = verify_snapshot_consistency(&snapshot_state, replayed_state);
    if !ok {
        tracing::warn!("Snapshot hash mismatch detected against replayed state");
    }
    Ok(ok)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::event::KernelEvent;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use valori_storage::wal_writer::WalWriter;
    use tempfile::tempdir;

    #[test]
    fn test_replay_wal_round_trip() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        {
            let mut writer = WalWriter::open(&wal_path, 16).unwrap();
            for i in 0..50 {
                let evt = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::new_zeros(16),
                    metadata: None,
                    tag: 0,
                };
                writer.append_event(&evt, 0).unwrap();
            }
        }

        let mut state = KernelState::new();
        let (count, _hasher) = replay_wal(&mut state, &wal_path).unwrap();

        assert_eq!(count, 50);
        for i in 0..50 {
            assert!(state.get_record(RecordId(i)).is_some());
        }
    }

    #[test]
    fn test_has_wal_missing() {
        assert!(!has_wal(std::path::Path::new("/nonexistent/path.wal")));
    }

    #[test]
    fn test_has_event_log_missing() {
        assert!(!has_event_log(std::path::Path::new("/nonexistent/events.log")));
    }
}
