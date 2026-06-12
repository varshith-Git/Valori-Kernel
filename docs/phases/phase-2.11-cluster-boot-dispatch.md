# Phase 2.11 — Boot Dispatch + Cluster Data Plane v1

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode) — the main.rs integration deferred
since 2.5/2.6, plus a first usable HTTP data plane over Raft.

## Goal

Make the stock binary actually do what the docs promise: one binary, one
env-var decision. `VALORI_CLUSTER_MEMBERS` absent → the standalone path,
byte-for-byte unchanged. Present → boot the full Phase 2 stack and serve a
usable HTTP data plane over Raft.

## Delivered

**`main.rs` dispatch** — first thing after telemetry: `ClusterConfig::
from_env()`. Malformed topology → hard exit (a typo silently booting
standalone is how you get two databases that each think they're the real
one). Cluster mode boots: audit sink over the chained `events.log` (loud
warning + NullAuditSink if no path is configured), `bootstrap_cluster`
(redb log, mTLS, metrics watcher — all the 2.10 wiring), and the new
cluster router on `VALORI_BIND`.

**`cluster_server.rs` — the data plane over Raft, v1:**

| Route | Behaviour |
|---|---|
| `POST /records` | insert → `Raft::client_write`; follower answers **307 + `Location: http://<leader-api>`** |
| `POST /search` | brute-force k-NN served **locally on any node** — the replicas' RAM paying for itself |
| `GET /health` / `GET /metrics` | cluster health / Prometheus |
| `/v1/cluster/*` | the 2.6 management plane, merged |

Insert details: f32 → Q16.16 conversion with the same range guard as the
Engine; optional `request_id` (16 bytes) flows into the Raft envelope so
HTTP retries deduplicate cluster-wide; id allocation + commit are
serialized per node behind an async mutex (see Findings), with a bounded
retry loop as belt-and-braces.

**`docker-compose.yml`** updated to the Raft topology: shared
`VALORI_CLUSTER_MEMBERS` over service names, per-node `VALORI_NODE_ID`,
`VALORI_CLUSTER_INIT=1` on node-1 only, `VALORI_RAFT_LOG_PATH` on the data
volume. The legacy `VALORI_FOLLOWER_OF` replication vars are gone —
superseded by Raft membership.

## Findings

- **Sequential-id contention is real:** 16 concurrent inserts through one
  node exceeded an 8-attempt retry budget (each loser must re-read the id
  and re-commit). v1 fix: serialize allocate+commit per node — each insert
  is a quorum round-trip anyway, so the cost is pipelining, not latency.
  The *right* fix is allocating the record id inside the state machine
  (event without id; SM assigns deterministically), which is a kernel
  event-schema change — recorded as the follow-up it is.

## Validation

`tests/cluster_data_plane.rs` — **5 tests**, 3× stable: insert-then-search
with correct nearest-neighbour ordering; 16 concurrent inserts all landing
exactly once; HTTP retry with a `request_id` answered `deduplicated:
true`; a write to a real follower answered 307 with the leader's API URL
in `Location`; health + metrics served.

Full workspace: **258 passing, 0 failures.**

## Follow-ups

- Full Engine integration: HNSW/IVF-indexed search, graph + batch +
  snapshot endpoints in cluster mode (the v1 data plane is brute-force
  search + insert only).
- Record-id allocation inside the state machine (kernel event-schema
  change) — removes the per-node insert serialization.
- 2.10d (partition harness) — the one remaining Phase 2 item.
