// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! WAL Writer for Durable Command Logging
//!
//! Unified Bincode Protocol (Phase 20).
//! Header: 16 Bytes [Ver:4][Enc:4][Dim:4][Crc:4]
//! Payload: Bincode Stream (No Length Prefix)

use valori_kernel::state::command::Command;
use valori_kernel::replay::WalHeader;
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WalError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Validation error: {0}")]
    Validation(String),
}

pub type WalResult<T> = Result<T, WalError>;

/// WAL Writer for appending commands to durable storage
pub struct WalWriter {
    file: BufWriter<File>,
    bytes_written: u64,
    dim: u32,
}

impl WalWriter {
    /// Open or create a WAL file at the specified path
    pub fn open<P: AsRef<Path>>(path: P, dim: u32) -> WalResult<Self> {
        let path = path.as_ref();
        let exists = path.exists();
        
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;
            
        let mut bytes_written = file.metadata()?.len();
        
        if exists && bytes_written > 0 {
             if bytes_written < WalHeader::SIZE as u64 {
                 return Err(WalError::Validation("Existing WAL too short for header".into()));
             }
             // For safety, let's just append. If it's invalid, reader will catch it.
        } else {
             // New File: Write Header
             let header = WalHeader {
                 version: 1,
                 encoding_version: 0,
                 dim,
                 checksum_len: 0,
             };
             
             let mut head_buf = [0u8; 16];
             head_buf[0..4].copy_from_slice(&header.version.to_le_bytes());
             head_buf[4..8].copy_from_slice(&header.encoding_version.to_le_bytes());
             head_buf[8..12].copy_from_slice(&header.dim.to_le_bytes());
             head_buf[12..16].copy_from_slice(&header.checksum_len.to_le_bytes());
             
             file.write_all(&head_buf)?;
             file.flush()?; 
             bytes_written = 16;
        }
        
        Ok(Self {
            file: BufWriter::new(file),
            bytes_written,
            dim,
        })
    }

    /// Append a command to the WAL
    pub fn append_command(
        &mut self,
        cmd: &Command,
    ) -> WalResult<()> {
        let config = bincode::config::standard();
        
        let len = bincode::serde::encode_into_std_write(cmd, &mut self.file, config)
            .map_err(|e| WalError::Serialization(e.to_string()))?;
            
        self.bytes_written += len as u64;
        self.file.flush()?;
        
        Ok(())
    }

    /// Force sync to disk
    pub fn sync(&mut self) -> WalResult<()> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(())
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;

    #[test]
    fn test_wal_header_written() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_header.wal");
        
        let _writer = WalWriter::open(&path, 16).unwrap();
        
        let content = std::fs::read(&path).unwrap();
        assert_eq!(content.len(), 16);
        let dim = u32::from_le_bytes(content[8..12].try_into().unwrap());
        assert_eq!(dim, 16);
    }

    #[test]
    fn test_append_command() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_append.wal");
        let mut writer = WalWriter::open(&path, 16).unwrap();
        
        let cmd = Command::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };
        
        writer.append_command(&cmd).unwrap();
        writer.sync().unwrap();
        
        assert!(writer.bytes_written > 16);
    }
}
