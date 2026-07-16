# Running a Valori Cluster

Valori runs as a single standalone node or as a Raft-replicated cluster. This
guide covers the cluster: how to start one, operate it, grow it, and recover it.

## Mental model

A cluster is an odd number of nodes (3 or 5 in practice) running Raft consensus.

- **One leader, the rest followers.** The leader orders all writes; followers
  replicate the log and apply it in the same order, so every node holds
  byte-identical state (verified by a shared BLAKE3 state hash).
- **Writes go to the leader.** Send a write to a follower and it answers
  `307 Temporary Redirect` with the leader's address in the `Location` header.
  The Python SDK and `curl -L` follow this automatically.
- **Reads are linearizable by default**, on any node. A follower establishes a
  read index against the leader (the read-index protocol) and waits for its own
  apply to catch up before answering, so the result reflects every write
  committed before the read began. Pass `consistency: "local"` (SDK:
  `search(..., consistency="local")`) to skip the round trip and read
  immediately from the queried node — eventually consistent, but faster.
- **A quorum (majority) must agree to commit.** A 3-node cluster tolerates 1
  node down; a 5-node cluster tolerates 2. A minority partition cannot commit
  writes (it stalls rather than forking — see [fault tolerance](#fault-tolerance)).

Two logs, never conflated:
- The **Raft log** is internal plumbing — truncatable, purgeable.
- The **audit log** (`events.log`, BLAKE3-chained) is append-only, written once
  per event at apply time, after quorum commit. This is what `valori-verify` audits.

## Three ways to start a cluster

### 1. The interactive wizard (easiest)

```bash
valori setup           # or just: valori
```

Pick "Multi-node", choose a node count (default 3), and the wizard starts all
nodes **in one process** and drops you into a live menu (insert, search, status,
add node, exit). Projects persist to `~/.valori/projects.json` and are offered
for resume next time. For a server/EC2 where clients connect from outside:

```bash
valori setup --bind 0.0.0.0
```

Default ports: API `51000, 51001, …`; Raft `51100, 51101, …` (API base + 100).

### 2. Docker Compose (3-node, closest to production)

```bash
docker compose up -d --build      # or: make cluster
docker compose ps                 # wait for all 3 healthy (~30s)
make cluster-down                 # tear down + wipe volumes
```

Nodes are published on host ports `3001`, `3002`, `3003`. From your laptop:

```bash
curl http://localhost:3001/health
curl -X POST http://localhost:3001/records \
     -H 'Content-Type: application/json' -d '{"values":[1.0,2.0,3.0]}'
```

See [DEPLOYMENT.md](DEPLOYMENT.md) for the EC2 playbook.

### 3. Manual, one process per node (full control)

Each node is a `valori-node` process configured entirely by environment. Boot
mode is decided by the presence of `VALORI_CLUSTER_MEMBERS`.

```bash
# Node 1 — the one that bootstraps the cluster (VALORI_CLUSTER_INIT=1)
VALORI_NODE_ID=1 \
VALORI_CLUSTER_INIT=1 \
VALORI_CLUSTER_MEMBERS="1=10.0.0.1:3100/10.0.0.1:3000,2=10.0.0.2:3100/10.0.0.2:3000,3=10.0.0.3:3100/10.0.0.3:3000" \
VALORI_RAFT_BIND=0.0.0.0:3100 \
VALORI_BIND=0.0.0.0:3000 \
VALORI_EVENT_LOG_PATH=/data/events.log \
VALORI_RAFT_LOG_PATH=/data/raft.redb \
  valori-node

# Nodes 2 and 3 — identical, but VALORI_NODE_ID=2/3 and NO VALORI_CLUSTER_INIT
```

`start-local-cluster.sh` runs this layout for 3 nodes locally without Docker.

## Environment reference

| Variable | Required | Meaning |
|---|---|---|
| `VALORI_CLUSTER_MEMBERS` | yes (cluster) | Topology: comma-separated `id=raft_addr/api_addr`. Its presence switches cluster mode on. |
| `VALORI_NODE_ID` | yes (cluster) | This node's numeric id — must appear in members. |
| `VALORI_CLUSTER_INIT` | one node | Set to `1` on exactly one node of a *new* cluster to bootstrap it. Never on a joiner. |
| `VALORI_RAFT_BIND` | no | gRPC consensus listener. Default `0.0.0.0:3100`. |
| `VALORI_BIND` | no | HTTP API listener. Default `0.0.0.0:3000`. |
| `VALORI_EVENT_LOG_PATH` | recommended | Path to the BLAKE3-chained audit log. Without it the node replicates but doesn't persist the audit chain locally. |
| `VALORI_EVENT_LOG_ROTATION_BYTES` | no | Seal the live `events.log` once it passes this many bytes (default 256 MiB; `0` disables). Sealed segments become `events.log.NNNNNN`; recovery replays them all. |
| `VALORI_RAFT_LOG_PATH` | recommended | redb path for a persistent Raft log + vote (survives restarts). Omit for in-memory. |
| `VALORI_TLS_CA` / `VALORI_TLS_CERT` / `VALORI_TLS_KEY` | no | All three → mutual TLS on the Raft channel. Partial → boot error. |
| `VALORI_TLS_DOMAIN` | no | Shared cert domain name. Default `valori-cluster.internal`. |
| `VALORI_SHARD_COUNT` | no | **Phase S1 — multi-Raft skeleton.** Number of independent Raft groups this process runs, sharing one gRPC listener. Default `1` (byte-identical to today's single-Raft-group behavior). Every configured member runs every shard (symmetric placement) — there is no namespace routing yet, so shards beyond 0 have no HTTP surface. See [phase-S1-multi-raft-skeleton.md](phases/phase-S1-multi-raft-skeleton.md). |

A malformed topology is a **hard stop** — the node refuses to boot rather than
silently starting standalone (which would be a split-brain factory).

## HTTP API

**Data plane** (any node; writes redirect to leader):

| Route | Method | Purpose |
|---|---|---|
| `/records` | POST | Insert a vector → `{ id }` |
| `/search` | POST | k-NN over the local replica → `{ results: [{id, score}] }` |
| `/v1/delete` | POST | Hard-delete a record |
| `/v1/soft-delete` | POST | Tombstone a record |
| `/v1/vectors/batch_insert` | POST | Insert many → `{ ids: [...] }` |
| `/v1/proof/state` | GET | `{ final_state_hash }` — the cross-node equality invariant |
| `/health`, `/metrics` | GET | Health / Prometheus (includes `valori_raft_*` gauges) |

**Management plane** (`/v1/cluster/*`):

| Route | Method | Purpose |
|---|---|---|
| `/v1/cluster/status` | GET | Leader, term, log indexes, member table |
| `/v1/cluster/health` | GET | `200` if a leader is elected, `503` otherwise |
| `/v1/cluster/role` | GET | `{"role":"leader"\|"follower","node_id":N}` — for LB write routing |
| `/v1/cluster/read-index` | GET | Leader-only: returns the read index for linearizable reads (`503` + leader id on a follower) |
| `/v1/cluster/add-node` | POST | Add a member (learner catch-up → voter). Leader-only. |
| `/v1/cluster/remove-node` | POST | Remove a voter. Leader-only. |

Membership changes are leader-only; a follower answers `403` naming the leader.

## The `valori cluster` CLI

Point `--url` at **any** node — the CLI follows redirects for leader-only actions.

```bash
valori cluster status  --url http://10.0.0.1:3000      # leadership + members
valori cluster health  --url http://10.0.0.1:3000      # exit 0 if a leader exists

# Grow: join a node already running elsewhere
valori cluster add-node --url http://<leader>:3000 \
    --id 4 --raft-addr 10.0.0.4:3100 --api-addr 10.0.0.4:3000

# Shrink: remove a voter (removing the last voter is refused)
valori cluster remove-node --url http://<leader>:3000 --id 4
```

`add-node` does the two-step openraft dance for you: it adds the node as a
**learner** (so it catches up on the log without affecting quorum), then
promotes it to a **voter**. The new node must already be running with the same
`VALORI_CLUSTER_MEMBERS` topology and **without** `VALORI_CLUSTER_INIT`.

## Growing a cluster

1. Start the new node (env-configured, no `VALORI_CLUSTER_INIT`).
2. `valori cluster add-node --url http://<leader>:3000 --id N --raft-addr … --api-addr …`
3. Confirm with `valori cluster status` — the new id should appear as a voter.

In the wizard, "Add another node (this machine)" and "Grow cluster (join a node
elsewhere)" do the same thing interactively.

## Fault tolerance

- **Follower down**: no impact on writes — the leader + remaining followers
  still form a quorum. The follower catches up on rejoin (via log or snapshot).
- **Leader down**: the remaining nodes elect a new leader within the election
  timeout (sub-second by default); writes resume automatically. SDK clients
  retry transparently.
- **Network partition**: only the side with a majority can commit. The minority
  side stalls writes (it does **not** fork) and reconciles when the partition heals.
  These properties are covered by the partition harness in
  `crates/valori-consensus/src/partition_harness.rs`.

## Driving a cluster from Python

Point the SDK at any node; it handles leader redirects and election retries:

```python
from valoricore import SyncRemoteClient

db = SyncRemoteClient("http://10.0.0.2:3000")   # any node, not necessarily the leader
rid = db.insert([1.0, 2.0, 3.0])                # transparently redirected to the leader
hits = db.search([1.0, 2.0, 3.0], k=5)          # served locally by node 2

print(db.cluster_status())                       # {'leader': 1, 'term': 3, 'members': [...]}
print(db.cluster_health())                       # True
print(db.get_state_hash())                        # same hash on every node
```

The async client (`AsyncRemoteClient`) has the same surface and also follows
redirects. On a prolonged leaderless window both raise `NotLeaderError` after
exhausting retries (`max_retries`, `retry_backoff` are constructor args).

## Verifying a cluster

Every node should report the **same** `final_state_hash`. To check:

```bash
for p in 3001 3002 3003; do curl -s http://localhost:$p/v1/proof/state; echo; done
```

To audit the append-only history of any node, copy its `events.log` and run the
offline verifier — no server required:

```bash
valori-verify --log /path/to/events.log
```

## Active divergence detection

Each node runs a background task (every 30 s by default, configurable via
`VALORI_STATE_HASH_CHECK_SECS`) that calls `/v1/proof/state` on every peer and
compares the returned BLAKE3 state hash to its own. The Prometheus gauge
`valori_raft_state_hash_match` is `1` when all reachable peers agree, `0` when
any peer reports a different hash. Mismatches are logged at `ERROR` level and
counted by `valori_raft_divergence_detections_total`.

In a healthy cluster this gauge should always be `1`. Wire an alert on:

```promql
valori_raft_state_hash_match == 0
```

## Load-balancer write routing

To avoid 307-redirect round trips for every write, configure your load balancer
to prefer the leader pod. Use `/v1/cluster/role` as the health-check endpoint:

```bash
# e.g. AWS Target Group health check per pod:
# Path: /v1/cluster/role  Method: GET  Healthy threshold: 200 OK
# Then create two target groups: one for leader-only (role=="leader" check via Lambda
# or nginx return), and one for all-nodes reads.
```

The simplest pattern: all three pods are in the read target group; writes go
through an nginx `lua` block or AWS Lambda that forwards to whichever pod
returns `"role":"leader"`. The SDK fallback (307 redirect) remains the safety net.

## Kubernetes (Helm)

```bash
helm install valori ./deploy/helm/valori \
  --set replicaCount=3 \
  --set image.tag=0.2.1
```

See [`deploy/helm/valori/values.yaml`](../deploy/helm/valori/values.yaml) for
the full configuration reference (storage classes, resource limits, mTLS, etc.).
