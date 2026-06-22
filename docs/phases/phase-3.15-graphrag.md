# Phase 3.15 — Native GraphRAG (one-call retrieval)

## Goal

Expose "retrieve the subgraph around the K nearest vectors" as a single
deterministic call — `POST /v1/graphrag` — plus a receipt-bearing
`memory_graph_recall` MCP tool. This collapses the Neo4j+Qdrant two-system
architecture (two stores, two query languages, two consistency models that
drift) into one read against one consistent kernel snapshot.

## Delivered

### Shared traversal — `crates/valori-node/src/graph_rag.rs` (new)

A single module both data planes call, so the standalone and cluster paths stay
identical by construction rather than copy-paste:

| Fn | Purpose |
|---|---|
| `expand_subgraph(&KernelState, seeds, depth)` | Multi-seed BFS → `(nodes, edges)` JSON, de-duplicated, depth clamped to `MAX_DEPTH = 4`. |
| `resolve_seed_nodes(&KernelState, record_ids)` | Resolve `record_id → node_id` from the kernel (the cluster has no `Engine.record_to_node`). |

The existing `get_subgraph` (standalone) and `get_graph_subgraph` (cluster) were
refactored to call `expand_subgraph`, removing two copies of the BFS.

### Endpoint — `POST /v1/graphrag`

Added to **both** `server.rs` and `cluster_server.rs` (per the keep-in-sync
invariant). Request: `{ query_vector, k, depth=2, collection? }` (cluster also
honours `consistency`, linearizable by default). Response:

```jsonc
{
  "hits":       [ { "memory_id", "record_id", "score", "node_id", "metadata" } ],
  "seed_nodes": [ <node ids the hits mapped to> ],
  "subgraph":   { "nodes": [...], "edges": [...] }
}
```

Flow, all under one `engine.read()` / `sm.with_state()` snapshot:
`search_l2_ns` (KNN) → `record_to_node` (resolve seeds) → `expand_subgraph` (BFS).

### MCP tool — `memory_graph_recall` (valori-mcp now exposes 7 tools)

Composes `/v1/graphrag` + `/v1/proof/*` into a recall that returns hits, the
subgraph, **and a receipt binding both**. The receipt grew an optional
`subgraph: { node_ids, edge_ids }` (sorted, order-independent). Plain
`memory_recall` receipts are byte-identical to before — the new field is
`skip_serializing_if = Option::is_none`.

### Python SDK

`graphrag(query_vector, k=5, depth=2, collection="default", consistency=None)`
added to all four clients — `SyncRemoteClient`, `AsyncRemoteClient`,
`ClusterClient`, `AsyncClusterClient` (cluster variants route to a read replica
and honour `consistency`). Returns `{ hits, seed_nodes, subgraph }`. The SDK
already had `create_node` / `create_edge`, so a Python user can build the graph
and query it end-to-end (`create_node(record_id=…)` populates `record_to_node`).

### Example

`examples/mcp_agent_memory.py` now links three memories into a chain and runs
`memory_graph_recall`, printing the returned subgraph and re-deriving the
receipt digest (hits **+** subgraph) in Python. Live run:

```
[2] GraphRAG — hits + connected subgraph in ONE call
  hits           : 1
  subgraph nodes : 3  edges: 2  (walked the chain)
  receipt binds  : 1 hits + 3 nodes / 2 edges
  verify: PASS
```

## Findings

- **The standard ingest edge points doc→chunk (inbound to the chunk seed).** A
  GraphRAG seed is the record's chunk node; `expand_subgraph` walks *outgoing*
  edges, so a freshly-ingested isolated memory expands to just its own chunk
  node. The subgraph's richness scales with the edges that exist (entity links,
  citations, manual edges) — documented, and the tests add an explicit outbound
  edge to exercise traversal.
- **Cluster has no `record_to_node` index.** The standalone `Engine` keeps an
  O(1) map; the cluster data plane only has `KernelState`, so `resolve_seed_nodes`
  scans `iter_nodes()` once per query (O(N)). Fine for v1; a kernel-side index is
  a follow-up if cluster GraphRAG becomes hot.
- **Score types differ by path** (standalone f32 distance vs cluster i64 fxp) —
  a pre-existing inconsistency inherited from each path's `search`; the receipt's
  `score_bits` handles either via the JSON number.
- **One snapshot = the whole differentiator.** Because vectors and graph share
  one `KernelState` behind one lock, KNN + seed-resolution + BFS run against a
  single consistent snapshot with no second system and no cross-store drift —
  the thing the two-system stacks cannot promise.

## Validation

```
cargo test -p valori-kernel -p valori-node -p valori-mcp
kernel 50  +  node 182  +  mcp 33   = 265 passing, 0 failing
```

New tests:
- `valori-node/tests/api_graphrag.rs` (3) — endpoint composition: hits + connected
  subgraph incl. the chunk→doc edge; empty store → empty (not error); depth 0 →
  seeds without edges.
- `valori-mcp` lib (+4) — subgraph fingerprint sorting/order-independence, digest
  changes when a subgraph is bound, plain recall digest unaffected by the new
  field, `memory_graph_recall` binds hits + subgraph.
- `valori-mcp/tests/integration_node.rs` (+1) — GraphRAG against a **real** node:
  one call returns hits + subgraph, and the receipt independently recomputes.
- `python/tests/test_graphrag_sdk.py` (new) — spawns a node, builds a 3-node chain
  via the SDK, and asserts `SyncRemoteClient.graphrag` / `AsyncRemoteClient.graphrag`
  return hits + the connected subgraph. Live: `sync/async: hits=3 nodes=3 edges=2 OK`.

Manual: `python3 examples/mcp_agent_memory.py` → both digests verify.

## Follow-ups

- **Hybrid scoring / re-rank** — weight hits by graph proximity to the seed set
  (PageRank-style) so connected context can re-order vector hits.
- **Edge-kind / direction filters** on `/v1/graphrag` (e.g. only `Cites` edges,
  or treat the graph as undirected for expansion).
- **Cluster `record_to_node` index** — replace the O(N) `resolve_seed_nodes` scan
  with a maintained map if cluster GraphRAG load warrants it.
- **Collection support in cluster GraphRAG** — mirror the cluster `search` limitation
  (currently default namespace); fold in once cluster search is namespace-aware.
