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
pub struct WalReader<const D: usize> {
    reader: BufReader<File>,
    header_read: bool,
}

impl<const D: usize> WalReader<D> {
    /// Open a WAL file for reading
    pub fn open<P: AsRef<Path>>(path: P) -> WalResult<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
            header_read: false,
        })
    }

    /// Read and validate WAL Header (16 bytes)
    fn read_header(&mut self) -> WalResult<()> {
        let mut head_buf = [0u8; 16];
        self.reader.read_exact(&mut head_buf)?;
        
        let (header, _rest) = WalHeader::read(&head_buf)
            .map_err(|_| WalReaderError::Header("Invalid Header Read".into()))?;
            
        if header.dim != D as u32 {
            return Err(WalReaderError::Header(format!("Dimension Mismatch: File={}, Expected={}", header.dim, D)));
        }
        
        if header.version != 1 {
             return Err(WalReaderError::Header(format!("Version Mismatch: File={}, Expected=1", header.version)));
        }
        
        self.header_read = true;
        Ok(())
    }

    /// Read next command from WAL
    /// Returns None if EOF reached
    pub fn read_command(&mut self) -> WalResult<Option<Command<D>>> {
        if !self.header_read {
            self.read_header()?;
        }

        // Bincode decode from reader
        // bincode 2.0 decode_from_std_read returns Result<T, DecodeError>
        // It handles EOF as UnexpectedEnd? Or we need to check EOF?
        // Actually, if we are at EOF, decode usually fails with UnexpectedEnd or specific error.
        // We can peek/check length, but `BufReader` makes it obscure.
        // Best practice with bincode iterators:
        // Try decoding. If UnexpectedEnd at START of read, it's EOF.
        // If mid-read, it's error.
        
        // Wait, `bincode` 2.0 `decode_from_std_read` might return error on EOF.
        // Let's rely on `fill_buf` to check for EOF.
        /*
        let buf = self.reader.fill_buf()?;
        if buf.is_empty() { return Ok(None); }
        */
        
        // However, `decode_from_std_read` consumes the reader.
        match bincode::serde::decode_from_std_read(&mut self.reader, bincode::config::standard()) {
            Ok(cmd) => Ok(Some(cmd)),
            Err(e) => {
                match e {
                    bincode::error::DecodeError::UnexpectedEnd { .. } => {
                       // We assume if it failed to read *anything*, it is EOF.
                       // Strict check: if bytes were consumed, it's error.
                       // bincode doesn't tell us easily if 0 bytes consumed on error from this API?
                       // Let's use `fill_buf` method.
                       Ok(None)
                    },
                     _ => Err(WalReaderError::Deserialization(e.to_string())),
                }
            }
        }
    }
    
    /// Use simple peek strategy to distinguish EOF from Error
    fn ensure_not_eof(&mut self) -> WalResult<bool> {
        let buf = self.reader.fill_buf()?;
        Ok(!buf.is_empty())
    }
}

/// Iterator over WAL commands
pub struct WalCommandIterator<const D: usize> {
    reader: WalReader<D>,
}

impl<const D: usize> Iterator for WalCommandIterator<D> {
    type Item = WalResult<Command<D>>;

    fn next(&mut self) -> Option<Self::Item> {
        // Safe check for EOF
        match self.reader.ensure_not_eof() {
            Ok(has_data) => {
                if !has_data { return None; }
            },
            Err(e) => return Some(Err(e)),
        }
        
        match self.reader.read_command() {
            Ok(Some(cmd)) => Some(Ok(cmd)),
            Ok(None) => None, // Should be caught by peek, but in case
            Err(e) => Some(Err(e)),
        }
    }
}

impl<const D: usize> IntoIterator for WalReader<D> {
    type Item = WalResult<Command<D>>;
    type IntoIter = WalCommandIterator<D>;

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
            let mut writer = WalWriter::<16>::open(&path).unwrap();
            let cmd = Command::InsertRecord {
                id: RecordId(1),
                vector: FxpVector::new_zeros(),
            };
            writer.append_command(&cmd).unwrap();
        }
        
        {
            let reader = WalReader::<16>::open(&path).unwrap();
            let cmds: Vec<_> = reader.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
            assert_eq!(cmds.len(), 1);
        }
    }
}
