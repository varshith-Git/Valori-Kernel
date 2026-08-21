# Phase G1.4.2 — Cluster Vector Search Namespace Isolation

Immediate follow-up to G1.4.1, per explicit direction: the bug found (not
fixed) there is fixed here before any further hybrid-retrieval feature
work.

## Goal

Fix BUG-6: cluster's `POST /search` ignored namespace/collection scoping
entirely whenever more than one namespace mapped to the same shard —
the default single-shard cluster deployment (`VALORI_SHARD_COUNT=1`, which
puts every namespace on shard 0). Prove isolation holds across the full
matrix: 1 shard/2 namespaces (the exploitable case), N shards/N namespaces
(the already-safe-by-routing case), for every search mode (plain, decay,
metadata_filter, graph_rerank), plus soft-delete and the default namespace.

## Root cause

`KernelState::search_l2` (`crates/valori-kernel/src/state/kernel.rs`) is
documented as searching "ALL records regardless of namespace
(backward-compat, single-tenant)". Every cluster search call site used
exactly this function and relied entirely on `shard_for(ns)` routing for
isolation — correct when every namespace maps to a distinct shard, silently
wrong once two or more namespaces share a shard (`shard_count=1` always
does this; it's the deployment default). Standalone's
`Engine::search_l2_ns` never had this bug — it already branches on the
active index (`KernelState::search_l2_ns`, exact and namespace-scoped, for
`BruteForce`; a global search + post-filter for anything else).

Confirmed directly, with `graph_rerank` entirely absent: two collections,
colliding vectors, a search scoped to one collection returned both.

## Delivered

- **`crates/valori-node/src/cluster_server.rs`** — new `shard_search_ns()`,
  mirroring standalone's `Engine::search_l2_ns` split exactly: calls the
  kernel's own `search_l2_ns` (exact, namespace-scoped via the intrusive
  per-namespace linked list) when `KernelState::index_variant() ==
  BruteForce`; falls back to `search_l2` (global) + post-filter by
  `record.namespace_id == ns_id` otherwise (inheriting the same
  already-documented, unrelated pool-sizing gap standalone's equivalent
  branch has — not introduced here). Cluster never calls
  `set_index_kind()` on any shard's `KernelState`, so every cluster shard
  is `BruteForce` today — confirmed by grep, not assumed — meaning the
  exact path is what actually runs in production right now; the fallback
  branch exists for correctness if that changes.
- Both call sites in `cluster_server.rs::search()` (the BM25/plain branch
  and the decay branch) now go through `shard_search_ns()` instead of
  calling `s.search_l2(...)` directly.
- Audited every other cluster search-adjacent call site
  (`search_vector`/`/v1/memory/search_vector`, metadata filtering, the
  graph_rerank wiring added in G1.4.1) — all already used `search_l2_ns`
  correctly or operate downstream of the now-fixed candidate list, so no
  other call site needed a change.

## Findings

- The bug was isolated to the plain `POST /search` handler only —
  `search_vector` (the memory-search endpoint) already called
  `search_l2_ns` correctly, and `graph_rerank`/metadata_filter/decay all
  operate on whatever candidate list `/search` hands them, so fixing the
  one root call site fixed every downstream mode automatically.
- Cluster shards never configure a non-`BruteForce` kernel index — `grep`
  found zero call sites of `set_index_kind()` anywhere in
  `valori-node`/`valori-consensus`. `VALORI_INDEX=bq`/`hnsw`/`ivf` has no
  effect on cluster mode today (a separate, pre-existing gap, not in scope
  here — flagged for awareness, not filed as a new bug since it has no
  correctness impact, only an unused-configuration one).

## Validation

- New file: `crates/valori-node/tests/cluster_search_namespace_isolation.rs`
  — 7 tests: 1-shard/2-namespace isolation for plain search, decay,
  metadata_filter, and graph_rerank (all four exploitable pre-fix, all four
  pass post-fix); soft-deleted record exclusion under namespace scoping;
  3-shard/2-namespace sanity (confirms the already-safe-by-routing case
  stays safe, using result-count assertions rather than raw-id comparison
  since each shard runs an independent record-id counter — two different
  shards' first inserts can legitimately share the same numeric id);
  default-namespace search unaffected by the fix.
- **Revert-and-confirm**: temporarily short-circuited `shard_search_ns` to
  always call the old namespace-blind `search_l2` — **4 of 7 tests failed**
  exactly as expected (plain, decay, metadata_filter, graph_rerank all
  leaked across namespaces; the 3-shard, soft-delete, and default-namespace
  tests still passed, since they don't depend on the fix). Restored the
  real implementation; all 7 pass again.
- `cargo fmt --check`: clean. `cargo clippy --workspace --all-targets -- -D
  warnings`: clean.
- `cargo test -p valori-node`: **363 passed, 0 failed** (up from 356 before
  this phase; +7 = this file's new tests). `route_parity`: 2/2 passed (no
  route/method changes — only a private helper function and an internal
  wiring change inside the existing `/search` handler).

## Follow-ups

- `VALORI_INDEX` has no effect on cluster mode (every shard is always
  `BruteForce`) — a separate, lower-severity gap (unused config, not a
  correctness bug) that could be its own future phase if cluster-mode BQ
  support is ever prioritized.
- No change made to non-`BruteForce` cluster search performance
  characteristics, since none exist today — the fallback branch in
  `shard_search_ns` is present for forward-compatibility, not exercised by
  current production configuration.
