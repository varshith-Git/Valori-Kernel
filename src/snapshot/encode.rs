//! Snapshot encoding.

use crate::state::kernel::KernelState;
use crate::error::{Result, KernelError};

pub const MAGIC: &[u8; 4] = b"VALK";
pub const SCHEMA_VERSION: u32 = 1;

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

fn write_u64(buf: &mut [u8], offset: &mut usize, val: u64) -> Result<()> {
    if *offset + 8 > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    let bytes = val.to_le_bytes();
    buf[*offset..*offset + 8].copy_from_slice(&bytes);
    *offset += 8;
    Ok(())
}

fn write_u8(buf: &mut [u8], offset: &mut usize, val: u8) -> Result<()> {
    if *offset + 1 > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    buf[*offset] = val;
    *offset += 1;
    Ok(())
}

fn write_i32(buf: &mut [u8], offset: &mut usize, val: i32) -> Result<()> {
    if *offset + 4 > buf.len() {
        return Err(KernelError::CapacityExceeded);
    }
    let bytes = val.to_le_bytes();
    buf[*offset..*offset + 4].copy_from_slice(&bytes);
    *offset += 4;
    Ok(())
}

pub fn encode_state<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    state: &KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
    buf: &mut [u8],
) -> Result<usize> {
    let mut offset = 0;

    // Header
    if offset + 4 > buf.len() { return Err(KernelError::CapacityExceeded); }
    buf[offset..offset+4].copy_from_slice(MAGIC);
    offset += 4;

    write_u32(buf, &mut offset, SCHEMA_VERSION)?;
    write_u64(buf, &mut offset, state.version.0)?;
    
    // Capacities (to check compatibility on restore)
    write_u32(buf, &mut offset, MAX_RECORDS as u32)?;
    write_u32(buf, &mut offset, D as u32)?;
    write_u32(buf, &mut offset, MAX_NODES as u32)?;
    write_u32(buf, &mut offset, MAX_EDGES as u32)?;

    // Records
    let record_count = state.records.len() as u32;
    write_u32(buf, &mut offset, record_count)?;

    for record in state.records.iter() {
        write_u32(buf, &mut offset, record.id.0)?;
        write_u8(buf, &mut offset, record.flags)?;
        for scalar in record.vector.data.iter() {
            write_i32(buf, &mut offset, scalar.0)?;
        }
    }

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
        }
    }

    Ok(offset)
}
