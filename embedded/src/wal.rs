use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;

pub enum ApplyResult {
    Applied(usize),
    Incomplete,
    Error,
}

/// WAL Header: 16 bytes
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
        Some(Self {
            version:          u32::from_le_bytes([bytes[0],  bytes[1],  bytes[2],  bytes[3]]),
            encoding_version: u32::from_le_bytes([bytes[4],  bytes[5],  bytes[6],  bytes[7]]),
            dim:              u32::from_le_bytes([bytes[8],  bytes[9],  bytes[10], bytes[11]]),
            checksum_len:     u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }
}

/// Try to apply a single `KernelEvent` from the buffer (bincode-encoded).
/// Returns bytes consumed, or status.
pub fn try_apply_event(state: &mut KernelState, buf: &[u8]) -> ApplyResult {
    if buf.is_empty() { return ApplyResult::Incomplete; }

    let config = bincode::config::standard();

    match bincode::serde::decode_from_slice::<KernelEvent, _>(buf, config) {
        Ok((evt, len)) => {
            if state.apply_event(&evt).is_err() {
                return ApplyResult::Error;
            }
            ApplyResult::Applied(len)
        }
        Err(e) => match e {
            bincode::error::DecodeError::UnexpectedEnd { .. } => ApplyResult::Incomplete,
            _ => ApplyResult::Error,
        },
    }
}
