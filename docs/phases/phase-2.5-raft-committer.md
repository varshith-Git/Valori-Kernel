# Phase 2.5 — RaftCommitter + Cluster Bootstrap

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 5 of 10

## Goal

Bring the consensus stack into valori-node: the Phase 1.9 `Committer` seam
gets its cluster implementation, the audit log gets its cluster write path,
and standalone-vs-cluster becomes a boot-time decision. The acceptance bar:
a cluster-written `events.log` must be replayable and chain-verifiable by
the *standalone* recovery path — an auditor cannot tell the difference.

## Delivered

**`commit/audit.rs` — `EventLogAuditSink`** implements valori-consensus's
`AuditSink` over the BLAKE3-chained `EventLogWriter`
(`append_with_request_id`, so the idempotency token lands in the v3
envelope). In cluster mode this is THE audit write point: once per event,
at apply, after quorum, after a successful kernel apply. Same v3 format,
same rotation splice, same `valori-verify` workflow as standalone.

**`commit/raft.rs` — `RaftCommitter`** implements `Committer` over the
openraft handle: `commit` = `client_write`, returning only after quorum
replication + local apply (+ audit). Sync-over-async via a captured runtime
handle (`block_in_place` on runtime workers). Error mapping:

| Raft outcome | CommitError |
|---|---|
| kernel rejected (deterministic, replicated) | `Rejected(reason)` — new variant |
| not the leader | `NotLeader { leader_api_addr }` — new variant; Phase 2.6 answers HTTP 307 with it |
| no quorum / fatal | `Io` |

**`ClientResponse.rejected`** (valori-consensus, append-only serde field):
the state machine now reports deterministic kernel rejections in the
response, so the committer can distinguish "applied" from "rejected" —
previously invisible to the caller.

**`cluster.rs`** — `ClusterConfig::parse` (pure, testable) +
`ClusterConfig::from_env`:

| Env | Meaning |
|---|---|
| `VALORI_CLUSTER_MEMBERS` | `id=raft_addr/api_addr,…` — presence switches cluster mode on |
| `VALORI_NODE_ID` | this node (must appear in members) |
| `VALORI_RAFT_BIND` | gRPC listener, default `0.0.0.0:3100` |
| `VALORI_CLUSTER_INIT` | `1` on exactly one node of a NEW cluster |

Config errors stop the process — a typo'd topology silently booting
standalone would be a split-brain factory. `bootstrap_cluster` assembles
log store + state machine (over the audit sink) + gRPC server + Raft and
returns a `ClusterHandle` whose `.committer()` plugs into the Engine seam.

## Findings

- The address parser initially accepted `1==/` (a "=" raft address) —
  caught by the malformed-entry test; addresses now require a colon and
  forbid stray `=`.
- The Phase 2.4 metrics-lag race reappeared in `log_height()` (it reads
  Raft metrics): tests must wait on `applied_index_at_least` before
  asserting on it. Same root cause, now documented twice — Phase 2.6's
  HTTP layer should treat `log_height` as advisory, never as a write
  barrier.

## Validation

`tests/cluster_boot.rs` — **5 tests**, 3× stable:

- Topology parsing: full form, optional api_addr, self-not-in-members,
  malformed entries.
- **`raft_committer_writes_a_verifiable_audit_log`** — boots a real
  single-node cluster (gRPC and all), commits 3 events through the
  `Committer` trait, then replays the on-disk `events.log` with the
  standalone `read_event_log` path: exactly the committed events, in
  commit order, chain intact.
- Deterministic kernel rejection surfaces as `Rejected`, not `Io`; state
  untouched.

Full workspace: **229 passing, 0 failures.**

## Follow-ups

- Phase 2.6: HTTP layer maps `NotLeader` → 307 + Location, `Rejected` →
  422; cluster management endpoints over `ClusterHandle.raft`.
- Engine's full `Box<dyn Committer>` swap (replacing the
  `Option<EventCommitter>`/`Option<WalWriter>` pair) lands with 2.6's
  server wiring — the committer and boot path are ready; the swap touches
  every Engine mutation method and belongs with the HTTP changes that
  consume it.
- `main.rs` boot dispatch (read `ClusterConfig::from_env`, choose
  standalone vs `bootstrap_cluster`) — with 2.6.
