use crate::error::{PersistenceError, Result};
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use crc64fast::Digest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalEntryHeader {
    pub event_id: u64,
    pub payload_len: u32,
    pub checksum: u64,
}

impl WalEntryHeader {
    pub const SIZE: usize = 8 + 4 + 8; // 20 bytes

    pub fn read_from<R: Read>(mut reader: R) -> Result<Self> {
        let mut buf = [0u8; Self::SIZE];
        reader.read_exact(&mut buf)?;

        let event_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let payload_len = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let checksum = u64::from_le_bytes(buf[12..20].try_into().unwrap());

        Ok(Self {
            event_id,
            payload_len,
            checksum,
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.event_id.to_le_bytes());
        buf[8..12].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[12..20].copy_from_slice(&self.checksum.to_le_bytes());
        buf
    }
}

pub struct WalEntry {
    pub header: WalEntryHeader,
    pub payload: Vec<u8>,
}

pub fn append_entry(path: impl AsRef<Path>, event_id: u64, payload: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    let mut digest = Digest::new();
    digest.write(&event_id.to_le_bytes());
    digest.write(&(payload.len() as u32).to_le_bytes());
    digest.write(payload);
    let checksum = digest.sum64();

    let header = WalEntryHeader {
        event_id,
        payload_len: payload.len() as u32,
        checksum,
    };

    file.write_all(&header.to_bytes())?;
    file.write_all(payload)?;
    file.sync_data()?;

    Ok(())
}

pub struct WalReader {
    reader: BufReader<File>,
}

impl WalReader {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
        })
    }
}

impl Iterator for WalReader {
    type Item = Result<WalEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        let header = match WalEntryHeader::read_from(&mut self.reader) {
            Ok(h) => h,
            Err(PersistenceError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => return None,
            Err(e) => return Some(Err(e)),
        };

        let mut payload = vec![0u8; header.payload_len as usize];
        if let Err(e) = self.reader.read_exact(&mut payload) {
             return Some(Err(PersistenceError::IoError(e)));
        }

        // Verify Checksum
        let mut digest = Digest::new();
        digest.write(&header.event_id.to_le_bytes());
        digest.write(&header.payload_len.to_le_bytes());
        digest.write(&payload);
        
        if digest.sum64() != header.checksum {
            return Some(Err(PersistenceError::ChecksumMismatch {
                expected: header.checksum,
                found: digest.sum64(),
            }));
        }

        Some(Ok(WalEntry { header, payload }))
    }
}

pub fn read_stream(path: impl AsRef<Path>) -> Result<WalReader> {
    WalReader::new(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wal_entry_serialization() {
        let payload = b"hello world";
        let mut digest = Digest::new();
        digest.write(&1u64.to_le_bytes());
        digest.write(&(payload.len() as u32).to_le_bytes());
        digest.write(payload);
        let checksum = digest.sum64();

        let header = WalEntryHeader {
            event_id: 1,
            payload_len: payload.len() as u32,
            checksum,
        };

        let bytes = header.to_bytes();
        let mut reader = &bytes[..];
        let decoded = WalEntryHeader::read_from(&mut reader).unwrap();
        
        assert_eq!(header, decoded);
    }
}
