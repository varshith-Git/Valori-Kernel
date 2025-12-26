use crate::error::Result;
use crate::idx;
use crate::snapshot::{self, SnapshotHeader};
use crate::wal;

use std::fs;
use std::path::{Path, PathBuf};

pub struct TestPaths {
    pub snapshot: PathBuf,
    pub wal: PathBuf,
    pub idx: PathBuf,
}

pub fn generate_test_scenario(dir: &Path) -> Result<TestPaths> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }

    // 1. Create snapshot.val
    let snapshot_path = dir.join("snapshot.val");
    let mut kernel = ValoriKernel::new();
    // Insert 1: [100, 100]
    kernel.apply_event(&create_insert_payload(1, vec![100, 100])).unwrap();
    // Insert 2: [200, 200]
    kernel.apply_event(&create_insert_payload(2, vec![200, 200])).unwrap();
    // Insert 3: [300, 300]
    kernel.apply_event(&create_insert_payload(3, vec![300, 300])).unwrap();

    let body = kernel.save_snapshot().expect("Failed to create snapshot");

    // FIX: Compute real CRC64 for the body
    let mut digest = crc64fast::Digest::new();
    digest.write(&body);
    let checksum = digest.sum64();

    // Fill the 16-byte hash array: [CRC64 (8 bytes)] + [Padding (8 bytes)]
    let mut state_hash = [0u8; 16];
    state_hash[0..8].copy_from_slice(&checksum.to_le_bytes());

    let header = SnapshotHeader::new(100, 1234567890, state_hash);
    
    snapshot::write_to(&snapshot_path, header, &body)?;

    // 2. Create events.log
    let wal_path = dir.join("events.log");
    for i in 1..=5 {
        let payload = format!("event_payload_{}", i).into_bytes();
        wal::append_entry(&wal_path, i, &payload)?;
    }

    // 3. Create metadata.idx
    let idx_path = dir.join("metadata.idx");
    idx::append_metadata(&idx_path, 0, Some(0), "init".to_string())?;
    idx::append_metadata(&idx_path, 3, Some(0), "batch:1".to_string())?;
    idx::append_metadata(&idx_path, 5, Some(0), "manual-checkpoint".to_string())?;

    Ok(TestPaths {
        snapshot: snapshot_path,
        wal: wal_path,
        idx: idx_path,
    })
}

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use valori_kernel::ValoriKernel;

/// Creates a strictly formatted InsertPayload: [CMD(1), ID(8), DIM(2), VALUES(dim*4)]
pub fn create_insert_payload(id: u64, values: Vec<i32>) -> Vec<u8> {
    let dim = values.len() as u16;
    let mut wtr = Vec::new();
    wtr.write_u8(1).unwrap(); // CMD_INSERT
    wtr.write_u64::<LittleEndian>(id).unwrap();
    wtr.write_u16::<LittleEndian>(dim).unwrap();
    for v in values {
        wtr.write_i32::<LittleEndian>(v).unwrap();
    }
    wtr
}

/// Generates a scenario where WAL events exist AFTER the snapshot.
/// Snapshot at index 100. WAL has 101, 102, 103.
pub fn generate_replay_scenario(dir: &Path) -> Result<TestPaths> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }

    // 1. Snapshot at Index 100
    // 1. Snapshot at Index 100
    let snapshot_path = dir.join("snapshot.val");
    let mut kernel = ValoriKernel::new();
    // Insert 1: Low level
    kernel.apply_event(&create_insert_payload(1, vec![10, 10])).unwrap();
    // Insert 8: High level (binary 1000 usually has trailing zeros depending on hash)
    kernel.apply_event(&create_insert_payload(8, vec![80, 80])).unwrap();
    // Insert 3: Another point
    kernel.apply_event(&create_insert_payload(3, vec![30, 30])).unwrap();
    
    let body = kernel.save_snapshot().expect("Failed to create snapshot");
    
    let mut digest = crc64fast::Digest::new();
    digest.write(&body);
    let checksum = digest.sum64();
    let mut state_hash = [0u8; 16];
    state_hash[0..8].copy_from_slice(&checksum.to_le_bytes());
    
    // Note: event_index is 100
    let header = SnapshotHeader::new(100, 1234567890, state_hash);
    snapshot::write_to(&snapshot_path, header, &body)?;

    // 2. WAL events 101, 102, 103 (After snapshot)
    let wal_path = dir.join("events.log");
    for i in 101..=103 {
        // Use VALID InsertPayload!
        // ID=i, Values=[i, i+1] (dim 2)
        let values = vec![i as i32, (i+1) as i32];
        let payload = create_insert_payload(i, values);
        wal::append_entry(&wal_path, i, &payload)?;
    }

    // 3. Metadata
    let idx_path = dir.join("metadata.idx");
    idx::append_metadata(&idx_path, 100, Some(0), "snapshot_taken".to_string())?;
    idx::append_metadata(&idx_path, 102, Some(0), "batch_1_ingested".to_string())?;

    Ok(TestPaths {
        snapshot: snapshot_path,
        wal: wal_path,
        idx: idx_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_fixture_generator() {
        let dir = tempdir().unwrap();
        let paths = generate_test_scenario(dir.path()).unwrap();

        assert!(paths.snapshot.exists());
        assert!(paths.wal.exists());
        assert!(paths.idx.exists());

        // Verify Snapshot
        let header = snapshot::read_header(&paths.snapshot).unwrap();
        assert_eq!(header.version, 1);
        assert_eq!(header.event_index, 100);

        // Verify WAL
        let mut count = 0;
        for entry in wal::read_stream(&paths.wal).unwrap() {
            let entry = entry.unwrap();
            count += 1;
            assert_eq!(entry.header.event_id, count);
        }
        assert_eq!(count, 5);

        // Verify Index
        let entries = idx::read_all(&paths.idx).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].label, "init");
        assert_eq!(entries[1].label, "batch:1");
        assert_eq!(entries[2].label, "manual-checkpoint");
    }
}
