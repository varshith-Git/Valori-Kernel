use crate::error::{KernelError, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

pub mod id;
pub mod vector;
pub mod enums;
pub mod scalar;

pub type FixedPointVector = Vec<i32>;

pub const CMD_INSERT: u8 = 1;
pub const CMD_DELETE: u8 = 2;

#[derive(Debug, PartialEq)]
pub struct InsertPayload {
    pub cmd: u8,
    pub id: u64,
    pub dim: u16,
    pub values: Vec<i32>,
    pub tag: u64,
    pub metadata: Option<Vec<u8>>,
}

impl InsertPayload {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);
        
        // 1. Read Command (u8)
        let cmd = cursor.read_u8()?;
        if cmd != CMD_INSERT {
            return Err(KernelError::InvalidCommand(cmd));
        }

        // 2. Read ID (u64)
        let id = cursor.read_u64::<LittleEndian>()?;

        // 3. Read Dim (u16)
        let dim = cursor.read_u16::<LittleEndian>()?;

        // Basic Vector Length Check
        let vector_end = 11 + (dim as usize * 4);
        if data.len() < vector_end {
            return Err(KernelError::InvalidPayloadLength {
                expected: vector_end,
                found: data.len(),
            });
        }

        // 4. Read Values
        let mut values = Vec::with_capacity(dim as usize);
        for _ in 0..dim {
            values.push(cursor.read_i32::<LittleEndian>()?);
        }
        
        // 5. Read Tag (u64)
        // If data ends after vector, tag is 0? 
        // No, we should enforce tag presence for V3 compatibility.
        // Wait, backward compatibility? Phase 3 didn't have tag.
        // For simplicity in this "Phase 4", we generally assume strict payload updates.
        // I'll read u64.
        
        let tag = if cursor.position() + 8 <= data.len() as u64 {
             cursor.read_u64::<LittleEndian>()?
        } else {
             // Fallback for Phase 3 payloads (0)
             0
        };

        // 6. Read Metadata (Optional)
        // [Len(u64) | Bytes...]
        let metadata = if cursor.position() < data.len() as u64 {
            // Read Metadata Length (u64)
            if (data.len() as u64 - cursor.position()) < 8 {
                 return Err(KernelError::InvalidPayloadLength { expected: cursor.position() as usize + 8, found: data.len() });
            }
            let meta_len = cursor.read_u64::<LittleEndian>()?;
            
            let current_pos = cursor.position();
            let remaining = data.len() as u64 - current_pos;
            if remaining != meta_len {
                 return Err(KernelError::InvalidPayloadLength { expected: (current_pos + meta_len) as usize, found: data.len() });
            }
            
            let mut meta_bytes = vec![0u8; meta_len as usize];
            use std::io::Read;
            cursor.read_exact(&mut meta_bytes)?;
            Some(meta_bytes)
        } else {
            None
        };

        Ok(Self {
            cmd,
            id,
            dim,
            values,
            tag,
            metadata,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct DeletePayload {
    pub cmd: u8,
    pub id: u64,
}

impl DeletePayload {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        // 1. Read Command (u8)
        let cmd = cursor.read_u8()?;
        if cmd != CMD_DELETE {
            return Err(KernelError::InvalidCommand(cmd));
        }

        // 2. Read ID (u64)
        let id = cursor.read_u64::<LittleEndian>()?;

        // Validate Length: 1 (cmd) + 8 (id) = 9 bytes.
        if data.len() != 9 {
             return Err(KernelError::InvalidPayloadLength {
                expected: 9,
                found: data.len(),
            });
        }

        Ok(Self { cmd, id })
    }
}
