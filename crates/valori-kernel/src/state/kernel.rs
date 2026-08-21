// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Kernel State definition.

use crate::error::{KernelError, Result};
use crate::event::KernelEvent;
use crate::graph::adjacency::{add_edge, OutEdgeIterator};
use crate::graph::node::GraphNode;
use crate::graph::pool::{EdgePool, NodePool};
use crate::index::{
    ActiveIndex, BinaryQuantizationIndex, BruteForceIndex, IndexVariant, Metric, SearchResult,
    VectorIndex,
};
use crate::math::l2::fxp_l2_sq;
use crate::storage::pool::RecordPool;
use crate::storage::record::Record;
use crate::types::id::{EdgeId, NodeId, RecordId};
use crate::types::id::{Version, DEFAULT_NS, MAX_NAMESPACES, NS_LIST_NIL};
use crate::types::vector::FxpVector;

#[derive(Clone)]
pub struct KernelState {
    /// The legacy, single, process-wide dimension fallback — used ONLY by
    /// namespaces with no explicit entry in `namespace_configs` (see
    /// `validate_dim_for_ns`). Self-locks to whatever the first insert into
    /// any such namespace uses.
    ///
    /// # Classification (Phase 3.2 audit)
    ///
    /// **Still active product logic — not dead code, not legacy-only** —
    /// but deliberately narrowed in scope: as of Phase 3.2, the only
    /// namespace the live REST API can still reach without an explicit
    /// `configure_namespace` call is `DEFAULT_NS` (`"default"`, id 0), the
    /// built-in zero-config collection every deployment gets for free (see
    /// `valori_engine::Engine::create_collection`'s doc comment). Every
    /// other collection is created via `configure_namespace` before its
    /// first insert can succeed, so this field is simply never consulted
    /// for them. It is also what a V1–V8 snapshot's header `dim` field
    /// still decodes into (`SNAPSHOT FORMAT` classification: legacy
    /// decode target) — kept for that reason even independent of the
    /// `"default"` namespace's own use of it.
    pub dim: Option<usize>,
    pub(crate) version: Version,
    pub(crate) records: RecordPool,
    pub(crate) nodes: NodePool,
    pub(crate) edges: EdgePool,
    pub(crate) index: ActiveIndex,
    /// Head of the intrusive per-namespace record linked list.
    /// `namespace_record_heads[ns] = NS_LIST_NIL` means namespace `ns` has no records.
    pub(crate) namespace_record_heads: alloc::vec::Vec<u32>,
    /// Head of the intrusive per-namespace node linked list.
    /// Maps DEK (key_id) → list of RecordIds encrypted under it.
    /// Rebuilt during WAL replay; used by `apply_shred_key` to mark records in O(N_key).
    #[cfg(feature = "std")]
    pub(crate) encrypted_record_keys: rustc_hash::FxHashMap<[u8; 16], alloc::vec::Vec<RecordId>>,
    pub(crate) namespace_node_heads: alloc::vec::Vec<u32>,
    /// Replicated metadata sidecar — set via `KernelEvent::SetMeta`.
    /// Key: arbitrary string (e.g. "record:42"). Value: pre-serialised JSON string.
    pub meta: alloc::collections::BTreeMap<alloc::string::String, alloc::string::String>,
    /// Collection-scoped vector configuration, set via
    /// `KernelEvent::ConfigureNamespace`. A namespace absent from this map has
    /// no explicit collection config and continues to use the legacy,
    /// process-wide `dim`/`index` fields — this is what makes an old project
    /// (or any namespace nobody ever configured) behave exactly as before.
    ///
    /// `BTreeMap` (not `HashMap`): `no_std`-compatible via `alloc`, and
    /// iteration order is key-sorted, matching the existing `meta` field's
    /// determinism rationale — relevant if this ever joins the hashed state
    /// (it does not yet; see the `ConfigureNamespace` doc comment).
    pub namespace_configs: alloc::collections::BTreeMap<u16, NamespaceConfig>,
}

/// Collection-scoped vector configuration for one namespace.
///
/// Recorded, replicated, and snapshotted (V8+), but **not enforced** by the
/// kernel itself in this phase — `valori-engine` validates inserts against
/// this before committing, on both the standalone and cluster paths. See
/// `KernelEvent::ConfigureNamespace`'s doc comment for why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamespaceConfig {
    pub dim: u32,
    pub metric: Metric,
    /// Opaque engine-level index-kind tag (`valori_engine::config::IndexKind`
    /// as `u8`). The kernel does not interpret this — it only carries it so
    /// every replica and every snapshot restore reconstructs the same value.
    pub index_kind: u8,
}

impl KernelState {
    pub fn new() -> Self {
        Self {
            dim: None,
            version: Version(0),
            records: RecordPool::new(),
            nodes: NodePool::new(),
            edges: EdgePool::new(),
            index: ActiveIndex::default(),
            namespace_record_heads: alloc::vec![NS_LIST_NIL; MAX_NAMESPACES],
            namespace_node_heads: alloc::vec![NS_LIST_NIL; MAX_NAMESPACES],
            #[cfg(feature = "std")]
            encrypted_record_keys: rustc_hash::FxHashMap::default(),
            meta: alloc::collections::BTreeMap::new(),
            namespace_configs: alloc::collections::BTreeMap::new(),
        }
    }

    /// Create a kernel pre-seeded with an expected dimension from config.
    ///
    /// Use this when VALORI_DIM is known at startup. The first insert will be
    /// validated against this dim rather than silently locking to whatever length
    /// the first vector happens to be.
    pub fn with_dim(dim: usize) -> Self {
        let mut s = Self::new();
        if dim > 0 {
            s.dim = Some(dim);
        }
        s
    }

    // ── Index management ─────────────────────────────────────────────────────

    /// Switch the active kernel index variant and rebuild it from the current
    /// record pool. The rebuild is O(N) over live records.
    pub fn set_index_kind(&mut self, variant: IndexVariant) {
        if self.index.variant() == variant {
            return;
        }
        self.index = match variant {
            IndexVariant::BruteForce => ActiveIndex::BruteForce(BruteForceIndex::default()),
            IndexVariant::BinaryQuantization => {
                ActiveIndex::BinaryQuantization(BinaryQuantizationIndex::new())
            }
        };
        self.index.rebuild(&self.records);
    }

    /// Return the currently active index variant.
    pub fn index_variant(&self) -> IndexVariant {
        self.index.variant()
    }

    // ── Collection-scoped configuration ──────────────────────────────────────

    /// Record collection-scoped config for `namespace_id`. Idempotent when the
    /// dimension matches an existing entry; rejects a conflicting dimension —
    /// configuration is immutable after first set (`CLAUDE.md`'s "immutable
    /// after first insert" convention, applied at configure time instead).
    pub fn configure_namespace(
        &mut self,
        namespace_id: u16,
        dim: u32,
        metric: Metric,
        index_kind: u8,
    ) -> Result<()> {
        if namespace_id as usize >= MAX_NAMESPACES {
            return Err(KernelError::InvalidOperation);
        }
        if dim == 0 {
            return Err(KernelError::InvalidOperation);
        }
        if let Some(existing) = self.namespace_configs.get(&namespace_id) {
            if existing.dim != dim {
                return Err(KernelError::NamespaceAlreadyConfigured {
                    namespace_id,
                    existing_dim: existing.dim,
                });
            }
            return Ok(()); // idempotent re-apply (e.g. replay)
        }
        self.namespace_configs.insert(
            namespace_id,
            NamespaceConfig {
                dim,
                metric,
                index_kind,
            },
        );
        Ok(())
    }

    /// The effective dimension for `namespace_id`: its own explicit config if
    /// one exists, otherwise the legacy process-wide `dim`. `None` means
    /// "not yet known" (no config, no insert yet) — matches the pre-existing
    /// meaning of `self.dim == None`.
    pub fn namespace_dim(&self, namespace_id: u16) -> Option<usize> {
        self.namespace_configs
            .get(&namespace_id)
            .map(|c| c.dim as usize)
            .or(self.dim)
    }

    /// The effective metric for `namespace_id` — always `SquaredL2` today,
    /// explicit config or not (see `Metric`'s doc comment).
    pub fn namespace_metric(&self, namespace_id: u16) -> Metric {
        self.namespace_configs
            .get(&namespace_id)
            .map(|c| c.metric)
            .unwrap_or_default()
    }

    /// `true` if `namespace_id` has an explicit collection config — i.e. was
    /// created through the new per-collection configuration path rather than
    /// inheriting the legacy process-wide `dim`/index.
    pub fn has_namespace_config(&self, namespace_id: u16) -> bool {
        self.namespace_configs.contains_key(&namespace_id)
    }

    /// Validate an incoming vector's length against `namespace_id`'s
    /// effective dimension, auto-locking the legacy `self.dim` on first
    /// insert exactly as before this phase. This is the single dimension
    /// gate shared by `InsertRecord` and `AutoInsertRecord` below.
    ///
    /// Enforced here (not only in `valori-engine`) because the cluster
    /// state machine (`valori-consensus::ValoriStateMachine`) applies
    /// events directly against `KernelState`, bypassing `Engine` entirely —
    /// enforcement at the engine layer alone would leave cluster-mode
    /// inserts to explicitly-configured collections unvalidated. Namespaces
    /// with **no** explicit config are unaffected: this reproduces the
    /// pre-existing `self.dim` check byte-for-byte.
    fn validate_dim_for_ns(&mut self, namespace_id: u16, d: usize) -> Result<()> {
        if let Some(cfg) = self.namespace_configs.get(&namespace_id) {
            let expected = cfg.dim as usize;
            if d != expected {
                return Err(KernelError::DimensionMismatch { expected, found: d });
            }
            return Ok(());
        }
        if let Some(dim) = self.dim {
            if d != dim {
                return Err(KernelError::DimensionMismatch {
                    expected: dim,
                    found: d,
                });
            }
        } else {
            self.dim = Some(d);
        }
        Ok(())
    }

    // ── Crypto-shredding (Phase 3.6) ─────────────────────────────────────────

    /// Destroy a Data Encryption Key and mark all records encrypted under it as
    /// `FLAG_SHREDDED`. Called when applying `KernelEvent::ShredKey`.
    #[cfg(feature = "std")]
    pub fn apply_shred_key(&mut self, key_id: [u8; 16]) -> Result<()> {
        let records = self
            .encrypted_record_keys
            .remove(&key_id)
            .unwrap_or_default();
        for rid in records {
            let _ = self.records.mark_shredded(rid);
        }
        Ok(())
    }

    // --- Read APIs ---

    pub fn version(&self) -> u64 {
        self.version.0
    }

    pub fn record_count(&self) -> usize {
        self.records.iter().count()
    }

    /// Total number of allocated record slots (live + soft-deleted; excludes hard-deleted gaps).
    /// Used to size snapshot buffers and rebuild-index loops correctly.
    pub fn total_record_slots(&self) -> usize {
        self.records.total_slots()
    }

    pub fn get_record(&self, id: RecordId) -> Option<&Record> {
        self.records.get(id)
    }

    pub fn get_node(&self, id: NodeId) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub fn get_edge(&self, id: EdgeId) -> Option<&crate::graph::edge::GraphEdge> {
        self.edges.get(id)
    }

    pub fn outgoing_edges<'a>(&'a self, node_id: NodeId) -> Option<OutEdgeIterator<'a>> {
        self.nodes
            .get(node_id)
            .map(|node| OutEdgeIterator::new(&self.edges, node.first_out_edge))
    }

    pub fn incoming_edges<'a>(
        &'a self,
        node_id: NodeId,
    ) -> Option<crate::graph::adjacency::InEdgeIterator<'a>> {
        self.nodes.get(node_id).map(|node| {
            crate::graph::adjacency::InEdgeIterator::new(&self.edges, node.first_in_edge)
        })
    }

    /// Iterate over all live graph nodes (excludes deleted/hole slots).
    pub fn iter_nodes(&self) -> impl Iterator<Item = &crate::graph::node::GraphNode> {
        self.nodes.nodes.iter().filter_map(|slot| slot.as_ref())
    }

    /// Iterate over all live graph nodes belonging to `namespace_id`.
    pub fn iter_nodes_in_ns(
        &self,
        namespace_id: u16,
    ) -> impl Iterator<Item = &crate::graph::node::GraphNode> {
        self.iter_nodes()
            .filter(move |n| n.namespace_id == namespace_id)
    }

    /// Iterate over all live graph edges.
    pub fn iter_edges(&self) -> impl Iterator<Item = &crate::graph::edge::GraphEdge> {
        self.edges.edges.iter().filter_map(|slot| slot.as_ref())
    }

    /// Iterate over all live graph edges connecting nodes in `namespace_id`.
    pub fn iter_edges_in_ns(
        &self,
        namespace_id: u16,
    ) -> impl Iterator<Item = &crate::graph::edge::GraphEdge> {
        self.iter_edges().filter(move |e| {
            self.nodes
                .get(e.from)
                .map(|n| n.namespace_id == namespace_id)
                .unwrap_or(false)
        })
    }

    /// Iterate over all live records in a given namespace.
    pub fn iter_records_in_ns(
        &self,
        namespace_id: u16,
    ) -> impl Iterator<Item = &crate::storage::record::Record> {
        self.records
            .iter()
            .filter(move |r| r.namespace_id == namespace_id)
    }

    pub fn next_record_id(&self) -> RecordId {
        RecordId(self.records.raw_records().len() as u32)
    }

    /// Live (non-deleted) node count.
    pub fn node_count(&self) -> usize {
        self.nodes.live_count()
    }

    pub fn next_node_id(&self) -> NodeId {
        NodeId(self.nodes.len() as u32)
    }

    /// Live (non-deleted) edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.live_count()
    }

    pub fn next_edge_id(&self) -> EdgeId {
        EdgeId(self.edges.len() as u32)
    }

    pub fn is_edge_active(&self, id: EdgeId) -> bool {
        self.edges.get(id).is_some()
    }

    /// Search across ALL records regardless of namespace (backward-compat, single-tenant).
    pub fn search_l2(
        &self,
        query: &FxpVector,
        results: &mut [SearchResult],
        filter: Option<u64>,
    ) -> usize {
        self.index.search(&self.records, query, results, filter)
    }

    /// Namespace-scoped brute-force search.
    /// Traverses only the records in `namespace_id`'s intrusive linked list — O(N_tenant).
    pub fn search_l2_ns(
        &self,
        query: &FxpVector,
        results: &mut [SearchResult],
        namespace_id: u16,
    ) -> usize {
        let ns = namespace_id as usize;
        if ns >= MAX_NAMESPACES {
            return 0;
        }
        let k = results.len();
        if k == 0 {
            return 0;
        }

        for r in results.iter_mut() {
            *r = SearchResult {
                score: i64::MAX,
                id: RecordId(u32::MAX),
            };
        }

        let mut found = 0usize;
        let mut cursor = self.namespace_record_heads[ns];

        while cursor != NS_LIST_NIL {
            let (next, vec_ref) = match self
                .records
                .records
                .get(cursor as usize)
                .and_then(|s| s.as_ref())
            {
                Some(rec) if rec.is_active() => (rec.next_in_ns, Some(&rec.vector)),
                Some(rec) => (rec.next_in_ns, None),
                None => break,
            };

            if let Some(vec) = vec_ref {
                let dist = fxp_l2_sq(vec, query);
                let candidate = SearchResult {
                    score: dist,
                    id: RecordId(cursor),
                };

                if found < k {
                    // Insertion sort into the result buffer
                    let mut pos = found;
                    while pos > 0 && results[pos - 1] > candidate {
                        results[pos] = results[pos - 1];
                        pos -= 1;
                    }
                    results[pos] = candidate;
                    found += 1;
                } else if candidate < results[k - 1] {
                    let mut pos = k - 1;
                    while pos > 0 && results[pos - 1] > candidate {
                        results[pos] = results[pos - 1];
                        pos -= 1;
                    }
                    results[pos] = candidate;
                }
            }

            cursor = next;
        }

        found
    }

    pub fn create_node(
        &mut self,
        kind: crate::types::enums::NodeKind,
        record: Option<RecordId>,
    ) -> Result<NodeId> {
        let id = NodeId(self.nodes.len() as u32);
        self.apply_event_ns(&KernelEvent::CreateNode { id, kind, record }, DEFAULT_NS.0)?;
        Ok(id)
    }

    pub fn create_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        kind: crate::types::enums::EdgeKind,
    ) -> Result<EdgeId> {
        let id = EdgeId(self.edges.len() as u32);
        self.apply_event_ns(
            &KernelEvent::CreateEdge { id, from, to, kind },
            DEFAULT_NS.0,
        )?;
        Ok(id)
    }

    // --- Event-Sourced Write Logic ---

    /// Apply a `KernelEvent` in the default namespace (single-tenant / backward-compat path).
    pub fn apply_event(&mut self, evt: &KernelEvent) -> Result<()> {
        self.apply_event_ns(evt, DEFAULT_NS.0)
    }

    /// Apply a `KernelEvent` targeting a specific namespace.
    ///
    /// This is the single authoritative apply path. Every mutation flows through here;
    /// there is no intermediate representation between `KernelEvent` and `KernelState`.
    pub fn apply_event_ns(&mut self, evt: &KernelEvent, namespace_id: u16) -> Result<()> {
        match evt {
            KernelEvent::InsertRecord {
                id,
                vector,
                metadata,
                tag,
            } => {
                let ns = namespace_id as usize;
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                if self.records.next_id() != *id {
                    return Err(KernelError::InvalidOperation);
                }
                let d = vector.len();
                self.validate_dim_for_ns(namespace_id, d)?;
                use crate::config::MAX_METADATA_SIZE;
                if let Some(m) = metadata {
                    if m.len() > MAX_METADATA_SIZE {
                        return Err(KernelError::MetadataTooLarge);
                    }
                }
                let allocated_id =
                    self.records
                        .insert(vector.clone(), metadata.clone(), *tag, namespace_id)?;
                debug_assert_eq!(allocated_id, *id);
                let old_head = self.namespace_record_heads[ns];
                {
                    let r = self.records.records[allocated_id.0 as usize]
                        .as_mut()
                        .unwrap();
                    r.next_in_ns = old_head;
                    r.prev_in_ns = NS_LIST_NIL;
                }
                if old_head != NS_LIST_NIL {
                    if let Some(prev_head) = self.records.records[old_head as usize].as_mut() {
                        prev_head.prev_in_ns = allocated_id.0;
                    }
                }
                self.namespace_record_heads[ns] = allocated_id.0;
                self.index.on_insert(allocated_id, vector);
            }

            KernelEvent::DeleteRecord { id } => {
                let (ns, prev_in_ns, next_in_ns) = {
                    let r = self.records.get(*id).ok_or(KernelError::NotFound)?;
                    (r.namespace_id as usize, r.prev_in_ns, r.next_in_ns)
                };
                self._unlink_record_from_ns(ns, prev_in_ns, next_in_ns);
                self.records.delete(*id)?;
                self.index.on_delete(*id);
            }

            KernelEvent::SoftDeleteRecord { id } => {
                let (ns, prev_in_ns, next_in_ns) = {
                    let r = self.records.get(*id).ok_or(KernelError::NotFound)?;
                    (r.namespace_id as usize, r.prev_in_ns, r.next_in_ns)
                };
                self._unlink_record_from_ns(ns, prev_in_ns, next_in_ns);
                self.records.soft_delete(*id)?;
                self.index.on_delete(*id);
            }

            KernelEvent::CreateNode { id, kind, record } => {
                let ns = namespace_id as usize;
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                if self.next_node_id() != *id {
                    return Err(KernelError::InvalidOperation);
                }
                if let Some(rid) = record {
                    let rec = self.records.get(*rid).ok_or(KernelError::NotFound)?;
                    if rec.namespace_id != namespace_id {
                        return Err(KernelError::InvalidOperation);
                    }
                }
                let node = GraphNode::new(*id, *kind, *record, namespace_id);
                let allocated = self.nodes.insert(node)?;
                debug_assert_eq!(allocated, *id);
                let old_head = self.namespace_node_heads[ns];
                {
                    let n = self.nodes.nodes[allocated.0 as usize].as_mut().unwrap();
                    n.next_in_ns = old_head;
                    n.prev_in_ns = NS_LIST_NIL;
                }
                if old_head != NS_LIST_NIL {
                    if let Some(prev_head) = self.nodes.nodes[old_head as usize].as_mut() {
                        prev_head.prev_in_ns = allocated.0;
                    }
                }
                self.namespace_node_heads[ns] = allocated.0;
            }

            KernelEvent::CreateEdge { id, from, to, kind } => {
                if self.next_edge_id() != *id {
                    return Err(KernelError::InvalidOperation);
                }
                let from_ns = self
                    .nodes
                    .get(*from)
                    .ok_or(KernelError::NotFound)?
                    .namespace_id;
                let to_ns = self
                    .nodes
                    .get(*to)
                    .ok_or(KernelError::NotFound)?
                    .namespace_id;
                if from_ns != to_ns {
                    return Err(KernelError::InvalidOperation);
                }
                let allocated = add_edge(&mut self.nodes, &mut self.edges, *kind, *from, *to)?;
                debug_assert_eq!(allocated, *id);
            }

            KernelEvent::DeleteNode { id } => {
                self._delete_node(*id)?;
            }

            KernelEvent::DeleteEdge { id } => {
                self._delete_edge(*id)?;
            }

            KernelEvent::AutoInsertRecord {
                vector,
                metadata,
                tag,
            } => {
                let id = self.next_record_id();
                let ns = namespace_id as usize;
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                let d = vector.len();
                self.validate_dim_for_ns(namespace_id, d)?;
                use crate::config::MAX_METADATA_SIZE;
                if let Some(m) = metadata {
                    if m.len() > MAX_METADATA_SIZE {
                        return Err(KernelError::MetadataTooLarge);
                    }
                }
                let allocated_id =
                    self.records
                        .insert(vector.clone(), metadata.clone(), *tag, namespace_id)?;
                debug_assert_eq!(allocated_id, id);
                let old_head = self.namespace_record_heads[ns];
                {
                    let r = self.records.records[allocated_id.0 as usize]
                        .as_mut()
                        .unwrap();
                    r.next_in_ns = old_head;
                    r.prev_in_ns = NS_LIST_NIL;
                }
                if old_head != NS_LIST_NIL {
                    if let Some(prev_head) = self.records.records[old_head as usize].as_mut() {
                        prev_head.prev_in_ns = allocated_id.0;
                    }
                }
                self.namespace_record_heads[ns] = allocated_id.0;
                self.index.on_insert(allocated_id, vector);
            }

            KernelEvent::AutoCreateNode { kind, record } => {
                let id = self.next_node_id();
                let ns = namespace_id as usize;
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                if let Some(rid) = record {
                    let rec = self.records.get(*rid).ok_or(KernelError::NotFound)?;
                    if rec.namespace_id != namespace_id {
                        return Err(KernelError::InvalidOperation);
                    }
                }
                let node = GraphNode::new(id, *kind, *record, namespace_id);
                let allocated = self.nodes.insert(node)?;
                debug_assert_eq!(allocated, id);
                let old_head = self.namespace_node_heads[ns];
                {
                    let n = self.nodes.nodes[allocated.0 as usize].as_mut().unwrap();
                    n.next_in_ns = old_head;
                    n.prev_in_ns = NS_LIST_NIL;
                }
                if old_head != NS_LIST_NIL {
                    if let Some(prev_head) = self.nodes.nodes[old_head as usize].as_mut() {
                        prev_head.prev_in_ns = allocated.0;
                    }
                }
                self.namespace_node_heads[ns] = allocated.0;
            }

            KernelEvent::AutoCreateEdge { from, to, kind } => {
                let id = self.next_edge_id();
                let from_ns = self
                    .nodes
                    .get(*from)
                    .ok_or(KernelError::NotFound)?
                    .namespace_id;
                let to_ns = self
                    .nodes
                    .get(*to)
                    .ok_or(KernelError::NotFound)?
                    .namespace_id;
                if from_ns != to_ns {
                    return Err(KernelError::InvalidOperation);
                }
                let allocated = add_edge(&mut self.nodes, &mut self.edges, *kind, *from, *to)?;
                debug_assert_eq!(allocated, id);
            }

            KernelEvent::AutoInsertRecordEncrypted {
                namespace_id: evt_ns,
                key_id,
                ciphertext,
                tag,
            } => {
                let id = self.next_record_id();
                // Delegate to the concrete InsertRecordEncrypted arm; that arm will bump version.
                return self.apply_event_ns(
                    &KernelEvent::InsertRecordEncrypted {
                        id,
                        key_id: *key_id,
                        ciphertext: ciphertext.clone(),
                        metadata_ciphertext: None,
                        tag: *tag,
                    },
                    *evt_ns,
                );
            }

            KernelEvent::UpdateRecordMetadata { id, metadata } => {
                self.records.update_metadata(*id, metadata.clone())?;
            }

            KernelEvent::SetMeta { key, value } => {
                self.meta.insert(key.clone(), value.clone());
            }

            KernelEvent::AutoCreateNamespace { name: _ } => {
                // The name is not stored in KernelState — namespaces are pure integer ids here.
                // `namespace_id` is the id already allocated by the consensus layer.
                let ns = namespace_id as usize;
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                // Idempotent: head is already NS_LIST_NIL for a fresh namespace.
            }

            KernelEvent::DropNamespace { name: _ } => {
                // The consensus layer resolved name -> id before calling here.
                let ns = namespace_id as usize;
                if ns == 0 {
                    return Err(KernelError::InvalidOperation);
                }
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                let mut cursor = self.namespace_record_heads[ns];
                while cursor != NS_LIST_NIL {
                    let next = self
                        .records
                        .records
                        .get(cursor as usize)
                        .and_then(|s| s.as_ref())
                        .map(|r| r.next_in_ns)
                        .unwrap_or(NS_LIST_NIL);
                    self.records.records[cursor as usize] = None;
                    self.index.on_delete(RecordId(cursor));
                    cursor = next;
                }
                self.namespace_record_heads[ns] = NS_LIST_NIL;
                let mut node_ids = alloc::vec::Vec::new();
                let mut node_cursor = self.namespace_node_heads[ns];
                while node_cursor != NS_LIST_NIL {
                    let next = self
                        .nodes
                        .nodes
                        .get(node_cursor as usize)
                        .and_then(|s| s.as_ref())
                        .map(|n| n.next_in_ns)
                        .unwrap_or(NS_LIST_NIL);
                    node_ids.push(NodeId(node_cursor));
                    node_cursor = next;
                }
                for nid in node_ids {
                    if self.nodes.get(nid).is_some() {
                        let _ = self._delete_node(nid);
                    }
                }
                self.namespace_node_heads[ns] = NS_LIST_NIL;
                self.namespace_configs.remove(&namespace_id);
            }

            KernelEvent::ConfigureNamespace {
                namespace_id: ns_id,
                dim,
                metric,
                index_kind,
            } => {
                let metric = Metric::from_u8(*metric).ok_or(KernelError::InvalidOperation)?;
                self.configure_namespace(*ns_id, *dim, metric, *index_kind)?;
            }

            KernelEvent::InsertRecordEncrypted {
                id,
                #[cfg(feature = "std")]
                key_id,
                ciphertext,
                tag,
                ..
            } => {
                let ns = namespace_id as usize;
                if ns >= MAX_NAMESPACES {
                    return Err(KernelError::InvalidOperation);
                }
                if self.records.next_id() != *id {
                    return Err(KernelError::InvalidOperation);
                }
                use crate::config::MAX_METADATA_SIZE;
                if ciphertext.len() > MAX_METADATA_SIZE + 28 {
                    return Err(KernelError::MetadataTooLarge);
                }
                // Phase 3.3 fix: this used to read the legacy global `self.dim`
                // directly, ignoring `namespace_id` entirely — every other
                // insert path resolves dimension through `namespace_dim`
                // (namespace-specific config, falling back to the legacy
                // global only for an unconfigured namespace). A configured
                // non-legacy namespace with no insert yet (so `self.dim` is
                // still `None`) would incorrectly 500 here even though its
                // own `NamespaceConfig` has a perfectly good dimension.
                let dim = self
                    .namespace_dim(namespace_id)
                    .ok_or(KernelError::InvalidOperation)?;
                let zero_vec = FxpVector::new_zeros(dim);
                let allocated_id =
                    self.records
                        .insert(zero_vec, Some(ciphertext.clone()), *tag, namespace_id)?;
                debug_assert_eq!(allocated_id, *id);
                self.records.mark_encrypted(allocated_id)?;
                #[cfg(feature = "std")]
                self.encrypted_record_keys
                    .entry(*key_id)
                    .or_default()
                    .push(allocated_id);
                let old_head = self.namespace_record_heads[ns];
                {
                    let r = self.records.records[allocated_id.0 as usize]
                        .as_mut()
                        .unwrap();
                    r.next_in_ns = old_head;
                    r.prev_in_ns = NS_LIST_NIL;
                }
                if old_head != NS_LIST_NIL {
                    if let Some(prev_head) = self.records.records[old_head as usize].as_mut() {
                        prev_head.prev_in_ns = allocated_id.0;
                    }
                }
                self.namespace_record_heads[ns] = allocated_id.0;
                // Zero vectors are not added to the search index.
            }

            KernelEvent::ShredKey { key_id } => {
                #[cfg(feature = "std")]
                self.apply_shred_key(*key_id)?;
                #[cfg(not(feature = "std"))]
                {
                    let _ = key_id;
                    return Err(KernelError::InvalidOperation);
                }
            }
        }

        self.version = self.version.next();
        Ok(())
    }

    // --- Intrusive list helpers ---

    /// Unlink a record from its namespace list using the stored prev/next pointers.
    fn _unlink_record_from_ns(&mut self, ns: usize, prev: u32, next: u32) {
        if prev != NS_LIST_NIL {
            if let Some(r) = self
                .records
                .records
                .get_mut(prev as usize)
                .and_then(|s| s.as_mut())
            {
                r.next_in_ns = next;
            }
        } else {
            // This record was the head
            self.namespace_record_heads[ns] = next;
        }
        if next != NS_LIST_NIL {
            if let Some(r) = self
                .records
                .records
                .get_mut(next as usize)
                .and_then(|s| s.as_mut())
            {
                r.prev_in_ns = prev;
            }
        }
    }

    /// Unlink a node from its namespace list using the stored prev/next pointers.
    fn _unlink_node_from_ns(&mut self, ns: usize, prev: u32, next: u32) {
        if prev != NS_LIST_NIL {
            if let Some(n) = self
                .nodes
                .nodes
                .get_mut(prev as usize)
                .and_then(|s| s.as_mut())
            {
                n.next_in_ns = next;
            }
        } else {
            self.namespace_node_heads[ns] = next;
        }
        if next != NS_LIST_NIL {
            if let Some(n) = self
                .nodes
                .nodes
                .get_mut(next as usize)
                .and_then(|s| s.as_mut())
            {
                n.prev_in_ns = prev;
            }
        }
    }

    fn _delete_node(&mut self, node_id: NodeId) -> Result<()> {
        if self.nodes.get(node_id).is_none() {
            return Err(KernelError::NotFound);
        }

        // Unlink from namespace node list before deletion
        let (ns, prev, next) = {
            let n = self.nodes.get(node_id).unwrap();
            (n.namespace_id as usize, n.prev_in_ns, n.next_in_ns)
        };
        self._unlink_node_from_ns(ns, prev, next);

        let out_edges: alloc::vec::Vec<EdgeId> = {
            let mut acc = alloc::vec::Vec::new();
            let mut curr = self.nodes.get(node_id).and_then(|n| n.first_out_edge);
            while let Some(eid) = curr {
                acc.push(eid);
                curr = self.edges.get(eid).and_then(|e| e.next_out);
            }
            acc
        };

        let in_edges: alloc::vec::Vec<EdgeId> = {
            let mut acc = alloc::vec::Vec::new();
            let mut curr = self.nodes.get(node_id).and_then(|n| n.first_in_edge);
            while let Some(eid) = curr {
                acc.push(eid);
                curr = self.edges.get(eid).and_then(|e| e.next_in);
            }
            acc
        };

        for &eid in out_edges.iter().chain(in_edges.iter()) {
            if self.edges.get(eid).is_some() {
                self._delete_edge(eid)?;
            }
        }

        self.nodes.delete(node_id)?;
        Ok(())
    }

    fn _delete_edge(&mut self, edge_id: EdgeId) -> Result<()> {
        let edge = self.edges.get(edge_id).ok_or(KernelError::NotFound)?;
        let from_node_id = edge.from;
        let to_node_id = edge.to;

        {
            let mut prev: Option<EdgeId> = None;
            let mut curr = self.nodes.get(from_node_id).and_then(|n| n.first_out_edge);
            while let Some(c) = curr {
                if c == edge_id {
                    let next = self.edges.get(c).and_then(|e| e.next_out);
                    if let Some(p) = prev {
                        self.edges.get_mut(p).unwrap().next_out = next;
                    } else {
                        self.nodes.get_mut(from_node_id).unwrap().first_out_edge = next;
                    }
                    break;
                }
                prev = Some(c);
                curr = self.edges.get(c).and_then(|e| e.next_out);
            }
        }

        {
            let mut prev: Option<EdgeId> = None;
            let mut curr = self.nodes.get(to_node_id).and_then(|n| n.first_in_edge);
            while let Some(c) = curr {
                if c == edge_id {
                    let next = self.edges.get(c).and_then(|e| e.next_in);
                    if let Some(p) = prev {
                        self.edges.get_mut(p).unwrap().next_in = next;
                    } else {
                        self.nodes.get_mut(to_node_id).unwrap().first_in_edge = next;
                    }
                    break;
                }
                prev = Some(c);
                curr = self.edges.get(c).and_then(|e| e.next_in);
            }
        }

        self.edges.delete(edge_id)?;
        Ok(())
    }

    // --- Invariant Checker ---

    pub fn check_invariants(&self) -> Result<()> {
        for (i, slot) in self.nodes.raw_nodes().iter().enumerate() {
            if let Some(node) = slot {
                if node.id.0 as usize != i {
                    return Err(KernelError::InvalidOperation);
                }
                if let Some(rid) = node.record {
                    if self.records.get(rid).is_none() {
                        return Err(KernelError::NotFound);
                    }
                }
                if let Some(eid) = node.first_out_edge {
                    if self.edges.get(eid).is_none() {
                        return Err(KernelError::NotFound);
                    }
                    let edge = self.edges.get(eid).unwrap();
                    if edge.from != node.id {
                        return Err(KernelError::InvalidOperation);
                    }
                }
            }
        }

        for (i, slot) in self.edges.raw_edges().iter().enumerate() {
            if let Some(edge) = slot {
                if edge.id.0 as usize != i {
                    return Err(KernelError::InvalidOperation);
                }
                if self.nodes.get(edge.from).is_none() || self.nodes.get(edge.to).is_none() {
                    return Err(KernelError::NotFound);
                }
                if let Some(next_id) = edge.next_out {
                    if self.edges.get(next_id).is_none() {
                        return Err(KernelError::NotFound);
                    }
                    let next_edge = self.edges.get(next_id).unwrap();
                    if next_edge.from != edge.from {
                        return Err(KernelError::InvalidOperation);
                    }
                }
            }
        }

        Ok(())
    }

    /// Rebuild namespace linked lists from the namespace_id fields on records and nodes.
    /// Called after snapshot restore for V1-V5 snapshots (which predate namespaces)
    /// and after any direct pool manipulation that bypasses `apply()`.
    pub fn rebuild_namespace_lists(&mut self) {
        // Reset all heads
        for h in self.namespace_record_heads.iter_mut() {
            *h = NS_LIST_NIL;
        }
        for h in self.namespace_node_heads.iter_mut() {
            *h = NS_LIST_NIL;
        }

        // Walk records in REVERSE order so that after prepend-to-head the
        // list is in forward (ascending ID) order — matching insert order.
        let n = self.records.records.len();
        for idx in (0..n).rev() {
            if let Some(rec) = self.records.records[idx].as_mut() {
                let ns = (rec.namespace_id as usize).min(MAX_NAMESPACES - 1);
                let old_head = self.namespace_record_heads[ns];
                rec.next_in_ns = old_head;
                rec.prev_in_ns = NS_LIST_NIL;
                if old_head != NS_LIST_NIL {
                    if let Some(h) = self.records.records[old_head as usize].as_mut() {
                        h.prev_in_ns = idx as u32;
                    }
                }
                self.namespace_record_heads[ns] = idx as u32;
            }
        }

        let m = self.nodes.nodes.len();
        for idx in (0..m).rev() {
            if let Some(node) = self.nodes.nodes[idx].as_mut() {
                let ns = (node.namespace_id as usize).min(MAX_NAMESPACES - 1);
                let old_head = self.namespace_node_heads[ns];
                node.next_in_ns = old_head;
                node.prev_in_ns = NS_LIST_NIL;
                if old_head != NS_LIST_NIL {
                    if let Some(h) = self.nodes.nodes[old_head as usize].as_mut() {
                        h.prev_in_ns = idx as u32;
                    }
                }
                self.namespace_node_heads[ns] = idx as u32;
            }
        }
    }
}
