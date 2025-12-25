use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;

pub enum ApplyResult {
    Applied(usize),
    Incomplete,
    Error,
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
