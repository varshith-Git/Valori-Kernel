// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! WAL Writer for Durable Command Logging
//!
//! This module provides write-ahead logging for valori-node,
//! enabling crash recovery and deterministic replay.

use valori_kernel::state::command::Command;
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::path::Path;
use thiserror::Error;

const WAL_VERSION: u8 = 1;

#[derive(Debug, Error)]
pub enum WalError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("WAL file corrupted")]
    Corrupted,
}

pub type WalResult<T> = Result<T, WalError>;

/// WAL Writer for appending commands to durable storage
pub struct WalWriter {
    file: BufWriter<File>,
    bytes_written: u64,
    version_written: bool,
}

impl WalWriter {
    /// Open or create a WAL file at the specified path
    pub fn open<P: AsRef<Path>>(path: P) -> WalResult<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        
        let bytes_written = file.metadata()?.len();
        let version_written = bytes_written > 0;
        
        let mut writer = Self {
            file: BufWriter::new(file),
            bytes_written,
            version_written,
        };
        
        // Write version byte immediately if this is a new file
        if !version_written {
            writer.file.write_all(&[WAL_VERSION])?;
            writer.file.flush()?;
            writer.bytes_written = 1;
            writer.version_written = true;
        }
        
        Ok(writer)
    }

    /// Append a command to the WAL
    /// 
    /// Format:
    /// - First write (if new file): WAL_VERSION (1 byte)
    /// - Then for each command: serialize via bincode (using serde)
    /// - Fsync after write for durability
    pub fn append_command<const D: usize>(
        &mut self,
        cmd: &Command<D>,
    ) -> WalResult<()> {
        // Version header already written in open()
        
        // Serialize command using bincode's serde mode for kernel compatibility
        // Command already has Serialize derive from serde
        let encoded = bincode::serde::encode_to_vec(cmd, bincode::config::standard())
            .map_err(|e| WalError::Serialization(e.to_string()))?;

        // Write length prefix (u32) for framing
        let len = encoded.len() as u32;
        self.file.write_all(&len.to_le_bytes())?;
        self.bytes_written += 4;

        // Write command data
        self.file.write_all(&encoded)?;
        self.bytes_written += encoded.len() as u64;

        // Flush to OS buffer
        self.file.flush()?;
        
        // Force fsync for durability guarantee
        self.file.get_ref().sync_all()?;

        Ok(())
    }

    /// Get total bytes written to WAL
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Manually flush and sync (called automatically on append_command)
    pub fn sync(&mut self) -> WalResult<()> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_wal_writer_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        
        let writer = WalWriter::open(&path).unwrap();
        assert!(path.exists());
        assert_eq!(writer.bytes_written(), 1); // Version byte written on creation
    }

    #[test]
    fn test_wal_writer_appends_command() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        
        let mut writer = WalWriter::open(&path).unwrap();
        
        let cmd = Command::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::<16>::new_zeros(),
        };
        
        writer.append_command(&cmd).unwrap();
        assert!(writer.bytes_written() > 1); // Version byte + command data
        
        // Verify file on disk
        let contents = fs::read(&path).unwrap();
        assert_eq!(contents[0], WAL_VERSION);
    }

    #[test]
    fn test_wal_writer_multiple_commands() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        
        let mut writer = WalWriter::open(&path).unwrap();
        
        for i in 0..10 {
            let cmd = Command::InsertRecord {
                id: RecordId(i),
                vector: FxpVector::<16>::new_zeros(),
            };
            writer.append_command(&cmd).unwrap();
        }
        
        let bytes = writer.bytes_written();
        assert!(bytes > 100); // Should have substantial data
    }
}
