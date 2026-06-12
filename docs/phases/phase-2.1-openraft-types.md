# Phase 2.1 — openraft Type Config

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 1 of 10

## Goal

Pin every type parameter openraft is generic over, in one place, before any
async Raft code exists. Everything in 2.2–2.10 (log store, state machine,
network, committer) is written against `TypeConfig`; getting these types
right first means the rest of Phase 2 is implementation, not design.

## Delivered

**Dependencies** (`valori-consensus/Cargo.toml`): `openraft 0.9` (stable
line — Databend runs it in production; 0.10 is still alpha) with the `serde`
feature; `tonic 0.12` + `prost 0.13` declared now so cargo-deny vets their
license trees before Phase 2.4 needs them; `tokio` with `sync/rt/macros/time`.

**`src/types.rs`** — the config:

| Parameter | Choice | Why |
|---|---|---|
| `NodeId` | `u64` | matches `VALORI_NODE_ID` (Phase 1.8) |
| `Node` | `ValoriNode { api_addr, raft_addr }` | membership entries carry both the HTTP data-plane and gRPC consensus addresses, so any node can point a client at the leader's API |
| `D` (app data) | `ClientRequest { event, request_id }` | Raft replicates the envelope, not bare `KernelEvent` — the Phase 1.2 idempotency token is itself replicated, so every node makes the same dedup decision deterministically |
| `R` (response) | `ClientResponse { log_index, state_hash, deduplicated }` | returning the post-apply BLAKE3 hash lets clients verify they observed the leader's state |
| `SnapshotData` | `Cursor<Vec<u8>>` | the Phase 1.3 V5 snapshot (with its format byte) is the Raft snapshot payload verbatim |
| `AsyncRuntime` | `TokioRuntime` | the node is already tokio/axum |

Shorthand aliases exported for downstream phases: `LogId`, `Vote`, `Entry`,
`StoredMembership`, `SnapshotMeta`, `Raft`.

**Evolution policy:** `ClientRequest`/`ClientResponse` cross the wire between
nodes. Fields are append-only with `#[serde(default)]` — the same policy
valori-wire enforces with fixtures. Both new optional fields
(`request_id`, `deduplicated`) have tests proving old payloads without the
field still decode.

**`lib.rs`** rewritten from "intentionally empty" placeholder to the Phase 2
module map, documenting the one rule: *Raft commits, kernel applies, audit
log records.*

**Crate README** rewritten to match.

## Findings

- openraft 0.9's `declare_raft_types!` accepted `KernelEvent` (with its
  custom Serialize/Deserialize for the V2 metadata format) without friction —
  the custom serde impls from the wire-format work compose cleanly.
- `Vote` serialization is covered by a dedicated test because Phase 2.2
  persists it: a corrupted vote can elect two leaders in one term. The test
  documents that stake before the storage code exists.

## Validation

- `cargo build -p valori-consensus` — clean
- `tests/type_config.rs` — **7 tests**: ClientRequest round-trip,
  missing-field decode (evolution), ClientResponse dedup-flag default,
  ValoriNode round-trip + Display, openraft `Entry` round-trip with a real
  kernel event, `Vote` persistence round-trip, `Raft<TypeConfig>`
  instantiability (compile-time proof)
- Full workspace suite: no regressions (197 passing, 0 failures)

## Follow-ups

- Phase 2.2: `ValoriLogStore` (in-memory `RaftLogStorage`), persisting
  exactly the `Vote` and `Entry` types pinned here
- Phase 2.4: tonic codegen for these types (`raft.proto` + build.rs)
- The bincode-vs-JSON wire question for gRPC payloads is decided in 2.4;
  the types are serde-agnostic so both work
