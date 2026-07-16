// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! WAL Reader — supports both v1 (legacy Command) and v2 (KernelEvent + ns) formats.

use valori_kernel::event::KernelEvent;
use crate::wal_compat::{LegacyWalCommand, legacy_to_event};
use std::fs::File;
use std::io::{Read, BufReader, BufRead};
use std::path::Path;
use thiserror::Error;

/// 16-byte header at the start of every WAL file.
/// Layout: [Version:u32 LE][EncodingVersion:u32 LE][Dim:u32 LE][ChecksumLen:u32 LE]
///
/// Version 1 = legacy Command bincode stream.
/// Version 2 = (KernelEvent, namespace_id: u16) bincode stream.
pub struct WalHeader {
    pub version: u32,
    pub encoding_version: u32,
    pub dim: u32,
    pub checksum_len: u32,
}

impl WalHeader {
    pub const SIZE: usize = 16;

    pub fn read(buf: &[u8]) -> Result<(Self, &[u8]), ()> {
        if buf.len() < Self::SIZE {
            return Err(());
        }
        Ok((
            Self {
                version:          u32::from_le_bytes([buf[0],  buf[1],  buf[2],  buf[3]]),
                encoding_version: u32::from_le_bytes([buf[4],  buf[5],  buf[6],  buf[7]]),
                dim:              u32::from_le_bytes([buf[8],  buf[9],  buf[10], buf[11]]),
                checksum_len:     u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            },
            &buf[Self::SIZE..],
        ))
    }
}

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

/// True when a bincode decode failure means "no more complete entries" —
/// either the stream had nothing left (`Io` wrapping `UnexpectedEof`, what
/// `decode_from_std_read` actually raises on a `BufReader<File>` at EOF) or
/// bincode's own short-read signal (`UnexpectedEnd`, from a slice-based
/// decode elsewhere). Distinct from every other `DecodeError` variant,
/// which is a real (possibly mid-write) corruption and must propagate.
fn is_clean_eof(e: &bincode::error::DecodeError) -> bool {
    match e {
        bincode::error::DecodeError::UnexpectedEnd { .. } => true,
        bincode::error::DecodeError::Io { inner, .. } => {
            inner.kind() == std::io::ErrorKind::UnexpectedEof
        }
        _ => false,
    }
}

/// WAL reader. Transparently handles v1 (Command) and v2 (KernelEvent+ns) formats.
/// The iterator always yields `(KernelEvent, namespace_id)` regardless of format.
pub struct WalReader {
    reader: BufReader<File>,
    /// Set after `read_header()` is called.
    version: u32,
    header_read: bool,
    expected_dim: Option<u32>,
}

impl WalReader {
    pub fn open<P: AsRef<Path>>(path: P, expected_dim: Option<u32>) -> WalResult<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
            version: 0,
            header_read: false,
            expected_dim,
        })
    }

    fn read_header(&mut self) -> WalResult<()> {
        let mut head_buf = [0u8; 16];
        self.reader.read_exact(&mut head_buf)?;

        let (header, _) = WalHeader::read(&head_buf)
            .map_err(|_| WalReaderError::Header("Invalid header".into()))?;

        if header.version != 1 && header.version != 2 {
            return Err(WalReaderError::Header(
                format!("Unsupported WAL version {}", header.version)
            ));
        }

        if let Some(expected) = self.expected_dim {
            if header.dim != 0 && header.dim != expected {
                return Err(WalReaderError::Header(format!(
                    "Dimension mismatch: file={}, expected={}", header.dim, expected
                )));
            }
        }

        self.version = header.version;
        self.header_read = true;
        Ok(())
    }

    /// Read the next entry from the WAL, translated into `(KernelEvent, namespace_id)`.
    ///
    /// Returns `Ok(None)` at a clean end of stream — no more complete
    /// entries remain (either nothing left to read, or a trailing partial
    /// write from a crash mid-append). Safe to call directly in a loop;
    /// does not require pre-checking EOF via `into_iter()`.
    pub fn read_entry(&mut self) -> WalResult<Option<(KernelEvent, u16)>> {
        if !self.header_read {
            self.read_header()?;
        }

        let config = bincode::config::standard();

        match self.version {
            1 => {
                match bincode::serde::decode_from_std_read::<LegacyWalCommand, _, _>(
                    &mut self.reader, config,
                ) {
                    Ok(cmd) => Ok(Some(legacy_to_event(cmd))),
                    Err(e) if is_clean_eof(&e) => Ok(None),
                    Err(e) => Err(WalReaderError::Deserialization(e.to_string())),
                }
            }
            2 => {
                match bincode::serde::decode_from_std_read::<(KernelEvent, u16), _, _>(
                    &mut self.reader, config,
                ) {
                    Ok(pair) => Ok(Some(pair)),
                    Err(e) if is_clean_eof(&e) => Ok(None),
                    Err(e) => Err(WalReaderError::Deserialization(e.to_string())),
                }
            }
            _ => Err(WalReaderError::Header(format!("Unsupported version {}", self.version))),
        }
    }

    fn ensure_not_eof(&mut self) -> WalResult<bool> {
        let buf = self.reader.fill_buf()?;
        Ok(!buf.is_empty())
    }
}

/// Iterator that yields `(KernelEvent, namespace_id)` pairs from the WAL.
pub struct WalEntryIterator {
    reader: WalReader,
}

impl Iterator for WalEntryIterator {
    type Item = WalResult<(KernelEvent, u16)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.ensure_not_eof() {
            Ok(false) => return None,
            Err(e) => return Some(Err(e)),
            Ok(true) => {}
        }
        match self.reader.read_entry() {
            Ok(Some(pair)) => Some(Ok(pair)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

impl IntoIterator for WalReader {
    type Item = WalResult<(KernelEvent, u16)>;
    type IntoIter = WalEntryIterator;

    fn into_iter(self) -> Self::IntoIter {
        WalEntryIterator { reader: self }
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
    fn test_wal_roundtrip_v2() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("roundtrip.wal");

        {
            let mut writer = WalWriter::open(&path, 16).unwrap();
            let evt = KernelEvent::InsertRecord {
                id: RecordId(0),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            };
            writer.append_event(&evt, 0).unwrap();
        }

        {
            let reader = WalReader::open(&path, Some(16)).unwrap();
            let entries: Vec<_> = reader.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
            assert_eq!(entries.len(), 1);
            assert!(matches!(entries[0].0, KernelEvent::InsertRecord { .. }));
        }
    }
}
