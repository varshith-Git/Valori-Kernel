<div align="center">

<img src="https://img.shields.io/badge/Valoricore-v0.1.2-6c47ff?style=for-the-badge&logo=rust" alt="version"/>

# Valoricore 🛡️

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

Every insert, search, and graph edge is backed by **fixed-point Q16.16 arithmetic**, producing bit-identical results across x86, ARM, and RISC-V. The global state is always summarised in a single **BLAKE3 Merkle root** you can store, compare, and prove.

---

## ✨ What Makes Valoricore Different?

| Feature | Valoricore | Chroma / FAISS / Pinecone |
|---|---|---|
| **Results across hardware** | ✅ Bit-identical (Q16.16 fixed-point) | ❌ Float drift |
| **Cryptographic state proof** | ✅ BLAKE3 Merkle root per operation | ❌ None |
| **Hybrid Vector + Graph** | ✅ Native, same memory space | ⚠️ Graph is separate system |
| **Offline proof verification** | ✅ No DB connection required | ❌ N/A |
| **Snapshot / replay** | ✅ Byte-exact restore | ⚠️ Partial / format-specific |
| **`no_std` embeddable core** | ✅ Zero heap allocation in kernel | ❌ Heap-heavy |
| **Air-gapped deployment** | ✅ Local FFI, no cloud required | ⚠️ Varies |

---

## 📦 Installation

Valoricore ships with **pre-compiled Rust binaries** for Linux (x86-64, arm64), macOS (x86-64, Apple Silicon), and Windows. A Rust compiler is **only** required when building from source.

### Core (vector DB + knowledge graph)
```bash
pip install valoricore
```

### With local / offline embeddings (no API key needed)
```bash
pip install "valoricore[local]"
# Uses sentence-transformers + PyTorch
```

### With cloud embedding providers
```bash
pip install "valoricore[openai]"    # OpenAI text-embedding-3-*
pip install "valoricore[cohere]"    # Cohere embed-english-v3.0
```

### Full installation (all providers + LangChain + LlamaIndex)
```bash
pip install "valoricore[all]"
```

### Optional integrations
```bash
pip install "valoricore[langchain]"    # LangChain VectorStore + Retriever
pip install "valoricore[llamaindex]"   # LlamaIndex VectorStore
pip install "valoricore[pdf]"          # PDF document ingestion (pypdf)
```

---

## 🚀 Quick Start

### 1 · Embedded Local Engine (no server required)

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

# ① Load a local model (downloads once, runs fully offline after that)
embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")   # dim=384

# ② Initialize the embedded Rust engine
client = MemoryClient(path="./my_valori_db")

# ③ Add a document — automatically chunks, embeds, and links in the Knowledge Graph
result = client.add_document(
    text  = "Valoricore is a deterministic, no_std Rust kernel "
            "that unifies vector memory and knowledge graphs.",
    embed = embedder,
    title = "Introduction",
)
print(f"Document Node ID : {result['document_node_id']}")
print(f"Chunk count      : {result['chunk_count']}")
print(f"Proof hashes     : {result['proof_hashes']}")

# ④ Semantic search
hits = client.semantic_search("What does Valoricore unify?", embed=embedder, k=3)
for h in hits:
    print(f"  id={h['id']}  l2_score={h['score']}")

# ⑤ Cryptographic state proof
print(f"\nDatabase state : {client.get_state_hash()}")
```

---

### 2 · Remote / Cluster Mode

Connect to a standalone `valori-node` HTTP server and use the **exact same API**:

```python
from valoricore import MemoryClient
from valoricore.embeddings import OpenAIEmbedder

embedder = OpenAIEmbedder()          # reads OPENAI_API_KEY from env

# Simply pass a remote URL — everything else is identical
client = MemoryClient(remote="http://my-valori-node:3000")

result = client.add_document(
    text  = "Remote deployment with full audit trail.",
    embed = embedder,
)
print(result["document_node_id"])

# Snapshot the remote node state to local bytes
snap = client.snapshot()
with open("backup.snap", "wb") as f:
    f.write(snap)
```

---

### 3 · Async API (FastAPI / asyncio)

```python
import asyncio
from valoricore import AsyncMemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")

async def main():
    # Async context manager – auto-closes on exit
    async with AsyncMemoryClient(path="./async_db") as client:

        result = await client.add_document(
            text  = "Non-blocking deterministic vector storage.",
            embed = embedder,
        )
        print(f"node_id={result['document_node_id']}")

        hits = await client.semantic_search(
            "Non-blocking search", embed=embedder, k=5
        )
        print(f"Found {len(hits)} results")

        # Snapshot + audit from async context
        snap  = await client.snapshot()
        state = await client.get_state_hash()
        print(f"State: {state}")

asyncio.run(main())
```

---

## 🔌 Embedding Providers

The `valoricore.embeddings` module provides production-ready adapters for every major embedding provider.  Every adapter implements `__call__` so it works **directly** wherever an `EmbedFn` is accepted.

### Provider Overview

| Provider | Class | Offline? | Extra install |
|---|---|---|---|
| **SentenceTransformers** | `SentenceTransformerEmbedder` | ✅ Yes | `pip install "valoricore[local]"` |
| **OpenAI** | `OpenAIEmbedder` | ❌ Cloud | `pip install "valoricore[openai]"` |
| **Cohere** | `CohereEmbedder` | ❌ Cloud | `pip install "valoricore[cohere]"` |
| **HuggingFace Inference** | `HuggingFaceEmbedder` | ❌ Cloud | *(requests, built-in)* |
| **Ollama** | `OllamaEmbedder` | ✅ Local server | `ollama pull nomic-embed-text` |
| **Dummy / CI** | `DummyEmbedder` | ✅ Yes | *(built-in)* |
| **Hash / CI** | `HashEmbedder` | ✅ Yes | *(built-in)* |

### Local / Offline Production (Recommended for Air-Gapped Environments)

```python
from valoricore.embeddings import SentenceTransformerEmbedder, CachedEmbedder

# High-quality model, fully offline after first download
raw_embedder = SentenceTransformerEmbedder(
    model_name = "BAAI/bge-small-en-v1.5",   # dim=384, state-of-the-art
    device     = "cpu",                        # or "cuda", "mps"
    normalize  = True,                         # cosine similarity friendly
)

# Optional: wrap with LRU cache to avoid re-embedding identical texts
embedder = CachedEmbedder(raw_embedder, max_size=5000)
```

### OpenAI (Cloud)

```python
import os
from valoricore.embeddings import OpenAIEmbedder

embedder = OpenAIEmbedder(
    api_key    = os.environ["OPENAI_API_KEY"],   # or pass directly
    model      = "text-embedding-3-small",        # dim=1536
    dimensions = 384,                            # optional truncation (3-* models only)
)
```

### Ollama (Local Server — Zero Cloud Dependency)

```bash
# One-time setup
brew install ollama && ollama serve
ollama pull nomic-embed-text        # dim=768
```

```python
from valoricore.embeddings import OllamaEmbedder

embedder = OllamaEmbedder(
    model    = "nomic-embed-text",
    base_url = "http://localhost:11434",
)
```

### Convenience Factory

```python
from valoricore.embeddings import get_embedder

# Swap providers with one line change
embedder = get_embedder("local",       model_name="all-MiniLM-L6-v2")
embedder = get_embedder("openai",      api_key="sk-...")
embedder = get_embedder("ollama",      model="nomic-embed-text")
embedder = get_embedder("cohere",      api_key="...")
embedder = get_embedder("huggingface", api_key="hf_...", model="sentence-transformers/all-MiniLM-L6-v2")
embedder = get_embedder("dummy",       dim=384)   # CI / tests
```

### Async Embedder (for asyncio pipelines)

```python
from valoricore.embeddings import SentenceTransformerEmbedder, AsyncEmbedder

sync_embedder  = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
async_embedder = AsyncEmbedder(sync_embedder)   # runs in thread-pool

async def pipeline():
    vec  = await async_embedder.embed("Hello")
    vecs = await async_embedder.embed_batch(["Hello", "World"])
```

---

## 🧠 Core Concepts

### Records
A **Record** is a dense fixed-point vector stored in the kernel's `RecordPool`. Every insert returns an integer `record_id` and a BLAKE3 Merkle proof.

### Nodes & Edges (Knowledge Graph)
A **Node** is a named entity that optionally points to a Record.  An **Edge** is a directed relationship between two Nodes. The graph is stored in the same memory space as the vector pool — no separate database.

### Node Kinds (built-in constants)

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

### Edge Kinds (built-in constants)

```python
from valoricore import (
    EDGE_RELATION,    # 0 – generic relation
    EDGE_FOLLOWS,     # 1 – sequential ordering
    EDGE_IN_EPISODE,  # 2 – membership in episode
    EDGE_BY_AGENT,    # 3 – created/sent by agent
    EDGE_MENTIONS,    # 4 – entity mention
    EDGE_REFERS_TO,   # 5 – cross-reference
    EDGE_PARENT_OF,   # 6 – hierarchical parent→child
)
```

---

## 📖 Step-by-Step Usage Guide

### Step 1 — Install & Verify

```bash
pip install "valoricore[local]"
python -c "import valoricore; print(valoricore.__version__)"
```

### Step 2 — Choose Your Embedding Provider

```python
from valoricore.embeddings import get_embedder

# Local (no API key, no internet after first download)
embedder = get_embedder("local", model_name="all-MiniLM-L6-v2")

# OpenAI
# embedder = get_embedder("openai")   # reads OPENAI_API_KEY env var

# CI / testing (zero-cost, deterministic)
# embedder = get_embedder("dummy", dim=384)
```

### Step 3 — Initialize the Client

```python
from valoricore import MemoryClient

# Local embedded engine (no server needed)
client = MemoryClient(path="./my_db")

# OR connect to a remote cluster
# client = MemoryClient(remote="http://my-node:3000")
```

### Step 4 — Ingest Documents

```python
# From a string
result = client.add_document(
    text       = open("my_paper.txt").read(),
    embed      = embedder,
    title      = "My Paper",
    chunk_size = 512,         # chars per chunk
)

# From a PDF file (requires: pip install "valoricore[pdf]")
from valoricore import load_text_from_file
text   = load_text_from_file("report.pdf")
result = client.add_document(text=text, embed=embedder)

# Insert a raw pre-computed vector
result = client.upsert_vector(vector=[0.1, 0.2, ...])  # len must match kernel dim
```

### Step 5 — Semantic Search

```python
hits = client.semantic_search(
    query = "What is deterministic AI memory?",
    embed = embedder,
    k     = 10,
)

for hit in hits:
    print(f"Record ID : {hit['id']}")
    print(f"L2 Score  : {hit['score']}")   # lower = closer (L2 squared)
```

### Step 6 — Knowledge Graph Operations

```python
from valoricore import NODE_AGENT, NODE_DOCUMENT, EDGE_BY_AGENT

# Manual graph construction
record_id  = client._db.insert([0.5] * 384)
agent_node = client.create_node(kind=NODE_AGENT)
doc_node   = client.create_node(kind=NODE_DOCUMENT, record_id=record_id)

# Link agent → document
client.create_edge(from_id=agent_node, to_id=doc_node, kind=EDGE_BY_AGENT)

# Inspect
print(client.get_node(doc_node))       # {"kind": 5, "record_id": 0}
print(client.get_edges(agent_node))    # [{"edge_id": 0, "to_node": 1, "kind": 3}]

# Traversal: BFS up to depth 2
visited_nodes = client.walk(agent_node, max_depth=2)

# Collect all record_ids reachable from a starting node
record_ids = client.expand(agent_node, max_depth=2)
```

### Step 7 — Lifecycle (Delete, Soft Delete)

```python
# Permanently remove record from pool and search index
client.delete(record_id=0)

# Soft delete: deactivates record but preserves pool slot
client.soft_delete(record_id=1)

# Count active records
n = client.record_count()
print(f"Active records: {n}")
```

### Step 8 — Snapshot, Restore, and Audit

```python
# Snapshot the full kernel state to bytes
snap = client.snapshot()
with open("state.snap", "wb") as f:
    f.write(snap)

# Restore to a fresh engine (bit-exact)
fresh = MemoryClient(path="./restored_db")
fresh.restore(snap)

# The state hashes must be identical
assert fresh.get_state_hash() == client.get_state_hash()
print("✅ Bit-exact restore verified")

# View full event timeline
for event in client.get_timeline():
    print(event)
```

### Step 9 — Cryptographic Proof Verification (Offline)

```python
from valoricore import ingest_embedding, generate_proof, verify_embedding

my_vector = [0.1] * 384

# Generate a standalone proof for this vector (no DB connection required)
fixed_values = ingest_embedding(my_vector)   # float → Q16.16
proof_hex    = generate_proof(fixed_values)  # BLAKE3 Merkle node

# Verify offline — proves the vector has not been tampered with
is_valid = verify_embedding(floats=my_vector, claimed_hash=proof_hex)
print(f"Proof valid: {is_valid}")
```

---

## 🔗 Framework Integrations

### LangChain

```bash
pip install "valoricore[langchain]"
```

```python
from langchain_openai import OpenAIEmbeddings
from valoricore.adapters import ValoricoreAdapter, LangChainVectorStore

adapter     = ValoricoreAdapter(base_url="http://localhost:3000")
embeddings  = OpenAIEmbeddings()

vectorstore = LangChainVectorStore(adapter=adapter, embedding=embeddings)

# Add documents
vectorstore.add_texts(
    texts     = ["Valoricore is deterministic.", "Fixed-point arithmetic rocks."],
    metadatas = [{"source": "intro"}, {"source": "math"}],
)

# Search
docs = vectorstore.similarity_search("What is deterministic AI?", k=3)
for doc in docs:
    print(doc.page_content)

# With scores
docs_scores = vectorstore.similarity_search_with_score("deterministic", k=3)
for doc, score in docs_scores:
    print(f"{doc.page_content[:60]}…  score={score:.4f}")
```

**LangChain Retriever:**

```python
from valoricore.adapters import ValoricoreAdapter, LangChainRetriever

adapter   = ValoricoreAdapter(base_url="http://localhost:3000")
retriever = LangChainRetriever(
    adapter  = adapter,
    embed_fn = lambda t: embeddings.embed_query(t),
    k        = 5,
)

docs = retriever.get_relevant_documents("deterministic vector search")
```

### LlamaIndex

```bash
pip install "valoricore[llamaindex]"
```

```python
from llama_index.core import VectorStoreIndex, StorageContext
from valoricore.adapters import ValoricoreAdapter, LlamaIndexVectorStore

adapter      = ValoricoreAdapter(base_url="http://localhost:3000")
vector_store = LlamaIndexVectorStore(adapter=adapter)

storage_ctx  = StorageContext.from_defaults(vector_store=vector_store)
index        = VectorStoreIndex.from_documents(documents, storage_context=storage_ctx)

query_engine = index.as_query_engine()
response     = query_engine.query("What is Valoricore?")
print(response)
```

---

## 🔐 Error Handling

```python
from valoricore import (
    MemoryClient,
    ValoricoreError,   # base – catch all SDK errors
    ValidationError,   # bad vector dimension / FXP out-of-range
    ConnectionError,   # remote node unreachable
    IntegrityError,    # BLAKE3 proof mismatch
    NotFoundError,     # record / node / edge doesn't exist
    KernelError,       # unrecoverable Rust kernel error
)

client = MemoryClient(path="./db")

try:
    client.delete(record_id=9999)
except NotFoundError:
    print("Record does not exist — safe to ignore")

try:
    client.upsert_vector([0.1] * 128)   # wrong dimension
except ValidationError as e:
    print(f"Bad embedding: {e}")

try:
    remote = MemoryClient(remote="http://offline-node:3000")
    remote.snapshot()
except ConnectionError as e:
    print(f"Node unreachable: {e}")
```

---

## 📊 Performance Characteristics

Valoricore enforces **deterministic L2 brute-force scanning** to guarantee auditability.

| Operation | Local FFI | Remote HTTP |
|---|---|---|
| Single insert | ~20 µs | ~0.5 ms |
| Batch insert (1k vectors) | ~15 ms | ~50 ms |
| L2 search (10k×384) | ~8 ms | ~10 ms |
| L2 search (100k×384) | ~80 ms | ~90 ms |
| Graph BFS (depth 2, 50 nodes) | ~0.5 ms | ~2 ms |
| State hash (BLAKE3) | <1 µs | ~1 ms |
| Snapshot (10k records) | ~5 ms | ~20 ms |

*Benchmarked on Apple M2. The local FFI path calls Rust directly with zero serialization overhead.*

> [!NOTE]
> Valoricore uses **Q16.16 fixed-point** arithmetic. Safe input range for embedding values is **[-32767.0, 32767.0]**. Standard normalized embeddings (OpenAI, SentenceTransformers) are always in [-1.0, 1.0] and are therefore safe.

---

## ⚙️ Configuration Reference

### `MemoryClient` / `AsyncMemoryClient`

| Parameter | Type | Default | Description |
|---|---|---|---|
| `path` | `str` | `"./valori_db"` | Local database directory |
| `remote` | `str \| None` | `None` | Remote node URL. When set, `path` is ignored |
| `index_kind` | `str` | `"bruteforce"` | Future: `"hnsw"` / `"ivf"` |
| `quantization` | `str` | `"none"` | Future: `"int8"` / `"binary"` |

### `Valoricore` / `AsyncValoricore` (factories)

```python
from valoricore import Valoricore, AsyncValoricore

db       = Valoricore(path="./db")                        # local
db       = Valoricore(remote="http://node:3000")          # remote

async_db = AsyncValoricore(path="./db")                   # local async
async_db = AsyncValoricore(remote="http://node:3000")     # remote async
```

---

## 🛠 Forensic CLI

The `valori` CLI lets you inspect the append-only event log and reproduce the exact state of any historical snapshot.

```bash
# Install CLI (included with the package)
pip install valoricore

# Deep forensic inspection
valori inspect --dir ./my_valori_db --snapshot-path ./my_valori_db/state.snap

# View chronological event timeline
valori timeline ./my_valori_db/events.log

# Verify a snapshot's state hash
valori verify --snapshot ./my_valori_db/state.snap --expected-hash <64-char-hex>
```

---

## 🗂 Project Structure

```
valoricore/
├── __init__.py                  # Public API surface
├── embeddings.py                # 🆕 Embedding provider adapters
├── factory.py                   # Valoricore() / AsyncValoricore() factories
├── local.py                     # LocalClient (FFI)
├── remote.py                    # SyncRemoteClient / AsyncRemoteClient
├── memory.py                    # MemoryClient (high-level)
├── async_memory.py              # AsyncMemoryClient (full async mirror)
├── protocol.py                  # ProtocolClient (unified local+remote)
├── adapter.py                   # ValoricoreAdapter (proof overlay)
├── chunking.py                  # Deterministic text chunkers
├── ingest.py                    # File loaders (.txt, .md, .pdf)
├── kinds.py                     # Node / Edge kind constants
├── types.py                     # Type aliases (Vector, Proof, etc.)
├── exceptions.py                # Exception hierarchy
├── utils.py                     # Internal helpers
└── adapters/                    # Framework adapters (optional)
    ├── base.py                  # ValoricoreAdapter (retry + validation)
    ├── langchain.py             # LangChain Retriever
    ├── langchain_vectorstore.py # LangChain VectorStore
    ├── llamaindex.py            # LlamaIndex VectorStore
    └── sentence_transformers_adapter.py
```

---

## 📚 Documentation

| Resource | Description |
|---|---|
| [Getting Started Guide](https://github.com/varshith-Git/Valori-Kernel/blob/main/python/docs/getting_started.md) | First 5 minutes walkthrough |
| [API Reference](https://github.com/varshith-Git/Valori-Kernel/blob/main/python/docs/api_reference.md) | Complete method signatures and return types |
| [Architecture](https://github.com/varshith-Git/Valori-Kernel/blob/main/architecture.md) | Rust kernel internals and design decisions |

---

## 🤝 Contributing

```bash
# Clone and install for development
git clone https://github.com/varshith-Git/Valori-Kernel
cd Valori-Kernel/python
pip install -e ".[dev]"

# Build the Rust FFI extension
cd ..
maturin develop

# Run tests
pytest python/tests/ -v
```

---

## 📄 License

Licensed under the **GNU Affero General Public License v3.0** (AGPL-3.0).  
See [LICENSE](https://github.com/varshith-Git/Valori-Kernel/blob/main/LICENSE) for details.

---

<div align="center">
<strong>Built with ❤️ by the Valoricore team</strong><br/>
<em>Integrity-First AI Infrastructure</em>
</div>
