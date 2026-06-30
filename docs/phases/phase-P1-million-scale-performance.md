# Phase P1 — Million-scale performance & snapshot scalability

## Goal

Make Valori correct and fast at 1M records: eliminate the snapshot
`CapacityExceeded` ceiling, guarantee no WAL loss on clean teardown, accelerate
the L2 hot path with SIMD, and publish a reproducible benchmark suite so the
performance story is grounded in our own numbers rather than borrowed ones.

## Delivered

### Snapshot encoder — growable `Vec` (the headline fix)
- **`crates/valori-kernel/src/snapshot/encode.rs`** — rewrote `encode_state`
  from a fixed `&mut [u8]` slice (returning a byte count) to a growable
  `&mut alloc::vec::Vec<u8>` that grows on demand. `CapacityExceeded` from the
  encoder is now **structurally impossible** at any record count or dimension.
  All `write_*` helpers replaced with infallible `push_*` helpers.
  Stays `no_std` — uses `alloc::vec::Vec`, verified against
  `wasm32-unknown-unknown`.
- New `encode_capacity_hint(state)` — V6-correct pre-allocation estimate
  (`28 + dim×4` per record slot, plus nodes/edges/namespace-head arrays) so the
  `Vec` avoids repeated reallocation on the hot path.
- **Root cause fixed:** V6 added 10 bytes/record (`namespace_id` + `next_in_ns`
  + `prev_in_ns`) but the old buffer-size formula in `engine.rs` still counted
  `18 + dim×4` — a 10 MB underestimate at 1M records that tripped the fixed
  buffer.

### Callers updated to the new encoder API
- **`crates/valori-node/src/engine.rs`** — `snapshot()` encodes into a hinted
  `Vec`; broken size formula removed.
- **`crates/valori-consensus/src/state_machine.rs`** — `encode_kernel()` uses
  the hint; the now-unnecessary 1 GB cap guard removed.
- **`crates/valori-node/src/cluster_server.rs`** — `encode_cluster_snapshot()`
  replaces the hardcoded `vec![0u8; 1 << 20]` (1 MB — far too small) with the
  growable path.
- Tests rewired: `snapshot_roundtrip.rs`, `format.rs`, `valori-cli`
  `integration_test.rs`.

### WAL flush on teardown (durability)
- **`crates/valori-node/src/engine.rs`** — added `impl Drop for Engine` to flush
  buffered WAL entries via `EventCommitter::flush_pending()` when the engine is
  dropped.
- **`crates/valori-node/src/events/event_commit.rs`** — added
  `impl Drop for EventCommitter` (idempotent flush) and made `into_parts()`
  flush-then-decompose safely via `ManuallyDrop` (can no longer move out of a
  `Drop` type directly). Fixes `test_event_log_recovery_preserves_hash` and
  `test_proof_hash_stable_through_event_log_recovery`, which previously found 0
  events after a "crash" because the batched write buffer was never flushed.

### SIMD L2 distance
- **`crates/valori-kernel/src/math/l2.rs`** — `l2_sq_i32` now dispatches to
  NEON (aarch64, `vmull_s32` widening to i64) and AVX2 (x86_64), with a scalar
  fallback. Widens to i64 for the horizontal sum to avoid overflow at `dim > 512`.
  **Determinism preserved** — every path returns the identical integer value;
  SIMD is a speedup only.

### Benchmark suite
- **`benchmarks/local_perf.py`** (new) — B1 single insert, B2 batch insert,
  B3 search-at-scale, B4 index comparison, B5 dimension impact, B6 snapshot
  timing, B7 batch-size sweet spot. Flags: `--quick`, `--million`, `--dim`,
  `--out`.
- **`benchmarks/RESULTS_1M.md`** (new) — captured run at dim=128, release build.
- **`README.md`** — full performance section with per-model tables and two
  operational warnings: (1) `insert_batch` < 100 vectors is slower than a single
  insert loop; (2) **HNSW is mandatory above 50K records**.
- **`python/valoricore/local.py`** — docstrings document index selection
  thresholds and batch-size guidance.

## Findings

- **The fixed-buffer snapshot design was a latent schema-evolution trap.** Any
  field added to the record without a matching bump to the size formula in
  *three* separate call sites silently re-introduced `CapacityExceeded`. The
  growable `Vec` removes the whole class of bug — this is what every production
  serializer (protobuf, bincode, RocksDB SST, Qdrant) does.
- **`Engine` had no `Drop`.** WAL batching (buffer + periodic fsync) was added
  earlier for throughput, but nothing flushed the tail buffer on scope exit, so
  a clean shutdown could lose up to `flush_every` events. Only surfaced because
  the recovery test dropped the committer mid-test.
- **Two `.so` copies in the dev venv.** `valoricore_ffi/valoricore_ffi.abi3.so`
  *and* a bundled `valoricore/valoricore_ffi.abi3.so`; the package imports the
  bundled one first. Updating only the top-level copy left stale code loaded and
  produced a confusing "fix doesn't work" loop. Both must be replaced when
  hand-installing a freshly built wheel.
- **IVF degrades 153× from 10K→1M** (1,100 → 16 QPS) without centroid tuning —
  deferred (see Follow-ups).

## Validation

- `cargo test -p valori-kernel -p valori-node` → **266 passed; 0 failed.**
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` → clean
  (`no_std` invariant holds after the encoder rewrite).
- End-to-end 1M-record snapshot (dim=128, bruteforce, via FFI `LocalClient`):

  | Records   | `snapshot()` | size    | `save_snapshot()` | `restore()` |
  | --------- | ------------ | ------- | ----------------- | ----------- |
  | 100,000   | 41 ms        | 51.5 MB | —                 | —           |
  | 300,000   | 208 ms       | 154 MB  | —                 | —           |
  | 500,000   | 403 ms       | 257 MB  | —                 | —           |
  | 1,000,000 | 1,200 ms     | 515 MB  | 1,174 ms          | 2,666 ms (1M restored) |

  Previously every snapshot above ~250K records failed with
  `Kernel(CapacityExceeded)`.

## Follow-ups

- **IVF centroid scaling** — scale `k_centroids ≈ sqrt(N)` so IVF stops
  collapsing to 16 QPS at 1M. (Future perf phase.)
- **HNSW bulk-build** — current build is ~4.4 min for 1M (train-then-add path
  would cut this dramatically). (Future perf phase.)
- **Version bump to 0.2.4 + CHANGELOG promotion** — once IVF/HNSW build work
  lands, cut the release entry.
