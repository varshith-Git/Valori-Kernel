//! Deterministic Hashing and Verification.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::state::kernel::KernelState;

/// Computes the cryptographic hash of the Kernel State.
///
/// **Scope**: This hash covers the deterministic kernel state ONLY.
/// It includes:
/// - Kernel Version
/// - Records (Content + Position in Memory)
/// - Nodes (Content + Position + Topology)
/// - Edges (Content + Position + Topology)
///
/// It explicitly **EXCLUDES**:
/// - Node-level metadata (HTTP headers, user sessions)
/// - Index structures (HNSW/IVF aux data)
/// - Runtime caches
pub fn kernel_state_hash<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    state: &KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();

    // 1. Kernel Version
    hasher.update(&state.version.0.to_le_bytes());

    // 2. Records (Canonical Order: By Position)
    // Critical: We must hash the structure of memory (including holes)
    // to differentiate [A, None] from [None, A].
    for (i, slot) in state.records.raw_records().iter().enumerate() {
        hasher.update(&(i as u32).to_le_bytes()); // Hash Memory Address
        if let Some(record) = slot {
            hasher.update(&[1]); // Presence Marker
            
            // Hash Content
            hasher.update(&record.id.0.to_le_bytes());
            hasher.update(&[record.flags]);
            for scalar in record.vector.data.iter() {
                hasher.update(&scalar.0.to_le_bytes());
            }
        } else {
             hasher.update(&[0]); // Absence Marker
        }
    }

    // 3. Nodes (Canonical Order: By Position)
    for (i, slot) in state.nodes.raw_nodes().iter().enumerate() {
        hasher.update(&(i as u32).to_le_bytes());
        if let Some(node) = slot {
            hasher.update(&[1]);
            
            hasher.update(&node.id.0.to_le_bytes());
            hasher.update(&[node.kind as u8]);
            
            if let Some(rid) = node.record {
                hasher.update(&[1]);
                hasher.update(&rid.0.to_le_bytes());
            } else {
                hasher.update(&[0]);
            }

            if let Some(eid) = node.first_out_edge {
                hasher.update(&[1]);
                hasher.update(&eid.0.to_le_bytes());
            } else {
                hasher.update(&[0]);
            }
        } else {
            hasher.update(&[0]);
        }
    }

    // 4. Edges (Canonical Order: By Position)
    for (i, slot) in state.edges.raw_edges().iter().enumerate() {
        hasher.update(&(i as u32).to_le_bytes());
        if let Some(edge) = slot {
            hasher.update(&[1]);
            
            hasher.update(&edge.id.0.to_le_bytes());
            hasher.update(&[edge.kind as u8]);
            hasher.update(&edge.from.0.to_le_bytes());
            hasher.update(&edge.to.0.to_le_bytes());
            
            if let Some(next) = edge.next_out {
                hasher.update(&[1]);
                hasher.update(&next.0.to_le_bytes());
            } else {
                hasher.update(&[0]);
            }
        } else {
            hasher.update(&[0]);
        }
    }

    *hasher.finalize().as_bytes()
}

pub fn snapshot_hash(snapshot_bytes: &[u8]) -> [u8; 32] {
    blake3::hash(snapshot_bytes).into()
}

pub fn wal_hash(wal_bytes: &[u8]) -> [u8; 32] {
    blake3::hash(wal_bytes).into()
}
