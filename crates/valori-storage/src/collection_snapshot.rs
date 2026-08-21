// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Per-collection snapshot format — the structural fix for the
//! mixed-dimension gap disclosed in the collection-scoped-vector-config
//! phase (`snapshot_roundtrip_known_limitation_mixed_dimensions`).
//!
//! # Why a separate format instead of extending `valori_kernel::snapshot`
//!
//! The kernel's whole-process V8 snapshot format stores exactly ONE vector
//! byte-width (the header `dim`) for every record in the file, because it
//! was designed around one project-wide dimension. That constraint is
//! structural to the record loop's decode order (dim must be known before
//! any record's vector bytes can be read) and fixing it in place would mean
//! reordering the V8 layout — a real follow-up, not scope for this phase
//! (see that phase's report).
//!
//! A **per-collection** snapshot sidesteps the problem entirely: one file
//! ever describes exactly one collection, so it only ever needs to know
//! ONE dimension — its own. `CollectionSnapshot{project_id, collection_id, generation}`
//! artifacts for dimensions 384, 768, and 1536 can coexist in the same
//! project because they are never in the same file.
//!
//! This format is intentionally minimal — Q16.16 scalars, no index bytes,
//! no graph nodes/edges (a collection's graph nodes are out of scope for
//! this phase's storage foundation; the existing whole-process kernel
//! snapshot remains the source of truth for graph state until a
//! collection-scoped graph model exists). It exists to prove and exercise
//! the storage/recovery model this phase establishes, not to replace the
//! kernel snapshot format wholesale.
//!
//! # RecordId restore design (Phase 2.2 fix)
//!
//! **Audited invariant** (`crates/valori-kernel/src/storage/pool.rs`,
//! `RecordPool::insert`/`KernelState::apply_event_ns`): `RecordId` is a
//! single, global, monotonically-increasing slab index shared by every
//! namespace in one `KernelState` — never per-collection, never reused.
//! `InsertRecord` requires the given id to equal `records.next_id()`
//! *exactly*; a hard delete leaves the slot `None` (a permanent hole) but
//! `next_id()` never rewinds past it. `RecordPool::iter()` (and therefore
//! `hash_state_blake3`) skips `None`/soft-deleted slots entirely — holes
//! are invisible to search and to the state hash, but they are NOT
//! invisible to `next_id()`.
//!
//! This is why a naive per-collection restore breaks: if collection A's
//! live records are `[0, 2]` (id 1 was hard-deleted, originally either A's
//! own record or another collection's), inserting id 0 then id 2 violates
//! `next_id() == id` — there is no way to "skip" a hole through the real
//! `InsertRecord` path.
//!
//! **Chosen design (Option A from the Phase 2.2 spec's menu — keep
//! `RecordId` globally unique, persist enough allocator state per snapshot
//! to reconstruct it)**, not Option B/C (a `CollectionId`-scoped or
//! namespace-local identity): changing `RecordId`'s meaning would ripple
//! into the WAL event format, the state hash, request dedup, and the
//! cluster state machine — all called out explicitly in the spec as things
//! not to disturb without proof it's safe, and no such proof was needed
//! once Option A closes the gap cleanly.
//!
//! Each snapshot's `pool_ceiling` records `state.next_record_id()` at
//! snapshot time — the global id space's high-water mark, a
//! whole-`KernelState` fact that every collection snapshotted at the same
//! coordinated point naturally agrees on (see Phase 2's "coordinated
//! snapshot" reasoning). `restore_project_into` reconstructs every hole up
//! to the highest `pool_ceiling` among the collections being restored by
//! inserting a throwaway zero-vector record and immediately hard-deleting
//! it — through the REAL `InsertRecord`/`DeleteRecord` event path, no
//! private bypass, satisfying the spec's explicit "do not create a private
//! bypass that skips validation." The hole never appears in the final
//! live state (deleted immediately) and never touches the state hash
//! (`iter()` skips it) — its only effect is correctly advancing
//! `next_id()` past the gap, exactly as the original process's hard delete
//! did.

use serde::{Deserialize, Serialize};
use valori_core::{EdgeKind, NamespaceId, NodeKind};
use valori_domain::Metric;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

use crate::collection_manifest::Lsn;
use crate::provider::{StorageError, StorageKey};

/// V3 (Phase 2.4): adds Collection-owned graph state (`nodes`, `edges`,
/// `node_pool_ceiling`, `edge_pool_ceiling`). V2 snapshots remain decode-compatible
/// and populate empty graph state.
pub const COLLECTION_SNAPSHOT_SCHEMA_VERSION: u32 = 3;
const MAGIC: &[u8; 4] = b"CSN1";

/// One record as captured by a collection snapshot. Deliberately a
/// standalone type, not `valori_kernel::storage::record::Record` — that
/// type carries intrusive linked-list pointers (`next_in_ns`/`prev_in_ns`)
/// that are meaningless outside a live `RecordPool`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionSnapshotRecord {
    pub id: u32,
    pub vector: Vec<i32>,
    pub metadata: Option<Vec<u8>>,
    pub tag: u64,
    pub flags: u8,
}

/// One graph node as captured by a collection snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionSnapshotNode {
    pub id: u32,
    pub kind: u8,
    pub record: Option<u32>,
}

/// One graph edge as captured by a collection snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionSnapshotEdge {
    pub id: u32,
    pub kind: u8,
    pub from: u32,
    pub to: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionSnapshotMeta {
    pub schema_version: u32,
    pub collection_id: NamespaceId,
    pub generation: u32,
    pub base_lsn: Lsn,
    pub dimension: u32,
    pub metric: Metric,
    pub record_count: u32,
    /// `state.next_record_id()` — the global `RecordId` allocator
    /// high-water mark at snapshot time, across the WHOLE `KernelState`.
    pub pool_ceiling: u32,
    pub node_count: u32,
    pub edge_count: u32,
    pub node_pool_ceiling: u32,
    pub edge_pool_ceiling: u32,
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn read_u32(buf: &[u8], off: &mut usize) -> Option<u32> {
    let b: [u8; 4] = buf.get(*off..*off + 4)?.try_into().ok()?;
    *off += 4;
    Some(u32::from_le_bytes(b))
}
fn read_u64(buf: &[u8], off: &mut usize) -> Option<u64> {
    let b: [u8; 8] = buf.get(*off..*off + 8)?.try_into().ok()?;
    *off += 8;
    Some(u64::from_le_bytes(b))
}
fn read_i32(buf: &[u8], off: &mut usize) -> Option<i32> {
    let b: [u8; 4] = buf.get(*off..*off + 4)?.try_into().ok()?;
    *off += 4;
    Some(i32::from_le_bytes(b))
}

/// Encode one collection's meta + records + nodes + edges into a self-contained artifact.
pub fn encode(
    meta: &CollectionSnapshotMeta,
    records: &[CollectionSnapshotRecord],
    nodes: &[CollectionSnapshotNode],
    edges: &[CollectionSnapshotEdge],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        128 + records.len() * (28 + meta.dimension as usize * 4)
            + nodes.len() * 10
            + edges.len() * 14,
    );
    out.extend_from_slice(MAGIC);
    push_u32(&mut out, meta.schema_version);
    push_u32(&mut out, meta.collection_id.0 as u32);
    push_u32(&mut out, meta.generation);
    push_u64(&mut out, meta.base_lsn.0);
    push_u32(&mut out, meta.dimension);
    out.push(meta.metric.as_u8());
    push_u32(&mut out, meta.pool_ceiling);
    push_u32(&mut out, records.len() as u32);

    // V3 fields
    push_u32(&mut out, meta.node_pool_ceiling);
    push_u32(&mut out, meta.edge_pool_ceiling);
    push_u32(&mut out, nodes.len() as u32);
    push_u32(&mut out, edges.len() as u32);

    for r in records {
        push_u32(&mut out, r.id);
        out.push(r.flags);
        push_u64(&mut out, r.tag);
        debug_assert_eq!(r.vector.len(), meta.dimension as usize);
        for scalar in &r.vector {
            push_i32(&mut out, *scalar);
        }
        match &r.metadata {
            Some(m) => {
                push_u32(&mut out, m.len() as u32);
                out.extend_from_slice(m);
            }
            None => push_u32(&mut out, 0),
        }
    }

    for n in nodes {
        push_u32(&mut out, n.id);
        out.push(n.kind);
        match n.record {
            Some(rid) => {
                out.push(1);
                push_u32(&mut out, rid);
            }
            None => out.push(0),
        }
    }

    for e in edges {
        push_u32(&mut out, e.id);
        out.push(e.kind);
        push_u32(&mut out, e.from);
        push_u32(&mut out, e.to);
    }

    out
}

/// Decode a collection-snapshot artifact. Supports V2 and V3 formats cleanly.
pub fn decode(
    key: &StorageKey,
    buf: &[u8],
) -> Result<
    (
        CollectionSnapshotMeta,
        Vec<CollectionSnapshotRecord>,
        Vec<CollectionSnapshotNode>,
        Vec<CollectionSnapshotEdge>,
    ),
    StorageError,
> {
    let invalid = |reason: String| StorageError::InvalidManifest {
        key: key.clone(),
        reason,
    };

    if buf.len() < 4 || &buf[0..4] != MAGIC {
        return Err(invalid("bad magic".to_string()));
    }
    let mut off = 4usize;
    let schema_version =
        read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
    if schema_version > COLLECTION_SNAPSHOT_SCHEMA_VERSION || schema_version < 2 {
        return Err(StorageError::UnsupportedVersion {
            key: key.clone(),
            version: schema_version,
        });
    }
    let collection_id =
        read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
    if collection_id > u16::MAX as u32 {
        return Err(invalid("collection_id out of range".into()));
    }
    let generation = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
    let base_lsn = read_u64(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
    let dimension = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
    if dimension == 0 || dimension as usize > valori_kernel::config::MAX_DIM {
        return Err(invalid(format!("dimension {dimension} out of range")));
    }
    let metric_byte = *buf
        .get(off)
        .ok_or_else(|| invalid("truncated header".into()))?;
    off += 1;
    let metric = Metric::from_u8(metric_byte)
        .ok_or_else(|| invalid(format!("unknown metric tag {metric_byte}")))?;
    let pool_ceiling = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
    let record_count = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;

    let (node_pool_ceiling, edge_pool_ceiling, node_count, edge_count) = if schema_version >= 3 {
        let npc = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
        let epc = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
        let nc = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
        let ec = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated header".into()))?;
        (npc, epc, nc, ec)
    } else {
        (0, 0, 0, 0)
    };

    let mut records = Vec::with_capacity(record_count as usize);
    for _ in 0..record_count {
        let id = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated record".into()))?;
        let flags = *buf
            .get(off)
            .ok_or_else(|| invalid("truncated record".into()))?;
        off += 1;
        let tag = read_u64(buf, &mut off).ok_or_else(|| invalid("truncated record".into()))?;
        let mut vector = Vec::with_capacity(dimension as usize);
        for _ in 0..dimension {
            vector.push(read_i32(buf, &mut off).ok_or_else(|| invalid("truncated vector".into()))?);
        }
        let meta_len =
            read_u32(buf, &mut off).ok_or_else(|| invalid("truncated record".into()))? as usize;
        let metadata = if meta_len > 0 {
            let bytes = buf
                .get(off..off + meta_len)
                .ok_or_else(|| invalid("truncated metadata".into()))?
                .to_vec();
            off += meta_len;
            Some(bytes)
        } else {
            None
        };
        records.push(CollectionSnapshotRecord {
            id,
            vector,
            metadata,
            tag,
            flags,
        });
    }

    let mut nodes = Vec::with_capacity(node_count as usize);
    for _ in 0..node_count {
        let id = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated node".into()))?;
        let kind = *buf
            .get(off)
            .ok_or_else(|| invalid("truncated node".into()))?;
        off += 1;
        let has_rec = *buf
            .get(off)
            .ok_or_else(|| invalid("truncated node".into()))?;
        off += 1;
        let record = if has_rec == 1 {
            let rid = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated node".into()))?;
            Some(rid)
        } else if has_rec == 0 {
            None
        } else {
            return Err(invalid("invalid node record flag".into()));
        };
        nodes.push(CollectionSnapshotNode { id, kind, record });
    }

    let mut edges = Vec::with_capacity(edge_count as usize);
    for _ in 0..edge_count {
        let id = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated edge".into()))?;
        let kind = *buf
            .get(off)
            .ok_or_else(|| invalid("truncated edge".into()))?;
        off += 1;
        let from = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated edge".into()))?;
        let to = read_u32(buf, &mut off).ok_or_else(|| invalid("truncated edge".into()))?;
        edges.push(CollectionSnapshotEdge { id, kind, from, to });
    }

    Ok((
        CollectionSnapshotMeta {
            schema_version,
            collection_id: NamespaceId(collection_id as u16),
            generation,
            base_lsn: Lsn(base_lsn),
            dimension,
            metric,
            record_count,
            pool_ceiling,
            node_count,
            edge_count,
            node_pool_ceiling,
            edge_pool_ceiling,
        },
        records,
        nodes,
        edges,
    ))
}

/// Extract exactly one namespace's live, searchable records, nodes, and edges from a
/// `KernelState`, plus its effective dimension.
pub fn extract_from_kernel_state(
    state: &KernelState,
    collection_id: NamespaceId,
    generation: u32,
    base_lsn: Lsn,
    metric: Metric,
) -> Option<(
    CollectionSnapshotMeta,
    Vec<CollectionSnapshotRecord>,
    Vec<CollectionSnapshotNode>,
    Vec<CollectionSnapshotEdge>,
)> {
    let dim = state.namespace_dim(collection_id.0)? as u32;
    let records: Vec<CollectionSnapshotRecord> = state
        .iter_records_in_ns(collection_id.0)
        .filter(|r| r.is_searchable())
        .map(|r| CollectionSnapshotRecord {
            id: r.id.0,
            vector: r.vector.data.iter().map(|s| s.0).collect(),
            metadata: r.metadata.clone(),
            tag: r.tag,
            flags: r.flags,
        })
        .collect();

    let nodes: Vec<CollectionSnapshotNode> = state
        .iter_nodes_in_ns(collection_id.0)
        .map(|n| CollectionSnapshotNode {
            id: n.id.0,
            kind: n.kind as u8,
            record: n.record.map(|r| r.0),
        })
        .collect();

    let edges: Vec<CollectionSnapshotEdge> = state
        .iter_edges_in_ns(collection_id.0)
        .map(|e| CollectionSnapshotEdge {
            id: e.id.0,
            kind: e.kind as u8,
            from: e.from.0,
            to: e.to.0,
        })
        .collect();

    let meta = CollectionSnapshotMeta {
        schema_version: COLLECTION_SNAPSHOT_SCHEMA_VERSION,
        collection_id,
        generation,
        base_lsn,
        dimension: dim,
        metric,
        record_count: records.len() as u32,
        pool_ceiling: state.next_record_id().0,
        node_count: nodes.len() as u32,
        edge_count: edges.len() as u32,
        node_pool_ceiling: state.next_node_id().0,
        edge_pool_ceiling: state.next_edge_id().0,
    };
    Some((meta, records, nodes, edges))
}

/// Restore one collection's snapshot into `state`.
pub fn restore_collection_into(
    state: &mut KernelState,
    meta: &CollectionSnapshotMeta,
    records: &[CollectionSnapshotRecord],
    nodes: &[CollectionSnapshotNode],
    edges: &[CollectionSnapshotEdge],
) -> Result<(), valori_kernel::error::KernelError> {
    restore_project_into(state, &[(meta, records, nodes, edges)])
}

/// Restore MULTIPLE collections' snapshots into one fresh `state`, in the
/// correct global insertion order — reconstructing records, graph nodes, and graph
/// edges, as well as advancing allocators over any holes.
pub fn restore_project_into(
    state: &mut KernelState,
    collections: &[(
        &CollectionSnapshotMeta,
        &[CollectionSnapshotRecord],
        &[CollectionSnapshotNode],
        &[CollectionSnapshotEdge],
    )],
) -> Result<(), valori_kernel::error::KernelError> {
    let hole_namespace = collections
        .first()
        .map(|(meta, _, _, _)| meta.collection_id);

    // ── 1. Restore records ───────────────────────────────────────────────────
    let mut merged_records: Vec<(u32, NamespaceId, &CollectionSnapshotRecord)> = collections
        .iter()
        .flat_map(|(meta, records, _, _)| {
            let ns = meta.collection_id;
            records.iter().map(move |r| (r.id, ns, r))
        })
        .collect();
    merged_records.sort_by_key(|(id, _, _)| *id);

    let record_ceiling = collections
        .iter()
        .map(|(meta, _, _, _)| meta.pool_ceiling)
        .max()
        .unwrap_or(0);

    for (id, ns, r) in merged_records {
        fill_record_holes_up_to(state, id, hole_namespace)?;
        let vector = FxpVector {
            data: r.vector.iter().map(|&v| FxpScalar(v)).collect(),
        };
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::InsertRecord {
                id: RecordId(id),
                vector,
                metadata: r.metadata.clone(),
                tag: r.tag,
            },
            ns.0,
        )?;
    }
    fill_record_holes_up_to(state, record_ceiling, hole_namespace)?;

    // ── 2. Restore graph nodes ───────────────────────────────────────────────
    let mut merged_nodes: Vec<(u32, NamespaceId, &CollectionSnapshotNode)> = collections
        .iter()
        .flat_map(|(meta, _, nodes, _)| {
            let ns = meta.collection_id;
            nodes.iter().map(move |n| (n.id, ns, n))
        })
        .collect();
    merged_nodes.sort_by_key(|(id, _, _)| *id);

    let node_ceiling = collections
        .iter()
        .map(|(meta, _, _, _)| meta.node_pool_ceiling)
        .max()
        .unwrap_or(0);

    for (id, ns, n) in merged_nodes {
        fill_node_holes_up_to(state, id, hole_namespace)?;
        let kind =
            NodeKind::from_u8(n.kind).ok_or(valori_kernel::error::KernelError::InvalidOperation)?;
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::CreateNode {
                id: NodeId(id),
                kind,
                record: n.record.map(RecordId),
            },
            ns.0,
        )?;
    }
    fill_node_holes_up_to(state, node_ceiling, hole_namespace)?;

    // ── 3. Restore graph edges ───────────────────────────────────────────────
    let mut merged_edges: Vec<(u32, NamespaceId, &CollectionSnapshotEdge)> = collections
        .iter()
        .flat_map(|(meta, _, _, edges)| {
            let ns = meta.collection_id;
            edges.iter().map(move |e| (e.id, ns, e))
        })
        .collect();
    merged_edges.sort_by_key(|(id, _, _)| *id);

    let edge_ceiling = collections
        .iter()
        .map(|(meta, _, _, _)| meta.edge_pool_ceiling)
        .max()
        .unwrap_or(0);

    for (id, ns, e) in merged_edges {
        fill_edge_holes_up_to(state, id, hole_namespace)?;
        let kind =
            EdgeKind::from_u8(e.kind).ok_or(valori_kernel::error::KernelError::InvalidOperation)?;
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::CreateEdge {
                id: EdgeId(id),
                from: NodeId(e.from),
                to: NodeId(e.to),
                kind,
            },
            ns.0,
        )?;
    }
    fill_edge_holes_up_to(state, edge_ceiling, hole_namespace)?;

    Ok(())
}

/// Advance `state`'s `RecordId` allocator up to `target_id` using throwaway insert+delete pairs.
fn fill_record_holes_up_to(
    state: &mut KernelState,
    target_id: u32,
    hole_namespace: Option<NamespaceId>,
) -> Result<(), valori_kernel::error::KernelError> {
    let Some(hole_ns) = hole_namespace else {
        return Ok(());
    };
    while state.next_record_id().0 < target_id {
        let dim = state.namespace_dim(hole_ns.0).unwrap_or(1).max(1);
        let id = state.next_record_id();
        let vector = FxpVector {
            data: vec![FxpScalar(0); dim],
        };
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::InsertRecord {
                id,
                vector,
                metadata: None,
                tag: 0,
            },
            hole_ns.0,
        )?;
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::DeleteRecord { id },
            hole_ns.0,
        )?;
    }
    Ok(())
}

/// Advance `state`'s `NodeId` allocator up to `target_id` using throwaway CreateNode+DeleteNode pairs.
fn fill_node_holes_up_to(
    state: &mut KernelState,
    target_id: u32,
    hole_namespace: Option<NamespaceId>,
) -> Result<(), valori_kernel::error::KernelError> {
    let Some(hole_ns) = hole_namespace else {
        return Ok(());
    };
    while state.next_node_id().0 < target_id {
        let id = state.next_node_id();
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::CreateNode {
                id,
                kind: NodeKind::Concept,
                record: None,
            },
            hole_ns.0,
        )?;
        state.apply_event_ns(
            &valori_kernel::event::KernelEvent::DeleteNode { id },
            hole_ns.0,
        )?;
    }
    Ok(())
}

/// Advance `state`'s `EdgeId` allocator up to `target_id` using throwaway CreateEdge+DeleteEdge pairs.
fn fill_edge_holes_up_to(
    state: &mut KernelState,
    target_id: u32,
    hole_namespace: Option<NamespaceId>,
) -> Result<(), valori_kernel::error::KernelError> {
    let Some(hole_ns) = hole_namespace else {
        return Ok(());
    };
    while state.next_edge_id().0 < target_id {
        let id = state.next_edge_id();
        let live_node = state.iter_nodes_in_ns(hole_ns.0).next().map(|n| n.id);
        if let Some(node_id) = live_node {
            state.apply_event_ns(
                &valori_kernel::event::KernelEvent::CreateEdge {
                    id,
                    from: node_id,
                    to: node_id,
                    kind: EdgeKind::Relation,
                },
                hole_ns.0,
            )?;
            state.apply_event_ns(
                &valori_kernel::event::KernelEvent::DeleteEdge { id },
                hole_ns.0,
            )?;
        } else {
            // No node currently exists in hole_ns: create a temporary node, attach throwaway edge, delete both
            let dummy_node_id = state.next_node_id();
            state.apply_event_ns(
                &valori_kernel::event::KernelEvent::CreateNode {
                    id: dummy_node_id,
                    kind: NodeKind::Concept,
                    record: None,
                },
                hole_ns.0,
            )?;
            state.apply_event_ns(
                &valori_kernel::event::KernelEvent::CreateEdge {
                    id,
                    from: dummy_node_id,
                    to: dummy_node_id,
                    kind: EdgeKind::Relation,
                },
                hole_ns.0,
            )?;
            state.apply_event_ns(
                &valori_kernel::event::KernelEvent::DeleteEdge { id },
                hole_ns.0,
            )?;
            state.apply_event_ns(
                &valori_kernel::event::KernelEvent::DeleteNode { id: dummy_node_id },
                hole_ns.0,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::event::KernelEvent;

    /// Inserts `count` records into namespace `ns`, using the kernel's own
    /// next-id sequence — required because `RecordId`s are shared across
    /// every namespace in one `KernelState` (Phase 1's audit finding), so a
    /// caller seeding multiple namespaces cannot restart ids at 0 each time.
    fn seed(state: &mut KernelState, ns: u16, dim: usize, count: u32) {
        state
            .configure_namespace(ns, dim as u32, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        for i in 0..count {
            let id = state.next_record_id();
            let data = (0..dim)
                .map(|d| FxpScalar((i * 10 + d as u32) as i32))
                .collect();
            state
                .apply_event_ns(
                    &KernelEvent::InsertRecord {
                        id,
                        vector: FxpVector { data },
                        metadata: Some(vec![i as u8]),
                        tag: i as u64,
                    },
                    ns,
                )
                .unwrap();
        }
    }

    #[test]
    fn encode_decode_roundtrip_preserves_records_and_graph() {
        let mut state = KernelState::new();
        seed(&mut state, 1, 4, 3);
        let n0 = state.next_node_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: n0,
                    kind: NodeKind::Document,
                    record: Some(RecordId(0)),
                },
                1,
            )
            .unwrap();
        let n1 = state.next_node_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: n1,
                    kind: NodeKind::Concept,
                    record: None,
                },
                1,
            )
            .unwrap();
        let e0 = state.next_edge_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateEdge {
                    id: e0,
                    from: n0,
                    to: n1,
                    kind: EdgeKind::RefersTo,
                },
                1,
            )
            .unwrap();

        let (meta, records, nodes, edges) =
            extract_from_kernel_state(&state, NamespaceId(1), 1, Lsn(100), Metric::SquaredL2)
                .unwrap();
        assert_eq!(meta.dimension, 4);
        assert_eq!(records.len(), 3);
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);

        let bytes = encode(&meta, &records, &nodes, &edges);
        let key = StorageKey::CollectionSnapshot {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(1),
            generation: 1,
        };
        let (decoded_meta, decoded_records, decoded_nodes, decoded_edges) =
            decode(&key, &bytes).unwrap();
        assert_eq!(decoded_meta, meta);
        assert_eq!(decoded_records, records);
        assert_eq!(decoded_nodes, nodes);
        assert_eq!(decoded_edges, edges);
    }

    #[test]
    fn restore_into_fresh_state_reproduces_original() {
        let mut original = KernelState::new();
        seed(&mut original, 2, 8, 5);
        let n0 = original.next_node_id();
        original
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: n0,
                    kind: NodeKind::Document,
                    record: Some(RecordId(0)),
                },
                2,
            )
            .unwrap();
        let n1 = original.next_node_id();
        original
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: n1,
                    kind: NodeKind::Concept,
                    record: None,
                },
                2,
            )
            .unwrap();
        let e0 = original.next_edge_id();
        original
            .apply_event_ns(
                &KernelEvent::CreateEdge {
                    id: e0,
                    from: n0,
                    to: n1,
                    kind: EdgeKind::Mentions,
                },
                2,
            )
            .unwrap();

        let (meta, records, nodes, edges) =
            extract_from_kernel_state(&original, NamespaceId(2), 1, Lsn(50), Metric::SquaredL2)
                .unwrap();

        let mut restored = KernelState::new();
        restored
            .configure_namespace(
                2,
                meta.dimension,
                valori_kernel::index::Metric::SquaredL2,
                0,
            )
            .unwrap();
        restore_collection_into(&mut restored, &meta, &records, &nodes, &edges).unwrap();

        assert_eq!(restored.record_count(), original.record_count());
        for i in 0..5u32 {
            let orig = original.get_record(RecordId(i)).unwrap();
            let rest = restored.get_record(RecordId(i)).unwrap();
            assert_eq!(orig.vector.data, rest.vector.data);
            assert_eq!(orig.metadata, rest.metadata);
        }

        assert_eq!(restored.node_count(), 2);
        assert_eq!(restored.edge_count(), 1);
        let r_n0 = restored.get_node(n0).unwrap();
        assert_eq!(r_n0.kind, NodeKind::Document);
        assert_eq!(r_n0.record, Some(RecordId(0)));
        assert_eq!(r_n0.namespace_id, 2);

        let r_e0 = restored.get_edge(e0).unwrap();
        assert_eq!(r_e0.from, n0);
        assert_eq!(r_e0.to, n1);
        assert_eq!(r_e0.kind, EdgeKind::Mentions);
    }

    /// The core mixed-dimension proof (§20 of the phase spec): three
    /// collections at 384/768/1536 each snapshot and decode independently,
    /// each carrying its OWN dimension — never one project-wide value.
    #[test]
    fn mixed_dimensions_persist_independently() {
        let mut state = KernelState::new();
        seed(&mut state, 1, 384, 2);
        seed(&mut state, 2, 768, 2);
        seed(&mut state, 3, 1536, 2);

        for (ns, dim) in [(1u16, 384u32), (2, 768), (3, 1536)] {
            let (meta, records, nodes, edges) =
                extract_from_kernel_state(&state, NamespaceId(ns), 1, Lsn(0), Metric::SquaredL2)
                    .unwrap();
            assert_eq!(
                meta.dimension, dim,
                "namespace {ns} must report its own dimension"
            );
            let bytes = encode(&meta, &records, &nodes, &edges);
            let key = StorageKey::CollectionSnapshot {
                project_id: valori_domain::ProjectId::new(),
                collection_id: NamespaceId(ns),
                generation: 1,
            };
            let (decoded_meta, decoded_records, _, _) = decode(&key, &bytes).unwrap();
            assert_eq!(decoded_meta.dimension, dim);
            for r in &decoded_records {
                assert_eq!(r.vector.len(), dim as usize);
            }
        }
    }

    #[test]
    fn decode_rejects_truncated_bytes() {
        let mut state = KernelState::new();
        seed(&mut state, 1, 4, 1);
        let (meta, records, nodes, edges) =
            extract_from_kernel_state(&state, NamespaceId(1), 1, Lsn(0), Metric::SquaredL2)
                .unwrap();
        let bytes = encode(&meta, &records, &nodes, &edges);
        let key = StorageKey::CollectionSnapshot {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(1),
            generation: 1,
        };
        let truncated = &bytes[..bytes.len() - 3];
        assert!(decode(&key, truncated).is_err());
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let key = StorageKey::CollectionSnapshot {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(1),
            generation: 1,
        };
        assert!(decode(&key, b"NOTMAGIC_garbage").is_err());
    }

    /// The mandatory RecordId regression test (Phase 2.2 spec §28.F): a
    /// hard-deleted, EARLIER `RecordId` in collection A must not break
    /// restoring collection B's later, higher ids.
    #[test]
    fn hard_deleted_record_in_one_collection_does_not_break_restoring_another() {
        let mut state = KernelState::new();
        state
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        state
            .configure_namespace(2, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();

        // A: id 0, id 1 (later hard-deleted), id 2.
        let mut ids_a = Vec::new();
        for i in 0..3u32 {
            let id = state.next_record_id();
            ids_a.push(id);
            state
                .apply_event_ns(
                    &KernelEvent::InsertRecord {
                        id,
                        vector: FxpVector {
                            data: vec![FxpScalar(i as i32); 4],
                        },
                        metadata: None,
                        tag: 0,
                    },
                    1,
                )
                .unwrap();
        }
        // Hard-delete A's middle record — creates the global hole.
        state
            .apply_event_ns(&KernelEvent::DeleteRecord { id: ids_a[1] }, 1)
            .unwrap();

        // B: two records with ids strictly AFTER the hole.
        for i in 0..2u32 {
            let id = state.next_record_id();
            state
                .apply_event_ns(
                    &KernelEvent::InsertRecord {
                        id,
                        vector: FxpVector {
                            data: vec![FxpScalar(100 + i as i32); 4],
                        },
                        metadata: None,
                        tag: 0,
                    },
                    2,
                )
                .unwrap();
        }

        assert_eq!(state.record_count(), 4); // A: 2 live (0, 2); B: 2 live

        let (meta_a, records_a, nodes_a, edges_a) =
            extract_from_kernel_state(&state, NamespaceId(1), 1, Lsn(0), Metric::SquaredL2)
                .unwrap();
        let (meta_b, records_b, nodes_b, edges_b) =
            extract_from_kernel_state(&state, NamespaceId(2), 1, Lsn(0), Metric::SquaredL2)
                .unwrap();
        assert_eq!(
            records_a.len(),
            2,
            "A's hard-deleted record must be absent from its own snapshot"
        );
        assert_eq!(records_b.len(), 2);
        assert_eq!(
            meta_a.pool_ceiling, meta_b.pool_ceiling,
            "coordinated snapshot: same global ceiling"
        );

        let mut restored = KernelState::new();
        restored
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        restored
            .configure_namespace(2, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();

        restore_project_into(
            &mut restored,
            &[
                (
                    &meta_a,
                    records_a.as_slice(),
                    nodes_a.as_slice(),
                    edges_a.as_slice(),
                ),
                (
                    &meta_b,
                    records_b.as_slice(),
                    nodes_b.as_slice(),
                    edges_b.as_slice(),
                ),
            ],
        )
        .expect("restoring B must not fail because of A's earlier hard-deleted RecordId");

        assert_eq!(restored.record_count(), 4);
        for r in &records_b {
            let restored_rec = restored
                .get_record(RecordId(r.id))
                .expect("B record must exist at its original id");
            assert_eq!(restored_rec.namespace_id, 2);
        }
        assert!(restored.get_record(ids_a[1]).is_none());
    }

    #[test]
    fn graph_nodes_and_edges_restore_with_interleaved_collections() {
        let mut state = KernelState::new();
        state
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        state
            .configure_namespace(2, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();

        // Interleaved node and edge creations
        let na0 = state.next_node_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: na0,
                    kind: NodeKind::Document,
                    record: None,
                },
                1,
            )
            .unwrap();
        let nb0 = state.next_node_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: nb0,
                    kind: NodeKind::User,
                    record: None,
                },
                2,
            )
            .unwrap();
        let na1 = state.next_node_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: na1,
                    kind: NodeKind::Concept,
                    record: None,
                },
                1,
            )
            .unwrap();
        let nb1 = state.next_node_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateNode {
                    id: nb1,
                    kind: NodeKind::Tool,
                    record: None,
                },
                2,
            )
            .unwrap();

        // Edge in A
        let ea0 = state.next_edge_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateEdge {
                    id: ea0,
                    from: na0,
                    to: na1,
                    kind: EdgeKind::ParentOf,
                },
                1,
            )
            .unwrap();
        // Edge in B
        let eb0 = state.next_edge_id();
        state
            .apply_event_ns(
                &KernelEvent::CreateEdge {
                    id: eb0,
                    from: nb0,
                    to: nb1,
                    kind: EdgeKind::Follows,
                },
                2,
            )
            .unwrap();

        let (meta_a, recs_a, nodes_a, edges_a) =
            extract_from_kernel_state(&state, NamespaceId(1), 1, Lsn(0), Metric::SquaredL2)
                .unwrap();
        let (meta_b, recs_b, nodes_b, edges_b) =
            extract_from_kernel_state(&state, NamespaceId(2), 1, Lsn(0), Metric::SquaredL2)
                .unwrap();

        assert_eq!(nodes_a.len(), 2);
        assert_eq!(edges_a.len(), 1);
        assert_eq!(nodes_b.len(), 2);
        assert_eq!(edges_b.len(), 1);

        let mut restored = KernelState::new();
        restored
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        restored
            .configure_namespace(2, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();

        restore_project_into(
            &mut restored,
            &[
                (&meta_a, &recs_a, &nodes_a, &edges_a),
                (&meta_b, &recs_b, &nodes_b, &edges_b),
            ],
        )
        .unwrap();

        assert_eq!(restored.node_count(), 4);
        assert_eq!(restored.edge_count(), 2);

        // Verification of namespace isolation
        let r_na0 = restored.get_node(na0).unwrap();
        assert_eq!(r_na0.namespace_id, 1);
        let r_nb0 = restored.get_node(nb0).unwrap();
        assert_eq!(r_nb0.namespace_id, 2);

        let r_ea0 = restored.get_edge(ea0).unwrap();
        assert_eq!(r_ea0.from, na0);
        assert_eq!(r_ea0.to, na1);
        assert_eq!(r_ea0.kind, EdgeKind::ParentOf);

        let r_eb0 = restored.get_edge(eb0).unwrap();
        assert_eq!(r_eb0.from, nb0);
        assert_eq!(r_eb0.to, nb1);
        assert_eq!(r_eb0.kind, EdgeKind::Follows);
    }

    #[test]
    fn empty_collection_snapshots_and_restores_cleanly() {
        let mut state = KernelState::new();
        state
            .configure_namespace(9, 16, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        let (meta, records, nodes, edges) =
            extract_from_kernel_state(&state, NamespaceId(9), 1, Lsn(0), Metric::SquaredL2)
                .unwrap();
        assert_eq!(records.len(), 0);
        assert_eq!(nodes.len(), 0);
        assert_eq!(edges.len(), 0);
        assert_eq!(meta.record_count, 0);
    }
}
