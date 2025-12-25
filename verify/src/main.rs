use clap::Parser;
use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};

// Use core default constants matching node/src/config.rs
// Ideally these would be shared, but values are effectively protocol constants for v1.
const MAX_RECORDS: usize = 1024;
const D: usize = 16;
const MAX_NODES: usize = 1024;
const MAX_EDGES: usize = 2048;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the Snapshot file (e.g. snapshot.bin)
    snapshot: PathBuf,

    /// Path to the WAL file (optional/required? prompt implied required)
    /// If no WAL, we just hash the snapshot state.
    wal: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
struct SnapshotMeta {
    pub version: u32,
    pub timestamp: u64,
    pub kernel_len: u64,
    pub metadata_len: u64,
    pub index_len: u64,
    // Ignoring other fields for now
}

const MAGIC: u32 = 0x56414C4F; // VALO

fn parse_snapshot(path: &PathBuf) -> Result<(Vec<u8>, Vec<u8>)> { // (FullBytes, KernelBlob)
    let buffer = fs::read(path).context("Failed to read snapshot file")?;
    
    if buffer.len() < 16 {
        anyhow::bail!("Snapshot too short");
    }

    // Parse Header from content (excluding trailer CRC)
    let split_idx = buffer.len() - 4;
    let (content, _trailer) = buffer.split_at(split_idx);
    
    // Check MAGIC
    let magic = u32::from_le_bytes(content[0..4].try_into()?);
    if magic != MAGIC {
        anyhow::bail!("Invalid Magic Number");
    }

    let meta_len = u32::from_le_bytes(content[8..12].try_into()?) as usize;
    let meta_end = 12 + meta_len;
    
    if content.len() < meta_end {
        anyhow::bail!("Truncated metadata");
    }

    // Parse Meta to get lengths
    let meta: SnapshotMeta = serde_json::from_slice(&content[12..meta_end])
        .context("Failed to parse Snapshot Metadata JSON")?;

    let k_len = meta.kernel_len as usize;
    let k_start = meta_end;
    let k_end = k_start + k_len;

    if content.len() < k_end {
        anyhow::bail!("Truncated kernel data");
    }

    let kernel_blob = content[k_start..k_end].to_vec();
    
    // Return full buffer (for snapshot_hash) and kernel blob (for restore)
    Ok((buffer, kernel_blob))
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    eprintln!("Valori Verifier v0.1.0");
    eprintln!("Protocol: D={}, MaxRecords={}", D, MAX_RECORDS);

    // 1. Load and Parse Snapshot
    let (snap_bytes, kernel_blob) = parse_snapshot(&args.snapshot)
        .context("Failed to parse snapshot container")?;

    // 2. Load WAL
    let wal_bytes = fs::read(&args.wal)
        .context("Failed to read WAL file")?;

    // 3. Replay and Compute State Hash
    let final_state_hash = valori_kernel::replay::replay_and_hash::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(
        &kernel_blob, 
        &wal_bytes
    ).map_err(|e| anyhow::anyhow!("Replay failed: {:?}", e))?;

    // 4. Compute Input Hashes
    let snapshot_hash = valori_kernel::verify::snapshot_hash(&snap_bytes);
    let wal_hash = valori_kernel::verify::wal_hash(&wal_bytes);

    // 5. Construct Proof
    let proof = valori_kernel::proof::DeterministicProof {
        kernel_version: 1, // Protocol version
        snapshot_hash,
        wal_hash,
        final_state_hash,
    };

    // 6. Output JSON
    let json = serde_json::to_string_pretty(&proof)?;
    println!("{}", json);

    Ok(())
}
