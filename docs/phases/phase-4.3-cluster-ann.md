# Phase 4.3 — Cluster ANN

## Goal

Remove the 501 "not supported" limitation on `POST/GET /v1/namespaces/{name}/index` in
cluster mode. Each Raft node now builds and activates its own node-local ANN index
(HNSW, IVF, or BQ) while the authoritative collection configuration (desired spec +
generation) is replicated through the existing Raft log via `KernelEvent::SetMeta`.

## Delivered

### `crates/valori-node/src/cluster_server.rs`

| Change | Detail |
|--------|--------|
| `ClusterCollectionIndex` struct | Holds `CollectionIndexState` (for API responses) and `Option<Box<dyn VectorIndex + Send + Sync>>` (the live index). NOT Raft state. |
| `cluster_indexes` field on `DataPlaneState` | `Arc<RwLock<HashMap<u16, ClusterCollectionIndex>>>` — per-node, non-replicated. |
| `idx_spec_key(ns_id)` | Returns `"__valori_idx_spec:{ns_id}"` — the SetMeta key for the desired spec. |
| `snapshot_records_for_build(ns_id)` | Reads searchable records from the local KernelState in `(record_id, f32_vec)` form for index construction. |
| `trigger_local_build(ns_id, gen, spec)` | Sets `state.next_generation = gen` before calling `start_build()` so the allocated generation id equals the Raft-replicated one; spawns a `spawn_blocking` task that builds HNSW/IVF/BQ and activates on success, marks failed on error. |
| `check_and_trigger_pending_builds()` | Watcher method: iterates all known namespaces, reads the replicated desired spec from `SetMeta`, starts a local build if the local state is behind. Also cleans up indexes for dropped collections. |
| `try_ann_search(ns_id, query_f32, k)` | Tries the node-local active ANN index; returns `None` to fall back to exact brute-force. |
| Background watcher task | Spawned once in `build_cluster_router_with_keys`; polls `check_and_trigger_pending_builds` every 5 s so followers pick up new index builds automatically. |
| `IndexOps` impl for `DataPlaneState` | Wires `create_or_change_index` / `get_index_status` shared handlers to cluster machinery: `resolve` reads replicated namespace registry; `start_build` commits `SetMeta` through Raft then triggers local build; `drop_index` commits `SetMeta("null")` and clears local index; `supports_ann_builds()` returns `true`. |
| `cluster_index_lifecycle_create` / `_status` | Now delegate to shared `index_lifecycle::create_or_change_index` / `get_index_status` — 501 is gone. |
| `search` handler | Tries `try_ann_search` before `shard_search_ns` on the no-decay path. Falls back transparently. |
| `cluster_index_config` | Reports actual `VALORI_INDEX` env var instead of hardcoded `"brute_force"`; adds note about per-collection endpoints. |
| `cluster_index_rebuild` | Updated note points to `POST /v1/namespaces/{name}/index`. |

### `crates/valori-node/src/routes/index_lifecycle.rs`

- Updated module doc comment and `IndexOps` trait doc to reflect Phase 4.3 cluster support.

## Activation model decision

**Node-local activation** (Option A) was chosen over cluster-coordinated activation:

- The underlying `KernelState` is byte-identical on all nodes (Raft guarantee).
- ANN indexes are acceleration structures derived from that state — they are
  approximate by design. Temporary result divergence between nodes during a
  partial-activation window is acceptable; the base data is always correct.
- Node-local activation eliminates a cluster-wide coordination round-trip on the
  critical path. A node that fails to build its index silently falls back to
  exact brute-force; the collection is never unavailable.
- Cluster-coordinated activation would require a new Raft command type and a
  distributed barrier, adding complexity without improving correctness.

## Findings

- `CollectionIndexState.next_generation` must be set to the Raft-replicated
  generation before calling `start_build()` to ensure node-local generation ids
  don't diverge from the cluster-wide generation. The manipulation is deliberate
  and documented in `trigger_local_build`.
- `IndexState` was imported but not directly referenced by name in the module;
  removed it to keep the import list clean.
- The 5-second watcher interval means followers pick up a new index spec within
  ≤5 s of the leader committing it. This is acceptable for a background build
  operation; the spec itself is immediately available via Raft reads.

## Validation

```
cargo build -p valori-node                     → Finished (0 errors)
cargo fmt --check -p valori-node               → Clean
cargo clippy -p valori-node                    → 0 errors, 0 new warnings
cargo test -p valori-kernel                    → 16 passed, 0 failed
cargo test -p valori-node --lib                → 27 passed, 0 failed
cargo test -p valori-node --tests              → all test results ok (0 failed)
```

Parity test `v1_route_sets_match_between_standalone_and_cluster` passes — the
index lifecycle endpoints are registered in both routers.

## Follow-ups

- **Phase 5**: Cross-collection ANN search (not implemented here per spec).
- **Index artifact persistence**: Standalone persists built ANN index artifacts to
  disk via `StorageProvider`. Cluster does not yet persist them — after a node
  restart, the watcher re-triggers the build. A future phase should store the
  artifact in the object store keyed by `(collection_name, generation)` and restore
  it on startup to skip the rebuild cost.
- **Build status fan-out**: `GET /v1/namespaces/{name}/index` on the leader returns
  the leader's node-local state. A future phase could aggregate status across peers
  via the management API to show which nodes are behind.
- **Decay-path ANN**: The search handler's decay reranking path still uses exact
  brute-force. ANN results could feed the decay pool with minimal changes.
