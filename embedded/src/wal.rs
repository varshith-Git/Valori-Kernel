// This module defines Valori Embedded deterministic WAL replay mode.
//
// Given:
//     Initial state S0
//     Command log L
//
// Applying:
//     Device_A = Apply(S0, L)
//     Device_B = Apply(S0, L)
//
// Guarantee:
//     hash(Device_A) == hash(Device_B)
//
// This establishes cross-architecture state convergence.
// The MCU does not create memory — it proves it.

use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;

const WAL_OP_INSERT: u8 = 0x00;

fn read_u8(buf: &[u8], offset: &mut usize) -> Result<u8, ()> {
    if *offset + 1 > buf.len() { return Err(()); }
    let val = buf[*offset];
    *offset += 1;
    Ok(val)
}

fn read_u16(buf: &[u8], offset: &mut usize) -> Result<u16, ()> {
    if *offset + 2 > buf.len() { return Err(()); }
    let bytes: [u8; 2] = buf[*offset..*offset+2].try_into().map_err(|_| ())?;
    *offset += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(buf: &[u8], offset: &mut usize) -> Result<u32, ()> {
    if *offset + 4 > buf.len() { return Err(()); }
    let bytes: [u8; 4] = buf[*offset..*offset+4].try_into().map_err(|_| ())?;
    *offset += 4;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(buf: &[u8], offset: &mut usize) -> Result<i32, ()> {
    if *offset + 4 > buf.len() { return Err(()); }
    let bytes: [u8; 4] = buf[*offset..*offset+4].try_into().map_err(|_| ())?;
    *offset += 4;
    Ok(i32::from_le_bytes(bytes))
}

pub fn apply_wal_log<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &mut KernelState<M, D, N, E>,
    wal_bytes: &[u8]
) -> Result<(), ()> {
    let mut offset = 0;
    
    // Process until buffer exhausted
    while offset < wal_bytes.len() {
        let opcode = read_u8(wal_bytes, &mut offset)?;
        
        match opcode {
            WAL_OP_INSERT => {
                // Format: RecordID(u32) | Dim(u16) | Values...
                let rid = read_u32(wal_bytes, &mut offset)?;
                let dim = read_u16(wal_bytes, &mut offset)?;
                
                if dim as usize != D {
                    return Err(()); // Dimension mismatch
                }
                
                let mut vector = FxpVector::<D>::new_zeros();
                for i in 0..D {
                    let val = read_i32(wal_bytes, &mut offset)?;
                    vector.data[i] = FxpScalar(val);
                }
                
                // Apply to Kernel
                let id = RecordId(rid);
                let cmd = Command::InsertRecord { id, vector };
                
                state.apply(&cmd).map_err(|_| ())?;
            }
            _ => {
                return Err(()); // Invalid Opcode
            }
        }
    }
    
    Ok(())
}
