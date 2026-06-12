# Phase 2.3 — Raft State Machine

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 3 of 10

## Goal

Adapt `KernelState` to openraft's `RaftStateMachine` — the core correctness
piece of cluster mode. The kernel's determinism contract maps directly onto
Raft's SMR contract: identical committed entries produce identical state on
every node, hash for hash.

## Delivered

**`src/state_machine.rs` — `ValoriStateMachine`**, apply pipeline per
committed entry:

1. **Dedup** — if the entry's `request_id` is in the dedup table, skip the
   apply and answer `deduplicated: true`. The table is part of replicated
   state (travels in snapshots), so every node decides identically. FIFO
   capped at 65 536 entries (revisited in 2.10). Only *successful* applies
   enter the table — a rejected event's request_id stays retryable.
2. **Kernel apply** — `KernelState::apply_event`. Rejections (bad sequential
   id, dimension mismatch) are deterministic: every node rejects identically,
   state untouched, entry still consumed (`last_applied` advances).
3. **Audit record** — strictly after a successful apply, the event goes to
   the **`AuditSink`** trait. This is THE audit-log write point in cluster
   mode: once per event, at apply time, after quorum. valori-node plugs its
   BLAKE3-chained `EventLogWriter` in at Phase 2.5; `MemoryAuditSink`
   captures ordering for tests; `NullAuditSink` for the compliance suite.

**Snapshots:** payload = V5 kernel snapshot bytes + dedup table + **expected
BLAKE3 state hash**, bincode-framed. `install_snapshot` decodes, recomputes
the hash, and refuses a mismatch — keeping the node's old state. Foreign
arithmetic formats are already refused by V5 decode (Phase 1.3).

**Compliance:** `tests/openraft_compliance.rs` runs the **official
`openraft::testing::Suite`** over the `ValoriLogStore` + `ValoriStateMachine`
pair — the same suite openraft's reference stores pass. It passed first run.

## Findings

⚠️ **The V5 kernel snapshot has no internal checksum.** The corruption test
(flip one byte mid-payload) initially *passed install* — bincode and the V5
decoder both accepted the tampered bytes, silently producing corrupt vector
data. Fixed at the consensus layer by making the snapshot payload
self-verifying: the builder records the BLAKE3 state hash, install recomputes
it from the decoded state and refuses a mismatch. Any single-bit corruption
now fails one of three gates: bincode framing, V5 decode, or hash equality.
*Follow-up consideration: a checksum inside the V5 format itself (kernel-level)
would protect standalone-mode snapshots too — deferred, noted below.*

## Validation

- `tests/state_machine.rs` — **11 tests**: apply + hash response, blank
  entries, dedup across leader failover (same request_id at a later log
  index), distinct ids both apply, rejected-event-does-not-poison-dedup,
  deterministic rejection, audit ordering (successful applies only),
  snapshot round-trip restoring state hash *and* dedup table, corruption
  refusal, current-snapshot bookkeeping, two-node hash convergence.
- `tests/openraft_compliance.rs` — official suite, passing.
- Full workspace: **220 passing, 0 failures.**

## Follow-ups

- Phase 2.5: valori-node implements `AuditSink` over `EventLogWriter` —
  the audit chain then records exactly the quorum-committed event stream.
- Phase 2.10: dedup-table sizing policy; redb store re-runs the compliance
  suite.
- Kernel backlog: consider an internal checksum in snapshot format V6 so
  standalone-mode snapshots get the same protection (cluster mode is covered
  by the payload hash).
