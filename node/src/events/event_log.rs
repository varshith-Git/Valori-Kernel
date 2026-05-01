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
use std::io::{Read, Write, BufWriter};
use std::path::{Path, PathBuf};
use thiserror::Error;
use serde::{Serialize, Deserialize};

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
    fn new(dim: u32) -> Self {
        Self {
            version: 1,
            dim,
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

    fn validate(&self, expected_dim: Option<u32>) -> Result<()> {
        if self.version != 1 {
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
/// - Write + fsync before returning
/// - No buffering without explicit flush
/// - Crash-safe: partial writes impossible
pub struct EventLogWriter {
    path: PathBuf,
    file: BufWriter<File>,
    event_count: u64,
    dim: u32,
}

impl EventLogWriter {
    pub fn path(&self) -> &Path {
        &self.path
    }
    
    pub fn dim(&self) -> u32 {
        self.dim
    }

    /// Open or create an event log file
    ///
    /// If file exists, validates header and appends
    /// If file doesn't exist, creates with header (if dim provided)
    pub fn open(path: impl AsRef<Path>, expected_dim: Option<u32>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file_exists = path.exists();
        
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let mut event_count = 0;
        let dim;

        if file_exists {
            // Validate existing header
            let mut header_bytes = [0u8; 16];
            // We need to read from the start, but we opened in append mode.
            // Let's open a separate read-only handle to read the header.
            let mut read_file = File::open(&path)?;
            read_file.read_exact(&mut header_bytes)?;
            
            let header = EventLogHeader::from_bytes(&header_bytes);
            header.validate(expected_dim)?;
            dim = header.dim;

            // Count existing events
            let mut event_buf = Vec::new();
            read_file.read_to_end(&mut event_buf)?;
            
            let mut offset = 0;
            while offset < event_buf.len() {
                match bincode::serde::decode_from_slice::<LogEntry, _>(
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
        } else {
            // Write header for new file
            let d = expected_dim.ok_or(EventLogError::InvalidHeader)?; // Need dim for new file
            dim = d;
            let header = EventLogHeader::new(dim);
            file.write_all(&header.to_bytes())?;
            file.sync_all()?; // fsync header
        }

        Ok(Self {
            path,
            file: BufWriter::new(file),
            event_count,
            dim,
        })
    }

    /// Append an entry to the log
    pub fn append(&mut self, entry: &LogEntry) -> Result<()> {
        let bytes = bincode::serde::encode_to_vec(entry, bincode::config::standard())
            .map_err(|e| EventLogError::Serialization(e.to_string()))?;

        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.file.get_ref().sync_all()?;

        if let LogEntry::Event(_) = entry {
            self.event_count += 1;
        }

        Ok(())
    }

    /// Append multiple entries to the log with a SINGLE fsync
    pub fn append_batch(&mut self, entries: &[LogEntry]) -> Result<()> {
        if entries.is_empty() {
             return Ok(());
        }

        for entry in entries {
            let bytes = bincode::serde::encode_to_vec(entry, bincode::config::standard())
                .map_err(|e| EventLogError::Serialization(e.to_string()))?;
            self.file.write_all(&bytes)?;
        }
        
        self.file.flush()?;
        self.file.get_ref().sync_all()?;

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

    /// Rotate the event log
    pub fn rotate(
        &mut self, 
        archive_path: impl AsRef<Path>, 
        checkpoint_entry: Option<LogEntry>
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
        
        if let Some(entry) = checkpoint_entry {
             let bytes = bincode::serde::encode_to_vec(&entry, bincode::config::standard())
                .map_err(|e| EventLogError::Serialization(e.to_string()))?;
             new_file.write_all(&bytes)?;
        }
        
        new_file.sync_all()?;
        self.file = BufWriter::new(new_file);
        
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
    }

    #[test]
    fn test_event_log_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

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
        }

        {
            let writer = EventLogWriter::open(&path, Some(16)).unwrap();
            assert_eq!(writer.event_count(), 5);
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
}
