// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Forensic replay engine.
//!
//! [`ForensicEngine`] loads a Valori snapshot into a live [`KernelState`] and
//! then replays events from the write-ahead event log to any target event
//! count.  No WAL writers or event-log writers are opened — this is a
//! **read-only, forensic view** of the database.

use anyhow::{bail, Context, Result};
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::state::kernel::KernelState;
use valori_node::events::event_log::LogEntry;

/// Magic bytes that prefix every Valori snapshot blob.
const SNAPSHOT_MAGIC: &[u8; 4] = b"VAL1";

// ─── ForensicEngine ──────────────────────────────────────────────────────────

/// A read-only, forensic view of a Valori database.
///
/// The engine can be seeded from a snapshot file (the recommended workflow) or
/// started with an empty state and replayed from the very first event.
pub struct ForensicEngine {
    /// Live kernel state — mutated only by [`replay_to`](Self::replay_to).
    pub state: KernelState,

    /// Total events that have been applied to `state` so far.
    pub current_event_count: u64,

    /// Ordered list of 1-based event indices applied during [`replay_to`](Self::replay_to).
    pub applied_events: Vec<u64>,
}

impl ForensicEngine {
    /// Restore from a `snapshot_path` (VAL1 format).
    ///
    /// The resulting state reflects the exact point-in-time captured by the
    /// snapshot.  Call [`replay_to`](Self::replay_to) afterwards to advance
    /// the state further along the event log.
    pub fn from_snapshot(snapshot_path: &str) -> Result<Self> {
        let data = std::fs::read(snapshot_path)
            .with_context(|| format!("Cannot read snapshot: {snapshot_path}"))?;

        let state = parse_kernel_from_snapshot_bytes(&data).context(
            "Snapshot decode failed — file may be corrupt or from an incompatible version",
        )?;

        Ok(Self {
            state,
            current_event_count: 0,
            applied_events: Vec::new(),
        })
    }

    /// Build an engine starting from an **empty** state (no snapshot required).
    ///
    /// Use this when you want to replay the full history from event zero.
    pub fn empty() -> Self {
        Self {
            state: KernelState::new(),
            current_event_count: 0,
            applied_events: Vec::new(),
        }
    }

    /// Replay events from `log_path`, applying each one to `self.state` until
    /// `target_count` events have been applied.
    ///
    /// Events are **1-indexed**: event #1 is the first entry in the log.
    ///
    /// Returns the number of events actually applied in this call.
    pub fn replay_to(&mut self, log_path: &str, target_count: u64) -> Result<usize> {
        let raw = std::fs::read(log_path)
            .with_context(|| format!("Cannot read event log: {log_path}"))?;

        if raw.len() < 16 {
            return Ok(0); // Empty log — nothing to replay.
        }

        let header = valori_wire::parse_header(&raw)
            .map_err(|e| anyhow::anyhow!("Invalid event log header: {e}"))?;
        let mut offset = header.header_len;
        let mut event_index: u64 = 0;
        let mut replayed = 0;

        while offset < raw.len() {
            match valori_wire::decode_entry(header.version, &raw[offset..]) {
                Ok((chained, bytes_read)) => {
                    offset += bytes_read;
                    match chained.entry {
                        LogEntry::Event(event) => {
                            event_index += 1;

                            if event_index > target_count {
                                break;
                            }

                            self.state.apply_event(&event).map_err(|e| {
                                anyhow::anyhow!("Event #{event_index} failed: {e:?}")
                            })?;

                            self.current_event_count = event_index;
                            self.applied_events.push(event_index);
                            replayed += 1;
                        }
                        // S15: namespace-scoped data event — replay into its
                        // own collection so point-in-time state matches.
                        LogEntry::EventNs {
                            namespace_id,
                            event,
                        } => {
                            event_index += 1;

                            if event_index > target_count {
                                break;
                            }

                            self.state
                                .apply_event_ns(&event, namespace_id)
                                .map_err(|e| {
                                    anyhow::anyhow!("Event #{event_index} failed: {e:?}")
                                })?;

                            self.current_event_count = event_index;
                            self.applied_events.push(event_index);
                            replayed += 1;
                        }
                        LogEntry::Checkpoint { event_count, .. } => {
                            // Checkpoint entries record cumulative event count
                            // at the time a snapshot was taken.
                            event_index = event_count;
                        }
                        // Admin events never touch kernel state.
                        LogEntry::Admin(_) => {}
                    }
                }
                Err(e) => {
                    bail!("Event log corrupt at byte offset {offset}: {e}");
                }
            }
        }

        Ok(replayed)
    }

    // Mirror the Engine accessor API so CLI commands compile unchanged.
    pub fn record_count(&self) -> usize {
        self.state.record_count()
    }
    pub fn node_count(&self) -> usize {
        self.state.node_count()
    }
    pub fn edge_count(&self) -> usize {
        self.state.edge_count()
    }
    pub fn kernel_state(&self) -> &KernelState {
        &self.state
    }

    /// Returns the BLAKE3 content hash of the current kernel state as raw bytes.
    ///
    /// This is the same hash exposed by the Python `db.get_state_hash()` API.
    pub fn blake3_hash(&self) -> [u8; 32] {
        hash_state_blake3(&self.state)
    }

    /// Returns the BLAKE3 hash as a 64-character lowercase hex string.
    pub fn blake3_hex(&self) -> String {
        self.blake3_hash()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Convert f64 float values to a Q16.16 fixed-point vector for kernel search.
pub fn floats_to_fxp(floats: &[f64]) -> valori_kernel::types::vector::FxpVector {
    use valori_kernel::types::scalar::FxpScalar;
    use valori_kernel::types::vector::FxpVector;
    FxpVector {
        data: floats
            .iter()
            .map(|&f| {
                FxpScalar(
                    (f as f32 * 65536.0)
                        .round()
                        .clamp(i32::MIN as f32, i32::MAX as f32) as i32,
                )
            })
            .collect(),
    }
}

// ─── Snapshot parsing helpers ─────────────────────────────────────────────────

/// Parse a [`KernelState`] from raw snapshot bytes (VAL1 format).
///
/// # Layout
/// ```text
/// [4 B]  magic       "VAL1"
/// [4 B]  kernel_len  (u32 LE)
/// [N B]  kernel_data (VALK-encoded KernelState)
/// [4 B]  meta_len    (u32 LE)
/// [M B]  meta_data
/// [4 B]  index_len   (u32 LE)
/// [K B]  index_data
/// ```
pub fn parse_kernel_from_snapshot_bytes(data: &[u8]) -> Result<KernelState> {
    if data.len() < 12 {
        bail!("Snapshot is too short ({} bytes)", data.len());
    }
    if &data[0..4] != SNAPSHOT_MAGIC {
        bail!(
            "Invalid snapshot magic: expected {:?}, got {:?}",
            SNAPSHOT_MAGIC,
            &data[0..4]
        );
    }

    let k_len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if 8 + k_len > data.len() {
        bail!(
            "Snapshot kernel section truncated (need {} bytes, have {})",
            8 + k_len,
            data.len()
        );
    }

    decode_state(&data[8..8 + k_len])
        .map_err(|e| anyhow::anyhow!("KernelState decode error: {e:?}"))
}

/// Read the snapshot magic and section lengths without fully decoding the
/// state.  Cheap structural check — suitable for the `inspect` command.
pub fn inspect_snapshot_bytes(data: &[u8]) -> Result<SnapshotInfo> {
    if data.len() < 4 {
        bail!(
            "File is too short to be a Valori snapshot ({} bytes)",
            data.len()
        );
    }

    let magic_ok = &data[0..4] == SNAPSHOT_MAGIC;

    if !magic_ok || data.len() < 12 {
        return Ok(SnapshotInfo {
            magic_ok,
            kernel_len: 0,
            metadata_len: 0,
            index_len: 0,
            total_size: data.len(),
        });
    }

    let k_len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let mut cursor = 8 + k_len;

    let metadata_len = if cursor + 4 <= data.len() {
        let v = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4 + v;
        v
    } else {
        0
    };

    let index_len = if cursor + 4 <= data.len() {
        u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize
    } else {
        0
    };

    Ok(SnapshotInfo {
        magic_ok,
        kernel_len: k_len,
        metadata_len,
        index_len,
        total_size: data.len(),
    })
}

/// Lightweight structural summary returned by [`inspect_snapshot_bytes`].
#[derive(Debug)]
pub struct SnapshotInfo {
    pub magic_ok: bool,
    pub kernel_len: usize,
    pub metadata_len: usize,
    pub index_len: usize,
    pub total_size: usize,
}
