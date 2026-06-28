# Contributing to Valori

This guide gets you from a fresh clone to a running node in under 10 minutes.

## Quickstart — automated setup

Run this once after cloning. It installs Rust, the wasm32 target, Python SDK, and UI dependencies, then builds the workspace:

```bash
bash dev-setup.sh
```

Works on macOS and Linux. If you prefer to install things manually, follow the steps below.

---

## Prerequisites

| Tool | Minimum version | How to install |
|---|---|---|
| **Rust + cargo** | 1.80 stable | `curl https://sh.rustup.rs -sSf \| sh` |
| **Python** | 3.9+ | `brew install python` or system package manager |
| **Node.js** | 18+ (UI only) | `brew install node` or [nodejs.org](https://nodejs.org) |
| **Docker** | any recent (cluster only) | [docker.com](https://www.docker.com) |

Verify your Rust install:

```bash
rustc --version    # should print 1.80 or newer
cargo --version
```

> **wasm32 target** — only needed if you touch `crates/valori-kernel`:
> ```bash
> rustup target add wasm32-unknown-unknown
> ```

---

## 1. Clone and build

```bash
git clone https://github.com/varshith-Git/Valori-Kernel.git
cd Valori-Kernel

# Build all default crates (excludes valori-ffi which needs maturin)
cargo build

# Run the core test suite
cargo test -p valori-kernel -p valori-node
```

All tests should pass. On Linux, install `build-essential` / `gcc` if you hit a linker error.

---

## 2. Run a standalone node (the simplest start)

```bash
# In-memory only — no persistence, data lost on restart (fastest for dev/testing)
VALORI_DIM=128 cargo run -p valori-node

# With WAL + snapshot — data survives restarts (recommended for real work)
VALORI_DIM=128 \
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori.snap \
cargo run -p valori-node

# With auth token — all endpoints require "Authorization: Bearer <token>"
VALORI_DIM=128 \
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori.snap \
VALORI_AUTH_TOKEN=mysecrettoken \
cargo run -p valori-node

# HNSW index instead of brute-force (faster search, uses more RAM)
VALORI_DIM=128 \
VALORI_INDEX=hnsw \
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori.snap \
cargo run -p valori-node

# Custom port (e.g. 8080 — default is 3000)
VALORI_DIM=128 \
VALORI_BIND=0.0.0.0:3000 \
cargo run -p valori-node

# With embedding provider (enables POST /v1/ingest — chunk+embed+insert in one call)
VALORI_DIM=768 \
VALORI_EMBED_PROVIDER=ollama \
VALORI_EMBED_MODEL=nomic-embed-text \
VALORI_EMBED_URL=http://localhost:11434 \
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori.snap \
cargo run -p valori-node
```

The node listens on **`http://localhost:3000`** by default.

> **`VALORI_DIM` is immutable after the first insert.** Set it once and never change it for the same data directory.

Wipe and start fresh (persisted node):
```bash
rm -f /tmp/valori-events.log /tmp/valori.snap
```

For the full curl reference of every endpoint see [Section 13](#13-curl-reference--all-endpoints) below.

---

## 3. Python SDK

```bash
# Install the remote client (pure Python, no compilation needed)
pip install -e python/

python3 - <<'EOF'
from valoricore.remote import SyncRemoteClient
c = SyncRemoteClient("http://localhost:3000")
print(c.health())                       # ok
rid = c.insert([0.1]*8)
print(c.search([0.1]*8, k=3))
EOF
```

The **in-process `LocalClient`** (PyO3 FFI, no server needed) requires compiling the native extension:

```bash
pip install maturin
cd crates/valori-ffi
maturin develop        # compiles and installs into your active Python env
```

---

## 4. Web dashboard (Next.js UI)

```bash
# Node.js 20+ required (pinned in ui/package.json engines field)
cd ui
npm ci           # installs exact versions from package-lock.json — do not use npm install
npm run dev      # starts at http://localhost:3001
```

The UI reverse-proxies API calls to the node, so **start the node first** (step 2). No extra config needed for local dev.

For a production build:

```bash
npm run build
npm start
```

---

## 5. Local 3-node cluster

### Option A — Docker (easiest)

```bash
# Start 3 nodes + bootstrap automatically
docker compose -f docker-compose.cluster.yml up -d

# Wait ~3 seconds for the leader election, then check
curl http://localhost:3001/health
curl http://localhost:3001/v1/cluster/status | jq .
```

Tear down and wipe volumes:

```bash
docker compose -f docker-compose.cluster.yml down -v
```

### Option B — Raw `cargo run` (no Docker)

Open **3 separate terminal tabs** in the repo root.

**Tab 1 — node 1 (bootstrap leader)**
```bash
VALORI_NODE_ID=1 \
VALORI_CLUSTER_INIT=1 \
VALORI_CLUSTER_MEMBERS="1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003" \
VALORI_BIND=0.0.0.0:3001 \
VALORI_RAFT_BIND=0.0.0.0:3101 \
VALORI_RAFT_LOG_PATH=/tmp/valori-n1.redb \
VALORI_EVENT_LOG_PATH=/tmp/valori-n1-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori-n1.snap \
VALORI_DIM=128 \
cargo run -p valori-node
```

**Tab 2 — node 2**
```bash
VALORI_NODE_ID=2 \
VALORI_CLUSTER_MEMBERS="1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003" \
VALORI_BIND=0.0.0.0:3002 \
VALORI_RAFT_BIND=0.0.0.0:3102 \
VALORI_RAFT_LOG_PATH=/tmp/valori-n2.redb \
VALORI_EVENT_LOG_PATH=/tmp/valori-n2-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori-n2.snap \
VALORI_DIM=128 \
cargo run -p valori-node
```

**Tab 3 — node 3**
```bash
VALORI_NODE_ID=3 \
VALORI_CLUSTER_MEMBERS="1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003" \
VALORI_BIND=0.0.0.0:3003 \
VALORI_RAFT_BIND=0.0.0.0:3103 \
VALORI_RAFT_LOG_PATH=/tmp/valori-n3.redb \
VALORI_EVENT_LOG_PATH=/tmp/valori-n3-events.log \
VALORI_SNAPSHOT_PATH=/tmp/valori-n3.snap \
VALORI_DIM=128 \
cargo run -p valori-node
```

**Start node 1 first**, wait for `"Raft initialized"` in its log, then start nodes 2 and 3.

**Verify from any terminal:**
```bash
curl http://localhost:3001/health
curl http://localhost:3001/v1/cluster/status | jq .
```

**Port layout:**

| Node | HTTP | Raft gRPC |
|---|---|---|
| node-1 | 3001 | 3101 |
| node-2 | 3002 | 3102 |
| node-3 | 3003 | 3103 |

**Wipe and restart cleanly:**
```bash
rm -f /tmp/valori-n{1,2,3}.{redb,snap} /tmp/valori-n{1,2,3}-events.log
```

---

## 6. Key environment variables

### Standalone node

| Variable | Default | What it does |
|---|---|---|
| `VALORI_DIM` | `128` | Vector dimension — immutable after first insert |
| `VALORI_BIND` | `0.0.0.0:3000` | HTTP listen address |
| `VALORI_EVENT_LOG_PATH` | _(none)_ | WAL path; omit for in-memory only |
| `VALORI_SNAPSHOT_PATH` | _(none)_ | Snapshot file path |
| `VALORI_AUTH_TOKEN` | _(none)_ | Bearer token for all endpoints; omit to disable auth |
| `VALORI_INDEX` | `brute` | `brute` or `hnsw` |
| `VALORI_MAX_RECORDS` | `1000000` | Record slab capacity |
| `VALORI_EMBED_PROVIDER` | _(none)_ | `ollama` / `openai` / `custom`; enables `POST /v1/ingest` |
| `VALORI_EMBED_MODEL` | provider default | e.g. `nomic-embed-text`, `text-embedding-3-small` |
| `VALORI_EMBED_URL` | provider default | e.g. `http://localhost:11434` for Ollama |
| `VALORI_EMBED_API_KEY` | _(none)_ | API key for OpenAI / custom providers |
| `VALORI_DECAY_HALF_LIFE_SECS` | _(none)_ | Default recency decay half-life for search |

### Cluster additions

| Variable | What it does |
|---|---|
| `VALORI_NODE_ID` | Integer ID for this node (1, 2, 3, …) |
| `VALORI_CLUSTER_MEMBERS` | Full topology: `1=host:3100/host:3000,2=…` |
| `VALORI_CLUSTER_INIT` | Set to `1` on the **bootstrap node only**, first boot only |
| `VALORI_RAFT_LOG_PATH` | Persistent redb file for Raft log and vote |
| `VALORI_RAFT_BIND` | gRPC listen address (default `0.0.0.0:3100`) |

Copy `.env.example` → `.env` and fill in values for your deployment.

---

## 7. Ingest pipeline (optional — needs an embed provider)

With an embed provider configured you can POST raw text and let the node chunk + embed + insert:

```bash
VALORI_DIM=768 \
VALORI_EMBED_PROVIDER=ollama \
VALORI_EMBED_MODEL=nomic-embed-text \
VALORI_EMBED_URL=http://localhost:11434 \
  cargo run -p valori-node
```

```bash
curl -s -X POST http://localhost:3000/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"text": "Your document text here...", "source": "my-doc.pdf"}' | jq .
```

---

## 8. MCP server (Claude Desktop integration)

```bash
cargo build --release -p valori-mcp

# Add to ~/Library/Application Support/Claude/claude_desktop_config.json
# (see examples/claude_desktop_config.json for the full snippet)
VALORI_URL=http://localhost:3000 ./target/release/valori-mcp
```

---

## 9. Architecture rules — read before touching code

These are enforced invariants, not style preferences:

1. **`valori-kernel` is `no_std`** — never add `use std::` inside `crates/valori-kernel/src/`. Use `core::` or `alloc::` instead. After any kernel change, verify:
   ```bash
   cargo build -p valori-kernel --target wasm32-unknown-unknown
   ```

2. **Every new HTTP endpoint goes in BOTH `server.rs` AND `cluster_server.rs`** — standalone-only endpoints silently 404 in cluster mode. No compile error catches this.

3. **No floats in the kernel hot path** — all vector ops use `FxpScalar` (Q16.16, backed by `i32`). `f32`/`f64` only appear in the HTTP JSON layer for I/O.

4. **Apply → then audit** — `DEDUP CHECK → KERNEL APPLY → AUDIT WRITE`. Never write a BLAKE3 audit entry for a rejected or duplicate event.

5. **Python SDK: always update both clients** — `SyncRemoteClient` AND `AsyncRemoteClient` in `python/valoricore/remote.py`. `ClusterClient`/`AsyncClusterClient` inherit via `**kwargs`, so you usually only touch the two base classes.

---

## 10. Running specific tests

```bash
# Full kernel test suite
cargo test -p valori-kernel

# Full node test suite
cargo test -p valori-node

# One test with stdout
cargo test -p valori-node test_collections_isolation -- --nocapture

# Verify kernel stays wasm-compatible after your change
cargo build -p valori-kernel --target wasm32-unknown-unknown

# Python SDK tests
cd python && python -m pytest tests/ -v
```

---

## 11. Project layout at a glance

```
crates/
  valori-kernel/      # no_std deterministic core: vector store, graph, BLAKE3 audit chain
  valori-consensus/   # Raft state machine (openraft 0.9 + tonic/gRPC)
  valori-node/        # axum HTTP server + cluster orchestration + metadata store
  valori-cli/         # `valori` binary: setup wizard, import, verify, timeline
  valori-mcp/         # Model Context Protocol server for Claude Desktop
  valori-verify/      # Standalone offline BLAKE3 verifier binary
  valori-ffi/         # PyO3 bindings — in-process Python SDK
  valori-wire/        # Shared serde types (node ↔ SDK ↔ CLI)
python/valoricore/    # Python SDK: SyncRemoteClient, AsyncRemoteClient, local.py (FFI)
ui/                   # Next.js dashboard
examples/             # Quickstart scripts (LangChain, LlamaIndex, MCP, cluster)
deploy/               # Kubernetes Helm chart, Terraform modules
docs/                 # Architecture docs, API reference, phase build history
```

---

## 12. PR checklist

Before opening a pull request:

- [ ] `cargo test -p valori-kernel -p valori-node` passes
- [ ] If you touched `valori-kernel`: `cargo build -p valori-kernel --target wasm32-unknown-unknown` passes
- [ ] New endpoint added to **both** `server.rs` and `cluster_server.rs`
- [ ] Python SDK updated (both `SyncRemoteClient` and `AsyncRemoteClient`) if the API surface changed
- [ ] UI: no hardcoded dark colors — use semantic CSS tokens (`--background`, `--foreground`, `--border`, `--v-accent`, etc.)
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] If a new phase: phase doc created in `docs/phases/` and status table in `docs/phases/README.md` updated

---

## 13. Curl reference — all endpoints

All examples assume the node is running on `http://localhost:3000`.
If you set `VALORI_AUTH_TOKEN`, add `-H "Authorization: Bearer <token>"` to every request.

### Health & info

```bash
# Health check
curl http://localhost:3000/health
# → "ok"

# Version
curl http://localhost:3000/version

# Index config (brute or hnsw, current dim)
curl http://localhost:3000/v1/index/config | jq .

# Prometheus metrics
curl http://localhost:3000/metrics
```

### Collections (namespaces)

```bash
# List all collections
curl http://localhost:3000/v1/namespaces | jq .

# Create a collection
curl -s -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "my-collection"}' | jq .

# Drop a collection (deletes all records inside it)
curl -s -X DELETE http://localhost:3000/v1/namespaces/my-collection | jq .
```

### CRUD — Records

> **No update endpoint by design.** The kernel is an append-only audit chain. To replace a record, use `POST /v1/memory/consolidate` — it soft-deletes the old one, inserts the new vector, and commits a `Supersedes` edge to the BLAKE3 chain.

```bash
# CREATE — insert a single record (8-dim example — match your VALORI_DIM)
curl -s -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]}' | jq .
# → {"id": 0}

# CREATE — insert into a specific collection
curl -s -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "collection": "my-collection"}' | jq .

# CREATE — insert with text (indexed for BM25 hybrid reranking)
curl -s -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "text": "AdamW optimizer learning rate 3e-4"}' | jq .

# CREATE — batch insert
curl -s -X POST http://localhost:3000/v1/vectors/batch_insert \
  -H "Content-Type: application/json" \
  -d '{
    "vectors": [
      [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8],
      [0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2]
    ],
    "collection": "my-collection"
  }' | jq .

# READ — records have no direct GET-by-ID endpoint; use search or metadata
# Get metadata attached to a record
curl -s "http://localhost:3000/v1/memory/meta/get?target_id=rec:0" | jq .

# READ — find a record by similarity (nearest neighbour to itself = exact lookup)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 1}' | jq .

# UPDATE — replace a record (soft-delete old + insert new + Supersedes edge in audit chain)
curl -s -X POST http://localhost:3000/v1/memory/consolidate \
  -H "Content-Type: application/json" \
  -d '{"old_record_id": 0, "new_vector": [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]}' | jq .
# → {"old_record_id": 0, "new_record_id": 1, "supersedes_edge_id": 0, "state_hash": "..."}

# DELETE — soft-delete a record by id
curl -s -X POST http://localhost:3000/v1/delete \
  -H "Content-Type: application/json" \
  -d '{"id": 0}' | jq .
# → {"success": true}
```

### CRUD — Collections

```bash
# CREATE
curl -s -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "my-collection"}' | jq .
# → {"name": "my-collection"}

# READ — list all collections
curl -s http://localhost:3000/v1/namespaces | jq .
# → ["default", "my-collection"]

# UPDATE — no rename; drop and recreate instead

# DELETE — drops the collection and all records inside it
curl -s -X DELETE http://localhost:3000/v1/namespaces/my-collection | jq .
# → {"success": true}
```

### CRUD — Graph nodes

```bash
# CREATE
curl -s -X POST http://localhost:3000/graph/node \
  -H "Content-Type: application/json" \
  -d '{"record_id": 0, "kind": 1, "collection": "my-collection"}' | jq .
# → {"node_id": 0}
# kind: 0=Document, 1=Chunk

# READ — get one node
curl -s http://localhost:3000/graph/node/0 | jq .
# → {"kind": 1, "record_id": 0, "namespace_id": 0}

# READ — list all nodes in a collection
curl -s "http://localhost:3000/graph/nodes?collection=my-collection" | jq .

# UPDATE — no in-place update; delete and recreate

# DELETE
curl -s -X DELETE http://localhost:3000/graph/node/0 | jq .
# → {"success": true}
```

### CRUD — Graph edges

```bash
# CREATE
curl -s -X POST http://localhost:3000/graph/edge \
  -H "Content-Type: application/json" \
  -d '{"from": 0, "to": 1, "kind": 0}' | jq .
# → {"edge_id": 0}
# kind: 0=ParentOf, 1=Supersedes, 2=Contradicts

# READ — get all outgoing edges from a node
curl -s http://localhost:3000/graph/edges/0 | jq .
# → {"edges": [{"edge_id": 0, "to_node": 1, "kind": 0}]}

# READ — get subgraph (node + all reachable nodes up to depth)
curl -s "http://localhost:3000/graph/subgraph?root=0&depth=2" | jq .

# UPDATE — no in-place update

# DELETE — no individual edge delete; delete the source node to remove its edges
```

### CRUD — Metadata

```bash
# CREATE / UPDATE — upsert metadata for any target id
curl -s -X POST http://localhost:3000/v1/memory/meta/set \
  -H "Content-Type: application/json" \
  -d '{"target_id": "rec:0", "metadata": {"author": "Alice", "year": 2024, "tags": ["ml"]}}' | jq .
# → {"success": true}
# target_id format: "rec:<record_id>", "document:<node_id>", or any string key

# READ
curl -s "http://localhost:3000/v1/memory/meta/get?target_id=rec:0" | jq .
# → {"target_id": "rec:0", "metadata": {"author": "Alice", "year": 2024}}

# UPDATE — same as CREATE (upsert, overwrites existing value)

# DELETE — set metadata to null to clear it
curl -s -X POST http://localhost:3000/v1/memory/meta/set \
  -H "Content-Type: application/json" \
  -d '{"target_id": "rec:0", "metadata": null}' | jq .
```

### Search

```bash
# Basic k-NN search (k=5)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5}' | jq .

# Search within a collection
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "collection": "my-collection"}' | jq .

# Hybrid rerank — vector top-K re-scored by term frequency (Valori Reranker)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "rerank": true, "query_text": "AdamW optimizer"}' | jq .

# Recency-aware search — older records decay in ranking (1-day half-life)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "decay_half_life_secs": 86400}' | jq .

# Metadata filter — exact match (all keys must match)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "metadata_filter": {"author": "Alice"}}' | jq .

# Metadata filter — range operator (gte, gt, lte, lt, eq)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "metadata_filter": {"year": {"gte": 2020}}}' | jq .

# Point-in-time search (replay to a past state hash)
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "as_of": "a3f1..."}' | jq .
```

### GraphRAG

```bash
# k-NN + connected subgraph in one call
curl -s -X POST http://localhost:3000/v1/graphrag \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5, "depth": 2}' | jq .

# Subgraph around a known node (depth 2)
curl -s "http://localhost:3000/graph/subgraph?root=0&depth=2" | jq .
```

### Graph primitives

```bash
# Create a graph node (kind 0=Document, 1=Chunk)
curl -s -X POST http://localhost:3000/graph/node \
  -H "Content-Type: application/json" \
  -d '{"record_id": 0, "kind": 1}' | jq .

# Get a node by id
curl -s http://localhost:3000/graph/node/0 | jq .

# Delete a node
curl -s -X DELETE http://localhost:3000/graph/node/0 | jq .

# List all nodes (optionally scoped to a collection)
curl -s "http://localhost:3000/graph/nodes?collection=my-collection" | jq .

# Create an edge between two nodes (kind 0=ParentOf, 1=Supersedes, 2=Contradicts)
curl -s -X POST http://localhost:3000/graph/edge \
  -H "Content-Type: application/json" \
  -d '{"from": 0, "to": 1, "kind": 0}' | jq .

# Get all outgoing edges from a node
curl -s http://localhost:3000/graph/edges/0 | jq .
```

### Agent memory

```bash
# Upsert a memory vector (creates document + chunk nodes, optional metadata)
curl -s -X POST http://localhost:3000/v1/memory/upsert_vector \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "metadata": {"role": "note", "text": "reminder"}}' | jq .

# Search memory (returns memory_id, record_id, score, metadata)
curl -s -X POST http://localhost:3000/v1/memory/search_vector \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5}' | jq .

# Consolidate — supersede an old memory with a new vector
curl -s -X POST http://localhost:3000/v1/memory/consolidate \
  -H "Content-Type: application/json" \
  -d '{"old_record_id": 0, "new_vector": [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]}' | jq .

# Contradict — flag two memories as contradictory (if cosine ≥ threshold)
curl -s -X POST http://localhost:3000/v1/memory/contradict \
  -H "Content-Type: application/json" \
  -d '{"record_a": 0, "record_b": 1, "threshold": 0.9}' | jq .
```

### Metadata sidecar

```bash
# Set metadata for any target (record, document node, etc.)
curl -s -X POST http://localhost:3000/v1/memory/meta/set \
  -H "Content-Type: application/json" \
  -d '{"target_id": "rec:0", "metadata": {"author": "Alice", "year": 2024}}' | jq .

# Get metadata
curl -s "http://localhost:3000/v1/memory/meta/get?target_id=rec:0" | jq .
```

### Proof & audit

```bash
# Current BLAKE3 state hash
curl -s http://localhost:3000/v1/proof/state | jq .

# Full event-log proof (hash of the WAL file + committed height)
curl -s http://localhost:3000/v1/proof/event-log | jq .

# Audit timeline (all committed events)
curl -s http://localhost:3000/v1/timeline | jq .
```

### Ingest pipeline (requires VALORI_EMBED_PROVIDER)

```bash
# Chunk only — no embedding, returns chunks with titles
curl -s -X POST http://localhost:3000/v1/ingest/document \
  -H "Content-Type: application/json" \
  -d '{"text": "# Introduction\n\nThis is the intro.\n\n# Methods\n\nThis is methods.", "strategy": "auto"}' | jq .

# Full ingest — chunk + embed + insert + graph nodes (needs embed provider configured)
curl -s -X POST http://localhost:3000/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"text": "Your document text here...", "source": "my-doc.pdf", "collection": "research"}' | jq .
```

### Tree-RAG

```bash
# Build a tree index from markdown
curl -s -X POST http://localhost:3000/v1/tree/build \
  -H "Content-Type: application/json" \
  -d '{"text": "# Section 1\n\nContent here.\n\n# Section 2\n\nMore content.", "doc_name": "handbook"}' | jq .

# Query the tree (returns answer + breadcrumb citations + receipt)
curl -s -X POST http://localhost:3000/v1/tree/query \
  -H "Content-Type: application/json" \
  -d '{"tree": <tree-from-build>, "query": "what is in section 2?"}' | jq .

# Verify a receipt
curl -s -X POST http://localhost:3000/v1/tree/verify \
  -H "Content-Type: application/json" \
  -d '{"tree": <tree>, "receipt": <receipt-from-query>}' | jq .

# Hybrid tree + vector search
curl -s -X POST http://localhost:3000/v1/tree/hybrid \
  -H "Content-Type: application/json" \
  -d '{"query": "section 2 content", "doc_name": "handbook"}' | jq .
```

### Community detection (GraphRAG layer)

```bash
# Run label propagation to detect communities (must call before community_search)
curl -s -X POST http://localhost:3000/v1/community/detect \
  -H "Content-Type: application/json" \
  -d '{"namespace": "my-collection", "max_iter": 20}' | jq .

# Search communities by vector
curl -s -X POST http://localhost:3000/v1/community/search \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5}' | jq .

# Extract entities from text via LLM (requires VALORI_EMBED_PROVIDER)
curl -s -X POST http://localhost:3000/v1/ingest/extract-entities \
  -H "Content-Type: application/json" \
  -d '{"text": "Alice works at Acme Corp in New York.", "namespace": "my-collection"}' | jq .
```

### Snapshots

```bash
# Save a snapshot to the configured VALORI_SNAPSHOT_PATH
curl -s -X POST http://localhost:3000/v1/snapshot/save \
  -H "Content-Type: application/json" \
  -d '{}' | jq .

# Download the current snapshot as binary (pipe to file)
curl -s http://localhost:3000/v1/snapshot/download -o snapshot.bin

# Upload a snapshot to restore from (binary body)
curl -s -X POST http://localhost:3000/v1/snapshot/upload \
  --data-binary @snapshot.bin | jq .
```

### Crypto shredding (GDPR erasure)

```bash
# Insert an encrypted record (key_id ties it to a DEK)
curl -s -X POST http://localhost:3000/v1/records/encrypted \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "key_id": "user-123"}' | jq .

# Shred a DEK — O(1) erasure, audit chain stays intact
curl -s -X DELETE http://localhost:3000/v1/crypto/shred/user-123 | jq .

# Check shred status
curl -s http://localhost:3000/v1/crypto/status/user-123 | jq .
```

### Object store (requires VALORI_OBJECT_STORE_URL)

```bash
# Upload current snapshot to S3/MinIO/R2
curl -s -X POST http://localhost:3000/v1/storage/snapshots/upload | jq .

# List remote snapshots
curl -s http://localhost:3000/v1/storage/snapshots | jq .

# Restore from a remote snapshot
curl -s -X POST http://localhost:3000/v1/storage/snapshots/restore \
  -H "Content-Type: application/json" \
  -d '{"key": "snapshots/state-2024-01-15.snap"}' | jq .
```

### API key management

```bash
# Create an API key
curl -s -X POST http://localhost:3000/v1/keys \
  -H "Content-Type: application/json" \
  -d '{"name": "ci-runner", "scopes": ["read", "write"]}' | jq .

# List all keys
curl -s http://localhost:3000/v1/keys | jq .

# Revoke a key
curl -s -X DELETE http://localhost:3000/v1/keys/<key-id> | jq .
```

---

## Contribution philosophy

Valori's core guarantee is **bit-identical results on every architecture**. Contributions that trade determinism for raw performance will not be accepted. Everything else — new retrieval features, SDK improvements, deployment tooling, documentation — is welcome.

Open an issue first for changes that touch the kernel wire format, snapshot version, or BLAKE3 chain structure so the approach can be agreed before you write code.

**Contact:** varshith.gudur17@gmail.com
