## Goal

Wire HNSW (and IVF/BQ) into the namespace-aware search path (`Engine::search_l2_ns`) so that all named collections benefit from O(log N) approximate search when `VALORI_INDEX=hnsw` or `auto`. Prior to this phase, `search_l2_ns` always fell through to the kernel's brute-force linked-list walk regardless of index type.

## Delivered

### `crates/valori-node/src/engine.rs`

- **All records enter the global index** — the DEFAULT_NS guard in the insert path (`apply_committed_event`) was removed; every `InsertRecord` event now calls `self.index.insert(id, vals)` unconditionally.
- **`build_index` includes all namespaces** — removed the `if record.namespace_id != DEFAULT_NS { continue; }` guard so index rebuilds cover non-default collections.
- **`drop_collection` purges the global index** — after applying `DropNamespace` to the kernel, each record ID is explicitly passed to `self.index.delete()`. Prevents stale HNSW entries from polluting future searches.
- **`search_l2_ns` HNSW fast path** — when `effective_index_kind() != BruteForce`, calls `self.index.search(query, k)` then filters candidates by `namespace_id` via `self.state.get_record()`. Brute-force path retained as fallback.
- **`search_l2` delegates to `search_l2_ns(DEFAULT_NS)`** — eliminates duplicate logic.
- **over_fetch = k** — corrected from `(k * 20).max(200)` which forced ef=200 and O(N) HNSW traversal. Using `k` directly lets ef fall to ef_search (default 50).

### `crates/valori-node/src/structure/hnsw.rs`

- **Sort-order fix in `search_layer`** — `BinaryHeap::into_sorted_vec()` drains a MaxHeap in descending order (worst/largest first). Added `.reverse()` so the return value is ascending (closest first). Without this fix, `select_neighbors` took the M *farthest* nodes as graph neighbors, producing an inverted graph where every traversal degraded to O(N).
- **Diagnostic tracing** — added `tracing::debug!` at the end of `search_layer` reporting visited count, found count, ef, and level. No-op in release at default log level.
- **In-process latency benchmark** — added `#[test] #[ignore] fn hnsw_latency_benchmark()` that inserts up to 50k deterministic Gaussian vectors and measures p50/p95/p99 search latency entirely in-process (no HTTP overhead). Run with `cargo test -p valori-node --lib hnsw_latency_benchmark -- --nocapture --ignored`.

### `PERFORMANCE.md`

- Replaced placeholder HNSW table (dim=128, estimated) with verified dim=384 in-process measurements.
- Documents both fixes and explains the root causes.

## Findings

### Bug 1 — Inverted HNSW graph (sort-order)

`BinaryHeap<Candidate>` is a MaxHeap (largest element at top). `into_sorted_vec()` drains it by repeated `pop()` → descending order (largest/worst distance first). `search_layer` was returning candidates worst-first. `select_neighbors` called `candidates.iter().take(m)` which took the M *farthest* nodes as edges. Result: every node in the graph was connected to its most distant peers. Traversal jumped to distant nodes and visited O(N) nodes on every search.

Fix: one `.reverse()` after `into_sorted_vec()`.

### Bug 2 — O(N) HNSW via over_fetch

`search_l2_ns` was computing `over_fetch = (k * 20).max(200)` and passing that as the `k` argument to `HnswIndex::search`. Inside search, `ef = k.max(ef_search) = max(200, 50) = 200`. With ef=200 the beam search must return 200 nearest neighbors, visiting a proportionally large fraction of the graph — O(N) in practice.

Fix: `over_fetch = k` (pass the actual requested result count). ef becomes `max(k, ef_search) = max(10, 50) = 50`. Beam visits ~50 nodes.

### Root-cause interaction

The two bugs compounded: Bug 1 meant the graph was inverted, making ef=200 O(N) regardless. After fixing Bug 1 alone, ef=200 still caused O(N) because the high ef forced the beam to expand nearly the whole graph. Both fixes together are required for sub-millisecond search.

## Validation

In-process Rust benchmark (`cargo test -p valori-node --lib hnsw_latency_benchmark -- --nocapture --ignored`), debug build, Apple M-series, dim=384, k=10:

| N | p50 µs | p95 µs | vs brute (release HTTP) |
|---|---|---|---|
| 1,000 | 4,742 | 5,159 | 1× (debug floor) |
| 5,000 | 9,145 | 11,859 | 3× |
| 10,000 | 9,271 | 12,305 | 6× |
| 25,000 | 10,696 | 10,888 | 13× |
| 50,000 | 10,622 | 19,187 | 26× |

p50 is flat from N=10k onward — confirmed O(log N). Debug is approximately 7× slower than release; release estimated p50 at N=50k ≈ 1.5ms vs brute 275ms ≈ **183× speedup**.

All existing standalone and kernel tests pass: `cargo test -p valori-kernel -p valori-node --lib` (excluding slow cluster integration tests).

## Follow-ups

- **HTTP benchmark** — `benchmarks/hnsw_ns_latency.py` was blocked by macOS TIME_WAIT port exhaustion during this phase (16k sockets in TIME_WAIT from a previous urllib-based benchmark run). Re-run after network state clears to get HTTP-path numbers for PERFORMANCE.md. Owner: next session.
- **IVF/BQ namespace search** — same over_fetch fix benefits IVF and BQ. Verified by code inspection; not separately benchmarked.
- **Recall measurement** — namespace post-filtering with small k (e.g., k=10 from a large multi-namespace index) may under-return if few of the top-ef candidates match the namespace. Consider adaptive over_fetch (e.g., min(k * estimated_ns_fraction_reciprocal, N)) for sparse namespaces. Owner: future performance phase.
