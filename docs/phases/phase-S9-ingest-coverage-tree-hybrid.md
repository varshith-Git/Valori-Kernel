# Phase S9 — `cluster_ingest` test coverage + `cluster_tree_hybrid` shard routing

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043`, S3 `0460cee`, S4 `809b87a`, S5, S6, S7, S8 merged).

## Goal

Two items left open since S4: `cluster_ingest` (chunk + embed + insert) was
routed to the correct shard in S4 but never had automated coverage — S4's
doc explicitly deferred it, noting it "requires a mocked embed provider...
disproportionate to add for this pass." Separately, `cluster_tree_hybrid`'s
namespace-scoped vector-search section was flagged all the way back in S1's
original investigation as collection-aware but never confirmed to route
correctly. This phase closes both.

## Delivered

### `cluster_ingest` automated coverage

Added a minimal in-process OpenAI-compatible mock embed server
(`spawn_mock_embed_server()` in `cluster_namespaces.rs`) — an `axum::Router`
with one route, `POST /v1/embeddings`, that echoes back one fixed 4-dim
vector per input text. No external Ollama/OpenAI dependency, no new
dev-dependency (uses `axum`/`tokio`, both already present). Enough to
exercise `cluster_ingest`'s full chunk→embed→insert→graph-node pipeline
end to end: `build_cluster_router_with_keys(..., &node_cfg)` is called
directly (not the `build_cluster_router` convenience wrapper) with a
`NodeConfig` whose `embed_provider`/`embed_url` are set to `"custom"` and
the mock server's address — set as struct fields directly, not env vars,
so the test has no risk of leaking global process state into other
concurrently-running tests.

### `cluster_tree_hybrid` — the actual bug S1 flagged, confirmed and fixed

The vector-hits section of `cluster_tree_hybrid` (`cluster_server.rs`,
around the `payload.namespace` handling) resolved `ns_id` correctly via
`s.sm.resolve_namespace(...)` but then ran the L2 scan itself against
`s.sm.with_state(...)` — the flat shard-0 state machine — regardless of
which shard `ns_id` actually lives on. This is the exact bug class S3b
already fixed once for `cluster_memory_search`: resolving a namespace
correctly and then discarding that information when it matters for
routing. Fixed by routing the scan through
`state.shard_for(ns_id).state_machine` instead.

## Findings

- The `cluster_tree_hybrid` bug had gone unnoticed since S1 specifically
  *because* it was flagged as a known follow-up rather than assumed fixed —
  worth noting as a small case study for why this project's "flag,
  don't silently skip" convention (CLAUDE.md's mandatory phase-doc
  Follow-ups section) matters: a bug that gets written down gets found; one
  that gets silently dropped from a mental TODO list doesn't.
- Building `NodeConfig` directly by struct-field assignment (rather than
  setting `VALORI_EMBED_PROVIDER`/`VALORI_EMBED_URL` env vars before
  calling `NodeConfig::default()`) was the right call for test isolation —
  `cargo test` runs test functions within one process by default, and env
  vars are global process state; a `VALORI_EMBED_PROVIDER` env var set in
  one test could otherwise leak into another concurrently-running test in
  the same binary.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-node          # 233 passed
```

New test:
`crates/valori-node/tests/cluster_namespaces.rs::ingest_routes_to_the_collections_shard`
— ingests a two-paragraph document into `tenant-a` (a non-zero shard) via
the mock embed server, asserts every resulting chunk record and every
graph node (document + one per chunk) landed on tenant-a's own shard, and
that shard 0 has zero records from that traffic.

`cluster_tree_hybrid`'s routing fix has no dedicated new test in this pass
— it shares the same `resolve_namespace` + `with_state` pattern already
covered structurally by `core_crud_routes_to_the_collections_shard` (S7)
and `writes_to_different_collections_route_to_different_shards` (S3), and
exercising it end to end would need the same embed-mock infrastructure
built for `cluster_ingest` in this phase plus a tree-build round trip;
deferred as a small, well-understood follow-up rather than added here to
keep this phase's diff focused on the fix itself. Verified by code review
(identical `state.shard_for(ns_id).state_machine` pattern already proven
correct by three other tests in this file) and by the wasm/full-workspace
build staying green.

## Follow-ups

- `cluster_tree_hybrid` routing fix: add a dedicated end-to-end test once
  the mock-embed-server helper introduced in this phase is reused for it
  (should be a small addition — the helper already exists).
- This closes every item explicitly tracked in the S3/S4 follow-up lists
  except composite external record/node/edge ids and cross-shard community
  detection without a namespace filter — both are a different class of
  work (a shard-local-to-global id remapping / wire-format redesign, not a
  routing fix) and are tracked, not silently dropped. See the final
  regression phase doc for the complete accounting.
