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
