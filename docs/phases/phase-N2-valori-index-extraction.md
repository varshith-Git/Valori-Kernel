# Phase N2 — valori-index extraction

## Goal

Extract all vector index structures from `valori-node/src/structure/` into a standalone `valori-index` crate with a single `VectorIndex` trait interface. Applies SOLID principles (ISP: narrow trait; SRP: one file per index kind; DIP: engine depends on trait, not concrete types).

## Delivered

| File | Change |
|------|--------|
| `crates/valori-index/Cargo.toml` | New crate — deps: `valori-kernel`, `serde`, `bincode`, `rustc-hash`, `tracing` |
| `crates/valori-index/src/lib.rs` | Crate root — module declarations + re-exports |
| `crates/valori-index/src/traits.rs` | `VectorIndex` trait + `l2_distance_sq` helper (moved from `structure/index.rs`) |
| `crates/valori-index/src/brute_force.rs` | `BruteForceIndex` — exact O(N) search + tests |
| `crates/valori-index/src/hnsw.rs` | `HnswConfig`, `HnswIndex` — NEON SIMD, deterministic level, Algorithm 4 heuristic |
| `crates/valori-index/src/ivf.rs` | `IvfConfig`, `IvfIndex` — Q16.16 centroids, NEON SIMD, auto-scale, thread-local scratch |
| `crates/valori-index/src/bq.rs` | `BqIndex` — two-stage Hamming coarse + L2 exact rescore |
| `crates/valori-index/src/quant/mod.rs` | `Quantizer` trait, `NoQuantizer`, `ScalarQuantizer` |
| `crates/valori-index/src/quant/pq.rs` | `ProductQuantizer`, `PqConfig` — PQ with Q16.16 codebooks |
| `crates/valori-index/src/deterministic/mod.rs` | Module declaration |
| `crates/valori-index/src/deterministic/kmeans.rs` | `deterministic_kmeans`, `f32_to_q16`, `l2_sq_q16` |
| `crates/valori-index/README.md` | Crate README with usage examples and scalability table |
| `Cargo.toml` | Added `valori-index` to workspace members + `workspace.dependencies` |
| `crates/valori-node/Cargo.toml` | Added `valori-index` dep |
| `crates/valori-node/src/lib.rs` | Removed `pub mod structure;` |
| `crates/valori-node/src/engine.rs` | All `crate::structure::*` → `valori_index::*` |
| `crates/valori-node/tests/deterministic_edge_tests.rs` | Updated imports to `valori_index::` |
| `crates/valori-node/tests/deterministic_ivf_tests.rs` | Updated imports |
| `crates/valori-node/tests/deterministic_kmeans_tests.rs` | Updated imports |
| `crates/valori-node/tests/deterministic_pq_tests.rs` | Updated imports |
| `crates/valori-node/tests/ivf_recall.rs` | Updated imports |
| `crates/valori-node/src/structure/` | **Deleted** (all 9 files, fully replaced by `valori-index`) |

## Findings

**Trait boundary clarified:** The original `VectorIndex` in `structure/index.rs` was a private module — tests had to go through `valori_node::structure::index::VectorIndex`. The public crate surface now lets integration tests import directly from `valori_index`, which is cleaner and avoids re-exporting internal modules.

**ScalarQuantizer was declared as a struct literal:** `ScalarQuantizer {}` required braces everywhere — replaced with a zero-field struct (no `{}` needed). Both forms compile; the new form matches `NoQuantizer`.

**`dist_neon` / `l2_sq_neon` are internally gated** by `#[target_feature(enable = "neon")]` — they compile on all platforms but only dispatch on aarch64.

**BQ snapshot is intentionally a no-op:** `BqIndex` stores only raw `f32` vectors; snapshot/restore return `Vec::new()` / `Ok(())`. The engine rebuilds BQ from the record pool after restore, same as BruteForce. This is correct because binarization is a deterministic function of the stored f32 values.

## Validation

```
cargo test -p valori-index   → 21 passed, 0 failed
cargo test -p valori-node    → all passed (route_parity + all integration tests)
cargo build -p valori-node   → 0 errors
```

## Follow-ups

- Phase N3: extract `valori-ingest` — `ingest.rs` + `embedder.rs` → new crate
- Phase N4: extract `valori-rag` — `graph_rag.rs` + `tree_rag.rs` + `community.rs`
- Phase N5: extract `valori-engine` — `engine.rs` + `commit/` (highest risk, do last)
- IVF benchmarks (currently `#[ignore]`) could be promoted to CI once a recall threshold is agreed on
