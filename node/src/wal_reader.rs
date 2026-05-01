// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! WAL Reader for Command Replay
//!
//! Unified Bincode Protocol (Phase 20).

use valori_kernel::state::command::Command;
use valori_kernel::replay::WalHeader;
use std::fs::File;
use std::io::{Read, BufReader, BufRead};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WalReaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    
    #[error("Header error: {0}")]
    Header(String),
}

pub type WalResult<T> = Result<T, WalReaderError>;

/// WAL Reader for replaying commands
pub struct WalReader {
    reader: BufReader<File>,
    header_read: bool,
    expected_dim: Option<u32>,
}

impl WalReader {
    /// Open a WAL file for reading
    pub fn open<P: AsRef<Path>>(path: P, expected_dim: Option<u32>) -> WalResult<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
            header_read: false,
            expected_dim,
        })
    }

    /// Read and validate WAL Header (16 bytes)
    fn read_header(&mut self) -> WalResult<()> {
        let mut head_buf = [0u8; 16];
        self.reader.read_exact(&mut head_buf)?;
        
        let (header, _rest) = WalHeader::read(&head_buf)
            .map_err(|_| WalReaderError::Header("Invalid Header Read".into()))?;
            
        if let Some(expected) = self.expected_dim {
            if header.dim != expected {
                return Err(WalReaderError::Header(format!("Dimension Mismatch: File={}, Expected={}", header.dim, expected)));
            }
        }
        
        if header.version != 1 {
             return Err(WalReaderError::Header(format!("Version Mismatch: File={}, Expected=1", header.version)));
        }
        
        self.header_read = true;
        Ok(())
    }

    /// Read next command from WAL
    pub fn read_command(&mut self) -> WalResult<Option<Command>> {
        if !self.header_read {
            self.read_header()?;
        }

        match bincode::serde::decode_from_std_read(&mut self.reader, bincode::config::standard()) {
            Ok(cmd) => Ok(Some(cmd)),
            Err(e) => {
                match e {
                    bincode::error::DecodeError::UnexpectedEnd { .. } => {
                       Ok(None)
                    },
                     _ => Err(WalReaderError::Deserialization(e.to_string())),
                }
            }
        }
    }
    
    fn ensure_not_eof(&mut self) -> WalResult<bool> {
        let buf = self.reader.fill_buf()?;
        Ok(!buf.is_empty())
    }
}

/// Iterator over WAL commands
pub struct WalCommandIterator {
    reader: WalReader,
}

impl Iterator for WalCommandIterator {
    type Item = WalResult<Command>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.ensure_not_eof() {
            Ok(has_data) => {
                if !has_data { return None; }
            },
            Err(e) => return Some(Err(e)),
        }
        
        match self.reader.read_command() {
            Ok(Some(cmd)) => Some(Ok(cmd)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

impl IntoIterator for WalReader {
    type Item = WalResult<Command>;
    type IntoIter = WalCommandIterator;

    fn into_iter(self) -> Self::IntoIter {
        WalCommandIterator { reader: self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal_writer::WalWriter;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;

    #[test]
    fn test_wal_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("roundtrip.wal");
        
        {
            let mut writer = WalWriter::open(&path, 16).unwrap();
            let cmd = Command::InsertRecord {
                id: RecordId(1),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            };
            writer.append_command(&cmd).unwrap();
        }
        
        {
            let reader = WalReader::open(&path, Some(16)).unwrap();
            let cmds: Vec<_> = reader.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
            assert_eq!(cmds.len(), 1);
        }
    }
}
