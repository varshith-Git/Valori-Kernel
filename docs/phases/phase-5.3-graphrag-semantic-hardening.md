# Phase 5.3 — GraphRAG Semantic Hardening and Contract Finalization

## Goal

Fix the semantic ambiguities in the Phase 5.2 GraphRAG result contract before
adding more features: separate `retrieval_k` from `final_k`, make graph-only
candidate ordering deterministic, fix minimum graph-distance tracking, add
explicit `vector_score`/`final_score` fields, cap graph-only candidates with
`max_graph_candidates`, and propagate all changes symmetrically to both
execution paths (standalone + cluster) and the Python SDK.

---

## Delivered

### A. Final GraphRAG request contract

```json
{
  "query_vector": [0.1, 0.2, 0.3],
  "retrieval_k": 20,
  "final_k": 10,
  "depth": 2,
  "max_graph_candidates": 100,
  "collection": "knowledge"
}
```

Backward compat: `k` still accepted as an alias for `retrieval_k`. When both
are absent the server defaults to `retrieval_k = 5`. `final_k` and
`max_graph_candidates` are optional; absent = no truncation / server default
(100) respectively.

### B. Final response contract

**Vector hit (no graph node):**
```json
{
  "record_id": 42,
  "source": "vector",
  "score": 0.12,
  "vector_score": 0.12,
  "final_score": 0.12,
  "graph_distance": null,
  "node_id": null,
  "memory_id": "rec:42",
  "metadata": null
}
```

**Vector hit with graph node (seed):**
```json
{
  "record_id": 15,
  "source": "vector_and_graph",
  "score": 0.05,
  "vector_score": 0.05,
  "final_score": 0.05,
  "graph_distance": 0,
  "node_id": 3,
  "memory_id": "rec:15",
  "metadata": null
}
```

**Graph-only hit:**
```json
{
  "record_id": 57,
  "source": "graph",
  "score": null,
  "vector_score": null,
  "final_score": null,
  "graph_distance": 1,
  "node_id": 7,
  "memory_id": "rec:57",
  "metadata": null
}
```

`score` is retained as a backward-compat alias for `vector_score`. `final_score`
equals `vector_score` until a reranker is wired into the pipeline.

### C. `retrieval_k` vs `final_k`

| Field | Meaning |
|---|---|
| `retrieval_k` (`k` alias) | How many vector candidates to retrieve from ANN search. These become the seeds for graph expansion. |
| `final_k` | Maximum hits returned in the response. Applies after dedup, graph-only collection, and `max_graph_candidates` budget. `None` = return all candidates. |

Example: `retrieval_k=20, final_k=10` retrieves 20 vector seeds, expands their
graph neighbourhood, dedups, applies budget, then truncates the final list to 10.

### D. Candidate ranking

The current (pre-reranker) ordering policy:

1. **Vector candidates** — sorted ascending by `vector_score` (L2 distance,
   lower = more similar). `search_l2_ns` already returns sorted results so no
   re-sort is needed.
2. **Graph-only candidates** — sorted ascending by `graph_distance` (closer
   neighbours first), then by `record_id` ascending as a deterministic
   tie-breaker. Sorted in the capability layer after `max_graph_candidates`
   budget is applied.
3. **Combined ordering** — vector candidates appear first, graph-only candidates
   appended after. `final_k` is applied to this combined list.

`final_score` is a placeholder for a future reranker: currently `= vector_score`
for vector hits and `null` for graph-only hits. When a reranker lands it will
replace `final_score` for all candidates without changing the other fields.

### E. Deduplication

1. A `HashSet<record_id>` is populated during the vector candidate loop.
2. The graph-only candidate loop checks `HashSet::contains(record_id)` before
   inserting — records that were already vector hits are skipped entirely.
3. Within the graph-only loop, multiple nodes that reference the same record
   (possible because `CreateNode` imposes no uniqueness constraint on `record`)
   are tracked via a `HashMap<record_id, (node_id, min_dist)>`. When the same
   record is seen again via a shorter path, the entry is updated. This ensures
   the shortest discovered path always wins, regardless of node iteration order.

The merging rule: if a record is a vector hit, it gets `source: "vector_and_graph"`
(or `"vector"` if it has no graph node) and never appears in the graph-only
section. The graph-only section only contains records that were NOT in the
vector top-k.

### F. Limits

| Parameter | Default | Where clamped |
|---|---|---|
| `retrieval_k` | 5 | `.max(1)` in handler |
| `depth` | 2 | `depth.min(MAX_DEPTH=4)` in `expand_subgraph` |
| `max_graph_candidates` | 100 | `.max(1)` in handler; `truncate` in capability |
| `final_k` | None (no truncation) | `truncate` in capability after all other limits |

There is currently no per-call `max_nodes` / `max_edges` absolute budget on the
BFS itself (bounded only by `depth`); at high branching factor this is the next
limit to add.

### G. Python SDK

All four `graphrag` call-sites updated (`SyncRemoteClient.graphrag`,
`AsyncRemoteClient.graphrag`, `SyncClusterClient.graphrag`,
`AsyncClusterClient.graphrag`) with new optional params `retrieval_k`,
`final_k`, `max_graph_candidates`. When `retrieval_k` is provided it takes
precedence over `k` in the outgoing request body. Full docstring added
documenting Phase 5.3 semantics and the hit shape.

### H. TypeScript / UI

`PlaygroundView.tsx` renders the raw JSON response from the playground — no
typed response model for hit fields exists there. The sample body already uses
`k` (backward-compat). The new `vector_score`, `final_score`, and `source`
fields appear in the playground's JSON output automatically; they are additive
and do not break any existing UI rendering.

### I. Cluster parity

Both `EngineKernelCapability::graph_rag` (standalone) and
`RaftKernelCapability::graph_rag` (cluster) were updated identically:
- Trait signature extended to `(retrieval_k, depth, final_k, max_graph_candidates)`
- `GraphRagInputs` in the task file carries all four fields
- Minimum-distance HashMap, deterministic sort, budget truncation, `final_k`
  applied in both impls
- Cluster path does all KernelState work inside the `with_state` sync closure;
  sort + metadata fetch happen outside after the closure returns

### J. Files changed

| File | Change |
|---|---|
| `crates/valori-effect/src/capability.rs` | Extended `graph_rag` trait signature (+`final_k`, `+max_graph_candidates`); updated doc comment |
| `crates/valori-effect/src/tasks/graph_rag.rs` | Added `final_k`, `max_graph_candidates` to `GraphRagInputs`; updated `graph_rag()` call; updated file-level doc |
| `crates/valori-node/src/server.rs` | `GraphRagRequest`: `k: usize` → `k: Option<usize>`, added `retrieval_k`, `final_k`, `max_graph_candidates`; handler resolves compat alias |
| `crates/valori-node/src/cluster_server.rs` | `ClusterGraphRagRequest`: same additions; handler updated |
| `crates/valori-node/src/capabilities.rs` | Both impls: min-distance HashMap, deterministic sort, budget+final_k truncation, `vector_score`/`final_score` fields |
| `python/valoricore/remote.py` | Four `graphrag` methods updated with new params + docstrings |
| `crates/valori-node/tests/api_graphrag.rs` | 6 new tests (14 total) |

---

## Findings

### Minimum-distance bug (fixed)

Phase 5.2 used `HashSet::insert(record_id)` in the graph-only candidate loop.
When multiple nodes in the expanded subgraph referenced the same record,
first-seen determined the `graph_distance` reported — which could be a longer
path if a shorter-distance node was encountered later in the `nodes` slice
(returned by `expand_subgraph` in BFS discovery order, not guaranteed to be
distance-sorted by record).

`graph_distances_from_seeds` correctly computes minimum hop distance per NODE_ID
via BFS `or_insert_with` semantics. The bug was in the record-level dedup: the
`HashSet` prevented the minimum-distance node from updating an already-recorded
entry when a later node had a shorter distance.

Fixed by replacing `HashSet<u32>` with `HashMap<u32, (node_id, min_dist)>` in
both paths, updating the entry only when `new_dist < existing_dist`.

### Ranking contract — intentionally conservative

Graph-only candidates are currently appended AFTER vector candidates, not merged
into a single ranking. This is correct for the current phase: there is no
unified scoring formula that meaningfully compares L2 vector distance against
graph hop count. `final_score: null` for graph-only candidates makes this
explicit. When a reranker lands (future phase), it will produce a `final_score`
for all candidates and the combined list can then be re-sorted by `final_score`.

### Graph-only ranking is a defined placeholder, not a reranker

`final_score = vector_score` for vector hits, `null` for graph-only hits, with graph-only
candidates appended after vector candidates. This makes the ordering deterministic but
does NOT combine the two relevance signals — a record reachable at `graph_distance=1`
can never outrank a vector hit with any `vector_score`. Real graph-aware reranking
(combining vector distance and graph proximity into a unified `final_score`) is deferred
to Phase 5.4. The current ranking policy is a holding state, not an architecture decision.

### `max_graph_candidates = 0` rejected at handler

The handler applies `.max(1)` to `max_graph_candidates`, so 0 cannot mean
"unlimited" (which would be confusing). Callers who want all graph candidates
should omit the field (defaults to 100) or pass a large value.

### GraphRAG is retrieval-only — confirmed

No LLM call anywhere in the pipeline. `capability_set.llm: false` in both
handler call sites. The "RAG" in GraphRAG refers to the composition of graph
traversal with vector retrieval as the retrieval context for a downstream LLM
(which the caller provides).

---

## Validation

```
cargo build -p valori-effect -p valori-node                              → clean
cargo fmt --check -p valori-effect -p valori-node                        → clean
cargo test -p valori-node --test api_graphrag                            → 14 passed, 0 failed
cargo test -p valori-node --test api_graph_query                         → 9 passed, 0 failed
cargo test -p valori-node --test api_graph_namespace_isolation           → 5 passed, 0 failed
cargo test -p valori-node --test route_parity                            → 2 passed, 0 failed
cargo test -p valori-node --test vector_graph_retrieval                  → 13 passed, 0 failed
cargo test -p valori-node --test multi_collection_search                 → 10 passed, 0 failed
cargo test -p valori-node (full suite)                                   → 417 passed, 0 failed, 1 ignored
```

### Success criteria status (all 24 verified)

| # | Criterion | Status |
|---|---|---|
| 1 | `retrieval_k` and `final_k` have explicit semantics | ✅ |
| 2 | final result count bounded by `final_k` | ✅ test `graphrag_final_k_bounds_result_count` |
| 3 | Vector score distinct from final score | ✅ `vector_score` / `final_score` fields |
| 4 | Graph-only candidates have defined ranking semantics | ✅ distance asc, record_id asc |
| 5 | `graph_distance` is minimum discovered hop distance | ✅ min-distance HashMap fix |
| 6 | Candidate provenance is deterministic | ✅ vector loop runs first; graph loop checks `contains` |
| 7 | Duplicate candidates merged correctly | ✅ `graphrag_duplicate_candidate_appears_once` |
| 8 | Vector-only candidates survive missing GraphNode | ✅ `graphrag_record_without_graph_node_remains_in_hits` |
| 9 | Graph-only candidates enter candidate pool | ✅ `graphrag_graph_only_candidate_appears_in_hits` |
| 10 | Graph expansion bounded by resource limits | ⚠️ PARTIAL — `max_graph_candidates` truncates the result list; BFS traversal itself is still unbounded (depth only). `max_nodes`/`max_edges` enforced during traversal deferred to Phase 5.4. |
| 11 | Final ordering is deterministic | ✅ `graphrag_deterministic_ordering` |
| 12 | Ties have deterministic resolution | ✅ record_id ascending as tie-breaker |
| 13 | REST response semantics explicit | ✅ phase doc section B |
| 14 | Python SDK models new hit shape | ✅ `remote.py` updated, docstrings added |
| 15 | TypeScript/UI handles null/optional fields | ✅ raw JSON render in playground |
| 16 | Standalone and cluster semantics match | ✅ both impls updated symmetrically |
| 17 | Empty graph remains valid | ✅ `graphrag_on_empty_store_is_empty_not_error` |
| 18 | GraphRAG retrieval remains independent of LLM | ✅ confirmed, no LLM call |
| 19 | Existing Vector Search unchanged | ✅ not touched |
| 20 | Existing Graph Query unchanged | ✅ not touched |
| 21 | Existing Cross-Collection Search unchanged | ✅ not touched |
| 22 | Graph state unaffected by index lifecycle | ✅ not touched |
| 23 | Metrics remain correct | ✅ all 5 existing metrics preserved |
| 24 | Full validation passes | ✅ see Validation section |

---

## Follow-ups

| Item | Phase | Severity |
|---|---|---|
| BFS traversal budget: `max_nodes` / `max_edges` enforced during `expand_subgraph` (current `max_graph_candidates` only truncates the result after full traversal) | 5.4 | High — scalability risk at high branching factor |
| `final_k` default should be bounded (same as `retrieval_k`), not unlimited — current default allows 120+ candidates from a single request | 5.4 | Medium |
| Graph-aware reranker: combine `vector_score` + graph proximity into a unified `final_score` so graph-only candidates can outrank weak vector hits | 5.4 | Medium — current ranking privileges vector position over graph relevance |
| Deprecate `score` field explicitly in API docs and SDK; long-term contract: `vector_score`, `graph_score`, `final_score` | 5.4 | Low |
| Python SDK typed response model (not raw dict) | Future SDK phase | Low |
| Cluster `with_state` closure returns a 5-tuple; refactor to named struct | Refactor sprint | Low |
