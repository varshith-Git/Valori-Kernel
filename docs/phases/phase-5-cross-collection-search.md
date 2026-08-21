# Phase 5 — Query Orchestration and Cross-Collection Search

## Goal

Add a thin query orchestration layer above individual Collections that fans a
vector query out to multiple Collections in parallel, merges results globally by
Squared L2 distance, and returns hits tagged with their source Collection.
Cross-collection search is an **orchestration problem, not an index-sharing
problem** — no physical index is shared, merged, or created at the project level.

## Delivered

### `crates/valori-node/src/api.rs`

| Type | Purpose |
|---|---|
| `MultiSearchRequest` | Request body for `POST /v1/search/multi`: `query`, `k`, `collections: Vec<String>`, `decay_half_life_secs?`, `metadata_filter?` |
| `MultiSearchHit` | Single hit annotated with `collection: String`, `id`, `score`, `decay_factor?`, `age_secs?` |
| `MultiSearchResponse` | `results: Vec<MultiSearchHit>`, `collections_searched`, `partial_failures?` |
| `PartialSearchFailure` | Per-collection runtime error for partial-result scenarios |

### `crates/valori-node/src/routes/query_planner.rs` (new file)

Pure orchestration helpers, free of axum/engine references:

| Item | Purpose |
|---|---|
| `check_compatibility` | Validates all Collections share the same `dim` and `metric`. Returns `(dim, metric)` or an error string. Different index types are explicitly allowed. |
| `merge_top_k` | Merges `Vec<CollectionHits>` into a globally sorted `MultiSearchResponse` — sort criterion: `score` ascending (Squared L2, smaller = better). No score mutation. |
| `CollectionHits` | Per-collection search result container used between the fan-out and merge steps. |
| `MAX_MULTI_COLLECTIONS = 32` | Hard ceiling on collections per request. |
| `MAX_MULTI_SEARCH_K = 5000` | Mirrors `MAX_SEARCH_K` on both single-collection paths. |

### `crates/valori-node/src/routes/mod.rs`

Added `pub mod query_planner`.

### `crates/valori-node/src/server.rs` — standalone path

Added `async fn multi_search` handler and `.route("/v1/search/multi", post(multi_search))`.

Handler flow:
1. Validate `k` bounds and `collections` list size.
2. Under one read lock: resolve all collection names → ns_ids + `CollectionVectorConfig`.
3. `check_compatibility` → 400 if dim or metric mismatch.
4. Query length vs. dim check → 400 on mismatch.
5. Fan-out: `futures::future::join_all` over independent `state.read().await` locks
   (tokio's RwLock allows concurrent readers → truly parallel fan-out).
6. No-decay path: `engine.search_l2_ns` + `apply_metadata_filter`.
7. Decay path: over-fetch pool (4×k), `decay_rerank`, metadata filter, take k.
8. `merge_top_k` → return `MultiSearchResponse`.
9. Runtime errors from individual collections become `PartialSearchFailure` entries;
   other collections' results still return.

**Intentional exclusions:**
- BM25 reranking: hybrid scores from different Collection corpora are not comparable and would distort the global merge.
- Graph reranking: graph edges are Collection-scoped (no cross-collection graph).
- Point-in-time (`as_of`): supported only for single-collection queries.

### `crates/valori-node/src/cluster_server.rs` — cluster path

Added `async fn cluster_multi_search` handler and `.route("/v1/search/multi", post(cluster_multi_search))`.

Cluster handler flow mirrors standalone, but:
- Uses `state.sm.resolve_namespace` + `state.sm.namespace_config` for resolution.
- Readiness gate via `state.readiness.check(&state.raft)`.
- Fan-out: per-collection → `state.try_ann_search` (ANN path) fallback to `shard_search_ns` (brute-force).
- Reads use **local consistency** (no per-shard linearizability round-trip). Noted as a known gap below.
- Metadata filter reads from `state.meta` via `shard_sm.with_state`.

### `python/valoricore/remote.py`

Added `search_multi` to both `SyncRemoteClient._SyncSearchMixin` and `AsyncRemoteClient._AsyncSearchMixin`:

```python
c.search_multi(
    query=[0.1, 0.2, 0.3],
    k=10,
    collections=["products", "documents"],
    decay_half_life_secs=86400,       # optional
    metadata_filter={"author": "Alice"},  # optional
)
# → {"results": [{"collection": "products", "id": 7, "score": 0.02, ...}, ...],
#    "collections_searched": ["products", "documents"],
#    "partial_failures": None}
```

### `crates/valori-node/tests/multi_collection_search.rs` (new file)

10 integration tests against the standalone path:

| Test | What it proves |
|---|---|
| `golden_merge_order_and_collection_tag` | Merge is sorted by score; each hit carries its `collection`; exact match beats close match globally |
| `collections_searched_field` | `collections_searched` always lists all requested collections |
| `k_zero_returns_400` | k=0 is rejected |
| `empty_collections_returns_400` | Empty collections list returns 400 with "empty" in the error |
| `unknown_collection_returns_error` | Unknown collection returns 400/404 |
| `dimension_mismatch_returns_400` | Different-dim collections return 400 with "dimension" in the error |
| `query_dim_mismatch_returns_400` | Query length ≠ collection dim returns 400 |
| `decay_in_multi_search` | Decay path activates per-collection; `decay_factor` present in all hits |
| `metadata_filter_in_multi_search` | Filter is applied per-collection; only matching records survive the global merge |
| `single_collection_parity_with_regular_search` | Single-collection multi-search returns same record set as `POST /v1/search` |

## Findings

### BM25 reranking excluded intentionally

BM25 reranking changes each hit's `score` from a Squared L2 distance to a
hybrid score computed against a per-Collection term-frequency corpus. When
merging across Collections, these hybrid scores are incommensurable — a score
of 0.7 in Collection A means something different than 0.7 in Collection B,
depending on each corpus's IDF distribution. Applying reranking before the
merge would violate the Phase 5 spec's "score semantics: Squared L2, smaller =
better, no arbitrary normalization" invariant. BM25 reranking is left for
single-collection `POST /v1/search` where the score semantics are well-defined.

### Cluster path uses local reads

The cluster `cluster_multi_search` handler does not call
`ensure_read_consistency` per shard before searching. A multi-collection query
that spans multiple shards would need a linearizability check per shard — this
is non-trivial when the shards differ. The current implementation reads local
KernelState, which is correct but not linearizable. A single-shard deployment
(the default) is unaffected since all namespaces are on shard 0.

### Partial failures return the collection name

An earlier iteration of the server.rs fan-out closure lost the collection name
in the Err branch (it was moved into the Ok branch). The fix was to have each
future return `(name, Result<hits, err_str>)` — the name is always available
regardless of the search result.

### `engine.metadata` vs. `state.meta` in metadata filtering

`apply_metadata_filter` in `server.rs` reads from `engine.metadata` (the
in-memory MetadataStore sidecar). This store is populated by `set_meta_audited`
(called from `/v1/memory/meta/set`), NOT by `update_record_metadata` (called
from `PATCH /v1/records/:id/metadata`). The multi-search metadata test therefore
uses `/v1/memory/meta/set` with key `"rec:{id}"` — the same approach used by the
existing cluster namespace-isolation tests. This is a pre-existing design
choice, not introduced by Phase 5.

## Validation

```
cargo test -p valori-kernel            → 16 passed, 0 failed
cargo test -p valori-node --test multi_collection_search   → 10 passed, 0 failed
cargo test -p valori-node --test route_parity              → 2 passed, 0 failed
cargo test -p valori-node              → 70 passed, 0 failed
cargo build -p valori-node             → clean
```

## Follow-ups

| Item | Phase |
|---|---|
| Cluster multi-search: add per-shard linearizable read before searching | 5.1 or hardening sprint |
| BM25 reranking across collections with normalized scores (requires per-collection IDF calibration) | Future |
| Cross-collection GraphRAG: vector → multi-collection seed nodes → intra-collection subgraph expansion | Future |
| `max_per_collection` field to allow asymmetric oversampling per collection | Future |
| UI: cross-collection selection checkboxes in MultiSearch.tsx | Future |
