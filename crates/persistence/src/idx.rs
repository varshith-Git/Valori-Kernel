use crate::error::Result;
use std::fs::OpenOptions;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::fs::File;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub event_id: u64,
    pub timestamp: u64,
    pub label_len: u32,
    pub label: String,
}

impl IndexEntry {
    pub fn read_from<R: Read>(mut reader: R) -> Result<Self> {
        let mut buf = [0u8; 20]; // 8 + 8 + 4
        reader.read_exact(&mut buf)?;

        let event_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let timestamp = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let label_len = u32::from_le_bytes(buf[16..20].try_into().unwrap());

        let mut label_bytes = vec![0u8; label_len as usize];
        reader.read_exact(&mut label_bytes)?;
        
        let label = String::from_utf8(label_bytes)
            .map_err(|e| crate::error::PersistenceError::InvalidFormat(format!("Invalid UTF-8 in label: {}", e)))?;

        Ok(Self {
            event_id,
            timestamp,
            label_len,
            label,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.label.len());
        buf.extend_from_slice(&self.event_id.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&self.label_len.to_le_bytes());
        buf.extend_from_slice(self.label.as_bytes());
        buf
    }
}

pub fn append_metadata(path: impl AsRef<Path>, event_id: u64, timestamp: Option<u64>, label: String) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    let timestamp = timestamp.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    let entry = IndexEntry {
        event_id,
        timestamp,
        label_len: label.len() as u32,
        label,
    };

    file.write_all(&entry.to_bytes())?;
    file.sync_data()?;

    Ok(())
}

pub fn read_all(path: impl AsRef<Path>) -> Result<Vec<IndexEntry>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut entries = Vec::new();

    loop {
        // Peek to handle EOF
        // Since we don't have peek, we try to read.
        // If we fail on the first Read of the loop with EOF, it's fine.
        match IndexEntry::read_from(&mut reader) {
            Ok(entry) => entries.push(entry),
            Err(crate::error::PersistenceError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }

    Ok(entries)
}
