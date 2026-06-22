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
| `/search` | `POST` | K-nearest-neighbour search. Supports `as_of` / `as_of_log_index` for point-in-time reads. |
| `/v1/delete` | `POST` | Soft-delete a record by ID. |
| `/v1/timeline` | `GET` | Structured event timeline. Accepts `from=<ISO8601>` and `to=<ISO8601>` filters. |

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

### Batch insert with per-item idempotency (Phase 3.12)

Supply a 32-hex string per slot in `request_ids`. Duplicate keys are detected
server-side and the previously assigned record ID is returned — safe for
at-least-once delivery.

```bash
curl -X POST http://localhost:3000/v1/vectors/batch_insert \
  -H "Content-Type: application/json" \
  -d '{
    "batch": [[0.1,0.2,0.3,0.4],[0.5,0.6,0.7,0.8]],
    "request_ids": ["aabbccddeeff00112233445566778899", null]
  }'
# → {"ids": [0, 1]}
# Retrying with the same request_id returns the same IDs without double-insert.
```

A `null` entry in `request_ids` opts that slot out of dedup. Omitting
`request_ids` entirely is fully backward-compatible.

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

### Point-in-time (as-of) search — Phase 3.4

Requires `VALORI_EVENT_LOG_PATH` to be set. Replays the event log up to the
target point and searches the resulting state.

```bash
# Search as the state was after the 5th committed event (log_index 4).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "as_of_log_index": 4}'
# → {
#     "results": [...],
#     "as_of_log_index": 4,
#     "as_of_timestamp_iso": "2026-03-03T10:00:00Z",
#     "as_of_state_hash": "a3f...bc9"   ← BLAKE3 of the replayed state
#   }

# Search the state as it existed on March 3, 2026 (UTC).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "as_of": "2026-03-03T00:00:00Z"}'
```

```bash
# Python SDK
from valoricore.remote import SyncRemoteClient
c = SyncRemoteClient("http://localhost:3000")
resp = c.search([0.1, 0.2, 0.3, 0.4], k=5, as_of="2026-03-03T00:00:00Z")
print(resp["results"], resp["as_of_state_hash"])
```

### Event timeline

```bash
# All events (structured JSON).
curl http://localhost:3000/v1/timeline

# Events in a specific time window.
curl "http://localhost:3000/v1/timeline?from=2026-03-01T00:00:00Z&to=2026-03-31T23:59:59Z"
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

### GraphRAG — `POST /v1/graphrag` (Phase 3.15)

Retrieve the K nearest vectors **and** the connected knowledge subgraph around
them in a single call, from one consistent kernel snapshot — no second store, no
cross-system drift. Vectors and graph live in the same kernel, so the KNN, the
record→node resolution, and the subgraph BFS all run under one read lock.

```bash
curl -X POST http://localhost:3000/v1/graphrag \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, 0.3, 0.4], "k": 5, "depth": 2}'
```

```jsonc
{
  "hits":       [ { "memory_id", "record_id", "score", "node_id", "metadata" } ],
  "seed_nodes": [ /* node ids the hits mapped to */ ],
  "subgraph":   { "nodes": [...], "edges": [...] }
}
```

`depth` is clamped to 4. The subgraph is only as rich as the edges that exist
(ingest creates a document→chunk edge per memory; entity/citation edges and
manual `/graph/edge` calls add more). On a cluster the request also honours
`consistency` (linearizable by default). For agents, prefer the
`memory_graph_recall` MCP tool, which wraps this with a verifiable receipt.

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

## API Key Management (Phase 3.5)

Per-tenant scoped credentials. Three scope tiers: `read_only < read_write < admin`.

| Endpoint | Method | Scope required | Description |
|---|---|---|---|
| `/v1/keys` | `POST` | admin | Create a new API key. |
| `/v1/keys` | `GET` | admin | List all keys (masked — no raw token). |
| `/v1/keys/:id` | `DELETE` | admin | Revoke a key immediately. |

```bash
# Create a read-write key (using the legacy admin token or an existing admin key).
curl -X POST http://localhost:3000/v1/keys \
  -H "Authorization: Bearer <admin-token>" \
  -H "Content-Type: application/json" \
  -d '{"scope": "read_write", "description": "tenant-acme write key"}'
# → {"id":"key_a3f2...","token":"vk_...","scope":"read_write","created_at":1719000000}
# Token is shown ONCE — store it now.

# List keys (token masked after creation).
curl http://localhost:3000/v1/keys \
  -H "Authorization: Bearer <admin-token>"

# Revoke a key.
curl -X DELETE http://localhost:3000/v1/keys/key_a3f2... \
  -H "Authorization: Bearer <admin-token>"
```

Env var: `VALORI_KEYS_PATH=./keys.json` — persist across restarts.
`VALORI_AUTH_TOKEN` continues to work as a legacy admin credential.

---

## Crypto-shredding / GDPR Erasure (Phase 3.6)

AES-256-GCM per-record encryption with cryptographic erasure. Destroying a
Data Encryption Key (DEK) makes all data encrypted under it permanently
unrecoverable — GDPR Article 17 compliance without truncating the audit log.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/records/encrypted` | `POST` | Encrypt payload and store as a non-searchable record. Returns `{"id", "key_id"}`. |
| `/v1/crypto/shred/:key_id` | `DELETE` | Destroy the DEK. All records under this key become permanently unrecoverable. |
| `/v1/crypto/status/:key_id` | `GET` | Returns `{"exists": bool}` — false after shredding. |

**Request body for `POST /v1/records/encrypted`:**
```json
{
  "payload": "<base64-encoded plaintext>",
  "tag": 0,
  "collection": "default",
  "key_id": "<optional 32-hex key — omit for auto-generated>"
}
```

**Durability:** Set `VALORI_SHRED_LOG_PATH=./shred.log` to persist shredded key_ids across restarts.

**Grouping:** Multiple records can share one `key_id` and be shredded atomically with a single `DELETE`.

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
| `VALORI_SNAPSHOT_EVERY_EVENTS` | Trigger a Raft snapshot every N applied entries (default `5000`). Lower values bound the log-replay window on restart at the cost of more snapshot I/O. |
| `VALORI_RAFT_SNAPSHOT_KEEP` | Log entries to retain after each snapshot for followers that are only slightly behind (default `1000`). |
| `VALORI_STATE_HASH_CHECK_SECS` | Hash-convergence poll interval in seconds (default `30`, `0` = off). |

---

## Python SDK

### Single-node client

```python
from valoricore.remote import SyncRemoteClient

client = SyncRemoteClient("http://localhost:3000")

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

### Multi-node cluster client

```python
from valoricore.remote import ClusterClient

# Point at all nodes — client discovers the leader automatically.
c = ClusterClient([
    "http://node1:3000",
    "http://node2:3000",
    "http://node3:3000",
])

# Writes go to the leader (307-redirect self-heal).
rid = c.insert([0.1, 0.2, 0.3, 0.4], collection="tenant-acme")

# Local reads round-robin across all nodes for throughput.
hits = c.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="local")

# Linearizable reads go to the leader.
hits = c.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="linearizable")

# Cluster inspection.
print(c.leader_url())          # → 'http://node2:3000'
print(c.get_cluster_role())    # → 'leader'
print(c.cluster_health())      # → True
```

Every mutating call auto-generates a UUID4 idempotency key. On a retry after a
connection reset, the Raft cluster deduplicates the write server-side.
Pass `idempotency_key=my_bytes` to supply your own 16-byte token.
```

---

## Index configuration (Phase 3.13)

### `GET /v1/index/config`

Returns the active index type and its parameters.

```bash
curl http://localhost:3000/v1/index/config
# BruteForce (default):
# {"index_type":"brute_force","hnsw":null}

# HNSW:
# {"index_type":"hnsw","hnsw":{"m":16,"m_max0":32,"ef_construction":100,"ef_search":50}}
```

### HNSW environment variables

| Variable | Default | Description |
|---|---|---|
| `VALORI_HNSW_M` | `16` | Max edges per node per layer. `m_max0` and `lambda` are derived automatically (`m_max0 = 2*M`). |
| `VALORI_HNSW_EF_CONSTRUCTION` | `100` | Beam width during index build. Higher = better recall, slower inserts. |
| `VALORI_HNSW_EF_SEARCH` | `50` | Beam width floor during queries. Higher = better recall, slower search. |

Only takes effect when `VALORI_INDEX=hnsw`. Has no effect in cluster mode (cluster uses kernel brute-force for linearizable consistency).

---

## Concurrency model (Phase 3.11)

The engine state is wrapped in `Arc<RwLock<Engine>>`. Read-only handlers
(search, proof, health, timeline, metrics, list collections, etc.) acquire a
shared read lock and execute concurrently. Write handlers (insert, delete,
restore, shred) acquire an exclusive write lock.

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
| `tests/api_batch_idempotency.rs` | 4 tests: per-item dedup, mixed batches, backward compat, fully-deduped batch. |
| `tests/api_index_config.rs` | 5 tests: brute-force config, HNSW defaults, custom M derivation, ef_search, all params. |
