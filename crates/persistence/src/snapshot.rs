use crate::error::{PersistenceError, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub magic: [u8; 4],
    pub version: u32,
    pub event_index: u64,
    pub timestamp: u64,
    pub state_hash: [u8; 16],
    pub reserved: [u8; 8],
}

impl SnapshotHeader {
    pub const SIZE: usize = 4 + 4 + 8 + 8 + 16 + 8; // 48 bytes
    pub const MAGIC: [u8; 4] = *b"VALO";

    pub fn new(event_index: u64, timestamp: u64, state_hash: [u8; 16]) -> Self {
        Self {
            magic: Self::MAGIC,
            version: 1,
            event_index,
            timestamp,
            state_hash,
            reserved: [0; 8],
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.magic);
        buf[4..8].copy_from_slice(&self.version.to_le_bytes());
        buf[8..16].copy_from_slice(&self.event_index.to_le_bytes());
        buf[16..24].copy_from_slice(&self.timestamp.to_le_bytes());
        buf[24..40].copy_from_slice(&self.state_hash);
        buf[40..48].copy_from_slice(&self.reserved);
        buf
    }

    pub fn read_from<R: Read>(mut reader: R) -> Result<Self> {
        let mut buf = [0u8; Self::SIZE];
        reader.read_exact(&mut buf)?;

        let magic: [u8; 4] = buf[0..4].try_into().unwrap();
        if magic != Self::MAGIC {
            return Err(PersistenceError::InvalidMagic);
        }

        let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let event_index = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let timestamp = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        let state_hash: [u8; 16] = buf[24..40].try_into().unwrap();
        let reserved: [u8; 8] = buf[40..48].try_into().unwrap();

        Ok(Self {
            magic,
            version,
            event_index,
            timestamp,
            state_hash,
            reserved,
        })
    }
}

pub fn write_to(path: impl AsRef<Path>, header: SnapshotHeader, body: &[u8]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(&header.to_bytes())?;
    file.write_all(body)?;
    file.sync_data()?;
    Ok(())
}

pub fn read_header(path: impl AsRef<Path>) -> Result<SnapshotHeader> {
    let file = File::open(path)?;
    SnapshotHeader::read_from(file)
}

pub fn read_snapshot(path: impl AsRef<Path>) -> Result<(SnapshotHeader, Vec<u8>)> {
    let mut file = File::open(path)?;
    let header = SnapshotHeader::read_from(&mut file)?;
    let mut body = Vec::new();
    file.read_to_end(&mut body)?;
    Ok((header, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_header_serialization() {
        let header = SnapshotHeader::new(100, 1234567890, [0xAA; 16]);
        let bytes = header.to_bytes();
        
        let mut reader = &bytes[..];
        let decoded = SnapshotHeader::read_from(&mut reader).unwrap();
        
        assert_eq!(header, decoded);
    }

    #[test]
    fn test_invalid_magic() {
        let mut bytes = [0u8; SnapshotHeader::SIZE];
        bytes[0..4].copy_from_slice(b"BADM");
        let mut reader = &bytes[..];
        let result = SnapshotHeader::read_from(&mut reader);
        assert!(matches!(result, Err(PersistenceError::InvalidMagic)));
    }
}
