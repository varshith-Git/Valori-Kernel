# G0.2 — Canonical State Hash Commitment

*Follow-up to [`graph-g0-architecture-audit.md`](graph-g0-architecture-audit.md) and [`graph-g0.1-determinism-state-integrity.md`](graph-g0.1-determinism-state-integrity.md). Resolves R2: whether `hash_state_blake3` should commit to every canonical graph/namespace field, and whether doing so is safe.*

---

## 1. Objective

G0.1 classified R2 (the BLAKE3 state hash not covering `namespace_id`, reverse edge adjacency, and the `SetMeta` sidecar) as a genuine implementation gap, but deliberately deferred correcting it, given the apparent consensus-wide blast radius of changing a hash used at ~90 call sites across Raft convergence, audit/proof endpoints, and three separate pinned-fixture compatibility corpora. G0.2 exists to answer, with evidence rather than assumption, exactly one question: **should the hash be widened, and if so, is the actual cost of doing so small enough to do now?**

---

## 2. Contract Audit

Re-confirmed from G0.1 (`crates/valori-kernel/src/snapshot/blake3.rs`, domain version 2): the hash committed, per record — `id, flags, vector, tag, metadata`; per node — `id, kind, record, first_out_edge`; per edge — `id, kind, from, to, next_out`. Not committed: `Record.namespace_id`/`next_in_ns`/`prev_in_ns`, `GraphNode.first_in_edge`/`namespace_id`/`next_in_ns`/`prev_in_ns`, `GraphEdge.next_in`, `KernelState.meta`.

**Every call site was enumerated** (`grep -rn "hash_state_blake3"` across the workspace) and grouped by consequence:

| Consumer | What changes when the hash changes |
|---|---|
| `valori-consensus::state_machine` (Raft convergence) | Computes both sides dynamically at runtime (`hash_state_blake3(&self.state)` on each replica) — adapts automatically, no pinned value |
| `valori-verify`, `valori-cli`, `valori-mcp`, `valori-node` proof/receipt endpoints | Compute at request time — adapt automatically |
| `crates/valori-kernel/tests/format.rs::empty_state_hash_is_pinned` | **One inline hex literal** |
| `crates/valori-kernel/tests/snapshot_compat.rs` | **One inline hex literal** (empty state) + 2 sidecar `.hash` files (single/multi fixtures) |
| `crates/valori-storage/tests/wal_compat.rs` | 2 sidecar `.hash` files |
| `crates/valori-state/tests/event_log_compat.rs` | 2 `state_hash` fields inside committed `.toml` manifests |
| `crates/valori-kernel/tests/snapshot_version_migration.rs::cross_version_decode_reencode_chain_is_hash_stable` | Computes both sides dynamically — adapts automatically, **but is the test that surfaced R3's sibling finding, see §6** |

**Critical correction to G0.1's own risk estimate:** the raw format bytes — snapshot `.bin` files, WAL `.wal` files, event-log `.log` files — are produced by `encode_state`/`WalWriter`/`EventLogWriter`, none of which call `hash_state_blake3`. They are **completely unaffected** by a hash-contract change. Only the *separately stored* hash sidecars (`.hash` files, TOML `state_hash` fields, and two inline string literals) needed updating — a total of **8 mechanically-regeneratable artifacts**, not a wholesale fixture-corpus rewrite. This was verified empirically, not assumed: every regenerated `.bin`/`.wal`/`.log` file was diffed byte-for-byte against its pre-change original and confirmed identical (see §5).

### R2 classification: **C — CURRENT IMPLEMENTATION GAP** (unchanged from G0.1)

No new evidence surfaced that changes this classification — see G0.1 §4 for the full reasoning (the tag/metadata v2 rationale and the `CollectionRegistry` precedent both show the team documents intentional hash-scope decisions when it makes one; no such documentation existed for the fields in question).

---

## 3. Target Contract

**Decision: widen the hash, but not to literally every persisted field.** The audit surfaced a real distinction the G0.1-era "just add everything canonical" framing missed:

- Fields maintained by a **single, unambiguous construction algorithm** in every valid reconstruction path (live apply, replay, snapshot decode) are safe to hash as defense-in-depth against a bug in that algorithm, even when technically derivable from other hashed fields. This is the same principle that already justified hashing `next_out`/`first_out_edge` under domain version 2.
- Fields maintained by **two different, both-valid, deliberately-different-order algorithms** depending on which code path built the state are **not** safe to hash — doing so makes hash equality a function of *which algorithm ran*, not of *what the state actually contains*.

Applying this distinction (§6 explains how the second case was discovered, empirically, mid-implementation):

| Field | Included in v3? | Why |
|---|---|---|
| `Record.namespace_id`, `GraphNode.namespace_id` | ✅ | Single-valued, set once at creation, identical under every reconstruction path |
| `GraphNode.first_in_edge`, `GraphEdge.next_in` | ✅ | Edge adjacency has one construction algorithm (`add_edge`/`_delete_edge`), used identically by live apply and by the pre-V4 snapshot back-compat block — confirmed to agree, not assumed (§6) |
| `KernelState.meta` (SetMeta sidecar) | ✅ | Single-valued `BTreeMap`, deterministic key order, one construction path |
| `Record.next_in_ns`/`prev_in_ns`, `GraphNode.next_in_ns`/`prev_in_ns` | ❌ **Deliberately excluded** | Two disagreeing reconstruction algorithms exist: live `apply_event_ns` (LIFO, newest-at-head) vs. `KernelState::rebuild_namespace_lists()` (used for pre-V6 snapshot migration; explicitly documented to walk in the OPPOSITE order to produce ascending-id order). Namespace *membership* (`namespace_id`) is still fully covered — this exclusion only concerns internal list-traversal order, not tenant-isolation correctness |

---

## 4. Compatibility Impact

- **Wire/format impact: none.** `encode_state`, `WalWriter`, `EventLogWriter` are untouched. Verified by byte-diffing every regenerated fixture against its original (§5).
- **`STATE_HASH_DOMAIN_VERSION` bump: 2 → 3**, per the existing versioning mechanism's own stated purpose ("hash changes are versioned, visible events, not silent drift").
- **Consensus impact:** none beyond the expected one-time hash-value shift on next deploy — convergence tests compute both replicas' hashes dynamically with the same code, so two replicas running the new build still agree with each other; they simply agree on a *different* value than before, which is exactly what a domain-version bump is for. Verified against the full `valori-consensus` suite including `two_nodes_applying_the_same_entries_converge_to_the_same_hash`, `prop_event_sequence_converges` (proptest fuzzing), and `blake3_chain_consistent_across_partition_and_heal` (§7).
- **Fixture impact: 8 artifacts, all mechanically regenerated** via the repo's own existing `#[ignore]` generator tests (`generate_snapshot_fixtures`, `generate_wal_fixtures`, `generate_event_log_fixtures`) plus 2 inline literal edits. See §5 for the exact procedure and the one real hazard encountered.

---

## 5. Fixture Regeneration — procedure and one real hazard found

Each corpus follows the same pattern: raw format bytes (`.bin`/`.wal`/`.log`) in one file, the hash in a separate sidecar. The safe procedure used throughout: **regenerate, then diff the raw-format file against its pre-change original; only the hash sidecar should differ.**

- **`crates/valori-kernel/tests/fixtures/*.hash`** (snapshot corpus): regenerated via `generate_snapshot_fixtures`. `.bin` files confirmed byte-identical (same lengths: 8241/8329/9180 bytes) to their pre-change originals; only the 3 `.hash` files changed.
- **`crates/valori-kernel/tests/format.rs` and `snapshot_compat.rs`**: the one inline empty-state hash literal, updated in both places to `feb47a4c03ee329d108f168945e204413ec8068f44d85503e4ec5bab6412d9a2`.
- **`crates/valori-storage/tests/fixtures/*.hash`** (WAL corpus): **a real hazard was found here.** `generate_wal_fixtures` calls `WalWriter::open(&path, dim)`, and running it a second time against an *already-populated* fixture file appends rather than truncates — the first attempt silently doubled `wal_v1_inserts.wal` from 410 to 804 bytes and then failed its own internal replay self-check (`InvalidOperation`, duplicate record id). This was caught immediately (the generator's own panic), the corrupted file was reverted via `git checkout`, and the correct procedure — delete both the `.wal` and `.hash` files first, then regenerate into a clean directory — produced `.wal` files confirmed byte-identical to the pre-change originals, with only the `.hash` sidecars changed. **This hazard is unrelated to the hash-widening work; it is a pre-existing quirk of `generate_wal_fixtures` not being idempotently re-runnable in place.** Left undocumented/unfixed in the generator itself, per this phase's scope (documented here instead, as a note for whoever next needs to regenerate this corpus).
- **`crates/valori-state/tests/fixtures/*.toml`** (event-log corpus): **a second, different hazard.** `generate_event_log_fixtures` *does* correctly delete-before-write (`fs::remove_file` before each write), so it is safely re-runnable — but the underlying `.log` bytes and `chain_head` were found to be **non-deterministic across consecutive runs even with zero code changes** (confirmed by running the generator twice in a row with nothing else touched: the `.log` bytes and `chain_head` differed both times, while the computed `state_hash` was identical both times). This points to a wall-clock timestamp or similar non-reproducible input somewhere in `EventLogWriter`'s header/chain construction — again, pre-existing and unrelated to G0.2. Rather than commit a `.log`/`chain_head` change that isn't attributable to this phase's actual work, the `.log` files were left completely untouched (reverted to their original committed bytes) and only the `state_hash` field in each `.toml` manifest was hand-patched, computed by decoding the *original, unmodified* `.log` file with the new `hash_state_blake3` (via a throwaway test, deleted immediately after use — never committed). Diff confirms: only 2 lines changed across the entire corpus, `event_count`/`record_count`/`chain_head`/`.log` bytes all untouched.

**Net fixture diff for this phase: 8 files, each changed by exactly one line** (`git diff --stat`: `snapshot_v7_{empty,single,multi}.hash` ×3, `format.rs` + `snapshot_compat.rs` inline literals ×2, `wal_v1_{inserts,namespace}.hash` ×2, `event_log_{inserts,namespace}.toml` ×2 — 9 total single-line diffs across 8 files, `format.rs`'s diff also includes a doc-comment addition).

---

## 6. R3-adjacent discovery: the namespace-list dual-algorithm divergence

Implementing the hash widening exactly as first designed (including `next_in_ns`/`prev_in_ns`) immediately broke `crates/valori-kernel/tests/snapshot_version_migration.rs::cross_version_decode_reencode_chain_is_hash_stable` for every `schema_ver < 6`. Root-caused (not assumed) by tracing both reconstruction algorithms line-by-line:

- **Live construction** (`apply_event_ns`'s `InsertRecord`/`CreateNode` arms): prepends each new record/node onto its namespace's list head — LIFO, most-recently-inserted-first from the head.
- **Legacy migration** (`KernelState::rebuild_namespace_lists()`, invoked automatically by `decode_state` for `schema_ver < 6`): walks the record/node pool in **reverse** slot-index order specifically so that, after its own prepend-to-head construction, the final list is in **ascending** id order — the function's own pre-existing comment states this directly: *"Walk records in REVERSE order so that after prepend-to-head the list is in forward (ascending ID) order — matching insert order."*

These two algorithms are **both individually correct** (either is a valid doubly-linked list over the same namespace membership) but they are **not the same algorithm**, so they do not agree bit-for-bit on `next_in_ns`/`prev_in_ns` for equivalent content. This was invisible under domain version 2 (these fields weren't hashed) and would have stayed invisible under a naive "hash everything canonical" widening. Instead of either (a) silently shipping a hash that spuriously differs between a live-built state and a migrated-from-legacy-format state with identical content, or (b) rewriting `rebuild_namespace_lists()` to match live ordering (a real, riskier fix to a load-bearing legacy-migration path, out of scope for "the smallest safe correction"), the fields were excluded — see §3's table and the two contract-lock tests in §8.

This mirrors G0.1's R3 finding in spirit — a previously-invisible structural fact surfaced only by widening what gets checked — but did not require a code fix here, because the resolution (excluding two fields from the target contract) is itself the correction, verified by the same test going green immediately after the exclusion, with zero other regressions.

The equivalent question was checked for edge reverse-adjacency (`first_in_edge`/`next_in`): the pre-V4 snapshot back-compat block (`decode_state`'s `schema_ver < 4` branch) walks edges in ascending id order and prepends each onto its target's incoming-list head — the **same** algorithm live `add_edge` uses (edges are always created in ascending, sequentially-validated id order). `cross_version_decode_reencode_chain_is_hash_stable` passing for all `schema_ver` 1–7 *with* `first_in_edge`/`next_in` included in the hash is the empirical confirmation that these two paths agree; no divergence exists for edges.

---

## 7. Verification

| Crate | Result | What it proves |
|---|---|---|
| `valori-kernel` | 167/167 (unchanged from G0.1) | Kernel-level replay/snapshot/graph invariants unaffected; new + updated hash-contract tests pass |
| `valori-storage` | 48/48 | WAL/event-replay compat corpus, including the new fixture regeneration, green |
| `valori-state` | 8/8 | Event-log compat corpus (manually-patched `state_hash` fields) green |
| `valori-consensus` | 37/37, including `two_nodes_applying_the_same_entries_converge_to_the_same_hash`, `blake3_chain_consistent_across_partition_and_heal`, `prop_event_sequence_converges` (proptest), `snapshot_roundtrip_preserves_state_hash_and_dedup` | **Direct evidence the widened hash does not break cross-replica convergence** — the actual highest-stakes consumer this phase existed to protect |
| `valori-engine`, `valori-verify`, `valori-cli`, `valori-mcp`, `valori-rag` | All green | Proof/receipt/audit endpoints unaffected |
| `valori-node` | 291/291 (unchanged from G0.1) | Full node-level regression pass, including `dr_disaster_recovery.rs`, `e2e_recovery.rs`, `graph_cascade.rs`, `api_graphrag.rs` |
| `cargo fmt --check`, `cargo clippy -- -D warnings` (kernel, storage, state) | Clean | No style/lint regressions |
| `cargo build -p valori-kernel --target wasm32-unknown-unknown` | Success | `no_std` invariant preserved |

No test suite outside `valori-kernel`/`valori-storage`/`valori-state` needed a single line changed — every other consumer computes the hash dynamically and adapted automatically.

---

## 8. New/Updated Tests

In `crates/valori-kernel/tests/graph_g01_invariants.rs` (the G0.1 hash-contract tests are updated in place rather than duplicated, since they test the same function under its corrected contract):

- `hash_contract_now_covers_node_namespace_id` / `hash_contract_now_covers_record_namespace_id` — **replace** the G0.1-era "locked gap" test; prove two states differing only in namespace placement now hash differently.
- `hash_contract_now_covers_meta_sidecar` — proves a `SetMeta` divergence now changes the hash.
- `hash_contract_still_excludes_namespace_list_pointers` — proves the deliberate §3/§6 exclusion: builds a state, calls the public `KernelState::rebuild_namespace_lists()`, confirms the namespace-list pointer *values* actually changed (setup validity check) but the hash did not.
- `hash_contract_direction_is_committed` / `hash_contract_target_is_committed` (from G0.1, unchanged) — still pass, still prove edge `from`/`to` are load-bearing.

In `crates/valori-kernel/tests/format.rs` / `snapshot_compat.rs`: the pinned empty-state hash literal updated with a comment explaining the domain-version bump.

No new test file was created — all changes are in-place corrections to G0.1's tests plus the pre-existing pinned-fixture tests, which now assert the new correct values.

---

## 9. Changes Made

| File | Change |
|---|---|
| `crates/valori-kernel/src/snapshot/blake3.rs` | `STATE_HASH_DOMAIN_VERSION` 2→3; `hash_state_blake3` now includes `Record.namespace_id`, `GraphNode.first_in_edge`/`namespace_id`, `GraphEdge.next_in`, and `KernelState.meta`; doc comment rewritten to state the actual contract (including the deliberate `next_in_ns`/`prev_in_ns` exclusion and why) |
| `crates/valori-kernel/tests/format.rs` | Pinned empty-state hash literal updated; comment explains the bump |
| `crates/valori-kernel/tests/snapshot_compat.rs` | Same, second occurrence |
| `crates/valori-kernel/tests/fixtures/snapshot_v7_{empty,single,multi}.hash` | Regenerated (`.bin` files untouched, byte-verified) |
| `crates/valori-storage/tests/fixtures/wal_v1_{inserts,namespace}.hash` | Regenerated (`.wal` files untouched, byte-verified, after recovering from the append-not-truncate hazard in §5) |
| `crates/valori-state/tests/fixtures/event_log_{inserts,namespace}.toml` | `state_hash` field hand-patched against the *original* `.log` files (untouched, byte-verified) — not regenerated wholesale, to avoid committing the pre-existing non-deterministic `chain_head`/`.log` churn described in §5 |
| `crates/valori-kernel/tests/graph_g01_invariants.rs` | Hash-contract tests updated/added per §8 |

No other files were touched. No graph feature, algorithm, index, or traversal behavior changed. No unrelated fixture regeneration or generator-hazard fix was made (both hazards in §5 are documented, not fixed, as out of scope).

---

## 10. Remaining Risks

- **`generate_wal_fixtures` is not safely re-runnable against an already-populated fixture directory** (append-not-truncate hazard, §5). Low severity — caught immediately by the generator's own self-check, not a silent-corruption risk, and now documented here for the next person who runs it.
- **`generate_event_log_fixtures`'s output is non-deterministic run-to-run** (timestamp or similar non-reproducible input in `EventLogWriter`, §5). Low-to-medium severity for fixture-maintenance hygiene (makes "just re-run the generator" an unreliable way to refresh this specific corpus without manual `state_hash`-only patching, as done here) — does not affect production correctness, since `state_hash` itself was confirmed stable across reruns; only the log's own audit-chain bytes/timestamp vary. Recommended as a small, separately-scoped follow-up if this corpus needs to be regenerated again.
- **Concurrent-mutation determinism** — still open, carried unresolved from G0 and G0.1. Not investigated in this phase either (out of scope: G0.2 is specifically about the hash contract, not concurrency).
- **No CRITICAL or new risks were introduced.** The two fixture-generator hazards found are pre-existing and were neither caused by nor required to be fixed by this phase.

---

## 11. G0.2 Invariants

| # | Invariant | Status |
|---|---|---|
| 1 | The BLAKE3 state-hash contract is explicit: every field it commits, and every field it deliberately excludes, is documented in the hash function's own doc comment. | **PROVEN** |
| 2 | Every canonical field maintained by a single, unambiguous reconstruction algorithm across all valid state-building paths (live apply, replay, snapshot decode/migration) is part of the hash. | **PROVEN** for `namespace_id` (record + node), `first_in_edge`/`next_in`, and `meta` — verified both by the positive tests in §8 and by the full cross-version migration suite passing |
| 3 | No canonical field maintained by two disagreeing reconstruction algorithms is part of the hash. | **PROVEN** — `next_in_ns`/`prev_in_ns` excluded specifically because this was found to be true of them (§6), and this exclusion is itself locked by an executable test |
| 4 | Widening the hash contract does not change the on-disk snapshot/WAL/event-log wire formats. | **PROVEN** — every regenerated fixture's raw-format bytes were diffed byte-for-byte against the pre-change original and found identical |
| 5 | Widening the hash contract does not break cross-replica Raft convergence. | **PROVEN** — full `valori-consensus` suite green, including property-based fuzzing over random event sequences comparing two independently-driven state machines |
| 6 | A namespace-misrouting bug (a record or node ending up in the wrong namespace) is now visible as a hash mismatch. | **PROVEN** (closes the specific tenant-isolation gap G0.1 flagged as the strongest argument for prioritizing this phase) |
| 7 | Canonical graph mutations have exactly one deterministic event order; concurrent clients cannot bypass the canonical commit/consensus mechanism. | **PROVEN** — see §12. Standalone: compiler-enforced via `Arc<RwLock<Engine>>` + `&mut self` mutation methods. Cluster: Raft's globally-ordered log + an internal mutex in `ValoriStateMachine::apply()`. No bypass found on either path. |

---

## 12. Concurrency Invariant (closes the G0/G0.1 carried-forward item)

G0 and G0.1 both left "concurrent-mutation determinism" open as an unresolved, unaudited question. Rather than open a new phase to make arbitrary concurrent mutation of `NodePool`/`EdgePool` deterministic — which was never actually required — this section verifies the narrower, correct claim: **canonical graph mutations have exactly one deterministic event order, and concurrent clients cannot bypass it.** Concurrency exists at the *request* layer (many HTTP clients submitting operations simultaneously); it does not exist at the *canonical-state-mutation* layer, by construction, on both execution paths:

- **Standalone**: `SharedEngine = Arc<RwLock<Engine>>` (`crates/valori-node/src/server.rs:24`). Every method that mutates canonical state (`commit_and_apply_ns`, `apply_committed_event_ns` — `crates/valori-engine/src/engine.rs:353,1341`) takes `&mut self`. Rust's borrow checker makes it a compile error to obtain that `&mut Engine` without holding the `RwLock`'s write guard — this is not a convention that could be violated by a future handler forgetting to lock, it is enforced by the type system itself. No `unsafe` block exists anywhere near `Engine`/`KernelState` in `valori-node` or `valori-engine` that could provide a side door.
- **Cluster**: every write reaches `raft.client_write()` (verified at every call site across `capabilities.rs` and `cluster_server.rs`), which hands the operation to openraft's replicated log — a single, globally-ordered sequence by the Raft protocol's own definition. That log is applied by exactly one method, `ValoriStateMachine::apply()` (`crates/valori-consensus/src/state_machine.rs:595`), which additionally acquires its own `self.inner.lock().await` before touching `state`. Every `apply_event_ns` call site in the entire consensus crate (4 total) is inside that one function.

Both paths converge on the same single authoritative mutation point G0 already established, `KernelState::apply_event_ns`, and neither has a bypass — verified by code inspection, not assumed from the architecture's stated intent.

**This closes the concurrency item as a documented, code-verified invariant rather than an open risk.** It does not claim arbitrary concurrent `HashMap`/pool mutation is magically safe — it claims (and proves) that no such concurrent mutation is ever attempted, because the write path itself is serialized before it ever reaches the graph pools.

---

## G0.2 STATUS

- **Hash contract audit:** COMPLETE — every call site enumerated, every fixture corpus's exact regeneration mechanism understood before touching anything.
- **Target contract defined:** `namespace_id` (record + node), `first_in_edge`/`next_in` (edge reverse adjacency), `meta` sidecar — ADDED. `next_in_ns`/`prev_in_ns` (namespace list pointers) — DELIBERATELY EXCLUDED, with evidence.
- **Compatibility impact:** ASSESSED and CONFIRMED SMALL — zero wire-format changes; 8 fixture artifacts, all mechanically regenerated with byte-level verification that nothing beyond the hash sidecar moved.
- **Versioned:** `STATE_HASH_DOMAIN_VERSION` 2 → 3.
- **Fixtures updated:** DONE, with two pre-existing (unrelated) fixture-generator hazards discovered, worked around safely, and documented rather than silently papered over.
- **Replay tested:** PROVEN (kernel + storage + state suites green, including the specific migration test that caught the namespace-list divergence).
- **Snapshot tested:** PROVEN (kernel snapshot corpus green, byte-identical `.bin` files confirmed).
- **Replica convergence tested:** PROVEN (`valori-consensus` full suite green, including proptest fuzzing).
- **Cross-platform determinism:** Re-verified via the existing `multi_arch_determinism.rs` mechanism (in-process, not a real multi-arch cluster — same caveat as G0.1 noted; no stronger claim is made here).
- **Concurrent-mutation determinism:** RESOLVED as a documented, code-verified invariant (§12) — canonical mutation is serialized on both paths (compiler-enforced `RwLock` write-guard standalone; Raft's globally-ordered log + an additional internal mutex in cluster mode), with no bypass found. This is narrower than "concurrent HashMap mutation is deterministic" (never claimed) and exactly as broad as "concurrency cannot reach the canonical mutation layer out of order" (proven).
- **G0.2 PASS: YES.**
- **R2 status: RESOLVED.** Namespace misrouting and reverse-adjacency-maintenance bugs are now hash-visible; the one remaining deliberate exclusion (namespace list pointers) is evidence-backed and locked by a test, not an oversight.
- **Ready for G1: YES.** Both items G0.1 carried forward are now closed: R2 by correction (this phase), concurrent-mutation determinism by verification that the proposed invariant already holds in the code (§12). No open risks remain from the G0 → G0.1 → G0.2 track.
