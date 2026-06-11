# valori-consensus

Raft consensus layer for Valori cluster mode — **Phase 2 of the multi-node
roadmap** ([docs/MULTINODE_ROADMAP.md](../../docs/MULTINODE_ROADMAP.md)).

This crate is intentionally empty in Phase 1. It exists so the workspace
layout, CI wiring, and feature flags are settled before consensus code lands.

## What lands here in Phase 2

- `openraft` integration: log storage over the chained event log, vote and
  membership storage, state-machine adapter over `KernelState::apply_event`,
  snapshot adapter over the V4 snapshot format
- The raft/audit storage split: a truncatable Raft log (`raft/log/`) plus the
  append-only hash-chained audit log written at apply time
- tonic/gRPC inter-node transport with mTLS
- The version + wire-format + arithmetic-format connection handshake

## Design rules

- `valori-kernel` is never modified by this crate — it is consumed as a
  deterministic state machine, nothing more.
- Everything here is feature-flagged (`--features cluster` on valori-node);
  standalone mode must never pay for consensus machinery.

## Testing

`tests/placeholder.rs` keeps the crate in the test lifecycle. Phase 2 brings
turmoil network-partition simulations, real-process cluster kill-tests, and
cross-node hash-equality invariants.
