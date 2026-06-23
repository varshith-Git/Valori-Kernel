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

pub fn generate_proof(state: &KernelState, snapshot_bytes: &[u8]) -> EmbeddedProof {
    let s_hash = snapshot_hash(snapshot_bytes);
    let k_hash = kernel_state_hash(state);

    EmbeddedProof {
        kernel_version: state.version(),
        snapshot_hash: hex::encode(s_hash),
        final_state_hash: hex::encode(k_hash),
    }
}
