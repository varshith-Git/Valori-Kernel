//! State hashing.

use crate::state::kernel::KernelState;

/// Computes a hash of the current state.
/// 
/// For simplicity and determinism, this function encodes the state to a temporary buffer
/// and hashes the bytes.
/// 
/// Note: Allocates a stack buffer? Or requires one?
/// Kernel is `no_std`, huge stack buffer is bad.
/// But hashing usually streams.
/// 
/// Better approach: Implement a "Hasher" that key fields are fed into.
/// FNV-1a implementation.
pub struct FnvHasher {
    state: u64,
}

impl FnvHasher {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x1099511628211;

    pub fn new() -> Self {
        Self { state: Self::OFFSET_BASIS }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= b as u64;
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }
    
    pub fn write_u32(&mut self, val: u32) {
        self.write(&val.to_le_bytes());
    }

    pub fn write_i32(&mut self, val: i32) {
        self.write(&val.to_le_bytes());
    }

    pub fn finish(self) -> u64 {
        self.state
    }
}

pub fn hash_state<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    state: &KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
) -> u64 {
    let mut hasher = FnvHasher::new();

    // Version
    hasher.write(&state.version.0.to_le_bytes());

    // Records (iteration order is deterministic by pool implementation)
    // We only hash ACTIVE records to allow for equivalent states even if sparse slots differ?
    // No, snapshot/encode also encodes active ones. 
    // Strict determinism implies hash must match snapshot.
    
    for record in state.records.iter() {
        hasher.write_u32(record.id.0);
        hasher.write(&[record.flags]);
        for scalar in record.vector.data.iter() {
            hasher.write_i32(scalar.0);
        }
    }

    // Nodes
    for slot in state.nodes.raw_nodes().iter() {
        if let Some(node) = slot {
            hasher.write_u32(node.id.0);
            hasher.write(&[node.kind as u8]);
            match node.record {
                Some(id) => hasher.write_u32(id.0),
                None => hasher.write_u32(u32::MAX), // Sentinel
            }
            match node.first_out_edge {
                Some(id) => hasher.write_u32(id.0),
                None => hasher.write_u32(u32::MAX),
            }
        }
    }

    // Edges
    for slot in state.edges.raw_edges().iter() {
         if let Some(edge) = slot {
            hasher.write_u32(edge.id.0);
            hasher.write(&[edge.kind as u8]);
            hasher.write_u32(edge.from.0);
            hasher.write_u32(edge.to.0);
            match edge.next_out {
                Some(id) => hasher.write_u32(id.0),
                None => hasher.write_u32(u32::MAX),
            }
        }
    }

    hasher.finish()
}
