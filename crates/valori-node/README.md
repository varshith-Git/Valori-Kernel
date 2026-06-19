# valori-node

HTTP API server and orchestration layer for Valori. Runs in standalone mode
or as a member of a Raft cluster (`VALORI_CLUSTER_MEMBERS`).

## Base URL

- **Local**: `http://localhost:3000`
- **Production**: `https://<your-app>.koyeb.app`

---

## Core & System

| Endpoint | Method | Description |
|---|---|---|
| `/health` | `GET` | Liveness probe. |
| `/version` | `GET` | Server version string. |
| `/metrics` | `GET` | Prometheus metrics. |

```bash
curl http://localhost:3000/health
curl http://localhost:3000/version
```

---

## Collections (Multi-tenancy)

Valori supports up to **1 024 named collections** (namespaces). Every data
endpoint accepts an optional `"collection"` field. Omitting it (or setting it
to `"default"`) targets the always-present default collection.

Records in non-default collections are **fully isolated** — they never appear
in default-collection searches and vice versa.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/namespaces` | `POST` | Create a collection (idempotent). |
| `/v1/namespaces` | `GET` | List all collections and their numeric IDs. |
| `/v1/namespaces/:name` | `DELETE` | Drop a collection and all its records. |

### Create a collection

```bash
curl -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "tenant-acme"}'
# → {"name":"tenant-acme","id":1,"created":true}
# Second call with the same name → {"created":false,"id":1} (idempotent)
```

### List collections

```bash
curl http://localhost:3000/v1/namespaces
# → {"collections":[{"name":"default","id":0},{"name":"tenant-acme","id":1}]}
```

### Drop a collection

```bash
curl -X DELETE http://localhost:3000/v1/namespaces/tenant-acme
# → 204 No Content
# Dropping "default" → 400 Bad Request
```

---

## Vector Operations

All endpoints accept an optional `"collection"` field. If the named collection
does not exist the request is rejected with `400 Bad Request`.

| Endpoint | Method | Description |
|---|---|---|
| `/records` | `POST` | Insert a single vector. |
| `/v1/vectors/batch_insert` | `POST` | Insert multiple vectors in one call. |
| `/search` | `POST` | K-nearest-neighbour search. |
| `/v1/delete` | `POST` | Soft-delete a record by ID. |

### Insert into a collection

```bash
curl -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"values": [0.1, 0.2, 0.3, 0.4], "collection": "tenant-acme"}'
# → {"id": 0}
```

### Batch insert

```bash
curl -X POST http://localhost:3000/v1/vectors/batch_insert \
  -H "Content-Type: application/json" \
  -d '{"batch": [[0.1,0.2,0.3,0.4],[0.5,0.6,0.7,0.8]], "collection": "tenant-acme"}'
# → {"ids": [0, 1]}
```

### Search within a collection

```bash
# Scoped to tenant-acme — default-namespace records are excluded.
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "collection": "tenant-acme"}'
# → {"results":[{"id":0,"score":0.0}]}

# Search the default collection (no "collection" field needed).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5}'
```

---

## Memory Protocol (Recommended for AI agents)

High-level endpoints that combine vector storage with graph metadata.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/memory/upsert_vector` | `POST` | Insert vector + metadata + graph nodes. |
| `/v1/memory/search_vector` | `POST` | Search for similar vectors. |
| `/v1/memory/meta/get` | `GET` | Retrieve metadata by ID. |
| `/v1/memory/meta/set` | `POST` | Update metadata for an existing ID. |

```bash
curl -X POST http://localhost:3000/v1/memory/upsert_vector \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4], "metadata": {"role": "assistant-memory"}}'

curl -X POST http://localhost:3000/v1/memory/search_vector \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, 0.3, 0.4], "k": 3}'
```

---

## Snapshots & Recovery

| Endpoint | Method | Description |
|---|---|---|
| `/v1/snapshot/save` | `POST` | Persist in-memory state to disk. |
| `/v1/snapshot/restore` | `POST` | Restore state from a disk file. |
| `/v1/snapshot/download` | `GET` | Download the snapshot as raw bytes. |
| `/v1/snapshot/upload` | `POST` | Upload a snapshot binary to restore state. |

Snapshots include the full namespace registry — collection names, IDs, and all
records survive a round-trip.

```bash
curl -X POST http://localhost:3000/v1/snapshot/save \
  -H "Content-Type: application/json" \
  -d '{"path": "./backup.snap"}'
```

---

## Proofs & Audit

| Endpoint | Method | Description |
|---|---|---|
| `/v1/proof/state` | `GET` | BLAKE3 hash of the current engine state (hex). |
| `/v1/proof/event-log` | `GET` | BLAKE3 hash of the immutable event log (hex). |

```bash
curl http://localhost:3000/v1/proof/state
# → {"final_state_hash":"a3f2..."}
```

---

## Cluster Management

Available when the node boots in cluster mode (`VALORI_CLUSTER_MEMBERS` set).
Write requests are leader-only; a follower answers **403** with the leader's
API address.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/cluster/status` | `GET` | Leader, term, log indices, membership. |
| `/v1/cluster/health` | `GET` | `200` when a leader is visible; `503` otherwise. |
| `/v1/cluster/role` | `GET` | This node's current Raft role (`Leader`/`Follower`/`Candidate`). |
| `/v1/cluster/add-node` | `POST` | Join a node (learner catch-up → voter promotion). |
| `/v1/cluster/remove-node` | `POST` | Remove a voter (last-voter removal refused with `422`). |

```bash
curl http://localhost:3000/v1/cluster/status

curl -X POST http://localhost:3000/v1/cluster/add-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 2, "raft_addr": "10.0.0.2:3100", "api_addr": "10.0.0.2:3000"}'

curl -X POST http://localhost:3000/v1/cluster/remove-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 2}'
```

### Cluster environment variables

| Variable | Description |
|---|---|
| `VALORI_CLUSTER_MEMBERS` | `id=raft_addr/api_addr,…` — presence activates cluster mode. |
| `VALORI_NODE_ID` | This node's ID (must appear in `VALORI_CLUSTER_MEMBERS`). |
| `VALORI_RAFT_BIND` | gRPC consensus listener (default `0.0.0.0:3100`). |
| `VALORI_CLUSTER_INIT` | Set to `1` on exactly one node of a brand-new cluster. |
| `VALORI_RAFT_LOG_PATH` | Path to the `redb` file for the persistent Raft log. When set, the state machine shares this database so `last_applied` and snapshots survive restarts without replaying audit events. |

---

## Python SDK

```python
from valoricore.remote import ValoriClient

client = ValoriClient("http://localhost:3000")

# Create a collection
client.create_collection("tenant-acme")

# Insert into a named collection
record_id = client.insert([0.1, 0.2, 0.3, 0.4], collection="tenant-acme")

# Batch insert
ids = client.insert_batch([[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]],
                          collection="tenant-acme")

# Scoped search
results = client.search([0.1, 0.2, 0.3, 0.4], k=5, collection="tenant-acme")

# List and drop
collections = client.list_collections()   # [{"name": "default", "id": 0}, ...]
client.drop_collection("tenant-acme")
```

---

## Testing

```bash
cargo test -p valori-node
```

Key test suites:

| Suite | What it covers |
|---|---|
| `tests/collections.rs` | 16 tests: collection CRUD, namespace isolation, snapshot persistence, error paths. |
| `tests/cluster_boot.rs` | Single-node Raft boot, restart recovery from redb log, state-hash watcher teardown. |
| `tests/replication.rs` | Leader→follower snapshot push, `LeaderProof` hex-format verification. |
| `tests/api.rs` | All HTTP endpoints, status codes, and response shapes. |
