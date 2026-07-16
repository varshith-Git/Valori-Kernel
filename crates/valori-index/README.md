# valori-index

Vector index structures for the Valori platform.

Five implementations behind one uniform `VectorIndex` trait. No dependency on `valori-node`, no HTTP layer, no engine state — pure computation.

## Indexes

| Index | When to use | Notes |
|-------|------------|-------|
| `BruteForceIndex` | < 10 k records / correctness reference | Exact; O(N) search |
| `HnswIndex` | > 2 M records | NEON SIMD on aarch64; deterministic level assignment; Algorithm 4 heuristic pruning |
| `IvfIndex` | 10 k – 2 M records | Q16.16 centroids; auto-scale n_list/n_probe; NEON SIMD |
| `BqIndex` | 10 k – 2 M, RAM-constrained | Two-stage Hamming coarse + L2 exact; 1-bit per dimension |
| (all) | — | `Auto` mode (engine-side) tiers BruteForce → BQ → HNSW by record count |

## Quantizers

| Quantizer | What it does |
|-----------|-------------|
| `NoQuantizer` | Identity — stores full f32 as LE bytes |
| `ScalarQuantizer` | 8-bit per dimension; maps [-1,1] → [0,255] |
| `ProductQuantizer` | PQ with Q16.16 codebooks; deterministic build via `deterministic_kmeans` |

## Usage

```toml
[dependencies]
valori-index = { workspace = true }
```

### Search

```rust
use valori_index::{VectorIndex, HnswIndex};

let mut idx = HnswIndex::new();
idx.insert(1, &[0.1, 0.2, 0.3, 0.4]);
idx.insert(2, &[0.9, 0.8, 0.7, 0.6]);

let results = idx.search(&[0.1, 0.2, 0.3, 0.4], 1);
assert_eq!(results[0].0, 1); // (record_id, l2_distance)
```

### Snapshot / restore

```rust
// Snapshot to bytes (embed in a larger snapshot)
let bytes = idx.snapshot().unwrap();

// Restore into a fresh index
let mut idx2 = HnswIndex::new();
idx2.restore(&bytes).unwrap();
```

### IVF with auto-scaling

```rust
use valori_index::{IvfIndex, IvfConfig};

let mut idx = IvfIndex::new(IvfConfig::default(), 128);
// build() computes n_list = max(16, sqrt(N)), n_probe = max(1, sqrt(n_list))
idx.build(&records);
```

### Deterministic K-Means

```rust
use valori_index::deterministic_kmeans;

let centroids = deterministic_kmeans(&records, 64, 20);
// bit-identical on x86 / ARM / WASM — FNV seed + Q16.16 i64 arithmetic
```

## Design invariants

- **One trait, all indexes.** `VectorIndex` is the only public interface. Concrete types are exposed for config access only.
- **No kernel dependency on hot paths.** `deterministic_kmeans` calls `valori_kernel::math::l2::l2_sq_i32` for the scalar fallback; everything else is pure.
- **Determinism.** HNSW level assignment uses FNV hash; K-Means seeding uses FNV + Q16.16; IVF tie-breaking uses integer comparisons only.
- **NEON SIMD.** Both HNSW (`dist_neon`) and IVF (`l2_sq_neon`) have `#[target_feature(enable = "neon")]` kernels with scalar fallbacks — correct on all platforms.

## Scalability notes

| Operation | HNSW | IVF | BQ |
|-----------|------|-----|----|
| Insert | O(log N) amortized | O(n_list) centroid scan | O(dim/64) |
| Search | O(ef × m × log N) | O(n_probe × N/n_list) | O(N/64) coarse + O(k × POOL_FACTOR × dim) |
| Build | O(N log N) | O(N × iters × n_list) kmeans | O(N × dim/64) |
