// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Canonical BLAKE3 Hashing
//!
//! This module defines the CANONICAL hash primitive for Valori:
//! **BLAKE3 = Valori's cryptographic hash standard**
//!
//! # Why BLAKE3?
//! - Cryptographically sound
//! - Deterministic across architectures
//! - Fast (SIMD optimized)
//! - Incremental-friendly
//! - Industry standard for verifiable systems
//!
//! # Usage
//! ALL externally-visible proofs MUST use BLAKE3:
//! - State proofs
//! - Event log proofs
//! - Snapshot proofs
//! - WAL proofs
//! - Replication validation
//!
//! # Guarantee
//! Same state → Same hash (x86 = ARM = RISC-V = WASM)

use crate::state::kernel::KernelState;
use blake3;

/// Compute BLAKE3 hash of kernel state
///
/// This is the CANONICAL state hash for all proof generation.
///
/// # Determinism
/// - Iterates state in fixed order
/// - Uses deterministic serialization
/// - No timestamps, no randomness
/// - Cross-architecture guarantee
///
/// # Commitment contract (G0.2)
///
/// This hash commits to every canonical field for which every valid
/// reconstruction path (live `apply_event_ns`, event-log replay, and
/// snapshot decode/migration) agrees on a single value — not a hand-picked
/// subset, but also not naively "every persisted field" (see the exclusion
/// note below for why that would be wrong). As of domain version 3, this
/// includes per-record/per-node namespace placement (`namespace_id`) and
/// the full bidirectional edge adjacency structure (`first_in_edge`/
/// `next_in`), and the `SetMeta` sidecar. Some of these fields (e.g.
/// `next_out`, and now `next_in`) are technically derivable from the rest
/// of the committed state given the single deterministic edge-construction
/// algorithm in `graph::adjacency` — they are hashed anyway, deliberately,
/// as defense-in-depth: a bug in the list-maintenance code itself (not just
/// a divergence in *what* is stored) must also be visible as a hash
/// mismatch, not silently pass because the hash only re-derives the
/// "should be correct" value analytically.
///
/// **Deliberate exclusion — `next_in_ns`/`prev_in_ns` (the namespace
/// intrusive-list pointers on `Record` and `GraphNode`):** unlike edge
/// adjacency, the namespace list has a SECOND, independent reconstruction
/// path — `KernelState::rebuild_namespace_lists()`, used when migrating a
/// pre-V6 snapshot — which intentionally walks in the OPPOSITE order from
/// live `apply_event_ns` construction (see that function's doc comment).
/// Both orderings are equally valid linked lists over the same namespace
/// membership; hashing the pointers would make hash equality depend on
/// *which* of two correct reconstruction algorithms built the state, not on
/// any real content divergence. `namespace_id` itself IS hashed and is
/// invariant across both reconstruction paths, so namespace-misrouting
/// (the actual correctness property worth committing to) is still covered.
///
/// See `docs/reviews/graph-g0.2-canonical-state-hash-commitment.md` for the
/// full audit that established this contract (superseding the G0.1
/// "confirmed gap" finding, and for the empirical discovery — via
/// `snapshot_version_migration.rs::cross_version_decode_reencode_chain_is_hash_stable`
/// failing — that motivated the exclusion above).
///
/// # Hash Input Structure
/// ```text
/// domain: "valori-state" || domain_version (u8) || format_id (u8)
/// ↓
/// version (u64 LE)
/// ↓
/// For each record (in pool order):
///   id (u32 LE)
///   flags (u8)
///   vector[0..D] (i32 LE each)
///   tag (u64 LE)
///   metadata length (u32 LE, None = u32::MAX) + metadata bytes
///   namespace_id (u16 LE)                        -- G0.2
/// ↓
/// For each node (in pool order):
///   id (u32 LE)
///   kind (u8)
///   record_id (Option<u32> LE, None = u32::MAX)
///   first_out_edge (Option<u32> LE, None = u32::MAX)
///   first_in_edge (Option<u32> LE, None = u32::MAX) -- G0.2
///   namespace_id (u16 LE)                           -- G0.2
/// ↓
/// For each edge (in pool order):
///   id (u32 LE)
///   kind (u8)
///   from (u32 LE)
///   to (u32 LE)
///   next_out (Option<u32> LE, None = u32::MAX)
///   next_in (Option<u32> LE, None = u32::MAX)     -- G0.2
/// ↓
/// meta entry count (u32 LE)                        -- G0.2
/// For each (key, value) in state.meta (BTreeMap — key-ordered, deterministic):
///   key length (u32 LE) + key bytes
///   value length (u32 LE) + value bytes
/// ```
///
/// Returns: [u8; 32] - BLAKE3 hash
/// Version of the hash-input schema itself. Bumped whenever the structure
/// above changes (v2 = added domain separation + tag/metadata coverage;
/// v3 = G0.2 — added namespace placement (`namespace_id` only), reverse
/// edge adjacency (`first_in_edge`/`next_in`), and the meta sidecar).
/// A state hashed under one domain version can never collide with the
/// same bytes hashed under another — hash changes are versioned, visible
/// events, not silent drift.
pub const STATE_HASH_DOMAIN_VERSION: u8 = 3;

pub fn hash_state_blake3(state: &KernelState) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();

    // Domain separation: a Q8.8 state must never hash-collide with a
    // Q16.16 state, and schema changes must be distinguishable.
    hasher.update(b"valori-state");
    hasher.update(&[
        STATE_HASH_DOMAIN_VERSION,
        crate::fxp::format::ACTIVE_FORMAT_ID,
    ]);

    // Version
    hasher.update(&state.version.0.to_le_bytes());

    // Records (iteration order is deterministic by pool implementation)
    for record in state.records.iter() {
        hasher.update(&record.id.0.to_le_bytes());
        hasher.update(&[record.flags]);
        for scalar in record.vector.data.iter() {
            hasher.update(&scalar.0.to_le_bytes());
        }
        // Tag and metadata are state: tags drive filtered search and
        // metadata carries per-record proofs. Leaving them out of the
        // hash would let replicas diverge invisibly (length prefix keeps
        // None / Some(empty) / adjacent-bytes cases unambiguous).
        hasher.update(&record.tag.to_le_bytes());
        match &record.metadata {
            Some(bytes) => {
                hasher.update(&(bytes.len() as u32).to_le_bytes());
                hasher.update(bytes);
            }
            None => {
                hasher.update(&u32::MAX.to_le_bytes());
            }
        }
        // G0.2: namespace placement is canonical (event-sourced, persisted)
        // but was not committed under domain version 2 — a namespace
        // misrouting bug would have been invisible to cross-replica hash
        // comparison. Deliberately NOT hashing next_in_ns/prev_in_ns here:
        // unlike the edge adjacency pointers below, the namespace list has
        // a SECOND, independent reconstruction path — `rebuild_namespace_
        // lists()` (used when migrating pre-V6 snapshots) intentionally
        // walks in the OPPOSITE order from live `apply_event_ns` construction
        // to produce an ascending-id list — see that function's own comment.
        // Both orderings are equally valid linked lists over the same
        // content; hashing the pointers would make hash equality depend on
        // which of two correct reconstruction algorithms built the state,
        // not on any real content divergence. Confirmed empirically: doing
        // so broke `snapshot_version_migration.rs::
        // cross_version_decode_reencode_chain_is_hash_stable` for every
        // schema_ver < 6, which is exactly this case, not a bug in either
        // reconstruction path.
        hasher.update(&record.namespace_id.to_le_bytes());
    }

    // Nodes (in pool order - deterministic)
    for slot in state.nodes.raw_nodes().iter() {
        if let Some(node) = slot {
            hasher.update(&node.id.0.to_le_bytes());
            hasher.update(&[node.kind as u8]);

            // Record ID (None = sentinel u32::MAX)
            match node.record {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }

            // First out edge (None = sentinel u32::MAX)
            match node.first_out_edge {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }

            // G0.2: first_in_edge (reverse adjacency) — same defense-in-depth
            // rationale as first_out_edge above: a bug in `add_edge`/
            // `_delete_edge`'s incoming-list surgery must be visible as a
            // hash mismatch, not silently pass because the hash only
            // re-derives the "should be correct" value from the edge set.
            match node.first_in_edge {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }
            // namespace_id only — see the identical rationale on the record
            // loop above for why next_in_ns/prev_in_ns are deliberately
            // excluded (the pre-V6 `rebuild_namespace_lists()` migration
            // path produces a different, equally-valid pointer ordering
            // than live construction for the same namespace membership).
            hasher.update(&node.namespace_id.to_le_bytes());
        }
    }

    // Edges (in pool order - deterministic)
    for slot in state.edges.raw_edges().iter() {
        if let Some(edge) = slot {
            hasher.update(&edge.id.0.to_le_bytes());
            hasher.update(&[edge.kind as u8]);
            hasher.update(&edge.from.0.to_le_bytes());
            hasher.update(&edge.to.0.to_le_bytes());

            // Next out edge (None = sentinel u32::MAX)
            match edge.next_out {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }
            // G0.2: next_in (reverse adjacency link) — see node.first_in_edge above.
            match edge.next_in {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }
        }
    }

    // G0.2: the SetMeta sidecar (`state.meta`) is canonical — event-sourced
    // via `KernelEvent::SetMeta`, persisted in the V7 snapshot section — but
    // was entirely absent from the hash under domain version 2. BTreeMap
    // iteration is key-ordered, so this is deterministic across replicas
    // without any extra sorting step (same reasoning `encode_state`'s own
    // V7 meta section comment already relies on).
    hasher.update(&(state.meta.len() as u32).to_le_bytes());
    for (key, value) in state.meta.iter() {
        hasher.update(&(key.len() as u32).to_le_bytes());
        hasher.update(key.as_bytes());
        hasher.update(&(value.len() as u32).to_le_bytes());
        hasher.update(value.as_bytes());
    }

    *hasher.finalize().as_bytes()
}

/// Compute BLAKE3 hash of a byte slice
///
/// Generic helper for hashing snapshots, event logs, WAL, etc.
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::kernel::KernelState;

    #[test]
    fn test_blake3_determinism() {
        let state1 = KernelState::new();
        let state2 = KernelState::new();

        let hash1 = hash_state_blake3(&state1);
        let hash2 = hash_state_blake3(&state2);

        assert_eq!(hash1, hash2, "Empty states must hash identically");
    }

    #[test]
    fn test_blake3_output_length() {
        let state = KernelState::new();
        let hash = hash_state_blake3(&state);

        assert_eq!(hash.len(), 32, "BLAKE3 must produce 32 bytes");
    }

    #[test]
    fn test_blake3_bytes_hash() {
        let data = b"test data";
        let hash1 = hash_bytes(data);
        let hash2 = hash_bytes(data);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 32);
    }
}
