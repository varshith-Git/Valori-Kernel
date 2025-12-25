// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! WAL Reader for Command Replay
//!
//! Reads WAL files and reconstructs command stream for deterministic replay.

use valori_kernel::state::command::Command;
use std::fs::File;
use std::io::{Read, BufReader};
use std::path::Path;
use thiserror::Error;

const WAL_VERSION: u8 = 1;

#[derive(Debug, Error)]
pub enum WalReaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    
    #[error("WAL version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u8, actual: u8 },
    
    #[error("Incomplete WAL entry")]
    Incomplete,
}

pub type WalResult<T> = Result<T, WalReaderError>;

/// WAL Reader for replaying commands
pub struct WalReader {
    reader: BufReader<File>,
    version_read: bool,
}

impl WalReader {
    /// Open a WAL file for reading
    pub fn open<P: AsRef<Path>>(path: P) -> WalResult<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
            version_read: false,
        })
    }

    /// Read and validate WAL version header
    fn read_version(&mut self) -> WalResult<()> {
        let mut version_byte = [0u8; 1];
        self.reader.read_exact(&mut version_byte)?;
        
        if version_byte[0] != WAL_VERSION {
            return Err(WalReaderError::VersionMismatch {
                expected: WAL_VERSION,
                actual: version_byte[0],
            });
        }
        
        self.version_read = true;
        Ok(())
    }

    /// Read next command from WAL
    /// Returns None if EOF reached
    pub fn read_command<const D: usize>(&mut self) -> WalResult<Option<Command<D>>> {
        // Read version on first call
        if !self.version_read {
            self.read_version()?;
        }

        // Read length prefix (u32)
        let mut len_bytes = [0u8; 4];
        match self.reader.read_exact(&mut len_bytes) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // EOF reached cleanly
                return Ok(None);
            },
            Err(e) => return Err(e.into()),
        }
        
        let len = u32::from_le_bytes(len_bytes) as usize;
        
        // Sanity check: prevent reading gigabytes for corrupted length
        if len > 10 * 1024 * 1024 {
            // 10MB max per command (very generous)
            return Err(WalReaderError::Deserialization(
                format!("Command size {} exceeds maximum", len)
            ));
        }

        // Read command data
        let mut cmd_bytes = vec![0u8; len];
        self.reader.read_exact(&mut cmd_bytes)?;

        // Deserialize via bincode's serde mode
        let (cmd, _): (Command<D>, usize) = bincode::serde::decode_from_slice(&cmd_bytes, bincode::config::standard())
            .map_err(|e| WalReaderError::Deserialization(e.to_string()))?;

        Ok(Some(cmd))
    }

    /// Iterator over all commands in WAL
    pub fn commands<const D: usize>(mut self) -> WalCommandIterator<D> {
        WalCommandIterator {
            reader: self,
            finished: false,
        }
    }
}

/// Iterator over WAL commands
pub struct WalCommandIterator<const D: usize> {
    reader: WalReader,
    finished: bool,
}

impl<const D: usize> Iterator for WalCommandIterator<D> {
    type Item = WalResult<Command<D>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.reader.read_command() {
            Ok(Some(cmd)) => Some(Ok(cmd)),
            Ok(None) => {
                self.finished = true;
                None
            },
            Err(e) => {
                self.finished = true;
                Some(Err(e))
            }
        }
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
    fn test_wal_read_write_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");

        // Write commands
        {
            let mut writer = WalWriter::open(&path).unwrap();
            
            for i in 0..10 {
                let cmd = Command::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::<16>::new_zeros(),
                };
                writer.append_command(&cmd).unwrap();
            }
        }

        // Read commands back
        {
            let reader = WalReader::open(&path).unwrap();
            let commands: Vec<_> = reader.commands::<16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            assert_eq!(commands.len(), 10);
            
            for (i, cmd) in commands.iter().enumerate() {
                match cmd {
                    Command::InsertRecord { id, .. } => {
                        assert_eq!(id.0, i as u32);
                    },
                    _ => panic!("Unexpected command type"),
                }
            }
        }
    }

    #[test]
    fn test_wal_reader_empty_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.wal");

        // Create WAL with only version header (no commands)
        {
            let _writer = WalWriter::open(&path).unwrap();
            // Don't write any commands, just create the file
        }

        // Should read no commands and not panic
        let mut reader = WalReader::open(&path).unwrap();
        
        // First read should get version
        assert!(!reader.version_read);
        
        // Try to read a command - should return None (EOF)
        let result: Option<Command<16>> = reader.read_command().unwrap();
        assert!(result.is_none());
        
        // Version should now be read
        assert!(reader.version_read);
    }
}
