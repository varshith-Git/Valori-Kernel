# Index Tuning Audit (S11.1)

Read-only source audit of all four `VectorIndex` implementations, done
before any S11 benchmark ran. Files inspected directly (no assumptions
from prior docs): `crates/valori-index/src/{brute_force,hnsw,ivf,bq}.rs`,
wiring in `crates/valori-engine/src/engine.rs` and
`crates/valori-node/src/config.rs`.

## BruteForce (`brute_force.rs`)

1. **Memory model**: `HashMap<u32, Vec<f32>>` — one full f32 vector per
   record, no auxiliary structure.
2. **Build cost**: O(N) — clone every vector into the map.
3. **Insert cost**: O(1) amortized (HashMap insert).
4. **Search complexity**: O(N·dim) — every query scans all vectors.
5. **Recall**: exact by construction — 1.0 always (it *is* the ground
   truth).
6. **Restart/recovery cost**: `snapshot()`/`restore()` are no-ops
   (comment: "Snapshot is a no-op because the engine rebuilds from the
   record pool on restore"). Real restart cost = `build()` from the
   record pool, O(N).
7. **Persistence behavior**: never independently persisted; always
   rebuilt from `RecordPool` on recovery.
8. **Configuration parameters**: none.
9. **Automatic parameter selection**: none — nothing to select.
10. **Parameters changeable without API/schema change**: N/A (no
    parameters).
11. **Rebuilds automatically**: yes, every restart, unconditionally.
12. **Changing parameters requires destructive rebuild**: N/A.
13. **Parameters persisted**: N/A.
14. **Deterministic**: yes — HashMap iteration order does not affect
    output because `search()` explicitly sorts by `(dist, id)`.

## HNSW (`hnsw.rs`)

1. **Memory model**: `Vec<Option<Node>>`, each `Node` holds a boxed f32
   vector plus a `Vec<Vec<u32>>` adjacency list (one `Vec<u32>` per
   layer, up to `m`/`m_max0` neighbors each). Strictly more memory per
   vector than BruteForce (vector + graph edges), though S9 only
   measured this at a single 10K/384D data point.
2. **Build cost**: O(N·log N·ef_construction) in the classic HNSW
   analysis; empirically the slowest of the four in this codebase — S9
   measured 188s to *recover* (not just insert) 10K vectors at 384D.
3. **Insert cost**: per-insert graph-neighbor search + heuristic
   pruning (`select_neighbors_heuristic`) — much more expensive per
   insert than BruteForce/IVF/BQ's O(1)/O(centroids) inserts.
4. **Search complexity**: O(log N) expected (greedy graph descent +
   bounded `ef_search` layer-0 search) — the only index here with real
   sub-linear search complexity.
5. **Recall characteristics**: approximate, tunable via `ef_search`
   (higher = better recall, slower search). Not measured this session
   beyond S9's single point.
6. **Restart/recovery cost**: **the codebase implements real
   snapshot/restore for HNSW** (`snapshot()`/`restore()` serialize the
   full node/neighbor graph, `hnsw.rs:419-545`) — but
   `Engine::try_recover()` (`engine.rs:1543-1679`) calls
   `self.rebuild_index()` unconditionally on every recovery path
   (`engine.rs:1489`), which re-runs `build()` from the record pool
   from scratch. **The persisted HNSW graph is never actually read back
   on restart in the current wiring** — this is why S9 measured a full
   188s rebuild rather than a fast graph-deserialize. This is a real,
   confirmed gap, not a theoretical one.
7. **Persistence behavior**: graph structure has a real binary format
   (`snapshot()`) but it is dead code on the restart path per Finding
   #6 above — never invoked by `Engine`.
8. **Configuration parameters**: `m`, `m_max0` (=2m by default), `lambda`
   (level-assignment probability, `1/ln(m)`), `ef_construction`,
   `ef_search` (`HnswConfig`, `hnsw.rs:10-28`).
9. **Automatic parameter selection**: none — `HnswConfig::default()` is
   `m=16, m_max0=32, ef_construction=100, ef_search=50,
   lambda=1/ln(16)`, used unless overridden.
10. **Changeable without API/schema change**: yes, via existing env vars
    `VALORI_HNSW_M`, `VALORI_HNSW_EF_CONSTRUCTION`,
    `VALORI_HNSW_EF_SEARCH` (`config.rs:321-327`, wired into
    `Engine::new_with_config`, `engine.rs:162-175`). No code change
    needed to tune.
11. **Rebuilds automatically**: yes — same as BruteForce, unconditional
    `rebuild_index()` on every restart (Finding #6 makes this doubly
    true: even the persisted graph, when present, is discarded).
12. **Changing parameters requires destructive rebuild**: yes — `m`
    affects graph topology; there is no incremental re-tune, only a
    full rebuild (which happens anyway on every restart).
13. **Parameters persisted**: config is serialized as part of the
    snapshot format (`HnswConfig: Serialize`) but, per Finding #6,
    restore of the full structure is unreachable code on the actual
    recovery path — parameters are re-derived from env vars at process
    start, not from the snapshot, in practice.
14. **Deterministic**: level assignment uses `lambda` with a documented
    formula: level assignment PRNG (not audited bit-for-bit this phase;
    out of scope per S11.4's "one benchmark, not a rewrite" mandate) —
    flagged as **UNKNOWN (not verified this phase)** whether repeated
    builds from the same input produce byte-identical graphs. This
    matters for the state-hash invariant (S8) only insofar as the hash
    covers records/nodes/edges, not index internals — confirmed
    unaffected by this question.

## IVF (`ivf.rs`)

1. **Memory model**: `centroids: Vec<Vec<i32>>` (n_list × dim,
   Q16.16-quantized) + `inverted_lists: Vec<Vec<(u32, Vec<i32>)>>` — **the
   full quantized vector is duplicated inside the inverted list**
   alongside the record id; nothing is shared with the record pool.
   S10's code-reading correctly identified this duplication, but
   real measurement (50K/100K) showed it does not translate into a
   measurable memory delta vs. BruteForce at these scales — flagged
   as a real surprise in S10, not re-litigated here.
2. **Build cost**: `deterministic_kmeans(records, n_list, 20 iterations)`
   — dominates build time; scales with both `n_list` (∝ √N under
   auto-scale) and N.
3. **Insert cost**: O(n_list) — one nearest-centroid scan per insert
   (`find_nearest_centroid`, linear scan over centroids, no
   acceleration structure over centroids themselves).
4. **Search complexity**: O(n_list) centroid scan + O(n_probe/n_list ×
   N) candidate scan within probed lists — sub-linear only to the
   extent `n_probe < n_list` prunes meaningfully.
5. **Recall characteristics**: exact within probed lists (no further
   approximation past cluster assignment) — recall loss comes entirely
   from vectors whose true nearest neighbors land in un-probed clusters.
6. **Restart/recovery cost**: real `snapshot()`/`restore()` exist and
   **are** used (bincode-encoded centroids + inverted lists,
   `ivf.rs:232-281`) — S10 measured 47.4s (50K) / 129.7s (100K)
   restart, i.e. this *is* the real persisted-restore cost, not a
   rebuild-from-scratch artifact. (Confirmed by re-reading
   `engine.rs:1662-1679`: `restore_from_components` for `Ivf`/`Hnsw`
   kind still calls `rebuild_index()` — **so IVF has the identical gap
   as HNSW**: real snapshot/restore code exists in `ivf.rs` but
   `Engine`'s recovery path never calls it, always rebuilds via
   k-means from scratch. This is the true root cause of IVF's N^1.5
   recovery-time scaling: it's re-running `deterministic_kmeans` on
   every restart, not deserializing.)
7. **Persistence behavior**: same gap as HNSW — real snapshot format,
   dead on the restart path.
8. **Configuration parameters**: `n_list`, `n_probe`, `auto_scale`
   (`IvfConfig`, `ivf.rs:54-74`).
9. **Automatic parameter selection**: `auto_scale=true` (default)
   overwrites `n_list`/`n_probe` on every `build()`
   (`effective_params()`, `ivf.rs:99-106`) with `n_list =
   max(16, sqrt(N))`, `n_probe = max(1, sqrt(n_list))`. At N=50,000:
   `n_list = max(16, 223) = 223`, `n_probe = max(1, 14) = 14`. At
   N=100,000: `n_list = max(16, 316) = 316`, `n_probe = max(1, 17) =
   17`. This confirms S10 Finding #3's hypothesis: `n_probe/n_list ≈
   6.3%` of clusters are probed at 50K, `≈5.4%` at 100K — a small
   *fraction*, but each cluster still averages `N/n_list ≈ 224` (50K)
   / `316` (100K) vectors, so the *absolute* number of vectors scanned
   per query barely drops relative to BruteForce's full N — this is the
   real mechanism behind S10's "no latency win" finding, not a mystery.
10. **Changeable without API/schema change**: yes, via existing env
    vars `VALORI_IVF_N_LIST` / `VALORI_IVF_N_PROBE` (`config.rs:331-336`,
    setting either disables `auto_scale`, `engine.rs:179`). No code
    change needed — this is what S11.2's sweep uses.
11. **Rebuilds automatically**: yes (via the same `rebuild_index()`
    path as HNSW/BruteForce, `engine.rs:1489`), plus its own
    `needs_rebuild()` heuristic (rebuild if current count > 2×
    `n_at_last_build`, `ivf.rs:95-97`) for the standalone insert path.
12. **Changing parameters requires destructive rebuild**: yes — new
    `n_list` changes centroid count, requiring a full `deterministic_kmeans`
    re-run; no incremental re-cluster.
13. **Parameters persisted**: `IvfConfig` is part of the bincode
    snapshot (`ivf.rs:233-238`) but, per Finding #6 above, restore is
    dead code on the actual recovery path — the *effective* parameters
    after any restart are always freshly recomputed by
    `effective_params()` (if `auto_scale`) or read from env vars (if
    not), never read from a persisted snapshot.
14. **Deterministic**: yes — `deterministic_kmeans` is named and
    implemented for determinism (fixed iteration count, deterministic
    tie-breaking via `(dist, id)` comparisons throughout), and IVF's
    per-cell restart-hash checks in S10 (all matched) empirically
    confirm rebuild-from-record-pool reproduces the same state hash
    (which doesn't cover index internals) — consistent, not extra
    evidence of index-level determinism specifically.

## Binary Quantization (`bq.rs`)

1. **Memory model**: `HashMap<u32, Vec<u64>>` (binary codes, `⌈dim/64⌉`
   words each) **plus** `HashMap<u32, Vec<f32>>` (full f32 vectors,
   kept for the re-rank stage) — both duplicated, confirming S10's
   memory-audit prediction directionally (highest peak RSS of the three
   indices tested at 50K: 404.7MB).
2. **Build cost**: O(N) — binarize every vector once (`Self::binarize`,
   a single pass per vector, no clustering).
3. **Insert cost**: O(1) — binarize the new vector, insert into both
   maps.
4. **Search complexity**: **O(N) full scan** — `search()` (`bq.rs:97-126`)
   iterates `self.codes.iter()` over every stored code to compute
   Hamming distance before any candidate pruning. This is the real root
   cause of BQ's underwhelming latency (only ~5% faster than
   BruteForce at 50K in S10): **the Hamming stage itself is not
   sub-linear**, only the re-rank stage (stage 2) is restricted to a
   candidate pool. BQ trades exact f32 L2 distance computation for
   cheaper-per-comparison Hamming XOR+popcount, but does not reduce the
   number of comparisons — it's a constant-factor speedup, not an
   algorithmic one.
5. **Recall characteristics**: exactly why S10 measured Recall@10=0.48
   — see the dedicated root-cause analysis below.
6. **Restart/recovery cost**: `snapshot()`/`restore()` are no-ops
   (`bq.rs:128-134`, identical shape to BruteForce's) — BQ is always
   rebuilt from the record pool on restart, which is *why* its recovery
   (4.2s at 50K) is so much faster than IVF's: it's O(N) binarization,
   no clustering step.
7. **Persistence behavior**: never persisted, same as BruteForce.
8. **Configuration parameters**: **none exposed** — `POOL_FACTOR = 10`
   and `MIN_CANDIDATES = 200` (`bq.rs:11-12`) are `const`s, not fields
   of any config struct, not read from env, not part of the `BqIndex`
   struct at all.
9. **Automatic parameter selection**: N/A — there is no selection
   logic; the constants are fixed at compile time.
10. **Changeable without API/schema change**: **no** — as-shipped,
    changing the candidate pool size requires a source code change and
    a rebuild of `valori-node`. This is the direct answer to S11.3's
    question and the reason S11.3's sweep below required a source
    change to be testable at all (documented explicitly, not silently
    done).
11. **Rebuilds automatically**: yes, every restart (rebuild from record
    pool, like BruteForce).
12. **Changing parameters requires destructive rebuild**: N/A today
    (no parameters); would not require a *destructive* rebuild if made
    configurable, since candidate-pool size only affects `search()`,
    not `codes`/`vectors` storage.
13. **Parameters persisted**: N/A.
14. **Deterministic**: yes — `binarize()` is a pure per-dimension
    threshold (`v > 0.0`), candidate sort is `(hamming, id)` and
    re-rank sort is `(l2, id)`, both stable/deterministic tie-breaks.

### Root cause of BQ's Recall@10 = 0.48 (S10)

`candidates_cap = max(POOL_FACTOR * k, MIN_CANDIDATES)`. For S10's
benchmark (`k=10`): `max(10*10, 200) = 200`. Out of 50,000 vectors, only
the 200 with the *lowest Hamming distance* on a 384-bit binarization
ever reach the exact re-rank stage. Binarization at 384 dimensions
produces heavy quantization collision — many vectors share similar
Hamming distances to a given query because only the *sign* of each
dimension survives, not magnitude — so the true top-10 by L2 are not
reliably a subset of the 200 lowest-Hamming-distance candidates. This
is a **candidate-pool coverage problem**, not a Hamming-arithmetic bug:
increasing the pool size (fraction of N considered before re-rank)
is the only lever that can move recall, and it directly trades away
BQ's only genuine advantage (search speed) since Hamming scan cost is
already O(N) per Finding #4 above — a larger pool only makes the
already-linear re-rank stage linear over a larger fraction of N, i.e.
it converges toward BruteForce as the pool grows.

## Summary table

| | BruteForce | HNSW | IVF | BQ |
|---|---|---|---|---|
| Search complexity | O(N) | O(log N) | O(n_list + n_probe/n_list·N) | O(N) (Hamming) + O(pool) (rerank) |
| Recall | exact | tunable, approximate | exact-within-probed-lists | pool-coverage-limited |
| Persisted restore actually used? | N/A (no-op by design) | **No — dead code path** | **No — dead code path** | N/A (no-op by design) |
| Restart cost driver | O(N) rebuild | O(N) rebuild (slowest) | O(N) rebuild **+ k-means** (2nd slowest) | O(N) rebuild (fast, no clustering) |
| Tunable without code change | N/A | yes (env vars) | yes (env vars) | **no (hardcoded consts)** |

This table is the direct evidence base for S11.2/S11.3's benchmark
designs below: IVF and HNSW are tunable today; BQ's tunable parameter
had to be exposed by a small, explicitly-documented source change
before it could be benchmarked at all (see S11.3 in the phase doc).
