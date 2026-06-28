//! Snapshot decoding.

use crate::state::kernel::KernelState;
use crate::error::{Result, KernelError};
use crate::types::id::{Version, RecordId, NodeId, EdgeId, NS_LIST_NIL};
// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::vector::FxpVector;
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

fn read_u16(buf: &[u8], offset: &mut usize) -> Result<u16> {
    if *offset + 2 > buf.len() { return Err(KernelError::InvalidOperation); }
    let bytes = buf[*offset..*offset+2].try_into().map_err(|_| KernelError::InvalidOperation)?;
    *offset += 2;
    Ok(u16::from_le_bytes(bytes))
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
    // We support V1 through V6
    if schema_ver < 1 || schema_ver > 6 {
        return Err(KernelError::InvalidOperation); // Version mismatch
    }

    let version_val = read_u64(buf, &mut offset)?;

    // Read 4 u32s that used to be capacities, but in V3 they are dim and lengths
    let _cap_records = read_u32(buf, &mut offset)?;
    let dim = read_u32(buf, &mut offset)?;
    let _cap_nodes = read_u32(buf, &mut offset)?;
    let _cap_edges = read_u32(buf, &mut offset)?;

    // V5: arithmetic format byte. Pre-V5 snapshots are implicitly Q16.16.
    // Restoring a snapshot produced under a different format would silently
    // corrupt every distance computation, so a mismatch is refused.
    if schema_ver >= 5 {
        let format_id = read_u8(buf, &mut offset)?;
        if format_id != crate::fxp::format::ACTIVE_FORMAT_ID {
            return Err(KernelError::InvalidOperation);
        }
    }

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

            // V6: namespace_id + linked-list pointers
            let (namespace_id, next_in_ns, prev_in_ns) = if schema_ver >= 6 {
                let ns = read_u16(buf, &mut offset)?;
                use crate::types::id::MAX_NAMESPACES;
                if ns as usize >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                (ns, read_u32(buf, &mut offset)?, read_u32(buf, &mut offset)?)
            } else {
                (0u16, NS_LIST_NIL, NS_LIST_NIL)
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
                namespace_id,
                next_in_ns,
                prev_in_ns,
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
        // V6: namespace_id + linked-list pointers
        let (node_namespace_id, node_next_in_ns, node_prev_in_ns) = if schema_ver >= 6 {
            let ns = read_u16(buf, &mut offset)?;
            use crate::types::id::MAX_NAMESPACES;
            if ns as usize >= MAX_NAMESPACES {
                return Err(KernelError::InvalidOperation);
            }
            (ns, read_u32(buf, &mut offset)?, read_u32(buf, &mut offset)?)
        } else {
            (0u16, NS_LIST_NIL, NS_LIST_NIL)
        };

        state.nodes.nodes[idx] = Some(GraphNode {
            id: NodeId(id_val),
            kind,
            record,
            first_out_edge: first_out,
            first_in_edge: first_in,
            namespace_id: node_namespace_id,
            next_in_ns: node_next_in_ns,
            prev_in_ns: node_prev_in_ns,
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

    // V6: read explicit namespace head arrays.
    // V1-V5: all namespace_id fields default to 0; rebuild lists from them.
    use crate::types::id::MAX_NAMESPACES;
    if schema_ver >= 6 {
        for i in 0..MAX_NAMESPACES {
            state.namespace_record_heads[i] = read_u32(buf, &mut offset)?;
        }
        for i in 0..MAX_NAMESPACES {
            state.namespace_node_heads[i] = read_u32(buf, &mut offset)?;
        }
    } else {
        // Reconstruct namespace lists — all records/nodes are in namespace 0
        state.rebuild_namespace_lists();
    }

    Ok(state)
}
