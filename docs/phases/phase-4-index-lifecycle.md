# Phase 4 — Mutable Collection Index Lifecycle

## Goal

Turn collection indexes from a static, node-wide configuration switch into a
real lifecycle-managed derived subsystem. Users can create, replace, or drop
ANN indexes per collection without downtime; the active index continues to
serve searches while a new one builds in the background.

## Delivered

### New files

| File | Purpose |
|------|---------|
| `crates/valori-engine/src/index_manager.rs` | Index lifecycle types: `IndexState`, `IndexSpec`, `IndexGeneration`, `CollectionIndexState`, `IndexBuildRequest`, `IndexStatusResponse` |
| `crates/valori-node/src/routes/index_lifecycle.rs` | Shared handler module: `IndexOps` trait + `create_or_change_index` + `get_index_status` shared handlers |
| `crates/valori-node/tests/index_lifecycle.rs` | Integration test matrix — 12 tests covering all spec scenarios |

### Modified files

| File | Change |
|------|--------|
| `crates/valori-engine/src/lib.rs` | Added `pub mod index_manager` + re-exports |
| `crates/valori-engine/src/engine.rs` | Added `index_states: HashMap<u16, CollectionIndexState>` field; 6 new methods: `index_state`, `start_index_build`, `finish_index_build`, `fail_index_build`, `drop_collection_index`, `snapshot_records_for_ns` |
| `crates/valori-node/src/routes/mod.rs` | Added `pub mod index_lifecycle` |
| `crates/valori-node/src/server.rs` | `IndexOps` impl for `SharedEngine` with async background build; route `/v1/namespaces/:name/index` (POST + GET) |
| `crates/valori-node/src/cluster_server.rs` | Cluster handlers returning honest `501 Not Implemented` for builds; `200 none` for status; same route registered |
| `python/valoricore/remote.py` | `_SyncIndexMixin` + `_AsyncIndexMixin` — added `collection_index_status`, `create_collection_index`, `change_collection_index`, `drop_collection_index` |

## Findings

### Design decisions

- **Desired vs active distinction**: the spec was clear. `CollectionIndexState` carries both `desired` (what the user asked for) and `active_generation` (what is actually serving). These differ while a build is in-flight.

- **Concurrent build rejection (409)**: `start_index_build` checks `is_building()` and returns `EngineError::InvalidInput` before allocating a generation; the handler maps this to a 409 Conflict.

- **WAL catch-up in `finish_index_build`**: borrow checker conflict resolved by splitting into explicit steps — extract `base_lsn` from `index_states` first (immutable), then collect catch-up events from `self.persistence` (immutable), then apply them to `new_idx` (no engine lock), then mutate `collection_indexes` and `index_states` (write lock re-acquired by the caller).

- **Background build pattern**: identical to snapshot encoding — `tokio::spawn` wraps `tokio::task::spawn_blocking` so heavy k-means clustering doesn't block the async runtime.

- **Cluster path returns 501**: the spec explicitly calls this out. Rather than silently claiming brute-force is HNSW, the cluster path returns a clear error with context. This is honest and will be fixable in a future phase.

- **Graph test required `collection` parameter**: Phase 3.3 removed the "default" namespace auto-creation. All graph node operations now require an explicit `collection=` query parameter (GET) or `"collection"` field (POST). The test was updated accordingly.

### Bugs found during testing

- **Graph test wrong field names**: integration test initially used `"label"` (not a real field), `"from_id"`/`"to_id"` (actual fields are `"from"`/`"to"`), and response `"id"` (actual field is `"node_id"`). Fixed in the test.

- **Concurrent test timing sensitivity**: with 5 records, HNSW builds in ~1 ms — faster than the time between two sequential HTTP requests. Fixed by using `tokio::join!` to fire both requests truly concurrently, which guarantees they contend at the engine write lock.

## Validation

### Test matrix results (12/12 passing)

| Test | Status |
|------|--------|
| `none_to_hnsw` | ✅ |
| `none_to_ivf` | ✅ |
| `none_to_bq` | ✅ |
| `hnsw_to_none` | ✅ |
| `hnsw_to_ivf` (replacement with active serving during build) | ✅ |
| `unknown_type_rejected` (400) | ✅ |
| `concurrent_build_rejected` (409) | ✅ |
| `insert_during_build` (WAL catch-up covers new records) | ✅ |
| `collection_a_build_does_not_affect_b` | ✅ |
| `index_change_preserves_graph` | ✅ |
| `two_collections_different_dims` (4-dim + 8-dim simultaneously) | ✅ |
| `index_on_unknown_collection_returns_404` | ✅ |

### valori-kernel: 61 passed, 0 failed
### valori-node: all test files pass, 0 failed

Route parity test: 2/2 — `/v1/namespaces/:name/index` registered on both standalone and cluster routers.

### Manual smoke test

```bash
# Create a collection and build HNSW
curl -X POST http://localhost:3000/v1/namespaces \
  -H 'Content-Type: application/json' \
  -d '{"name":"docs","dimension":4,"metric":"squared_l2"}'

# Insert some vectors
curl -X POST http://localhost:3000/records \
  -d '{"values":[1,0,0,0],"collection":"docs"}'

# Start HNSW build (returns 202 immediately)
curl -X POST http://localhost:3000/v1/namespaces/docs/index \
  -d '{"type":"hnsw"}'

# Poll until active
curl http://localhost:3000/v1/namespaces/docs/index
# → {"collection":"docs","active_type":"hnsw","status":"active",...}

# Drop the index (revert to exact search)
curl -X POST http://localhost:3000/v1/namespaces/docs/index \
  -d '{"type":null}'
```

## Follow-ups

| Item | Notes |
|------|-------|
| **UI panel** | Index management panel for the desktop app — create/change/drop dialog, status polling badge (BUILDING → ACTIVE), per-collection index card. Spec §31. |
| **Cluster ANN** | The cluster path returns 501. Full cluster ANN requires the index to be rebuilt identically on all peers after Raft commit — a non-trivial coordination problem. Future phase. |
| **GC of retired generations** | `IndexState::Retiring` entries accumulate. A GC pass (on snapshot, on timer, or on next build) should drop them once they are no longer needed for recovery. |
| **Persistence across restart** | `index_states` (the lifecycle state) and `collection_indexes` (the actual index objects) are both in-memory. On restart, `ensure_collection_index` rebuilds the index synchronously at collection creation time and marks it ACTIVE. The lifecycle history (generations, build timestamps) is not persisted — a restart resets to a single ACTIVE generation with a new generation id 0. |
| **IVF `n_list` override via parameters** | The `parameters` field is deserialized but currently ignored for IVF — the handler always uses `max(16, sqrt(N))`. A follow-up should wire `parameters.n_list` and `parameters.n_probe` through to the build. |
| **Python SDK payload tests** | The SDK methods were added but not covered by the existing `python/tests/` suite. Add sync + async round-trip tests for `collection_index_status`, `create_collection_index`, and `drop_collection_index`. |
