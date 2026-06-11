//! Snapshot encoding.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::state::kernel::KernelState;
use crate::error::{Result, KernelError};

pub const MAGIC: &[u8; 4] = b"VALK";
pub const SCHEMA_VERSION: u32 = 5; // V5: arithmetic format_id byte after the capacity block

/// writes a u32 to the buffer at offset
fn write_u32(buf: &mut [u8], offset: &mut usize, val: u32) -> Result<()> {
    if *offset + 4 > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    let bytes = val.to_le_bytes();
    buf[*offset..*offset + 4].copy_from_slice(&bytes);
    *offset += 4;
    Ok(())
}

fn write_i32(buf: &mut [u8], offset: &mut usize, val: i32) -> Result<()> {
    write_u32(buf, offset, val as u32)
}

fn write_u8(buf: &mut [u8], offset: &mut usize, val: u8) -> Result<()> {
    if *offset + 1 > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    buf[*offset] = val;
    *offset += 1;
    Ok(())
}

fn write_u64(buf: &mut [u8], offset: &mut usize, val: u64) -> Result<()> {
    if *offset + 8 > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    let bytes = val.to_le_bytes();
    buf[*offset..*offset + 8].copy_from_slice(&bytes);
    *offset += 8;
    Ok(())
}

fn write_bytes(buf: &mut [u8], offset: &mut usize, data: &[u8]) -> Result<()> {
    if *offset + data.len() > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    buf[*offset..*offset + data.len()].copy_from_slice(data);
    *offset += data.len();
    Ok(())
}

pub fn encode_state(
    state: &KernelState,
    buf: &mut [u8],
) -> Result<usize> {
    let mut offset = 0;
    
    // Header
    write_bytes(buf, &mut offset, MAGIC)?;
    write_u32(buf, &mut offset, SCHEMA_VERSION)?; // Version
    write_u64(buf, &mut offset, state.version.0)?; // State Version

    // V3: Dynamic Capacities / Dimension
    // Order: [RecordsCap/Len][Dim][NodesCap/Len][EdgesCap/Len]
    // This matches V1/V2 header where D was the 2nd capacity field (offset 20).
    write_u32(buf, &mut offset, state.records.raw_records().len() as u32)?;
    write_u32(buf, &mut offset, state.dim.unwrap_or(0) as u32)?;
    write_u32(buf, &mut offset, state.nodes.raw_nodes().len() as u32)?;
    write_u32(buf, &mut offset, state.edges.raw_edges().len() as u32)?;

    // V5: arithmetic format — a snapshot is only meaningful under the
    // format that produced it (restore refuses a mismatch).
    write_u8(buf, &mut offset, crate::fxp::format::ACTIVE_FORMAT_ID)?;
    // Records
    let total_slots = state.records.raw_records().len() as u32;
    write_u32(buf, &mut offset, total_slots)?;

    for slot in state.records.raw_records() {
        if let Some(record) = slot {
            write_u8(buf, &mut offset, 1)?; // Present
            write_u32(buf, &mut offset, record.id.0)?;
            write_u8(buf, &mut offset, record.flags)?;
            write_u64(buf, &mut offset, record.tag)?;
            for scalar in record.vector.data.iter() {
                write_i32(buf, &mut offset, scalar.0)?;
            }
            match &record.metadata {
                Some(m) => {
                    write_u32(buf, &mut offset, m.len() as u32)?;
                    write_bytes(buf, &mut offset, m)?;
                }
                None => {
                    write_u32(buf, &mut offset, 0)?;
                }
            }
        } else {
            write_u8(buf, &mut offset, 0)?; // Absent
        }
    }
// ...

    // Nodes
    let mut node_count = 0;
    for slot in state.nodes.raw_nodes().iter() {
        if slot.is_some() { node_count += 1; }
    }
    write_u32(buf, &mut offset, node_count)?;

    for slot in state.nodes.raw_nodes().iter() {
        if let Some(node) = slot {
            write_u32(buf, &mut offset, node.id.0)?;
            write_u8(buf, &mut offset, node.kind as u8)?;
            
            match node.record {
                Some(rid) => {
                    write_u8(buf, &mut offset, 1)?;
                    write_u32(buf, &mut offset, rid.0)?;
                }
                None => write_u8(buf, &mut offset, 0)?,
            }

            match node.first_out_edge {
                Some(eid) => {
                    write_u8(buf, &mut offset, 1)?;
                    write_u32(buf, &mut offset, eid.0)?;
                }
                None => write_u8(buf, &mut offset, 0)?,
            }

            // V4: incoming edge back-pointer head
            match node.first_in_edge {
                Some(eid) => {
                    write_u8(buf, &mut offset, 1)?;
                    write_u32(buf, &mut offset, eid.0)?;
                }
                None => write_u8(buf, &mut offset, 0)?,
            }
        }
    }

    // Edges
    let mut edge_count = 0;
    for slot in state.edges.raw_edges().iter() {
        if slot.is_some() { edge_count += 1; }
    }
    write_u32(buf, &mut offset, edge_count)?;

    for slot in state.edges.raw_edges().iter() {
        if let Some(edge) = slot {
            write_u32(buf, &mut offset, edge.id.0)?;
            write_u8(buf, &mut offset, edge.kind as u8)?;
            write_u32(buf, &mut offset, edge.from.0)?;
            write_u32(buf, &mut offset, edge.to.0)?;
            
            match edge.next_out {
                Some(eid) => {
                    write_u8(buf, &mut offset, 1)?;
                    write_u32(buf, &mut offset, eid.0)?;
                }
                None => write_u8(buf, &mut offset, 0)?,
            }

            // V4: incoming list back-pointer
            match edge.next_in {
                Some(eid) => {
                    write_u8(buf, &mut offset, 1)?;
                    write_u32(buf, &mut offset, eid.0)?;
                }
                None => write_u8(buf, &mut offset, 0)?,
            }
        }
    }

    Ok(offset)
}
