# Phase 2.7 — Snapshot Transfer (New-Node Catch-Up)

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 7 of 10

## Goal

Prove that a node joining a cluster whose Raft log has been compacted —
the normal state of any long-running cluster — receives the leader's
snapshot over gRPC, passes the Phase 2.3 verification gates on install,
and converges to the leader's exact state. Without this, "add a node"
only works on young clusters.

## Delivered

No new production code was required — the machinery has been in place
since 2.3 (`build_snapshot`/`install_snapshot` with V5 + BLAKE3
self-verification) and 2.4 (the chunked `InstallSnapshot` RPC). openraft
routes a too-far-behind learner to the snapshot path automatically. This
phase *proves* the path end-to-end over real sockets and pins it in CI.

**`tests/snapshot_transfer.rs`** — both tests use
`max_in_snapshot_log_to_keep: 0` plus the `Trigger` API
(`trigger().snapshot()`, `trigger().purge_log()`) for deterministic
compaction instead of waiting on background policy:

1. **`late_joiner_catches_up_via_snapshot_after_log_purge`** — 20 writes,
   snapshot, purge everything; a brand-new node joins. The log it needs is
   gone, so catch-up *must* be the snapshot. Asserts: record count, exact
   state-hash equality with the leader, `last_applied` covering the purged
   history — and then promotes the joiner to voter and retries a
   **pre-snapshot `request_id`** through the cluster: deduplicated on every
   node, proving the dedup table travels inside the snapshot in production
   transfer, not just in the 2.3 unit test.
2. **`snapshot_then_more_writes_joiner_gets_snapshot_plus_tail`** —
   history is snapshot (10 writes) + live log tail (5 more). The joiner
   needs both transfer mechanisms in sequence; asserts count 15 and hash
   equality.

## Findings

- None on the happy path — the 2.3/2.4 layering paid off: snapshot
  transfer worked first run, 5× stable. The deliberate design decision
  that made this a test-only phase: snapshot *content* (V5 + hash
  self-verification) was settled independently of snapshot *transport*
  (chunked RPC), so their composition had no seams to debug.

## Validation

- `tests/snapshot_transfer.rs` — 2 tests, 5× consecutive runs stable.
- Full workspace: **237 passing, 0 failures.**

## Follow-ups

- Phase 2.10: snapshot-chunk size and a transfer cap
  (`MAX_SEGMENT_DECOMPRESSED_BYTES`-style guard) on `install_snapshot`'s
  receiving side; snapshot policy knobs (`VALORI_SNAPSHOT_EVERY_EVENTS`
  from Phase 1.8) wired into openraft's `snapshot_policy`.
- Phase 2.8 (turmoil): snapshot transfer under partitions and mid-transfer
  leader changes.
