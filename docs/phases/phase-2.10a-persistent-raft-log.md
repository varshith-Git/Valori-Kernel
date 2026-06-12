# Phase 2.10a — Persistent Raft Log (redb)

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 10 of 10, part a of d
(2.10 splits into: **a** persistent log · b mTLS · c metrics · d partition harness)

## Goal

Make the Raft log and vote survive process crashes — the prerequisite for
crash-*restart* fault tolerance (the gap 2.8 documented). A node must come
back from a dead process with its log, vote, committed pointer, and purge
floor intact, and rebuild its kernel state by replaying its own log.

## Delivered

**`src/log_store_redb.rs` — `RedbLogStore`**: same `RaftLogStorage`
contract as the in-memory store, on a single-file embedded redb database.
Two tables: `logs` (`u64 index → bincode(Entry)`) and `meta`
(vote / committed / last_purged).

**Durability discipline:** every write method commits its redb transaction
(fsync-backed) before returning; `append` fires the `LogFlushed` callback
only *after* the commit — the exact guarantee openraft's correctness
argument needs. The in-memory store fires it immediately; this is the one
behavioural difference between them.

**Bootstrap wiring:** `ClusterConfig.raft_log_path` /
`VALORI_RAFT_LOG_PATH`. Set → redb at that path; unset → in-memory
(tests, ephemeral deployments). Both stores pass the identical compliance
suite, so the choice is purely a durability knob.

## Validation

- `tests/log_store_redb.rs` — behaviour parity (ranges, truncate-rewrite,
  monotonic purge floor), **`everything_survives_reopen`** (log, vote,
  committed pointer, purge floor, and full payload bytes across a
  drop-and-reopen), fresh-database semantics, and the **official openraft
  compliance suite over redb** (~20 s of real fsyncs — the price of the
  guarantee being real).
- `cluster_boot.rs::node_restart_recovers_state_from_the_persistent_raft_log`
  — the flagship: boot a cluster node with a redb path, commit 5 events,
  record the BLAKE3 hash, **crash it** (Raft shutdown + server abort),
  bootstrap a second life on the same file. The node re-elects itself from
  the persisted vote+membership, replays its own log into a fresh kernel,
  and **reproduces the exact pre-crash state hash** — then keeps accepting
  writes. 3× stable.
- Full workspace: **248 passing, 0 failures.**

## Findings

- redb 2.x's `len()` lives on `ReadableTableMetadata`, not
  `ReadableTable` — a one-line import, noted for the next reader.
- `init: true` on restart is safe by construction: openraft refuses a
  second initialize and `bootstrap_cluster` already treated that as
  "fine" (Phase 2.5) — membership authority is the log itself. The
  restart test exercises this deliberately.

## Follow-ups

- 2.10b: mTLS on the tonic channel (rustls), cluster CA tooling,
  `CertRotated` admin event.
- 2.10c: Prometheus metrics (term, commit index, apply lag, snapshot
  installs).
- 2.10d: partition harness (simulated transport) — heal-and-catch-up,
  split-brain prevention under symmetric/asymmetric partitions; plus the
  gRPC decode cap noted in 2.4.
- Compose (Phase 1.11) should mount a volume for `VALORI_RAFT_LOG_PATH`
  once nodes run cluster mode by default.
