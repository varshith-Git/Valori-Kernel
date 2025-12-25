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

const WAL_VERSION: u8 = 1;
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

pub enum ApplyResult {
    Applied(usize),
    Incomplete,
    Error,
}

/// Try to apply a single command from the buffer.
/// Returns byte count consumed, or status.
pub fn try_apply_command<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &mut KernelState<M, D, N, E>,
    buf: &[u8]
) -> ApplyResult {
    let mut offset = 0;

    // 1. Check WAL Version (Only if at start of buffer? No, Version is Stream Header?
    // Wait, users previous prompt "Each packet includes WAL_VERSION... chunk data".
    // Is the "WAL Stream" versioned, or the "Command Log" versioned?
    // In Phase 3, I put `WAL_VERSION` byte at start of `apply_wal_log`.
    // In Phase 4, the *Stream* has a version in Packet Header.
    // Does the *Payload* (the concatenated command log) have a version?
    // Phase 3 `main.rs` constructed payload with `0x01` at index 0.
    // If we buffer chunks, the first byte of the *assembled stream* is Sequence 0?
    // Or is every command versioned?
    // Phase 3 `wal.rs` checks version *once* at start of `apply_wal_log`.
    // If we are streaming, we only see the start once (at the beginning of time/segment).
    // The `ShadowKernel` should handle the "Stream Header" or "Log Header" byte.
    // BUT `try_apply_command` implies applying *commands*.
    // The Version Byte is NOT a command.
    // I should treat the Version Byte as a "Header" that must be consumed 
    // before processing commands.
    // I will add `consume_header` or just handle it in Shadow logic?
    // Simpler: `try_apply_command` handles Opcode.
    // The `Version` check in `wal.rs` was for the whole buffer.
    // I should refactor `wal.rs` to NOT expect Version byte in `try_apply_command`?
    // Or `WAL_OP_VERSION`?
    // Current `wal.rs` expects Byte 0 = Version.
    // If I split this, `try_apply_command` should probably just look for Opcodes.
    // And `ShadowKernel` handles the initial Version Byte consumption.
    // "Reserve byte 0 = WAL format version".
    // I will stick to "First byte of entire log is version".
    // ShadowKernel needs to know if it has processed header.
    
    // Command Parsing
    if buf.is_empty() { return ApplyResult::Incomplete; }
    
    // Peek Opcode
    let opcode = buf[0];
    offset += 1; // Consume opcode check placeholder (will re-read or just assume)
    
    match opcode {
        WAL_OP_INSERT => {
            // Opcode(1) + ID(4) + Dim(2)
            if buf.len() < 7 { return ApplyResult::Incomplete; }
            
            // Read headers to get dim (to know size)
            // But I don't want to advance `offset` destructively if incomplete?
            // `read_u*` checks bounds.
            
            // Re-read carefully
            let mut probe = 0;
            let _op = read_u8(buf, &mut probe).unwrap(); // 1
            let rid_res = read_u32(buf, &mut probe); // +4 = 5
            let dim_res = read_u16(buf, &mut probe); // +2 = 7
            
            if rid_res.is_err() || dim_res.is_err() { return ApplyResult::Incomplete; }
            
            let _rid = rid_res.unwrap();
            let dim = dim_res.unwrap();
            
            if dim as usize != D { return ApplyResult::Error; }
            
            let payload_size = (D * 4) as usize;
            if buf.len() < 7 + payload_size { return ApplyResult::Incomplete; }
            
            // Full command available. Execute.
            offset = 0;
            let _ = read_u8(buf, &mut offset); // Op
            let rid = read_u32(buf, &mut offset).unwrap();
            let _ = read_u16(buf, &mut offset).unwrap(); // Dim
            
            let mut vector = FxpVector::<D>::new_zeros();
            for i in 0..D {
                vector.data[i] = FxpScalar(read_i32(buf, &mut offset).unwrap());
            }
            
            // Apply
             let id = RecordId(rid);
             let cmd = Command::InsertRecord { id, vector };
             if state.apply(&cmd).is_err() { return ApplyResult::Error; }
             
             return ApplyResult::Applied(offset);
        }
        _ => return ApplyResult::Error,
    }
}
