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

pub fn decode_state<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    buf: &[u8],
) -> Result<KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>> {
    let mut offset = 0;
    
    // Header
    if offset + 4 > buf.len() { return Err(KernelError::InvalidOperation); }
    if &buf[offset..offset+4] != crate::snapshot::encode::MAGIC {
        return Err(KernelError::InvalidOperation); // Bad Magic
    }
    offset += 4;

    let schema_ver = read_u32(buf, &mut offset)?;
    if schema_ver != crate::snapshot::encode::SCHEMA_VERSION {
        return Err(KernelError::InvalidOperation); // Version mismatch
    }

    let version_val = read_u64(buf, &mut offset)?;
    
    // Verify Capacities
    let cap_records = read_u32(buf, &mut offset)?;
    let dim = read_u32(buf, &mut offset)?;
    let cap_nodes = read_u32(buf, &mut offset)?;
    let cap_edges = read_u32(buf, &mut offset)?;
    
    if cap_records != MAX_RECORDS as u32 || dim != D as u32 || cap_nodes != MAX_NODES as u32 || cap_edges != MAX_EDGES as u32 {
        // Mismatch in kernel configuration
        return Err(KernelError::InvalidOperation); 
    }

    let mut state = KernelState::new();
    state.version = Version(version_val);

    // Records
    let record_count = read_u32(buf, &mut offset)?;
    for _ in 0..record_count {
        let id_val = read_u32(buf, &mut offset)?;
        let flags = read_u8(buf, &mut offset)?;
        let mut vector = FxpVector::<D>::new_zeros();
        for i in 0..D {
            vector.data[i] = FxpScalar(read_i32(buf, &mut offset)?);
        }
        
        // Validation of ID bounds is implicit via array access check or capacity check above?
        // But id_val could be > MAX_RECORDS inside the byte stream even if cap matches.
        let idx = id_val as usize;
        if idx >= MAX_RECORDS {
            return Err(KernelError::CapacityExceeded);
        }
        state.records.records[idx] = Some(Record {
            id: RecordId(id_val),
            vector,
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
        if idx >= MAX_NODES { return Err(KernelError::CapacityExceeded); }
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
        if idx >= MAX_EDGES { return Err(KernelError::CapacityExceeded); }
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
