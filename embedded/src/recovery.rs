use crate::checkpoint::WalCheckpoint;
use crate::flash::FlashStorage;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::verify::snapshot_hash;

// -----------------------------------------------------------------------
// Recovery Pipeline
// -----------------------------------------------------------------------

pub fn recover<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &mut KernelState<M, D, N, E>
) -> Result<u64, ()> {
    // 1. Load Checkpoint
    let checkpoint = WalCheckpoint::load();
    let last_seq = checkpoint.last_committed_wal_index;
    
    // 2. Read Snapshot from Flash
    let snap_data = FlashStorage::read_snapshot(); // Returns entire buffer
    
    // 3. Verify Snapshot Hash vs Checkpoint
    // This is the atomic link check.
    // Hash of the data in flash must match what we committed in checkpoint.
    let current_hash = snapshot_hash(snap_data);
    
    // Note: Checkpoint init is all zeros. Hash of empty flash might not match zero hash.
    // If defaults (new device), we might skip check or expect specific behavior.
    // For Phase 4 demo, we assume "Initialized" state or handle boot.
    // If checkpoint is fresh (seq=0), we might accept empty snapshot?
    
    if last_seq > 0 {
        if current_hash != checkpoint.snapshot_hash {
             // CRITICAL: Snapshot divergence.
             // "If pointer contradicts snapshot -> HALT"
             return Err(());
        }
    }

    // 4. Restore State
    // Deserialize snapshot into RAM kernel.
    // If snapshot empty/invalid, decode_state might fail.
    // On fresh boot (erased flash), decode fails?
    // We handle clean boot vs recovery.
    // If Flash is 0xFF, decode fails.
    // If new device, we just return seq=0 and clean state (already new).
    
    if snap_data[0] != 0xFF {
         // Attempt restore
         match decode_state(snap_data) {
             Ok(s) => *state = s,
             Err(_) => return Err(()) // Corrupt snapshot data
         }
    }
    
    Ok(last_seq)
}
