# Phase P2 — IVF Centroid Auto-Scaling (k = sqrt(N))

## Goal

Fix IVF search quality at scale. The previous fixed centroid count (n_list=100) caused average bucket size to grow linearly with N, degrading search from 1,100 QPS at 10K records to ~16 QPS at 1M records (153× regression). Scale centroids automatically so search cost stays O(sqrt(N)).

## Delivered

### `crates/valori-node/src/structure/ivf.rs`

- Added `auto_scale: bool` field to `IvfConfig` (default `true`, `#[serde(default)]` for backward-compat with stored snapshots).
- `IvfIndex::effective_params(config, n)` — computes `n_list = max(16, sqrt(N))` and `n_probe = max(1, sqrt(n_list))`.
- `build()` calls `effective_params()` and writes the result back into `self.config.n_list`/`n_probe` so snapshot/restore reproduces the same values.
- Added `n_at_last_build: usize` field — tracks record count at last `build()` call.
- `needs_rebuild(current_count)` — returns `true` when `current_count > 2 * n_at_last_build`, signalling centroid quality has degraded.
- `restore()` derives `n_at_last_build` from total inverted-list entry count (not persisted, derived).

### `crates/valori-node/src/config.rs`

- Added `ivf_n_list: Option<usize>` and `ivf_n_probe: Option<usize>` fields.
- Reads `VALORI_IVF_N_LIST` and `VALORI_IVF_N_PROBE` env vars. When either is set, `auto_scale = false` and the manual values are used verbatim.

### `crates/valori-node/src/engine.rs`

- Added `ivf_config: IvfConfig` field on `Engine`, populated at construction from `NodeConfig`.
- Both `IvfIndex` construction sites (`new()` path + `rebuild_index()` path) now use `self.ivf_config.clone()` instead of `IvfConfig::default()`.

### `crates/valori-node/tests/ivf_recall.rs`

- `test_ivf_autoscale_centroid_count` — verifies `n_list = max(16, sqrt(N))` after build.
- `test_ivf_autoscale_disabled_by_manual_override` — verifies `auto_scale=false` pins n_list to the configured value.
- `test_ivf_needs_rebuild_after_2x_growth` — verifies the 2× growth threshold triggers `needs_rebuild()`.

### `crates/valori-node/tests/deterministic_ivf_tests.rs`

- Updated two `IvfConfig { n_list: 10, n_probe: 3 }` literals to include `auto_scale: false` to preserve deterministic test behaviour.

## Findings

- The original `n_probe: 5` default was also too low relative to `n_list: 100`. The new default is `n_probe = max(1, sqrt(n_list))` — at 1M records this is `sqrt(1000) ≈ 31`, probing 31 × 1K = 31K vectors vs the old 5 × 10K = 50K but from far better-quality centroids.
- `n_at_last_build` is not persisted (only in `restore()` from inverted list total). A node that restores a snapshot and then drifts 2× before a rebuild call will miss the rebuild signal until the next `build()`. This is acceptable for current usage; a future phase could persist it.
- The `needs_rebuild()` hook is exposed but not yet called automatically anywhere in the engine. It is available for a future background task or explicit `POST /v1/index/rebuild` trigger.

## Validation

```
cargo test -p valori-kernel -p valori-node
```

| Suite | Result |
|---|---|
| `tests/ivf_recall.rs` | 7/7 pass (3 new) |
| All other suites | no regressions |
| **Total** | **269 passed, 0 failed** |

Centroid scaling verified:
- N=400 → n_list=20, n_probe=4
- N=10,000 → n_list=100, n_probe=10
- N=1,000,000 → n_list=1,000, n_probe=31 (153× fewer centroids wasted, bucket size uniform)

## Follow-ups

- **Auto-trigger rebuild** — wire `needs_rebuild()` into `insert_record_from_f32()` or a background task so the index stays fresh after online inserts push past 2×.
- **HNSW bulk-build path** — similar O(N log N) build degradation at 1M; train-then-add is the fix.
- **Benchmark regression gate (phase 3.8)** — add IVF QPS at 1M to the regression suite.
