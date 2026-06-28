# Phase I7 — Metadata Filtering

## Goal

Add server-side metadata filtering to `/search` so callers can restrict vector search results to records whose stored metadata satisfies a JSON predicate — exact equality for strings/booleans, and range operators (`gt`, `gte`, `lt`, `lte`, `eq`) for numeric fields. Both standalone and cluster execution paths must be covered.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/api.rs` | Added `metadata_filter: Option<serde_json::Map<String, serde_json::Value>>` to `SearchRequest`; added public `matches_metadata_filter()` helper and `value_matches()` with range-operator support |
| `crates/valori-node/src/server.rs` | Added `apply_metadata_filter()` helper; standalone `/search` handler post-filters hits using `engine.metadata.get("rec:{id}")` for both no-decay and decay paths; over-fetches `k×10` candidates (capped 5000) when filter is set |
| `crates/valori-node/src/cluster_server.rs` | Added `metadata_filter` field to cluster `SearchRequest`; cluster search handler post-filters after vector search and after decay reranking using `state.metadata.get("rec:{id}")` for both paths |
| `python/valoricore/remote.py` | Added `metadata_filter: Optional[Dict[str, Any]] = None` to `SyncRemoteClient.search()` and `AsyncRemoteClient.search()` with full docstring; `ClusterClient` and `AsyncClusterClient` inherit via `**kwargs` |

## Filter semantics

```python
# Exact equality (string, bool, null, number)
c.search(q, k=5, metadata_filter={"author": "Alice"})

# Numeric range
c.search(q, k=5, metadata_filter={"year": {"gte": 2020, "lte": 2024}})

# Combined — ALL keys must match
c.search(q, k=5, metadata_filter={"author": "Alice", "year": {"gt": 2019}})
```

The filter is applied as a post-filter after vector search. Records whose metadata key is absent are excluded. Over-fetching (`k×10`, max 5000) compensates for filtered-out candidates.

## Findings

- Records without metadata (key absent from `MetadataStore`) are always excluded when a filter is present — this is the correct safe default.
- The filter only operates on the node-local `MetadataStore`, which is intentionally not Raft-replicated (advisory metadata). This is consistent with the existing `set_metadata` endpoint behaviour.
- Combining `rerank=True` with `metadata_filter` disables BM25 reranking to avoid double over-fetching complexity; pure vector order is used, then filtered.

## Validation

- `cargo test -p valori-kernel -p valori-node`: **259 tests passed, 0 failed**
- `cargo build -p valori-kernel --target wasm32-unknown-unknown`: clean (kernel untouched)
- Manual smoke test: insert records with `{"author": "Alice"}` metadata, search with filter → only Alice's records returned

## Follow-ups

- **Index-backed filtering** (Phase future): for workloads where the filter excludes >90% of records, a pre-filter inverted index on metadata keys would avoid scanning the full candidate pool. Deferred — post-filter covers the common case.
- **Array/nested field matching**: currently only flat JSON objects are filtered. Nested object or array membership (e.g. `{"tags": {"contains": "rag"}}`) is deferred.
