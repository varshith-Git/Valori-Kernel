# Phase I6 — Community Layer

## Goal

Add deterministic community detection to Valori's graph, making it possible to
answer "what are the themes across all documents?" — the global-sensemaking
capability that the GraphRAG paper (Microsoft, 2024) shows beats plain vector
search for comprehensiveness. Unlike GraphRAG's LLM-generated community
summaries, Valori's approach is deterministic, LLM-free, and BLAKE3-auditable.

## Delivered

### New file

| File | What it adds |
|---|---|
| `crates/valori-node/src/community.rs` | `label_propagation()`, `build_community_store()`, `rank_communities()`, `extract_entities_via_llm()`; all HTTP request/response types for the three endpoints |

### Modified files

| File | Change |
|---|---|
| `crates/valori-kernel/src/state/kernel.rs` | Added `incoming_edges()` public method — Label Propagation needs both directions |
| `crates/valori-node/src/lib.rs` | `pub mod community;` |
| `crates/valori-node/src/engine.rs` | Added `community_store: Option<CommunityStore>` to `Engine` |
| `crates/valori-node/src/server.rs` | Routes + handlers: `community_detect`, `community_search`, `extract_entities` (standalone path) |
| `crates/valori-node/src/cluster_server.rs` | `community_store` on `DataPlaneState`; routes + handlers: `cluster_community_detect`, `cluster_community_search`, `cluster_extract_entities` (cluster path) |
| `python/valoricore/remote.py` | `community_detect()`, `community_search()`, `extract_entities()` on `SyncRemoteClient` + `AsyncRemoteClient` |

### New HTTP endpoints (standalone + cluster)

| Method | Path | Description |
|---|---|---|
| `POST` | `/v1/community/detect` | Label Propagation → community_id per node → centroid per community → BLAKE3 receipt |
| `POST` | `/v1/community/search` | Cosine similarity vs community centroids → top-k communities ranked best-first |
| `POST` | `/v1/ingest/extract-entities` | LLM entity extraction → embed descriptions → insert Concept nodes + Relation edges |

## Algorithm

### Label Propagation

- Iterates all nodes in sorted order (deterministic).
- Collects labels from both outgoing (`outgoing_edges`) and incoming (`incoming_edges`) neighbours.
- Picks the **most frequent label**, breaking ties by **minimum label** → reproducible from the same graph.
- Converges in typically < 10 iterations (default max: 20).
- Complexity: O(n + e) per iteration.

### Community centroid

- For each community, average the `FxpVector` values of all member records (Q16.16 → f64 → f32).
- Stored in `CommunityStore.centroids: HashMap<u32, Vec<f32>>`.

### BLAKE3 receipt

- Sorts `(node_id, community_id)` pairs into a `BTreeMap` for reproducibility.
- Concatenates `node_id.to_le_bytes()` + `cid.to_le_bytes()` into the BLAKE3 hasher.
- Hex-encodes the digest → `CommunityStore.receipt`.
- This receipt proves community structure at a point in time without requiring an LLM.

## Findings

- `incoming_edges()` was missing from `KernelState` — added it via `InEdgeIterator` (already existed in `graph/adjacency.rs`).
- `KernelEvent::AutoInsertRecord` and `AutoCreateNode` do not carry `namespace_id` — namespace context is carried by the `ValoriStateMachine::apply()` path. The cluster entity-extraction handler uses `AutoInsertRecord` / `AutoCreateNode` / `AutoCreateEdge` variants.
- `ClientRequest` uses `request_id: Option<[u8; 16]>`, not `Uuid` — fixed.
- The `community_store` on `DataPlaneState` cannot be written inside a `with_state` closure (read-only borrow) — detect handler calls `with_state` then writes to a separate `Arc<RwLock<Option<CommunityStore>>>`.

## Validation

```
cargo test -p valori-kernel -p valori-node
```

All 0 failures across all test suites. Selected counts:
- valori-kernel: 54 passed (format), 11 passed (events), 6 passed (snapshot), ...
- valori-node: 4 passed (recovery), 5 passed (search), 7 passed (cluster), ...

Total: 0 failures.

## Follow-ups

| Item | Future phase |
|---|---|
| Per-namespace community detection in cluster path (namespace registry not replicated yet) | Phase I7 or cluster-metadata phase |
| Persist `CommunityStore` across node restart (snapshot field) | Phase I7 |
| Community-level summarisation via LLM (optional, opt-in) | Phase I7 |
| `extract_entities` → cluster path uses ID pre-fetch heuristic which can race under concurrent inserts; add idempotency token | Phase I7 |
