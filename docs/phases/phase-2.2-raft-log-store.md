# Phase 2.2 — Raft Log Store

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 2 of 10

## Goal

Implement openraft's `RaftLogStorage` for Valori: the internal,
truncatable, purgeable Raft log plus persisted vote — while proving by
construction that it never touches the append-only audit log.

## Delivered

**`src/log_store.rs` — `ValoriLogStore`** implementing
`RaftLogStorage<TypeConfig>` + `RaftLogReader<TypeConfig>`:

| Operation | Semantics |
|---|---|
| `append` | insert entries, fire the `LogFlushed` callback (in-memory: immediately; redb in 2.10: after fsync) |
| `truncate(log_id)` | conflict resolution — delete **at and after**, rewritable by the new leader |
| `purge(log_id)` | compaction — delete **up to and including**, floor recorded monotonically |
| `get_log_state` | last log id falls back to the purge floor when the log is empty (openraft requirement) |
| `save_vote` / `read_vote` | persisted vote; a lost vote can elect two leaders in one term |
| `save_committed` / `read_committed` | optional committed-pointer persistence — implemented rather than defaulted |
| `get_log_reader` | clones share the `Arc<Mutex<…>>` — replication tasks hold readers long-term and must observe later writes |

**Storage backend:** in-memory `BTreeMap` behind one `Arc<Mutex<…>>` so vote
and log writes are serialized (an explicit openraft correctness rule).
Durability lands in Phase 2.10 (redb) behind the same interface.

**Cargo feature:** `storage-v2` added to the openraft dependency — without
it the modern `RaftLogStorage`/`RaftStateMachine` split is sealed and only
the legacy `RaftStorage` trait is implementable.

**The one rule, enforced by structure:** the module has no import of
valori-wire, no file I/O, no path config. The audit log is written by the
state machine (2.3) at APPLY time, after quorum — the Raft log is plumbing.

## Findings

- openraft 0.9 seals the v2 storage traits behind the `storage-v2` feature
  flag; the unconditional re-export of the trait names makes this a
  confusing compile error ("Sealed is not satisfied") rather than a missing
  type. Documented in Cargo.toml for the next reader.
- `purge` may be replayed with an older log id; the floor must be monotonic
  or `get_log_state` regresses after restart. Covered by a dedicated test.

## Validation

- `tests/log_store.rs` — **11 tests**: empty state, append/read round-trip,
  half-open range semantics, missing-range-is-empty-not-error,
  truncate-then-rewrite under a new term, purge floor + survivors,
  purge-everything fallback, floor monotonicity, vote overwrite,
  committed pointer, reader-clone visibility.
- Full workspace: **208 passing, 0 failures.**

## Follow-ups

- Phase 2.3: state machine; then run `openraft::testing::Suite` over the
  pair — the official compliance suite needs both halves.
- Phase 2.10: redb-backed persistence behind the same interface; the
  compliance suite re-validates the swap.
