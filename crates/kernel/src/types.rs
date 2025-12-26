use crate::error::{KernelError, Result};
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use std::io::Cursor;

pub const CMD_INSERT: u8 = 1;
pub const CMD_DELETE: u8 = 2;

#[derive(Debug, PartialEq)]
pub struct InsertPayload {
    pub cmd: u8,
    pub id: u64,
    pub dim: u16,
    pub values: Vec<i32>,
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

        // Validate Length:
        // Already read: 1 (cmd) + 8 (id) + 2 (dim) = 11 bytes.
        // Remaining should be: dim * 4 (i32 is 4 bytes).
        // Total expected: 11 + (dim * 4).
        let expected_len = 11 + (dim as usize * 4);
        if data.len() != expected_len {
            return Err(KernelError::InvalidPayloadLength {
                expected: expected_len,
                found: data.len(),
            });
        }

        // 4. Read Values
        let mut values = Vec::with_capacity(dim as usize);
        for _ in 0..dim {
            values.push(cursor.read_i32::<LittleEndian>()?);
        }

        Ok(Self {
            cmd,
            id,
            dim,
            values,
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
