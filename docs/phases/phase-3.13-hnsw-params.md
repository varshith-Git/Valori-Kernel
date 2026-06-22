# Phase 3.13 — HNSW parameter exposure

## Goal

Expose HNSW index parameters (`M`, `ef_construction`, `ef_search`) via environment variables so operators can tune recall/throughput trade-offs without recompiling, and provide a `GET /v1/index/config` endpoint to introspect the active index type and parameters.

## Delivered

### `crates/valori-node/src/structure/hnsw.rs`

- Added `ef_search: usize` field to `HnswConfig` (default `50`).  
  Previously the search beam width was the hardcoded literal `k.max(50)`.
- Added `HnswIndex::new_with_config(config: HnswConfig) -> Self` constructor.
- Added `HnswIndex::config() -> &HnswConfig` accessor.
- Search beam width now uses `k.max(self.config.ef_search)` — user-supplied floor is respected.

### `crates/valori-node/src/config.rs`

Three new optional fields on `NodeConfig`:

| Field | Env var | Default |
|---|---|---|
| `hnsw_m` | `VALORI_HNSW_M` | `16` |
| `hnsw_ef_construction` | `VALORI_HNSW_EF_CONSTRUCTION` | `100` |
| `hnsw_ef_search` | `VALORI_HNSW_EF_SEARCH` | `50` |

When `VALORI_HNSW_M` is set, `m_max0` and `lambda` are derived automatically (`m_max0 = 2*M`, `lambda = 1/ln(M)`).

### `crates/valori-node/src/engine.rs`

- `Engine::new()` constructs `HnswIndex::new_with_config(hnsw_cfg)` using the config from `NodeConfig`.
- Added `hnsw_config: HnswConfig` field to `Engine` struct — stored at construction time so `rebuild_index()` and the index-config endpoint can access it without re-reading env vars.
- `Engine::rebuild_index()` now passes `self.hnsw_config.clone()` instead of `HnswConfig::default()`.

### `crates/valori-node/src/server.rs`

- New route: `GET /v1/index/config`
- Handler `index_config_handler` returns:
  ```json
  // BruteForce
  {"index_type": "brute_force", "hnsw": null}

  // HNSW
  {"index_type": "hnsw", "hnsw": {"m": 16, "m_max0": 32, "ef_construction": 100, "ef_search": 50}}
  ```

### `crates/valori-node/src/cluster_server.rs`

- Same route wired in: `GET /v1/index/config`
- Cluster mode always returns `"index_type": "brute_force"` with a note explaining that the cluster data plane uses `KernelState`'s brute-force search for linearizable consistency.

### `python/valoricore/remote.py`

- `SyncRemoteClient.get_index_config()` — GET `/v1/index/config`, returns dict.
- `AsyncRemoteClient.get_index_config()` — async equivalent.

### `crates/valori-node/tests/api_index_config.rs` (NEW)

5 tests:
- `brute_force_config_returns_correct_type` — `hnsw` field is null for brute-force
- `hnsw_default_config_returns_defaults` — m=16, m_max0=32, ef_construction=100, ef_search=50
- `hnsw_custom_m_derives_m_max0_and_lambda` — m=8 → m_max0=16
- `hnsw_custom_ef_search_is_reflected` — ef_search=200, ef_construction unchanged at 100
- `hnsw_all_params_set` — m=32, m_max0=64, ef_construction=400, ef_search=100

## Findings

- **`lambda` must be re-derived when M changes** — the level distribution parameter `lambda = 1/ln(M)` is hardcoded in `HnswConfig::default()` for M=16. When an operator sets `VALORI_HNSW_M`, `lambda` must be recomputed or the probabilistic level distribution will be wrong (biased toward M=16's distribution regardless of the actual M). Fix: `lambda` is recalculated in `Engine::new()` whenever `hnsw_m` is set.
- **`rebuild_index()` previously used `HnswConfig::default()`** — after a crash-recovery restore, the index is rebuilt via `rebuild_index()`. With the old code this would silently drop operator-supplied M and ef values. Fixed by storing `hnsw_config` on `Engine`.
- **Cluster path has no HNSW** — the cluster data plane applies events through `ValoriStateMachine → KernelState` which has its own brute-force L2 search. The HNSW index is a standalone-node feature. Returning a clear `"note"` in the cluster response avoids operator confusion.

## Validation

```
cargo test -p valori-kernel -p valori-node
229 passing, 0 failing
```

Manual smoke test (standalone node):
```bash
VALORI_INDEX=hnsw VALORI_HNSW_M=8 VALORI_HNSW_EF_CONSTRUCTION=200 VALORI_HNSW_EF_SEARCH=100 \
  valori-node &
curl http://localhost:3000/v1/index/config
# → {"index_type":"hnsw","hnsw":{"m":8,"m_max0":16,"ef_construction":200,"ef_search":100}}
```

## Follow-ups

- **Phase 4.3** — Disk-mode HNSW (`usearch` FFI or mmap redb): will add new params (`max_elements`, `mmap_path`) exposed via the same `/v1/index/config` endpoint.
- **ef_search per-query override** — operators may want to lower ef_search at query time for latency-critical paths. Could be added as a `?ef` query param on `/search`. Deferred — no use-case yet.
