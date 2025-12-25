use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;

pub enum ApplyResult {
    Applied(usize),
    Incomplete,
    Error,
}

/// WAL Header: 16 Bytes
/// [Version:4][Encoding:4][Dim:4][ChecksumLen:4]
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct WalHeader {
    pub version: u32,
    pub encoding_version: u32,
    pub dim: u32,
    pub checksum_len: u32,
}

impl WalHeader {
    pub const SIZE: usize = 16;
    
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 { return None; }
        
        // Manual LE decode for no_std safety without boilerplate
        let version = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let encoding_version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let dim = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let checksum_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        
        Some(Self {
            version,
            encoding_version,
            dim,
            checksum_len,
        })
    }
}

/// Try to apply a single command from the buffer using Bincode.
/// Returns byte count consumed, or status.
pub fn try_apply_command<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &mut KernelState<M, D, N, E>,
    buf: &[u8]
) -> ApplyResult {
    if buf.is_empty() { return ApplyResult::Incomplete; }

    let config = bincode::config::standard();
    
    // Attempt bincode decode
    match bincode::serde::decode_from_slice::<Command<D>, _>(buf, config) {
        Ok((cmd, len)) => {
            // Apply to Kernel
            if state.apply(&cmd).is_err() {
                 return ApplyResult::Error; // Semantic Error (Capacity etc)
            }
            ApplyResult::Applied(len)
        },
        Err(e) => {
            match e {
                bincode::error::DecodeError::UnexpectedEnd { .. } => ApplyResult::Incomplete,
                 // Also handle "LimitExceeded" if we set limits? Standard has no limit but check buffer len?
                 // decode_from_slice handles buffer bounds via UnexpectedEnd.
                _ => ApplyResult::Error, // Malformed Data
            }
        }
    }
}
