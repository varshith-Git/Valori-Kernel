use crate::flash::FlashStorage;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::encode::encode_state;

extern crate alloc;
use alloc::vec;

pub fn snapshot_to_flash<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &KernelState<M, D, N, E>
) -> Result<usize, ()> {
    // 1. Allocate buffer (on Heap)
    // We allocation 64KB for snapshot.
    let mut buffer = vec![0u8; 64 * 1024];
    
    // 2. Encode State
    let len = match encode_state(state, &mut buffer) {
        Ok(l) => l,
        Err(_) => return Err(()) // Capacity exceeded or other error
    };

    // 3. Commit to Flash
    FlashStorage::erase_snapshot_sector()?;
    FlashStorage::write_snapshot(&buffer[0..len])?;

    Ok(len)
}
