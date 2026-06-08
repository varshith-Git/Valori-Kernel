<div align="center">

<img src="https://img.shields.io/badge/Valoricore-v0.1.10-6c47ff?style=for-the-badge&logo=rust" alt="version"/>

# Valoricore

### The Official Python SDK for **Valori-Kernel**

*Deterministic Vector Memory · Cryptographic Audit Trails · Hybrid Knowledge Graphs*

<br/>

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://github.com/varshith-Git/Valori-Kernel/blob/main/LICENSE)
[![Python 3.8+](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/downloads/)
[![Rust Core](https://img.shields.io/badge/core-Rust%20%2Fno__std-orange.svg)](https://www.rust-lang.org/)
[![PyPI](https://img.shields.io/pypi/v/valoricore.svg)](https://pypi.org/project/valoricore/)
[![Build](https://img.shields.io/github/actions/workflow/status/varshith-Git/Valori-Kernel/ci.yml?branch=main)](https://github.com/varshith-Git/Valori-Kernel/actions)

</div>

---

`valoricore` is the official Python SDK for [**Valori-Kernel**](https://github.com/varshith-Git/Valori-Kernel) — a `no_std` Rust engine that unifies **Vector Memory** and **Knowledge Graphs** into a single, cryptographically auditable memory space.

Every insert, search, and graph edge is backed by **Q16.16 fixed-point arithmetic**, producing bit-identical results across x86, ARM, and RISC-V. The global state is always summarised in a single **BLAKE3 Merkle root** you can store, compare, and prove.

---

## What Makes Valoricore Different?

| Feature | Valoricore | Chroma / FAISS / Pinecone |
|---|---|---|
| **Results across hardware** | Bit-identical (Q16.16 fixed-point) | Float drift |
| **Cryptographic state proof** | BLAKE3 Merkle root per operation | None |
| **Hybrid Vector + Graph** | Native, same memory space | Graph is a separate system |
| **Offline proof verification** | No DB connection required | N/A |
| **Snapshot / replay** | Byte-exact restore | Partial / format-specific |
| **`no_std` embeddable core** | Runs on ARM Cortex-M4 | Heap-heavy |
| **Air-gapped deployment** | Local FFI, no cloud required | Varies |

---

## Installation

Valoricore ships with **pre-compiled Rust binaries** for Linux (x86-64, arm64), macOS (x86-64, Apple Silicon), and Windows. A Rust compiler is only required when building from source.

### Core (vector DB + knowledge graph)
```bash
pip install valoricore
```

### With local / offline embeddings
```bash
pip install "valoricore[local]"
```

### With cloud embedding providers
```bash
pip install "valoricore[openai]"
pip install "valoricore[cohere]"
```

### Full installation (all providers + LangChain + LlamaIndex)
```bash
pip install "valoricore[all]"
```

### Optional integrations
```bash
pip install "valoricore[langchain]"
pip install "valoricore[llamaindex]"
pip install "valoricore[pdf]"
```

---

## Quick Start

### Interactive Colab Notebooks
Test Valoricore in your browser with zero local setup:
- [**End-to-End Demo**](https://colab.research.google.com/drive/1QO1yQMQoGbp9fwrb00KVKTq5bYVGXgJv#scrollTo=hM-PiglYd20l) — Determinism, Knowledge Graph, Crypto Proofs
- [**LangChain Integration**](https://colab.research.google.com/drive/1HezK4l-Hbc6AdHxJNLwSqAgzr8WaKhiq#scrollTo=Hxcyq4OkN0MO)
- [**LlamaIndex Integration**](https://colab.research.google.com/drive/1Q72ANZxBm1fthNpgVW-FftS8sZz6uCr3#scrollTo=XHFOODSTVE6N)

### 1 · Embedded Local Engine (no server required)

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")   # dim=384

client = MemoryClient(path="./my_valori_db")

# Add a document — chunks, embeds, and links in the Knowledge Graph automatically
result = client.add_document(
    text  = "Valoricore is a deterministic Rust kernel that unifies "
            "vector memory and knowledge graphs.",
    embed = embedder,
    title = "Introduction",
)
print(f"Document Node ID : {result['document_node_id']}")
print(f"Chunk count      : {result['chunk_count']}")

# Semantic search
hits = client.semantic_search("What does Valoricore unify?", embed=embedder, k=3)
for h in hits:
    print(f"  id={h['id']}  score={h['score']}")

# Cryptographic state proof
print(f"State hash: {client.get_state_hash()}")
```

### 2 · Remote / Cluster Mode

```python
from valoricore import MemoryClient
from valoricore.embeddings import OpenAIEmbedder

embedder = OpenAIEmbedder()

# Identical API — only the constructor changes
client = MemoryClient(remote="http://my-valori-node:3000")

result = client.add_document(text="Remote deployment with full audit trail.", embed=embedder)
snap = client.snapshot()
with open("backup.snap", "wb") as f:
    f.write(snap)
```

### 3 · Async API (FastAPI / asyncio)

```python
import asyncio
from valoricore import AsyncMemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")

async def main():
    async with AsyncMemoryClient(path="./async_db") as client:
        result = await client.add_document(
            text  = "Non-blocking deterministic vector storage.",
            embed = embedder,
        )
        hits  = await client.semantic_search("Non-blocking search", embed=embedder, k=5)
        state = await client.get_state_hash()
        print(f"State: {state}")

asyncio.run(main())
```

---

## Embedding Providers

| Provider | Class | Offline? | Install |
|---|---|---|---|
| **SentenceTransformers** | `SentenceTransformerEmbedder` | Yes | `pip install "valoricore[local]"` |
| **OpenAI** | `OpenAIEmbedder` | No | `pip install "valoricore[openai]"` |
| **Cohere** | `CohereEmbedder` | No | `pip install "valoricore[cohere]"` |
| **HuggingFace Inference** | `HuggingFaceEmbedder` | No | *(requests, built-in)* |
| **Ollama** | `OllamaEmbedder` | Yes (local server) | `ollama pull nomic-embed-text` |
| **Dummy / CI** | `DummyEmbedder` | Yes | *(built-in)* |
| **Hash / CI** | `HashEmbedder` | Yes | *(built-in)* |

### Convenience Factory

```python
from valoricore.embeddings import get_embedder

embedder = get_embedder("local",       model_name="all-MiniLM-L6-v2")
embedder = get_embedder("openai",      api_key="sk-...")
embedder = get_embedder("ollama",      model="nomic-embed-text")
embedder = get_embedder("cohere",      api_key="...")
embedder = get_embedder("huggingface", api_key="hf_...", model="sentence-transformers/all-MiniLM-L6-v2")
embedder = get_embedder("dummy",       dim=384)   # CI / tests
```

### LRU Caching

```python
from valoricore.embeddings import SentenceTransformerEmbedder, CachedEmbedder

embedder = CachedEmbedder(SentenceTransformerEmbedder("BAAI/bge-small-en-v1.5"), max_size=5000)
```

### Async Embedder

```python
from valoricore.embeddings import SentenceTransformerEmbedder, AsyncEmbedder

async_embedder = AsyncEmbedder(SentenceTransformerEmbedder("all-MiniLM-L6-v2"))

async def pipeline():
    vec  = await async_embedder.embed("Hello")
    vecs = await async_embedder.embed_batch(["Hello", "World"])
```

---

## Core Concepts

### Records
A **Record** is a dense Q16.16 fixed-point vector stored in the kernel's `RecordPool`. Every insert returns an integer `record_id` and a BLAKE3 Merkle proof.

### Nodes & Edges (Knowledge Graph)
A **Node** is a named entity that optionally points to a Record. An **Edge** is a directed relationship between two Nodes. Both live in the same memory space as the vector pool — no separate database.

### Node Kinds

```python
from valoricore import (
    NODE_RECORD,    # 0 – raw vector record
    NODE_CONCEPT,   # 1 – abstract concept
    NODE_AGENT,     # 2 – AI agent / process
    NODE_USER,      # 3 – human user
    NODE_TOOL,      # 4 – tool or function
    NODE_DOCUMENT,  # 5 – top-level document
    NODE_CHUNK,     # 6 – text chunk (child of document)
)
```

### Edge Kinds

```python
from valoricore import (
    EDGE_RELATION,   # 0 – generic relation
    EDGE_FOLLOWS,    # 1 – sequential ordering
    EDGE_MENTIONS,   # 4 – entity mention
    EDGE_REFERS_TO,  # 5 – cross-reference
    EDGE_PARENT_OF,  # 6 – hierarchical parent→child
)
```

---

## Step-by-Step Usage Guide

### Step 1 — Initialize

```python
from valoricore import MemoryClient

# Local embedded (no server needed)
client = MemoryClient(
    path       = "./my_db",
    index_kind = "hnsw",        # "bruteforce" (default), "hnsw", or "ivf"
)

# Remote cluster
# client = MemoryClient(remote="http://my-node:3000")
```

### Step 2 — Ingest Documents

```python
# From a string
result = client.add_document(
    text       = open("report.txt").read(),
    embed      = embedder,
    title      = "Q4 Report",
    chunk_size = 512,
)

# From a PDF (requires: pip install "valoricore[pdf]")
from valoricore import load_text_from_file
result = client.add_document(text=load_text_from_file("report.pdf"), embed=embedder)

# Insert a raw pre-computed vector
result = client.upsert_vector(vector=[0.1, 0.2, ...])
```

### Step 3 — Batch Insert

```python
# Batch insert (high-throughput)
vectors = [[0.1] * 384, [0.2] * 384, [0.3] * 384]
ids = client.insert_batch(vectors)

# Batch insert with cryptographic proofs
results = client.insert_batch_with_proof(vectors, tags=[1, 2, 3])
for record_id, proof_bytes in results:
    print(f"id={record_id}  proof={proof_bytes.hex()[:16]}...")
```

### Step 4 — Semantic Search

```python
hits = client.semantic_search(
    query = "What is deterministic AI memory?",
    embed = embedder,
    k     = 10,
)

for hit in hits:
    print(f"Record ID : {hit['id']}")
    print(f"L2 Score  : {hit['score']}")   # lower = closer
```

### Step 5 — Tag-Filtered Search

```python
# Insert with tags to segment by tenant, user, or document type
client._db.insert([0.1] * 384, tag=42)

# Search within a specific tag only — O(1) overhead, 100% accuracy
hits = client._db.search([0.1] * 384, k=5, filter_tag=42)
```

### Step 6 — Knowledge Graph

```python
from valoricore import NODE_AGENT, NODE_DOCUMENT, EDGE_BY_AGENT

record_id  = client._db.insert([0.5] * 384)
agent_node = client.create_node(kind=NODE_AGENT)
doc_node   = client.create_node(kind=NODE_DOCUMENT, record_id=record_id)

client.create_edge(from_id=agent_node, to_id=doc_node, kind=EDGE_BY_AGENT)

print(client.get_node(doc_node))       # {"kind": 5, "record_id": 0}
print(client.get_edges(agent_node))    # [{"edge_id": 0, "to_node": 1, "kind": 3}]

# BFS traversal up to depth 2
visited_nodes = client.walk(agent_node, max_depth=2)

# All record_ids reachable from a starting node
record_ids = client.expand(agent_node, max_depth=2)
```

### Step 7 — Metadata

```python
import json

# Attach arbitrary metadata to a record (max 64 KB)
client.set_metadata(record_id=0, metadata=json.dumps({"source": "report.pdf", "page": 3}).encode())

# Retrieve it
raw = client.get_metadata(record_id=0)
meta = json.loads(raw)
print(meta["source"])   # "report.pdf"
```

### Step 8 — Lifecycle

```python
# Permanently remove record from pool and search index
client.delete(record_id=0)

# Soft delete: deactivates the record but preserves the pool slot for reuse.
# The record will no longer appear in search results.
# The state hash changes to reflect the deletion.
client.soft_delete(record_id=1)

print(f"Active records: {client.record_count()}")
```

### Step 9 — Snapshot, Restore, and Audit

```python
# Snapshot full kernel state to bytes
snap = client.snapshot()
with open("state.snap", "wb") as f:
    f.write(snap)

# Restore to a fresh engine — bit-exact
fresh = MemoryClient(path="./restored_db")
fresh.restore(snap)

assert fresh.get_state_hash() == client.get_state_hash()
print("Bit-exact restore verified")

# Full event timeline (append-only, human-readable)
for event in client.get_timeline():
    print(event)
```

### Step 10 — Cryptographic Proof Verification (Offline)

```python
from valoricore import ingest_embedding, generate_proof, verify_embedding

my_vector = [0.1] * 384

# Generate a standalone proof — no DB connection required
fixed_values = ingest_embedding(my_vector)   # float → Q16.16
proof_hex    = generate_proof(fixed_values)  # BLAKE3 Merkle node

# Verify on any machine, any time
is_valid = verify_embedding(floats=my_vector, claimed_hash=proof_hex)
print(f"Proof valid: {is_valid}")   # True
```

---

## Framework Integrations

Both adapters live in `valoricore.integrations` — a single import, no adapter boilerplate,
works in **local embedded** and **remote HTTP** modes without changing any code.

### LangChain

```bash
pip install "valoricore[langchain]"
```

**Local embedded (no server needed):**

```python
from valoricore.integrations import ValoricoreLangChain
from langchain_openai import OpenAIEmbeddings

store = ValoricoreLangChain(
    path       = "./my_db",
    embedding  = OpenAIEmbeddings(),
    index_kind = "hnsw",          # "bruteforce" | "hnsw" | "ivf"
)

# Add texts — batch embedded + batch inserted in one call
store.add_texts(
    texts     = ["Valoricore is deterministic.", "Fixed-point arithmetic rocks."],
    metadatas = [{"source": "intro"}, {"source": "math"}],
)

# Similarity search
docs = store.similarity_search("What is deterministic AI?", k=3)
for doc in docs:
    print(doc.page_content, doc.metadata)

# With distance scores (lower = closer)
pairs = store.similarity_search_with_score("fixed-point", k=3)

# Pre-computed vector search
docs = store.similarity_search_by_vector(my_embedding, k=3)

# Cryptographic audit hash
print(store.get_state_hash())   # 64-char BLAKE3 hex, survives crash recovery
```

**Remote HTTP node:**

```python
store = ValoricoreLangChain(
    remote    = "http://my-valori-node:3000",
    embedding = OpenAIEmbeddings(),
)
```

**From documents (standard LangChain factory pattern):**

```python
from langchain.document_loaders import PyPDFLoader

docs  = PyPDFLoader("report.pdf").load()
store = ValoricoreLangChain.from_documents(docs, OpenAIEmbeddings(), path="./db")
```

**As a retriever in a RAG chain:**

```python
from langchain.chains import RetrievalQA
from langchain_openai import ChatOpenAI

# k and filter_tag are optional
retriever = store.as_retriever(k=5, filter_tag=tenant_id)

chain = RetrievalQA.from_chain_type(
    llm       = ChatOpenAI(),
    retriever = retriever,
)
answer = chain.run("What is deterministic AI memory?")
```

**Tag-filtered search (tenant isolation):**

```python
# Insert records tagged by tenant
store.add_texts(["tenant A doc"], metadatas=[{"tenant": "A"}])

# Search only within a tag — O(1) overhead, 100% accuracy
docs = store.similarity_search("query", k=5, filter_tag=42)
```

---

### LlamaIndex

```bash
pip install "valoricore[llamaindex]"
```

**Local embedded:**

```python
from llama_index.core import VectorStoreIndex, StorageContext
from llama_index.core.node_parser import SentenceSplitter
from llama_index.embeddings.openai import OpenAIEmbedding
from valoricore.integrations import ValoricoreLlamaIndex

embed_model  = OpenAIEmbedding()
vector_store = ValoricoreLlamaIndex(
    path       = "./my_db",
    index_kind = "hnsw",    # "bruteforce" | "hnsw" | "ivf"
)

storage_ctx = StorageContext.from_defaults(vector_store=vector_store)
index       = VectorStoreIndex.from_documents(
    documents,
    storage_context = storage_ctx,
    embed_model     = embed_model,
    transformations = [SentenceSplitter(chunk_size=512)],
)

# Query
engine   = index.as_query_engine()
response = engine.query("What is deterministic AI memory?")
print(response)
```

**Remote HTTP node:**

```python
vector_store = ValoricoreLlamaIndex(remote="http://my-valori-node:3000")
```

**Similarity score semantics:**

LlamaIndex expects similarity in `(0, 1]` where `1 = identical`. Valoricore converts
its raw Q16.16² L2 distance automatically: `similarity = 1 / (1 + distance)`.

**Audit hash:**

```python
print(vector_store.get_state_hash())   # 64-char BLAKE3 hex
snap = vector_store.snapshot()         # full kernel state as bytes
vector_store.restore(snap)             # bit-exact restore
```

---

## Error Handling

```python
from valoricore import (
    ValoricoreError,   # base — catch all SDK errors
    ValidationError,   # bad vector dimension / FXP out-of-range
    ConnectionError,   # remote node unreachable
    IntegrityError,    # BLAKE3 proof mismatch
    NotFoundError,     # record / node / edge doesn't exist
    KernelError,       # unrecoverable Rust kernel error
)

try:
    client.delete(record_id=9999)
except NotFoundError:
    print("Record does not exist")

try:
    client.upsert_vector([0.1] * 128)   # wrong dimension
except ValidationError as e:
    print(f"Bad embedding: {e}")

try:
    MemoryClient(remote="http://offline-node:3000").snapshot()
except ConnectionError as e:
    print(f"Node unreachable: {e}")
```

---

## Performance

| Operation | Local FFI | Remote HTTP |
|---|---|---|
| Single insert | ~20 µs | ~0.5 ms |
| Batch insert (1 k vectors) | ~15 ms | ~50 ms |
| L2 search (10 k × 384) | ~8 ms | ~10 ms |
| L2 search (100 k × 384) | ~80 ms | ~90 ms |
| Graph BFS (depth 2, 50 nodes) | ~0.5 ms | ~2 ms |
| State hash (BLAKE3) | < 1 µs | ~1 ms |
| Snapshot (10 k records) | ~5 ms | ~20 ms |

*Benchmarked on Apple M2. The local FFI path calls Rust directly with zero serialization overhead.*

> **Note:** Safe input range for embedding values is **[-32767.0, 32767.0]**. Standard normalized embeddings (OpenAI, SentenceTransformers) are always in [-1.0, 1.0] and are safe.

---

## Configuration Reference

### `MemoryClient` / `AsyncMemoryClient`

| Parameter | Type | Default | Description |
|---|---|---|---|
| `path` | `str` | `"./valori_db"` | Local database directory |
| `remote` | `str \| None` | `None` | Remote node URL. When set, `path` is ignored |
| `index_kind` | `str` | `"bruteforce"` | Vector index: `"bruteforce"`, `"hnsw"`, or `"ivf"` |
| `quantization` | `str` | `"none"` | Quantization: `"none"`, `"scalar"`, or `"product"` |

### `Valoricore` / `AsyncValoricore` factory

| Parameter | Type | Default | Description |
|---|---|---|---|
| `path` | `str` | `"./valori_db"` | Local database directory |
| `remote` | `str \| None` | `None` | Remote node URL |
| `index_kind` | `str` | `"bruteforce"` | Vector index backend |

### Environment variables (server mode)

| Variable | Default | Description |
|---|---|---|
| `VALORI_MAX_RECORDS` | `1024` | Soft record limit |
| `VALORI_DIM` | `16` | Embedding dimension |
| `VALORI_INDEX` | `bruteforce` | `bruteforce`, `hnsw`, or `ivf` |
| `VALORI_QUANT` | *(none)* | `scalar` or `product` |
| `VALORI_SNAPSHOT_PATH` | *(none)* | Path to write snapshots |
| `VALORI_WAL_PATH` | *(none)* | Path to write WAL |
| `VALORI_EVENT_LOG_PATH` | *(none)* | Path to write event log |
| `VALORI_AUTH_TOKEN` | *(none)* | Bearer token for HTTP API |
| `VALORI_FOLLOWER_OF` | *(none)* | Leader URL (enables follower mode) |

---

## API Reference

### `MemoryClient`

#### Ingestion
| Method | Description |
|---|---|
| `add_document(text, embed, title, doc_id, chunk_size)` | Chunk, embed, and store a document with Knowledge Graph links |
| `add_chunks(chunks, embed, parent_document_node, title)` | Lower-level chunked ingestion |
| `upsert_vector(vector, attach_to_document_node)` | Insert a raw pre-computed vector |
| `insert_batch(vectors)` | Batch insert multiple raw vectors |
| `insert_batch_with_proof(vectors, tags)` | Batch insert with per-record BLAKE3 proofs |

#### Search
| Method | Description |
|---|---|
| `semantic_search(query, embed, k)` | Embed query string and return nearest neighbours |

#### Lifecycle
| Method | Description |
|---|---|
| `delete(record_id)` | Permanently remove record from pool and index |
| `soft_delete(record_id)` | Deactivate record; slot preserved for reuse; state hash updated |
| `record_count()` | Total active records |

#### Metadata
| Method | Description |
|---|---|
| `get_metadata(record_id)` | Retrieve raw binary metadata for a record |
| `set_metadata(record_id, metadata)` | Attach up to 64 KB of binary metadata to a record |

#### Persistence & Audit
| Method | Description |
|---|---|
| `snapshot()` | Serialize full kernel state to bytes |
| `restore(data)` | Replace current state with a snapshot |
| `get_state_hash()` | 64-char BLAKE3 hex digest of the entire kernel state |
| `get_timeline()` | Chronological list of all state transitions from the event log |

#### Knowledge Graph
| Method | Description |
|---|---|
| `create_node(kind, record_id)` | Create a graph node |
| `create_edge(from_id, to_id, kind)` | Create a directed edge |
| `get_node(node_id)` | Fetch node kind and attached record_id |
| `get_edges(node_id)` | Fetch all outgoing edges |
| `walk(start_node, max_depth)` | BFS traversal; returns visited node IDs |
| `expand(start_node, max_depth)` | BFS traversal; returns reachable record IDs |

---

## Module-Level Cryptographic Helpers

```python
from valoricore import ingest_embedding, generate_proof, verify_embedding

fixed = ingest_embedding([0.1, 0.2, 0.3])   # List[float] → List[int] (Q16.16)
proof = generate_proof(fixed)               # → hex string (BLAKE3 Merkle root)
ok    = verify_embedding([0.1, 0.2, 0.3], proof)  # → bool
```

These functions are implemented in Rust (via PyO3) and work offline — no running engine required.

---

## License

AGPL-3.0 — see [LICENSE](https://github.com/varshith-Git/Valori-Kernel/blob/main/LICENSE).

Commercial licensing available for proprietary deployments. Contact: varshith.gudur17@gmail.com
