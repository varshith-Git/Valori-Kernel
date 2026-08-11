# IVF / Binary Quantization Implementation Audit (S10)

Read-only audit of the real `valori-index` source, done before any S10
benchmark ran, per the instruction not to assume theoretical IVF/BQ
memory characteristics.

## IVF (`crates/valori-index/src/ivf.rs`)

- **Vector storage format**: `inverted_lists: Vec<Vec<(u32, Vec<i32>)>>`
  — each inverted list entry stores the record id **plus a full copy of
  the quantized (Q16.16, `i32`) vector**. This is a real, structural
  fact: IVF does **not** store only cluster assignments — it duplicates
  the vector data a second time, in addition to the record pool's own
  copy.
- **Index metadata**: `centroids: Vec<Vec<i32>>` (one `i32` vector per
  cluster, same Q16.16 representation) plus `n_at_last_build: usize`.
- **Auxiliary structures**: none beyond the above; no separate distance
  cache or precomputed norms.
- **Training/build requirement**: `build()` runs `deterministic_kmeans`
  over the full record set, 20 iterations, `n_list` clusters
  (auto-scaled to `max(16, sqrt(N))` unless `VALORI_IVF_N_LIST` is set).
  This is real k-means, not a stub.
- **Eager or lazy**: `insert()` (single-vector, used by the live insert
  path) does NOT trigger a rebuild — it just finds the nearest EXISTING
  centroid and appends. Centroids are only (re)computed by `build()`,
  which `Engine::rebuild_index()` calls — confirmed in the S8/S9 audits
  to run unconditionally on every restart (event-log recovery path).
  So: **inserts are lazy** (no reclustering per insert), but **every
  restart re-clusters from scratch** — real, measured cost, not a
  one-time setup cost.
- **Rebuild trigger**: `needs_rebuild()` returns true once
  `current_count > n_at_last_build * 2` — i.e., IVF's clustering goes
  stale as data grows and needs periodic rebuilding independent of
  restarts, which this phase's benchmarks (single insert pass, one
  restart) do not exercise.
- **Minimum vectors for meaningful behavior**: `n_list` auto-scales to
  `max(16, sqrt(N))` — at very small N (e.g. under ~256), `n_list=16`
  with very few vectors per cluster, meaning IVF degenerates toward
  scanning most of the dataset anyway. Not independently tested at
  degenerate small scale this phase (S9 already covered small-scale
  behavior where IVF looked identical to BruteForce/BQ).
- **Search**: `find_nearest_centroid` scans ALL centroids linearly (not
  itself indexed) — O(n_list) just to pick which list(s) to search, then
  scans `n_probe` lists' members exhaustively. Genuinely sub-linear in
  N for large N (touches roughly `n_probe/n_list × N` records), but with
  real per-query centroid-scan overhead that doesn't shrink.
- **Search approximate**: yes — only the selected `n_probe` clusters are
  searched; a true nearest neighbor in an unsearched cluster is missed.
- **Configurable params**: `VALORI_IVF_N_LIST`, `VALORI_IVF_N_PROBE`
  (both auto-scale when unset).
- **Persisted?**: no — confirmed same as every other index type, rebuilt
  from the record pool on every restart via `Engine::rebuild_index()`.
- **Memory-heavy operations**: `build()` allocates `centroids` +
  re-allocates the entire `inverted_lists` structure from scratch (a
  second full copy of every vector, transiently alongside the record
  pool's own copy during the rebuild).
- **Insert temporary memory spike**: no — single-vector insert is O(1)
  allocation (one `Vec<i32>` per inserted vector, appended to a list).
- **Search temporary memory**: `CENTROID_SCRATCH`/`CANDIDATE_SCRATCH`
  thread-locals are reused across calls (not reallocated per search) —
  genuinely low temporary overhead per query.

## Binary Quantization (`crates/valori-index/src/bq.rs`)

- **Vector storage format**: TWO parallel `HashMap<u32, _>` structures —
  `codes: HashMap<u32, Vec<u64>>` (the actual 1-bit-per-dimension binary
  codes, packed into `u64` words — genuinely compact, `dim/64` u64s per
  vector) AND `vectors: HashMap<u32, Vec<f32>>` (**the full, uncompressed
  f32 vector, stored a second time** — needed for the exact-L2 re-rank
  stage). This is the real, structural reason BQ is not memory-efficient
  in this implementation: it keeps the compact representation for the
  first-pass filter AND the full-precision original for the second pass.
- **Auxiliary structures**: none beyond the two HashMaps. `HashMap`
  itself carries real per-entry overhead (bucket/hash/tombstone
  bookkeeping) beyond a plain `Vec`, on top of the raw data duplication
  above — both contribute to real measured memory being higher than
  either IVF or BruteForce, not lower.
- **Training/build requirement**: none — binarization
  (`v > 0.0 → 1`) is a pure per-vector function, no fitting/calibration
  step, unlike IVF's k-means.
- **Eager/lazy**: fully eager and O(1) per insert — `insert()` computes
  the code and stores both representations immediately, no batching or
  deferred build step.
- **Persisted?**: no (same as every index type — `snapshot()`/
  `restore()` are both literal no-ops returning `Ok(())`/empty bytes;
  BQ is always rebuilt from the record pool, and rebuild is a simple
  linear re-binarization, no clustering — this predicts, correctly per
  the S10 measurement, that BQ recovery should be much faster than
  IVF's).
- **Search algorithm**: two-stage — stage 1 computes Hamming distance
  (XOR + popcount) against **every** stored code (`self.codes.iter()`
  — a full O(N) scan, just with a much cheaper per-comparison cost than
  float L2), keeps the top `max(10×k, 200)` candidates
  (`POOL_FACTOR=10`, `MIN_CANDIDATES=200`); stage 2 re-ranks those
  candidates with exact f32 L2 using the second (duplicated) vector
  store.
- **Search approximate**: yes, in two ways — the Hamming-distance
  ranking used to select stage-2 candidates is inherently lossy
  (binarization discards magnitude information), and only the top
  `candidates_cap` survive to the exact re-rank; a true top-k neighbor
  outside that candidate pool is permanently missed regardless of how
  good the re-rank is.
- **Configurable params**: none exposed via env var — `POOL_FACTOR`/
  `MIN_CANDIDATES` are compile-time constants.
- **Minimum vectors for meaningful behavior**: none structurally
  required (no clustering/training step) — but with a fixed
  `MIN_CANDIDATES=200` candidate pool, recall behavior at very small N
  (where 200 candidates ≈ the whole dataset) is trivially perfect;
  meaningful recall degradation only shows up once N meaningfully
  exceeds the 200-candidate pool, which the S9 10K-scale tests never
  reached (recall wasn't measured there) but this phase's 50K+ tests do.
- **Memory-heavy operations**: `build()` — clears and rebuilds both
  HashMaps from scratch on every restart (real, measured: `4.22s` at
  50K vectors — much faster than IVF's clustering rebuild since it's a
  pure linear pass with no iterative optimization).
- **Insert temporary memory**: none beyond the two per-vector
  allocations (`Vec<u64>` code + `Vec<f32>` clone) — no batching spikes.
- **Search temporary memory**: builds a full `Vec<(u32,u32)>` of ALL N
  Hamming distances every query (`self.codes.iter().map(...).collect()`)
  before truncating — a real, if modest, O(N) temporary allocation per
  search call that neither BruteForce nor IVF's search path has (IVF's
  candidate scratch is reused; BruteForce doesn't build an intermediate
  candidate list at all for a direct scan).

## What the code predicts vs. what was measured

The code audit predicted IVF ≈ 2× BruteForce memory (duplicated
vectors) and BQ ≈ 3×+ (duplicated codes+vectors, plus HashMap
overhead). **Measured 50K/384D results contradicted the IVF prediction**
(IVF's 345.9MB was nearly identical to BruteForce's 349MB, not ~700MB)
— see `benchmarks/capacity/results/s10-summary.md` for the full
comparison and an explicit note that this audit's structural prediction
did not hold at this scale, a genuine surprise worth flagging rather
than silently reconciling. **BQ's prediction held directionally** (404.7MB,
measurably higher than both BruteForce and IVF, consistent with real
duplicated storage), though not as dramatically as a naive 3× estimate.
