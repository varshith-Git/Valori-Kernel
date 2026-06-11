//! Snapshot decoding.

use crate::state::kernel::KernelState;
use crate::error::{Result, KernelError};
use crate::types::id::{Version, RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
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
    // We support V1, V2, V3, and V4
    if schema_ver < 1 || schema_ver > 4 {
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
    let total_slots = read_u32(buf, &mut offset)?;
    for i in 0..total_slots {
        let is_present = read_u8(buf, &mut offset)?;
        if is_present == 1 {
            let id_val = read_u32(buf, &mut offset)?;
            let flags = read_u8(buf, &mut offset)?;
            let tag = if schema_ver >= 3 {
                read_u64(buf, &mut offset)?
            } else {
                0
            };
            let d_len = state.dim.unwrap_or(0);
            let mut vector = FxpVector::new_zeros(d_len);
            for i in 0..d_len {
                vector.data[i] = FxpScalar(read_i32(buf, &mut offset)?);
            }

            let metadata = if schema_ver >= 2 {
                let meta_len = read_u32(buf, &mut offset)?;
                if meta_len > 0 {
                    let len = meta_len as usize;
                    if offset + len > buf.len() {
                        return Err(KernelError::InvalidOperation);
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

            let idx = i as usize;
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
        } else {
            // Hole
            let idx = i as usize;
            if idx >= state.records.records.len() {
                state.records.records.resize(idx + 1, None);
            }
            state.records.records[idx] = None;
        }
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

        let has_out = read_u8(buf, &mut offset)?;
        let first_out = if has_out == 1 {
            Some(EdgeId(read_u32(buf, &mut offset)?))
        } else {
            None
        };

        // V4: incoming edge back-pointer head (absent in V1-V3, reconstructed below)
        let first_in = if schema_ver >= 4 {
            let has_in = read_u8(buf, &mut offset)?;
            if has_in == 1 {
                Some(EdgeId(read_u32(buf, &mut offset)?))
            } else {
                None
            }
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
            first_in_edge: first_in,
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

        let has_next_out = read_u8(buf, &mut offset)?;
        let next_out = if has_next_out == 1 {
            Some(EdgeId(read_u32(buf, &mut offset)?))
        } else {
            None
        };

        // V4: incoming list back-pointer (absent in V1-V3, reconstructed below)
        let next_in = if schema_ver >= 4 {
            let has_next_in = read_u8(buf, &mut offset)?;
            if has_next_in == 1 {
                Some(EdgeId(read_u32(buf, &mut offset)?))
            } else {
                None
            }
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
            next_in,
        });
    }

    // V1-V3 back-compat: reconstruct incoming edge pointers from the edge list.
    // For each edge (A → B), prepend it to B's first_in_edge list.
    // This is O(E) but runs only once on first load of an old snapshot.
    if schema_ver < 4 {
        let edge_targets: alloc::vec::Vec<(EdgeId, NodeId)> = state
            .edges
            .edges
            .iter()
            .filter_map(|slot| slot.as_ref())
            .map(|e| (e.id, e.to))
            .collect();

        for (eid, to) in edge_targets {
            let head = state.nodes.get(to).and_then(|n| n.first_in_edge);
            if let Some(e) = state.edges.get_mut(eid) {
                e.next_in = head;
            }
            if let Some(n) = state.nodes.get_mut(to) {
                n.first_in_edge = Some(eid);
            }
        }
    }

    Ok(state)
}
