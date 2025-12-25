extern crate alloc;
use alloc::string::String;

use serde::Serialize;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::verify::{kernel_state_hash, snapshot_hash};

#[derive(Serialize)]
pub struct EmbeddedProof {
    pub kernel_version: u64,
    pub snapshot_hash: String,
    pub final_state_hash: String,
}

pub fn generate_proof<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &KernelState<M, D, N, E>,
    snapshot_bytes: &[u8]
) -> EmbeddedProof {
    // 1. Compute Hashes
    let s_hash_bytes = snapshot_hash(snapshot_bytes);
    let k_hash_bytes = kernel_state_hash(state);

    // 2. Encode as Hex Strings (for JSON compatibility with Cloud/CLI)
    // hex::encode returns String when alloc feature is enabled.
    let s_hex = hex::encode(s_hash_bytes);
    let k_hex = hex::encode(k_hash_bytes);

    EmbeddedProof {
        kernel_version: state.version(),
        snapshot_hash: s_hex,
        final_state_hash: k_hex,
    }
}
