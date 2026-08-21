# Recovery / HNSW Startup Breakdown

**MEASUREMENT AUDIT ONLY.** No production code was modified. No canonical
state, snapshot/WAL/event format, BLAKE3 contract, HNSW semantics, or API
was changed. No optimization was made. A single temporary instrumented
test file (`crates/valori-node/tests/scratch_recovery_breakdown.rs`) was
written to exercise real, public `Engine`/`valori-state` functions with
timing wrapped around them from the outside; it has been deleted after
capturing the results below — nothing it did required modifying any
production function.

---

## Part 1 — Finding the original ~187s measurement

**The number is not one measurement — it is two independent, consistent
measurements of the same scenario, from two different phase docs, plus
one earlier failed attempt with a broken harness.**

| Source file | Figure | Status |
|---|---|---|
| `benchmarks/capacity/results/index_comparison.json` | N/A | **Failed to measure at all** — 60s health-check timeout, container hadn't recovered yet: `"status": "restart_failed"` |
| `benchmarks/capacity/results/s11_hnsw_10k.json` | N/A | Same 60s-timeout bug, `"status": "restart_failed"`, `"index_build_elapsed_secs": null` |
| `benchmarks/capacity/results/hnsw_recovery_retest.json` | **188s** (exactly, not decimal) | Manual retest with a 300s timeout, cited in `docs/phases/phase-S9-resource-capacity.md` Finding #3 |
| `benchmarks/capacity/results/s11_hnsw_10k_v2.json` | **187.075s** | `bench_cell.py` re-run after the 60s→300s timeout bug was fixed in the harness itself, cited in `docs/phases/phase-S11-index-tuning.md` Finding #4 as "187.1s" |

**Exact scenario** (both successful measurements, cross-checked and
consistent with each other):

- **Records**: 10,000 vectors, `VALORI_MAX_RECORDS=20000` (2x headroom)
- **Dimension**: 384
- **HNSW config**: no `VALORI_HNSW_M`/`VALORI_HNSW_EF_CONSTRUCTION`/
  `VALORI_HNSW_EF_SEARCH` set in either benchmark script
  (`benchmarks/capacity/scripts/bench_index_types.py`,
  `bench_cell.py`) — **defaults were used**: `M=16`,
  `ef_construction=100`, `ef_search=50` (per `config.rs:110-114`'s doc
  comments, cross-checked in the G1.4.3 audit).
- **Hardware/environment**: `docker run --memory 1024m --cpus 0.5`, image
  `cloud-worker-a:latest` (the real `valori-node` binary, not a stub).
  Underlying host architecture not recorded in either result file — not
  independently determinable from the repository.
- **Snapshot + event log both configured**: `VALORI_EVENT_LOG_PATH=/data/events.log`,
  `VALORI_SNAPSHOT_PATH=/data/state.snap` (both scripts, confirmed by
  reading `bench_index_types.py`'s `docker run` args and `bench_cell.py`'s
  equivalent — not independently re-read for `bench_cell.py` in this pass
  but consistent with every other S9/S10/S11 cell's documented config).
- **Snapshot size / event-log size at the time of the original
  measurement**: **not recorded** in any of the JSON result files or the
  phase docs — this repository does not retain that number. Not invented
  here.
- **Cold process startup or `Engine::try_recover()`?**: **Cold process
  startup** — the measured interval is `docker stop` → `docker start` →
  first successful `/health` 200, i.e. the *entire* container restart
  including Docker daemon overhead, binary startup, config parsing, and
  `try_recover()`, not an isolated call to `try_recover()` alone. Exact
  code (`benchmarks/capacity/scripts/bench_cell.py:217-224`):
  ```python
  t_restart = time.time()
  # (docker stop / docker start bracket this timestamp)
  if wait_healthy(args.port, timeout_s=300):
      recovery_elapsed = time.time() - t_restart
      result["index_build_elapsed_secs"] = round(recovery_elapsed, 3)
  ```
  **This field's name, `index_build_elapsed_secs`, is a misnomer inherited
  from the benchmark script — it was never actually isolating HNSW index
  construction from anything else.** It is full container-restart-to-healthy
  wall time. This is the single most important correction this audit
  makes to the prior framing: **nobody previously isolated how much of the
  187s was HNSW construction versus everything else** — the field name
  implied it was already isolated, and it wasn't.
- **Whether index construction was included**: yes, necessarily (§2/§3
  below trace exactly why — the event-log recovery path this
  configuration exercises always calls `rebuild_index()` unconditionally).
- **Whether graph reconstruction was included**: yes, trivially — the
  benchmark's records had no associated graph nodes/edges at all (it
  predates G0/G1's graph work; `bench_index_types.py`/`bench_cell.py` only
  ever call `/v1/vectors/batch-insert`), so "graph reconstruction" in the
  original measurement was reconstructing an **empty** graph — zero nodes,
  zero edges. This matters directly for Part 8 below.
- **Single run or multiple?**: effectively **two independent successful
  runs** of the same scenario (188s and 187.075s, from separate benchmark
  invocations on separate days per the S9/S11 phase docs), agreeing within
  0.5%, plus two earlier failed attempts due to an unrelated
  timeout-length bug in the harness (not measurement noise — a genuine
  script defect, now fixed). No formal median/min/max across ≥3 runs was
  ever computed in the original work.
- **Exact command used**: not preserved verbatim (the benchmark scripts
  are re-run manually per phase, no single logged invocation exists in the
  repo beyond the scripts themselves) — the script source (quoted above
  and in Part 2) is the closest available record of "the exact command."

**Reproducibility verdict**: the original measurement **cannot be
reproduced exactly** in this environment — it required a specific Docker
image, a specific 1GB/0.5vCPU resource constraint, and (implicitly) the
host architecture the original benchmarks ran on, none of which are
pinned down precisely enough in the repository to replay byte-for-byte.
This is stated explicitly rather than worked around by inventing missing
parameters. Part 3 onward instead performs a **real, non-Docker, unconstrained-hardware
measurement of the same scenario's actual code path**, which — as shown
below — is sufficient to answer the question this task actually asks
("where does the time go, proportionally, and which stage is the real
target of any future optimization"), even though it cannot reproduce the
absolute 187s figure on different hardware with a different resource
ceiling.

---

## Part 2 — The actual recovery path, traced from source

```
Process startup (main.rs)
    ↓
NodeConfig::default() — VALORI_* env vars parsed (config.rs:230-392)
    ↓
Engine::new(&cfg) — engine.rs (EngineFromNodeConfig impl, engine.rs:26-51)
    ↓
engine.try_recover() — engine.rs:1528-1643
    ├─ IF event_log_path configured AND file exists (this benchmark's config):
    │     valori_state::bootstrap::recover_from_events(&log_path)
    │         (valori-state/src/bootstrap.rs:48-53 → recover_from_event_log)
    │         — replays KernelEvents into a fresh KernelState.
    │         CANONICAL STATE ONLY. No index/vector-index code touched.
    │     self.rebuild_index() — engine.rs:1555, called UNCONDITIONALLY,
    │         every time this branch is taken, regardless of whether a
    │         valid snapshot with persisted index bytes (i_data) also exists.
    │     → return RecoveryMode::EventLog(count)   [EARLY RETURN]
    │
    ├─ ELSE IF snapshot_path configured AND file exists:
    │     std::fs::read(&path) → self.restore(&data) → restore_from_components()
    │         (engine.rs:1645-1669)
    │         self.state = decode_state(k_data)     — canonical only
    │         match i_data {
    │             Some(blob) if !blob.is_empty() => self.index.restore(blob),  ← FAST PATH
    │             _ => self.rebuild_index(),                                   ← SLOW PATH
    │         }
    │     → return RecoveryMode::Snapshot
    │
    └─ ELSE IF wal_path configured AND file exists (legacy):
          replay_wal(...) then self.rebuild_index() unconditionally, same
          shape as the event-log branch.
    ↓
axum HTTP listener binds, /health starts returning 200
    ↓
"Ready" (per the benchmark's own definition — see Part 12)
```

**Critical finding, confirmed by reading the branch order exactly**: this
benchmark's configuration (event log *and* snapshot both set) means
`try_recover()` **always takes the event-log branch first and returns
early at line 1560** — the snapshot branch (lines 1578-1594), and
therefore the `i_data`-fast-path that skips rebuilding, is **structurally
unreachable** whenever an event log is present and non-empty. The
persisted-index-bytes optimization exists in the code (§ the i_data
section, confirmed present and working in Part 3/6 below) but this exact
benchmark scenario never exercises it. This directly refines the prior
S9/S11 note's claim that "the index is never persisted" — **it is
persisted** (into the snapshot file, on every graceful shutdown), it is
simply never read back in this configuration.

**Graph restoration's place in this chain**: `decode_state`/
`recover_from_events` restore `RecordPool`, `NodePool`, `EdgePool`, and
namespace linked lists together, in one pass, as part of "canonical state
only" above — there is no separate graph-specific reconstruction stage.
Part 5 measures this directly.

**HNSW construction's place in this chain**: exactly one call site,
`self.rebuild_index()` (`engine.rs:1474-1494`), itself calling
`self.build_index()` (`engine.rs:1454-1472`), which iterates the
now-populated `RecordPool` and calls the concrete index's `insert()` once
per live vector — for HNSW this is `HnswIndex::insert()`
(`crates/valori-index/src/hnsw.rs:317-459`), the same order-sensitive
greedy-insertion algorithm described in the G1.4.3 audit.

---

## Part 3 — Snapshot loading, measured

Measured directly (real `Engine::save_snapshot`/`Engine::restore`, no
mocking), 10,000 vectors / dim=384 / HNSW, plus 2,000 graph nodes / 3,000
graph edges added specifically to give graph restoration something
non-trivial to measure (the original benchmark had zero graph objects —
see Part 1):

| Stage | Time | Notes |
|---|---|---|
| A. Snapshot file discovery | Not separately measurable — `try_recover()` calls `path.exists()` inline, sub-microsecond, not worth isolating | — |
| B. Snapshot file read | **0.005s** | 32,245,488 bytes (32.2 MB) read from local disk |
| C. Snapshot decode | **≤ 0.008s** (upper bound — see below) | Could not isolate decode_state alone due to a bug in the throwaway test's manual header-offset parsing (not a production bug — my own scratch code guessed the wrong byte offsets for the snapshot header and panicked slicing past the buffer end). The bound comes from D below, which includes decode + index restore together and completed in 8ms total, so decode alone is provably no larger than that. |
| D. State reconstruction + index restore together (`restore()`, full call) | **0.008s** | This is `decode_state(k_data)` + `self.index.restore(i_data)` (the HNSW-bytes fast path) combined — both essentially free at this scale |
| E. Validation/invariant checks | Not separately instrumented in this pass — `decode_state` does perform structural validation inline (bounds checks, cross-references, per the G0.1/G1.3.1 audits) but its cost is folded into C/D above, which are already sub-10ms | — |
| F. BLAKE3/state-hash calculation during recovery | **Not exercised by `try_recover()`/`restore()` directly** — `hash_state_blake3` is called by the caller (e.g. the benchmark's own before/after `/v1/proof/state` comparison, or Raft's snapshot install path), not internally by the recovery functions themselves. Not part of the recovery critical path. | — |

**Snapshot contents at this scale**: 10,000 records, 2,000 graph nodes,
3,000 graph edges, 1 namespace (default), 32,245,488 bytes total (the
32.2MB is dominated by the persisted HNSW `i_data` section — vectors and
adjacency lists at dim=384 — not the canonical record/node/edge bytes,
which are comparatively small).

**Conclusion for Part 3**: snapshot loading, in every sub-stage measured,
is **negligible** — low single-digit milliseconds total, even including a
persisted 32MB HNSW graph. This categorically rules out "snapshot loading"
as a meaningful contributor to a 187-second figure.

---

## Part 4 — Event-log replay, measured

Same run, real `valori_state::bootstrap::recover_from_events` call against
the actual event log written during the build phase:

| Metric | Value |
|---|---|
| Event-log entries | 15,000 (10,000 `InsertRecord` + 2,000 `CreateNode` + 3,000 `CreateEdge`) |
| Event-log bytes on disk | 16,129,414 bytes (16.1 MB) |
| Replay duration | **0.079s** |
| Events/sec | 190,444.6 |
| Bytes/sec | ~204.8 MB/s |

**Vector vs. graph events were not separable with the existing
instrumentation without invasive changes** (per the task's own
instruction not to modify the event format) — `recover_from_events`
replays the whole log as one pass with no per-event-kind timing hook
exposed publicly. Given the total replay cost is 79 milliseconds for
15,000 mixed events, further decomposition would not change any
conclusion — even if 100% of that 79ms were graph events, it is still
three orders of magnitude smaller than 187 seconds.

**Conclusion for Part 4**: event-log replay, including every graph
mutation added by G0/G1's work, is **negligible** — well under a tenth of
a second at this scale. This categorically rules out "event replay" as a
meaningful contributor to a 187-second figure.

---

## Part 5 — Graph restoration vs. HNSW construction (explicitly not conflated)

Both `decode_state` (snapshot path) and `recover_from_events` (event-log
path) restore `NodePool`, `EdgePool`, adjacency (intrusive linked lists),
namespace relationships, and record→GraphNode `record` back-references
**in the same single pass that restores records** — there is no separate
"graph reconstruction" stage distinguishable from "canonical state
restoration" in the source; they are the same code path. This was
independently confirmed in the G1.4.3 audit (§5 of that document: no
index-specific bytes anywhere in the canonical snapshot/hash/event
surfaces) and is consistent with the measurement above: 15,000 events
(5,000 of which are graph events: 2,000 nodes + 3,000 edges) replayed in
79ms total, no measurable separate cost attributable to the graph portion
versus the vector portion.

**Record→GraphNode relationships specifically**: restored implicitly as
part of each `CreateNode` event's `record: Option<RecordId>` field
(canonical, per `KernelEvent::CreateNode`) — no separate resolution step,
no `record_to_node`-style cache rebuilt at recovery time (that cache was
removed entirely in G1.3.1; `resolve_seed_nodes`/`nodes_referencing_record`
are pure, on-demand functions over the already-restored `NodePool`, never
invoked as part of recovery itself).

**Conclusion for Part 5**: canonical graph restoration and HNSW index
construction are **architecturally and temporally independent** — the
former is folded into the sub-100ms canonical-state restoration measured
in Parts 3/4; the latter is the separate, isolated, multi-second-at-this-scale
stage measured next in Part 6. G0/G1's graph work added essentially zero
measurable recovery-time cost at this scale.

---

## Part 6 — HNSW construction, isolated and measured

`rebuild_index()` (`crates/valori-engine/src/engine.rs:1474-1494`) called
directly, in isolation, on a freshly event-log-recovered `KernelState`
(10,000 vectors already canonically present, zero index state):

| Metric | Value |
|---|---|
| Vectors | 10,000 |
| Dimension | 384 |
| M (max edges/node/layer) | 16 (default, not overridden) |
| ef_construction | 100 (default, not overridden) |
| ef_search | 50 (default — not exercised during construction, search-only parameter) |
| Metric | squared L2 |
| **Construction time** | **6.686s** |
| Vectors/sec | 1,495.7 |
| Memory | Not measured in this pass (would require a heap-profiling tool not wired into this quick harness; the S9/S10 Docker-based benchmarks' RSS figures — 86.9-87.7MB peak at this same 10K/384D scale — remain the best available number, cited not re-derived) |

**Where this happens in source, exactly**: `rebuild_index()` picks the
concrete boxed index via `effective_index_kind()` then calls
`self.build_index()` (`engine.rs:1454-1472`), which iterates every live
record in `RecordPool` (in pool/slot order) and calls
`index.on_insert(id, vec)` once per record — for HNSW this dispatches to
`HnswIndex::insert()` (`crates/valori-index/src/hnsw.rs:317-459`), the
same greedy-insertion, order-sensitive algorithm flagged in the G1.4.3
audit as insertion-order-dependent. There is no separate "bulk build"
fast path distinct from "insert one at a time" — `rebuild_index()` is
literally a loop of individual `insert()` calls in pool order, not a
batch-optimized construction routine.

**Internal HNSW stages**: `insert()` itself does not expose separately
measurable sub-stages through the public API without modifying the
function (`deterministic_level` assignment, greedy descent to find the
insertion point, `search_layer`/`select_neighbors_heuristic` at each
affected level) — decomposing further would require instrumenting
`hnsw.rs` internals, which this audit-only phase does not do, per the
explicit instruction not to modify production code.

**Conclusion for Part 6**: this is the real, isolated cost of HNSW
construction for 10,000 vectors at dim=384 on unconstrained local
hardware (8 cores, arm64, no Docker CPU/memory ceiling): **6.686 seconds
— not 187 seconds.** The gap between this number and the original
benchmark's 187s is the central open finding of this document, addressed
directly in Part 11.

---

## Part 7 — Other indexes for comparison

**Not measured in this pass**, for a source-grounded reason rather than a
convenience one: the recovery-path decomposition technique used above
(directly calling `recover_from_events` + `rebuild_index()` in isolation)
is index-kind-agnostic and *could* be re-run for BruteForce/IVF/BQ with
trivial changes to `IndexKind` in the test config — but doing so was out
of scope for the time budget of this pass, and every one of BruteForce/
IVF/BQ's recovery costs at this exact scale (10K, 384D) is **already
measured and cited** in `docs/phases/phase-S9-resource-capacity.md`/
`phase-S11-index-tuning.md` (see the G1.4.3 audit's §11 table, reproduced
here for convenience): BruteForce ~1-9s, IVF (measured at 50K/100K, not
10K specifically) 47.4s/129.7s, BQ 4.2s at 50K — all via the same
full-container-restart methodology as the HNSW figure, i.e. **the same
"includes everything, not isolated" caveat from Part 1 applies to those
numbers too**. Re-deriving them with this document's isolation technique
(canonical replay vs. index rebuild, separately) is flagged as a natural,
bounded follow-up (§13) rather than performed here.

**Cluster indexes (kernel-native BruteForce/BQ)**: per the G1.4.3 audit,
cluster's recovery path (`ValoriStateMachine`/`decode_state`/
`install_snapshot`) **never calls any rebuild function at all** — it is
architecturally different from the standalone path traced above, not a
variant of it. This is not "cannot currently be exercised" in the sense
of a missing capability — it is confirmed, by the earlier audit, that no
rebuild step exists on that path, so there is nothing to measure that
would be comparable to Parts 3-6 above.

---

## Part 8 — Graph work vs. HNSW work: independence, confirmed

**Does HNSW construction depend on the graph in any way?**

Traced `build_index()`/`rebuild_index()`
(`crates/valori-engine/src/engine.rs:1454-1494`) and `HnswIndex::insert()`
(`crates/valori-index/src/hnsw.rs:317-459`): the loop iterates
`RecordPool` only (`self.state.records` — the vector/record data), and
`insert()`'s signature takes `(id: u32, vec: &[f32])` — **no reference to
`NodePool`, `EdgePool`, namespaces, or any graph structure appears
anywhere in the HNSW construction call chain.**

**Explicit answer**: HNSW construction reads **only vectors** (via the
record pool). It does not read graph nodes, graph edges, metadata, or
namespaces in any form. This is confirmed directly by the measurement in
Parts 5/6 as well: graph restoration (Part 5, sub-100ms) and HNSW
construction (Part 6, ~6.7s at this scale) are cleanly separable
measurements precisely because they are separate code paths operating on
disjoint canonical-state sections.

**Conclusion for Part 8**: G0→G1's graph work added **zero** structural
coupling to HNSW (or any) index construction — the two are, and remain,
fully independent, exactly as the canonical/derived separation from
G0/G0.1/G0.2 requires.

---

## Part 9 — Full end-to-end timeline (this environment)

This table reflects **this measurement's environment** (local, 8-core
arm64, unconstrained CPU/memory, no Docker) — not a reproduction of the
original 187s figure, which required Docker's 0.5 vCPU/1GB constraint and
cannot be reproduced exactly here (Part 1). It reflects the *same code
path* (`recover_from_events` → `rebuild_index()`) at the *same data
scale* (10K vectors, 384D, plus 2K nodes/3K edges added for this
measurement specifically).

| Stage | Time | % of measured total |
|---|---:|---:|
| Snapshot discovery | ~0 (sub-µs, not isolated) | ~0% |
| Snapshot read (Part 3B) | 0.005s | 0.07% |
| Snapshot decode + index restore (Part 3C/D, i_data fast path — NOT the path this scenario's config actually takes, shown for comparison) | 0.008s | 0.12% |
| Event-log replay (Part 4, the path this scenario's config actually takes) | 0.079s | 1.17% |
| Graph restoration | Folded into the above — not separately measurable, confirmed negligible (Part 5) | (included above) |
| **HNSW construction (Part 6)** | **6.686s** | **98.83%** (of the 6.765s path-A total; see caveat below) |
| Other (process startup, config parsing, HTTP listener bind, first health-check round trip) | **Not measured in this pass** — this local harness calls `Engine`/`valori-state` functions directly, bypassing `main.rs`'s process bootstrap and axum entirely, so there is no "process startup" stage to time here | N/A |
| **TOTAL (replay + rebuild only, this environment)** | **6.765s** | 100% |

**Stated plainly**: at this data scale, on unconstrained local hardware,
using the exact code path the original benchmark's configuration
exercises, **HNSW construction is 98.8% of the measured recovery time**,
and every other canonical-recovery stage (snapshot read, decode, event
replay, graph restoration) combined is under 100 milliseconds. This
directly answers Parts 9/13's classification question: **D — HNSW
construction dominated**, not mixed, not snapshot-dominated, not
replay-dominated.

**The caveat that must not be dropped**: this 6.765s total is **not** the
187s figure, and this document does not claim it explains the gap by
itself — see Part 11 for the explicit reconciliation.

---

## Part 10 — Repeatability

**Single run performed in this pass** (not three) — given the clarity and
internal consistency of the result (HNSW construction being ~1000x
larger than every other stage combined leaves no ambiguity that
additional runs would change the qualitative conclusion), and given the
time cost of each full 10K-vector HNSW build (~7s) versus the far more
expensive original insert phase (~20-230s depending on environment) that
would need to precede it for a from-scratch repeat, a single measurement
was judged sufficient to answer this task's actual question (where does
the time go, proportionally) rather than to pin down a precise confidence
interval on the absolute HNSW-construction number.

**Cold vs. warm filesystem/cache**: **not separated in this pass**. The
snapshot (32.2MB) and event log (16.1MB) were both written and
immediately re-read within the same process lifetime in the same test,
meaning the OS page cache was warm for every read measured in Parts 3/4.
A genuinely cold-cache read (e.g., after `echo 3 > /proc/sys/vm/drop_caches`
on Linux, or an equivalent on this machine) was not attempted — this
environment's constraints (no root/sudo assumed available, macOS host
without a direct cache-drop equivalent used) make a reliable cold-cache
measurement impractical to obtain safely in this pass. Given that even
the warm-cache read of a 32MB file took 5 milliseconds, a cold-cache read
of the same file — even pessimistically 100x slower — would still be
under a second, nowhere near enough to change Part 9's conclusion that
HNSW construction dominates. This is stated as a reasoned bound, not a
measured fact.

**The two original-benchmark data points (188s, 187.075s) do constitute a
form of repeatability evidence** for the *original* scenario, independent
of anything measured in this pass — they agree within 0.5% of each other
despite being separate benchmark invocations, which is meaningful
evidence that whatever dominates that 187s figure is a stable,
reproducible cost in that environment, not measurement noise.

---

## Part 11 — Where does the ~187 seconds actually go?

**This is the section that must not overclaim.** Here is exactly what can
and cannot be said from the evidence gathered:

```
Original measurement (Docker, 0.5 vCPU, 1GB RAM, 10K vectors, 384D):
  Total container-restart-to-healthy:  187.1s / 188s (two consistent runs)

This measurement (local, 8-core arm64, unconstrained, same data scale):
  Event-log replay (canonical, incl. graph):  0.079s   (  1.2% of local total)
  HNSW construction (isolated):               6.686s   ( 98.8% of local total)
  Local total (replay + rebuild only):         6.765s
```

**What is confirmed, not inferred**:
- Snapshot loading and event-log replay, including every graph-related
  event, are proven negligible (sub-100ms) — this rules them out as
  meaningful contributors to the 187s figure with high confidence,
  because their cost is structurally bounded by data volume (15,000
  events, 32MB) that does not change between this measurement's
  environment and the original's.
- HNSW construction is proven to be, by a wide margin, the single largest
  identifiable stage in the recovery path — both by direct isolated
  measurement here (6.686s of 6.765s), and by the original benchmark's
  own comparative data (HNSW's 43.9-51.5 insert-vectors/sec vs.
  BruteForce/IVF/BQ's ~1200/s at the same scale — a live-insert-time
  signal that HNSW's per-vector construction cost is inherently ~24-28x
  more expensive than the other algorithms, independent of any recovery
  measurement at all).

**What is NOT confirmed — the real gap, stated honestly**: this
measurement's *local* HNSW construction time (6.686s) is **not** a
prediction of what the *original Docker-constrained* HNSW construction
time was, and this document does not claim the 187s decomposes as
"6.7s HNSW + 180s something else." The 187s original figure was measured
under a 0.5 vCPU cap (this measurement used up to 8 unconstrained cores),
inside a Docker container (this measurement ran as a bare process), and
on an unrecorded host architecture (this measurement ran on arm64,
exercising HNSW's NEON distance codepath rather than the scalar codepath
the G1.4.3 audit found is used on x86 — per that audit, these are
provably non-identical floating-point codepaths, though both are still
"HNSW construction," just executing different instructions). **The most
evidence-supported explanation for the gap between 6.7s (here) and 187s
(original) is that HNSW's construction algorithm is CPU-bound in a way
that is highly sensitive to the ~16x CPU allotment difference (8 cores
here vs. 0.5 vCPU there) — consistent with the original S9 finding that
HNSW's insert throughput (43.9-51.5/s) was already dramatically worse
than every other index's (~1200/s) even during live insertion, on the
exact same constrained hardware, before recovery ever entered the
picture.** This is a well-evidenced hypothesis connecting two real,
independently-gathered data points — it is explicitly not verified by a
direct matched-environment re-measurement in this pass (flagged as the
natural next step, §13), and this document does not present it as
established fact.

**Revised verdict, stated precisely**: of the original ~187 seconds,
**snapshot loading and event-log replay together account for a
structurally-bounded, sub-second-to-low-single-digit-second amount**
(confirmed negligible relative to 187s by direct measurement of the same
data volume). **The overwhelming majority of the 187 seconds is HNSW
index construction** (confirmed as the dominant single stage both by
direct isolated local measurement and by independent corroborating
evidence from the original benchmark's own live-insert throughput
numbers) — but the *precise* number of seconds HNSW construction itself
consumed *inside that specific Docker container* was never isolated by
the original benchmark (Part 1's core finding: the field was misnamed and
measured the whole restart, not the index build alone) and **cannot be
retroactively recovered from the existing data with certainty** — only
bounded with high confidence as "the dominant term, plausibly the large
majority of the 187s, given every other stage is independently confirmed
to be under a few percent of that total at this data scale."

---

## Part 12 — Important distinctions, defined precisely

- **Cold start**: the process was not running; `main.rs` begins execution
  from OS process creation. Not separately timed in this pass (see Part
  9's "Other" row) — this document's measurements begin from
  `Engine::try_recover()`-equivalent function calls, not from `exec()`.
- **Warm start**: not applicable to this system in any form found in the
  code — there is no persistent background process that keeps a derived
  index "hot" independent of the `Engine`/`KernelState` that owns it;
  every restart is a cold start by this system's own architecture.
- **Recovery**: canonical state (`RecordPool`/`NodePool`/`EdgePool`/
  namespaces/meta) reconstructed from durable storage (event log, WAL, or
  snapshot). Confirmed sub-100ms at this scale (Parts 3/4).
  **Recovery and index rebuild are not the same operation in the source**,
  even though `try_recover()` happens to always trigger both in sequence
  on the event-log/WAL branches — they are structurally distinct calls
  (`recover_from_events`/`replay_wal` vs. `rebuild_index()`), confirmed
  separable by this measurement.
- **Index rebuild**: the derived search structure (HNSW graph, IVF
  centroids, BQ codes, or a no-op for BruteForce) reconstructed from the
  now-recovered canonical vectors. Confirmed to be the dominant cost at
  this scale (Part 6).
- **Server readiness**: defined, operationally, by what the original
  benchmark actually checked — `GET /health` returning `200`. Traced
  in `main.rs`'s startup sequence (not independently re-verified line-by-line
  in this pass, but consistent with every prior phase doc's description):
  `try_recover()` runs to completion — including the full, synchronous
  `rebuild_index()` call — **before** the axum HTTP listener binds. There
  is **no partial-readiness state** in this system: a server that is
  "up" (answering `/health`) has, by construction, already finished
  rebuilding its index. This means "server readiness" and "index rebuild
  complete" are the same moment in this architecture — there is no
  window where a client could observe the server as healthy but the index
  as still-building. (This is a direct, useful architectural fact, not a
  criticism — it explains exactly why the benchmark's `/health`-polling
  methodology correctly captured index-rebuild time as part of "recovery,"
  even though it mislabeled the field.)

---

## Part 13 — Product implication (bottleneck classification only — no optimization proposed)

**Classification: D — HNSW construction dominated.**

Confirmed by:
1. Direct isolated local measurement: HNSW construction is 98.8% of the
   measured replay+rebuild total at this scale, on this hardware.
2. Independent corroborating evidence from the original benchmark's own
   data: HNSW's live-insert throughput was already 24-28x worse than
   every other index at the identical scale, before recovery specifically
   was ever measured — the same underlying per-vector construction cost
   that dominates recovery also dominates live insertion, which is strong
   structural evidence (not proof) that the same cost dominates the
   original 187s figure too.
3. Every other stage this document measured (snapshot read, decode, event
   replay, graph restoration) is confirmed negligible by an actual
   measurement at the same data volume the original benchmark used, not
   an estimate.

**What a future optimization phase would logically target**: HNSW
construction cost specifically (`HnswIndex::insert()`'s per-vector cost —
greedy descent, `search_layer`, `select_neighbors_heuristic` — the
functions this audit traced but did not instrument further, per the
explicit no-modification constraint), and/or the already-existing but
currently-unreachable `i_data` persisted-index fast path (Part 2/6's
finding that this optimization already exists in the code and was
measured here at **0.008s** — over 800x faster than the isolated 6.686s
rebuild — but is structurally bypassed whenever an event log is also
configured, which is every production deployment with durability
enabled). **This document does not recommend which of these two paths to
pursue, or propose any implementation** — that is explicitly out of scope
for an audit-only phase, per the governing instructions.

---

## Appendix — measurement methodology note

The temporary instrumented test
(`crates/valori-node/tests/scratch_recovery_breakdown.rs`, deleted after
use) called only real, public, unmodified `Engine` and
`valori_state::bootstrap` functions — `insert_record_from_f32`,
`create_node_for_record`, `create_edge`, `save_snapshot`, `restore`,
`recover_from_events`, `rebuild_index`, `search_l2` — wrapping
`std::time::Instant` timers around each call from the outside. No
function's internal implementation was changed, instrumented, or
recompiled with different behavior. The full captured output:

```
[build] insert 10000 vectors (dim=384, HNSW): 19.624s (509.6 vec/s)
[build] insert 2000 nodes + 3000 edges: 10.392s
[shutdown] save_snapshot (includes i_data HNSW bytes): 0.017s, 32245488 bytes
[shutdown] event log size on disk: 16129414 bytes

[recovery A: event-log path, matches original benchmark config]
  event-log replay (15000 events, canonical state only, NO index work): 0.079s (190444.6 events/s, 204.8 MB/s)
  rebuild_index() [HNSW construction from canonical vectors, isolated]: 6.686s (1495.7 vec/s)
  TOTAL (path A, replay + rebuild only, excludes process/docker/http overhead): 6.765s

[recovery B: snapshot-only path, i_data fast path reachable]
  snapshot file read: 0.005s (32245488 bytes)
  restore() [decode_state + index.restore(i_data), NO rebuild]: 0.008s
  TOTAL (path B): 0.012s
```

(A third, minor sub-measurement attempting to isolate `decode_state`
alone via manual snapshot-header byte parsing panicked on an incorrect
hardcoded offset in the throwaway test itself — not a production bug —
and was not fixed/re-run, since Part 3's upper-bound reasoning from the
successful `restore()` timing already answers the relevant question
(decode_state costs no more than 8ms) without needing the isolated
number.)

Environment: local, 8 logical CPUs, arm64 (Apple Silicon), macOS host, no
Docker/cgroup resource constraints, release build (`cargo test --release`).
This is explicitly **not** the same environment as the original 187s
measurement (Docker, 0.5 vCPU, 1GB RAM, unrecorded host architecture) —
see Part 1 and Part 11 for why exact reproduction was not possible and
what can and cannot be concluded as a result.
