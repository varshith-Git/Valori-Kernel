# Phase 2.8 — Fault-Tolerance Tests

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 8 of 10

## Goal

Prove the three behaviours that make a consensus cluster worth running:
a leader crash heals itself, a minority failure is invisible to writers,
and a majority failure **stalls writes rather than forking** — the safety
property everything else rests on.

## Delivered

**`tests/fault_tolerance.rs`** — process-level fault injection on real
sockets: a "kill" shuts down the node's Raft core (`Raft::shutdown`) *and*
aborts its tonic server, so peers experience genuine connection failures.

1. **`leader_crash_triggers_reelection_and_writes_continue`** — 3 nodes,
   3 writes, kill the leader. A survivor wins an election (asserted: new
   leader ≠ crashed leader), 3 more writes commit through it, both
   survivors converge to 6 records and one BLAKE3 hash.
2. **`minority_follower_loss_does_not_stop_writes`** — kill one follower;
   5 writes commit on the 2/3 quorum; survivors hash-equal.
3. **`majority_loss_stalls_writes_instead_of_forking`** — kill both
   followers; a write through the lone leader must NOT succeed. The test
   accepts either observable correct behaviour (an error, or hanging past
   a 3 s deadline while openraft retries replication) and forbids the one
   incorrect one: returning success. Also asserts the leader's state
   machine applied nothing.

## Scope honesty

Two fault classes are *not* coverable with the current build, and the test
file header says so:

- **True partitions** (both sides alive but separated) need a simulated
  transport — the tonic client can't be selectively severed per-link.
- **Crash-restart** (node comes back with its log) needs a persistent
  Raft log; ours is in-memory until the redb store lands.

Both are Phase 2.10 work (turmoil-style harness + redb), which is exactly
where the roadmap already places them.

## Findings

- None — 5× consecutive stable on the first design. The Phase 2.4 lessons
  (wait on the *observer's* metrics view; never use metrics as a write
  barrier) were applied from the start, which is plausibly why.

## Validation

- 3 tests, 5× consecutive runs stable (~3 s per run; the majority-loss
  test deliberately burns its 3 s deadline).
- Full workspace: **240 passing, 0 failures.**

## Follow-ups

- Phase 2.10: turmoil-style partition simulation (heal-and-catch-up,
  symmetric/asymmetric partitions, mid-snapshot-transfer leader change)
  and crash-restart tests over the redb log store.
