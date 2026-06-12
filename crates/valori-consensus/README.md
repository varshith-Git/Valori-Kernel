# valori-consensus

Raft consensus layer for Valori cluster mode — **Phase 2 of the multi-node
roadmap** ([docs/phases/README.md](../../docs/phases/README.md)).

Built on [openraft 0.9](https://docs.rs/openraft/0.9) (the stable line;
Databend runs it in production) with tonic/gRPC transport.

## The one rule

**Raft commits, kernel applies, audit log records.**

The Raft log is internal plumbing — truncatable, purgeable, never shown to
auditors. The audit log (`events.log`, BLAKE3-chained) is written exactly
once per event, at APPLY time, strictly after quorum commit. The two must
never be conflated.

## Module map (per sub-phase)

| Module | Phase | Status | What it is |
|---|---|---|---|
| `types` | 2.1 | ✅ | openraft type config — every generic pinned once |
| `log_store` | 2.2 | ✅ | Raft log storage (internal, truncatable; in-memory until 2.10) |
| `state_machine` | 2.3 | ✅ | `KernelState` adapter + audit-sink write at apply |
| `network` | 2.4 | ✅ | tonic/gRPC transport between peers |

The `Committer` implementation over the Raft handle (`RaftCommitter`) lives
in **valori-node** (`commit::raft`, Phase 2.5) — the trait is node-side and
valori-node depends on this crate, not the other way around. The same phase
added `EventLogAuditSink` (node-side) plugging the chained `events.log`
into this crate's `AuditSink` seam, and `cluster::bootstrap_cluster`
assembling the whole stack.

## The types (Phase 2.1)

- **`NodeId = u64`** — from `VALORI_NODE_ID` (Phase 1.8 config knob).
- **`ValoriNode { api_addr, raft_addr }`** — both addresses travel in
  membership entries, so any node can tell a client where the leader's
  HTTP API lives.
- **`ClientRequest { event, request_id }`** — Raft replicates this, not a
  bare `KernelEvent`: the Phase 1.2 idempotency token rides along so every
  node makes the same dedup decision deterministically.
- **`ClientResponse { log_index, state_hash, deduplicated }`** — the state
  hash after apply lets a client verify it observed the same state the
  leader produced.
- **`SnapshotData = Cursor<Vec<u8>>`** — the V5 snapshot format from
  Phase 1.3, including its arithmetic-format byte, is the Raft snapshot
  payload verbatim.

Evolution policy: `ClientRequest`/`ClientResponse` cross the wire between
nodes — fields are append-only with `#[serde(default)]`, same as valori-wire.

## Design rules

- `valori-kernel` is never modified by this crate — it is consumed as a
  deterministic state machine, nothing more.
- Standalone mode never pays for consensus machinery: valori-node links this
  crate only behind cluster boot (Phase 2.5).

## The log store (Phase 2.2)

`ValoriLogStore` implements `RaftLogStorage` (the storage-v2 API). It holds
the *internal* Raft log: truncatable on leader-change conflicts, purgeable
on snapshot compaction — properties the append-only audit log must never
have, which is exactly why they are different logs. Nothing in the log store
touches `events.log`; the audit write happens in the state machine (2.3) at
APPLY time.

Backend: in-memory `BTreeMap` behind `Arc<Mutex<…>>` (vote and log writes
serialized, as openraft requires; reader clones share state with the store).
Phase 2.10 swaps in `redb` for durability behind the same interface.

## The state machine (Phase 2.3)

`ValoriStateMachine` adapts `KernelState` to `RaftStateMachine`. Per
committed entry: **dedup → kernel apply → audit record.**

- Dedup table is part of replicated state (travels in snapshots) — every
  node makes the same dedup decision. Only *successful* applies enter the
  table, so a rejected event's request_id can be retried.
- Kernel rejections are deterministic: every node rejects identically,
  state untouched, entry still consumed.
- The [`AuditSink`] trait is THE audit-log write point in cluster mode:
  once per event, at apply, after quorum. valori-node plugs its chained
  `EventLogWriter` in at Phase 2.5; tests use `MemoryAuditSink`.
- Snapshots are **self-verifying**: the payload carries the BLAKE3 state
  hash; install recomputes and refuses a mismatch (the V5 format alone has
  no internal checksum — found by the corruption test in this phase).

## The transport (Phase 2.4)

Three RPCs — `AppendEntries`, `Vote`, `InstallSnapshot` — each carrying the
bincode encoding of the corresponding openraft type. Protobuf is the
framing, not the schema: openraft's types stay the single source of truth.
Replies carry `Result<Resp, RaftError>` so Raft-level errors travel as
data; gRPC status codes mean real transport failures (and drop the channel,
so the next RPC reconnects).

- `ValoriNetworkFactory` reads each peer's `raft_addr` from the membership
  config itself — no separate address book to drift.
- `serve_raft(raft, addr)` binds the tonic server; `…:0` works for tests.
- protoc is vendored (`protoc-bin-vendored`) — no system install needed.

## Testing

- `tests/type_config.rs` — wire-type round-trips, serde evolution defaults,
  openraft entry/vote instantiation against the config.
- `tests/log_store.rs` — append/read round-trips, half-open ranges,
  truncate-then-rewrite under a new term, purge floor monotonicity,
  vote overwrite, long-lived reader clones observing later writes.
- `tests/state_machine.rs` — apply pipeline, dedup across leader failover,
  rejected-event semantics, audit ordering, snapshot round-trip with dedup
  transfer, corruption refusal, two-node hash convergence.
- `tests/openraft_compliance.rs` — the **official openraft storage
  compliance suite** over the log store + state machine pair. Phase 2.10's
  redb-backed store must pass this exact suite to land.
- `tests/grpc_cluster.rs` — a **real 3-node cluster over localhost gRPC**:
  leader election over the wire, 10 replicated writes converging to one
  BLAKE3 hash on all three kernels, cross-cluster request-id dedup, and
  follower writes answered with ForwardToLeader naming the leader's
  addresses.
- `tests/snapshot_transfer.rs` — late joiners after log compaction:
  snapshot-only catch-up (log purged), snapshot + live-tail catch-up,
  dedup table surviving the transfer, exact hash convergence.
- `tests/fault_tolerance.rs` — process-level fault injection: leader
  crash → re-election → writes continue; minority loss invisible to
  writers; **majority loss stalls writes instead of forking**. True
  partitions and crash-restart need the 2.10 harness (simulated
  transport + persistent redb log).

Run: `cargo test -p valori-consensus`
