# Getting Started with Valori

Zero to running in under 10 minutes.

## Prerequisites

- **Python 3.9+**
- **Rust 1.80+** (only for building from source; skip if you only use the remote SDK)

---

## Pick your path

### Path A — Embedded (no server, local process)

Install the SDK with local embedding support:

```bash
pip install "valoricore[local]"
```

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")  # downloads ~90 MB once
db = MemoryClient(path="./my_db", dim=384)

db.add_document(text="The patient presented with hypertension.", embed=embedder)
hits = db.semantic_search("blood pressure", embed=embedder, k=5)
print(db.get_state_hash())   # 64-char BLAKE3 hex — reproducible on any machine
```

`MemoryClient` opens (or creates) a local database directory. The state hash is identical
on every machine that applied the same events in the same order.

---

### Path B — HTTP server (standalone node)

Build and start the server:

```bash
VALORI_DIM=128 cargo run --release -p valori-node
# Listening on http://0.0.0.0:3000
```

Connect with the remote SDK:

```bash
pip install valoricore
```

```python
from valoricore.remote import SyncRemoteClient

db = SyncRemoteClient("http://localhost:3000")
print(db.health())           # → "ok"

db.insert([0.1, 0.2, 0.3])  # vector length must equal VALORI_DIM
hits = db.search([0.1, 0.2, 0.3], k=5)
print(db.get_state_hash())
```

The node is in-memory by default. Enable durability:

```bash
VALORI_DIM=128 \
VALORI_EVENT_LOG_PATH=./data/events.log \
VALORI_SNAPSHOT_PATH=./data/snapshot.bin \
  cargo run --release -p valori-node
```

---

### Path C — One-call ingest (chunk + embed on-node)

Start the node with an embedding provider so clients can POST raw text:

```bash
VALORI_DIM=768 \
VALORI_EMBED_PROVIDER=ollama \
VALORI_EMBED_MODEL=nomic-embed-text \
VALORI_EMBED_URL=http://localhost:11434 \
  cargo run --release -p valori-node
```

```python
from valoricore.remote import SyncRemoteClient

db = SyncRemoteClient("http://localhost:3000")
result = db.ingest("Full text of a research paper...", source="paper.pdf")
print(f"{result['chunk_count']} chunks — doc node {result['document_node_id']}")
```

---

## Interactive setup wizard

The `valori` CLI walks you through single-node and cluster setup:

```bash
cargo install --path crates/valori-cli
valori setup
```

---

## Next steps

- [Core concepts](./core-concepts.md) — determinism, fixed-point math, snapshots
- [Cluster setup](./CLUSTER.md) — 3/5-node Raft with `docker compose` or raw terminals
- [Python SDK reference](./python-reference.md) — all 40 SDK methods
- [API reference](./api-reference.md) — HTTP endpoints
- [MCP agent memory](../crates/valori-mcp/README.md) — Claude Desktop integration
