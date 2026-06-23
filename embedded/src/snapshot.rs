extern crate alloc;
use alloc::vec;

use crate::flash::FlashStorage;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::encode::encode_state;

pub fn snapshot_to_flash(state: &KernelState) -> Result<usize, ()> {
    let mut buffer = vec![0u8; 64 * 1024];

    let len = match encode_state(state, &mut buffer) {
        Ok(l) => l,
        Err(_) => return Err(()),
    };

    FlashStorage::erase_snapshot_sector()?;
    FlashStorage::write_snapshot(&buffer[0..len])?;

    Ok(len)
}
