# Phase S8 — Graph management + community endpoint routing

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043`, S3 `0460cee`, S4 `809b87a`, S5, S6, S7 merged).

## Goal

Graph node/edge CRUD (`/v1/graph/node`, `/v1/graph/node/:id`,
`/v1/graph/edge`, `/v1/graph/edges/:id`, `/v1/graph/subgraph`),
`/v1/graphrag`, and the community-detection endpoints
(`/v1/community/detect`, `/v1/community/search`, `/v1/community/overview`)
had no shard routing on the cluster path — every one of them always read
from or wrote to shard 0's `KernelState`, even though the standalone
server's equivalents (`create_node`/`get_node`/etc. in `server.rs`) already
resolve and honor a `collection` field. This was a documented gap in the
node README (`crates/valori-node/README.md:30-35` promises "every data
endpoint accepts an optional `collection` field" project-wide) that the
cluster path was silently violating for this entire surface.

## Delivered

### Graph node/edge CRUD

- **`create_graph_node`** (`/v1/graph/node`) — `CreateNodeRequest` gained
  `collection`; resolves `ns_id`, routes `AutoCreateNode` through
  `state.shard_for(ns_id).raft` with `namespace_id: ns_id`.
- **`get_graph_node`** (`/v1/graph/node/:id`) — gained a
  `?collection=` query param (new shared `CollectionQuery` extractor, also
  used by `get_graph_edges`). Node ids are only unique within their own
  shard's kernel state (same reasoning as S7's `DeleteRequest::collection`
  for record ids), so a bare `:id` path segment can't resolve a node
  without knowing which shard to look in.
- **`create_graph_edge`** (`/v1/graph/edge`) — `CreateEdgeRequest` gained
  `collection`; routes the same way as node creation.
- **`get_graph_edges`** (`/v1/graph/edges/:id`) — same `?collection=`
  treatment as node lookup.
- **`get_graph_subgraph`** (`/v1/graph/subgraph`) — `SubgraphQuery` (already
  a query-string struct) gained `collection`; the BFS (`expand_subgraph`)
  now runs against the target shard's `KernelState`, not always shard 0's.

### `cluster_graphrag` (`/v1/graphrag`)

`ClusterGraphRagRequest` gained `collection`. The dim check, the
linearizable read-index call (now passes
`shard_for_namespace(ns_id, state.shard_count)` and that shard's `raft` —
the same S6 signature already used by `search`), and the KNN + subgraph
scan itself all route through the resolved shard's state machine.

### Community endpoints

`crate::community::DetectRequest` already had a `namespace: Option<String>`
field with a doc comment ("Limit detection to nodes in a specific
collection") — the **standalone** `community_detect` handler already
honored it (resolves via `eng.namespaces.resolve(...)`), but the
**cluster** handler discarded it entirely, always scanning `s.sm` (shard
0) with `namespace_filter: None` regardless of what was requested. Fixed:
when `payload.namespace` is `Some(name)`, `cluster_community_detect` now
resolves `ns_id` and routes the entire label-propagation scan to
`state.shard_for(ns_id).state_machine`, with `Some(ns_id)` as the filter —
matching the standalone handler's behavior exactly, including its silent
fallback (an unknown namespace name resolves to `None`/no-filter, same as
`server.rs`'s existing behavior — not a new gap introduced here).

`cluster_community_search` and `cluster_community_overview` needed no
changes: both read only from the node-local cached `community_store`
(populated by the most recent `detect` call), never touch `KernelState`
directly, so there is nothing to route.

## Findings — explicitly out of scope, not silently dropped

**`cluster_community_detect` with `namespace` omitted** still scans shard 0
only, unchanged from before this phase. True "detect communities across
every shard" is a materially different problem, not a routing fix: node
ids are only unique **within** a single shard's kernel state (each shard
runs its own independent id counter, confirmed back in S4's
`cluster_extract_entities` finding). Label propagation's output
(`node_id → community_id`) from two different shards can't be merged
without first solving a shard-local-to-global id remapping — the same
class of problem as the composite external record/node/edge id gap already
tracked since S3/S4, not something this pass attempts. Cross-namespace
graph edges are already rejected at apply time (confirmed architectural
invariant), so communities *within* one namespace are never split across
shards — only the "no namespace given, scan everything" case is affected,
and it keeps its pre-S8 shard-0-only behavior rather than silently
returning a partial or incorrectly-merged result.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-node          # 233 passed
```

New tests in `crates/valori-node/tests/cluster_namespaces.rs`:

- `graph_endpoints_route_to_the_collections_shard` — creates 2 nodes + 1
  edge in `tenant-a` (a non-zero shard), asserts both nodes and the edge
  landed there (not shard 0) via direct `KernelState::node_count()`/
  `edge_count()`, then proves the `?collection=` reads
  (`GET /v1/graph/node/:id`, `GET /v1/graph/edges/:id`,
  `GET /v1/graph/subgraph`) find them.
- `community_detect_scoped_to_namespace_scans_the_right_shard` — 2 nodes in
  `tenant-a`, 1 node in the default namespace (shard 0); asserts
  `POST /v1/community/detect {"namespace": "tenant-a"}` returns
  `node_count: 2` (only tenant-a's), and the unscoped call still returns
  `node_count: 1` (shard 0 only, unchanged default).

## Follow-ups

- Community detection across every shard (no namespace filter) remains
  shard-0-only by design — see Findings above; needs a
  shard-local-to-global id remapping scheme before it can be attempted,
  tracked alongside composite external ids.
- `cluster_ingest` test coverage and `cluster_tree_hybrid`'s namespace-scoped
  query section were still open at the start of this phase — delivered in
  the same working session, see `phase-S9-ingest-coverage-tree-hybrid.md`.
