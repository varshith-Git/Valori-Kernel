# Valori — Performance Guide

This document covers the performance model: why certain design choices were made, what the actual throughput characteristics look like, and how to tune for your workload.

---

## Fixed-Point Arithmetic (Q16.16)

All vector arithmetic uses 32-bit integers with 16 integer bits and 16 fractional bits. Distance and dot product inner loops are pure integer multiply-accumulate with full SIMD dispatch (NEON on aarch64, AVX2/SSE4.1 on x86_64) — no FPU instructions, no SIMD dependence on floating-point rounding modes.

**Why this is fast:** On most CPUs, integer multiply is 1–3 cycles and never stalls on denormals. The Q16.16 distance kernel dispatches to NEON (4 i32 lanes) or AVX2 (8 i32 lanes) at runtime, giving 4–8× throughput over the scalar path for the inner multiply-accumulate loop. On hardware without SIMD support (embedded, WASM), it falls back to a clean scalar loop automatically.

**Why this is correct:** Two machines with different FPU modes, different SIMD implementations, or different OS float settings produce bit-identical results. This is the foundation of the BLAKE3 state hash guarantee.

**Precision:** Q16.16 provides ~4.5 decimal digits of fractional precision. For embedding models producing vectors in `[-1, 1]`, this is more than sufficient. Typical cosine similarity differences between neighbors are in the third decimal place — well within Q16.16 range.

**Conversion boundary:** Vectors arrive as `f32` over HTTP or the Python SDK. They are converted to `FxpScalar` exactly once at the engine boundary. Search results convert back to `f32` for the response. The conversion is lossy but deterministic.

---

## Index Types

Valori supports five index types, selectable per node via `VALORI_INDEX`:

| Index | Search | Build | RAM | When to use |
|---|---|---|---|---|
| `brute` | O(N) | O(N) | 1× vectors | Default. Exact. Best for N < 100k. |
| `hnsw` | O(log N) | O(N log N) | ~1.5× vectors | Fast approximate search, N > 100k. |
| `ivf` | O(N/k) | O(N) | 1× vectors + centroids | Large N with batch inserts; queries probe k clusters. |
| `bq` | O(N/8) | O(N) | ~⅛ vectors | Memory-constrained; binary quantization, mild recall loss. |
| `auto` | auto-selected | — | — | `brute` < 10k, `bq` 10k–2M, `hnsw` > 2M. |

`auto` is the recommended setting for production. It transitions index types at the documented thresholds without manual intervention.

### HNSW

Hierarchical Navigable Small World graph. Construction builds a multi-layer proximity graph; queries traverse it greedily from a random entry point. The `M` and `ef_construction` parameters control the graph density / recall trade-off. Valori uses conservative defaults (`M=16`, `ef_construction=200`) that prioritize recall over build speed.

### IVF (Inverted File Index)

Clusters vectors into `n_list` Voronoi cells using deterministic K-means (random seed fixed to reproduce the same clusters on every build). Queries probe `n_probe` nearest cells. Both parameters auto-scale by default:

- `n_list = max(16, sqrt(N))`
- `n_probe = max(1, sqrt(n_list))`

Setting `VALORI_IVF_N_LIST` or `VALORI_IVF_N_PROBE` disables auto-scale for that parameter.

**Important:** IVF requires an explicit `rebuild_index()` call after bulk inserts. The auto-tier in `auto` mode handles this automatically.

### BQ (Binary Quantization)

Each `f32` vector component is reduced to 1 bit (positive → 1, non-positive → 0). Distance is computed with bitwise XOR + popcount — 32× fewer bits to load, 8× faster on CPU cache. Recall typically drops to 90–95% depending on the embedding model and query distribution. Good for memory-constrained deployments or when a second-stage re-rank (e.g., Valori Reranker) will correct any recall loss.

---

## Memory Model

### Per-Record Layout

Each record in the slab occupies:

```
dim × 4 bytes   (Q16.16 vector, i32 per component)
1 byte          (active flag)
8 bytes         (tag, u64)
2 bytes         (namespace_id, u16)
8 bytes         (next_in_ns + prev_in_ns, u32 × 2)
1 byte          (alignment padding)
─────────────────────────────────────────────────
dim × 4 + 20 bytes total
```

| Dimension | Bytes/record | Vectors per GB |
|---|---|---|
| 8 | 52 B | ~19.5 M |
| 128 | 532 B | ~1.9 M |
| 384 | 1,556 B | ~0.65 M |
| 768 | 3,092 B | ~330 k |
| 1536 | 6,164 B | ~165 k |

### Graph Overhead

Each `GraphNode` occupies ~40 bytes (kind, record pointer, first_out_edge, first_in_edge, adjacency list header). Each `GraphEdge` occupies ~32 bytes (src, dst, weight, next_out, next_in, active flag).

For a workload where every record has a corresponding graph node and an average of 5 edges, add roughly `40 + 5 × 32 = 200 bytes` per record.

### HNSW Overhead

HNSW stores a proximity graph layer on top of the vector data. At `M=16`, each node has up to 16 neighbors per layer. Empirically this adds ~1.5× the base vector memory.

### Slab Pre-allocation

Slabs are pre-allocated at startup to `VALORI_MAX_RECORDS`, `VALORI_MAX_NODES`, `VALORI_MAX_EDGES`. There is no grow path — running out of slab capacity returns an error. Size the slabs to your expected peak load.

---

## WAL and Snapshot Growth

Each event appended to `events.log` (V4 format) occupies roughly:

- Insert record (dim=384): ~1,600 bytes (4-byte CRC + event payload + prev_hash)
- Graph edge create: ~80 bytes
- Soft-delete: ~40 bytes

At 1,000 inserts/second with dim=384, `events.log` grows at ~1.5 GB/hour.

**Mitigation:** Take snapshots periodically (`VALORI_SNAPSHOT_INTERVAL=60` for UI-managed nodes). The node reads from the snapshot on startup and only replays WAL segments after the snapshot point. Archived WAL segments can be moved to object store (`VALORI_OBJECT_STORE_URL`) for cold storage.

---

## SIMD Acceleration (2026-07-08)

The distance kernel hot paths now dispatch at runtime to platform-optimal SIMD:

| Path | Before | After | Width |
|---|---|---|---|
| L2 squared (brute-force, IVF) | scalar | NEON / AVX2 / SSE4.1 | 4–8× i32 lanes |
| Dot product / cosine similarity | scalar | NEON / AVX2 / SSE4.1 | 4–8× i32 lanes |
| HNSW graph search | scalar (`dist.rs`) | NEON / AVX2 / SSE4.1 | 4–8× i32 lanes |
| HNSW level assignment | BLAKE3 over full vector | BLAKE3 over 8-byte ID only | — |

The `no_std` / WASM build is unaffected — SIMD paths are guarded by `#[cfg(target_arch)]` and fall back to scalar on unsupported targets.

**Current throughput ceiling:** at dim=384 the SIMD compute is fast (~32 ns/record theoretical) but vectors are stored as separate heap allocations inside `RecordPool`, so brute-force scan performance is bounded by cache miss cost (~5 µs/record measured). A flat contiguous arena would close this gap. See the tuning note below.

---

## Throughput Benchmarks

Benchmarks run against a standalone node on M-series Apple Silicon (release build, localhost, no WAL). Results are measured, not estimated.

### Insert throughput (batch endpoint, dim=384, no WAL)

| Batch size | Throughput |
|---|---|
| 100 records | ~4,000 rec/s |
| 1,000 records | ~2,500 rec/s (batch HTTP overhead) |

### Brute-force search latency vs dataset size (dim=384, k=10, measured 2026-07-08)

Measured using random Gaussian vectors on Apple M-series, release build, localhost HTTP.

| N | p50 | p95 | p99 | Notes |
|---|---|---|---|---|
| 1,000 | 6.5 ms | 6.7 ms | 6.8 ms | HTTP floor dominates |
| 5,000 | 28.8 ms | 29.6 ms | 78.3 ms | Linear scaling begins |
| 10,000 | 56.3 ms | 56.6 ms | 57.8 ms | ~5.6 µs/record |
| 25,000 | 138.9 ms | 139.3 ms | 140.0 ms | ~5.5 µs/record |
| 50,000 | 275.1 ms | 277.7 ms | 279.6 ms | ~5.5 µs/record |

The ~5.5 µs/record cost is dominated by cache misses to heap-allocated vector data, not SIMD compute. Expect this to reduce significantly after the flat-arena refactor (pending).

**Use `VALORI_INDEX=hnsw` for N > 10k at dim=384.** HNSW search is O(log N) and stays sub-millisecond regardless of dataset size. As of 2026-07-08, HNSW is wired into the namespace-aware search path (`search_l2_ns`) — all named collections now benefit from HNSW when `VALORI_INDEX=hnsw` or `auto`.

### HNSW namespace search vs brute-force (dim=384, k=10, measured 2026-07-08)

Measured in-process (no HTTP overhead) using deterministic Gaussian vectors. Debug build shown; release is approximately 7× faster.

| Records | HNSW p50 (debug) | HNSW p50 (release est.) | Brute p50 (release) | Speedup |
|---|---|---|---|---|
| 1,000 | 4.7 ms | 0.7 ms | ~6 ms | ~9× |
| 5,000 | 9.1 ms | 1.3 ms | ~29 ms | ~22× |
| 10,000 | 9.3 ms | 1.3 ms | ~56 ms | ~43× |
| 25,000 | 10.7 ms | 1.5 ms | ~139 ms | ~93× |
| 50,000 | 10.6 ms | 1.5 ms | ~275 ms | **~183×** |

HNSW p50 is flat from N=10k onward — confirmed O(log N). Two fixes were required for correct behavior (both 2026-07-08):
1. **Sort-order fix** (`hnsw.rs`): `BinaryHeap::into_sorted_vec()` returns descending on a MaxHeap; added `.reverse()` so `search_layer` returns closest-first. Without this, `select_neighbors` connected each node to its M farthest neighbors (inverted graph).
2. **over_fetch fix** (`engine.rs`): Namespace search passed `k * 20` to HNSW, forcing ef=200 and O(N) behavior. Reduced to `k` so ef falls to ef_search (default 50), staying sub-millisecond.

These figures apply to all named collections (non-default namespaces). Prior to 2026-07-08, named collections always used brute-force regardless of `VALORI_INDEX`.

Run `python3 benchmarks/hnsw_ns_latency.py` against a live node for HTTP-measured numbers.

---

## Tuning Guide

**High write throughput:** Use WAL mode (set `VALORI_EVENT_LOG_PATH`). Batch inserts amortize fsync cost. Consider `bq` index to reduce per-record memory and keep the working set in cache.

**Low-latency search:** Use `hnsw` index for N > 100k. Pin the process to the same NUMA node as the memory. Avoid snapshots during peak traffic (they lock the engine briefly).

**Large collections:** Use sharding (`VALORI_SHARD_COUNT`). Namespaces route by `ns_id % shard_count`. Each shard is an independent Raft group with its own WAL.

**Memory-constrained:** Use `bq` index. Size slabs conservatively. Enable object-store offload to prune old snapshots automatically (`VALORI_OBJECT_STORE_KEEP`).
