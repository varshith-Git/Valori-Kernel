extern crate alloc;
use alloc::vec::Vec;
use blake3::Hasher;

use valori_kernel::state::kernel::KernelState;
use crate::wal;

pub struct ShadowKernel<'a> {
    pub state: &'a mut KernelState,
    pub wal_accumulator: Hasher,
    pub segment_active: bool,
    pub buffer: Vec<u8>,
    pub header_processed: bool,
}

impl<'a> ShadowKernel<'a> {
    pub fn new(state: &'a mut KernelState) -> Self {
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

    /// Buffer an incoming WAL chunk and apply all complete events it contains.
    /// Updates the BLAKE3 accumulator for every applied event so the proof
    /// commits to the exact byte sequence that was applied.
    pub fn apply_chunk(&mut self, chunk: &[u8]) -> Result<(), ()> {
        if !self.segment_active { return Err(()); }

        self.buffer.extend_from_slice(chunk);

        loop {
            if !self.header_processed {
                if self.buffer.len() < wal::WalHeader::SIZE {
                    return Ok(());
                }

                let header = match wal::WalHeader::from_bytes(&self.buffer) {
                    Some(h) => h,
                    None => return Err(()),
                };

                // Dimension must match this firmware's compiled-in DIM.
                if header.dim != crate::DIM as u32 {
                    return Err(());
                }

                let header_bytes = &self.buffer[0..wal::WalHeader::SIZE];
                self.wal_accumulator.update(header_bytes);

                let _ = self.buffer.drain(0..wal::WalHeader::SIZE);
                self.header_processed = true;
            }

            if self.buffer.is_empty() { break; }

            match wal::try_apply_event(self.state, &self.buffer) {
                wal::ApplyResult::Applied(n) => {
                    self.wal_accumulator.update(&self.buffer[0..n]);
                    let _ = self.buffer.drain(0..n);
                }
                wal::ApplyResult::Incomplete => break,
                wal::ApplyResult::Error => return Err(()),
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_accumulator_hash(&self) -> [u8; 32] {
        *self.wal_accumulator.finalize().as_bytes()
    }
}
