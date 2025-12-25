use valori_kernel::error::{Result, KernelError};

const WAL_STREAM_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy)]
#[repr(packed)] 
struct PacketHeader {
    version: u8,
    flags: u8,   // 0x01 = END_OF_SEGMENT
    seq: u64,
    len: u32,
}

pub const FLAG_EOS: u8 = 0x01;

pub struct WalStream {
    pub next_expected_seq: u64,
}

impl WalStream {
    pub fn new(start_seq: u64) -> Self {
        Self {
            next_expected_seq: start_seq,
        }
    }

    /// Parse and validate a WAL Chunk Packet.
    /// Returns (Payload, is_eos).
    /// Errors if gap, replay, or version mismatch.
    pub fn ingest_packet<'a>(&mut self, packet: &'a [u8]) -> Result<(&'a [u8], bool)> {
        if packet.len() < 14 { // 1+1+8+4 = 14 bytes header
            return Err(KernelError::InvalidOperation); // Truncated header
        }

        let mut offset = 0;
        
        let version = packet[offset]; offset += 1;
        if version != WAL_STREAM_VERSION {
            return Err(KernelError::InvalidOperation); // Version mismatch
        }

        let flags = packet[offset]; offset += 1;
        
        // Read seq (u64 LE)
        let seq_bytes: [u8; 8] = packet[offset..offset+8].try_into().unwrap();
        let seq = u64::from_le_bytes(seq_bytes);
        offset += 8;

        // Read len (u32 LE)
        let len_bytes: [u8; 4] = packet[offset..offset+4].try_into().unwrap();
        let len = u32::from_le_bytes(len_bytes);
        offset += 4;

        if seq != self.next_expected_seq {
            // Replay or Gap
            return Err(KernelError::InvalidOperation); 
        }

        if packet.len() < offset + (len as usize) {
            return Err(KernelError::InvalidOperation); // Truncated payload
        }
        
        let payload = &packet[offset..offset + (len as usize)];
        
        // Advance sequence
        self.next_expected_seq += 1;

        let is_eos = (flags & FLAG_EOS) != 0;
        
        Ok((payload, is_eos))
    }
}
