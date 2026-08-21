# Phase G1.4.1 — Graph-Aware Vector Reranking

Design doc: [docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md](../reviews/graph-g1.4.1-graph-aware-reranking-design.md)
(15-section design contract, all 10 readiness questions resolved, verdict
**G1.4.1 READY**). Implements Option 1 from
[docs/reviews/graph-g1.4-hybrid-retrieval-design.md](../reviews/graph-g1.4-hybrid-retrieval-design.md).
Options 2 (reachability pre-filter) and 3 (independent-signal fusion / RRF)
remain explicitly deferred, per instruction.

## Goal

Let graph structure influence `/search` ranking — a candidate structurally
close to the query's own best vector hits ranks up, without ever excluding
a candidate for being graph-distant. Read-time only; zero canonical-state,
snapshot, WAL, or BLAKE3-hash impact.

## Delivered

- **`crates/valori-rag/src/graph.rs`** — `graph_distances_from_seeds(state,
  seeds, direction, max_depth) -> HashMap<u32, u32>`: multi-source, bounded,
  direction-scoped BFS, deterministic, path-independent (first-visit-wins).
  9 new unit tests.
- **`crates/valori-search/src/graph_rerank.rs`** (new file) — pure scoring:
  `graph_penalty(distance, weight)` and `rerank(hits, weight, k)`,
  multiplicative penalty `adjusted = score × (1 + weight × distance)`,
  mirroring `decay::rerank`'s shape exactly. 7 unit tests. Re-exported from
  `valori-search::lib.rs`.
- **`crates/valori-node/src/api.rs`** — `GraphRerankRequest` (seed_count,
  weight, direction, max_depth, all defaulted) as `Option<T>` on
  `SearchRequest.graph_rerank`; `graph_distance: Option<u32>` added to
  `SearchHit`, following the `decay_factor`/`age_secs` `skip_serializing_if`
  pattern exactly.
- **`crates/valori-node/src/server.rs`** — `apply_graph_rerank()`: resolves
  seeds from the top `seed_count` hits' own records (via
  `resolve_seed_nodes`), computes distances (`graph_distances_from_seeds`),
  reduces per-candidate to the minimum distance across
  `nodes_referencing_record` (G1.3.1's enumeration primitive), applies the
  penalty, re-sorts. Wired as an independent final pass after both the
  BM25/plain branch and the decay branch — composes with either.
- **`crates/valori-node/src/cluster_server.rs`** — `apply_graph_rerank_cluster()`,
  identical semantics, operating on `&KernelState` (no `Engine` on the
  cluster path) inside the shard's `with_state` closure. New
  `graph_rerank` field on cluster's own `SearchRequest`; new
  `graph_distance` field on cluster's own `SearchHit`.
- **`python/valoricore/remote.py`** — `graph_rerank: Optional[Dict]` param
  added to `search()` on both `SyncRemoteClient` and `AsyncRemoteClient`.
- **Tests**: `crates/valori-node/tests/graph_aware_reranking.rs` (15,
  standalone HTTP) + `cluster_graph_aware_reranking.rs` (3, real
  single-node Raft cluster) — covering vector-only-unchanged, direct/2-hop/
  unreachable/no-node candidates, multi-node-per-record minimum, multiple
  seeds, all three directions, deterministic ties, snapshot round-trip, a
  real restart, soft-delete exclusion, and namespace isolation (standalone).
- **Docs**: this phase doc, `docs/phases/README.md` status row (below),
  `crates/valori-node/README.md` (new `/search` field row + a full
  "Graph-aware reranking" section), `crates/valori-search/README.md` (new
  module row + usage example + scalability row), `crates/valori-rag/README.md`
  (new scalability row for `graph_distances_from_seeds`), `CLAUDE.md`'s
  Python SDK quick reference, `CHANGELOG.md`.

## Findings

**A real, pre-existing bug was found and is NOT fixed here** (out of
scope, flagged separately): cluster's `/search` ignores namespace/collection
scoping entirely when `VALORI_SHARD_COUNT=1` (the default) —
`cluster_server.rs::search()` calls `s.search_l2(&query, &mut buf, None)`
with no namespace filter, relying solely on shard routing for isolation,
which does nothing when every namespace maps to the same shard. Proven
directly with `graph_rerank` entirely absent (two collections, colliding
vectors, a namespace-A-scoped search returned both). This predates
G1.4.1 — it's the exact discrepancy #4 flagged in the G1.4 audit doc — and
is not something graph-aware reranking causes or worsens (it operates only
on whatever candidate list the existing, buggy search already returns). A
background task has been spawned for it (`task_932d08b1`); it is not part
of this phase's deliverable.

Because of that gap, `cluster_graph_aware_reranking.rs` does not include a
namespace-isolation test — one would be testing a capability the endpoint
doesn't actually have yet, independent of this feature. The standalone
equivalent (`server.rs::search()`, which correctly uses `search_l2_ns`)
does have the isolation test, and it passes.

## Validation

- `cargo fmt --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test -p valori-kernel -p valori-rag -p valori-search`: all green
  (`valori-rag` +9 tests, `valori-search` +7 tests, both new/passing).
- `cargo test -p valori-node`: **356 passed, 0 failed** (up from 338 before
  this phase; +18 = 15 standalone + 3 cluster integration tests).
- `route_parity`: 2/2 passed (no new routes — only new optional fields on
  the existing `/search` endpoint, present on both routers).
- **Revert-and-confirm**: temporarily short-circuited `apply_graph_rerank`
  to a no-op (`return hits` unconditionally) and reran the standalone
  integration suite — **7 of 15 tests failed** as expected (every test that
  asserts graph-distance-based reordering or a `graph_distance` value), 8
  still passed (the ones that don't depend on the rerank actually doing
  anything, e.g. the absent-field and namespace-isolation tests). Restored
  the real implementation; all 15 pass again. Confirms the new tests are
  not vacuous.

## Follow-ups

- **Cluster `/search` namespace isolation** (found here, not fixed here) —
  tracked as a spawned background task; needs its own phase given the
  severity (cross-tenant vector-search leak in the default single-shard
  cluster config).
- Options 2 (reachability pre-filter) and 3 (RRF / independent-signal
  fusion) remain deferred per G1.4's own design doc — no work started on
  either.
- No `edge_kind`/`node_kind` filtering on `graph_rerank` (deliberate, per
  design doc §10) — flagged as the most likely next extension once real
  usage shows it's needed.
- No explicit user-specified seed override (design doc §5) — seeds are
  always derived from the search's own top hits; a future `seed_node`/
  `seed_record` parameter is a natural, additive extension point, not
  built here.
- `graph_rerank` is not supported on `as_of` (point-in-time) queries —
  `search_as_of` operates on a locally replayed `KernelState`, not the live
  `engine`; wiring it in is straightforward but was kept out of this
  phase's minimal scope (documented inline in `server.rs`).
