# Valori Node API Reference

This document lists all available endpoints, their methods, purposes, and example usage.

## Base URL
- **Local**: `http://localhost:3000`
- **Production**: `https://<your-app>.koyeb.app`

---

## 🚀 Core & System

| Endpoint  | Method | Purpose |
| :---      | :---   | :---    |
| `/health` | `GET`  | Check if server is running. |
| `/version`| `GET`  | Check server version. |
| `/metrics`| `GET`  | Prometheus metrics for observability. |

**Examples:**
```bash
curl http://localhost:3000/health
curl http://localhost:3000/version
curl http://localhost:3000/metrics
```

---

## 🧠 Memory Protocol (Recommended for Agents)
These are the high-level endpoints for AI Agents (Orchestrators).

| Endpoint  | Method | Purpose | Payload |
| :---      | :---   | :---    | :---    |
| `/v1/memory/upsert_vector` | `POST` | Insert vector + metadata + graph nodes. | `{"vector": [...], "metadata": {...}}` |
| `/v1/memory/search_vector` | `POST` | Search for similar vectors. | `{"query_vector": [...], "k": 5}` |
| `/v1/memory/meta/get` | `GET` | Retrieve metadata by ID. | Query Param: `?target_id=rec:0` |
| `/v1/memory/meta/set` | `POST` | Update metadata for existing ID. | `{"target_id": "rec:0", "metadata": {...}}` |


**Examples:**
```bash
# Insert (Upsert)
curl -X POST http://localhost:3000/v1/memory/upsert_vector \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, ...], "metadata": {"role": "memory"}}'

# Search
curl -X POST http://localhost:3000/v1/memory/search_vector \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, ...], "k": 1}'

# Get Metadata
curl "http://localhost:3000/v1/memory/meta/get?target_id=rec:0"

# Update Metadata (without re-inserting vector)
curl -X POST http://localhost:3000/v1/memory/meta/set \
  -H "Content-Type: application/json" \
  -d '{"target_id": "rec:0", "metadata": {"status": "updated"}}'
```


---

## ⚡ Low-Level Kernel Operations
Direct access to the vector engine and graph primitives.

| Endpoint  | Method | Purpose |
| :---      | :---   | :---    |
| `/records` | `POST` | Insert raw vector (no metadata). |
| `/v1/vectors/batch_insert` | `POST` | Insert multiple vectors at once. |
| `/v1/delete` | `POST` | **Soft Delete** a record by ID. |
| `/search` | `POST` | Raw vector search (returns IDs/Scores only). |
| `/graph/node` | `POST` | Create a standalone graph node. |
| `/graph/edge` | `POST` | Create an edge between nodes. |

**Examples:**
```bash
# Delete Record 0
curl -X POST http://localhost:3000/v1/delete \
  -H "Content-Type: application/json" \
  -d '{"id": 0}'

# Batch Insert
curl -X POST http://localhost:3000/v1/vectors/batch_insert \
  -H "Content-Type: application/json" \
  -d '{"batch": [[0.1, ...], [0.2, ...]]}'
```

---

## 📸 Snapshots & Recovery
Manage disk persistence manually (if S3/WAL is not used).

| Endpoint  | Method | Purpose |
| :---      | :---   | :---    |
| `/v1/snapshot/save` | `POST` | Save in-memory state to disk. |
| `/v1/snapshot/restore` | `POST` | Load state from disk file. |
| `/v1/snapshot/download` | `GET` | Download full snapshot as binary. |
| `/v1/snapshot/upload` | `POST` | Upload binary snapshot to restore state. |

**Examples:**
```bash
# Save Snapshot
curl -X POST http://localhost:3000/v1/snapshot/save \
  -H "Content-Type: application/json" \
  -d '{"path": "./backup.snap"}'
```

---

## 🛡️ Proofs & Audit
Deterministic verification features.

| Endpoint  | Method | Purpose |
| :---      | :---   | :---    |
| `/v1/proof/state` | `GET` | Get cryptographic hash of current state. |
| `/v1/proof/event-log` | `GET` | Get hash of the immutable event log. |

**Examples:**
```bash
curl http://localhost:3000/v1/proof/state
```

---

## 🔄 Replication
Used by Follower nodes to sync with the Leader.

| Endpoint  | Method | Purpose |
| :---      | :---   | :---    |
| `/v1/replication/wal` | `GET` | Stream the Write-Ahead Log. |
| `/v1/replication/events` | `GET` | Stream real-time events. |
| `/v1/replication/state` | `GET` | Check replication status (Synced/Healing). |

---

## 🗳️ Cluster Management (Phase 2.6 — Raft cluster mode)
Available when the node boots in cluster mode (`VALORI_CLUSTER_MEMBERS` set).
Membership changes are leader-only; a follower answers **403** with the
leader's API address.

| Endpoint  | Method | Purpose |
| :---      | :---   | :---    |
| `/v1/cluster/status` | `GET` | Leader, term, log indexes, membership (with voter flags). |
| `/v1/cluster/health` | `GET` | 200 when this node sees a leader; 503 `no-leader` otherwise. |
| `/v1/cluster/add-node` | `POST` | Join a node: learner catch-up, then voter promotion. |
| `/v1/cluster/remove-node` | `POST` | Remove a voter (last-voter removal refused with 422). |

**Examples:**
```bash
curl http://localhost:3000/v1/cluster/status

# Add node 2 (its raft listener, optionally its API addr)
curl -X POST http://localhost:3000/v1/cluster/add-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 2, "raft_addr": "10.0.0.2:3100", "api_addr": "10.0.0.2:3000"}'

# Remove node 2
curl -X POST http://localhost:3000/v1/cluster/remove-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 2}'
```

**Cluster boot environment:**

| Variable | Meaning |
| :--- | :--- |
| `VALORI_CLUSTER_MEMBERS` | `id=raft_addr/api_addr,…` — presence switches cluster mode on. |
| `VALORI_NODE_ID` | This node's id (must appear in members). |
| `VALORI_RAFT_BIND` | gRPC consensus listener (default `0.0.0.0:3100`). |
| `VALORI_CLUSTER_INIT` | `1` on exactly one node of a brand-new cluster. |
