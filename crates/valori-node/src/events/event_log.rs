// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Append-Only Event Log Writer
//!
//! This is the CANONICAL durability layer.
//! - Events are written to disk BEFORE memory application
//! - Every write is fsync'd for crash safety
//! - No truncation or rewriting allowed
//! - Bincode serialization for determinism
//!
//! The on-disk format is defined ONCE in the `valori-wire` crate (shared
//! with `valori-verify` and `valori-cli`). This module only owns the
//! durability mechanics: open/restore, append+fsync, batch, rotation.
//!
//! ## Versions
//! - New files are written as **v3**: 48-byte header carrying the
//!   arithmetic format id, the segment sequence number, and the previous
//!   segment's final chain head (so rotated segments splice into one
//!   continuous chain instead of restarting from zeros).
//! - Existing **v2** files keep appending v2 entries; the first rotation
//!   upgrades the live segment to v3 and splices the chain.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write, BufWriter};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub use valori_wire::{DecodedEntry, EntryV2, EntryV3, LogEntry, SegmentHeader};
use valori_wire::{
    chain_advance, decode_entry, encode_entry, encode_header_v3, parse_header,
    FORMAT_Q16_16, VERSION_V3,
};

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

    #[error("Wire format error: {0}")]
    Wire(#[from] valori_wire::WireError),
}

pub type Result<T> = std::result::Result<T, EventLogError>;

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
    /// Wire version of the CURRENT segment (v2 legacy or v3).
    version: u32,
    /// Sequence number of the current segment (0 = genesis).
    segment_seq: u32,
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

    /// Wire version of the current segment.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Sequence number of the current segment.
    pub fn segment_seq(&self) -> u32 {
        self.segment_seq
    }

    /// Current BLAKE3 chain head — covers every durably written entry.
    pub fn chain_head(&self) -> &[u8; 32] {
        &self.chain_head
    }

    /// Open or create an event log file.
    ///
    /// If the file exists (v2 or v3), validates the header, decodes existing
    /// entries to restore `event_count` and `chain_head`, then opens in
    /// append mode. If the file doesn't exist, creates it with a fresh v3
    /// header (requires `expected_dim`).
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
        let version;
        let mut segment_seq = 0u32;

        if file_exists {
            let mut read_file = File::open(&path)?;
            let mut buf = Vec::new();
            read_file.read_to_end(&mut buf)?;

            let header = parse_header(&buf).map_err(|_| EventLogError::InvalidHeader)?;
            if let Some(expected) = expected_dim {
                if header.dim != expected {
                    return Err(EventLogError::DimensionMismatch {
                        expected,
                        found: header.dim,
                    });
                }
            }
            dim = header.dim;
            version = header.version;
            segment_seq = header.segment_seq;
            // v3 segments continue the chain from the previous segment's
            // final head (recorded in the header); v2 starts from zeros.
            chain_head = header.prev_segment_chain_head;

            let mut offset = header.header_len;
            while offset < buf.len() {
                match decode_entry(version, &buf[offset..]) {
                    Ok((decoded, bytes_read)) => {
                        chain_head = chain_advance(version, &chain_head, &decoded)?;
                        match decoded.entry {
                            LogEntry::Event(_) => event_count += 1,
                            LogEntry::Checkpoint { event_count: c, .. } => event_count = c,
                        }
                        offset += bytes_read;
                    }
                    // Trailing partial entry from a mid-write crash — replay
                    // stops here; the unacknowledged tail is ignored.
                    Err(_) => break,
                }
            }
        } else {
            let d = expected_dim.ok_or(EventLogError::InvalidHeader)?;
            dim = d;
            version = VERSION_V3;
            let header = encode_header_v3(dim, FORMAT_Q16_16, 0, &[0u8; 32]);
            file.write_all(&header)?;
            file.sync_all()?;
        }

        Ok(Self {
            path,
            file: BufWriter::new(file),
            event_count,
            dim,
            version,
            segment_seq,
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

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Append an entry to the log, durably.
    ///
    /// Writes, flushes, and fsyncs before returning. Once this returns
    /// `Ok`, the entry survives a crash (including SIGKILL).
    /// One fsync per call — bulk loads should use `append_batch`.
    pub fn append(&mut self, entry: &LogEntry) -> Result<()> {
        self.append_with_request_id(entry, None)
    }

    /// Append with a client idempotency token (v3 segments only; the id is
    /// not representable in legacy v2 segments and is dropped there).
    /// Phase 2 Raft dedup is keyed on this id.
    pub fn append_with_request_id(
        &mut self,
        entry: &LogEntry,
        request_id: Option<[u8; 16]>,
    ) -> Result<()> {
        let now = Self::now_secs();
        let request_id = if self.version == VERSION_V3 { request_id } else { None };

        let bytes = encode_entry(self.version, &self.chain_head, now, request_id, entry)?;

        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.file.get_ref().sync_all()?;

        self.chain_head = chain_advance(
            self.version,
            &self.chain_head,
            &DecodedEntry {
                prev_hash: self.chain_head,
                wall_time_secs: now,
                request_id,
                entry: entry.clone(),
            },
        )?;
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
    /// All entries share one flush+fsync. Advances the chain head for
    /// each entry in order so chain integrity is maintained.
    pub fn append_batch(&mut self, entries: &[LogEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let now = Self::now_secs();

        let mut total_bytes = 0u64;
        for entry in entries {
            let bytes = encode_entry(self.version, &self.chain_head, now, None, entry)?;
            total_bytes += bytes.len() as u64;
            self.file.write_all(&bytes)?;
            self.chain_head = chain_advance(
                self.version,
                &self.chain_head,
                &DecodedEntry {
                    prev_hash: self.chain_head,
                    wall_time_secs: now,
                    request_id: None,
                    entry: entry.clone(),
                },
            )?;
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
    /// start a fresh v3 segment.
    ///
    /// The chain does NOT reset: the new segment's header records the
    /// closing chain head of the archived segment
    /// (`prev_segment_chain_head`), and entries continue from it. Deleting
    /// or substituting an archived segment breaks the splice — verifiers
    /// can prove the full multi-segment history is intact.
    ///
    /// Rotation is also the v2 → v3 upgrade point: a legacy segment is
    /// archived as-is and the new live segment is always v3.
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

        // Splice: the new segment opens where the archived one closed.
        let prev_head = self.chain_head;
        self.segment_seq += 1;
        self.version = VERSION_V3;

        let header = encode_header_v3(self.dim, FORMAT_Q16_16, self.segment_seq, &prev_head);
        new_file.write_all(&header)?;

        if let Some(entry) = checkpoint_entry {
            let now = Self::now_secs();
            let bytes = encode_entry(self.version, &self.chain_head, now, None, &entry)?;
            new_file.write_all(&bytes)?;
            self.chain_head = chain_advance(
                self.version,
                &self.chain_head,
                &DecodedEntry {
                    prev_hash: self.chain_head,
                    wall_time_secs: now,
                    request_id: None,
                    entry,
                },
            )?;
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
    use valori_kernel::event::KernelEvent;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;
    use valori_wire::chain_advance_v3;

    fn event(i: u32) -> KernelEvent {
        KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        }
    }

    #[test]
    fn test_event_log_create_and_append() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        let mut writer = EventLogWriter::open(&path, Some(16)).unwrap();
        assert_eq!(writer.version(), valori_wire::VERSION_V3, "new files are v3");
        assert_eq!(writer.segment_seq(), 0);

        writer.append(&LogEntry::Event(event(1))).unwrap();
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
                writer.append(&LogEntry::Event(event(i))).unwrap();
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
    fn test_legacy_v2_file_reopens_and_appends() {
        // A v2-era log (16-byte header + EntryV2 records) must keep working:
        // reopen restores the chain, appends continue in v2 shape.
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        // Hand-write a v2 file.
        let mut head = [0u8; 32];
        let mut bytes = valori_wire::encode_header_v2(16).to_vec();
        for i in 0..3u32 {
            let entry = LogEntry::Event(event(i));
            bytes.extend(
                valori_wire::encode_entry(valori_wire::VERSION_V2, &head, 1_000, None, &entry)
                    .unwrap(),
            );
            head = valori_wire::chain_advance_v2(&head, 1_000, &entry);
        }
        std::fs::write(&path, &bytes).unwrap();

        let mut writer = EventLogWriter::open(&path, Some(16)).unwrap();
        assert_eq!(writer.version(), valori_wire::VERSION_V2);
        assert_eq!(writer.event_count(), 3);
        assert_eq!(writer.chain_head(), &head);

        // Appends continue in the file's own (v2) format.
        writer.append(&LogEntry::Event(event(3))).unwrap();
        drop(writer);

        let reopened = EventLogWriter::open(&path, Some(16)).unwrap();
        assert_eq!(reopened.event_count(), 4, "v2 append must be replayable");
    }

    #[test]
    fn test_rotation_splices_chain_and_upgrades_to_v3() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");
        let archive = dir.path().join("events.log.1");

        let mut writer = EventLogWriter::open(&path, Some(16)).unwrap();
        for i in 0..4 {
            writer.append(&LogEntry::Event(event(i))).unwrap();
        }
        let head_before_rotation = *writer.chain_head();

        writer
            .rotate(
                &archive,
                Some(LogEntry::Checkpoint {
                    event_count: 4,
                    snapshot_hash: head_before_rotation,
                    timestamp: 0,
                }),
            )
            .unwrap();
        assert_eq!(writer.segment_seq(), 1);
        assert_ne!(
            writer.chain_head(),
            &[0u8; 32],
            "chain must continue across rotation, not reset"
        );

        writer.append(&LogEntry::Event(event(4))).unwrap();
        drop(writer);

        // New segment's header must record the splice point.
        let new_bytes = std::fs::read(&path).unwrap();
        let header = valori_wire::parse_header(&new_bytes).unwrap();
        assert_eq!(header.version, valori_wire::VERSION_V3);
        assert_eq!(header.segment_seq, 1);
        assert_eq!(
            header.prev_segment_chain_head, head_before_rotation,
            "header must bind the new segment to the archived one"
        );

        // Reopen restores the continued chain and the checkpointed count
        // (4 from the checkpoint + 1 appended after rotation).
        let reopened = EventLogWriter::open(&path, Some(16)).unwrap();
        assert_eq!(reopened.event_count(), 5);
        assert_eq!(reopened.segment_seq(), 1);
    }

    #[test]
    fn test_chain_head_deterministic() {
        // The chain hash covers (wall_time_secs, request_id, entry) — so
        // determinism is defined over identical inputs, not across
        // wall-clock writes. (Cross-replica equality is the STATE hash's
        // job; the chain head is per-file integrity.)
        let build = || {
            let mut head = [0u8; 32];
            for i in 0..10u32 {
                head = chain_advance_v3(&head, 1_750_000_000 + i as u64, None, &LogEntry::Event(event(i)));
            }
            head
        };

        let h1 = build();
        let h2 = build();
        assert_eq!(h1, h2, "same (time, request_id, entry) sequence must give same chain head");

        // A different timestamp for one entry must change the head.
        let mut head = [0u8; 32];
        for i in 0..10u32 {
            let t = if i == 5 { 999 } else { 1_750_000_000 + i as u64 };
            head = chain_advance_v3(&head, t, None, &LogEntry::Event(event(i)));
        }
        assert_ne!(h1, head, "timestamp change must alter the chain head");

        // A request id must also alter the chain.
        let mut head = [0u8; 32];
        for i in 0..10u32 {
            let rid = if i == 5 { Some([7u8; 16]) } else { None };
            head = chain_advance_v3(&head, 1_750_000_000 + i as u64, rid, &LogEntry::Event(event(i)));
        }
        assert_ne!(h1, head, "request id must be covered by the chain");
    }
}
