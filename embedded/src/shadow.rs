extern crate alloc;
use alloc::vec::Vec;
use blake3::Hasher;

use valori_kernel::state::kernel::KernelState;
use crate::wal;

// -----------------------------------------------------------------------
// Shadow Kernel (Provisional Execution)
// -----------------------------------------------------------------------

pub struct ShadowKernel<'a, const M: usize, const D: usize, const N: usize, const E: usize> {
    pub state: &'a mut KernelState<M, D, N, E>,
    pub wal_accumulator: Hasher,
    pub segment_active: bool,
    pub buffer: Vec<u8>,
    pub header_processed: bool,
}

impl<'a, const M: usize, const D: usize, const N: usize, const E: usize> ShadowKernel<'a, M, D, N, E> {
    pub fn new(state: &'a mut KernelState<M, D, N, E>) -> Self {
        Self {
            state,
            wal_accumulator: Hasher::new(),
            segment_active: false,
            buffer: Vec::new(),
            header_processed: false,
        }
    }

    pub fn start_segment(&mut self) {
        self.wal_accumulator = Hasher::new();
        self.segment_active = true;
        self.buffer.clear();
        self.header_processed = false;
    }

    /// Apply a WAL chunk to the Shadow Kernel.
    /// Buffers data and applies only complete commands.
    /// Updates accumulator only for APPLIED commands.
    pub fn apply_chunk(&mut self, chunk: &[u8]) -> Result<(), ()> {
        if !self.segment_active {
            return Err(());
        }

        self.buffer.extend_from_slice(chunk);

        // Process Loop
        loop {
            // 1. Header Check (Once)
            if !self.header_processed {
                if self.buffer.is_empty() { return Ok(()); } // Need more data
                
                let version = self.buffer[0];
                if version != 1 {
                    return Err(()); // Bad Version
                }
                
                // Accumulate Header Byte?
                // User: "Running Hash Accumulator... incrementall per applied command"
                // Usually Header is part of the "WAL Log Hash".
                // I will include it.
                self.wal_accumulator.update(&[version]);
                
                self.buffer.remove(0); // Inefficient for Vec, but low freq (once).
                self.header_processed = true;
            }

            if self.buffer.is_empty() { break; }

            // 2. Try Apply Command
            // We pass a slice.
            match wal::try_apply_command(self.state, &self.buffer) {
                wal::ApplyResult::Applied(bytes_consumed) => {
                     // Update Hash with consumed bytes (Command Data)
                     let cmd_bytes = &self.buffer[0..bytes_consumed];
                     self.wal_accumulator.update(cmd_bytes);
                     
                     // Remove from buffer (inefficient drain from front, use VecDeque if std available, or circular buf if optimization needed. For Phase 4, Vec::drain is acceptable for correctness proof).
                     // self.buffer.drain(0..bytes_consumed); // drain returns iterator, drop it.
                     // drain is available in alloc::vec::Vec.
                     let _ = self.buffer.drain(0..bytes_consumed);
                },
                wal::ApplyResult::Incomplete => {
                    // Stop and wait for more data
                    break;
                },
                wal::ApplyResult::Error => {
                    return Err(()); // Invalid Data -> Halt
                }
            }
        }
        
        Ok(())
    }

    /// Finalize segment and return Accumulator Hash.
    pub fn get_accumulator_hash(&self) -> [u8; 32] {
        *self.wal_accumulator.finalize().as_bytes()
    }
}
