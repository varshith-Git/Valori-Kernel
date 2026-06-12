# Phase 2.6 — Cluster Management API

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 6 of 10

## Goal

Give operators an HTTP control plane for the cluster: status, health, and
membership changes — with every change going through Raft itself, so a
membership change is committed, durable, and ordered with respect to data
writes exactly like any other entry.

## Delivered

**`src/cluster_api.rs` — `cluster_router(Arc<Raft>)`**, mounted next to the
data-plane router when the node boots in cluster mode:

| Method | Path | Behaviour |
|---|---|---|
| GET | `/v1/cluster/status` | node id, leader, `is_leader`, term, last log/applied indexes, full membership with per-member voter flag and both addresses |
| GET | `/v1/cluster/health` | 200 + leader id when this node sees a leader; **503 `no-leader`** otherwise — the Docker/compose health check (Phase 1.11) plugs straight in |
| POST | `/v1/cluster/add-node` | `add_learner` (catch up without affecting quorum) then promote to voter via `change_membership` |
| POST | `/v1/cluster/remove-node` | voter removal; **422 `cannot-remove-last-voter`** guard |

Semantics chosen deliberately:

- **Leader-only writes, 403 on followers** — the error body carries
  openraft's ForwardToLeader detail so operators and scripts can retry
  against the right node.
- **Learner-then-voter join** — the new node replicates the existing log
  (or receives a snapshot, Phase 2.7) before it counts toward quorum; a
  cold join can never stall writes.
- **`retain: false` on promotion/removal** — members move between sets
  rather than lingering in both.
- **Last-voter protection** — removing the only voter would brick the
  cluster; refused with 422 before reaching Raft.

## Findings

- openraft's `change_membership` takes the new **voter-id set** — node
  addresses are registered by `add_learner` and live in the membership
  storage already. The first draft passed an id→node map, which type-errors;
  worth knowing because the two-step add-node dance (learner with address,
  then promote by id) is the API's intended shape, not an accident.

## Validation

`tests/cluster_api.rs` — **6 tests** over real booted clusters (gRPC
servers, elections), driven through `tower::oneshot`, 3× stable:

- status reports leadership + membership with voter flags
- health: 503 before any leader exists, 200 after initialization
- **add-node grows 1→2 and the joined node converges to the leader's exact
  state hash** (including a write committed before the join)
- remove-node shrinks back to one voter
- removing the last voter → 422
- membership change on an uninitialized (leaderless) node → 403

Full workspace: 235 passing, 0 failures.

## Follow-ups

- Phase 2.9: emit `NodeJoined`/`NodeLeft` admin events into the chained
  audit log from these endpoints (the Phase 1.6 schema).
- Phase 2.7: snapshot-based catch-up for joins into a long history (the
  add-node test joins into a short log; log replay suffices there).
- The Engine `Box<dyn Committer>` swap + `main.rs` boot dispatch remain
  open — tracked for 2.10 alongside the server assembly work, where the
  standalone/cluster router composition comes together.
- Auth: these endpoints must be admin-token-gated when the security model
  (Phase 1.6 / Phase 3 RBAC `ADMIN` role) lands.
