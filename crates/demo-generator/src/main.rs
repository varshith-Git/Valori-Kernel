use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use crc64fast::Digest;
use std::path::Path;
use valori_kernel::ValoriKernel;
use valori_persistence::{snapshot, wal};
use valori_persistence::snapshot::SnapshotHeader;

fn main() -> Result<()> {
    let out_dir = Path::new("demo_db");
    if out_dir.exists() {
        std::fs::remove_dir_all(out_dir)?;
    }
    std::fs::create_dir_all(&out_dir)?;

    let mut kernel = ValoriKernel::new();

    println!("🎬 Generating 'Semantic Drift' Dataset...");

    // --- SCENE 1: The "Red" Cluster (ID 1-100) ---
    // Center: [1000, 1000]
    println!("1. Ingesting 100 'Red' vectors...");
    for i in 1..=100 {
        let x = 1000 + rand_diff(i);
        let y = 1000 + rand_diff(i + 1000); // Different seed
        let payload = create_insert_payload(i, vec![x, y]);
        kernel.apply_event(&payload)?;
    }

    // --- CHECKPOINT 1: SNAPSHOT ---
    println!("2. Taking Snapshot (Checkpoint 1)...");
    let snap_body = kernel.save_snapshot()?;
    
    // Compute Hash
    let mut digest = Digest::new();
    digest.write(&snap_body);
    let checksum = digest.sum64();
    let mut state_hash = [0u8; 16];
    state_hash[0..8].copy_from_slice(&checksum.to_le_bytes());

    let header = SnapshotHeader::new(100, 1700000000, state_hash); 
    
    snapshot::write_to(out_dir.join("snapshot.val"), header, &snap_body)?;

    // --- SCENE 2: The "Blue" Intrusion (ID 101-200) ---
    // Center: [3000, 3000] (Far away from Red)
    println!("3. Ingesting 100 'Blue' vectors (Drift)...");
    
    let wal_path = out_dir.join("events.log");
    
    // We will append to WAL directly.
    for i in 101..=200 {
        let x = 3000 + rand_diff(i);
        let y = 3000 + rand_diff(i + 2000);
        let payload = create_insert_payload(i, vec![x, y]);
        
        // Append to WAL
        wal::append_entry(&wal_path, i, &payload)?;
    }

    // --- SCENE 3: The "Purple" Influencer (ID 201) ---
    // Position: [1500, 1500] (Exact match for the query)
    println!("4. Ingesting 1 'Purple' Influencer...");
    let x = 1500;
    let y = 1500;
    let payload = create_insert_payload(201, vec![x, y]);
    wal::append_entry(&wal_path, 201, &payload)?;

    // Create a dummy metadata index
    let idx_path = out_dir.join("metadata.idx");
    valori_persistence::idx::append_metadata(&idx_path, 100, None, "snapshot".to_string())?;
    valori_persistence::idx::append_metadata(&idx_path, 150, None, "blue_wave".to_string())?;
    valori_persistence::idx::append_metadata(&idx_path, 201, None, "purple_event".to_string())?;

    println!("✅ Demo Database generated at: {:?}", out_dir.canonicalize()?);
    println!("📊 Story: Red (0-100) -> Blue (101-200) -> Purple (201)");
    println!("   Try: valori diff --dir demo_db --from 100 --to 201 --query '[1500, 1500]'");

    Ok(())
}

fn create_insert_payload(id: u64, values: Vec<i32>) -> Vec<u8> {
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

// Deterministic Pseudo-Random
fn rand_diff(seed: u64) -> i32 {
    let mut x = seed;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    // Range -100 to 100
    ((x % 200) as i32) - 100
}
