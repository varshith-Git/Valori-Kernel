// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Append-Only Event Log Writer
//!
//! This is the CANONICAL durability layer.
//! - Events are written to disk BEFORE memory application
//! - Every write is fsync'd for crash safety
//! - No truncation or rewriting allowed
//! - Bincode serialization for determinism
//!
//! ## File layout (v2)
//! ```text
//! [Header: 16 bytes][ChainedEntry][ChainedEntry]...
//! ```
//! Header: version u32 LE (=2) | dim u32 LE | reserved u64 LE (=0)
//!
//! Each `ChainedEntry` = bincode of `{ prev_hash: [u8;32], wall_time_secs: u64, entry: LogEntry }`.
//!
//! ## Hash chain
//! `chain_hash[i] = BLAKE3(chain_hash[i-1] || bincode((wall_time_secs_i, entry_i)))`
//! Genesis: `chain_hash[-1] = [0u8; 32]`.
//! Any in-place edit to an entry shifts its chain hash, which breaks the
//! `prev_hash` field of the NEXT entry — giving verifiers exact event-level
//! tamper location.

use valori_kernel::event::KernelEvent;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, BufWriter};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use serde::{Serialize, Deserialize};

const LOG_VERSION: u32 = 2;

#[derive(Error, Debug)]
pub enum EventLogError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Invalid header")]
    InvalidHeader,

    #[error("Dimension mismatch: expected {expected}, found {found}")]
    DimensionMismatch { expected: u32, found: u32 },
}

/// Wrapper for persisted events to include metadata/checkpoints
/// without polluting the pure kernel event definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntry {
    Event(KernelEvent),
    Checkpoint {
        event_count: u64,
        snapshot_hash: [u8; 32],
        timestamp: u64,
    },
}

/// On-disk wrapper (v2 wire format).
///
/// Stored instead of a bare `LogEntry` so that the hash chain can be
/// verified without re-running the kernel state machine.
///
/// Public so log readers outside this crate (valori-cli forensics) decode
/// the real wire format — the single definition moves to `valori-wire` in
/// Phase 1.2 of the multi-node roadmap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainedEntry {
    /// BLAKE3 chain head immediately BEFORE this entry was written.
    /// For the first entry this is `[0u8; 32]`.
    pub prev_hash: [u8; 32],
    /// Unix timestamp (seconds) when this entry was appended.
    pub wall_time_secs: u64,
    pub entry: LogEntry,
}

pub type Result<T> = std::result::Result<T, EventLogError>;

/// Advance the running chain head by one entry.
/// `new_head = BLAKE3(prev_head || bincode((wall_time_secs, entry)))`
pub(crate) fn chain_advance(head: &[u8; 32], wall_time_secs: u64, entry: &LogEntry) -> [u8; 32] {
    let commit = bincode::serde::encode_to_vec(&(wall_time_secs, entry), bincode::config::standard())
        .expect("LogEntry is always serialisable");
    let mut hasher = blake3::Hasher::new();
    hasher.update(head);
    hasher.update(&commit);
    *hasher.finalize().as_bytes()
}

/// Event Log File Header (16 bytes)
#[repr(C)]
struct EventLogHeader {
    version: u32,
    dim: u32,
    reserved: u64,
}

impl EventLogHeader {
    fn new(dim: u32) -> Self {
        Self { version: LOG_VERSION, dim, reserved: 0 }
    }

    fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&self.version.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.dim.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.reserved.to_le_bytes());
        bytes
    }

    fn from_bytes(bytes: &[u8; 16]) -> Self {
        Self {
            version: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            dim: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            reserved: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        }
    }

    fn validate(&self, expected_dim: Option<u32>) -> Result<()> {
        if self.version != LOG_VERSION {
            return Err(EventLogError::InvalidHeader);
        }
        if let Some(expected) = expected_dim {
            if self.dim != expected {
                return Err(EventLogError::DimensionMismatch { expected, found: self.dim });
            }
        }
        Ok(())
    }
}

/// Append-Only Event Log Writer
///
/// # Safety Guarantees
/// - `append` and `append_batch` write + flush + fsync before returning;
///   committed entries survive a crash (including SIGKILL)
/// - `append_batch` performs a single fsync for the whole batch
/// - Recovery tolerates a trailing partial entry (a crash mid-write loses
///   only the in-flight, unacknowledged entry; replay stops at the first
///   undecodable record)
pub struct EventLogWriter {
    path: PathBuf,
    file: BufWriter<File>,
    event_count: u64,
    dim: u32,
    /// Running BLAKE3 chain head (reflects every durably written entry).
    chain_head: [u8; 32],
    /// Bytes written since last rotation (header not counted).
    bytes_written: u64,
}

impl EventLogWriter {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dim(&self) -> u32 {
        self.dim
    }

    /// Current BLAKE3 chain head — covers every durably written entry.
    pub fn chain_head(&self) -> &[u8; 32] {
        &self.chain_head
    }

    /// Open or create an event log file.
    ///
    /// If the file exists, validates the v2 header, decodes existing
    /// `ChainedEntry` records to restore `event_count` and `chain_head`,
    /// then opens in append mode.  If the file doesn't exist, creates it
    /// with a fresh v2 header (requires `expected_dim`).
    pub fn open(path: impl AsRef<Path>, expected_dim: Option<u32>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file_exists = path.exists();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let mut event_count = 0u64;
        let mut chain_head = [0u8; 32];
        let dim;

        if file_exists {
            let mut read_file = File::open(&path)?;
            let mut header_bytes = [0u8; 16];
            read_file.read_exact(&mut header_bytes)?;

            let header = EventLogHeader::from_bytes(&header_bytes);
            header.validate(expected_dim)?;
            dim = header.dim;

            let mut event_buf = Vec::new();
            read_file.read_to_end(&mut event_buf)?;

            let mut offset = 0;
            while offset < event_buf.len() {
                match bincode::serde::decode_from_slice::<ChainedEntry, _>(
                    &event_buf[offset..],
                    bincode::config::standard(),
                ) {
                    Ok((chained, bytes_read)) => {
                        // Restore chain head by re-advancing through every entry.
                        chain_head = chain_advance(
                            &chained.prev_hash,
                            chained.wall_time_secs,
                            &chained.entry,
                        );
                        match chained.entry {
                            LogEntry::Event(_) => event_count += 1,
                            LogEntry::Checkpoint { event_count: c, .. } => event_count = c,
                        }
                        offset += bytes_read;
                    }
                    Err(_) => break,
                }
            }
        } else {
            let d = expected_dim.ok_or(EventLogError::InvalidHeader)?;
            dim = d;
            let header = EventLogHeader::new(dim);
            file.write_all(&header.to_bytes())?;
            file.sync_all()?;
        }

        Ok(Self {
            path,
            file: BufWriter::new(file),
            event_count,
            dim,
            chain_head,
            bytes_written: 0,
        })
    }

    /// Returns how many bytes have been written since last rotation.
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    fn reset_bytes_written(&mut self) {
        self.bytes_written = 0;
    }

    /// Append an entry to the log, durably.
    ///
    /// Wraps the entry in a `ChainedEntry` (with current chain head and wall
    /// clock), writes, flushes, and fsyncs before returning.  Once this
    /// returns `Ok`, the entry survives a crash (including SIGKILL).
    /// One fsync per call — bulk loads should use `append_batch`.
    pub fn append(&mut self, entry: &LogEntry) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let chained = ChainedEntry {
            prev_hash: self.chain_head,
            wall_time_secs: now,
            entry: entry.clone(),
        };

        let bytes = bincode::serde::encode_to_vec(&chained, bincode::config::standard())
            .map_err(|e| EventLogError::Serialization(e.to_string()))?;

        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.file.get_ref().sync_all()?;

        self.chain_head = chain_advance(&chained.prev_hash, now, entry);
        self.bytes_written += bytes.len() as u64;

        if let LogEntry::Event(_) = entry {
            self.event_count += 1;
        }

        Ok(())
    }

    /// Explicitly flush the buffer to disk (no-op if already fsynced per entry).
    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(())
    }

    /// Append multiple entries with a SINGLE fsync.
    ///
    /// All entries share one flush+fsync.  Advances the chain head for
    /// each entry in order so chain integrity is maintained.
    pub fn append_batch(&mut self, entries: &[LogEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut total_bytes = 0u64;
        for entry in entries {
            let chained = ChainedEntry {
                prev_hash: self.chain_head,
                wall_time_secs: now,
                entry: entry.clone(),
            };
            let bytes = bincode::serde::encode_to_vec(&chained, bincode::config::standard())
                .map_err(|e| EventLogError::Serialization(e.to_string()))?;
            total_bytes += bytes.len() as u64;
            self.file.write_all(&bytes)?;
            self.chain_head = chain_advance(&chained.prev_hash, now, entry);
        }

        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        self.bytes_written += total_bytes;

        for entry in entries {
            if let LogEntry::Event(_) = entry {
                self.event_count += 1;
            }
        }

        Ok(())
    }

    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Rotate the event log — flush, rename current to `archive_path`,
    /// start a fresh file with a new header and optional checkpoint entry.
    /// Resets the chain head to the zero state for the new log segment.
    pub fn rotate(
        &mut self,
        archive_path: impl AsRef<Path>,
        checkpoint_entry: Option<LogEntry>,
    ) -> Result<()> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;

        std::fs::rename(&self.path, archive_path)?;

        let mut new_file = OpenOptions::new()
            .create(true)
            .write(true)
            .create_new(true)
            .open(&self.path)?;

        let header = EventLogHeader::new(self.dim);
        new_file.write_all(&header.to_bytes())?;

        // Reset chain head for the new segment.
        self.chain_head = [0u8; 32];

        if let Some(entry) = checkpoint_entry {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let chained = ChainedEntry {
                prev_hash: self.chain_head,
                wall_time_secs: now,
                entry: entry.clone(),
            };
            let bytes = bincode::serde::encode_to_vec(&chained, bincode::config::standard())
                .map_err(|e| EventLogError::Serialization(e.to_string()))?;
            new_file.write_all(&bytes)?;
            self.chain_head = chain_advance(&chained.prev_hash, now, &entry);
        }

        new_file.sync_all()?;
        self.file = BufWriter::new(new_file);
        self.reset_bytes_written();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;

    #[test]
    fn test_event_log_create_and_append() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        let mut writer = EventLogWriter::open(&path, Some(16)).unwrap();

        let event = KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };

        writer.append(&LogEntry::Event(event)).unwrap();
        assert_eq!(writer.event_count(), 1);
        assert_ne!(writer.chain_head(), &[0u8; 32], "chain head must advance after first append");
    }

    #[test]
    fn test_event_log_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        let chain_after_write;
        {
            let mut writer = EventLogWriter::open(&path, Some(16)).unwrap();
            for i in 0..5 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::new_zeros(16),
                    metadata: None,
                    tag: 0,
                };
                writer.append(&LogEntry::Event(event)).unwrap();
            }
            chain_after_write = *writer.chain_head();
        }

        {
            let writer = EventLogWriter::open(&path, Some(16)).unwrap();
            assert_eq!(writer.event_count(), 5);
            assert_eq!(
                writer.chain_head(), &chain_after_write,
                "chain head must be restored exactly on reopen"
            );
        }
    }

    #[test]
    fn test_event_log_dimension_validation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        {
            let _writer = EventLogWriter::open(&path, Some(16)).unwrap();
        }

        let result = EventLogWriter::open(&path, Some(32));
        assert!(result.is_err());
    }

    #[test]
    fn test_chain_head_deterministic() {
        let dir = tempdir().unwrap();
        let p1 = dir.path().join("a.log");
        let p2 = dir.path().join("b.log");

        let write_n = |path: &std::path::Path| {
            let mut w = EventLogWriter::open(path, Some(4)).unwrap();
            for i in 0..10u32 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::new_zeros(4),
                    metadata: None,
                    tag: 0,
                };
                // Use append_batch so wall_time_secs is identical (same `now` call).
                w.append_batch(&[LogEntry::Event(event)]).unwrap();
            }
            *w.chain_head()
        };

        // Same events → chain head should be identical.
        let h1 = write_n(&p1);
        let h2 = write_n(&p2);
        assert_eq!(h1, h2);
    }
}
