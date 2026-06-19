# valori-consensus

Raft consensus layer for Valori cluster mode.

Built on [openraft 0.9](https://docs.rs/openraft/0.9) with tonic/gRPC transport.

---

## The one rule

**Raft commits → kernel applies → audit log records. In that order. Always.**

The Raft log is internal plumbing — truncatable, purgeable, never shown to
auditors. The audit log (`events.log`, BLAKE3-chained) is written exactly once
per event, at APPLY time, strictly after quorum commit. The two must never be
conflated.

### Apply-vs-audit ordering invariant

The state machine processes each committed entry as an atomic three-step pipeline:

```
1. DEDUP CHECK   — if request_id already seen: skip steps 2 and 3, return cached response.
2. KERNEL APPLY  — call kernel.apply_event(event). This mutates KernelState.
                   On rejection: state is unchanged, entry is consumed, request_id is NOT recorded.
3. AUDIT WRITE   — append to events.log only if step 2 succeeded and was not a duplicate.
```

**The window between steps 2 and 3 is the only place where a process crash can
cause divergence between kernel state and the audit log.** If the process crashes
after APPLY but before AUDIT WRITE:

- The kernel state on this node reflects the event.
- The audit log does not.
- On restart, openraft replays the log entry through the state machine again.
- Step 2 re-applies the event (kernel state is idempotent for the same input at the same slot).
- Step 3 writes the audit record — exactly once.

This is safe because `replay_until` suppresses duplicate audit writes for entries
that were already written before the crash. The invariant that makes this work:
**a log entry's audit record is written at most once, at the moment its apply
succeeds for the first time on this node.**

Violations that would break this invariant (and are explicitly prevented):

| Action | Why it breaks the invariant |
|---|---|
| Writing the audit record before kernel apply | A kernel rejection would produce an audit record for an event that had no effect. |
| Writing the audit record from the Raft commit hook | Commit precedes apply; the entry may still be rejected by the kernel. |
| Writing the audit record outside the state machine | Ordering with apply is no longer guaranteed under concurrent applies. |
| Skipping `replay_until` suppression on restart | Entries applied before the crash would produce duplicate audit lines. |

---

## Module map

| Module | Status | Description |
|---|---|---|
| `types` | ✅ | openraft type config — every generic pinned once. |
| `log_store` | ✅ | In-memory Raft log (ephemeral; for tests). |
| `log_store_redb` | ✅ | Persistent Raft log backed by `redb`. Survives restarts. |
| `state_machine` | ✅ | `KernelState` adapter with audit-sink writes at apply time. |
| `network` | ✅ | tonic/gRPC peer transport (mTLS-capable). |

`RaftCommitter` (the `Committer` trait impl over the Raft handle) lives in
**valori-node** — the dependency direction is node → consensus, not the reverse.

---

## Types (Phase 2.1)

- **`NodeId = u64`** — from `VALORI_NODE_ID`.
- **`ValoriNode { api_addr, raft_addr }`** — both addresses ride in membership
  entries so any node can redirect a client to the leader's HTTP API.
- **`ClientRequest { event, request_id, schema_version }`** — the Phase 1.2
  idempotency token travels with the event so every replica makes the same dedup
  decision. `schema_version` (Phase 3.2) is stamped by the leader at proposal
  time; followers reject entries they don't understand (see below).
- **`CURRENT_SCHEMA_VERSION: u8`** — the wire version this binary speaks. Bump
  when a new `KernelEvent` variant would be uninterpretable to an older node.
  See [`docs/COMPATIBILITY.md`](../../docs/COMPATIBILITY.md).
- **`ClientResponse { log_index, state_hash, deduplicated }`** — the BLAKE3
  hash after apply lets a client verify it observed the leader's exact state.
- **`SnapshotData = Cursor<Vec<u8>>`** — the V6 binary snapshot (VAL1 frame)
  is the Raft snapshot payload verbatim.

`ClientRequest` / `ClientResponse` cross the wire — fields are append-only
with `#[serde(default)]`.

### Schema version gate (Phase 3.2)

`ValoriStateMachine::apply()` checks `req.schema_version > CURRENT_SCHEMA_VERSION`
**before** the dedup check and before any kernel mutation. If the entry is from a
future version:

- `StorageError` is returned — replication halts on this follower.
- The kernel state and audit log are untouched.
- The cluster continues writing through the remaining quorum.
- The follower self-heals after the operator replaces its binary and restarts.

Old nodes built before Phase 3.2 decode `schema_version` as `0` via
`#[serde(default)]` and are safe to include in a rolling upgrade window.
See [`docs/COMPATIBILITY.md`](../../docs/COMPATIBILITY.md) for the full
coexistence matrix.

---

## Persistent log store (`log_store_redb`)

`RedbLogStore` implements `RaftLogStorage` backed by a `redb` database. The
same database file is shared with the state machine via `RedbLogStore::db()`,
so both the Raft log and the state-machine metadata live in one file.

```rust
let store = RedbLogStore::open("/var/lib/valori/raft.redb")?;

// Share the database handle with the state machine.
let db = store.db();
let sm = ValoriStateMachine::with_db(audit_sink, db)?;

let raft = Raft::new(node_id, config, network, store, sm).await?;
```

redb table layout:

| Table | Keys | Contents |
|---|---|---|
| `logs` | `u64` log index | bincode-encoded `Entry<TypeConfig>` |
| `meta` | `&str` (`"vote"`, `"last_purged"`) | bincode-encoded metadata |
| `sm_meta` | `&str` (prefixed `"sm_"`) | State-machine persistence (see below). |

---

## Persistent state machine (`state_machine`)

`ValoriStateMachine` adapts `KernelState` to `RaftStateMachine`. Per committed
entry the pipeline is: **dedup → kernel apply → audit write → persist**.

### Shared-database persistence

When constructed via `ValoriStateMachine::with_db(audit, db)`, the state
machine persists `last_applied`, `membership`, and the current snapshot into
the `sm_meta` redb table after every apply. On restart openraft reads these
back and resumes from the correct position — preventing already-applied entries
from being replayed through the `AuditSink` a second time and producing
duplicate `events.log` lines.

```rust
// sm_meta table keys:
// "sm_last_applied"  → bincode-encoded Option<LogId>
// "sm_membership"    → bincode-encoded StoredMembership
// "sm_snapshot_meta" → bincode-encoded SnapshotMeta
// "sm_snapshot_data" → raw snapshot bytes
```

### Replay suppression

On restart, log entries up to and including the persisted `last_applied` index
are replayed by openraft to rebuild in-memory state. The state machine sets
`replay_until = last_applied` before processing begins and suppresses
`AuditSink::write()` calls for those entries. Only events committed *after*
restart produce new `events.log` lines.

### Dedup

The dedup table is part of replicated state (it travels in snapshots). Every
node makes the same dedup decision. Only successful applies enter the table, so
a rejected event's `request_id` can be retried.

### Snapshots

Snapshots are self-verifying: the payload carries the BLAKE3 state hash;
`install_snapshot` recomputes it and refuses a mismatch.

```rust
// AuditSink trait — plug in a real writer or the test double.
impl AuditSink for EventLogAuditSink {
    fn write(&mut self, entry: AuditEntry) -> Result<(), AuditError> {
        self.writer.append(entry)
    }
}

impl AuditSink for MemoryAuditSink {
    fn write(&mut self, entry: AuditEntry) -> Result<(), AuditError> {
        self.entries.push(entry);
        Ok(())
    }
}
```

---

## Transport (Phase 2.4)

Three RPCs — `AppendEntries`, `Vote`, `InstallSnapshot` — carry the bincode
encoding of the corresponding openraft types. Protobuf is the framing, not the
schema. Replies carry `Result<Resp, RaftError>` so Raft-level errors travel as
data; gRPC status codes mean real transport failures.

`ValoriNetworkFactory` reads each peer's `raft_addr` from the membership config
itself — no separate address book to drift. `protoc` is vendored
(`protoc-bin-vendored`) — no system install needed.

---

## Testing

```bash
cargo test -p valori-consensus
```

| Suite | What it covers |
|---|---|
| `tests/type_config.rs` | Wire-type round-trips, serde evolution defaults. |
| `tests/log_store.rs` | Append/read, truncate-then-rewrite, purge floor monotonicity. |
| `tests/state_machine.rs` | Apply pipeline, dedup, audit ordering, snapshot round-trip, corruption refusal. |
| `tests/openraft_compliance.rs` | Official openraft storage compliance suite. |
| `tests/grpc_cluster.rs` | Real 3-node cluster: election, 10 replicated writes, cross-cluster dedup, ForwardToLeader. |
| `tests/snapshot_transfer.rs` | Late-joiner catch-up: snapshot-only, snapshot + live-tail, hash convergence. |
| `tests/fault_tolerance.rs` | Leader crash → re-election, minority loss, majority loss stalls writes. |
