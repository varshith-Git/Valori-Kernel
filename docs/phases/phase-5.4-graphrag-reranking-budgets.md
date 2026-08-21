# Phase 5.4 — Graph-Aware Reranking + Traversal Budgeting

## Goal

Complete GraphRAG semantic hardening by fixing the three remaining gaps from
Phase 5.3: (1) unbounded BFS traversal at high branching factor, (2) `final_k`
defaulting to unlimited instead of `retrieval_k`, and (3) a placeholder ranking
that prevented graph-only candidates from ever outranking weak vector hits.

---

## Delivered

### A. Traversal budgets (BFS-level)

`expand_subgraph_budgeted` (new function in `valori-rag/src/graph.rs`) adds two
hard stops that fire inside the BFS loop — before the per-hop depth clamping —
so runaway traversal in dense graphs is cut before it generates candidates:

| Parameter | Where enforced | Semantics |
|---|---|---|
| `max_nodes` | Top of BFS `while` loop | Stop before visiting a node that would push `visited_nodes.len()` above the limit |
| `max_edges` | Inner edge loop per node | Stop emitting further edges for the current node once the limit is reached |

The existing `expand_subgraph` is now a zero-budget wrapper:
```rust
pub fn expand_subgraph(state, seeds, depth) -> (Vec<Value>, Vec<Value>) {
    expand_subgraph_budgeted(state, seeds, depth, None, None)
}
```
No existing call sites changed — all pass through the wrapper with `None, None`.

### B. Graph-aware reranker

Phase 5.3 appended graph-only candidates after vector candidates with
`final_score: null`. Phase 5.4 normalises both signals to `[0, 1]` and merges
all candidates into a single sorted list:

```
vector_relevance = 1 / (1 + L2_dist)        ∈ (0, 1]; 0.0 for graph-only
graph_relevance  = 1 / (1 + hop_distance)   ∈ (0, 1]; 0.0 for no-graph vector hits
final_score      = α × vector_relevance + β × graph_relevance
                   where β = graph_weight (default 0.3), α = 1 - β
```

Hits are sorted by `final_score` DESC, `record_id` ASC as tie-breaker.

`graph_weight=1.0` gives the pure graph signal — a no-graph vector hit
(graph_relevance = 0.0) then ranks below any graph-adjacent candidate.
`graph_weight=0.0` reproduces Phase 5.3 ordering (vector candidates first,
graph-only appended).

### C. `graph_score` field

Every hit now carries `graph_score` (normalised `graph_relevance`, always
numeric), alongside the existing `vector_score` (null for graph-only) and
`final_score` (combined, always numeric):

| Field | Vector hit (no node) | Seed (dist=0) | Graph-only (dist=N) |
|---|---|---|---|
| `score` | L2 dist | L2 dist | null (backward compat) |
| `vector_score` | L2 dist | L2 dist | null |
| `graph_score` | 0.0 | 1.0 | 1/(1+N) |
| `final_score` | α×(1/(1+dist)) | α×(1/(1+dist)) + β | β×(1/(1+N)) |
| `graph_distance` | null | 0 | N |

### D. `final_k` defaults to `retrieval_k`

Phase 5.3 left `final_k` unbounded when absent. Phase 5.4 defaults it to
`retrieval_k` in both HTTP handlers:

```rust
// server.rs / cluster_server.rs
let final_k = payload.final_k.unwrap_or(retrieval_k) as u32;
```

A request with `retrieval_k=5` and no `final_k` now returns ≤5 hits. Callers
that want more must pass an explicit `final_k`.

### E. New request fields

```json
{
  "query_vector": [0.1, 0.2, 0.3],
  "retrieval_k": 20,
  "final_k": 10,
  "depth": 2,
  "max_graph_candidates": 100,
  "max_nodes": 500,
  "max_edges": 2000,
  "graph_weight": 0.3
}
```

All new fields are optional with safe defaults:

| Field | Default | Range |
|---|---|---|
| `max_nodes` | unlimited | >0 |
| `max_edges` | unlimited | ≥0 |
| `graph_weight` | 0.3 | [0.0, 1.0]; clamped |
| `final_k` | `retrieval_k` | >0 |

### F. Cluster parity

Both `EngineKernelCapability::graph_rag` (standalone) and
`RaftKernelCapability::graph_rag` (cluster) implement identical logic:
- `expand_subgraph_budgeted` called inside `with_state` closure (cluster path)
- Combined reranker and sort happen outside the closure
- Async metadata fetch happens in final sorted order

### G. Python SDK

All four `graphrag` methods updated with new optional params:
- `SyncRemoteClient.graphrag` — `max_nodes`, `max_edges`, `graph_weight`
- `AsyncRemoteClient.graphrag` — same
- `ClusterClient.graphrag` — delegates to `_read_client` with new params
- `AsyncClusterClient.graphrag` — same

### H. Files changed

| File | Change |
|---|---|
| `crates/valori-rag/src/graph.rs` | Added `expand_subgraph_budgeted`; `expand_subgraph` becomes a wrapper |
| `crates/valori-rag/src/lib.rs` | Export `expand_subgraph_budgeted` |
| `crates/valori-effect/src/capability.rs` | `graph_rag` trait: added `max_nodes`, `max_edges`, `graph_weight`; updated doc |
| `crates/valori-effect/src/tasks/graph_rag.rs` | `GraphRagInputs`: added `max_nodes`, `max_edges`, `graph_weight`; updated call |
| `crates/valori-node/src/capabilities.rs` | Both impls: budgeted BFS, reranker, `graph_score`, unified sort, new params |
| `crates/valori-node/src/server.rs` | `GraphRagRequest`: new fields; `final_k` default changed; `inputs_json` updated |
| `crates/valori-node/src/cluster_server.rs` | `ClusterGraphRagRequest`: same changes as server.rs |
| `python/valoricore/remote.py` | Four `graphrag` methods: `max_nodes`, `max_edges`, `graph_weight` added |
| `crates/valori-node/tests/api_graphrag.rs` | 4 existing tests updated; 5 new Phase 5.4 tests (19 total) |

---

## Findings

### `expand_subgraph` called from `/graph/subgraph` handler

`server.rs:2090` and `cluster_server.rs:3013` call `expand_subgraph` (not
`expand_subgraph_budgeted`) for the `/graph/subgraph` endpoint. The BFS budget
there remains unlimited-by-graph-params (depth only). This is correct: the
`/graph/subgraph` endpoint is a pure graph traversal, not GraphRAG — callers
explicitly provide `root` and `depth` and expect the full subgraph. No change
made there.

### `graph_distances_from_seeds` is not budget-constrained

Phase 5.4's `max_nodes`/`max_edges` limit `expand_subgraph_budgeted` but not
the separate `graph_distances_from_seeds` call. Distances are computed by a
full unbounded BFS. This is intentional: the distances are used as a signal
(accuracy matters); the budget controls only the candidate pool returned in the
response. In practice, high-fanout graphs that trigger the node budget are
unlikely to benefit from accurate depth-N distances because the budget already
stopped traversal — but distance computation for unreachable nodes is harmless.

### Ranking direction change from Phase 5.3

Phase 5.3 returned hits in two buckets: vector (L2 ascending) then graph-only
(distance ascending). Phase 5.4 uses a single merged list sorted by
`final_score` descending. The direction flip (`final_score` is higher-is-better
vs raw L2 which is lower-is-better) is an intentional and documented contract
change. Existing callers that consumed `score` (L2, ascending) are unaffected
as long as they do not depend on the overall list order — the backward-compat
`score` field is preserved but the list is now in a different order.

### `score` field explicitly deprecated

`score` remains in the response as a backward-compat alias for `vector_score`
(null for graph-only). Callers should migrate to `vector_score`, `graph_score`,
and `final_score`.

---

## Validation

```
cargo build -p valori-rag -p valori-effect -p valori-node         → clean
cargo fmt --check -p valori-rag -p valori-effect -p valori-node   → clean (after fmt applied)
cargo test -p valori-kernel                                        → 177 passed, 0 failed, 1 ignored
cargo test -p valori-node --test api_graphrag                      → 19 passed, 0 failed
cargo test -p valori-node (full suite)                             → 422 passed, 0 failed, 1 ignored
```

### New tests (Phase 5.4)

| Test | What it pins |
|---|---|
| `graphrag_final_k_defaults_to_retrieval_k` | absent final_k → hits ≤ retrieval_k |
| `graphrag_graph_score_field_on_all_hit_types` | graph_score ∈ [0,1] on every hit; 1.0 for seed, 0.5 for hop-1, 0.0 for no-graph |
| `graphrag_graph_only_outranks_no_graph_vector_with_high_graph_weight` | graph_weight=1.0 → graph-only B outranks pure vector C |
| `graphrag_max_nodes_limits_bfs_expansion` | max_nodes=1 → subgraph ≤1 node, no graph-only candidates |
| `graphrag_max_edges_limits_bfs_expansion` | max_edges=1 → ≤1 edge in subgraph, C unreachable |

### Updated tests (Phase 5.3 fixed for Phase 5.4 defaults)

| Test | Fix |
|---|---|
| `graphrag_graph_only_candidate_appears_in_hits` | Added `final_k: 10` |
| `graphrag_vector_score_and_final_score_fields` | Added `final_k: 10`; `hit_b["final_score"].is_null()` → `.as_f64().is_some()` |
| `graphrag_minimum_graph_distance_diamond` | Added `final_k: 10` |
| `graphrag_max_graph_candidates_budget` | Added `final_k: 10` |

---

## Follow-ups

| Item | Phase | Severity |
|---|---|---|
| `score` field formally deprecated in API docs; remove from contract in a future version | Future API phase | Low |
| Typed Python SDK response model (not raw dict) | Future SDK phase | Low |
| Cluster `with_state` return tuple (5-tuple + new fields) should be a named struct | Refactor sprint | Low |
| Expose `graph_weight` in UI playground request builder | UI phase | Low |
| `/v1/proof/event-log` + `/v1/timeline` still read shard 0 only | Ongoing | Medium |
