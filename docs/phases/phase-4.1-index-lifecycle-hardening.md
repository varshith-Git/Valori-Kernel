# Phase 4.1 — Harden and Complete Standalone Mutable Index Lifecycle

## Goal

Close the eight gaps left open by Phase 4: make the active index generation survive
a restart (via durable artifact), wire IVF and HNSW build parameters correctly, and
add GC for retired generations. The result: a standalone node no longer blindly
rebuilds every ANN index from scratch on every restart.

## Delivered

### `crates/valori-storage/src/collection_manifest.rs`
- Added three optional Phase-4.1 fields to `CollectionManifest`:
  - `active_index_generation: Option<u32>` — which generation is currently ACTIVE
  - `active_index_type: Option<String>` — the artifact's algorithm name (`"hnsw"`, `"ivf"`, `"bq"`)
  - `active_index_base_lsn: Lsn` — WAL position at which the artifact was written
- All three carry `#[serde(default)]` for backward-compat with Phase-4 manifests (no schema-version bump required — additive change, no reader breaks).
- `CollectionManifest::new()` initialises all three to `None`/`Lsn::ZERO`.

### `crates/valori-engine/src/engine.rs`
- **`finish_index_build`** (step 6, new): after atomically activating the new generation, writes artifact bytes (`put_immutable(IndexArtifact{...})`), durably publishes `active_index_generation / type / base_lsn` to the manifest, and GCs the retired generation's artifact via `delete()`.  Best-effort: a storage failure logs a warning but does not fail the build (index is already serving).
- **`drop_collection_index`**: clears the manifest's index fields (`do_clear_manifest_index_fields`) and deletes the artifact file (`do_delete_index_artifact`).
- **`try_restore_index_artifacts(recovered_lsn)`** (new method): manifest-driven startup recovery.  For each namespace, reads its `CollectionManifest`, checks `active_index_generation` + `active_index_base_lsn`:
  - `base_lsn == recovered_lsn` → load artifact bytes, `restore()`, install (no rebuild).
  - `base_lsn < recovered_lsn` → artifact is stale; rebuild from `KernelState` records.
  - artifact missing / corrupt → rebuild from `KernelState` records (logged warning).
  - BQ → always rebuild (BQ snapshot returns empty bytes).
- **`try_recover`** (StorageProvider path): replaced `self.rebuild_index() + sync_collection_indexes_from_state()` with `self.try_restore_index_artifacts(highest_lsn.0)`.
- Private helpers: `read_collection_manifest`, `write_manifest_index_fields`, `do_write_manifest_index_fields`, `do_clear_manifest_index_fields`, `do_delete_index_artifact`.

### `crates/valori-node/src/server.rs` — `IndexOps::start_build`
- **IVF parameters**: parse `spec.parameters["n_list"]` / `["n_probe"]` from the build spec; if present, build `IvfConfig { auto_scale: false }` with the user values; otherwise fall back to auto-scale (`sqrt(N)` heuristic, `auto_scale: true`).
- **HNSW parameters**: parse `spec.parameters["m"]` / `["ef_construction"]` / `["ef_search"]`; if `m` is provided, also set `m_max0 = 2 * m`; otherwise use `HnswConfig::default()`.

### `crates/valori-node/tests/index_artifact_persistence.rs` (new)
7 integration tests exercising the artifact round-trip via the Engine API directly (no HTTP):
  - `artifact_hnsw_roundtrip` — build HNSW, snapshot, restart, verify restored from artifact
  - `artifact_ivf_roundtrip` — same for IVF
  - `artifact_missing_falls_back_to_rebuild` — delete artifact file before restart → brute-force fallback
  - `stale_artifact_triggers_rebuild` — insert after build → base_lsn < current → rebuild
  - `hnsw_explicit_parameters_accepted` — build with m/ef params, verify search works
  - `ivf_explicit_parameters_accepted` — build with n_list/n_probe, verify search works
  - `drop_index_clears_manifest` — drop index → manifest active_index_generation cleared

### `python/tests/test_index_lifecycle.py` (new)
11 Python SDK tests (no live node):
  - 8 sync tests: `collection_index_status` URL/response, `create_collection_index` minimal/parameterised/URL, `change_collection_index` alias, `drop_collection_index` payload/URL
  - 3 async tests: status URL, create payload, drop payload

## Findings

1. **`recover_project_from_storage` calls `configure_namespace(..., index_kind=0)`**: after
   StorageProvider recovery, all entries in `state.namespace_configs` have `index_kind=0`
   (BruteForce).  The original `try_restore_index_artifacts` used that value, so all
   collections appeared as BruteForce and no artifacts were loaded.  Fixed by making
   `try_restore_index_artifacts` manifest-driven: reads `active_index_type` and
   `desired_index` from the manifest instead of `namespace_configs.index_kind`.

2. **Tuple access on `Vec<(u32, IndexState, IndexGeneration)>`**: `IndexGeneration` fields
   (`generation`, `spec`) are on the third element (`.2`), not top-level.  Fixed field
   access in `finish_index_build` and `drop_collection_index`.

3. **BQ cannot be artifact-persisted**: `BqIndex::snapshot()` returns `Ok(Vec::new())`.
   Phase 4.1 explicitly skips artifact writing/loading for BQ; restarts rebuild from
   `KernelState` records (fast for BQ).

4. **`snapshot_collection` already does read-then-update**: preserves the new index fields
   added by `finish_index_build`.  No changes needed there.

## Validation

```
cargo test -p valori-kernel       → 61 passed (all pre-existing tests green)
cargo test -p valori-storage      → 78 passed
cargo test -p valori-engine       → 18 passed
cargo test -p valori-node --test index_lifecycle           → 12/12 passed
cargo test -p valori-node --test index_artifact_persistence → 7/7 passed
cargo test -p valori-node --test collections               → 27/27 passed
python3 -m pytest python/tests/test_index_lifecycle.py     → 11/11 passed
```

Total new tests added: 7 (Rust integration) + 11 (Python SDK) = **18 tests**.

## Follow-ups

- **Phase 4.2**: UI for index lifecycle (build progress, status badge, drop action).
- **Phase 4.3**: Cluster ANN — `cluster_server.rs` currently returns 501 for index endpoints;
  cluster mode needs a Raft-routed build path.
- **WAL catch-up on stale artifacts**: currently, a stale artifact (`base_lsn < current_lsn`)
  triggers a full rebuild.  A future optimisation: load the artifact, apply only the
  post-build WAL events (requires the WAL segments to be available after recovery).
- **BQ artifact persistence**: `BqIndex` has no serialisable state today.  A future version
  could encode the bit-signature matrix and restore it without rebuilding.
- **Cluster proof/timeline** still reads shard 0's log only (pre-existing gap, not introduced here).
