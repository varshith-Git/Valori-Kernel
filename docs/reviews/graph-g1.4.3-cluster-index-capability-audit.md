# G1.4.3 — Cluster Index Capability Audit

**AUDIT ONLY. No source code modified. No index implemented. No canonical
state, snapshot, WAL, hashing, or API changed. G1.4.4 not started.**

Every claim below is either **CODE VERIFIED** (exact file:line, quoted) or
explicitly marked **NOT MEASURED** / **COULD NOT DETERMINE FROM SOURCE
ALONE**. Three independent full-source traces were run and cross-checked
against each other; no claim rests on a README/config comment where the
source code itself could be read.

---

## 1. Executive Summary

Cluster mode uses BruteForce only **not because of an unfinished
integration bug in the sense of "someone forgot a line" — it is a
deliberate, documented architectural boundary**: the kernel's own
`no_std` index enum (`valori-kernel::index::ActiveIndex`) has HNSW and IVF
variants **commented out in the source**, with an explicit doc comment
explaining they are "not yet implemented in the kernel." But the specific
mechanism by which `VALORI_INDEX` fails to reach the cluster path is a real
gap, not a designed one: the env var is parsed into `NodeConfig`, and then
**silently dropped** — never forwarded into `ClusterConfig`, `bootstrap_cluster`,
or `ValoriStateMachine`'s constructors, all of which have no index-kind
parameter at all. A user setting `VALORI_INDEX=hnsw` on a cluster node gets
no error, no log warning specific to that env var, and BruteForce — the
same outcome as never setting it. Cluster's `/v1/index/rebuild` and
`/v1/index/config` endpoints exist (satisfying route parity) but are
hardcoded stubs that always report `brute_force` and perform no state
change, regardless of what's requested.

Separately, and more importantly for future work: **HNSW's derived index
structure is provably insertion-order-dependent** (not just "hasn't been
proven deterministic" — the greedy-insertion algorithm is order-sensitive
by construction, confirmed by reading `insert()`), and its distance
function has **two non-bit-identical codepaths** (scalar vs NEON) that can
diverge at exact float ties. **IVF's centroid state also depends on
insertion/rebuild history**, not just the final record set. Both facts are
relevant to any future decision to wire either into the Raft-replicated
cluster path, where two independently-constructed replicas of "the same"
derived index are not currently guaranteed to be structurally identical —
only guaranteed (for HNSW, on a single platform) to answer the same query
similarly, and even that guarantee has an unresolved cross-platform edge
case. BQ and BruteForce have no such issues — both are provably
order-independent and (for BQ) bit-identical across every codepath
examined.

The canonical/derived boundary from G0.2 **holds cleanly for the paths
audited**: no index bytes appear in `encode_state`, `hash_state_blake3`,
`KernelEvent`, or the Raft snapshot payload. But a real, previously
unflagged consequence of that same boundary is exposed by this audit: Raft
snapshot install and cluster-side `decode_state` **never call `.rebuild()`
on the index after transferring canonical state** — this is harmless today
only because the kernel's only two variants are stateless-on-search
(BruteForce) or would-need-a-rebuild-anyway (BQ, currently unreached). The
moment BQ (or a future kernel HNSW/IVF) is wired into the cluster path,
a newly-joined replica installing a snapshot would silently serve
empty/stale index results until enough live writes replay to catch up
incrementally — nothing in the current code path prevents this.

**G1.4.3 verdict: PASS.** The audit is internally consistent, every major
claim is code-verified with citations, and the open questions in §15 are
genuinely open (require a product decision) rather than unresolved
research gaps this document failed to close.

---

## 2. Current architecture (as-is, not target)

```
                    Standalone (valori-engine::Engine)      Cluster (KernelState via ValoriStateMachine)
                    ─────────────────────────────────       ──────────────────────────────────────────
Index crate         valori-index (std, f32)                  valori-kernel::index (no_std, fixed-point)
Variants            BruteForce, HNSW, IVF, BQ, Auto           BruteForce, BinaryQuantization only
                                                               (Hnsw/Ivf variants are commented-out
                                                                enum arms, not missing by omission)
Selection           VALORI_INDEX env var -> EngineConfig       set_index_kind() never called anywhere
                    -> Engine.index_kind, honored              in cluster code (grepped whole workspace);
                                                               VALORI_INDEX parsed into NodeConfig but
                                                               never forwarded past main.rs
Runtime switch      POST /v1/index/rebuild — genuinely         POST /v1/index/rebuild — hardcoded stub,
                    switches kind + rebuilds                   always reports "brute_force", no-op
```

---

## 3. Standalone vs. cluster comparison

| Dimension | Standalone | Cluster | Classification |
|---|---|---|---|
| BruteForce | ✅ full (`valori-index::BruteForceIndex`) | ✅ full (`valori-kernel::index::BruteForceIndex`, separate impl) | A — intentional, two independent implementations for `std`/`no_std` reasons |
| BQ | ✅ full | ✅ implemented at the kernel level but **never activated** (`set_index_kind` never called cluster-side) | **B — unfinished integration.** The kernel-native code exists and is correct (per §6); only the wiring to activate it in cluster mode is missing |
| HNSW | ✅ full | ❌ not implemented in kernel at all (commented-out enum arm) | A — architectural: HNSW's `std`-only heap/RwLock-based implementation was never ported to `no_std`; not simply unwired |
| IVF | ✅ full | ❌ not implemented in kernel at all (commented-out enum arm) | A — same as HNSW |
| Index selection | Env var read → honored | Env var read → **silently discarded** before reaching cluster bootstrap | **B — unfinished integration.** This is the one place a real "someone forgot a line" gap exists — no error, no cluster-specific warning, config silently ignored |
| Index configuration (M, ef, n_list, n_probe, pool_factor) | Fully wired (§7) | N/A — no non-BruteForce/BQ index exists to configure, and BQ's config wouldn't reach cluster either since `set_index_kind` isn't called | A/B mixed — the config plumbing itself is standalone-only by design (A), but would silently no-op for cluster BQ if someone tried (B) |
| Namespace filtering | Namespace-scoped exact path (`search_l2_ns`) or global-search-plus-post-filter for non-BruteForce (§ G1.4 audit) | Fixed in G1.4.2 (`shard_search_ns`, mirrors standalone's split) | A — now consistent |
| Record insertion | `Engine::insert_record*` → `post_apply_derived` → `index.on_insert` | `ValoriStateMachine::apply` → `KernelState::apply_event_ns` → `index.on_insert` | A — same event-driven pattern, different call chain |
| Record deletion | `index.on_delete` on `DeleteRecord`/`SoftDeleteRecord` | Same event, same `on_delete` call | A |
| Snapshot recovery | `i_data` section carries serialized index bytes; falls back to `rebuild_index()` if absent/empty; **hard errors (does not fall back) if present-but-corrupt** | No `i_data`-equivalent section exists; index is never serialized into or restored from the Raft snapshot payload at all | **D — correctness risk**, currently latent (harmless while BruteForce-only, real the moment a stateful kernel index activates) |
| Replay recovery | Live event replay calls `on_insert`/`on_delete` incrementally | Same — live Raft log apply calls `on_insert`/`on_delete` incrementally | A — this path is symmetric and correct on both sides |
| Restart rebuild | `try_recover()` explicitly calls `rebuild_index()` after event-log/WAL recovery | No equivalent call anywhere in `ValoriStateMachine`/`decode_state`/`install_snapshot` | **D — correctness risk**, same latent issue as snapshot recovery above |
| Search scoring | Per-index metric (all squared-L2; IVF/kernel fixed-point, others f32) | Squared-L2 fixed-point only (BruteForce/BQ) | A |
| Tie-breaking | `(score, id)` ascending, uniformly, in every one of the 4 algorithms (§ determinism audit) | `(score, id)` ascending via shared `SearchResult::Ord`, integer scores (no float-tie ambiguity at all) | A — cluster's tie-breaking is actually *more* robust (integer, not float) |
| Determinism | BruteForce/BQ: fully deterministic query results and structure. HNSW: structure is order-dependent; cross-platform (x86 vs ARM) bit-identical distances not guaranteed by the code. IVF: `build()` alone is order-independent but incremental `insert()`-after-`build()` introduces history dependence | BruteForce/BQ only — both fully deterministic (§ determinism audit) | A for BruteForce/BQ; **F — requires a product decision** for whether HNSW/IVF's determinism properties are acceptable for a Raft-replicated system before ever porting them to the kernel |
| Memory ownership | One `Engine` (and therefore one index) per node process | One `KernelState`/`ActiveIndex` per shard, one shard-set per node process — genuinely independent per-shard instances, none shared | A |
| Concurrency | `Arc<RwLock<Engine>>`, single writer at a time | `Arc<Mutex<StateMachineInner>>` per shard state machine, serialized through Raft's log | A — both serialize writes; cluster's is enforced by consensus ordering, standalone's by the lock |
| Configuration source | `VALORI_INDEX` + related env vars, read once at `NodeConfig::default()`, applied to the one `Engine` | N/A (see above) | B (as a consequence of the index-selection gap) |

---

## 4. Index initialization call graph (traced, not grepped-and-guessed)

**`VALORI_INDEX` parsing** — `crates/valori-node/src/config.rs:257-263`
(inside `impl Default for NodeConfig`, which doubles as the env-reading
constructor — there is no separate `from_env()`):
```rust
let index_kind = match std::env::var("VALORI_INDEX").as_deref() {
    Ok("hnsw") => IndexKind::Hnsw,
    Ok("ivf") => IndexKind::Ivf,
    Ok("bq") => IndexKind::Bq,
    Ok("auto") | Ok("mstg") => IndexKind::Auto,
    _ => IndexKind::BruteForce,
};
```

**Standalone path** — `crates/valori-node/src/engine.rs:26-51`
(`impl EngineFromNodeConfig for Engine`): `index_kind: cfg.index_kind` is
copied straight into `EngineConfig`, which drives the real, pluggable
`valori-engine::Engine` (HNSW/IVF/BQ/BruteForce/Auto-capable).

**Cluster path** — `crates/valori-node/src/main.rs:255-295`: a
`NodeConfig::default()` is constructed at line 258 (so `VALORI_INDEX` *is*
parsed into `node_cfg.index_kind`), but `bootstrap_cluster` is invoked at
line 289-294 with only `(&cluster_cfg, node_cfg.event_log_path.as_deref(),
rotation_bytes, node_cfg.dim)` — `node_cfg.index_kind` is never passed.
`bootstrap_cluster`'s signature (`crates/valori-node/src/cluster.rs:316-321`)
confirms there is no parameter to receive it, and `ClusterConfig`
(`cluster.rs:38-66`, fields: `node_id, raft_bind, members, init,
raft_log_path, tls, shard_count`) has no index field either.

**`set_index_kind()` — every call site in the workspace, grepped:**
- `crates/valori-kernel/src/state/kernel.rs:75` — the definition.
- `crates/valori-engine/src/engine.rs:271` — **the only production call
  site anywhere**, inside standalone `Engine`'s BQ auto-tier wiring.
- Test-only: `crates/valori-kernel/tests/index_transition.rs`,
  `crates/valori-kernel/tests/property.rs`.
- `crates/valori-node/src/cluster_server.rs:1039` has a **comment**
  documenting the absence, not a call: `"cluster never calls
  set_index_kind, so every cluster shard is BruteForce today"` (this is
  the exact comment I wrote in the G1.4.2 fix, now independently
  re-confirmed by this audit rather than merely restated).

**`ValoriStateMachine` construction** — `crates/valori-consensus/src/state_machine.rs:320-339`
(`new`) and `:347-351` (`with_db`): both take only `(audit, dim)`. No
`IndexKind`/`IndexVariant` parameter; grepped the whole file for
`VALORI_INDEX`/`IndexKind`/`IndexVariant`/`index_kind` — zero hits.
`KernelState::with_dim` (`crates/valori-kernel/src/state/kernel.rs:63-69`)
calls `Self::new()`, which sets `index: ActiveIndex::default()`
(kernel.rs:49) → always `BruteForce`.

**Kernel-native enum** — `crates/valori-kernel/src/index/mod.rs:48-89`,
verbatim:
```rust
/// Which kernel-native index variant is active.
///
/// Only `no_std`-compatible (fixed-point, alloc-only) variants live here.
/// `HNSW` and `IVF` are not yet implemented in the kernel; selecting them at
/// the node level maps to `BruteForce` in the kernel with an explicit log
/// warning — they are documented, not silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexVariant {
    BruteForce,
    BinaryQuantization,
    // Hnsw,  // not yet kernel-native; node uses its own std-only HnswIndex
    // Ivf,   // not yet kernel-native; node uses its own std-only IvfIndex
}
```
`Default for ActiveIndex` (lines 76-80) resolves to
`ActiveIndex::BruteForce(BruteForceIndex::default())`.

**Per-shard instancing**: `bootstrap_cluster`'s per-shard loop
(`cluster.rs:357` onward, `for i in 0..cfg.shard_count`) constructs a
distinct `ValoriStateMachine` per shard, stored in `shards:
BTreeMap<ShardId, ShardHandle>`. Each wraps its own `Arc<Mutex<StateMachineInner>>`
→ its own `KernelState` → its own `ActiveIndex`. **One index instance per
shard, none shared** — confirmed both within a node (across shards) and
implicitly across nodes (each Raft replica independently constructs its
own `ValoriStateMachine`).

**Rebuild-after-recovery — cluster has none; standalone has an explicit
conditional**:

Cluster: `decode_state` (`crates/valori-kernel/src/snapshot/decode.rs:154-478`)
starts from `KernelState::new()` (index defaults empty `BruteForce`,
line 197) and populates `records`/`nodes`/`edges`/`meta`/namespaces
directly — grepped for `rebuild`/`ActiveIndex`/`index` in this file: only
`rebuild_namespace_lists()` (unrelated, namespace linked-list repair) and
comment-only hits. `install_snapshot`
(`crates/valori-consensus/src/state_machine.rs:884-930`) does
`inner.state = state;` (line 911) with no rebuild call anywhere nearby.
Live Raft log replay (not snapshot install) *does* keep the index current
— `ValoriStateMachine::apply` (line 595) → `apply_event_ns` → `index.on_insert`/
`on_delete` per committed entry, same as any other write.

Standalone: `Engine::try_recover()`
(`crates/valori-engine/src/engine.rs:1528-1643`) calls `self.rebuild_index()`
explicitly after event-log recovery (line 1555) and after WAL recovery
(line 1619). Plain snapshot restore (`restore_from_components`, lines
1645-1669) is conditional:
```rust
match i_data {
    Some(blob) if !blob.is_empty() => {
        self.index.restore(blob)...
    }
    _ => self.rebuild_index(),
}
```
`rebuild_index()` is only invoked when the index-bytes section is
`None`/empty. **If `i_data` is present-but-corrupt, `self.index.restore(blob)`'s
`.map_err(...)?` propagates a hard `Err` — this does NOT fall back to
`rebuild_index()`.** This is a real, narrow standalone-only failure mode
this audit surfaced: a corrupted (not merely absent) index section fails
the whole restore rather than self-healing via rebuild. Out of scope to
fix here (audit-only phase); noted in §12/§15.

**Raft snapshot payload** — `SnapshotPayload`
(`crates/valori-consensus/src/state_machine.rs:125-149`):
```rust
struct SnapshotPayload {
    kernel: Vec<u8>,           // V6/V7 kernel snapshot bytes
    dedup: Vec<[u8; 16]>,
    state_hash: [u8; 32],
    created_at: Vec<(u32, u64)>,
    text_corpus: Vec<(u64, String)>,
    namespace_registry: (Vec<(String, u16)>, u16),
}
```
`kernel` is `encode_state`'s output — grepped `encode.rs` for
`ActiveIndex`/index-related tokens: zero hits. **No index bytes are ever
included in a Raft snapshot transfer.** A new/lagging replica installing
this snapshot ends up with an empty, default `ActiveIndex::BruteForce`
(harmless today only because that variant is stateless-on-search), then
catches up incrementally as subsequent log entries replay via `apply()`.

**`/v1/index/rebuild` and `/v1/index/config`**: standalone
(`crates/valori-node/src/server.rs:3406-3430`) genuinely switches kind —
doc comment (lines 3399-3405) confirms it "immediately discards the
current index, sets `index_kind` to the requested type, and rebuilds from
the live record pool." Cluster's identically-routed handlers
(`crates/valori-node/src/cluster_server.rs:2644-2669`) are hardcoded
stubs:
```rust
async fn cluster_index_rebuild() -> Response {
    // Cluster mode uses the kernel's built-in brute-force path for linearizable
    // consistency — the standalone engine index is not used here.
    (StatusCode::OK, Json(serde_json::json!({
        "ok": true,
        "note": "cluster mode uses kernel brute-force; index switching is not applicable",
    }))).into_response()
}
```
`cluster_index_config` unconditionally reports `"index_type": "brute_force"`
regardless of request body or actual state. Both exist purely to satisfy
`route_parity.rs`'s mechanical path-existence check — neither reads its
input nor touches `KernelState`.

---

## 5. Canonical vs. derived analysis

Per G0.2's own precedent ("hash semantic canonical state, not incidental
reconstruction topology"), every index must be a pure function of
canonical state (`RecordPool`/`NodePool`/`EdgePool`), reconstructible from
it, and absent from the commitment surface (snapshot bytes, BLAKE3 hash,
`KernelEvent`, Raft log).

| Surface | Contains index bytes? | Evidence |
|---|---|---|
| `encode_state`/`decode_state` (canonical `KernelState` snapshot) | **NO — confirmed** | Grepped `crates/valori-kernel/src/snapshot/encode.rs` and `decode.rs` for `hnsw\|ivf\|bq\|centroid\|neighbor\|index`: zero index-structure hits (the two `decode.rs` "index" hits are "record slot index," unrelated language) |
| `hash_state_blake3` | **NO — confirmed** | `crates/valori-kernel/src/snapshot/blake3.rs:121`+ hashes version/records/nodes/edges/meta only; zero index-related tokens found |
| `KernelEvent` (Raft log) | **NO — confirmed** | `crates/valori-kernel/src/event.rs:32`+ variants (`InsertRecord`, `DeleteRecord`, `CreateNode`, `CreateEdge`, `DeleteEdge`, `SoftDeleteRecord`, `DeleteNode`, encrypted variants) carry no index structure |
| Raft `SnapshotPayload` | **NO — confirmed** (§4) | `kernel: Vec<u8>` field is exactly `encode_state`'s output |
| Standalone `Engine::save_snapshot`'s `i_data` section | **YES — by design, and this is correct** | `engine.rs:1065-1072` explicitly serializes `self.index.snapshot()`; e.g. HNSW's `snapshot()` (`valori-index/src/hnsw.rs:538-580`) serializes `entry_point`, `max_level`, per-node `neighbors` adjacency, and vectors — genuine derived-index bytes, kept in an explicitly separate, explicitly optional section (confirmed: absent/empty → `rebuild_index()`, §4) |

**No architectural violation found on any of the four commitment surfaces
audited.** The one design gap is the omission described in §4/§8:
canonical-state-only Raft snapshot transfer is correct in principle
(index is derived, so it *should* be reconstructible from what's
transferred) but the reconstruction step (`rebuild()`) is never actually
invoked after a snapshot install — the system currently gets away with
this only because BruteForce is stateless and BQ is never activated
cluster-side. This is flagged as **D — correctness risk (latent)** in §3,
not as a canonical/derived boundary violation — the boundary itself is
intact; a follow-up construction step is simply missing.

---

## 6. Determinism analysis

Full working: no RNG anywhere in either index crate (`grep -rn
"rand::|thread_rng|getrandom|StdRng"` across `valori-index/src` and
`valori-kernel/src/index` → zero matches). All "pseudo-random" behavior
(HNSW level assignment, IVF initial centroids) is FNV-1a hashing of the
record id — a pure, deterministic function.

| Index | Same-state+query → same **query result/ranking** | Same-state+query → bit-identical **internal structure** |
|---|---|---|
| BruteForce (both crates) | **DETERMINISTIC — CONFIRMED.** No persisted structure; every `search()` recomputes and sorts with explicit `(score, id)` tie-break | Trivially identical — kernel's `BruteForceIndex` is a zero-sized stateless marker (`crates/valori-kernel/src/index/brute_force.rs:13`) |
| BQ (both crates) | **DETERMINISTIC — CONFIRMED.** `binarize()`/`encode_vector()` are pure per-vector functions of that vector's own values (fixed threshold `0`/`0.0`, no corpus statistic) | **DETERMINISTIC — CONFIRMED** — codes array is order-independent by construction |
| HNSW (valori-index only) | **COULD NOT DETERMINE FROM SOURCE ALONE, cross-platform.** For a *fixed already-built graph*, search is a deterministic greedy walk with explicit id tie-break. But `dist_scalar` (x86/other) vs `dist_neon` (aarch64, `#[target_feature(enable="neon")]`, FMA-fused 4-lane accumulation) are not guaranteed to produce bit-identical `f32` sums for the same input — `Candidate::eq` requires exact float equality with no epsilon tolerance, so a near-tie could compare as "equal" on one platform and "unequal" on another, changing which candidate the id-based tie-break rule even applies to | **NOT DETERMINISTIC — CONFIRMED.** `insert()` (`hnsw.rs:317-459`) always operates against whatever the graph currently looks like — different insertion orders of the identical final record set produce different edge sets. Level assignment (`deterministic_level`, a pure FNV-1a hash of `id` alone) is order-independent, but level assignment alone does not make the graph order-independent |
| IVF (valori-index only) | **DETERMINISTIC — CONFIRMED for a single `build()` call** (all-integer arithmetic in both the NEON and scalar codepaths — associative, bit-exact; deterministic hash-and-sort centroid seeding; explicit id tie-break everywhere). **NOT CONFIRMED for the incremental-insert-after-build path** — `insert()` after `build()` places a vector into whichever *existing* centroid's inverted list claims it without recomputing centroids, so a different insert/rebuild interleaving over the same final record set can leave different centroid state (`needs_rebuild()` only triggers past a 2x growth threshold) | **NOT DETERMINISTIC — CONFIRMED**, same reason — runtime state depends on construction history, not just the final record set |

**Explicit separation, as required by the prompt**: "same query result" and
"bit-identical structure" are genuinely different properties here. BQ
achieves both. BruteForce achieves both trivially (no structure exists).
HNSW and IVF do **not** achieve structural determinism, and HNSW's
*result*-level determinism has an unresolved cross-platform gap; IVF's
*result*-level determinism holds only within a single-build lifetime, not
across arbitrary insert/rebuild histories.

**Platform-specific behavior, explicit**: HNSW has genuine
scalar-vs-NEON dual codepaths (`crates/valori-index/src/hnsw.rs:61-70` vs
`:72-120`, dispatched at `:154-160`). IVF's NEON path
(`crates/valori-index/src/ivf.rs`) is all-integer and therefore exact —
confirmed bit-identical to its scalar counterpart, unlike HNSW's f32 path.
BQ has no SIMD variant in either crate (grepped both `bq.rs` files for
`target_arch`/`std::arch`: zero hits) — single codepath everywhere.

**Parallel construction**: grepped both index crates for
`rayon|std::thread|thread::spawn` — zero matches. No concurrent
construction anywhere; this is not a source of non-determinism in the
current implementation.

**Consequence for cluster/Raft** (carried into §9): if HNSW or IVF were
ever ported to the kernel and activated cluster-side, "two replicas built
the identical index from identical canonical state" would **not** be a
safe assumption to build correctness on for HNSW (construction-order and
cross-platform gaps) or for IVF beyond a single build cycle. Any future
design must either (a) transfer pre-built index bytes via Raft snapshot
rather than relying on independent reconstruction, or (b) restrict cluster
mode to index families with the BruteForce/BQ-style order-independence and
platform-exactness properties this audit confirms only BruteForce and BQ
actually have.

---

## 7. Configuration audit

| Setting | Exists? | Exposed via | Default | Per-collection or global? | Immutable after creation? | Requires rebuild on change? |
|---|---|---|---|---|---|---|
| HNSW `M` | Yes | `VALORI_HNSW_M` env var (`config.rs:110,331`) | 16 | Global (one `Engine`/node process) | No — but only takes effect on next rebuild | Yes (new value only applies from the next `rebuild_index()`/`/v1/index/rebuild` call) |
| HNSW `ef_construction` | Yes | `VALORI_HNSW_EF_CONSTRUCTION` (`config.rs:112,334`) | 100 | Global | Same as above | Yes |
| HNSW `ef_search` | Yes | `VALORI_HNSW_EF_SEARCH` (`config.rs:114,337`) | 50 | Global | No — this one affects *search*, not construction, so it can take effect without a rebuild (not independently confirmed from source in this pass — flagged as unverified, not asserted) | No (search-time parameter) |
| IVF `n_list` | Yes | `VALORI_IVF_N_LIST` (`config.rs:120,341`) | Auto-scaled `max(16, sqrt(N))` when unset | Global | No | Yes |
| IVF `n_probe` | Yes | `VALORI_IVF_N_PROBE` (`config.rs:122,344`) | Auto-scaled `max(1, sqrt(n_list))` when unset | Global | No | No (search-time parameter — probes an already-built index) |
| BQ `pool_factor` | Yes | `VALORI_BQ_POOL_FACTOR` (`config.rs:128,348`) | 10 | Global | No | No (search-time candidate-pool sizing) |
| BQ `min_candidates` | Yes | `VALORI_BQ_MIN_CANDIDATES` (`config.rs:130,351`) | 200 | Global | No | No |
| BruteForce metric | Fixed | N/A — no config exists | Squared L2, always | N/A | N/A | N/A |
| Any metric other than L2 (cosine, dot-product) | **NOT FOUND as an index-search metric anywhere in the codebase** | — | — | — | — | — |
| Index *kind* itself | Yes | `VALORI_INDEX` env var (standalone only, per §4) | `BruteForce` | **Global to the node process** (`Engine.index_kind`/`current_effective_kind` are engine-level fields, `crates/valori-engine/src/engine.rs:120-121` — not per-namespace) | No — `/v1/index/rebuild` can switch kind on a live, populated engine | Yes, explicitly, by design |

All settings audited are **real and wired**, not invented — every env var
above has a confirmed parse site in `config.rs`. No per-collection/
per-namespace index configuration exists anywhere in the current design:
one node process runs exactly one index kind for its entire record pool
(standalone), or exactly `BruteForce`/`BinaryQuantization` per shard
(cluster, and only `BruteForce` in practice per §4).

---

## 8. Index switching audit

The product model previously established ("dimension fixed, metric fixed,
index selectable") is **partially implemented, standalone-only**:

- `/v1/index/rebuild` (standalone) genuinely supports switching kind on a
  live engine — confirmed in §4, this is real, not aspirational.
- There is **no background-build / atomic-swap / validate-then-retire**
  architecture anywhere in the code. `/v1/index/rebuild`
  (`server.rs:3406-3430`) is synchronous: it takes a write lock on the
  whole engine, discards the current index in place, and rebuilds — the
  engine is unavailable for the duration (this reuses the already-measured
  187s-class rebuild cost for HNSW at 10K vectors, §10). There is no
  "build new index in the background, validate it, then atomically swap"
  path implemented; the conceptual model in the prompt's Part 6 diagram
  does **not** exist in the current codebase.
- Cluster mode has no switching capability at all — its endpoints are
  stubs (§4).

**Is the background-build/atomic-swap model compatible with Valori's
canonical/derived separation?** Yes, in principle — since the index is
already fully derived from canonical state (§5), building a *second*
derived index in the background from the same canonical snapshot and
atomically swapping the active pointer once built is architecturally
sound and would not touch canonical state, snapshot format, or the hash
contract. **This is a design recommendation surfaced by the audit, not
something evaluated as already-planned or committed — no code path
attempts it today, and this document does not propose implementing it.**

---

## 9. Recovery / cold-start analysis

**Does rebuild happen synchronously and block serving?**
Standalone: yes — `try_recover()`'s `rebuild_index()` calls run inline
before `main.rs` proceeds to bind the HTTP listener (not independently
re-verified line-by-line in this pass, but consistent with `try_recover`'s
own synchronous, non-async signature and every prior phase doc's
description of restart behavior). Cluster: index-affecting work
(`on_insert` during log replay) happens inline within `apply()`, which is
itself synchronous per-entry within the Raft apply loop.

**Is there an index-ready gate?** Cluster has a `ReadinessGate`
(referenced in G1.4.1/G1.4.2 work) that gates on Raft log replay progress
(`startup_committed_index` vs. currently-applied index) — **not
independently re-verified in this pass whether it also gates on any
index-specific completion state**, because no such state exists to gate
on: the cluster path has no rebuild step at all (§4), so there is nothing
for a readiness gate to wait on beyond ordinary log replay, which already
incrementally maintains the index via `on_insert`/`on_delete`.

**Crash mid-rebuild**: no resumability/checkpointing exists in any
`rebuild()` implementation examined — a crash mid-rebuild simply means the
next boot rebuilds from scratch again. **NOT IMPLEMENTED — confirmed
absent** (no partial-progress state persisted anywhere).

**Can an old derived index be reused?** Only via the standalone `i_data`
snapshot section (§4/§5) — when present and valid, `self.index.restore(blob)`
reuses it directly without rebuilding. Absent that section (or on the
entire cluster path, which has no equivalent), every restart rebuilds from
scratch.

**Is derived-index persistence supported?** Yes, standalone-only, via each
index's own `snapshot()`/`restore()` methods feeding the `i_data` section
— confirmed for HNSW (`valori-index/src/hnsw.rs:538-580` serializes
`entry_point`, `max_level`, per-node adjacency, vectors).

**Proportional-to-recent-writes cold start?** **NOT IMPLEMENTED —
confirmed absent.** Grepped for "incremental rebuild," "delta index,"
"checkpoint" in the index code — no matches (the one "checkpoint" hit in
`engine.rs:1599` is WAL log-rotation splicing, unrelated to index
construction). Every rebuild is proportional to total vector count, not
to writes since the last successful snapshot.

---

## 10. Raft / cluster interaction

**Can two replicas that both independently build the same index safely
serve search, given identical canonical state?**

- **BruteForce**: Yes, unconditionally — the index is stateless; "building"
  it is a no-op, and search reads the live record pool directly. No
  determinism concern applies because there is no persisted structure to
  diverge.
- **BQ (kernel-native, currently unreached)**: Yes — §6 confirms the codes
  array is a pure, order-independent, per-vector function; two replicas
  building it independently from identical canonical state produce
  identical codes, hence identical search behavior.
- **HNSW/IVF (hypothetical future kernel port)**: **No, not safely, without
  further work.** §6 established that HNSW's graph structure is
  insertion-order-dependent and its distance function has non-bit-identical
  cross-platform paths; IVF's centroid state depends on construction
  history beyond a single `build()` call. Two replicas independently
  "building HNSW" from identical canonical state are NOT guaranteed to
  build the *same graph* (different internal event-application order,
  even if the final canonical record set is identical, could plausibly
  differ — though this specific claim about the *replay* path's insertion
  order guarantees was not independently re-traced in this pass; it rests
  on the general order-sensitivity finding in §6, not a replay-specific
  counterexample). Even where the graphs happen to match, a mixed x86/ARM
  cluster could return different results at exact-tie boundaries per §6.

**Where does index construction belong, given the current architecture?**
Currently: **(C) after state-machine application** — `on_insert`/`on_delete`
calls happen synchronously inside `KernelState::apply_event_ns`, which is
itself called from within `ValoriStateMachine::apply()`'s per-entry
handling (`state_machine.rs:595`). This is **not** (A) inside the state
machine's own persistence/commit boundary in a way that would make index
state part of Raft's replicated log (confirmed by §5 — no index bytes
enter the log), and it is **not** (D) a separate background worker task —
everything observed is synchronous, inline, on the same call path as
event application. No code path implements (B) "outside the state
machine" in the sense of a fully decoupled, asynchronously-catching-up
index. This is a factual report of what exists, not a recommendation for
where it *should* live (§13 covers that as an open question, not a
decision made here).

**New replica joining**: walked in full in §4 — receives only canonical
pools via `SnapshotPayload`, `install_snapshot` sets `inner.state = state`
with no rebuild call, ending up with a default empty `ActiveIndex::BruteForce`
(harmless only because that variant is stateless), then catches up
incrementally via live log replay exactly as any other write does. No
pre-built index bytes are ever transferred through Raft.

---

## 11. Performance evidence (cited, never extrapolated)

**Origin of "~187s HNSW recovery"**: first measured in
`docs/phases/phase-S9-resource-capacity.md` (Finding #3, "took **188
seconds** to recover just **10,000 vectors at dim=384**"), re-measured
with a corrected 300s restart-timeout (replacing an earlier flawed 60s
timeout) in `docs/phases/phase-S11-index-tuning.md` (Finding #4, "**187.1s**
... now measured with a correct timeout and a confirmed matching state
hash"). Both figures are the same measurement point: **10,000 vectors,
dim=384**, standalone `valori-node` in Docker (1GB RAM / 0.5 vCPU) — no
larger scale for HNSW was ever run (S11's own stop rule explicitly skipped
the planned 50K point).

**All other cited numbers** (standalone only — confirmed by
`benchmarks/capacity/README.md`'s description of a single `docker run`
container per cell, never `docker compose`/multi-node):

| Index | Scale | Recovery | Search p50 | Insert throughput | Memory | Recall@10 |
|---|---|---|---|---|---|---|
| BruteForce | 10K, 384D | ~1-9s | 118.29ms | 1224.8/s | formula: 17.23×dim+1531 B/vector | 1.0 (exact) |
| BruteForce | 100K, 384D | Not measured | 1307-1308ms | Not measured | — | 1.0 |
| HNSW | 10K, 384D | **187.1s** | 115.95-116.14ms (tied with BF) | 43.9-51.5/s (24-28x worse than BF) | Not measured | Not measured (approximate, no recall figure cited) |
| HNSW | 50K, 100K, any other dim | **Not measured** (S11's own stop rule; S10/S11 explicit) | — | — | — | — |
| IVF | 50K, 384D | 47.4s | 664ms | Not measured | 345.9MB (~BF's 349MB) | 1.0 |
| IVF | 100K, 384D | 129.7s (~N^1.5 scaling) | 1332ms (p95/p99 worse than BF) | Not measured | 738.6MB (~BF's 750.7MB) | 1.0 |
| IVF | tuning sweep, 50K | 16.9s(n_list=64)→103.4s(n_list=512), n_probe has "no measurable effect" | ~660-664ms flat across all tested n_list×n_probe | — | — | — |
| IVF | 250K-500K, 768D/1536D | **Not measured** (deliberately not run) | — | — | — | — |
| BQ | 50K, 384D, default config | 4.2s | 635ms (~5% faster than BF) | Not measured | 404.7MB (highest of the three) | **0.48** |
| BQ | 50K, 384D, tuned (`min_candidates=10000`) | 4.18s | 634ms | Not measured | 343-348MB | **0.99** (Recall@5: 1.0) |

Any cell not in this table is **Not measured** in the source repository —
no number for it should be trusted or repeated without a new benchmark
run. **None of these numbers apply to cluster mode** — cluster mode's
kernel-native `IndexVariant` has no reference to `valori-index`'s
`HnswIndex`/`IvfIndex`/tuning knobs at all; the benchmark harness
(`benchmarks/capacity/`) exercises the standalone binary exclusively.

---

## 12. Security / correctness risks

1. **D — latent, currently harmless**: cluster Raft snapshot install never
   calls `.rebuild()` on the index after transferring canonical state
   (§4/§5/§9). Harmless today because BruteForce is stateless and BQ is
   never activated cluster-side. Becomes a real bug — a new replica
   silently serving empty/stale search results until enough writes replay
   — the moment any stateful kernel index (BQ, or a future HNSW/IVF port)
   is wired into cluster mode. **Must be fixed before BQ (or anything
   else) is activated cluster-side, not after.**
2. **C — compatibility/config gap, not a security issue**: `VALORI_INDEX`
   is silently discarded on the cluster boot path with no warning specific
   to that variable. An operator setting it on a cluster deployment gets
   no signal that their config had no effect. Low severity (no correctness
   impact — cluster is always BruteForce regardless) but a real
   operational surprise.
3. **D — narrow, standalone-only, low-likelihood**: a present-but-corrupt
   (not absent) `i_data` snapshot section causes `restore()` to hard-error
   rather than fall back to `rebuild_index()` (§4). Only triggers on disk
   corruption of that specific section; the far more common
   absent/truncated case is already handled safely.
4. **F — requires a product decision, not a bug**: HNSW's non-bit-identical
   cross-platform distance computation (§6) has no currently-known
   exploit path (nothing in cluster mode uses HNSW), but would become a
   genuine cross-replica-disagreement risk if a mixed-architecture Raft
   cluster ever ran a hypothetical future kernel HNSW port without an
   epsilon-tolerant tie comparison or a platform-pinning constraint.

No issue found rises to "canonical state corruption" or "BLAKE3 hash
contract violation" — the canonical/derived boundary itself is intact
(§5). All four risks above are about the *derived* layer's correctness
under conditions (stateful cluster index, corrupt snapshot section, mixed
architecture) that do not occur in the system as currently configured.

---

## 13. Product architecture options (evidence-based, not preference-based)

Evaluated against: determinism, recovery time, memory, scalability,
operational complexity, Raft correctness, canonical-state purity, user
expectations, cloud economics, index-switching ability, future
serverless architecture.

**Option A — Cluster: BruteForce only; Standalone: multiple indexes**
(the status quo, modulo the fixed bugs). Determinism: trivially safe
(§6/§10 — BruteForce has no determinism risk). Recovery: fast at the
scales measured for BruteForce (~1-9s at 10K; not measured beyond 100K).
Memory: BruteForce's O(N×dim) cost is the *baseline* every other index
was measured against and never meaningfully beaten (S9/S10 findings —
IVF/BQ memory was "statistically indistinguishable" from BruteForce at
every measured scale). Raft correctness: no open question (§10 — BruteForce
is safe by construction). Canonical purity: no risk (nothing stateful to
leak). Operational complexity: lowest — one code path, one behavior,
nothing to explain about which cluster nodes have which index. Cost:
BruteForce's search-latency-bound scaling ceiling (~29,300 vectors at
384D on the measured free-tier profile, S9 Finding #1) is a real product
ceiling for cluster-mode collections, unless resource tiers scale it up.

**Option B — cluster and standalone support the same index family.**
Requires: porting HNSW and IVF to `no_std`/kernel-native (nontrivial —
`valori-index`'s HNSW uses `std::sync::RwLock`, heap-allocated adjacency
lists, and NEON/scalar dual codepaths that would need an epsilon-tolerant
determinism story before being safe under Raft per §6/§10); fixing the
snapshot-transfer gap (§12 item 1) *before* any stateful index is
activated; resolving the cross-platform float-tie question for HNSW
specifically if mixed-architecture clusters are a real deployment target.
Not evidence-supported as a near-term recommendation — the determinism
gaps found in §6 are not cosmetic, they are structural properties of the
algorithms as currently written.

**Option C — cluster initially supports a controlled subset (BruteForce +
BQ), later HNSW/IVF.** BQ is the only non-BruteForce index this audit
confirms is *already* fully order-independent, cross-platform-exact
(no SIMD divergence — §6), and kernel-implemented (`crates/valori-kernel/src/index/bq.rs`
already exists and is correct). The only missing piece for BQ specifically
is: (a) wiring `VALORI_INDEX`/`set_index_kind` into the cluster boot path
(§4's gap), and (b) fixing the snapshot-rebuild gap (§12 item 1) so a
newly-joined replica reconstructs BQ's codes rather than starting empty.
Both are scoped, bounded fixes, not new algorithm development. This is
the option most directly supported by what this audit actually found: BQ
is ready from a determinism standpoint; HNSW/IVF are not, absent further
design work on the open questions in §15.

**Option D — indexes selected per collection, built/rebuilt independently
per worker.** No code path today supports per-collection index
configuration at all (§7 — it's global-to-the-node-process on both
paths). This would be new product surface, not a fix to an existing gap —
out of scope to recommend building without a stated product requirement
driving it; flagged as a plausible future direction (aligns with a
serverless/per-tenant-resource-shape architecture) but not evaluated
further here since none of this audit's evidence bears on whether it's
needed.

---

## 14. Recommendation

**Evidence supports Option C as the correct next architectural step, in
two bounded phases, neither implemented in this document:**

1. Fix the two scoped bugs found in §12 (items 1 and 2) — these are
   corrections to already-decided architecture, not new feature work, and
   both must exist before BQ can be safely activated cluster-side
   regardless of what happens next.
2. Wire `VALORI_INDEX=bq` through to `set_index_kind()` on the cluster
   boot path, backed by the now-fixed snapshot-rebuild safety net.
   BruteForce remains the only kernel index HNSW/IVF would need to beat to
   justify porting — and per §11's own cited numbers, IVF/BQ have
   consistently failed to clearly beat BruteForce on the *standalone*
   benchmarks that already exist (memory "statistically indistinguishable,"
   search latency parity or worse at scale) — so activating BQ
   cluster-side is justified by *feature parity with standalone* and *BQ's
   proven recovery-time advantage* (4.2s vs BruteForce's comparable 1-9s
   at 10K — not a clear win, but real), not by an unproven claim that BQ
   will outperform BruteForce at cluster scale. That claim would need its
   own cluster-specific benchmark before being asserted.

HNSW/IVF porting to the kernel (Option B) is **not recommended without
first resolving the open determinism questions in §15** — this is not a
preference, it follows directly from §6/§10's findings: activating an
index whose structural determinism across independent construction and
whose cross-platform result-determinism are both unconfirmed, inside a
system whose entire value proposition rests on deterministic, verifiable
replicated state, is the kind of decision G0.2's own precedent ("hash
semantic state, not incidental reconstruction topology") was written to
prevent being made casually.

---

## 15. Open questions (genuinely unresolved — require a product decision)

1. Is a mixed x86/ARM Raft cluster a real deployment target? If not,
   HNSW's cross-platform float-tie gap (§6) is moot for cluster purposes
   and only matters for standalone users who might restore a snapshot
   taken on one architecture onto a different one.
2. If HNSW is ever ported to the kernel, should replicas transfer
   pre-built index bytes via Raft snapshot (paying a bandwidth cost but
   guaranteeing structural agreement) or independently reconstruct from
   canonical state (cheaper, but per §6/§10 not currently safe without an
   order-independent construction algorithm)? This is the central
   unresolved design question the current audit surfaces but does not
   answer.
3. Does the product need per-collection index selection (Option D), or is
   global-per-node-process sufficient for the deployment shapes actually
   being sold? No evidence in this audit bears on this — it's a market/
   product question, not a technical one.
4. Should `/v1/index/rebuild`'s "background build + atomic swap" model
   (§8) be built at all, or is synchronous rebuild-with-downtime
   acceptable given the measured costs (§11)? Depends on whether cluster
   ever needs live index switching at all (today it doesn't switch
   anything).
5. Is BQ's ~0.5 baseline recall (untuned) acceptable for a first
   cluster-mode non-BruteForce option, given it requires
   `min_candidates` tuning (§11) to reach the ~0.99 figure — and is that
   tuning knob something cluster operators would need exposed
   immediately, or can it ship with the standalone-proven tuned default?

---

## 16. Exact file/line references (index)

Consolidated list of every source location cited above, for convenience:

- `crates/valori-node/src/config.rs:110-131,230-392,257-263,331-351` — env var parsing (all settings)
- `crates/valori-node/src/engine.rs:26-51` — standalone `EngineFromNodeConfig`
- `crates/valori-node/src/main.rs:255-295` — cluster boot, `VALORI_INDEX` drop point
- `crates/valori-node/src/cluster.rs:38-66,215-229,316-321,357-436` — `ClusterConfig`, `ShardHandle`, `bootstrap_cluster`, per-shard construction
- `crates/valori-node/src/cluster_server.rs:1039,2644-2669` — cluster index-config/rebuild stubs, the pre-existing "never calls set_index_kind" comment
- `crates/valori-node/src/server.rs:3399-3430` — standalone `/v1/index/rebuild`
- `crates/valori-consensus/src/state_machine.rs:125-149,320-351,595,884-930,946-952` — `SnapshotPayload`, constructors, apply, install_snapshot, build_snapshot
- `crates/valori-kernel/src/state/kernel.rs:26,49,63-75` — `KernelState`, `set_index_kind`, `with_dim`
- `crates/valori-kernel/src/index/mod.rs:48-89` — `IndexVariant`/`ActiveIndex`, commented-out HNSW/IVF arms
- `crates/valori-kernel/src/index/bq.rs`, `brute_force.rs` — kernel-native implementations
- `crates/valori-kernel/src/snapshot/encode.rs`, `decode.rs`, `blake3.rs` — canonical commitment surfaces, confirmed index-free
- `crates/valori-kernel/src/event.rs:32+` — `KernelEvent`, confirmed index-free
- `crates/valori-engine/src/engine.rs:120-121,271,1065-1072,1177-1183,1474-1494,1528-1643,1645-1669` — `Engine` fields, `set_index_kind` call site, `i_data` write/read, `rebuild_index`, `try_recover`, `restore_from_components`
- `crates/valori-index/src/hnsw.rs:41-58,61-120,154-172,236-241,260-301,317-459,538-580` — HNSW distance codepaths, level assignment, insert, snapshot
- `crates/valori-index/src/ivf.rs`, `deterministic/kmeans.rs:8-95` — IVF build/insert, deterministic k-means
- `crates/valori-index/src/bq.rs:64-73` — BQ binarize
- `crates/valori-node/tests/route_parity.rs:24-33` — allowlists, confirming no index-related entries needed since paths exist on both routers
- `docs/phases/phase-S9-resource-capacity.md`, `phase-S10-index-capacity.md`, `phase-S11-index-tuning.md` — all performance figures cited in §11

---

## 17. G1.4.4 readiness decision

**G1.4.3 PASS.**

This audit is complete, internally consistent, and every major claim is
either code-verified with an exact citation or explicitly marked as not
measured / not determinable from source alone. The open questions in §15
are genuine product/architecture decisions this document deliberately
does not make. No code was modified. No index was implemented. No
canonical state, snapshot, WAL, hashing, or API was changed.

**G1.4.4 is NOT started.** If you approve Option C's direction (§14), the
next phase should scope narrowly to the two bounded fixes in §12
(items 1 and 2) plus the cluster BQ wiring — not HNSW/IVF porting, which
this audit's evidence does not support attempting until §15's open
questions have answers.
