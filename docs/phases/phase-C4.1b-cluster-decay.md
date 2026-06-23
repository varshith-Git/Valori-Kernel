# Phase C4.1b — Cluster decay + state-machine creation timestamps

## Goal

Make recency-aware search (Phase C4.1) actually work in cluster mode. In C4.1
the cluster `/search` endpoint *accepted* `decay_half_life_secs` but ignored it,
because per-record creation time was only tracked in the standalone `Engine`,
not in the consensus state machine. C4.1b tracks creation timestamps inside the
Raft state machine and wires the decay re-rank into the cluster search path so
one SDK call behaves identically against standalone and cluster nodes.

## Delivered

### Creation-time tracking in the state machine — `crates/valori-consensus/src/state_machine.rs`

| Change | Detail |
|---|---|
| `StateMachineInner.created_at: HashMap<u32, u64>` | New field — unix-second creation timestamps keyed by record id. |
| Stamp on apply | After a successful `AutoInsertRecord` apply, the allocated record id is stamped with `now_unix()`. All replicas stamp at apply time, so the map is consistent across the cluster (not byte-identical clocks, but per-node monotonic and good enough for ranking — see Findings). |
| `record_created_at(id) -> Option<u64>` | Accessor for a single record's timestamp. |
| `with_state_and_timestamps(f)` | Read closure exposing both `&KernelState` and `&created_at` so the search path can re-rank under one lock. |

Both constructors (`new`, `with_db`) initialise `created_at` empty.

### Cluster search re-rank — `crates/valori-node/src/cluster_server.rs`

`search()` now mirrors the standalone `memory_search_vector` path:

- `decay_half_life_secs == 0` (or absent) → unchanged behaviour: brute-force
  scan, top-k by true L2 distance.
- `decay_half_life_secs > 0` → over-fetch a pool (`k*4`, clamped 50–1000), build
  `DecayHit`s using `created_at` from the state machine, call the shared
  `crate::decay::rerank`, return top-k. Identical re-rank math to standalone.

The `decay_half_life_secs` field on the cluster `SearchRequest` lost its
`#[allow(dead_code)]` — it is now read.

## Findings

- **`created_at` is not hashed, snapshotted, or replicated as data.** It is a
  derived, per-node side map — the same design decision as the standalone
  `Engine.created_at`. This keeps the BLAKE3 state hash independent of wall-clock
  time (determinism invariant intact). The cost: a node that **restarts** or
  **installs a snapshot** starts with an empty `created_at`, so records inserted
  before that event rank with factor 1.0 (neutral, no decay) until re-stamped.
  Deferred to **C4.1c** — durable creation timestamps via WAL/event timestamps.
- Stamping uses each node's local clock at apply time, so timestamps are *not*
  byte-identical across replicas. This does not affect the state hash (the map
  is non-hashed) and only perturbs ranking by clock skew, which is bounded and
  irrelevant at half-life granularity (seconds–days).

## Validation

- `cargo test -p valori-kernel -p valori-consensus` — 112 passed, 0 failed.
- `cargo test -p valori-node` — 193 passed, 0 failed.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` — clean
  (no_std invariant intact; this phase touched only std crates plus a non-hashed
  map).
- Manual smoke test: boot a 3-node cluster, insert N vectors, wait, insert M
  more, `POST /search` with `decay_half_life_secs` set → recent inserts rank
  above older near-matches; with the field absent the ranking is byte-identical
  to pre-C4.1b.

## Follow-ups

- **C4.1c** — persist creation timestamps durably (WAL/event timestamps) so
  decay survives restart and snapshot install. Owner: future C4.1c.
- Cluster `/v1/memory/search_vector` (the agent-memory wrapper, as opposed to
  `/search`) is standalone-only today; exposing it on the cluster data plane is
  a separate small follow-up.
