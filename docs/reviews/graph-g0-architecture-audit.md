# Graph G0 Architecture Audit

*G0 = "Lock down what the graph actually is." Investigation only — no source code was modified, refactored, or extended. Every conclusion below is traced to a file/function/line. Where no test or code proves a claim, this document says so explicitly instead of assuming it.*

Legend used throughout: `[CODE VERIFIED]` `[TEST VERIFIED]` `[NOT PROVEN]` `[DESIGN INTENT ONLY]` `[CONTRADICTION]`.

---

## 1. Executive Summary

The hypothesis in the prompt is **correct in shape and correct in almost every detail**. Valori's graph is:

- **Canonical**: `KernelState` owns `NodePool` and `EdgePool` directly (not behind a trait, not optional), alongside `RecordPool`. crates/valori-kernel/src/state/kernel.rs:20-39
- **Event-sourced**: `CreateNode`, `CreateEdge`, `DeleteNode`, `DeleteEdge` (plus cluster-mode `AutoCreateNode`/`AutoCreateEdge`) are first-class `KernelEvent` variants, applied through the single authoritative `apply_event_ns()` match statement — the same function that applies vector mutations. crates/valori-kernel/src/event.rs:44-68, 99-112; crates/valori-kernel/src/state/kernel.rs:309-681
- **Snapshotted**: nodes and edges, including every adjacency pointer and namespace linked-list field, are serialized in the kernel snapshot format (current schema version 7) and are covered by round-trip determinism tests. crates/valori-kernel/src/snapshot/encode.rs:133-208; crates/valori-kernel/tests/snapshot_roundtrip.rs:64-75
- **Reconstructible** from the event log via the real (namespace-aware) recovery path in `valori-storage`. crates/valori-storage/src/events/event_replay.rs:43-54, 175-190

Two real, code-verified gaps were found that the hypothesis did not anticipate:

1. **The kernel crate contains a second, namespace-blind `replay_events()` function** that always replays into the default namespace regardless of what namespace an event actually belongs to. It is not wired into the production recovery path, but it exists in the kernel crate under the same name as the real one and is exercised by a `valori-storage` test. See §11 and §18 (Risk R1).
2. **The BLAKE3 canonical state hash does not cover `namespace_id`, the namespace linked-list pointers, the incoming-edge back-pointer (`first_in_edge`/`next_in`), or the `meta` sidecar.** Two states that differ only in namespace placement or in reverse-adjacency pointers would hash identically. See §13 and §18 (Risk R2).

Everything else in the working hypothesis holds up against the code.

---

## 2. Actual Architecture (as found in source)

```
KernelState                                         crates/valori-kernel/src/state/kernel.rs:20-39
├── dim: Option<usize>
├── version: Version
├── records: RecordPool                             — vectors (Q16.16)
├── nodes: NodePool                                  — Vec<Option<GraphNode>>
├── edges: EdgePool                                  — Vec<Option<GraphEdge>>
├── index: ActiveIndex                               — BruteForce | BinaryQuantization (derived)
├── namespace_record_heads: Vec<u32>                 — per-namespace intrusive list heads (records)
├── namespace_node_heads: Vec<u32>                   — per-namespace intrusive list heads (nodes)
├── encrypted_record_keys: FxHashMap<[u8;16], Vec<RecordId>>  — std-only, crypto-shredding index
└── meta: BTreeMap<String, String>                   — SetMeta sidecar (NOT part of the graph)
```

This matches the hypothesis exactly, with two additions not mentioned in the prompt: `encrypted_record_keys` (a `std`-only `FxHashMap`, gated `#[cfg(feature = "std")]`, used only for crypto-shredding lookups — see §15 for its determinism status) and `meta` (a `BTreeMap`, unrelated to the graph, called out here only because it lives at the same struct level).

There is **no separate "graph module" struct wrapping `NodePool`+`EdgePool`** — `KernelState` owns them as sibling fields, exactly like `records`. There is no `Graph` type; the graph is simply "the node pool and edge pool, considered together."

---

## 3. Canonical State

Canonical = "the authoritative source of truth; nothing else may diverge from it."

| Field | Canonical? | Evidence |
|---|---|---|
| `KernelState.records` (RecordPool) | ✅ | mutated only via `apply_event_ns` |
| `KernelState.nodes` (NodePool) | ✅ | mutated only via `apply_event_ns`, same function |
| `KernelState.edges` (EdgePool) | ✅ | same |
| `KernelState.namespace_record_heads` / `namespace_node_heads` | ✅ (structural, derived-but-not-optional) | maintained inline during every insert/delete inside `apply_event_ns`; also independently reconstructible via `rebuild_namespace_lists()` (kernel.rs:876-918), which is invoked automatically for pre-V6 snapshots (decode.rs:434-436) |
| `KernelState.meta` | ✅ (canonical, but not graph) | `SetMeta` event, kernel.rs:562-564 |
| `KernelState.index` (`ActiveIndex`) | ❌ derived | rebuildable via `ActiveIndex::rebuild()`, kernel.rs:75-86 |
| `KernelState.encrypted_record_keys` | derived index over canonical `records` | rebuilt on WAL replay per its own doc comment (kernel.rs:31-32); not itself part of `apply_event_ns`'s "source of truth" — it is populated as a side effect of applying `InsertRecordEncrypted` (kernel.rs:646-650), not read back to reconstruct anything |

Nodes and edges are canonical **by the same mechanism and at the same level** as vectors — there is no architectural seam between "vector state" and "graph state" inside `KernelState`; both are `apply_event_ns` targets, both are covered by `check_invariants()` (kernel.rs:827-871), and both are in the snapshot.

---

## 4. Derived State

Everything that is *not* one of `records`/`nodes`/`edges`/`namespace_*_heads`/`meta` and can be fully reconstructed from them:

| Component | Derived from | Rebuild mechanism |
|---|---|---|
| `KernelState.index` (kernel-native BruteForce/BQ) | `records` | `ActiveIndex::rebuild(&records)`, kernel.rs:85 |
| `Engine.index` (std-level BruteForce/HNSW/IVF/BQ, in `valori-engine`) | `state` (records) | `Engine::build_index()` / `rebuild_index()`, crates/valori-engine/src/engine.rs:1476-1516 |
| `Engine.record_to_node` (`HashMap<u32,u32>`) | `nodes` | `rebuild_record_to_node()`, called after every restore path (engine.rs:1689, 1579, 1644) |
| GraphRAG subgraph result | `nodes`+`edges`, computed per-request | `expand_subgraph()`, crates/valori-rag/src/graph.rs:50-94 — pure read, no state stored |
| Community layer (`CommunityStore`) | `nodes`+`edges`, computed per-request, cached | `build_community_store()`, crates/valori-rag/src/community.rs:242 — **no `KernelEvent` variant exists for community state; it is never part of the event log or snapshot** `[CODE VERIFIED]` |

**No graph-adjacent structure was found that is treated as a second source of truth.** The community layer is the closest thing to a "graph-derived cache" and it is unambiguously derived — no event, no snapshot section, rebuilt from `nodes`/`edges` on each `/v1/community/detect` call.

---

## 5. Graph Node Model

```rust
// crates/valori-kernel/src/graph/node.rs:8-23
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,                 // fixed 7-variant enum
    pub record: Option<RecordId>,       // optional backing vector record
    pub first_out_edge: Option<EdgeId>, // head of outgoing adjacency list
    pub first_in_edge: Option<EdgeId>,  // head of incoming adjacency list (reverse index)
    pub namespace_id: u16,
    pub next_in_ns: u32,                // intrusive per-namespace list pointer
    pub prev_in_ns: u32,
}
```

`NodeKind` (crates/valori-core/src/enums.rs:13-22): `Record, Concept, Agent, User, Tool, Document, Chunk` — fixed enum, `#[default] Record`.

**Owner**: `NodePool { nodes: Vec<Option<GraphNode>> }`, owned directly by `KernelState.nodes`. crates/valori-kernel/src/graph/pool.rs:9-12

**Identity**: `NodeId(u32)`, allocated as `NodePool::insert()`'s slot index (`NodeId(self.nodes.len() as u32)`, pool.rs:26). IDs are never reused after deletion — a deleted slot becomes `None` and stays `None` forever (pool.rs:40-47); the slot count (`len()`) never shrinks (explicit comment, pool.rs:54-55).

---

## 6. Graph Edge Model

```rust
// crates/valori-kernel/src/graph/edge.rs:8-18
pub struct GraphEdge {
    pub id: EdgeId,
    pub kind: EdgeKind,      // fixed 9-variant enum
    pub from: NodeId,
    pub to: NodeId,
    pub next_out: Option<EdgeId>, // next edge in `from`'s outgoing list
    pub next_in: Option<EdgeId>,  // next edge in `to`'s incoming list
}
```

`EdgeKind` (crates/valori-core/src/enums.rs:46-59): `Relation, Follows, InEpisode, ByAgent, Mentions, RefersTo, ParentOf, Supersedes, Contradicts` — fixed enum, `#[default] Relation`. **No custom/free-form relationship type exists.**

**Owner**: `EdgePool { edges: Vec<Option<GraphEdge>> }`, owned directly by `KernelState.edges`.

**No properties field.** `GraphEdge` carries no metadata, no weight, no timestamp. This was verified by reading the full struct definition — there is nothing beyond `id, kind, from, to, next_out, next_in`.

**Directed**: always — `from`/`to` are distinct fields; there is no undirected-edge concept anywhere in the type or in `add_edge()`.

**Duplicates**: `add_edge()` (crates/valori-kernel/src/graph/adjacency.rs:16-45) performs no existence check against `(from, to, kind)` before inserting — creating the same `(A, B, Relation)` edge twice produces two distinct `EdgeId`s, both retained in both adjacency lists. `[CODE VERIFIED — no dedup logic found]`. **No test exercises this specific case** (`[NOT PROVEN]` either way by test; behavior is unambiguous by code reading).

**Self-loops**: allowed and correctly maintained — `test_delete_node_with_self_loop` (crates/valori-node/tests/graph_cascade.rs:407-417) proves a self-loop edge (A→A) is removed exactly once when A is deleted, not double-removed or left dangling.

---

## 7. Node Lifecycle

**Creation** — two entry events:
- `CreateNode { id, kind, record }` (id pre-allocated by caller) — kernel.rs:382-411
- `AutoCreateNode { kind, record }` (id allocated by the state machine at apply time, cluster-mode) — kernel.rs:490-517

Both arms:
1. Validate namespace bound (`ns >= MAX_NAMESPACES` → error).
2. **If `record` is `Some(rid)`**, look up the record and require `rec.namespace_id == namespace_id` (kernel.rs:390-395, 496-501) — **a node cannot be backed by a record from a different namespace.** Reject with `KernelError::NotFound` if the record doesn't exist, or `KernelError::InvalidOperation` if the namespaces mismatch.
3. Construct `GraphNode::new(...)`, insert into `NodePool` (slot-allocated).
4. `debug_assert_eq!` that the allocated slot equals the expected `id` — **this assertion is compiled out in release builds**; a duplicate/out-of-order `CreateNode { id }` in a release build would silently allocate at the *next* free slot rather than the caller-specified one and diverge from `id`. `[CODE VERIFIED — real risk, not previously flagged]` For `CreateNode` (explicit-id path), there is also an earlier hard check `if self.next_node_id() != *id { return Err(...) }` at kernel.rs:387-389, which *does* run in release builds and rejects an out-of-sequence id before the `debug_assert_eq!` is ever reached — so the debug_assert is effectively a redundant safety net given the id-sequence check already gates it. The `AutoCreateNode` path has no such pre-check because its id is computed as `next_node_id()` at that exact instant (kernel.rs:491), so the debug_assert there is checking something that cannot fail under single-threaded sequential application.
5. Prepend into the namespace's intrusive node list (`namespace_node_heads[ns]`).

**Duplicate creation**: attempting `CreateNode { id: X, ... }` where `X` is not exactly `next_node_id()` is rejected outright (kernel.rs:387-389) — you cannot create a node at an already-used id, and you cannot skip ids.

**Deletion**: `KernelEvent::DeleteNode { id }` → `_delete_node()` (kernel.rs:738-778):
1. Unlink the node from its namespace list.
2. Walk its outgoing list, collect every `EdgeId`.
3. Walk its incoming list, collect every `EdgeId`.
4. **Cascade-delete every collected edge** via `_delete_edge()`.
5. Delete the node slot (`nodes.delete(node_id)` → sets slot to `None`).

**Dangling references**: none possible by construction — cascade delete removes every edge touching the node *before* the node slot itself is cleared, so no edge can ever reference a nonexistent node after a `DeleteNode`. `check_invariants()` (kernel.rs:827-871) independently re-verifies this property (every node's `first_out_edge`, if present, must resolve to a real edge whose `from` matches the node).

**Snapshot/replay**: node slots (present/absent), all fields including both adjacency heads and namespace list pointers, are serialized (encode.rs:142-174) and restored with cross-reference validation (decode.rs:296-344, e.g. "if node claims a record, the record must exist" at decode.rs:328-332).

---

## 8. Edge Lifecycle

**Creation** — `CreateEdge { id, from, to, kind }` (kernel.rs:413-432) or `AutoCreateEdge { from, to, kind }` (kernel.rs:519-536):
1. Id-sequence check (`next_edge_id() != *id` → reject), same pattern as nodes.
2. **Both `from` and `to` must already exist as nodes** — `self.nodes.get(*from).ok_or(KernelError::NotFound)?` and same for `to`.
3. **`from` and `to` must be in the same namespace** — `if from_ns != to_ns { return Err(InvalidOperation) }` (kernel.rs:427-429, 531-533). **This is the one explicit cross-namespace graph rule found in the code — edges cannot cross namespace boundaries, enforced at the canonical apply layer, not just the API layer.**
4. `add_edge()` (adjacency.rs:16-45) prepends the new edge onto both `from`'s outgoing list and `to`'s incoming list.

No source/destination validation beyond node existence — self-loops (`from == to`) are permitted (no check against it), and duplicate `(from, to, kind)` tuples are permitted (no check against it either).

**Deletion**: `DeleteEdge { id }` → `_delete_edge()` (kernel.rs:780-823): walks the `from` node's outgoing list to unlink the edge, walks the `to` node's incoming list to unlink it, then deletes the edge slot. Both list-unlink operations are O(degree), not O(E) — this is the entire point of maintaining `first_in_edge`/`next_in` back-pointers per the module doc comment (adjacency.rs:10-13).

**Snapshot/replay**: edge slots including both `next_out` and `next_in` are serialized (encode.rs:185-208) and restored with endpoint-existence validation (decode.rs:358-393, "both endpoints must exist in the node pool").

---

## 9. Namespace Semantics

Determined **from code, not assumption**:

| Question | Answer | Evidence |
|---|---|---|
| Can a node belong to exactly one namespace? | Yes — `GraphNode.namespace_id: u16`, set once at creation, never mutated after | node.rs:18; no event mutates an existing node's namespace |
| Can an edge cross namespaces? | **No — explicitly rejected** | kernel.rs:427-429 (`if from_ns != to_ns { return Err(InvalidOperation) }`) |
| Can a node reference a record from a different namespace? | **No — explicitly rejected** at `CreateNode`/`AutoCreateNode` time | kernel.rs:390-395, 496-501 |
| Enforced at mutation time or only API level? | **At mutation time, inside `apply_event_ns` — the canonical apply path itself**, not merely at the HTTP layer | same lines |
| Enforced during replay? | Yes — replay calls the exact same `apply_event_ns`, so the same checks run; a corrupted/malicious event log with a cross-namespace edge would be rejected during replay exactly as it would during live application | kernel.rs:309 doc comment: "the single authoritative apply path... every mutation flows through here" |
| Are namespace IDs part of canonical state? | Yes — `namespace_id` is a stored field on both `Record` and `GraphNode`, serialized in the snapshot | storage/record.rs:33, graph/node.rs:18; encode.rs:125, 170 |
| Does the state hash cover namespace_id? | **No — see §13.** This is a real gap between "canonical" and "hashed." |
| Can traversal (BFS `expand_subgraph`) cross namespace boundaries? | **Not directly tested, but architecturally possible if misused**: `expand_subgraph()` (valori-rag/src/graph.rs:50-94) takes raw `NodeId` seeds and walks `outgoing_edges()` with no namespace filter of its own. Since edges cannot cross namespaces (enforced at creation), a BFS starting inside namespace N can never reach a node in namespace M through an edge — but the function itself does not independently verify this; it relies entirely on the invariant enforced at `CreateEdge` time. **If that invariant were ever violated (e.g., by a future distinct edge-creation path that bypasses `apply_event_ns`), `expand_subgraph` would silently traverse across namespaces with no guard of its own.** `[CODE VERIFIED — architecturally correct today, but this is a single point of enforcement, not defense-in-depth]` |

---

## 10. Graph + Vector Relationship

Actual implementation, verified against both structs:

```rust
GraphNode { record: Option<RecordId>, ... }   // graph/node.rs:11
Record    { id: RecordId, vector: FxpVector, ... }  // storage/record.rs (no back-reference to any NodeId)
```

- **Is every graph node backed by a Record?** No — `record: Option<RecordId>`. A node can exist with `record: None` (e.g., a purely structural/concept node).
- **Can graph nodes exist without vectors?** Yes, per the above.
- **Can records exist without graph nodes?** Yes — nothing in `InsertRecord`/`AutoInsertRecord` creates a node. Vector-only usage (no graph at all) is fully supported and is in fact the default for a plain `/v1/records` insert (§ confirmed in the prior vector audit — inserting a record never implicitly creates a node).
- **Is graph identity separate from vector identity?** Yes — `NodeId` and `RecordId` are distinct `u32` id spaces with independent allocators (`next_node_id()` vs `next_record_id()`), linked only by the optional `record: Option<RecordId>` field.
- **Does deleting a record delete its graph node?** **Only at the `Engine` level, not the kernel level.** `Engine::soft_delete_record()` (crates/valori-engine/src/engine.rs:1217-1229) explicitly checks `self.record_to_node.get(&id)` and, if a mapping exists, calls `self.delete_node(node_id)` *before* committing the `SoftDeleteRecord` event. **The kernel's own `apply_event_ns` for `SoftDeleteRecord`/`DeleteRecord` does nothing to any node** (kernel.rs:362-380) — if a caller committed a raw `KernelEvent::DeleteRecord` directly against `KernelState` (bypassing `Engine`), any `GraphNode.record` pointing at that record would become a dangling reference with no kernel-level guard against it. `check_invariants()` does not check this direction (it only validates node→record existence lazily when walking nodes, not on every delete). `[CODE VERIFIED — record→node cascade is an Engine-level convention, not a kernel-level invariant]`
- **Does deleting a graph node delete the record?** No — `_delete_node()` never touches `records`.
- **Does vector search return graph node IDs or Record IDs?** Record IDs — `search_l2_ns` returns `(RecordId, distance)` pairs (kernel.rs:205-272); nothing in the search path knows about nodes.
- **GraphRAG bridge**: `resolve_seed_nodes(state, record_ids)` (valori-rag/src/graph.rs:28-42) scans **every** live node (`state.iter_nodes()`) and builds a `RecordId → NodeId` map for the requested record ids, taking the *first* node found per record ("first node wins per record ... deterministic in iteration order" — the function's own doc comment, graph.rs:26-27). This is an O(total_nodes) scan per GraphRAG call, not an indexed lookup — there is no `record_to_node` structure available at the kernel/cluster level (that map exists only inside `Engine`, standalone-only, per engine.rs:134). The cluster data plane's GraphRAG task must re-scan the whole node pool every call.

---

## 11. Event Log

Every `KernelEvent` variant that touches the graph, and its replay path:

| Event | Creates canonical state? | Deletes canonical state? | Applied in | Deterministic? |
|---|---|---|---|---|
| `CreateNode { id, kind, record }` | ✅ node | — | kernel.rs:382-411 | ✅ (id-sequence gated) |
| `AutoCreateNode { kind, record }` | ✅ node (server-assigned id) | — | kernel.rs:490-517 | ✅ (sequential apply) |
| `CreateEdge { id, from, to, kind }` | ✅ edge | — | kernel.rs:413-432 | ✅ |
| `AutoCreateEdge { from, to, kind }` | ✅ edge (server-assigned id) | — | kernel.rs:519-536 | ✅ |
| `DeleteNode { id }` | — | ✅ node + cascades all incident edges | kernel.rs:434-436 → `_delete_node` | ✅ |
| `DeleteEdge { id }` | — | ✅ edge | kernel.rs:438-440 → `_delete_edge` | ✅ |
| `DropNamespace { name }` | — | ✅ cascades every record AND every node (with edge cascade) in that namespace | kernel.rs:576-618 | ✅ (walks the namespace linked list, deterministic order) |

**Bypass check**: I searched for any graph mutation path that does not go through `apply_event_ns`. `KernelState::create_node()`/`create_edge()` (kernel.rs:274-296) are convenience wrappers that themselves build a `KernelEvent` and call `apply_event_ns` internally — **not a bypass**, just sugar. No other mutation path into `nodes`/`edges` was found in the kernel crate (both fields are `pub(crate)`, so external crates cannot mutate them directly at all — Rust's visibility rules enforce this structurally, not just by convention). `[CODE VERIFIED — no bypass found]`

**Serialization determinism**: `KernelEvent` has a hand-written `Serialize`/`Deserialize` impl (event.rs:182-601) with a roundtrip test (`test_event_roundtrip`, event.rs:623-640, uses `CreateNode`) and a determinism test (`test_event_serialization_determinism`, event.rs:607-621, uses `InsertRecord` — **not a graph event**, so byte-for-byte serialization determinism is directly tested for records but only roundtrip-correctness, not byte-stability, is directly tested for `CreateNode`/`CreateEdge`). `[TEST VERIFIED for roundtrip; NOT PROVEN for byte-identical serialization of graph events specifically, though the same generic `#[derive]`-free serializer code path is used for both]`

---

## 12. Snapshot

Exact encoder/decoder: crates/valori-kernel/src/snapshot/{encode,decode}.rs. Current `SCHEMA_VERSION = 7` (encode.rs:22).

> **Discrepancy flag**: CLAUDE.md's "Snapshot format versions" table lists V6 as current. The code's `SCHEMA_VERSION` constant is **7** as of this audit (V7 adds the `KernelState.meta` sidecar — comment at encode.rs:22, confirmed by the `v7_meta_roundtrips` test at snapshot_roundtrip.rs:117-147). This is a documentation staleness issue in CLAUDE.md, not a code defect — flagged per the instruction to surface contradictions.

| Question | Answer | Evidence |
|---|---|---|
| Nodes serialized? | ✅ — id, kind, record, first_out_edge, first_in_edge, namespace_id, next_in_ns, prev_in_ns | encode.rs:142-174 |
| Edges serialized? | ✅ — id, kind, from, to, next_out, next_in | encode.rs:185-208 |
| Adjacency links serialized? | ✅ — both directions (`first_out_edge`/`first_in_edge` on nodes; `next_out`/`next_in` on edges) | same |
| IDs serialized? | ✅ | same |
| Namespaces serialized? | ✅ — per-node/edge `namespace_id` + the two 1024-entry namespace head arrays | encode.rs:210-217 |
| Graph "properties"? | N/A — none exist to serialize (§6) | — |
| Graph indexes (community, GraphRAG cache) serialized? | ❌ — not part of the snapshot at all (they are not canonical, §4) | — |
| Ordering preserved? | ✅ — pool slot order (`Vec` index), which is also insertion order for never-deleted-and-reinserted-at-same-slot data | encode.rs iterates `raw_nodes()`/`raw_edges()` in `Vec` order |
| Encoding deterministic? | ✅, tested | `snapshot_is_bit_identical_across_three_encodes` (kernel/tests/determinism.rs:152-160), `writer_encoder_matches_vec_encoder` (snapshot_roundtrip.rs:151-169) |
| Decoding deterministic + safe against corruption? | ✅, tested | decode.rs's entire "Security model" doc comment (lines 1-18) plus 5 dedicated hardening tests (snapshot_roundtrip.rs:205-260): invalid flag byte, oversized dim, oversized slot count, record-id/slot mismatch, unsupported schema version — all rejected |
| Cross-reference validation on decode? | ✅ — node claiming a nonexistent record is rejected (decode.rs:328-332); edge referencing a nonexistent node endpoint is rejected (decode.rs:371-376) | — |

**Node/edge round-trip test**: `roundtrip_preserves_state_hash` (kernel/tests/snapshot_roundtrip.rs:64-75) builds a state with 20 records, 4 nodes, 3 edges, encodes, decodes, and asserts: state-hash equality, `record_count()` equality, `node_count()` equality, `edge_count()` equality. This is a genuine, code-verified snapshot-equivalence proof, though it checks aggregate counts + hash rather than exhaustively diffing every field (the hash gap in §13 means this test does not, by itself, prove namespace/reverse-adjacency fidelity survives round-trip — though decode.rs's own cross-reference validation independently guards structural consistency).

---

## 13. Replay

Two distinct implementations exist under the same function name `replay_events`, in different crates — this is the most important finding of this audit:

### 13a. `valori_kernel::replay_events::replay_events` — namespace-blind, NOT the production path

```rust
// crates/valori-kernel/src/replay_events.rs:125-136
pub fn replay_events(events: &[KernelEvent]) -> Result<KernelState> {
    let mut state = KernelState::new();
    for evt in events {
        state.apply_event(evt)?;   // apply_event() hardcodes DEFAULT_NS (kernel.rs:301-303)
    }
    Ok(state)
}
```

Every event is applied via `apply_event()`, which is a thin wrapper that **always** passes `DEFAULT_NS.0` (kernel.rs:301-303). A multi-namespace event log replayed through this function would have every record/node/edge land in namespace 0 regardless of where it actually belonged — a correctness bug if this function were used for real multi-tenant recovery.

**Is it used in production?** No production caller was found. Its only caller is `crates/valori-storage/tests/wal_validation.rs:280,292`, in two negative-assertion tests (`replay_events(&events).is_err()`) unrelated to namespace correctness.

### 13b. `valori_storage::events::event_replay::replay_events` — the real, namespace-aware recovery path

```rust
// crates/valori-storage/src/events/event_replay.rs:43-54
pub fn replay_events(events: &[(u16, KernelEvent)]) -> Result<KernelState> {
    let mut state = KernelState::new();
    for (idx, (namespace_id, event)) in events.iter().enumerate() {
        state.apply_event_ns(event, *namespace_id).map_err(...)?;
    }
    Ok(state)
}
```

This version correctly threads each event's recorded namespace through `apply_event_ns`. It is called from `recover_from_event_log()` (event_replay.rs:175-190), which is invoked via `valori_state::bootstrap::recover_from_events()` from `Engine::try_recover()` (crates/valori-engine/src/engine.rs:1557) — **this is the actual startup recovery path used by the node.** Per-event namespace is read from the on-disk log format's `LogEntry::EventNs { namespace_id, event }` variant (backward-compatible with pre-namespace `LogEntry::Event`, defaulting to `DEFAULT_NS.0` — event_replay.rs:96-109).

**Does replay use the same mutation code as normal execution?** Yes — both paths ultimately call `KernelState::apply_event_ns`, the single authoritative apply function (§11). There is no separate "replay-mode" code path that could diverge from live-apply semantics.

**Multi-segment replay** (rotated event logs): `read_all_segments()` (event_replay.rs:126-171) discovers sealed archive segments plus the live file, sorts by `segment_seq`, and verifies each segment's `prev_segment_chain_head` splices onto the prior segment's `final_chain_head` before concatenating events — a broken/substituted archive is detected and rejected (`ReplayError::Corrupted`), not silently skipped.

**Graph-specific replay-equivalence test**: `two_identical_builds_produce_identical_snapshot_bytes` (kernel/tests/determinism.rs:171-180) builds `complex_state()` (which includes 8 `CreateNode` + 4 `CreateEdge` events, kernel/tests/determinism.rs:113-148) via two independent `KernelState` instances and asserts byte-identical snapshots. Since "replay" and "apply" are the literal same function call (`apply_event`) executed twice on the same event sequence, this **is** a valid proof of "same events ⇒ same graph state" for graph-containing state, though it is not phrased as an explicit `apply(S0,E) vs replay(E)` A/B test — no test was found with that exact framing that specifically includes `CreateNode`/`CreateEdge`/`DeleteNode`/`DeleteEdge` events. `[TEST VERIFIED for construction-determinism of graph state; NOT PROVEN as an explicitly-labeled event-log-replay-vs-live-apply comparison for graph mutations specifically — the closest such test (`replay_produces_same_hash_and_record_count`, determinism.rs:184-219) uses only `InsertRecord` events]`

---

## 14. Traversal

`crates/valori-rag/src/graph.rs` — the only traversal implementation found.

- **Algorithm**: BFS (`VecDeque` FIFO queue, `expand_subgraph`, graph.rs:50-94). No DFS, no shortest-path, no relationship-type-filtered traversal anywhere in the codebase (searched for these terms across `valori-rag`, `valori-node`, `valori-kernel`; only BFS exists).
- **Maximum depth**: hard-capped at `MAX_DEPTH = 4` (graph.rs:19, 51) — a caller-supplied `depth` is silently clamped, not rejected.
- **Visited set**: `HashSet<u32>` for both nodes and edges (`visited_nodes`, `visited_edges`, graph.rs:53-54) — a node/edge is emitted at most once even if reachable via multiple paths.
- **Ordering**: node/edge emission order follows BFS queue-pop order, which follows the order edges were pushed, which follows `outgoing_edges()` iteration order — itself the `next_out` linked-list order, which is **most-recently-created-edge-first** (new edges are prepended to the list head, adjacency.rs:34, 40). This is deterministic given fixed input state, but is *not* the same as edge-creation order (it's reverse creation order per node).
- **Namespace behavior**: no explicit namespace filter inside `expand_subgraph` itself — relies entirely on the invariant that edges never cross namespaces (§9's caveat applies here too).
- **Determinism**: given the same `KernelState` and the same seed set, `expand_subgraph` is deterministic (`HashSet`/`HashMap` here only gate *visited/dedup* membership tests, not iteration order that feeds output — output order comes from the `Vec` push order, not hash-map iteration). `[CODE VERIFIED]` No dedicated determinism test was found for `expand_subgraph` output ordering itself (`[NOT PROVEN by test]` — only `empty_seeds_returns_empty` and `resolve_seeds_empty_state`, graph.rs:100-115, exist as tests, both trivial edge cases).
- **Complexity**: O(visited nodes + visited edges), bounded by `MAX_DEPTH`; no maximum-graph-size assumption is enforced inside the function itself (a namespace with millions of nodes at depth 4 with high fan-out could still produce a very large result — no size cap beyond depth).
- **Touches canonical or derived structures?** Canonical only — reads directly from `KernelState.nodes`/`.edges` via `get_node`/`outgoing_edges`; produces no persisted structure of its own (pure read, JSON-serialized inline).

**GraphRAG trace** (`POST /v1/graphrag`, crates/valori-node/src/server.rs:1825-1908, cluster equivalent at cluster_server.rs:2002):
1. Resolve `collection` → namespace id.
2. Vector KNN search runs (via the planner's `GraphRagTask`, dispatched through `run_graph_inline`) → yields hit `RecordId`s.
3. `resolve_seed_nodes(state, record_ids)` (graph.rs:28-42) → `RecordId → NodeId` map, first-node-wins.
4. `expand_subgraph(state, seeds, depth)` → BFS.
5. Response assembled: `{ hits, seed_nodes, subgraph: { nodes, edges } }`.

All four steps read a single `KernelState` snapshot reference — there is no intermediate copy or second store, so no cross-store drift is architecturally possible within one call. `[CODE VERIFIED]`

---

## 15. Determinism

Systematic scan for anything that could break deterministic graph behavior:

| Concern | Found where | Classification |
|---|---|---|
| `HashMap`/`HashSet` in `KernelState` graph fields | None in `NodePool`/`EdgePool` themselves (both are plain `Vec<Option<T>>`) | **SAFE** |
| `HashSet<u32>` in BFS visited-set (`expand_subgraph`) | graph.rs:53-54 | **SAFE** — only used for O(1) membership tests, not iterated for output order (output order comes from `Vec` push order) |
| `HashMap<u32,u32>` for `resolve_seed_nodes` | graph.rs:30 | **SAFE** — used only for lookup by known key (`record_ids`), never iterated for ordering |
| `HashMap<u32,u32>` `Engine.record_to_node` | engine.rs:134 | **SAFE for correctness** (lookup only) but **derived/rebuildable, not canonical** — rebuilt via `rebuild_record_to_node()` after every restore, so its construction order doesn't need to be deterministic |
| `rustc_hash::FxHashMap<[u8;16], Vec<RecordId>>` `encrypted_record_keys` | kernel.rs:34 | **SAFE** — lookup-only (`apply_shred_key`, kernel.rs:98-107), never iterated for canonical output |
| Random IDs / UUID generation for nodes or edges | none found — `NodeId`/`EdgeId` are sequential slot indices | **SAFE** |
| Timestamps on nodes/edges | none — `GraphNode`/`GraphEdge` have no timestamp fields (matches the kernel-wide "no timestamps" determinism invariant stated in event.rs:8) | **SAFE** |
| Floating point in graph structures | none — all graph fields are integer (`u32`/`u16`/enum-as-u8) | **SAFE** |
| Nondeterministic iteration order affecting canonical output | `Vec<Option<T>>` iteration is always index order — deterministic by construction | **SAFE** |
| Parallel mutation of `KernelState` | Not investigated in this pass at the concurrency-primitive level (e.g., whether `Engine`'s `RwLock<Engine>` could allow interleaved graph writes from concurrent Raft-committed events) — CLAUDE.md's invariant #6 (`watcher_tasks` abort ordering) suggests concurrency hazards are a known, actively-managed concern elsewhere in the codebase, but this audit did not trace every write-lock acquisition site for the graph specifically. | **NOT PROVEN either way — flagged as a gap in this pass, not a confirmed violation** |
| Unordered serialization of graph structures | Snapshot encode iterates `Vec` in index order (§12); event serialization is a hand-written struct-variant encoder (event.rs), not derive-based hashmap serialization | **SAFE** |
| Platform/OS-dependent behavior in graph code | none found — no `#[cfg(target_os)]`/`#[cfg(target_arch)]` branches anywhere in `graph/` module | **SAFE** |
| Kernel-level `replay_events()` namespace-blindness | §13a | **CONFIRMED VIOLATION of intended semantics, but unused in production** — see Risk R1 |
| BLAKE3 state hash omits `namespace_id`, namespace list pointers, `first_in_edge`/`next_in`, `meta` | §16, blake3.rs:110-156 | **CONFIRMED GAP between "canonical" and "hashed"** — see Risk R2 |

---

## 16. Canonical vs Derived Matrix

| Component | Canonical? | Persisted? | Event sourced? | Snapshot? | Rebuilt? | Affects state hash? | Evidence |
|---|---|---|---|---|---|---|---|
| `RecordPool` | ✅ | ✅ (snapshot + event log) | ✅ | ✅ | via replay/decode | ✅ (id, flags, vector, tag, metadata) | blake3.rs:88-108 |
| `Record.namespace_id`/`next_in_ns`/`prev_in_ns` | ✅ (canonical field) | ✅ (snapshot) | ✅ (implicit via `apply_event_ns`) | ✅ | ✅ (`rebuild_namespace_lists` for old snapshots) | **❌ NOT hashed** | blake3.rs:88-108 hashes only id/flags/vector/tag/metadata |
| `NodePool` | ✅ | ✅ | ✅ (`CreateNode`/`AutoCreateNode`) | ✅ | via replay/decode | ⚠️ partially — `id`, `kind`, `record`, `first_out_edge` only | blake3.rs:110-136 |
| `GraphNode.first_in_edge` | ✅ (canonical field) | ✅ (snapshot) | ✅ (maintained by `add_edge`/`_delete_edge`) | ✅ | ✅ | **❌ NOT hashed** | blake3.rs:110-136 has no `first_in_edge` line |
| `GraphNode.namespace_id`/`next_in_ns`/`prev_in_ns` | ✅ | ✅ | ✅ | ✅ | ✅ | **❌ NOT hashed** | same |
| `EdgePool` | ✅ | ✅ | ✅ (`CreateEdge`/`AutoCreateEdge`) | ✅ | via replay/decode | ⚠️ partially — `id`, `kind`, `from`, `to`, `next_out` only | blake3.rs:138-156 |
| `GraphEdge.next_in` | ✅ (canonical field) | ✅ (snapshot) | ✅ | ✅ | ✅ | **❌ NOT hashed** | blake3.rs:138-156 has no `next_in` line |
| `KernelState.meta` | ✅ (canonical, non-graph) | ✅ (snapshot V7) | ✅ (`SetMeta`) | ✅ | — | **❌ NOT hashed** | blake3.rs has no meta section at all |
| `KernelState.index` (BruteForce/BQ) | ❌ derived | N/A | ❌ | N/A (not in snapshot) | ✅ always, from `records` | N/A | kernel.rs:75-86 |
| `Engine.index` (valori-index: HNSW/IVF/BQ-std/BruteForce) | ❌ derived | ✅ for HNSW/IVF (snapshot Index section); ❌ no-op for BQ-std | ❌ | ✅/❌ (index-dependent, see prior audit §5) | ✅ | N/A — **kernel proof/hash never covers this regardless of persistence** | engine.rs:1076-1083, 1680-1687 |
| `Engine.record_to_node` | ❌ derived | ❌ | ❌ | ❌ | ✅ `rebuild_record_to_node()` | N/A | engine.rs:1689 |
| GraphRAG subgraph result | ❌ derived (pure computation) | ❌ | ❌ | ❌ | recomputed every call | N/A | graph.rs:50-94 |
| Community layer (`CommunityStore`) | ❌ derived | ❌ (in-memory cache only) | ❌ (no `KernelEvent` variant) | ❌ | recomputed on `/v1/community/detect` | N/A | community.rs:242 |

---

## 17. Test Coverage

| Property | Test exists? | Test location | What it proves |
|---|---|---|---|
| Node creation | ✅ | kernel/tests/snapshot_roundtrip.rs:36-44, determinism.rs:125-132, state_machine.rs:70-77 | `CreateNode` applies successfully as part of larger state-building fixtures |
| Node deletion | ✅ | node/tests/graph_cascade.rs (multiple), kernel/tests/state_machine.rs:90 | cascade correctness under various topologies |
| Edge creation | ✅ | same fixtures as node creation | `CreateEdge` applies; namespace/endpoint validation implied by not erroring |
| Edge deletion | ✅ | graph_cascade.rs:279-357 (`test_delete_edge_unlinks_from_both_lists`, `test_delete_middle_incoming_edge_stitches_list`) | both-direction list unlinking correctness |
| Duplicate edge (same from/to/kind twice) | ❌ | none found | **NOT PROVEN by test** — behavior verified only by code reading (§6: allowed) |
| Self-loop | ✅ | graph_cascade.rs:404-417 (`test_delete_node_with_self_loop`) | self-loop edge removed exactly once on node deletion |
| Cross-namespace edge rejection | ❌ | none found | **NOT PROVEN by test** — behavior verified only by code reading (§9/§8: rejected) |
| Namespace isolation (records) | ✅ | kernel/tests/state_machine.rs:148-166 (`drop_namespace_cascades_records_in_that_namespace`) | cascading namespace drop leaves other namespaces untouched — for records; the equivalent for **nodes specifically** is exercised by the same `DropNamespace` code path but not asserted on with a node-specific test |
| Snapshot roundtrip (graph-inclusive) | ✅ | kernel/tests/snapshot_roundtrip.rs:64-75 (`roundtrip_preserves_state_hash`) | hash + record/node/edge count equality after encode→decode |
| Snapshot roundtrip (reverse index specifically) | ✅ | node/tests/graph_cascade.rs:358-403 (`test_snapshot_preserves_reverse_index`) | `first_in_edge`/`next_in` survive a snapshot round-trip (field-level, not just hash — since the hash doesn't cover these fields per §16, this test is the *only* proof these fields round-trip correctly) |
| Replay (event-log reconstruction) | ✅ (records only, directly); ⚠️ (graph, indirectly) | kernel/tests/determinism.rs:184-219 (records); :171-180 (graph, via construction-determinism) | see §13 nuance |
| Graph state hash stability | ✅ | kernel/tests/determinism.rs:152-160, 162-169 | hash + byte-identical encoding stable across repeated encodes of graph-inclusive state |
| Deterministic traversal (BFS output) | ⚠️ trivial only | valori-rag/src/graph.rs:100-115 | only empty-input cases tested; no test asserts BFS output order/content for a nontrivial graph |
| Graph + vector integration | ✅ | node/tests/api_graphrag.rs (file present) | `/v1/graphrag` wiring (not individually enumerated in this pass) |
| Restart recovery (graph-inclusive) | ⚠️ partial | `dr_disaster_recovery.rs` per CLAUDE.md is vector-focused (10k vectors); no equivalent "10k nodes/edges, drop engine, restore, verify hash" test was located for the graph specifically | **NOT PROVEN by a graph-equivalent DR test** |
| Corrupted graph data (decode hardening) | ✅ (record-focused; graph-adjacent via shared decoder) | snapshot_roundtrip.rs:205-260 | decoder rejects malformed flags/dim/slot-count/id-mismatch/version; node/edge cross-reference validation is separately code-verified (decode.rs:328-332, 371-376) but no dedicated *test* corrupts a node/edge field specifically (all 5 hardening tests in this file target record-section or header fields) |

---

## 18. Architectural Risks

**R1 — MEDIUM — Namespace-blind `replay_events()` exists in the kernel crate under the same name as the real, namespace-aware one.**
`crates/valori-kernel/src/replay_events.rs:125-136` always applies into `DEFAULT_NS`. It is not called by the production recovery path today (`valori-storage`'s own `replay_events` at event_replay.rs:43-54 is what `Engine::try_recover()` actually uses). The risk is **latent, not active**: a future refactor, a new test, an FFI binding, or a CLI tool that imports `valori_kernel::replay_events::replay_events` expecting "the" replay function would silently mis-route every non-default-namespace record/node/edge into namespace 0, with no compiler error and no runtime error (it succeeds, just wrong). Classified MEDIUM rather than CRITICAL because it is currently dead in the request path, but the name collision with the correct implementation in a different crate is a genuine landmine.

**R2 — MEDIUM — The canonical BLAKE3 state hash does not cover namespace membership, the incoming-edge reverse index, or the meta sidecar.**
`hash_state_blake3()` (crates/valori-kernel/src/snapshot/blake3.rs:73-159) hashes, per node: `id, kind, record, first_out_edge`. It does **not** hash `first_in_edge`, `namespace_id`, `next_in_ns`, `prev_in_ns`. Per edge, it hashes `id, kind, from, to, next_out` but **not** `next_in`. It does not hash `state.meta` at all. Practical consequence: two `KernelState` instances that differ *only* in which namespace a node lives in, or whose incoming-edge lists have diverged (e.g., through a hypothetical bug in `_delete_edge`'s unlink logic that corrupts `next_in` but not `next_out`), or that differ in `SetMeta` contents, would produce **identical** state hashes. Since the state hash is Valori's cross-replica convergence check and its cryptographic proof primitive (per crypto/mod.rs doc intent and CLAUDE.md's positioning), this is a real gap in what the proof actually guarantees for graph and namespace correctness. The dedicated `test_snapshot_preserves_reverse_index` test (graph_cascade.rs:358-403) compensates for this at the snapshot layer by checking `first_in_edge` field-by-field rather than via hash — but that test does not extend to cross-replica hash-convergence checks (e.g. the `state_hash_match` gauge mentioned in CLAUDE.md's `replication.rs` description), which *would* miss a reverse-index or namespace divergence between replicas.

**R3 — LOW — `debug_assert_eq!` on allocated-id in `CreateNode`/`AutoCreateNode`/`CreateEdge`/`AutoCreateEdge` compiles out in release builds.**
For the explicit-id paths (`CreateNode`, `CreateEdge`), a hard `if next_id() != *id { return Err(...) }` check runs *before* the debug_assert and is not compiled out, so this is low-risk there (the debug_assert is genuinely redundant). For the `Auto*` paths, the id is derived as `next_node_id()`/`next_edge_id()` immediately before use, so under strictly sequential single-threaded application it cannot diverge either. Classified LOW because no path was found where the debug_assert is load-bearing in release mode — but it is worth noting for anyone reasoning about "is this actually checked in prod."

**R4 — LOW — `expand_subgraph` and BFS traversal generally have no independent namespace guard; they rely entirely on the edge-creation-time invariant.**
Correct today because `CreateEdge`/`AutoCreateEdge` are the only ways to create an edge and both enforce `from_ns == to_ns`. If any future code path created an edge without going through `apply_event_ns` (which Rust's `pub(crate)` visibility on `NodePool`/`EdgePool` currently prevents from outside the kernel crate, §11), traversal would silently cross namespaces with no runtime check of its own to catch it.

**No CRITICAL or HIGH risks were found.** The graph's core mechanics — cascade delete, adjacency list integrity, snapshot round-trip, event-sourcing purity (no bypass path) — are solid and, for the most part, directly tested.

---

## 19. G0 Invariants

| # | Invariant | Status |
|---|---|---|
| 1 | Canonical graph state is reconstructible from the event log. | **PROVEN** — production recovery path (`valori-storage::event_replay::replay_events` → `Engine::try_recover`) is namespace-aware and uses the same `apply_event_ns` as live writes (§13b); construction-determinism for graph-containing state is directly tested (`two_identical_builds_produce_identical_snapshot_bytes`). |
| 2 | Snapshot restore reproduces equivalent graph state. | **PROVEN** for hash + aggregate counts (`roundtrip_preserves_state_hash`) and **PROVEN** field-level for the reverse index specifically (`test_snapshot_preserves_reverse_index`) — but note the hash itself does not cover every field (§16), so "equivalent" here means "equivalent in what the hash covers, plus the specific fields the dedicated test checks," not "every byte independently verified by the hash." |
| 3 | Derived graph indexes (GraphRAG results, community detection) never become the source of truth. | **PROVEN** — no `KernelEvent` variant exists for either; both are recomputed per-request from `nodes`/`edges` with no persisted cache that could drift and be mistaken for canonical (§4). |
| 4 | Every graph mutation goes through a canonical `KernelEvent`. | **PROVEN** — `nodes`/`edges` are `pub(crate)` on `KernelState`; the only mutation entry point is `apply_event_ns`; `create_node()`/`create_edge()` convenience methods internally construct and apply events rather than mutating pools directly (§11). |
| 5 | Graph state remains deterministic across replay. | **PARTIALLY PROVEN** — no randomness/timestamps/floats exist in graph structures (§15, all SAFE), and construction-determinism is tested; but no test is framed as an explicit "apply(S0,E) vs replay(E), assert equal" comparison using graph mutation events specifically (§13b caveat), and the concurrency-safety of graph writes under real multi-writer conditions was not traced in this pass. |
| 6 | Graph/vector identity boundaries remain explicit. | **PROVEN** — `NodeId`/`RecordId` are separate id spaces linked only by an optional field; nodes can exist without records and vice versa; deletion in either direction is one-way by default at the kernel level (record deletion does not cascade to nodes at the kernel layer — only at the `Engine` convenience layer) (§10). |
| 7 | Edges cannot cross namespace boundaries. | **PROVEN** at the enforcement point (`apply_event_ns`'s `CreateEdge`/`AutoCreateEdge` arms, §8/§9) — but **NOT PROVEN by a dedicated test** (no test attempts a cross-namespace edge and asserts rejection). |
| 8 | The canonical state hash fully represents canonical graph state. | **NOT PROVEN — actively FALSE as measured.** `namespace_id`, namespace list pointers, `first_in_edge`, `next_in`, and `meta` are canonical (persisted, event-sourced, snapshotted) but excluded from `hash_state_blake3` (§13, §16, Risk R2). This is the most important corrected assumption from the working hypothesis: **"canonical" and "hashed" are not currently the same set of fields.** |

---

## 20. Recommended G0 Follow-ups

*(Documented only, per instructions — nothing here has been implemented.)*

1. **Decide, explicitly, whether the state hash should be widened** to cover `namespace_id`, `first_in_edge`/`next_in`, and `meta`, or whether the current scope is an intentional design choice (e.g., "the hash only needs to prove reachable graph structure, not every bookkeeping pointer"). Either answer is legitimate, but it should be a documented decision, not a silent gap — this determines what Valori's proof/receipt system can actually promise about graph correctness across replicas.
2. **Resolve or clearly deprecate `valori_kernel::replay_events::replay_events`** so a future caller cannot mistake it for the namespace-aware production implementation in `valori-storage`. At minimum, its doc comment should say "not namespace-aware; do not use for multi-tenant recovery" until a decision is made about its fate.
3. **Add a dedicated cross-namespace-edge-rejection test** and a **duplicate-edge-is-allowed test** — both behaviors are real and code-verified but currently unproven by the test suite (§17).
4. **Add an explicit "apply(S0,E) vs replay(E)" test that includes `CreateNode`/`CreateEdge`/`DeleteNode`/`DeleteEdge` events**, not just `InsertRecord`/`SoftDeleteRecord` — closing the gap noted in §13b/§19 invariant 5.
5. **Consider a graph-equivalent of the mandatory `dr_disaster_recovery` test** (10k nodes/edges, drop engine, restore from object store, verify graph structure + hash) — the existing DR test is vector-only per CLAUDE.md.

None of the above have been started. This document is the freeze point; G1 begins only after these are explicitly triaged (accepted, scheduled, or consciously deferred) by the team.

---

## G0 STATUS

- **Canonical graph state**: `KernelState.nodes` (`NodePool`) + `KernelState.edges` (`EdgePool`), owned directly, `pub(crate)`-only mutation via `apply_event_ns`. Confirmed.
- **Event sourced**: Yes — `CreateNode`/`AutoCreateNode`/`CreateEdge`/`AutoCreateEdge`/`DeleteNode`/`DeleteEdge` are `KernelEvent` variants; no bypass path found.
- **Snapshot persisted**: Yes — full node/edge/adjacency/namespace fidelity in the V7 kernel snapshot format; decode-side hardening tested.
- **Replay deterministic**: Yes in practice (no randomness/float/timestamp in graph fields; production path is namespace-aware); **not proven by an explicitly-framed replay-vs-apply test for graph events specifically**.
- **Traversal deterministic**: Yes by code construction (BFS over deterministic linked lists); **not proven by a nontrivial-case test**.
- **Namespace isolation**: Enforced at the canonical mutation layer (`apply_event_ns`), not just the API — edges cannot cross namespaces, nodes cannot reference cross-namespace records. **Not covered by the state hash** — a real gap.
- **Vector/graph boundary**: Clean and explicit — separate id spaces, optional linkage, node-existence does not imply record-existence or vice versa; record→node cascade-delete is an `Engine`-layer convenience, not a kernel invariant.
- **Major architectural risk**: The state-hash coverage gap (R2) and the dormant namespace-blind `replay_events` name collision (R1) — both MEDIUM, neither CRITICAL.
- **Missing invariant test**: An explicit `apply(S0,E) == replay(E)` test using graph mutation events; a cross-namespace-edge-rejection test; a graph-equivalent disaster-recovery test.
- **Ready for G1**: **NO** — not because the graph is broken, but because G0's own charter is to freeze the boundary first, and two real findings (R1, R2) plus the missing invariant tests in §20 should be explicitly triaged by the team before building new graph features on top of this foundation.
