// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori verify` — snapshot integrity check.
//!
//! Performs two complementary checks:
//! 1. **Structural validity** — magic bytes and section-length consistency.
//! 2. **State hash** — decodes the kernel section and computes the canonical
//!    BLAKE3 content hash so the result can be compared against a known-good
//!    value.

use crate::engine::{inspect_snapshot_bytes, parse_kernel_from_snapshot_bytes};
use crc64fast::Digest;
use valori_kernel::snapshot::blake3::hash_state_blake3;

pub fn run(snapshot_path: &str) -> anyhow::Result<()> {
    let bytes = std::fs::read(snapshot_path)
        .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", snapshot_path, e))?;

    let file_kb = bytes.len() as f64 / 1024.0;

    println!("\nVerify — {snapshot_path}  ({file_kb:.2} KB)\n");

    // ── 1. Structural check ───────────────────────────────────────────────────
    let info = inspect_snapshot_bytes(&bytes)?;

    if !info.magic_ok {
        println!("❌  STRUCTURAL INTEGRITY   FAILED");
        println!("    Expected magic bytes: VAL1");
        println!(
            "    Found:              {:?}",
            bytes.get(0..4).unwrap_or(&[])
        );
        anyhow::bail!("Snapshot has invalid magic bytes");
    }

    // Verify total byte count matches sum of sections.
    let expected_total = 4                       // "VAL1"
        + 4 + info.kernel_len                    // kernel section
        + 4 + info.metadata_len                  // metadata section
        + 4 + info.index_len;                    // index section

    let structure_ok = expected_total == bytes.len();

    if structure_ok {
        println!("✅  STRUCTURAL INTEGRITY   PASSED");
    } else {
        println!("⚠️   STRUCTURAL INTEGRITY   PARTIAL");
        println!(
            "    Expected {} bytes from section headers, file is {} bytes",
            expected_total,
            bytes.len()
        );
    }

    // ── 2. CRC64 file checksum ────────────────────────────────────────────────
    // L-1: CRC64 is an error-detection code, NOT a cryptographic hash.
    // It cannot detect intentional tampering — use the BLAKE3 hash below for that.
    let crc = compute_crc64(&bytes);
    println!("    File CRC64:  {crc:016x}  (error-detection checksum — use BLAKE3 for tamper detection)");

    // ── 3. BLAKE3 state hash ─────────────────────────────────────────────────
    match parse_kernel_from_snapshot_bytes(&bytes) {
        Ok(state) => {
            let b3 = hash_state_blake3(&state);
            let hex: String = b3.iter().map(|b| format!("{b:02x}")).collect();
            println!("    BLAKE3 hash: {hex}");
            println!(
                "    Records: {}  Nodes: {}  Edges: {}  Dim: {}",
                state.record_count(),
                state.node_count(),
                state.edge_count(),
                state.dim.unwrap_or(0)
            );
            println!("\n✅  SNAPSHOT VALID\n");
            Ok(())
        }
        Err(e) => {
            println!("\n❌  KERNEL DECODE         FAILED");
            println!("    {e}");
            anyhow::bail!("Kernel section could not be decoded")
        }
    }
}

/// Compute a CRC-64/ECMA checksum over a byte slice.
pub fn compute_crc64(data: &[u8]) -> u64 {
    let mut digest = Digest::new();
    digest.write(data);
    digest.sum64()
}
