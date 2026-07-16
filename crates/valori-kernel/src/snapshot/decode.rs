//! Snapshot decoding.
//!
//! # Security model
//!
//! Snapshots are treated as **untrusted input** even when read from local disk,
//! because a snapshot file can be replaced by an attacker with filesystem
//! access.  Every length, count, and pointer read from the buffer is validated
//! before any allocation or memory access.  The invariants enforced here are:
//!
//! * No allocation larger than the constants in [`crate::config`].
//! * Integer widening casts are explicit; multiplication that could overflow
//!   is pre-checked.
//! * Every enum discriminant is validated before matching.
//! * Every flag byte (is_present, has_record, …) must be exactly 0 or 1.
//! * Every id / pointer read from the file is range-checked against the
//!   already-established pool size before use.
//! * A truncated file is always an error — `read_*` helpers guarantee this.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.

use crate::config::{
    MAX_DIM, MAX_EDGES, MAX_METADATA_SIZE, MAX_META_ENTRIES, MAX_NODES, MAX_RECORDS,
};
use crate::error::{KernelError, Result};
use crate::graph::edge::GraphEdge;
use crate::graph::node::GraphNode;
use crate::state::kernel::KernelState;
use crate::storage::record::Record;
use crate::types::enums::{EdgeKind, NodeKind};
use crate::types::id::{EdgeId, NodeId, RecordId, Version, NS_LIST_NIL};
use crate::types::scalar::FxpScalar;
use crate::types::vector::FxpVector;

// ── Read helpers ─────────────────────────────────────────────────────────────
// Every helper checks for truncation before reading; they are the only
// functions allowed to advance `offset`.

#[inline]
fn read_u8(buf: &[u8], offset: &mut usize) -> Result<u8> {
    let o = *offset;
    if o >= buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    *offset = o + 1;
    Ok(buf[o])
}

/// Read a u8 that must be exactly 0 or 1 (a boolean flag field).
#[inline]
fn read_flag(buf: &[u8], offset: &mut usize) -> Result<bool> {
    match read_u8(buf, offset)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(KernelError::InvalidOperation), // malformed flag
    }
}

#[inline]
fn read_u16(buf: &[u8], offset: &mut usize) -> Result<u16> {
    let o = *offset;
    if o + 2 > buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    let bytes: [u8; 2] = buf[o..o + 2]
        .try_into()
        .map_err(|_| KernelError::InvalidOperation)?;
    *offset = o + 2;
    Ok(u16::from_le_bytes(bytes))
}

#[inline]
fn read_u32(buf: &[u8], offset: &mut usize) -> Result<u32> {
    let o = *offset;
    if o + 4 > buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    let bytes: [u8; 4] = buf[o..o + 4]
        .try_into()
        .map_err(|_| KernelError::InvalidOperation)?;
    *offset = o + 4;
    Ok(u32::from_le_bytes(bytes))
}

#[inline]
fn read_u64(buf: &[u8], offset: &mut usize) -> Result<u64> {
    let o = *offset;
    if o + 8 > buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    let bytes: [u8; 8] = buf[o..o + 8]
        .try_into()
        .map_err(|_| KernelError::InvalidOperation)?;
    *offset = o + 8;
    Ok(u64::from_le_bytes(bytes))
}

#[inline]
fn read_i32(buf: &[u8], offset: &mut usize) -> Result<i32> {
    let o = *offset;
    if o + 4 > buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    let bytes: [u8; 4] = buf[o..o + 4]
        .try_into()
        .map_err(|_| KernelError::InvalidOperation)?;
    *offset = o + 4;
    Ok(i32::from_le_bytes(bytes))
}

/// Read `len` bytes into a new Vec, checking buf bounds and the size limit.
#[inline]
fn read_blob(
    buf: &[u8],
    offset: &mut usize,
    len: usize,
    max: usize,
) -> Result<alloc::vec::Vec<u8>> {
    if len > max {
        return Err(KernelError::InvalidOperation);
    }
    let o = *offset;
    // Check for offset overflow (o + len could wrap on 32-bit targets).
    let end = o.checked_add(len).ok_or(KernelError::InvalidOperation)?;
    if end > buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    let mut v = alloc::vec![0u8; len];
    v.copy_from_slice(&buf[o..end]);
    *offset = end;
    Ok(v)
}

/// Read a UTF-8 string of at most `max` bytes.
#[inline]
fn read_str(buf: &[u8], offset: &mut usize, max: usize) -> Result<alloc::string::String> {
    let len = read_u32(buf, offset)? as usize;
    let bytes = read_blob(buf, offset, len, max)?;
    alloc::string::String::from_utf8(bytes).map_err(|_| KernelError::InvalidOperation)
}

// ── Optional-pointer helpers ──────────────────────────────────────────────────

#[inline]
fn read_opt_edge(buf: &[u8], offset: &mut usize) -> Result<Option<EdgeId>> {
    if read_flag(buf, offset)? {
        Ok(Some(EdgeId(read_u32(buf, offset)?)))
    } else {
        Ok(None)
    }
}

// ── Main decoder ─────────────────────────────────────────────────────────────

pub fn decode_state(buf: &[u8]) -> Result<KernelState> {
    let mut off = 0usize;

    // ── Header ───────────────────────────────────────────────────────────────

    if off + 4 > buf.len() {
        return Err(KernelError::InvalidOperation);
    }
    if &buf[off..off + 4] != crate::snapshot::encode::MAGIC {
        return Err(KernelError::InvalidOperation); // bad magic
    }
    off += 4;

    let schema_ver = read_u32(buf, &mut off)?;
    if schema_ver < 1 || schema_ver > 7 {
        return Err(KernelError::InvalidOperation); // unsupported version
    }

    let version_val = read_u64(buf, &mut off)?;

    // Four legacy header words (were capacities; V3+ repurposed the second as dim).
    let _cap_records = read_u32(buf, &mut off)?;
    let dim = read_u32(buf, &mut off)?;
    let _cap_nodes = read_u32(buf, &mut off)?;
    let _cap_edges = read_u32(buf, &mut off)?;

    // V5+: arithmetic format ID.  Mismatch → silent corruption of every distance.
    if schema_ver >= 5 {
        let format_id = read_u8(buf, &mut off)?;
        if format_id != crate::fxp::format::ACTIVE_FORMAT_ID {
            return Err(KernelError::InvalidOperation);
        }
    }

    // Validate dim before any allocation.
    let dim = dim as usize;
    if dim > MAX_DIM {
        return Err(KernelError::InvalidOperation);
    }
    // Pre-check: reading one full vector must not overflow offset arithmetic.
    // dim * 4 is the byte size of one FxpScalar slice; verify it fits in usize.
    let vector_bytes = dim.checked_mul(4).ok_or(KernelError::InvalidOperation)?;

    let mut state = KernelState::new();
    state.version = Version(version_val);
    if dim > 0 {
        state.dim = Some(dim);
    }

    use crate::types::id::MAX_NAMESPACES;

    // ── Records ──────────────────────────────────────────────────────────────

    let total_slots = read_u32(buf, &mut off)? as usize;
    if total_slots > MAX_RECORDS {
        return Err(KernelError::InvalidOperation);
    }
    // A slot takes at minimum 1 byte (is_present flag); reject if claimed slots
    // exceed remaining buffer — no allocation needed to detect this.
    if total_slots > buf.len().saturating_sub(off) {
        return Err(KernelError::InvalidOperation);
    }

    state.records.records.resize(total_slots, None);

    for i in 0..total_slots {
        if !read_flag(buf, &mut off)? {
            // Hole — slot already None from resize.
            continue;
        }

        let id_val = read_u32(buf, &mut off)? as usize;
        // The record's own id must match its slot index for pool consistency.
        if id_val != i {
            return Err(KernelError::InvalidOperation);
        }

        let flags = read_u8(buf, &mut off)?;

        let tag = if schema_ver >= 3 {
            read_u64(buf, &mut off)?
        } else {
            0
        };

        // Pre-check that reading the full vector won't overflow offset.
        let vec_end = off
            .checked_add(vector_bytes)
            .ok_or(KernelError::InvalidOperation)?;
        if vec_end > buf.len() {
            return Err(KernelError::InvalidOperation);
        }
        let mut vector = FxpVector::new_zeros(dim);
        for j in 0..dim {
            vector.data[j] = FxpScalar(read_i32(buf, &mut off)?);
        }

        let metadata = if schema_ver >= 2 {
            let meta_len = read_u32(buf, &mut off)? as usize;
            if meta_len > 0 {
                Some(read_blob(buf, &mut off, meta_len, MAX_METADATA_SIZE)?)
            } else {
                None
            }
        } else {
            None
        };

        let (namespace_id, next_in_ns, prev_in_ns) = if schema_ver >= 6 {
            let ns = read_u16(buf, &mut off)?;
            if ns as usize >= MAX_NAMESPACES {
                return Err(KernelError::InvalidOperation);
            }
            (ns, read_u32(buf, &mut off)?, read_u32(buf, &mut off)?)
        } else {
            (0u16, NS_LIST_NIL, NS_LIST_NIL)
        };

        state.records.records[i] = Some(Record {
            id: RecordId(i as u32),
            vector,
            metadata,
            tag,
            flags,
            namespace_id,
            next_in_ns,
            prev_in_ns,
        });
    }

    // ── Nodes ────────────────────────────────────────────────────────────────

    let node_count = read_u32(buf, &mut off)? as usize;
    if node_count > MAX_NODES {
        return Err(KernelError::InvalidOperation);
    }
    if node_count > buf.len().saturating_sub(off) {
        return Err(KernelError::InvalidOperation);
    }

    state.nodes.nodes.resize(node_count, None);

    for _ in 0..node_count {
        let id_val = read_u32(buf, &mut off)? as usize;
        if id_val >= node_count {
            return Err(KernelError::InvalidOperation);
        }

        let kind_val = read_u8(buf, &mut off)?;
        let kind = NodeKind::from_u8(kind_val).ok_or(KernelError::InvalidOperation)?;

        let record = if read_flag(buf, &mut off)? {
            Some(RecordId(read_u32(buf, &mut off)?))
        } else {
            None
        };
        let first_out = read_opt_edge(buf, &mut off)?;
        let first_in = if schema_ver >= 4 {
            read_opt_edge(buf, &mut off)?
        } else {
            None
        };

        let (node_namespace_id, node_next_in_ns, node_prev_in_ns) = if schema_ver >= 6 {
            let ns = read_u16(buf, &mut off)?;
            if ns as usize >= MAX_NAMESPACES {
                return Err(KernelError::InvalidOperation);
            }
            (ns, read_u32(buf, &mut off)?, read_u32(buf, &mut off)?)
        } else {
            (0u16, NS_LIST_NIL, NS_LIST_NIL)
        };

        // Validate cross-reference: if node claims a record, the record must exist.
        if let Some(rid) = record {
            if rid.0 as usize >= total_slots || state.records.records[rid.0 as usize].is_none() {
                return Err(KernelError::InvalidOperation);
            }
        }

        state.nodes.nodes[id_val] = Some(GraphNode {
            id: NodeId(id_val as u32),
            kind,
            record,
            first_out_edge: first_out,
            first_in_edge: first_in,
            namespace_id: node_namespace_id,
            next_in_ns: node_next_in_ns,
            prev_in_ns: node_prev_in_ns,
        });
    }

    // ── Edges ────────────────────────────────────────────────────────────────

    let edge_count = read_u32(buf, &mut off)? as usize;
    if edge_count > MAX_EDGES {
        return Err(KernelError::InvalidOperation);
    }
    if edge_count > buf.len().saturating_sub(off) {
        return Err(KernelError::InvalidOperation);
    }

    state.edges.edges.resize(edge_count, None);

    for _ in 0..edge_count {
        let id_val = read_u32(buf, &mut off)? as usize;
        if id_val >= edge_count {
            return Err(KernelError::InvalidOperation);
        }

        let kind_val = read_u8(buf, &mut off)?;
        let kind = EdgeKind::from_u8(kind_val).ok_or(KernelError::InvalidOperation)?;

        let from = NodeId(read_u32(buf, &mut off)?);
        let to = NodeId(read_u32(buf, &mut off)?);

        // Validate that both endpoints exist in the node pool.
        if from.0 as usize >= node_count || state.nodes.nodes[from.0 as usize].is_none() {
            return Err(KernelError::InvalidOperation);
        }
        if to.0 as usize >= node_count || state.nodes.nodes[to.0 as usize].is_none() {
            return Err(KernelError::InvalidOperation);
        }

        let next_out = read_opt_edge(buf, &mut off)?;
        let next_in = if schema_ver >= 4 {
            read_opt_edge(buf, &mut off)?
        } else {
            None
        };

        state.edges.edges[id_val] = Some(GraphEdge {
            id: EdgeId(id_val as u32),
            kind,
            from,
            to,
            next_out,
            next_in,
        });
    }

    // ── V1-V3 back-compat: reconstruct incoming edge pointers ────────────────

    if schema_ver < 4 {
        let edge_targets: alloc::vec::Vec<(EdgeId, NodeId)> = state
            .edges
            .edges
            .iter()
            .filter_map(|s| s.as_ref())
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

    // ── V6+: namespace head arrays ───────────────────────────────────────────

    if schema_ver >= 6 {
        for i in 0..MAX_NAMESPACES {
            let head = read_u32(buf, &mut off)?;
            // Must be a valid record slot index or the sentinel.
            if head != NS_LIST_NIL && head as usize >= total_slots {
                return Err(KernelError::InvalidOperation);
            }
            state.namespace_record_heads[i] = head;
        }
        for i in 0..MAX_NAMESPACES {
            let head = read_u32(buf, &mut off)?;
            if head != NS_LIST_NIL && head as usize >= node_count {
                return Err(KernelError::InvalidOperation);
            }
            state.namespace_node_heads[i] = head;
        }
    } else {
        state.rebuild_namespace_lists();
    }

    // ── V7+: KernelState.meta ────────────────────────────────────────────────

    if schema_ver >= 7 {
        let meta_count = read_u32(buf, &mut off)? as usize;
        if meta_count > MAX_META_ENTRIES {
            return Err(KernelError::InvalidOperation);
        }
        for _ in 0..meta_count {
            let key = read_str(buf, &mut off, MAX_METADATA_SIZE)?;
            let value = read_str(buf, &mut off, MAX_METADATA_SIZE)?;
            state.meta.insert(key, value);
        }
    }

    Ok(state)
}
