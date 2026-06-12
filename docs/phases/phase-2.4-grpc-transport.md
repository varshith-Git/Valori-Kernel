# Phase 2.4 — gRPC Transport (tonic)

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 4 of 10

## Goal

The wire between Raft peers: tonic/gRPC carrying openraft's three RPCs,
proven by a real multi-node cluster talking over actual sockets — not mocks.

## Delivered

**`proto/raft.proto`** — `RaftService` with `AppendEntries`, `Vote`,
`InstallSnapshot`. Every message is one `bytes payload` field holding the
bincode encoding of the corresponding openraft type. Protobuf is the
framing, not the schema — re-modelling openraft's types field-by-field in
proto would create a second source of truth that drifts with every openraft
upgrade. Replies carry `Result<Resp, RaftError>`: Raft-level errors
(term conflicts, vote rejections) are *data*; gRPC status codes are
reserved for genuine transport failures.

**`build.rs`** — tonic codegen with **vendored protoc**
(`protoc-bin-vendored`): no system protoc needed on dev machines or CI
(the build machine indeed had none — that's why).

**`src/network.rs`**:

- `ValoriNetworkFactory` / `ValoriNetwork` — the client side. One channel
  per peer, connected lazily, dropped on any transport error so the next
  RPC reconnects; openraft's replication loop supplies the retry cadence.
  Peer addresses come from the `ValoriNode` in the membership config
  itself — no separate address book to drift.
- `RaftRpcService` — the server side: decode, hand to the local
  `Raft` handle, encode the answer.
- `serve_raft(raft, addr)` — binds tonic on `addr`, returns the bound
  address (`…:0` supported for tests) and the server task handle.

## Findings

- A generic "call" helper taking a closure over the tonic client hits a
  borrow-checker wall (boxed futures borrowing `&mut client` need HRTB
  closures that inference won't unify). Three explicit RPC bodies are
  shorter than the type gymnastics — duplication was the right call.
- Two test races worth recording:
  1. A follower answers `ForwardToLeader { leader_id: None }` until the
     first heartbeat arrives — tests must wait for the *follower's* view of
     the leader, not just the leader's own.
  2. The leader's `metrics().last_applied` lags a hair behind
     `client_write` returning; the authoritative index is the write
     response's own `log_index`. Using metrics made hash-convergence
     flaky (~1 in 4); using the response: 5/5 stable.

## Validation

`tests/grpc_cluster.rs` — a real 3-node cluster on localhost (OS-assigned
ports), **4 tests**:

1. Leader election over the wire.
2. 10 writes through the leader; all three kernels converge to one BLAKE3
   state hash; every kernel holds all 10 records.
3. Same `request_id` retried → `deduplicated: true`, record count stays 1.
4. Write to a follower → `ForwardToLeader` naming the leader's id *and*
   addresses (api + raft) for the client to retry against.

Run 5× consecutively: stable. Full workspace: **224 passing, 0 failures.**

## Follow-ups

- Phase 2.5: `RaftCommitter` drives `client_write` from valori-node's
  `Committer` seam; boot logic picks standalone vs cluster.
- Phase 2.10: mTLS on this exact channel (rustls via
  `tonic::transport::ServerTlsConfig`), plus a version/format handshake
  for mixed-version clusters.
- The bincode payloads inherit no decode limit here (unlike valori-wire's
  1 MiB cap) — peers are mTLS-authenticated in production, but a cap is
  cheap defence; fold into 2.10 hardening.
