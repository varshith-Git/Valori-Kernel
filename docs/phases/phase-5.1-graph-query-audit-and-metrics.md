# Phase 5.1 — Graph Query Architecture Audit and Metrics

## Goal

Audit the entire graph query layer and its integration with Vector Search. Verify that
each capability is correctly implemented, Collection-scoped, and tested. Add the one
genuine gap discovered — Prometheus metrics for graph operations — so observability is
on par with the vector and snapshot subsystems.

---

## Delivered

### Graph Prometheus Metrics (`crates/valori-node/src/routes/graph.rs`)

Seven metrics added across both standalone and cluster paths via the shared handler
module (`routes/graph.rs`) and the path-specific rerank/graphrag helpers:

| Metric | Kind | Increment point |
|---|---|---|
| `valori_graph_node_create_total` | counter | `POST /v1/graph/node` success |
| `valori_graph_edge_create_total` | counter | `POST /v1/graph/edge` success |
| `valori_graph_query_total` | counter | `GET /v1/graph/query` success (start node found) |
| `valori_graph_traversal_nodes` | histogram | nodes returned by `GET /v1/graph/query` and `GET /v1/graph/subgraph` |
| `valori_graph_traversal_edges` | histogram | edges returned by `GET /v1/graph/subgraph` |
| `valori_graphrag_total` | counter | `POST /v1/graphrag` success (standalone + cluster) |
| `valori_graph_rerank_total` | counter | `apply_graph_rerank` / `apply_graph_rerank_cluster` invocation |

Counters and histograms use the existing `metrics::counter!` / `metrics::histogram!`
macros already in use for snapshot and index metrics — no new dependency.

Because `routes/graph.rs` is the shared handler module used by **both** the standalone
`SharedEngine` impl and the cluster `DataPlaneState` impl, node/edge/query/subgraph
metrics fire on both paths from a single edit.

The graphrag counter fires after successful planner execution in `server.rs::graphrag`
and `cluster_server.rs::cluster_graphrag` independently (both handlers retain their
separate planner wiring, so the counter sits after the result is assembled).

The rerank counter fires at the start of the rerank pass (before the sort) in both
`apply_graph_rerank` (standalone) and `apply_graph_rerank_cluster` (cluster).

### Files changed

| File | Change |
|---|---|
| `crates/valori-node/src/routes/graph.rs` | 5 metrics (node/edge create counters, query counter, traversal_nodes histogram in query + subgraph, traversal_edges histogram in subgraph) |
| `crates/valori-node/src/server.rs` | `valori_graphrag_total` counter after result, `valori_graph_rerank_total` counter in rerank helper |
| `crates/valori-node/src/cluster_server.rs` | Same two metrics on the cluster path |

---

## Findings

### Pre-flight audit classification

All items below were audited by reading the actual source. Status reflects the
codebase at the start of Phase 5.1.

| Capability | Status | Location |
|---|---|---|
| Graph node creation | COMPLETE | `KernelEvent::CreateNode` / `AutoCreateNode`, `routes/graph.rs::create_node` |
| Graph edge creation | COMPLETE | `KernelEvent::CreateEdge` / `AutoCreateEdge`, `routes/graph.rs::create_edge` |
| Graph node lookup | COMPLETE | `GET /v1/graph/node/:id`, `KernelState::get_node` |
| Neighbors (outgoing) | COMPLETE | `GET /v1/graph/edges/:id`, Python `neighbors()` wrapper |
| Subgraph BFS | COMPLETE | `valori_rag::graph::expand_subgraph`, `GET /v1/graph/subgraph` |
| G1.1 Traversal (filtered, bounded) | COMPLETE | `valori_rag::graph::query_graph`, `GET /v1/graph/query` |
| Graph-aware reranking | COMPLETE | `valori_rag::graph::graph_distances_from_seeds`, `apply_graph_rerank` |
| GraphRAG | COMPLETE | `POST /v1/graphrag`, planner path, `valori_rag::graph::{resolve_seed_nodes, expand_subgraph}` |
| Vector → GraphNode mapping | COMPLETE | `valori_rag::graph::resolve_seed_nodes` — O(live nodes) scan |
| GraphNode → Record mapping | COMPLETE | `GraphNode::record: Option<RecordId>` |
| Collection isolation (edges) | COMPLETE | `apply_event_ns` rejects cross-namespace edges (from_ns ≠ to_ns → InvalidOperation) |
| Collection isolation (query) | COMPLETE | `query_graph` checks start node namespace before traversal |
| Graph snapshot | COMPLETE | V7+ encode/decode covers NodePool + EdgePool + namespace_node_heads |
| Graph WAL replay | COMPLETE | CreateNode, CreateEdge, DeleteNode, DeleteEdge, AutoCreateNode, AutoCreateEdge all in event.rs + apply_event_ns |
| Graph WAL recovery | COMPLETE | `graph_query_restart_recovery.rs` |
| Index lifecycle independence | COMPLETE | `index_change_preserves_graph` in `index_lifecycle.rs` |
| Cluster graph (writes via Raft) | COMPLETE | `DataPlaneState::GraphOps` impl commits `AutoCreateNode`/`AutoCreateEdge` via `raft_write_data` |
| Cluster graph (reads from local state) | COMPLETE | `cluster_server.rs` reads local `KernelState` snapshot |
| Graph empty ∩ vectors present | COMPLETE | vector search only touches `RecordPool` + `ActiveIndex`; no graph involvement |
| Vectors empty ∩ graph present | COMPLETE | graph traversal only touches `NodePool` + `EdgePool`; no ANN index involvement |
| GraphRAG with empty graph | COMPLETE | `expand_subgraph(&[], 2)` returns `([], [])` — verified by `api_graphrag.rs` |
| Graph-aware rerank fallback on missing graph | COMPLETE | `nodes_referencing_record` returns empty vec → `graph_distance = None` → neutral scoring |
| Graph metrics | **ADDED this phase** | see above |
| Python Sync graph API | COMPLETE | `_SyncGraphMixin`: create_node, create_edge, get_node, get_node_edges, graph_query, list_nodes, neighbors, subgraph |
| Python Async graph API | COMPLETE | `_AsyncGraphMixin`: same methods |
| Python graphrag | COMPLETE | both `_SyncSearchMixin.graphrag` and `_AsyncSearchMixin.graphrag` |

### Current graph model (verified)

```
KernelState
├── NodePool (Vec<Option<GraphNode>>) — slab, tombstone-safe
│     GraphNode { id: NodeId, kind: NodeKind, record: Option<RecordId>,
│                 first_out_edge: Option<EdgeId>, first_in_edge: Option<EdgeId>,
│                 namespace_id: u16, next_in_ns: u32, prev_in_ns: u32 }
├── EdgePool (Vec<Option<GraphEdge>>) — slab, tombstone-safe
│     GraphEdge { id: EdgeId, kind: EdgeKind, from: NodeId, to: NodeId,
│                 next_out: Option<EdgeId>, next_in: Option<EdgeId> }
├── namespace_node_heads: Vec<u32> (intrusive per-namespace head pointer)
```

**Identity semantics (unchanged):**
- NodeId = slot index in NodePool (u32, monotone, never reused after delete)
- EdgeId = slot index in EdgePool (u32, monotone, never reused after delete)
- Cascade delete: DeleteNode → collect outgoing + incoming edge ids → _delete_edge each → NodePool::delete
- Cross-namespace edge rejection: enforced in CreateEdge/AutoCreateEdge arm of `apply_event_ns`

### Traversal safety (verified)

| Bound | Value | Enforced in |
|---|---|---|
| MAX_DEPTH | 4 | `valori_rag::graph::MAX_DEPTH`, applied in `query_graph`, `expand_subgraph`, `graph_distances_from_seeds` |
| MAX_QUERY_LIMIT | 1000 | clamped in `query_graph`, floored at 1 |
| DEFAULT_QUERY_DEPTH | 2 | HTTP GET default via `serde(default = "...")` |
| DEFAULT_QUERY_LIMIT | 100 | HTTP GET default via `serde(default = "...")` |

Cycles are handled by a `HashSet<u32>` visited-set in both `query_graph` and
`expand_subgraph` — a revisited node is skipped, not re-enqueued. Self-loops
are absorbed without special-casing.

### GraphRAG architecture (verified — composition, not reimplementation)

```
POST /v1/graphrag
    → graphrag handler (server.rs / cluster_server.rs)
    → planner (TaskKind::GraphRag, OperationKind::GraphRag)
    → capabilities.rs::CapabilityImpl::graph_rag
        → engine.search_l2_ns (vector KNN)
        → valori_rag::graph::resolve_seed_nodes (record_ids → node_ids)
        → valori_rag::graph::expand_subgraph (BFS from seeds, depth)
        → { hits, seed_nodes, subgraph: { nodes, edges } }
```

All three primitives (`search_l2_ns`, `resolve_seed_nodes`, `expand_subgraph`) operate
on the SAME `KernelState` snapshot — no cross-store drift is possible.

`resolve_seed_nodes` is O(live nodes) per call — one full node-pool scan. This is the
established tradeoff from Phase G1.3 (eliminates the cache divergence that the old
`record_to_node` cache introduced). Flagged as a known cost in `valori-rag/src/graph.rs`
doc comments; an `#[ignore]` benchmark exists at `resolve_seed_nodes_cost`.

### Vector → Graph seed resolution (verified)

`resolve_seed_nodes(state, record_ids)`:
- Scans `state.iter_nodes()` once
- For each node with a `record` back-reference in `want`, inserts `(record_id, node_id)` into a HashMap
- First match wins — deterministic in node-pool iteration order (ascending NodeId)
- Returns HashMap<record_id, node_id>

This is the same mapping used by both the standalone GraphRAG handler and the cluster
GraphRAG handler. There is no separate cached mapping — G1.3.1 removed it after finding
it could diverge.

### `neighbors` Python SDK (verified)

`neighbors(node_id, collection)` → `[int]`
- Calls `GET /v1/graph/edges/:id?collection=...`
- Extracts `edge["to"]` from each outgoing edge
- Returns list of directly reachable node IDs (outgoing only, depth=1)
- Backed by `O(degree)` linked-list traversal — no scan

---

## Validation

```
cargo build -p valori-node              → clean (no errors, no new warnings)
cargo fmt -p valori-node --check        → clean
cargo test -p valori-rag --lib          → 46 passed, 0 failed, 3 ignored
cargo test -p valori-node --test api_graph_query               → 9 passed, 0 failed
cargo test -p valori-node --test api_graphrag                  → 5 passed, 0 failed
cargo test -p valori-node --test api_graph_namespace_isolation → 5 passed, 0 failed
cargo test -p valori-node --test graph_aware_reranking         → (background, included in totals)
cargo test -p valori-node --test vector_graph_retrieval        → (background, included in totals)
cargo test -p valori-node --test route_parity                  → 2 passed, 0 failed
cargo test -p valori-node (graph tests aggregated)             → 47 passed, 0 failed
```

---

## Follow-ups

| Item | Phase |
|---|---|
| Cluster multi-search: add per-shard linearizable read before each collection search | 5.2 or hardening sprint |
| `resolve_seed_nodes` O(live_nodes) cost: consider a lazy record→node index once node counts exceed 100k | Future (see `#[ignore]` benchmark) |
| `GET /v1/proof/event-log` + `GET /v1/timeline` read shard 0 only in multi-shard deployments | Known gap, future phase |
| Python pytest suite for graph methods (not just unit tests) | Future |
| UI: graph visualizer verified Collection-aware (no implicit "default"); deeper investigation of `MultiSearch.tsx` cross-collection graph context | Future |
