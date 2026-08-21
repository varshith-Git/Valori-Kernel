# Phase 5.2 — GraphRAG Query Orchestration and Retrieval Composition

## Goal

Audit the existing GraphRAG query pipeline end-to-end, confirm that the existing
Vector→Graph→Context composition is correct and Collection-scoped, fill the genuine
gaps discovered during the audit (provenance, graph-only candidates, deduplication,
observability), and fix a latent UI bug. Do NOT redesign the graph storage model,
vector search, or index lifecycle.

---

## Delivered

### Pre-flight audit: 24 items — 19 COMPLETE, 4 MISSING, 1 BUG

**All COMPLETE items verified by source inspection (not assumed):**

| Capability | Where verified |
|---|---|
| `POST /v1/graphrag` endpoint (standalone + cluster) | `server.rs:2184`, `cluster_server.rs:3123` |
| `GraphRagRequest` / `ClusterGraphRagRequest` (query_vector, k, depth, collection) | Both request structs |
| Vector retrieval (`search_l2_ns`) | `EngineKernelCapability::graph_rag`, `RaftKernelCapability::graph_rag` |
| Seed resolution (`resolve_seed_nodes`, canonical state, no cache) | `capabilities.rs:192`, `906` — G1.3 comments preserved |
| Graph expansion (`expand_subgraph`, outgoing BFS, depth-clamped to MAX_DEPTH=4) | `capabilities.rs:212`, `918` |
| Collection scoping (namespace resolved before all operations) | Both paths |
| Vector-only candidates (records with no graph node → `node_id: null`) | `capabilities.rs:197-209` before this phase |
| Empty graph → vector hits still returned | seeds=[] → expand_subgraph returns empty; hits populated normally |
| Expansion bounds (MAX_DEPTH=4 clamped inside `expand_subgraph`) | `valori_rag/src/graph.rs:360` |
| Python SDK `graphrag` sync + async | `remote.py:466`, `1634` |
| Cluster linearizable read (`ensure_read_consistency` before BFS) | `cluster_server.rs:3176` |
| Effect counter (`graphrag_queries`) in task | `tasks/graph_rag.rs:56-66` |
| `valori_graphrag_total` counter in handlers | added in Phase 5.1 |
| Snapshot/WAL/recovery intact | not touched |
| Graph state unaffected by index lifecycle | not touched |
| Cross-collection graph edges absent | enforced at kernel level |
| Normal vector search unchanged | not touched |
| Graph query independent of vector search | not touched |
| Cluster GraphRAG uses existing routing | Raft-level collection → namespace → shard routing |

### Gap 1: provenance and graph_distance on hits — `crates/valori-node/src/capabilities.rs`

Each hit in the `hits` array now carries two new fields:

```json
{
  "source": "vector_and_graph" | "vector" | "graph",
  "graph_distance": 0 | N | null
}
```

- `"vector"` — record came from vector search; no graph node in this collection.
  `graph_distance: null`.
- `"vector_and_graph"` — record came from vector search AND has a graph node
  (it seeded graph expansion). `graph_distance: 0` (seed nodes are distance 0
  from themselves).
- `"graph"` — record was NOT in the top-k vector results but was reached by
  graph expansion from a seed. `score: null`, `graph_distance: N` (hop count
  from the nearest seed node, via a second outgoing BFS pass using
  `graph_distances_from_seeds`).

Both standalone (`EngineKernelCapability::graph_rag`) and cluster
(`RaftKernelCapability::graph_rag`) were updated symmetrically. The JSON `null`
for `score` on graph-only candidates is intentional — there is no vector distance
for records that were not in the ANN result set.

### Gap 2: graph-only candidates in `hits` — `crates/valori-node/src/capabilities.rs`

Records referenced by expanded nodes that are NOT in the vector hit set now appear
in `hits` with `source: "graph"`. They enter the same `hits` array as vector
candidates, so the SDK and rerankers can treat the full set uniformly.

**Standalone path** (inside `EngineKernelCapability::graph_rag`):
After `expand_subgraph`, a second BFS (`graph_distances_from_seeds`, outgoing,
same depth) produces a node→distance map. The expanded nodes array (JSON) is
iterated; nodes with a non-null `record` field and a record_id not already in
`vector_record_set` (a `HashSet` built during vector hit processing) are added
to `hits_out` as graph-only candidates. `HashSet::insert` provides O(1) dedup:
if a record is referenced by multiple nodes in the expansion, only the first
occurrence is added.

**Cluster path** (inside `RaftKernelCapability::graph_rag`):
All computation that requires `KernelState` (vector search, seed resolution,
expand_subgraph, graph_distances_from_seeds, graph-only candidate extraction)
happens inside one `with_state` closure to avoid re-acquiring the state lock.
The closure now returns an extended tuple:
`(raw_hits, seeds, nodes, edges, graph_candidates, no_graph_seed)`
where `graph_candidates: Vec<(record_id, node_id, graph_dist)>`.
Metadata for graph-only candidates is fetched asynchronously in pass 2, after
`with_state` returns, matching the existing two-pass pattern.

### Gap 3: deduplication — `crates/valori-node/src/capabilities.rs`

A record that appears as BOTH a vector hit AND a graph neighbor of another seed
appears exactly once in `hits`. The `vector_record_set: HashSet<u32>` built
during vector hit processing ensures that the graph-only candidate loop skips
any record_id already in the set. The record's final `source` is `"vector_and_graph"`
(set during the vector hit loop, not overwritten by the graph candidate loop).

### Gap 4: 4 new GraphRAG observability metrics — `crates/valori-node/src/capabilities.rs`

| Metric | Kind | Meaning |
|---|---|---|
| `valori_graphrag_seed_count` | histogram | Number of vector hits that resolved to a graph node (seed count) per call |
| `valori_graphrag_expanded_nodes` | histogram | Nodes in the expanded subgraph per call |
| `valori_graphrag_expanded_edges` | histogram | Edges in the expanded subgraph per call |
| `valori_graphrag_no_graph_seed` | counter | Fires when vector hits exist but NONE map to a graph node — the graph is absent or unlinked |

Standalone: metrics fire directly after the relevant computations.
Cluster: `no_graph_seed` flag and counts are returned from `with_state` and metrics
fire outside the closure (after all async ops complete).

Combined with the existing `valori_graphrag_total` counter and `graphrag_queries`
effect counter, the 5 graphrag metrics now support debugging every phase of the
pipeline: how many seeds were found, how big the expansion was, and whether the
graph is ever usefully consulted.

### Bug fix: UI GraphRAG sample body — `ui/studio/src/components/projects/PlaygroundView.tsx`

The PlaygroundView sent `query:` instead of `query_vector:` in the GraphRAG sample
body (line 139). The server would have rejected this with a deserialization error
(`missing field 'query_vector'`). Fixed:

```diff
- sampleBody: (dim, collection) => ({ query: vec(dim), k: 5, depth: 2, collection }),
+ sampleBody: (dim, collection) => ({ query_vector: vec(dim), k: 5, depth: 2, collection }),
```

### New tests — `crates/valori-node/tests/api_graphrag.rs`

| Test | What it proves |
|---|---|
| `graphrag_record_without_graph_node_remains_in_hits` | Record with no graph node stays in hits as `source: "vector"`, `node_id: null`, `graph_distance: null` — graph absence never drops a vector candidate |
| `graphrag_graph_only_candidate_appears_in_hits` | Record B (far from query, not in top-k vector results) but reachable from seed A at depth 1: appears in hits with `source: "graph"`, `score: null`, `graph_distance: 1` |
| `graphrag_duplicate_candidate_appears_once` | Record B is both a vector hit AND a graph neighbor of A: appears exactly once in hits with `source: "vector_and_graph"`, not duplicated |

All 3 tests are revert-confirmed: the `graphrag_record_without_graph_node_remains_in_hits`
assertion on `source` would fail against old code (field absent); the graph-only candidate
test would fail because B would not appear in hits at all; the dedup test would fail if B
appeared twice.

---

## Findings

### Verified: GraphRAG is purely retrieval — no LLM

The full call stack was traced:
```
POST /v1/graphrag
  → graphrag handler (server.rs / cluster_server.rs)
  → run_graph_inline (single TaskKind::GraphRag task)
  → GraphRagTask::run (tasks/graph_rag.rs)
  → capabilities.kernel.graph_rag (EngineKernelCapability or RaftKernelCapability)
```

No LLM call anywhere in this path. `PlanningContext::capability_set.llm = false`
in both handler call sites. The response is purely retrieval context. The term
"GraphRAG" describes the composition of graph traversal with vector retrieval —
it does not imply an LLM generation step.

### Verified: Collection isolation is enforced structurally

Both `EngineKernelCapability::graph_rag` and `RaftKernelCapability::graph_rag`
resolve the collection name to a `namespace_id` before ANY kernel operation.
`search_l2_ns` restricts vector search to that namespace. `resolve_seed_nodes`
only finds graph nodes whose records are in that namespace's slab (because vector
search results are namespace-scoped). `expand_subgraph` follows edges in the kernel
state — cross-namespace edges are rejected at the kernel write path
(`apply_event_ns::CreateEdge` → `from_ns != to_ns` → `KernelError::InvalidOperation`),
so they cannot exist in the graph at all.

### Verified: `resolve_seed_nodes` is O(live_nodes), not O(1)

The implementation scans the full node pool once. This was a deliberate design
decision in Phase G1.3 to eliminate the stale `record_to_node` cache that caused
both parity and staleness bugs. The cost is bounded by the number of live nodes
and runs under the engine read lock (standalone) or inside `with_state` (cluster).
An `#[ignore]` benchmark exists for profiling at scale. No change needed.

### Verified: `graph_distances_from_seeds` takes `seeds: &[u32]` not `&[NodeId]`

The function signature uses raw `u32` for node IDs (same as the rest of the
traversal primitives). The `direction` parameter is `valori_rag::graph::Direction::Outgoing`
(matching the direction used by `expand_subgraph`) and `max_depth` matches the
request `depth` so the BFS distance never exceeds what was actually expanded.

### UI: GraphRAG is exposed in PlaygroundView under "Graph + RAG" tab

The PlaygroundView has a "Graph + RAG" section with the GraphRAG endpoint. The
sample body bug (wrong field name) was the only UI issue found. Collection
selection is provided via the `collection` field in the sample body (populated
from the active collection context). Graph depth is hardcoded to 2 in the sample
body; users can edit it in the raw JSON editor. This is adequate for Phase 5.2.

---

## Validation

```
cargo build -p valori-node              → clean
cargo fmt -p valori-node --check        → clean
cargo test -p valori-rag --lib          → 46 passed, 0 failed, 3 ignored
cargo test -p valori-node --test api_graphrag              → 8 passed, 0 failed (was 5)
cargo test -p valori-node --test api_graph_query           → 9 passed, 0 failed
cargo test -p valori-node --test api_graph_namespace_isolation → 5 passed, 0 failed
cargo test -p valori-node --test graph_aware_reranking     → included in aggregated run
cargo test -p valori-node --test cluster_graph_aware_reranking → included in aggregated run
cargo test -p valori-node --test vector_graph_retrieval    → included in aggregated run
cargo test -p valori-node --test graph_cascade_delete      → included in aggregated run
cargo test -p valori-node --test graph_query_restart_recovery → included in aggregated run
cargo test -p valori-node --test multi_collection_search   → 10 passed, 0 failed
cargo test -p valori-node --test route_parity              → 2 passed, 0 failed
[graph + cascade + restart + reranking aggregated]         → 13 passed, 0 failed
cargo test -p valori-node (full suite)                     → 411 passed, 0 failed
```

---

## GraphRAG Architecture (final state for this phase)

```
POST /v1/graphrag
    │
    ▼
handler (server.rs / cluster_server.rs)
    │  • resolve collection name → namespace_id
    │  • validate query_vector dimension (cluster path only)
    │  • ensure_read_consistency (cluster, linearizable)
    │  • build ExecutionGraph { TaskKind::GraphRag }
    │  • run_graph_inline
    │
    ▼
GraphRagTask::run (valori-effect/src/tasks/graph_rag.rs)
    │  • deserialize inputs: shard_id, namespace_id, vector, k, depth
    │  • dispatch graphrag_queries effect counter
    │
    ▼
EngineKernelCapability / RaftKernelCapability ::graph_rag
    │
    ├── [1] vector retrieval
    │       search_l2_ns(vector, k, namespace_id)
    │       → top-k (record_id, f32_score) pairs
    │
    ├── [2] seed resolution
    │       resolve_seed_nodes(state, record_ids)
    │       → HashMap<record_id, node_id>  (first node per record, O(live_nodes))
    │
    ├── [3] vector hits assembly
    │       for each (record_id, score) in hits:
    │           node_id = seed_map.get(record_id)
    │           source  = if node_id.is_some() { "vector_and_graph" } else { "vector" }
    │           graph_distance = if node_id.is_some() { 0 } else { null }
    │           → add to hits_out, add record_id to vector_record_set
    │
    ├── [4] graph expansion
    │       expand_subgraph(state, seeds, depth)  [depth clamped to MAX_DEPTH=4]
    │       → (Vec<node_json>, Vec<edge_json>)
    │
    ├── [5] graph-only candidates (Phase 5.2)
    │       graph_distances_from_seeds(state, seeds, Outgoing, depth)
    │       → for each node in expanded nodes with record not in vector_record_set:
    │           add (record_id, node_id, graph_dist) to graph_candidates
    │
    └── [6] response
            {
              "hits": [
                { "record_id", "score"|null, "node_id"|null,
                  "graph_distance"|null, "source", "memory_id", "metadata" },
                ...
              ],
              "seed_nodes": [node_id, ...],
              "subgraph": { "nodes": [...], "edges": [...] }
            }
```

**Separation of responsibilities (verified unchanged):**
- `POST /v1/search` — vector retrieval only; graph_rerank is opt-in via request field
- `GET /v1/graph/query` — graph traversal only; no vector search
- `POST /v1/graphrag` — explicit composition of vector + graph; both always run

---

## Follow-ups

| Item | Phase |
|---|---|
| Cluster GraphRAG: `with_state` closure now returns an 8-tuple; consider a named struct for readability | Refactor sprint |
| `resolve_seed_nodes` O(live_nodes): lazy record→node index at scale >100k nodes | Future |
| Python SDK: add explicit tests for `source` field and graph-only candidates | Future |
| UI: expose depth as an editable field in the GraphRAG playground panel (not just raw JSON) | Future UI |
| Multi-Collection GraphRAG: per-collection vector search → per-collection independent graph expansion (no cross-collection edges) | Future phase |
