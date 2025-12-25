// -----------------------------------------------------------------------
// WAL Checkpoint (Simulated Persistent Storage)
// -----------------------------------------------------------------------
// This structure is critical for recovery.
// It points to the last VALID Committed State.

// Simulated Flash Region for Checkpoint
// Smaller buffer (e.g. 1KB)
static mut CHECKPOINT_FLASH: [u8; 1024] = [0; 1024];

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct WalCheckpoint {
    pub last_committed_wal_index: u64,
    pub snapshot_hash: [u8; 32],
    pub kernel_protocol_version: u64,
    pub magic: u32, // Safety check
}

const CHECKPOINT_MAGIC: u32 = 0xCAFEBABE;

impl WalCheckpoint {
    pub fn new() -> Self {
        Self {
            last_committed_wal_index: 0,
            snapshot_hash: [0; 32],
            kernel_protocol_version: 0,
            magic: CHECKPOINT_MAGIC,
        }
    }

    /// Load checkpoint from Flash.
    /// If invalid or magic mismatch, returns default (Fresh State).
    pub fn load() -> Self {
        unsafe {
            let ptr = core::ptr::addr_of_mut!(CHECKPOINT_FLASH) as *const WalCheckpoint;
            let cp = core::ptr::read_volatile(ptr);
             if cp.magic == CHECKPOINT_MAGIC {
                 cp
             } else {
                 Self::new()
             }
        }
    }

    /// Commit checkpoint to Flash.
    /// Must be atomic.
    pub fn save(&self) {
        unsafe {
            let ptr = core::ptr::addr_of_mut!(CHECKPOINT_FLASH) as *mut WalCheckpoint;
            core::ptr::write_volatile(ptr, *self);
        }
    }
}
