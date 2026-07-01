# Phase S12 — Fix standalone/cluster wire-format mismatch on graph node/edge lookups

Branch: `Node-scaleup` (S1-S11 merged, `e79ad05`).

## Goal

While writing SDK documentation for S11's `collection` parameter, found that
`GET /v1/graph/node/:id` and `GET /v1/graph/edges/:id` returned **different
field names** depending on whether the node was running standalone or in
cluster mode:

| Endpoint | Standalone (`api.rs`) | Cluster (`cluster_server.rs`, before this phase) |
|---|---|---|
| `GET /v1/graph/node/:id` | `{"kind", "record_id", "namespace_id"}` | `{"id", "kind", "record"}` |
| `GET /v1/graph/edges/:id` | `{"edges": [{"edge_id", "to_node", "kind"}]}` | `{"edges": [{"id", "from", "to", "kind"}]}` |

The Python SDK's `get_node()`/`get_edges()` pass through `resp.json()`
unmodified — harmless on its own. But `walk()`, `expand()`, and
`neighbors()` read specific keys (`edge["to_node"]`, `n["record_id"]`) that
only exist in the standalone shape. Pointed at a cluster node, all three
methods throw `KeyError` immediately. This predates S1-S11 entirely — the
cluster graph API has had this shape since it was first built, unrelated
to any sharding work. Found as a side effect of documentation, not
sharding testing, because the S8 test only checked array lengths, never
specific field names.

## Delivered

`get_graph_node`/`get_graph_edges` in `cluster_server.rs` now emit the
same field names as standalone's `GetNodeResponse`/`GetEdgesResponse`
(`api.rs`) — `{"kind", "record_id", "namespace_id"}` and
`{"edges": [{"edge_id", "to_node", "kind"}]}` — dropping the previous
`id`/`record`/`from`/`to` field names. `expand_subgraph()` (used by
`/v1/graph/subgraph` and `/v1/graphrag` on both paths) was **not**
touched — it was already a single shared function used by both standalone
and cluster, so its shape (`{"id", "kind", "record"}` for nodes,
`{"id", "from", "to", "kind"}` for edges) was already consistent between
modes. Only the two single-item GET-by-id lookups had diverged.

The S8 test `graph_endpoints_route_to_the_collections_shard` asserted on
the old cluster-only shape (`body["id"]`) — updated to assert on the new,
wire-compatible shape (`body["kind"]`, `body["namespace_id"]`,
`edges[0]["to_node"]`).

## Findings

- This is the second real bug discovered this session purely by writing
  documentation carefully rather than testing behavior directly — the
  first was S7's `soft_delete()`/`/v1/delete` endpoint confusion. Worth
  noting as a pattern: writing accurate docs forces a level of scrutiny
  ("what does this actually return, on every path?") that routing tests
  alone don't, because a routing test can pass while asserting on the
  wrong/inconsistent shape (as S8's test did here).
- `expand_subgraph()` being a genuinely shared function (not duplicated
  per-mode) is why it never had this problem — the single-item GET
  handlers, by contrast, were separately hand-written in `server.rs` and
  `cluster_server.rs` with no shared source of truth, and drifted.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo build -p valori-node --lib                                # clean
cargo test -p valori-node                                       # 233 passed, 0 failed
```

`crates/valori-node/tests/cluster_namespaces.rs::graph_endpoints_route_to_the_collections_shard`
updated and passing against the new shape. No other test file in the
workspace referenced the old cluster-only field names (`id`/`record` on a
node, `id`/`from` on an edge from `get_graph_node`/`get_graph_edges`
specifically — `expand_subgraph`-based tests like `api_graphrag.rs` were
already asserting the correct, unchanged shape).

## Follow-ups

None outstanding for this specific mismatch. General lesson applied going
forward: when a handler exists in both `server.rs` and `cluster_server.rs`
without sharing a helper function, its response shape should be diffed
against its counterpart, not just each independently reviewed for
correctness.
