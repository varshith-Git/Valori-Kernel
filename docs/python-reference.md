# Valori Python SDK Reference

The SDK ships two clients depending on your deployment mode:

| Client | Import | Use when |
|---|---|---|
| `MemoryClient` | `from valoricore import MemoryClient` | Embedded (local process, PyO3 FFI) |
| `SyncRemoteClient` | `from valoricore.remote import SyncRemoteClient` | HTTP server (standalone or cluster) |
| `AsyncRemoteClient` | `from valoricore.remote import AsyncRemoteClient` | Async HTTP (FastAPI, asyncio) |

---

## MemoryClient (embedded)

Opens or creates a local database directory. Wraps the Rust kernel in-process via PyO3.

```python
from valoricore import MemoryClient

db = MemoryClient(path="./my_db", dim=384, index_kind="hnsw")
```

**Constructor args:**

| Arg | Type | Default | Notes |
|---|---|---|---|
| `path` | `str` | required | Directory for WAL + snapshot |
| `dim` | `int` | required | Vector dimension (immutable after first insert) |
| `index_kind` | `str` | `"brute"` | `"brute"` or `"hnsw"` |

### Core methods

```python
# Insert via embedding function
result = db.add_document(text="...", embed=my_embed_fn)
# → {"document_node_id": 0, "record_ids": [0, 1, ...], "chunk_count": 3}

# Search via embedding function
hits = db.semantic_search("query text", embed=my_embed_fn, k=5)
# → [{"id": 0, "score": 0.0}, ...]

# Low-level insert (raw vector)
rid = db.insert_with_proof([0.1, 0.2, ...])
# → int (record id)

# Graph
node_id = db.create_node(kind=1, record_id=rid)
edge_id = db.create_edge(from_id=0, to_id=1, kind=0)

# State hash
h = db.get_state_hash()   # → 64-char hex string

# Event timeline
events = db.get_timeline()  # → list of dicts
```

---

## SyncRemoteClient (HTTP)

Connects to a running `valori-node`. All methods are synchronous.

```python
from valoricore.remote import SyncRemoteClient, ClusterClient

db = SyncRemoteClient("http://localhost:3000")
db = SyncRemoteClient("http://localhost:3000", token="your-auth-token")

# Multi-node cluster
cluster = ClusterClient(["http://node1:3000", "http://node2:3000", "http://node3:3000"])
cluster = ClusterClient([...], token="your-auth-token")
```

### Data operations

```python
# Insert
rid = db.insert([0.1, 0.2, 0.3])
rid = db.insert([0.1, 0.2, 0.3], text="content to index for reranking", tag=0)
rids = db.insert_batch([[0.1, ...], [0.2, ...]])

# Search
hits = db.search([0.1, 0.2, 0.3], k=5)
hits = db.search([0.1, 0.2, 0.3], k=5, query_text="my query")          # hybrid rerank
hits = db.search([0.1, 0.2, 0.3], k=5, decay_half_life_secs=86400)     # recency
hits = db.search([0.1, 0.2, 0.3], k=5, metadata_filter={"author": "Alice"})
hits = db.search([0.1, 0.2, 0.3], k=5, metadata_filter={"year": {"gte": 2020}})
hits = db.search([0.1, 0.2, 0.3], k=5, collection="my-collection", consistency="linearizable")
```

### Collections (multi-tenancy)

```python
db.create_collection("tenant-acme")          # → {"id": 1, "name": "tenant-acme", ...}
db.list_collections()                         # → [{"id": 0, "name": "default"}, ...]
db.drop_collection("tenant-acme")
```

### Graph

```python
node_id = db.create_node(kind=1, record_id=0)
edge_id = db.create_edge(from_id=0, to_id=1, kind=0)
result = db.graphrag([0.1, 0.2, 0.3], k=5, depth=2)
# → {"hits": [...], "seed_nodes": [...], "subgraph": {"nodes": [...], "edges": [...]}}
```

### Agent memory

```python
result = db.memory_upsert([0.1, 0.2, 0.3], metadata={"role": "note"})
# → {"memory_id": "...", "record_id": 0, "document_node_id": 1, "chunk_node_id": 2}

hits = db.memory_search([0.1, 0.2, 0.3], k=5, decay_half_life_secs=86400)
# → [{"memory_id": ..., "score": ..., "metadata": ..., "decay_factor": ...}]

db.consolidate(old_record_id=7, new_vector=[0.2, 0.3, 0.4])
db.contradict(record_a=3, record_b=9, threshold=0.9)
```

### Ingest pipeline (requires `VALORI_EMBED_PROVIDER`)

```python
result = db.ingest("Full document text...", source="paper.pdf", strategy="auto")
# → {"ok": True, "chunk_count": 31, "record_ids": [...], "document_node_id": 42}

chunks = db.chunk_document("Full document text...", strategy="tree")
# → {"strategy_used": "tree", "chunk_count": 31, "chunks": [...]}
```

### Tree-RAG

```python
built = db.tree_build(markdown_text, doc_name="handbook")
ans   = db.tree_query(built["tree"], "how many sick days?")
ok    = db.tree_verify(built["tree"], ans["receipt"])

result = db.tree_hybrid("query text", tree=built["tree"], k=5)
```

### Proof / audit

```python
db.get_state_hash()           # → hex string
db.get_proof()                # → {"final_state_hash": "..."}
db.event_log_proof()          # → {"event_log_hash": ..., "committed_height": ...}
```

### Health / cluster

```python
db.health()                   # → "ok"
db.get_cluster_status()       # → {...}
```

---

## AsyncRemoteClient

Same API as `SyncRemoteClient` but all methods are `async`:

```python
from valoricore.remote import AsyncRemoteClient
import asyncio

async def main():
    async with AsyncRemoteClient("http://localhost:3000") as db:
        rid = await db.insert([0.1, 0.2, 0.3])
        hits = await db.search([0.1, 0.2, 0.3], k=5)

asyncio.run(main())
```

---

## Embeddings helpers

```python
from valoricore.embeddings import SentenceTransformerEmbedder, CachedEmbedder

# Requires: pip install "valoricore[local]"
embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")  # dim=384
cached   = CachedEmbedder(embedder, maxsize=1000)

vector = embedder.embed("some text")   # → List[float]
```

---

## Exceptions

```python
from valoricore.exceptions import (
    ValoricoreError,       # base
    ConnectionError,       # node unreachable
    ValidationError,       # bad request (dim mismatch, missing fields)
    NotFoundError,         # record/collection not found
    NotLeaderError,        # cluster write redirected; .leader_url has the right address
)
```
