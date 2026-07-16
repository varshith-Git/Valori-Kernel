// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! WAL Writer — writes v2 format: (KernelEvent, namespace_id) bincode stream.
//!
//! Header: 16 bytes [Ver:4=2][Enc:4=0][Dim:4][Crc:4=0]
//! Payload: bincode stream of (KernelEvent, u16) pairs, no length prefix.
//!
//! Legacy v1 files (Command format) are readable by `WalReader` but cannot be
//! appended to by this writer. Delete or archive the v1 file before using this
//! writer with the same path.

use crate::wal_reader::WalHeader;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use thiserror::Error;
use valori_kernel::event::KernelEvent;

pub const WAL_VERSION: u32 = 2;

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

/// WAL Writer for appending events to durable storage (v2 format).
pub struct WalWriter {
    file: BufWriter<File>,
    bytes_written: u64,
    #[allow(dead_code)]
    dim: u32,
}

impl WalWriter {
    /// Open or create a WAL file. Returns an error if the existing file uses
    /// the legacy v1 (Command) format — delete or archive it before calling.
    pub fn open<P: AsRef<Path>>(path: P, dim: u32) -> WalResult<Self> {
        let path = path.as_ref();
        let exists = path.exists();

        let mut raw_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;

        let file_len = raw_file.metadata()?.len();

        let initial_bytes = if exists && file_len >= WalHeader::SIZE as u64 {
            // Detect version from existing header via a separate read handle.
            let mut head_buf = [0u8; WalHeader::SIZE];
            let mut read_handle = File::open(path)?;
            read_handle.read_exact(&mut head_buf)?;
            let version = u32::from_le_bytes([head_buf[0], head_buf[1], head_buf[2], head_buf[3]]);
            if version != WAL_VERSION {
                return Err(WalError::Validation(format!(
                    "Existing WAL file uses format v{version}; \
                     this writer only appends v{WAL_VERSION}. \
                     Delete or archive the file to start fresh."
                )));
            }
            file_len
        } else if file_len == 0 {
            // New file — write v2 header.
            let mut head_buf = [0u8; WalHeader::SIZE];
            head_buf[0..4].copy_from_slice(&WAL_VERSION.to_le_bytes());
            head_buf[4..8].copy_from_slice(&0u32.to_le_bytes()); // encoding_version
            head_buf[8..12].copy_from_slice(&dim.to_le_bytes());
            head_buf[12..16].copy_from_slice(&0u32.to_le_bytes()); // checksum_len
            raw_file.write_all(&head_buf)?;
            raw_file.flush()?;
            WalHeader::SIZE as u64
        } else {
            return Err(WalError::Validation(
                "Existing WAL file is too short to contain a valid header".into(),
            ));
        };

        Ok(Self {
            file: BufWriter::new(raw_file),
            bytes_written: initial_bytes,
            dim,
        })
    }

    /// Append a `KernelEvent` targeting `namespace_id` to the WAL.
    pub fn append_event(&mut self, event: &KernelEvent, namespace_id: u16) -> WalResult<()> {
        let config = bincode::config::standard();
        let len =
            bincode::serde::encode_into_std_write(&(event, namespace_id), &mut self.file, config)
                .map_err(|e| WalError::Serialization(e.to_string()))?;
        self.bytes_written += len as u64;
        self.file.flush()?;
        Ok(())
    }

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
    use tempfile::tempdir;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;

    #[test]
    fn test_wal_header_written() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_header.wal");
        let _writer = WalWriter::open(&path, 16).unwrap();
        let content = std::fs::read(&path).unwrap();
        assert_eq!(content.len(), 16);
        let version = u32::from_le_bytes(content[0..4].try_into().unwrap());
        assert_eq!(version, WAL_VERSION);
        let dim = u32::from_le_bytes(content[8..12].try_into().unwrap());
        assert_eq!(dim, 16);
    }

    #[test]
    fn test_append_event() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_append.wal");
        let mut writer = WalWriter::open(&path, 16).unwrap();
        let evt = KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };
        writer.append_event(&evt, 0).unwrap();
        writer.sync().unwrap();
        assert!(writer.bytes_written > 16);
    }

    #[test]
    fn test_reopen_existing_v2() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reopen.wal");
        {
            let mut w = WalWriter::open(&path, 16).unwrap();
            let evt = KernelEvent::InsertRecord {
                id: RecordId(0),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            };
            w.append_event(&evt, 0).unwrap();
        }
        // Re-opening a v2 file should succeed.
        let w2 = WalWriter::open(&path, 16);
        assert!(w2.is_ok());
    }
}
