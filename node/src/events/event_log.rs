// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Append-Only Event Log Writer
//!
//! This is the CANONICAL durability layer.
//! - Events are written to disk BEFORE memory application
//! - Every write is fsync'd for crash safety
//! - No truncation or rewriting allowed
//! - Bincode serialization for determinism
//!
//! # File Format
//! ```
//! [Header: 16 bytes][Event][Event][Event]...
//! ```
//!
//! Header:
//! - version: u32 (1)
//! - dim: u32
//! - reserved: u64 (0)

use valori_kernel::event::KernelEvent;
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EventLogError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Invalid header")]
    InvalidHeader,
}

// use valori_kernel::event::KernelEvent; // Removed duplicate
use serde::{Serialize, Deserialize};

/// Wrapper for persisted events to include metadata/checkpoints
/// without polluting the pure kernel event definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntry<const D: usize> {
    Event(KernelEvent<D>),
    Checkpoint {
        event_count: u64,
        snapshot_hash: [u8; 32],
        timestamp: u64,
    }
}

pub type Result<T> = std::result::Result<T, EventLogError>;

/// Event Log File Header (16 bytes)
#[repr(C)]
struct EventLogHeader {
    version: u32,
    dim: u32,
    reserved: u64,
}

impl EventLogHeader {
    fn new(dim: usize) -> Self {
        Self {
            version: 1,
            dim: dim as u32,
            reserved: 0,
        }
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

    fn validate<const D: usize>(&self) -> Result<()> {
        if self.version != 1 {
            return Err(EventLogError::InvalidHeader);
        }
        if self.dim != D as u32 {
            return Err(EventLogError::InvalidHeader);
        }
        Ok(())
    }
}

/// Append-Only Event Log Writer
///
/// # Safety Guarantees
/// - Write + fsync before returning
/// - No buffering without explicit flush
/// - Crash-safe: partial writes impossible
pub struct EventLogWriter<const D: usize> {
    path: PathBuf,
    file: BufWriter<File>,
    event_count: u64,
}

impl<const D: usize> EventLogWriter<D> {
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Open or create an event log file
    ///
    /// If file exists, validates header and appends
    /// If file doesn't exist, creates with header
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        
        let file_exists = path.exists();
        
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let mut event_count = 0;

        if file_exists {
            // Validate existing header
            use std::io::Read;
            let mut header_bytes = [0u8; 16];
            file.read_exact(&mut header_bytes)?;
            
            let header = EventLogHeader::from_bytes(&header_bytes);
            header.validate::<D>()?;

            // Count existing events (for proof generation)
            // This is a simple scan - could be optimized with metadata file
            let mut event_buf = Vec::new();
            while let Ok(_) = file.read_to_end(&mut event_buf) {
                // Count events by attempting deserialization
                let mut offset = 0;
                while offset < event_buf.len() {
                    match bincode::serde::decode_from_slice::<LogEntry<D>, _>(
                        &event_buf[offset..],
                        bincode::config::standard()
                    ) {
                        Ok((entry, bytes_read)) => {
                            match entry {
                                LogEntry::Event(_) => event_count += 1,
                                LogEntry::Checkpoint { event_count: c, .. } => event_count = c,
                            }
                            offset += bytes_read;
                        }
                        Err(_) => break,
                    }
                }
                break;
            }
        } else {
            // Write header for new file
            let header = EventLogHeader::new(D);
            file.write_all(&header.to_bytes())?;
            file.sync_all()?; // fsync header
        }

        Ok(Self {
            path,
            file: BufWriter::new(file),
            event_count,
        })
    }

    /// Append an entry to the log
    ///
    /// # Safety
    /// - Serializes entry with bincode
    /// - Writes to buffer
    /// - Flushes buffer
    /// - fsync's file handle
    ///
    /// Only returns Ok() after durable write
    pub fn append(&mut self, entry: &LogEntry<D>) -> Result<()> {
        // Serialize entry
        let bytes = bincode::serde::encode_to_vec(entry, bincode::config::standard())
            .map_err(|e| EventLogError::Serialization(e.to_string()))?;

        // Write to buffer
        self.file.write_all(&bytes)?;
        
        // Flush buffer to OS
        self.file.flush()?;
        
        // Force fsync (critical for crash safety)
        self.file.get_ref().sync_all()?;

        // Increment count only for actual events (not checkpoints)
        if let LogEntry::Event(_) = entry {
            self.event_count += 1;
        }

        Ok(())
    }

    /// Append multiple entries to the log with a SINGLE fsync
    ///
    /// This provides atomicity for batches: either all specific bytes are physically on disk
    /// (after fsync return) or we crash before fsync returns (and they might not be).
    ///
    /// Note: If a partial write happens (less than full batch), the log recovery
    /// logic must handle truncation of incomplete tail writes.
    pub fn append_batch(&mut self, entries: &[LogEntry<D>]) -> Result<()> {
        if entries.is_empty() {
             return Ok(());
        }

        for entry in entries {
            let bytes = bincode::serde::encode_to_vec(entry, bincode::config::standard())
                .map_err(|e| EventLogError::Serialization(e.to_string()))?;
            self.file.write_all(&bytes)?;
        }
        
        // Flush buffer once
        self.file.flush()?;
        
        // Force fsync once
        self.file.get_ref().sync_all()?;

        // Update counts
        for entry in entries {
            if let LogEntry::Event(_) = entry {
                self.event_count += 1;
            }
        }
        
        Ok(())
    }

    /// Get the number of events written
    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Get the log file path
    /// Rotate the event log
    ///
    /// 1. Rename current file to `archive_path`
    /// 2. Create new file at `self.path`
    /// 3. Write checkpoint header to new file
    pub fn rotate(
        &mut self, 
        archive_path: impl AsRef<Path>, 
        checkpoint_entry: Option<LogEntry<D>>
    ) -> Result<()> {
        // 1. Sync current file
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        
        // 2. Rename (Atomic on POSIX)
        // We need to close the file handle first? 
        // On Unix, we can rename while open, but best to drop writer first or just rename.
        // But self.file owns the FD.
        // We can just rename the path. The struct holds a bufwriter to File.
        
        // Actually, renaming the path while we hold a File handle points to the OLD file (which is good).
        std::fs::rename(&self.path, archive_path)?;
        
        // 3. Open NEW file at original path
        let mut new_file = OpenOptions::new()
            .create(true)
            .write(true) // Truncate if exists? Should not exist if we just renamed it.
            .create_new(true) // Ensure we don't overwrite if race condition
            .open(&self.path)?;
            
        // 4. Write Header to NEW file
        let header = EventLogHeader::new(D);
        new_file.write_all(&header.to_bytes())?;
        
        // 5. Write Checkpoint if provided
        if let Some(entry) = checkpoint_entry {
             let bytes = bincode::serde::encode_to_vec(&entry, bincode::config::standard())
                .map_err(|e| EventLogError::Serialization(e.to_string()))?;
             new_file.write_all(&bytes)?;
        }
        
        new_file.sync_all()?;
        
        // 6. Replace handle
        self.file = BufWriter::new(new_file);
        
        // Keep event_count monotonically increasing?
        // Or does rotation reset specific file count?
        // The event_count field in this struct tracks TOTAL events if we want total history.
        // But if we archived the old events, should this reset?
        // The Checkpoint entry records the total count so far.
        // If we keep appending to `self.event_count`, it tracks global height.
        // That seems correct.
        
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

        let mut writer = EventLogWriter::<16>::open(&path).unwrap();

        let event = KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::<16>::new_zeros(),
            metadata: None,
            tag: 0,
        };

        writer.append(&LogEntry::Event(event)).unwrap();

        assert_eq!(writer.event_count(), 1);
    }

    #[test]
    fn test_event_log_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        // Write some events
        {
            let mut writer = EventLogWriter::<16>::open(&path).unwrap();
            for i in 0..5 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::<16>::new_zeros(),
                    metadata: None,
                    tag: 0,
                };
                writer.append(&LogEntry::Event(event)).unwrap();
            }
        }

        // Reopen and append more
        {
            let writer = EventLogWriter::<16>::open(&path).unwrap();
            assert_eq!(writer.event_count(), 5);
        }
    }

    #[test]
    fn test_event_log_dimension_validation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        // Create with D=16
        {
            let _writer = EventLogWriter::<16>::open(&path).unwrap();
        }

        // Attempt to open with D=32 should fail
        let result = EventLogWriter::<32>::open(&path);
        assert!(result.is_err());
    }
}
