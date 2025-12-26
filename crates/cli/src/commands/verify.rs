use crc64fast::Digest;
use std::fs::File;
use std::io::Read;
use valori_persistence::{snapshot, PersistenceError};

pub fn run(snapshot_path: &str) -> anyhow::Result<()> {
    // Single pass read: Open once.
    let mut file = File::open(snapshot_path)?;
    
    // 1. Read Header
    let header = snapshot::SnapshotHeader::read_from(&mut file)?;
    
    // 2. Read Body (Remaining bytes)
    let mut body = Vec::new();
    file.read_to_end(&mut body)?;

    // 3. Compute CRC64
    let computed_hash = compute_crc64(&body);
    
    // 4. Extract Stored CRC64 (First 8 bytes of state_hash)
    let stored_hash = u64::from_le_bytes(header.state_hash[0..8].try_into().unwrap());

    if computed_hash == stored_hash {
        println!("\n✅ VERIFIED\n");
        println!("Computed Hash: {:016x}", computed_hash);
        println!("Confidence:    STRONG (CRC64)\n");
        Ok(())
    } else {
        println!("\n❌ CORRUPTED\n");
        println!("Expected Hash: {:016x}", stored_hash);
        println!("Found Hash:    {:016x}", computed_hash);
        Err(PersistenceError::ChecksumMismatch {
            expected: stored_hash,
            found: computed_hash,
        }.into())
    }
}

pub fn compute_crc64(data: &[u8]) -> u64 {
    let mut digest = Digest::new();
    digest.write(data);
    digest.sum64()
}
