//! Snapshot encoding.
//!
//! # Design: growable Vec instead of fixed buffer
//!
//! All prior versions pre-allocated a fixed-size `&mut [u8]` and returned a
//! byte count.  That approach required a perfect size formula — any schema
//! addition that missed a field caused `CapacityExceeded` at runtime (the V6
//! namespace pointer fields caused exactly this bug at 1 M records).
//!
//! The new design follows every production serialiser (protobuf, bincode,
//! RocksDB SST writer, Qdrant's segment encoder): write into a `Vec<u8>` that
//! grows on demand.  `CapacityExceeded` from the encoder is now impossible.
//!
//! A capacity hint is computed once from the live record / node / edge counts
//! so the `Vec` avoids repeated reallocations on the hot path.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::error::Result;
use crate::state::kernel::KernelState;

pub const MAGIC: &[u8; 4] = b"VALK";
pub const SCHEMA_VERSION: u32 = 7; // V7: adds the KernelState.meta sidecar (SetMeta-committed key/value pairs)

// ── infallible push helpers ────────────────────────────────────────────────────
// Writing to a Vec<u8> can only fail on OOM, which panics (same as any alloc).
// No Result wrapping needed — the encoder is now infallible at the schema level.

#[inline(always)]
fn push_u8(out: &mut alloc::vec::Vec<u8>, val: u8) {
    out.push(val);
}

#[inline(always)]
fn push_u16(out: &mut alloc::vec::Vec<u8>, val: u16) {
    out.extend_from_slice(&val.to_le_bytes());
}

#[inline(always)]
fn push_u32(out: &mut alloc::vec::Vec<u8>, val: u32) {
    out.extend_from_slice(&val.to_le_bytes());
}

#[inline(always)]
fn push_i32(out: &mut alloc::vec::Vec<u8>, val: i32) {
    out.extend_from_slice(&val.to_le_bytes());
}

#[inline(always)]
fn push_u64(out: &mut alloc::vec::Vec<u8>, val: u64) {
    out.extend_from_slice(&val.to_le_bytes());
}

#[inline(always)]
fn push_bytes(out: &mut alloc::vec::Vec<u8>, data: &[u8]) {
    out.extend_from_slice(data);
}

// ── capacity hint ─────────────────────────────────────────────────────────────

/// Returns a byte estimate for pre-allocating the output Vec.
///
/// V6 per-record layout (present slot):
///   1 (flag) + 4 (id) + 1 (flags) + 8 (tag) + dim×4 (vector)
///   + 4 (metadata len) + 2 (namespace_id) + 4 (next_in_ns) + 4 (prev_in_ns)
///   = 28 + dim×4
///
/// Absent slot: 1 byte.  We pessimistically assume all slots are present.
pub fn encode_capacity_hint(state: &KernelState) -> usize {
    let dim = state.dim.unwrap_or(0);
    let total_slots = state.records.raw_records().len();
    let node_count = state.node_count();
    let edge_count = state.edge_count();

    64                                          // header
    + total_slots * (28 + dim * 4)             // records (V6 layout, all present)
    + node_count  * 30                         // nodes   (V6 layout)
    + edge_count  * 29                         // edges
    + 2 * 1024 * 4                             // namespace head arrays (2 × 1024 × u32)
    + state.meta.len() * 128                   // V7: rough per-entry meta estimate
    + 4096 // small safety margin
}

// ── public API ────────────────────────────────────────────────────────────────

/// Encode `state` into `out` (appended, not overwritten).
///
/// The caller should either pass an empty `Vec` or one pre-reserved with
/// [`encode_capacity_hint`].  The function never fails due to buffer size.
pub fn encode_state(state: &KernelState, out: &mut alloc::vec::Vec<u8>) -> Result<()> {
    // Header
    push_bytes(out, MAGIC);
    push_u32(out, SCHEMA_VERSION);
    push_u64(out, state.version.0);

    // V3: lengths (was capacities in V1/V2)
    push_u32(out, state.records.raw_records().len() as u32);
    push_u32(out, state.dim.unwrap_or(0) as u32);
    push_u32(out, state.nodes.raw_nodes().len() as u32);
    push_u32(out, state.edges.raw_edges().len() as u32);

    // V5: arithmetic format tag
    push_u8(out, crate::fxp::format::ACTIVE_FORMAT_ID);

    // Records
    let total_slots = state.records.raw_records().len() as u32;
    push_u32(out, total_slots);

    for slot in state.records.raw_records() {
        if let Some(record) = slot {
            push_u8(out, 1); // present
            push_u32(out, record.id.0);
            push_u8(out, record.flags);
            push_u64(out, record.tag);
            for scalar in record.vector.data.iter() {
                push_i32(out, scalar.0);
            }
            match &record.metadata {
                Some(m) => {
                    push_u32(out, m.len() as u32);
                    push_bytes(out, m);
                }
                None => push_u32(out, 0),
            }
            // V6: namespace linked-list fields
            push_u16(out, record.namespace_id);
            push_u32(out, record.next_in_ns);
            push_u32(out, record.prev_in_ns);
        } else {
            push_u8(out, 0); // absent slot
        }
    }

    // Nodes
    let node_count = state
        .nodes
        .raw_nodes()
        .iter()
        .filter(|s| s.is_some())
        .count() as u32;
    push_u32(out, node_count);

    for slot in state.nodes.raw_nodes().iter() {
        if let Some(node) = slot {
            push_u32(out, node.id.0);
            push_u8(out, node.kind as u8);

            match node.record {
                Some(rid) => {
                    push_u8(out, 1);
                    push_u32(out, rid.0);
                }
                None => push_u8(out, 0),
            }
            match node.first_out_edge {
                Some(eid) => {
                    push_u8(out, 1);
                    push_u32(out, eid.0);
                }
                None => push_u8(out, 0),
            }
            // V4: incoming edge back-pointer
            match node.first_in_edge {
                Some(eid) => {
                    push_u8(out, 1);
                    push_u32(out, eid.0);
                }
                None => push_u8(out, 0),
            }
            // V6: namespace linked-list fields
            push_u16(out, node.namespace_id);
            push_u32(out, node.next_in_ns);
            push_u32(out, node.prev_in_ns);
        }
    }

    // Edges
    let edge_count = state
        .edges
        .raw_edges()
        .iter()
        .filter(|s| s.is_some())
        .count() as u32;
    push_u32(out, edge_count);

    for slot in state.edges.raw_edges().iter() {
        if let Some(edge) = slot {
            push_u32(out, edge.id.0);
            push_u8(out, edge.kind as u8);
            push_u32(out, edge.from.0);
            push_u32(out, edge.to.0);

            match edge.next_out {
                Some(eid) => {
                    push_u8(out, 1);
                    push_u32(out, eid.0);
                }
                None => push_u8(out, 0),
            }
            // V4: incoming list back-pointer
            match edge.next_in {
                Some(eid) => {
                    push_u8(out, 1);
                    push_u32(out, eid.0);
                }
                None => push_u8(out, 0),
            }
        }
    }

    // V6: namespace head arrays (1024 × u32 each)
    use crate::types::id::MAX_NAMESPACES;
    for &head in state.namespace_record_heads.iter().take(MAX_NAMESPACES) {
        push_u32(out, head);
    }
    for &head in state.namespace_node_heads.iter().take(MAX_NAMESPACES) {
        push_u32(out, head);
    }

    // V7: KernelState.meta — SetMeta-committed key/value pairs. BTreeMap
    // iteration is key-ordered, so encoding is deterministic across replicas.
    push_u32(out, state.meta.len() as u32);
    for (key, value) in state.meta.iter() {
        push_u32(out, key.len() as u32);
        push_bytes(out, key.as_bytes());
        push_u32(out, value.len() as u32);
        push_bytes(out, value.as_bytes());
    }

    Ok(())
}
