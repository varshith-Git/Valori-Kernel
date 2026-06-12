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
| `log_store` | 2.2 | ⬜ | Raft log storage (internal, truncatable) |
| `state_machine` | 2.3 | ⬜ | `KernelState` adapter + audit-log write at apply |
| `network` | 2.4 | ⬜ | tonic/gRPC transport between peers |
| `raft_committer` | 2.5 | stub | `Committer` impl backed by the Raft handle |

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

## Testing

- `tests/type_config.rs` — wire-type round-trips, serde evolution defaults,
  openraft entry/vote instantiation against the config.
- Phase 2.8 brings turmoil network-partition simulations and cross-node
  hash-equality invariants.

Run: `cargo test -p valori-consensus`
