//! Snapshot decoding.

use crate::state::kernel::KernelState;
use crate::error::{Result, KernelError};
use crate::types::id::{Version, RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::scalar::FxpScalar;
use crate::storage::record::Record;
use crate::graph::node::GraphNode;
use crate::graph::edge::GraphEdge;
use crate::types::enums::{NodeKind, EdgeKind};

fn read_u32(buf: &[u8], offset: &mut usize) -> Result<u32> {
    if *offset + 4 > buf.len() { return Err(KernelError::InvalidOperation); } // Malformed
    let bytes = buf[*offset..*offset+4].try_into().map_err(|_| KernelError::InvalidOperation)?;
    *offset += 4;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(buf: &[u8], offset: &mut usize) -> Result<u64> {
    if *offset + 8 > buf.len() { return Err(KernelError::InvalidOperation); }
    let bytes = buf[*offset..*offset+8].try_into().map_err(|_| KernelError::InvalidOperation)?;
    *offset += 8;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u8(buf: &[u8], offset: &mut usize) -> Result<u8> {
    if *offset + 1 > buf.len() { return Err(KernelError::InvalidOperation); }
    let val = buf[*offset];
    *offset += 1;
    Ok(val)
}

fn read_i32(buf: &[u8], offset: &mut usize) -> Result<i32> {
    if *offset + 4 > buf.len() { return Err(KernelError::InvalidOperation); }
    let bytes = buf[*offset..*offset+4].try_into().map_err(|_| KernelError::InvalidOperation)?;
    *offset += 4;
    Ok(i32::from_le_bytes(bytes))
}

pub fn decode_state(
    buf: &[u8],
) -> Result<KernelState> {
    let mut offset = 0;
    
    // Header
    if offset + 4 > buf.len() { return Err(KernelError::InvalidOperation); }
    if &buf[offset..offset+4] != crate::snapshot::encode::MAGIC {
        return Err(KernelError::InvalidOperation); // Bad Magic
    }
    offset += 4;

    let schema_ver = read_u32(buf, &mut offset)?;
    // We support V1, V2, and V3
    if schema_ver != 1 && schema_ver != 2 && schema_ver != 3 {
        return Err(KernelError::InvalidOperation); // Version mismatch
    }

    let version_val = read_u64(buf, &mut offset)?;
    
    // Read 4 u32s that used to be capacities, but in V3 they are dim and lengths
    let _cap_records = read_u32(buf, &mut offset)?;
    let dim = read_u32(buf, &mut offset)?;
    let _cap_nodes = read_u32(buf, &mut offset)?;
    let _cap_edges = read_u32(buf, &mut offset)?;
    
    let mut state = KernelState::new();
    state.version = Version(version_val);
    if dim > 0 {
        state.dim = Some(dim as usize);
    }

    // Records
    let record_count = read_u32(buf, &mut offset)?;
    for _ in 0..record_count {
        let id_val = read_u32(buf, &mut offset)?;
        let flags = read_u8(buf, &mut offset)?;
        
        let tag = if schema_ver >= 3 {
            read_u64(buf, &mut offset)?
        } else {
            0
        };
        let d_len = state.dim.unwrap_or(0);
        let mut vector = FxpVector::new_zeros(d_len);
        // We must know how many elements to read. V1/V2 read 'dim' from header.
        for i in 0..d_len {
            vector.data[i] = FxpScalar(read_i32(buf, &mut offset)?);
        }

        // Metadata V2 logic
        let metadata = if schema_ver >= 2 {
            let meta_len = read_u32(buf, &mut offset)?;
            if meta_len > 0 {
                let len = meta_len as usize;
                if offset + len > buf.len() {
                    return Err(KernelError::InvalidOperation); // Truncated
                }
                let mut bytes = alloc::vec![0u8; len];
                bytes.copy_from_slice(&buf[offset..offset+len]);
                offset += len;
                Some(bytes)
            } else {
                None
            }
        } else {
            None
        };
        
        let idx = id_val as usize;
        if idx >= state.records.records.len() {
            state.records.records.resize(idx + 1, None);
        }
        state.records.records[idx] = Some(Record {
            id: RecordId(id_val),
            vector,
            metadata,
            tag,
            flags,
        });
    }

    // Nodes
    let node_count = read_u32(buf, &mut offset)?;
    for _ in 0..node_count {
        let id_val = read_u32(buf, &mut offset)?;
        let kind_val = read_u8(buf, &mut offset)?;
        let kind = NodeKind::from_u8(kind_val).ok_or(KernelError::InvalidOperation)?;
        
        let has_record = read_u8(buf, &mut offset)?;
        let record = if has_record == 1 {
            Some(RecordId(read_u32(buf, &mut offset)?))
        } else {
            None
        };

        let has_edge = read_u8(buf, &mut offset)?;
        let first_out = if has_edge == 1 {
            Some(EdgeId(read_u32(buf, &mut offset)?))
        } else {
            None
        };

        let idx = id_val as usize;
        if idx >= state.nodes.nodes.len() {
            state.nodes.nodes.resize(idx + 1, None);
        }
        state.nodes.nodes[idx] = Some(GraphNode {
            id: NodeId(id_val),
            kind,
            record,
            first_out_edge: first_out,
        });
    }

    // Edges
    let edge_count = read_u32(buf, &mut offset)?;
    for _ in 0..edge_count {
        let id_val = read_u32(buf, &mut offset)?;
        let kind_val = read_u8(buf, &mut offset)?;
        let kind = EdgeKind::from_u8(kind_val).ok_or(KernelError::InvalidOperation)?;

        let from = NodeId(read_u32(buf, &mut offset)?);
        let to = NodeId(read_u32(buf, &mut offset)?);

        let has_next = read_u8(buf, &mut offset)?;
        let next_out = if has_next == 1 {
            Some(EdgeId(read_u32(buf, &mut offset)?))
        } else {
            None
        };

        let idx = id_val as usize;
        if idx >= state.edges.edges.len() {
            state.edges.edges.resize(idx + 1, None);
        }
        state.edges.edges[idx] = Some(GraphEdge {
            id: EdgeId(id_val),
            kind,
            from,
            to,
            next_out,
        });
    }

    Ok(state)
}
