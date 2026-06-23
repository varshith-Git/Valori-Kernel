use crate::checkpoint::WalCheckpoint;
use crate::flash::FlashStorage;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::verify::snapshot_hash;

/// Boot recovery pipeline:
/// 1. Load checkpoint from flash.
/// 2. Read snapshot and verify its hash matches the checkpoint record.
/// 3. Restore kernel state from the snapshot.
///
/// Returns the last committed WAL sequence number, or 0 on a clean (first) boot.
pub fn recover(state: &mut KernelState) -> Result<u64, ()> {
    let checkpoint = WalCheckpoint::load();
    let last_seq = checkpoint.last_committed_wal_index;

    let snap_data = FlashStorage::read_snapshot();
    let current_hash = snapshot_hash(snap_data);

    // Only verify if this is not a fresh device (seq > 0).
    if last_seq > 0 && current_hash != checkpoint.snapshot_hash {
        return Err(());
    }

    // Flash erased state is 0xFF; a valid snapshot cannot start with that byte.
    if snap_data[0] != 0xFF {
        match decode_state(snap_data) {
            Ok(s) => *state = s,
            Err(_) => return Err(()),
        }
    }

    Ok(last_seq)
}
