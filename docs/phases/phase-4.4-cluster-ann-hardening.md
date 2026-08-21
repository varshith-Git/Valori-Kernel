# Phase 4.4 — Cluster ANN Hardening, Consistency Contract, Recovery, and Observability

## Goal

Harden the Phase 4.3 Cluster ANN implementation against correctness hazards
that exist in production but not in the happy-path test: stale build activation
(a slow gen-8 build activating after gen-9 was committed), FAILED retry storms,
collection deletion during a build, watcher drop-path bugs, and missing
observability. Formalize the cluster consistency contract in code and docs.

## Delivered

### `crates/valori-node/src/cluster_server.rs`

| Change | Detail |
|---|---|
| `ClusterCollectionIndex` made `pub` | Required so `ClusterHandle` can hold the shared `Arc<RwLock<HashMap<u16, ClusterCollectionIndex>>>`. |
| `last_build_started_at: Option<std::time::Instant>` added | Tracks when the most recent build attempt started; drives the 60-second FAILED-retry debounce. |
| **Stale-build detection** in `trigger_local_build` | After `spawn_blocking` returns, re-reads `sm.get_meta_json(&idx_spec_key(ns_id))` to check whether the desired generation still matches `generation`. If the desired gen advanced (or the collection was deleted), the just-built index is discarded and `mark_failed` is called so the watcher can trigger a fresh build. |
| **FAILED retry debounce** in `trigger_local_build` | Before starting a retry for a previously-failed generation, checks `last_build_started_at`. If < 60 s have elapsed the trigger is a no-op; this prevents a tight retry loop when a build fails repeatedly (e.g. bad configuration, OOM). |
| **Watcher drop-path fix** in `check_and_trigger_pending_builds` | When `desired = None` (SetMeta(null) committed via Raft), the watcher now clears both `building_generation` and `active_generation`, not just the active pointer. This ensures a "drop while building" scenario leaves the local index fully reset. |
| **Single `list_namespaces()` call** in `check_and_trigger_pending_builds` | Previously the cleanup loop and the build-trigger loop each called `list_namespaces()`. Now one call snapshots the set at the top of the function and both loops reuse it. |
| **Prometheus metrics** (7 new) | See `telemetry.rs` section below. All metric call sites use the `metrics` 0.21 macro syntax (`increment_counter!`, `histogram!`, `gauge!`). |
| **`IndexOps::get_index_state` augmented** | Always populates `state.desired` from `sm.get_meta_json` (authoritative Raft state) before returning. Followers that haven't built locally yet now correctly report `desired_type` in `GET /v1/namespaces/{name}/index`. |
| **Search handler** | No-decay path calls `state.record_ann_search_fallback(ns_id)` when falling back to brute-force; emits `valori_cluster_ann_search_fallback_total`. |

### `crates/valori-node/src/cluster.rs`

| Change | Detail |
|---|---|
| `ClusterHandle::cluster_indexes` field added | `Arc<tokio::sync::RwLock<HashMap<u16, ClusterCollectionIndex>>>` — the shared ANN index map, initialised to empty at bootstrap. `build_cluster_router_with_keys` now clones this Arc instead of creating a fresh map, so multiple router instances (tests, production startup) share the same index lifecycle state. |

### `crates/valori-node/src/telemetry.rs`

Seven new Prometheus metric descriptions (all Phase 4.4, all in the "Cluster ANN
index lifecycle" block):

| Metric | Kind | Purpose |
|---|---|---|
| `valori_cluster_ann_build_started_total` | counter | Each call to `trigger_local_build` that passes all gates |
| `valori_cluster_ann_build_completed_total` | counter | Builds that reached ACTIVE (not stale, not failed) |
| `valori_cluster_ann_build_failed_total` | counter | Builds that failed (not stale-discarded) |
| `valori_cluster_ann_build_duration_seconds` | histogram | Wall-clock build time, labels: collection |
| `valori_cluster_ann_generation_active` | gauge | Currently active generation number, labels: collection |
| `valori_cluster_ann_stale_activation_skipped_total` | counter | Builds that succeeded but were discarded due to stale-detection |
| `valori_cluster_ann_search_fallback_total` | counter | Search requests that fell back to brute-force because no ANN was active |

### `crates/valori-node/src/routes/index_lifecycle.rs`

Module docstring updated to reflect Phase 4.3 cluster support and the shared
handler pattern. No API changes.

### `crates/valori-node/tests/cluster_ann_hardening.rs` (new file)

9 integration tests using the single-node cluster (`bootstrap_cluster`, node_id=1)
which exercises the full Raft code path (SetMeta goes through Raft, not bypassed):

| Test | What it proves |
|---|---|
| `watcher_triggers_build_after_raft_commit` | SetMeta committed → build triggers → status reaches ACTIVE |
| `search_uses_ann_when_active` | Once ACTIVE, ANN search and brute-force agree on top-k elements |
| `search_falls_back_when_no_ann` | No index committed → search still works via brute-force |
| `drop_index_clears_local_state` | SetMeta(null) committed → status reaches NONE → search continues |
| `status_api_reports_desired_from_raft` | Response to `start_build` contains correct `desired_type` from Raft |
| `failed_build_does_not_corrupt_collection_state` | Build lifecycle transition doesn't affect collection records or searchability |
| `collection_recreation_does_not_inherit_old_index` | Drop + recreate same name → new ns_id → no old index visible |
| `successive_requests_are_handled_safely` | HNSW gen 1 → IVF gen 2 → final state is IVF ACTIVE at gen ≥ 2 |
| `index_lifecycle_does_not_affect_graph_state` | Building and activating an ANN index doesn't touch graph edges |

## Findings

### Root cause of "build not activating" in tests (fixed)

`build_cluster_router` created a fresh `cluster_indexes` HashMap on every call.
In the production server, the router is built once, so this was invisible.
In tests — where each HTTP request creates a new router — every request saw an
empty index map, so `trigger_local_build` from one request wrote state that the
next request's polling call could never see. Fixed by moving `cluster_indexes`
to `ClusterHandle` (initialised at bootstrap) and cloning the Arc in
`build_cluster_router_with_keys`.

### Equidistant vector tie-breaking (BQ vs exact)

`search_uses_ann_when_active` originally compared ordered top-3 IDs. With
uniform-value vectors and a query equidistant from two records, BQ tie-breaking
differed from exact L2. Fixed by comparing result *sets* rather than ordered
lists — the test still proves the ANN returns the correct top-k elements.

### `resolve_namespace(None)` in stale-build detection

The collection-existence check in `trigger_local_build` called
`sm.resolve_namespace(None)` (which resolves the default namespace, not the
target ns_id). This was harmless because the `list_namespaces()` fallback
correctly checks the target ns_id, but it was redundant. Left as-is; noted here
for future cleanup.

## Validation

```
cargo test -p valori-kernel     → 16 passed, 0 failed
cargo test -p valori-node --test cluster_ann_hardening    → 9 passed, 0 failed
cargo test -p valori-node --test cluster_search_namespace_isolation → 7 passed, 0 failed
cargo test -p valori-node --test cluster_api              → 8 passed, 0 failed
cargo test -p valori-node --test route_parity             → 2 passed, 0 failed
cargo fmt --check (valori-node, valori-engine)            → clean
cargo clippy -p valori-node -p valori-engine --all-targets → no errors
```

## Follow-ups

| Item | Phase |
|---|---|
| `resolve_namespace(None)` in stale-build check is redundant — only `list_namespaces()` is needed | 4.5 or cleanup |
| Watcher background task is not started in `build_cluster_router`; follower auto-build relies on the production startup path calling `start_cluster_ann_watcher`. Tests poll manually instead. A production integration test (3+ nodes) would cover follower auto-build. | Future P2 |
| Per-collection ANN metrics currently label by ns_id (u16 string), not collection name. A reverse-lookup from the SM would make dashboards more readable. | 4.5 or observability sprint |
