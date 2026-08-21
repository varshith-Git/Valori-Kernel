# G0.1 Graph Determinism & State Integrity

*Follow-up to [`graph-g0-architecture-audit.md`](graph-g0-architecture-audit.md). Source changes ARE in scope for this phase, but only to resolve R1, correct R2 if the audit proves correction is required, add the invariant tests G0 identified as missing, and document the results. No new graph features, algorithms, indexes, or traversal work were added — see §11 for the exact diff.*

---

## 1. Objective

G0 established that Valori's graph (`NodePool`/`EdgePool` inside `KernelState`) is genuinely canonical, event-sourced, and snapshotted — but flagged two real risks (R1: a namespace-blind replay function coexisting under the same name as the real one; R2: the BLAKE3 state hash not covering every canonical field) and several invariants that were true by code inspection but not proven by an executable test. G0.1 exists to close those gaps — resolving R1, formally establishing the hash contract (R2), and adding the tests needed to treat "canonical graph state is reconstructible, deterministic, and correctly committed" as an enforced contract rather than an audit finding.

---

## 2. G0 Findings Revalidated

Both G0 findings were re-verified against the current source tree before any change was made, and **both required correction to G0's own characterization**:

- **R1 was more severe than G0 stated.** G0 described it as "a namespace-blind function exists and might be confused with the real one." Re-verification found `crates/valori-kernel/src/replay_events.rs` was never declared via `mod`/`pub mod` anywhere in `crates/valori-kernel/src/lib.rs` — the file was not part of the compiled crate at all, not reachable via any import path, and not exercised by its own `#[cfg(test)]` module (which tested unrelated `EventJournal`/`EventLogFile` types defined in the same file). It was fully orphaned dead code, not a live landmine. See §3.
- **A third, more serious issue was discovered during G0.1 itself, not present in G0's findings at all.** Proving graph snapshot equivalence (Phase 4) surfaced a real decode bug: deleting any node or edge that is not the most-recently-created one, then taking a snapshot, made the snapshot **fail to decode** (`KernelError::InvalidOperation`). This is now tracked as **R3** (§6) and was fixed in this phase, since it directly blocked G0.1's core objective #4.

---

## 3. R1 Resolution

**Exact problem** (re-verified, not assumed): `crates/valori-kernel/src/replay_events.rs` defined `pub fn replay_events(events: &[KernelEvent]) -> Result<KernelState>`, which applied every event via `state.apply_event(evt)` — a wrapper that always uses `DEFAULT_NS` (kernel.rs). A namespace-aware production implementation, `pub fn replay_events(events: &[(u16, KernelEvent)]) -> Result<KernelState>`, already existed and is actually used, in `crates/valori-storage/src/events/event_replay.rs:43-54`, called from `recover_from_event_log()` → `valori_state::bootstrap::recover_from_events()` → `Engine::try_recover()` (`crates/valori-engine/src/engine.rs:1557`).

**Evidence the orphaned copy was unreachable, not merely unused:**
- `grep -rn "mod replay_events" crates/valori-kernel/src/` returned nothing — no file declares this module.
- `cargo check -p valori-kernel` compiled clean with zero warnings referencing it, because the compiler never sees the file (Rust only compiles files reachable from the crate root via `mod` declarations).
- `grep -rn "replay_events::" crates/` (workspace-wide) found no external reference to it under any path.
- The apparent "user" found by G0 (`crates/valori-storage/tests/wal_validation.rs:28`) imports `replay_events` from `valori_storage::events::event_replay` — the real, namespace-aware function — not from the kernel. G0's grep match was a false positive on the identical function *name*, not evidence of a shared caller.

**Chosen resolution: delete the orphaned file.** Not a rename, not a re-export — the file was never part of the compiled crate, so deleting it changes zero runtime behavior and zero public API (it was never public API to begin with). This directly removes the root cause of the confusion G0 flagged: a future engineer cannot resurrect a namespace-blind "the" replay function by mistake, because there is nothing left to resurrect under that name.

**Why this is safe:**
- `cargo check --workspace` after deletion: clean, zero errors (confirms nothing else in the workspace referenced it, directly or transitively).
- Full `cargo test -p valori-kernel` after deletion: 167/167 passing (baseline 154 + 13 new G0.1 tests; see §10), zero regressions.
- The real production replay path (`valori-storage`'s implementation) is untouched and is now, unambiguously, the only implementation named `replay_events` reachable from anywhere in the workspace.

**Additional hardening added** (not required for R1 itself, but directly strengthens confidence in the real replay path — see §5/§7): a new test, `graph_events_recover_into_their_own_namespace_and_reject_cross_ns_edges_on_replay` (`crates/valori-storage/src/events/event_replay.rs`, in the existing `#[cfg(test)] mod tests`), proves the real production path replays graph nodes/edges into their correct namespace across a full disk round-trip, and rejects a cross-namespace edge written directly into the event log — i.e., the namespace invariant cannot be bypassed by handing replay a malformed or adversarial log, only by going through the same `apply_event_ns` gate live writes use.

---

## 4. R2 Hash Contract

### What the audit found

`hash_state_blake3()` (`crates/valori-kernel/src/snapshot/blake3.rs:73-159`) hashes, in pool order: per record — `id, flags, vector, tag, metadata`; per node — `id, kind, record, first_out_edge`; per edge — `id, kind, from, to, next_out`. It does **not** hash: `Record.namespace_id`/`next_in_ns`/`prev_in_ns`, `GraphNode.first_in_edge`/`namespace_id`/`next_in_ns`/`prev_in_ns`, `GraphEdge.next_in`, or `KernelState.meta`. All of these fields are canonical (event-sourced, `pub(crate)`-mutated only via `apply_event_ns`, and included in the snapshot per `encode_state`).

### Evidence gathered on intent

- `hash_state_blake3`'s own doc comment documents a **versioned, deliberately-scoped** hash input: `STATE_HASH_DOMAIN_VERSION: u8 = 2`, with the comment "v2 = added domain separation + tag/metadata coverage" — proving the team has, at least once, deliberately widened the hash's coverage with an explicit rationale ("Leaving them out of the hash would let replicas diverge invisibly").
- `crates/valori-kernel/src/verify.rs`'s doc comment: *"State hashing is handled exclusively by `hash_state_blake3` — the single canonical function used by the consensus layer, the verifier, and all proof endpoints. Do not add a second state-hash function here."* — this function is treated project-wide as the sole correctness/convergence signal.
- `hash_state_blake3` has ~90 call sites across `valori-consensus` (Raft state-machine convergence, `state_machine.rs:197-209`), `valori-verify`/`valori-cli` (audit/proof), `valori-mcp` (receipts), `valori-node` (proof endpoints, disaster-recovery tests), and pinned-fixture compatibility corpora in three crates: `crates/valori-kernel/tests/snapshot_compat.rs`, `crates/valori-storage/tests/wal_compat.rs`, `crates/valori-state/tests/event_log_compat.rs`. All three corpora pin literal hash values (or `.hash` files / TOML manifest fields) against committed binary fixtures, with `snapshot_compat.rs`'s own doc comment stating: *"If a test fails it means the snapshot format, state-hash domain, or the encoder changed in a way that breaks backward compatibility — that is a breaking change and must be treated as such, not silently fixed by regenerating the fixture."*
- One directly relevant, but distinct, precedent was found: `crates/valori-consensus/src/state_machine.rs:170-171` explicitly documents excluding a *different* piece of namespace-related state (`namespace_registry: CollectionRegistry`, the name↔id mapping) from the hash, with a stated reason: *"Not part of the BLAKE3 state hash — replicated via Raft, converges identically."* This is **not** the same field G0 flagged (that's a separate struct on `ValoriStateMachine`, not `Record.namespace_id`/`GraphNode.namespace_id` inside `KernelState` itself), but it shows the team does actively reason about hash-scope decisions elsewhere in the codebase — and no equivalent comment exists anywhere near `hash_state_blake3` explaining the `namespace_id`/`first_in_edge`/`next_in`/`meta` omissions.

### R2 classification: **C — CURRENT IMPLEMENTATION GAP**

Not D (insufficient evidence) — there is a clear, consistent pattern (the tag/metadata v2 rationale, and the separate `CollectionRegistry` precedent) showing the team documents intentional hash-scope decisions when they make one, and no such documentation exists for the fields G0 flagged. Not B (intentionally partial with documented rationale) for the same reason — the silence is not itself evidence of intent, especially given the "would let replicas diverge invisibly" reasoning that justified widening the hash for tag/metadata applies with equal force to `namespace_id` (tenant-isolation correctness) and to `first_in_edge`/`next_in` (reverse-adjacency correctness, exactly the kind of implementation-bug-detection value `next_out`/`first_out_edge` already provide as defense-in-depth, since — as this audit's deeper analysis showed — those *forward* pointers are themselves technically derivable from the `(id, from, to)` edge set alone, and are hashed anyway specifically to catch bugs in the adjacency-maintenance code, not because they are new information).

### Decision: gap confirmed, but **not corrected in this pass**

Widening `hash_state_blake3`'s coverage is exactly the kind of change `STATE_HASH_DOMAIN_VERSION` exists to support (bump 2 → 3, re-run the fixture-generation tooling, commit new fixtures) — but it is a **wide-blast-radius, cluster-consensus-affecting change**: every one of the ~90 call sites is a potential behavior-change surface, and all three pinned-fixture compatibility corpora would need regeneration and re-review by their own explicit "this is a breaking change" policy. G0.1's scope rules explicitly limit source changes to "resolving R1, correcting R2 **if the audit proves correction is required**" — the audit proves a gap exists, but does not, by itself, make widening the hash the smallest safe correction available *right now*. Consistent with "Do NOT modify hashing immediately" and "if the answer is D, STOP the hash modification portion" (this case sits closer to that caution than its C-classification alone would suggest, given the consequences of getting a consensus-wide hash change wrong are severe and this repo has no multi-node cluster available in this environment to integration-test a convergence-behavior change against).

**What was done instead — the smallest safe action available:**
1. The current contract is now **explicitly documented** (this section) instead of being an implicit fact only visible by reading the hasher line-by-line.
2. The current contract is **locked down by an executable test** — `hash_contract_currently_does_not_cover_node_namespace_id` (`crates/valori-kernel/tests/graph_g01_invariants.rs`) constructs two states differing only in a node's namespace and asserts they hash identically today, with a comment explaining this documents a known gap, not a bug, and instructing whoever changes the contract in the future to update this test's expectation rather than treat its failure as a regression.
3. Two **positive** hash-contract tests were also added, proving the fields that ARE committed really are load-bearing: `hash_contract_direction_is_committed` (A→B must not hash the same as B→A) and `hash_contract_target_is_committed` (A→B must not hash the same as A→C) — both pass today, confirming the hash is not vacuously weak, just narrower than "every canonical field."
4. `first_in_edge`/`next_in`/`meta` coverage gaps could not be demonstrated by an executable black-box test through the public API alone — because the deterministic construction algorithm (`add_edge`/`_delete_edge`) makes it impossible to organically produce two states that differ *only* in these fields without also differing in a field that *is* hashed (the edge set itself). Their exclusion is established here by direct code citation (the absence of `node.first_in_edge`, `edge.next_in`, and any `state.meta` handling in `hash_state_blake3`'s ~85-line body is unambiguous by inspection) rather than by a white-box unit test, to avoid reaching into `pub(crate)` internals for a fact already provable by reading the function.
5. The widening itself is recommended as a dedicated, separately-reviewed follow-up (§12), not executed here.

---

## 5. Replay Equivalence

**Claim:** `S1 = apply(S0, E)` and `S2 = replay(E)` produce field-identical canonical graph state.

**How it was tested:** at the kernel layer, "replay" *is* re-invoking `apply_event_ns` in event order — this is exactly what the real production replay function does (`valori-storage`'s `replay_events`, §3). `graph_replay_produces_field_identical_state` and `graph_replay_is_stable_across_three_independent_applications` (`crates/valori-kernel/tests/graph_g01_invariants.rs`) build two (and then three) independent `KernelState`s from the identical event sequence — a realistic graph scenario using only real `KernelEvent` variants: `InsertRecord`, `CreateNode` ×3, `CreateEdge` ×4 (including a self-loop and a duplicate), `DeleteEdge`, `DeleteNode`, spanning two namespaces — and compare them via `assert_graph_states_equivalent`, a helper that checks, per Phase 3's explicit requirement to go beyond counts: every node's `(id, kind, record, namespace_id)`, every node's outgoing AND incoming adjacency (by id, in order), every edge's `(id, kind, from, to)`, and per-namespace record membership. Also confirmed hash-equal as a secondary check.

At the storage layer, `graph_events_recover_into_their_own_namespace_and_reject_cross_ns_edges_on_replay` (§3) proves the same property end-to-end through the real disk-backed `EventLogWriter` → `recover_from_event_log` path, including namespace correctness and cross-namespace-edge rejection surviving the log round-trip.

**Result: PROVEN** for the event types and topology exercised (node/edge creation, multiple edges, self-loop, duplicate edge, deletion with cascade, namespace placement). Not proven for every conceivable interleaving (e.g., concurrent/parallel mutation — see §9's concurrency caveat, carried over unresolved from G0).

---

## 6. Snapshot Equivalence

**Claim:** `S1 → snapshot → restore → S3` produces field-identical canonical graph state.

**How it was tested:** `graph_snapshot_restore_produces_field_identical_state` (`crates/valori-kernel/tests/graph_g01_invariants.rs`) encodes the same graph-and-namespace-inclusive scenario used for replay testing (§5) and decodes it, then runs the same field-by-field `assert_graph_states_equivalent` comparison (not just hash/count equality — the existing `roundtrip_preserves_state_hash` test in `snapshot_roundtrip.rs` already covered that weaker claim; this is additive).

### R3 — discovered during this proof, not anticipated by G0

Building this test surfaced a real bug: `snapshot_restore_produces_field_identical_state` initially failed with `decode: InvalidOperation` whenever the scenario deleted a node/edge that was **not** the most-recently-created one.

**Root cause:** `encode_state` (`crates/valori-kernel/src/snapshot/encode.rs`) writes only *live* node/edge entries for the nodes and edges sections (unlike the records section, which writes an explicit present/absent flag for every slot including holes). The written `node_count`/`edge_count` header fields are therefore live counts, not total-slot counts. `decode_state` pre-sized its in-memory pools to exactly that live count and rejected any entry whose `id` was `>= live_count`. Since ids are permanently allocated at creation and never reused (per the pool's tombstone design, matching G0's node/edge lifecycle findings), deleting anything but the tail node/edge leaves the surviving highest-id entry `>= post-deletion live_count` — decode rejected it as corrupted, even though the snapshot was produced by the project's own encoder from valid state. This affected `Engine::restore()` and thus both `try_recover()`'s snapshot-recovery path and any explicit `POST /v1/restore`-style flow, for any project that had ever deleted a non-tail graph node or edge.

**Fix (in `crates/valori-kernel/src/snapshot/decode.rs`):** decode-side only, no wire-format change. Instead of pre-sizing the node/edge pools to the live count read from the header and validating `id_val < live_count`, the decoder now grows each pool dynamically to fit the ids actually encountered (`if id_val >= pool.len() { pool.resize(id_val + 1, None) }`), bounded by `MAX_NODES`/`MAX_EDGES` to preserve the existing untrusted-input DoS guard (an adversarial `id_val` still cannot drive an unbounded allocation). Edge-endpoint validation and the V6+ namespace-node-head validation were updated to bound against the node pool's actual (now hole-correct) length instead of the wire-supplied live count. **The bytes `encode_state` produces are completely unchanged** — this is why the fix required zero fixture regeneration and zero schema-version bump: every snapshot ever written by this project's encoder (including the committed fixtures in `snapshot_compat.rs`) decodes identically to before, and snapshots with holes — which no prior test happened to construct — now additionally decode correctly instead of being rejected.

**Regression coverage added:** two standalone, minimal tests in `crates/valori-kernel/tests/snapshot_roundtrip.rs` — `snapshot_roundtrips_after_deleting_a_non_tail_node` and `snapshot_roundtrips_after_deleting_a_non_tail_edge` — each construct the smallest possible repro (3 nodes/edges, delete the middle one, snapshot, decode, assert success + hash equality + correct liveness), in addition to the broader scenario-level proof in `graph_g01_invariants.rs`.

**Verification that the fix caused zero regressions:** full `cargo test -p valori-kernel` (167/167), `cargo test -p valori-storage` (48/48), `cargo test -p valori-node` (291/291), `cargo test -p valori-consensus` (37/37 across all listed suites), `cargo test -p valori-engine`/`valori-verify`/`valori-state`/`valori-cli`/`valori-mcp` all green, and `cargo build -p valori-kernel --target wasm32-unknown-unknown` still succeeds (the `no_std` invariant is preserved — the fix uses only `alloc::vec::Vec::resize`, already used elsewhere in this file).

**Result: PROVEN** for the tested topology, and R3 is now closed (fixed, not merely documented) — this diverges from R1/R2's "resolve or document" outcomes because, unlike those, the smallest safe correction here was genuinely small (decode-only, byte-format-preserving, zero fixture impact) rather than wide-blast-radius.

---

## 7. Namespace Invariants

Tested at the **kernel level** (`KernelState::apply_event_ns`), not the HTTP/API layer, per the explicit instruction:

- `cross_namespace_edge_is_rejected_at_the_kernel_apply_layer` — two nodes in different namespaces; `CreateEdge` between them is rejected, edge count stays 0.
- `same_namespace_edge_is_accepted` — negative-space control: identical setup but both nodes in namespace 0; the edge succeeds. Confirms the rejection above is specifically about the namespace mismatch.
- `node_record_linkage_must_match_namespace` — a node cannot be created referencing a record from a different namespace.
- `graph_events_recover_into_their_own_namespace_and_reject_cross_ns_edges_on_replay` (`valori-storage`, §3) — proves the invariant holds not just for live API calls but for **replay of an adversarial event log** that attempts a cross-namespace edge directly, bypassing any API-layer validation entirely. Recovery fails the same way live application would.

**Result: PROVEN**, both for live application and for replay — closing G0's explicit gap ("no dedicated test found" for cross-namespace-edge rejection).

---

## 8. Edge Semantics

**Duplicate edges:** `duplicate_edges_are_allowed_and_independently_tracked` (`graph_g01_invariants.rs`) documents the actual, verified contract: creating the same `(from, to, kind)` tuple multiple times is **accepted every time**, is **not idempotent** (each call allocates a new, distinct `EdgeId`), and all duplicates appear in both adjacency directions. No behavior was changed — `add_edge` (`crates/valori-kernel/src/graph/adjacency.rs`) performs no dedup check and none was added, per the instruction not to change behavior merely because another behavior seems preferable.

**Self-loops:** G0 found deletion coverage already existed (`crates/valori-node/tests/graph_cascade.rs::test_delete_node_with_self_loop`) but not creation + both-adjacency-directions + snapshot + replay together, and not at the kernel-crate level. `self_loop_creation_appears_in_both_adjacency_directions` (`graph_g01_invariants.rs`) closes that gap: creates a self-loop, confirms it appears in both `outgoing_edges` and `incoming_edges`, confirms it survives a snapshot round-trip, and confirms it survives replay. The existing `graph_cascade.rs` deletion test was **not duplicated**.

**Deletion:** unchanged from G0's findings — cascade-delete on `DeleteNode` removes every incident edge before the node slot itself is cleared; `DeleteEdge` unlinks from both adjacency lists. No new deletion-semantics test was needed; existing coverage (G0 §7, §17) plus the new snapshot/replay-inclusive tests here are sufficient.

---

## 9. Determinism

Re-run against the current source tree (not re-derived from G0's list without verification):

| Concern | Classification | Note |
|---|---|---|
| `HashMap`/`HashSet` in `NodePool`/`EdgePool` | SAFE | both are plain `Vec<Option<T>>`; confirmed unchanged |
| `HashSet<u32>` visited-set in `expand_subgraph` BFS | SAFE | membership test only, output order comes from `Vec` push order, not hash iteration — now backed by an executable test (`traversal_output_is_deterministic_across_repeated_runs`, §9a) rather than code-reading alone |
| `HashMap<u32,u32>` in `resolve_seed_nodes` | SAFE | lookup by known key only |
| Node/edge IDs | SAFE | sequential slot allocation, never random, never reused |
| Timestamps on graph structures | SAFE | none exist on `GraphNode`/`GraphEdge` |
| Floating point in graph structures | SAFE | all fields integer/enum-as-u8 |
| Snapshot serialization ordering | SAFE | `Vec` index order; now additionally proven correct in the presence of holes (§6/R3) |
| Concurrent/parallel mutation of `KernelState` | **NOT PROVEN either way** | carried over unresolved from G0 — this pass did not trace every write-lock acquisition site; flagged again in §12 |
| Kernel-level namespace-blind `replay_events` | RESOLVED (was: CONFIRMED VIOLATION of intended semantics) | deleted, §3 |
| BLAKE3 hash coverage gap (`namespace_id`/`first_in_edge`/`next_in`/`meta`) | CONFIRMED, documented, locked by test | §4, not corrected this pass |

### 9a. Deterministic traversal

`traversal_output_is_deterministic_across_repeated_runs` (`crates/valori-rag/src/graph.rs`, extending the existing `#[cfg(test)] mod tests`) builds a nontrivial graph (a 4-node diamond with fan-out, a shared descendant reachable by two paths, and a self-loop — stronger than the pre-existing empty-input-only tests) and runs `expand_subgraph` three times against the same state (T1==T2==T3), then again against an independently-rebuilt but event-identical state, comparing both node and edge output **including order**, not just set membership.

**Result: PROVEN** for this topology. Traversal is a pure derived computation (§4 of the G0 doc) — it has no bearing on the state hash, so its determinism requirement is the weaker "same graph, same output" kind, not the stronger "must be part of the canonical commitment" kind; this distinction is preserved as-is from G0, not changed here.

---

## 10. Test Matrix

| Property | Existing test | New test (this phase) | Result |
|---|---|---|---|
| Node lifecycle | `state_machine.rs::node_and_edge_lifecycle`, `node_referencing_missing_record_is_rejected` | — | TESTED (unchanged) |
| Edge lifecycle | same | — | TESTED (unchanged) |
| Duplicate edges | — | `duplicate_edges_are_allowed_and_independently_tracked` | **PROVEN** (contract documented) |
| Self-loops | `graph_cascade.rs::test_delete_node_with_self_loop` (deletion only) | `self_loop_creation_appears_in_both_adjacency_directions` (creation + adjacency + snapshot + replay) | PROVEN |
| Namespace isolation (cross-namespace edge) | — | `cross_namespace_edge_is_rejected_at_the_kernel_apply_layer`, `same_namespace_edge_is_accepted`, `node_record_linkage_must_match_namespace`, replay-side equivalent in `event_replay.rs` | **PROVEN**, kernel-level AND replay-level |
| Replay equivalence (graph-inclusive) | `determinism.rs::two_identical_builds_produce_identical_snapshot_bytes` (indirect) | `graph_replay_produces_field_identical_state`, `graph_replay_is_stable_across_three_independent_applications` (direct, field-by-field) | **PROVEN** |
| Snapshot equivalence (graph-inclusive, field-level) | `snapshot_roundtrip.rs::roundtrip_preserves_state_hash` (hash+count only) | `graph_snapshot_restore_produces_field_identical_state` (field-by-field) | **PROVEN**; also surfaced and fixed R3 |
| Reverse adjacency (first_in_edge/next_in) survives snapshot | `graph_cascade.rs::test_snapshot_preserves_reverse_index` | (covered by the tests above too) | TESTED |
| Deterministic ordering (BFS traversal) | trivial empty-input only | `traversal_output_is_deterministic_across_repeated_runs` | **PROVEN** for a nontrivial graph |
| Graph hash contract | — | `hash_contract_direction_is_committed`, `hash_contract_target_is_committed`, `hash_contract_currently_does_not_cover_node_namespace_id` | **PROVEN** (both what IS and what is NOT committed, as of today) |
| Restart/crash recovery (graph-inclusive) | `dr_disaster_recovery.rs` (vector-only) | `graph_events_recover_into_their_own_namespace_and_reject_cross_ns_edges_on_replay` | **PROVEN** via the real disk-backed recovery path |
| Non-tail node/edge deletion + snapshot | — (bug, undetected) | `snapshot_roundtrips_after_deleting_a_non_tail_node`, `snapshot_roundtrips_after_deleting_a_non_tail_edge` | **PROVEN** (bug fixed) |

---

## 11. Changes Made

| File | Change | Reason |
|---|---|---|
| `crates/valori-kernel/src/replay_events.rs` | **Deleted** | R1 — orphaned, unreachable, namespace-blind dead code; see §3 |
| `crates/valori-kernel/src/snapshot/decode.rs` | Nodes/edges sections: pool arrays now grow to fit encountered ids instead of pre-sizing to the (incorrect, live-only) wire count; edge-endpoint and namespace-node-head bounds checks now use the actual pool length | R3 — fixes the non-tail-deletion snapshot decode bug; see §6. Decode-only, wire-format-preserving |
| `crates/valori-kernel/tests/graph_g01_invariants.rs` | **New file**, 11 tests | Phases 3, 4, 5, 6, 7, 10 — replay equivalence, snapshot equivalence, namespace invariants, duplicate-edge contract, self-loop creation, hash-contract lock |
| `crates/valori-kernel/tests/snapshot_roundtrip.rs` | +2 tests | R3 regression coverage — minimal standalone repros for non-tail node/edge deletion + snapshot |
| `crates/valori-storage/src/events/event_replay.rs` | +1 test in the existing `#[cfg(test)] mod tests` | Phases 5, 11 — graph+namespace recovery through the real production replay path, plus cross-namespace-edge rejection surviving replay |
| `crates/valori-rag/src/graph.rs` | +1 test in the existing `#[cfg(test)] mod tests` | Phase 9 — deterministic-traversal proof on a nontrivial graph |
| `docs/reviews/graph-g0.1-determinism-state-integrity.md` | **New file** | This document |

No other files were touched. No API was redesigned. No new graph features, algorithms, or indexes were added. No performance work was done.

---

## 12. Remaining Risks

- **R2 (BLAKE3 hash coverage gap) — still open, by deliberate choice.** `namespace_id` (record + node), `next_in_ns`/`prev_in_ns` (record + node), `first_in_edge`/`next_in` (graph reverse-adjacency), and `KernelState.meta` are canonical but not hashed. The gap is now documented and locked by an executable test (§4) so it cannot silently widen further, but the underlying hash contract has not been corrected. **Recommended as a dedicated follow-up phase** (not G1 — this is still a G0-family integrity item), scoped to: bump `STATE_HASH_DOMAIN_VERSION` 2→3, add the missing fields to `hash_state_blake3`, regenerate the pinned fixtures in `snapshot_compat.rs`/`wal_compat.rs`/`event_log_compat.rs`, and specifically verify the change against `valori-consensus`'s cross-replica convergence tests (`state_machine.rs`, `partition_scenarios.rs`) since that is the highest-stakes consumer of this hash.
- **Concurrent mutation of graph state — not traced in this pass either.** Carried over unresolved from G0 (§15 of the G0 doc). Whether two concurrent writers (e.g., overlapping Raft-committed events, or a standalone-mode race) could produce divergent adjacency-list construction was not investigated at the lock/concurrency-primitive level in either G0 or G0.1.
- **`GraphNode.namespace_id` being excluded from the hash also means a namespace-misrouting bug would not be caught by cross-replica hash comparison** — this is the practical consequence of the R2 gap and is the strongest argument for prioritizing the follow-up phase above, given namespace isolation is a stated security/tenancy boundary (G0 §9), not just a bookkeeping detail.
- **No new risks were introduced by this phase's changes.** The decode fix is byte-format-preserving and was verified against the full existing fixture corpus with zero regressions; the R1 deletion was verified to be reachable by nothing in the workspace before removal.

---

## 13. G0.1 Invariants

| # | Invariant | Status |
|---|---|---|
| 1 | The production graph replay path is unambiguous — exactly one `replay_events` implementation exists in the workspace, and it is namespace-aware. | **PROVEN** (R1 resolved by deletion of the orphaned alternative) |
| 2 | Graph replay produces field-identical canonical graph state, not just matching counts or hashes. | **PROVEN** for the tested topology (node/edge creation, multiple edges, self-loop, duplicate edge, cascade deletion, namespace placement); not proven under concurrent mutation |
| 3 | Snapshot restore produces field-identical canonical graph state, including for graphs with non-tail deletions. | **PROVEN** — and a real bug (R3) blocking this invariant was found and fixed during the proof, not merely documented |
| 4 | Namespace isolation for graph edges is enforced at the canonical mutation layer, and cannot be bypassed via the event log/replay path. | **PROVEN** — both live-apply and replay-of-adversarial-log cases tested |
| 5 | Duplicate-edge and self-loop semantics are explicit, tested, and unchanged from their pre-G0.1 behavior. | **PROVEN** (documented as "allowed, not deduplicated, not idempotent" for duplicates; "supported, appears in both adjacency directions, survives snapshot/replay" for self-loops) |
| 6 | Graph traversal (BFS) output is deterministic for a fixed canonical graph, including edge/node ordering. | **PROVEN** for a nontrivial topology (was previously proven only for trivial empty-input cases) |
| 7 | The BLAKE3 state-hash contract is explicitly documented: which canonical graph/namespace fields it commits, and which it does not. | **PROVEN as a documentation/test artifact** (§4) — the contract itself remains a **CONFIRMED GAP relative to full canonical-state coverage**, not corrected in this phase |
| 8 | Any field the hash contract DOES commit changes the hash when its canonical value changes. | **PROVEN** for edge `from`/`to` (direction and target) — the two properties explicitly required by Phase 10's "CRITICAL HASH PROPERTY" instruction |
| 9 | Derived graph/search structures (GraphRAG results, community detection, kernel-native and std-level vector indexes) remain rebuildable without changing canonical graph state. | **PROVEN** (unchanged from G0 — no derived structure was found or introduced with any canonical-state dependency in this phase) |
| 10 | Canonical graph state does not depend on randomized or hash-map iteration order. | **PROVEN** (re-verified in this phase; all graph pools remain `Vec<Option<T>>`, all hash maps in the graph path are lookup-only) |

---

## G0.1 STATUS

- **R1 replay ambiguity:** RESOLVED — orphaned, namespace-blind `crates/valori-kernel/src/replay_events.rs` deleted; exactly one `replay_events` implementation remains in the workspace, and it is namespace-aware.
- **R2 hash contract:** DOCUMENTED, NOT CORRECTED — classified as a genuine implementation gap (namespace_id, reverse-adjacency pointers, and the meta sidecar are canonical but unhashed); current behavior is now explicitly documented and locked by a test; widening the hash is recommended as a dedicated follow-up given its cluster-consensus-wide blast radius.
- **Replay equivalence:** PROVEN, field-by-field, for graph-inclusive event sequences (records, nodes, multiple edges, self-loop, duplicate edge, cascade deletion, namespace placement), both at the kernel layer and through the real disk-backed production recovery path.
- **Snapshot equivalence:** PROVEN, field-by-field — and a real, previously-undiscovered bug (R3: non-tail node/edge deletion broke snapshot decode) was found and fixed as part of proving this.
- **Namespace isolation:** PROVEN at the canonical mutation layer, and PROVEN to survive replay of an adversarial event log (cannot be bypassed via the log).
- **Duplicate-edge semantics:** DOCUMENTED and TESTED — duplicates are allowed, independently tracked, not idempotent, not deduplicated. Unchanged behavior.
- **Self-loop semantics:** TESTED — creation, both-direction adjacency, snapshot survival, and replay survival all proven; deletion coverage already existed and was not duplicated.
- **Deterministic canonical state:** PROVEN for everything re-audited (ids, ordering, adjacency, serialization); concurrent-mutation safety remains an open question carried over from G0.
- **Graph hash contract:** DOCUMENTED — proven for what IS committed (edge direction, edge target); proven (as locked-down current behavior, not as an endorsement) for what is NOT committed (node namespace_id).
- **Restart/recovery:** PROVEN for a small graph-and-namespace-inclusive scenario through the real event-log recovery path (`recover_from_event_log`), no new recovery mechanism introduced.
- **Tests:** 154 → 167 in `valori-kernel` (+13), 47 → 48 in `valori-storage` (+1), 13 → 14 in `valori-rag` (+1); `valori-node` 291/291 green (no new tests added there, full regression pass only). All previously-passing tests remain green — zero regressions. `cargo fmt --check`, `cargo clippy -- -D warnings` (on all touched crates), and `cargo build -p valori-kernel --target wasm32-unknown-unknown` (the mandatory `no_std` check) all pass.
- **Remaining risk:** R2 (hash coverage gap, documented but open) and concurrent-mutation determinism (open, carried over from G0) — both MEDIUM, neither CRITICAL. No new CRITICAL risks remain; R3 (the snapshot decode bug, which WAS critical-severity — silent data-loss-on-restore risk for any graph with a non-tail deletion) was found and closed within this phase.
- **Ready for G1: NO** — not because anything found in this phase is unresolved-and-blocking (R3 is fixed; R1 is fixed), but because R2 is explicitly deferred pending its own dedicated, wider-review phase, and G0.1's own success criteria list "R2 has a documented hash contract" (satisfied) rather than "R2 is corrected" as the bar — the team should explicitly decide whether to schedule the hash-widening follow-up before G1, or accept the documented gap and proceed. That decision, not further investigation, is the actual remaining gate.
