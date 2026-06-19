<div align="center">

<img src="https://img.shields.io/badge/Valoricore-v0.2.1-6c47ff?style=for-the-badge&logo=rust" alt="version"/>

# Valoricore

### The Official Python SDK for **Valori-Kernel**

*AI Memory That Is Cryptographically Auditable — By Design*

<br/>

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/varshith-Git/Valori-Kernel/blob/main/LICENSE-MIT)
[![Python 3.8+](https://img.shields.io/badge/python-3.8%2B-blue.svg)](https://www.python.org/downloads/)
[![Rust Core](https://img.shields.io/badge/core-Rust%20%2Fno__std-orange.svg)](https://www.rust-lang.org/)
[![PyPI](https://img.shields.io/pypi/v/valoricore.svg)](https://pypi.org/project/valoricore/)
[![Build](https://img.shields.io/github/actions/workflow/status/varshith-Git/Valori-Kernel/ci.yml?branch=main)](https://github.com/varshith-Git/Valori-Kernel/actions)

</div>

---

`valoricore` is the official Python SDK for [**Valori-Kernel**](https://github.com/varshith-Git/Valori-Kernel) — a `no_std` Rust engine that makes AI memory **reproducible and provable**.

Standard vector databases use floating-point arithmetic, which produces different search results on different CPUs. When a regulator or auditor asks you to replay an AI decision, you cannot guarantee the replay produces the same result on different hardware — or even on the same hardware after a library upgrade.

Valori fixes this by unifying **Vector Memory** and **Knowledge Graphs** using **Q16.16 fixed-point arithmetic**, producing bit-identical results across x86, ARM, and RISC-V. Every insert is recorded in a BLAKE3-chained event log. The entire state is summarised in a single **Merkle root hash** you can store, compare, and prove — without touching the database.

**The core use case:** any system where the AI's memory must be reproducible, tamper-evident, and independently verifiable. Finance, legal tech, autonomous systems, or any regulated environment where "trust, but verify" is a legal requirement — not an aspiration.

---

## Why Replace Your Current Vector DB?

| Feature | Valoricore | Chroma / FAISS / Pinecone | Business Value |
|---|---|---|---|
| **Results across hardware** | Bit-identical (Q16.16) | Float drift | Pass cross-platform audits; replay any AI decision with guaranteed identical output |
| **Cryptographic state proof** | BLAKE3 Merkle root per insert | None | Prove exactly what data the AI saw at any point in time |
| **Hybrid Vector + Graph** | Native, same memory space | Separate systems | Build GraphRAG pipelines without managing a second database |
| **Offline proof verification** | No DB connection required | N/A | Auditors can verify AI decisions without accessing production |
| **Snapshot / replay** | Byte-exact restore | Partial / format-specific | Disaster recovery that is provably correct, not just "probably fine" |
| **`no_std` embeddable core** | Runs on ARM Cortex-M4 | Heap-heavy | Deploy AI memory to edge devices, browsers, and air-gapped systems |
| **Multi-tenant collections** | Up to 1 024 isolated namespaces | Tag filtering only | True tenant isolation with zero cross-contamination risk |

---

## For Compliance & Audit Teams

Valori is not a black box. Every state change is written to a BLAKE3-chained append-only event log. An auditor or compliance officer can independently verify what data an AI system saw — without accessing the production database, without trusting the server, and without re-running the model.

```python
from valoricore import ingest_embedding, generate_proof, verify_embedding

# Step 1 — AI system ingests a vector in production and stores the proof
vector      = [0.142, 0.897, 0.334, 0.561]   # e.g. an embedding of a document
fixed_vals  = ingest_embedding(vector)         # convert to deterministic Q16.16
proof_hex   = generate_proof(fixed_vals)       # BLAKE3 Merkle node — store this

print(f"Proof: {proof_hex}")
# → "a3f2c1d9..." (64-char hex)

# Step 2 — Auditor verifies it independently, months later, on any machine
is_valid = verify_embedding(floats=vector, claimed_hash=proof_hex)
print(f"Verified: {is_valid}")   # True — math doesn't lie
```

The proof is computed entirely in Rust (via the embedded FFI) with no network calls. It is deterministic because the underlying arithmetic is fixed-point — no floating-point rounding, no hardware-dependent results.

**What the state hash proves:** that the database contained exactly these records, in exactly this order, at the time the hash was recorded. Any tampering — insert, delete, or reorder — produces a different hash.

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

```bash
pip install "valoricore[local]"
```

### The Audit Proof in 10 Lines

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
client   = MemoryClient(path="./my_valori_db")

# Insert a document — chunked, embedded, and stored in the Knowledge Graph
client.add_document(
    text  = "Loan approved for application #A-20241107 at 14:32 UTC.",
    embed = embedder,
    title = "Decision Log",
)

# Semantic search
hits = client.semantic_search("loan approval decisions", embed=embedder, k=3)

# Every insert changes this hash deterministically.
# Run this on Apple Silicon, Intel, or ARM: the output is identical.
print(f"State hash: {client.get_state_hash()}")
# → e3b0c44298fc1c149afb...  (64-char BLAKE3 hex — the same on every machine)
```

This hash is your cryptographic receipt. Store it in a database, a blockchain, or an audit log. Anyone holding this hash and the original data can verify the state independently — no network connection, no trust in the server required.

### Interactive Colab Notebooks

Test Valoricore in your browser with zero local setup:
- [**End-to-End Demo**](https://colab.research.google.com/drive/1QO1yQMQoGbp9fwrb00KVKTq5bYVGXgJv#scrollTo=hM-PiglYd20l) — Determinism, Knowledge Graph, Crypto Proofs
- [**LangChain Integration**](https://colab.research.google.com/drive/1HezK4l-Hbc6AdHxJNLwSqAgzr8WaKhiq#scrollTo=Hxcyq4OkN0MO)
- [**LlamaIndex Integration**](https://colab.research.google.com/drive/1Q72ANZxBm1fthNpgVW-FftS8sZz6uCr3#scrollTo=XHFOODSTVE6N)

### 1 · Embedded Local Engine (full example)

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
client   = MemoryClient(path="./my_valori_db")

result = client.add_document(
    text  = "Valoricore is a deterministic Rust kernel that unifies "
            "vector memory and knowledge graphs.",
    embed = embedder,
    title = "Introduction",
)
print(f"Document Node ID : {result['document_node_id']}")
print(f"Chunk count      : {result['chunk_count']}")

hits = client.semantic_search("What does Valoricore unify?", embed=embedder, k=3)
for h in hits:
    print(f"  id={h['id']}  score={h['score']}")

print(f"State hash: {client.get_state_hash()}")
```

### 2 · Remote / Cluster Mode

Point `remote` at any node in the cluster. Writes are transparently redirected to
the current leader (HTTP 307); the resolved leader is cached so subsequent writes
skip the extra hop. During a leader election the client retries with exponential
backoff before raising `NotLeaderError`.

```python
from valoricore import MemoryClient, SyncRemoteClient
from valoricore.embeddings import OpenAIEmbedder

embedder = OpenAIEmbedder()

# MemoryClient — high-level, same API as local embedded
client = MemoryClient(remote="http://my-valori-node:3000")
result = client.add_document(text="Remote deployment with full audit trail.", embed=embedder)

# SyncRemoteClient — lower-level, direct access to all endpoints
from valoricore import SyncRemoteClient, NotLeaderError

node = SyncRemoteClient("http://my-valori-node:3000", max_retries=5, retry_backoff=0.3)

# Check cluster health before writing
if not node.cluster_health():
    raise RuntimeError("no leader elected yet")

status = node.cluster_status()
print(f"Leader: node {status['leader']}  Term: {status['term']}")

# Insert — redirects to the leader automatically
record_id = node.insert([0.1, 0.2, 0.3, 0.4])

# Linearizable read: reflects every write committed before this read (default in cluster mode)
hits = node.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="linearizable")

# Eventually-consistent read: answered immediately from the local node (no leader round-trip)
hits_local = node.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="local")
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

## Collections (Multi-tenancy)

Valori supports up to **1 024 named collections** (namespaces). Every data
operation accepts an optional `collection` parameter. The `"default"` collection
always exists and cannot be dropped.

Records in different collections are **fully isolated** — a search scoped to
`"tenant-acme"` never returns records from `"tenant-beta"` or the default
collection, and vice versa.

```python
from valoricore import SyncRemoteClient

client = SyncRemoteClient("http://localhost:3000")

# ── Create ────────────────────────────────────────────────────────────────────
result = client.create_collection("tenant-acme")
# {"name": "tenant-acme", "id": 1, "created": True}

# Idempotent — same name returns the existing ID
result2 = client.create_collection("tenant-acme")
# {"name": "tenant-acme", "id": 1, "created": False}

# ── List ──────────────────────────────────────────────────────────────────────
collections = client.list_collections()
# [{"name": "default", "id": 0}, {"name": "tenant-acme", "id": 1}]

# ── Scoped insert ─────────────────────────────────────────────────────────────
rid_a = client.insert([0.1, 0.2, 0.3, 0.4], collection="tenant-acme")
rid_b = client.insert([0.5, 0.6, 0.7, 0.8])   # lands in "default"

batch_ids = client.insert_batch(
    [[0.1, 0.2, 0.3, 0.4], [0.9, 0.8, 0.7, 0.6]],
    collection="tenant-acme",
)

# ── Scoped search ─────────────────────────────────────────────────────────────
# Only "tenant-acme" records are considered.
hits = client.search([0.1, 0.2, 0.3, 0.4], k=5, collection="tenant-acme")

# Default search — never includes "tenant-acme" records.
default_hits = client.search([0.1, 0.2, 0.3, 0.4], k=5)

# ── Drop ──────────────────────────────────────────────────────────────────────
client.drop_collection("tenant-acme")   # 204, removes all scoped records
# client.drop_collection("default")    # raises ValueError — default is protected
```

Collections work identically in async mode:

```python
from valoricore import AsyncRemoteClient
import asyncio

async def main():
    client = AsyncRemoteClient("http://localhost:3000")

    await client.create_collection("tenant-async")
    ids = await client.insert_batch([[0.1]*4, [0.2]*4], collection="tenant-async")
    hits = await client.search([0.1]*4, k=5, collection="tenant-async")
    await client.drop_collection("tenant-async")
    await client.close()

asyncio.run(main())
```

### Collections in a cluster

Collections are managed through the **leader** exactly like writes. Point the
client at any node; the SDK follows the redirect automatically.

```python
from valoricore import SyncRemoteClient

# Any node — redirects to leader for writes
node = SyncRemoteClient("http://cluster-node-1:3000")

# Create the collection (leader-only, 307-redirect handled automatically)
node.create_collection("tenant-acme")

# Insert through any node in the cluster
for url in ["http://cluster-node-1:3000", "http://cluster-node-2:3000"]:
    c = SyncRemoteClient(url)
    c.insert([0.1, 0.2, 0.3, 0.4], collection="tenant-acme")

# Search on any node — linearizable consistency ensures it reflects all writes
hits = node.search([0.1, 0.2, 0.3, 0.4], k=5,
                   collection="tenant-acme",
                   consistency="linearizable")
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

### Step 6 — Knowledge Graph (Fluent API)

Valoricore ships a high-level **fluent API** so you never have to manage raw integer IDs.
`db.node()`, `node.link_to()`, and `db.build_document()` handle everything in one or two lines.

#### One-liner node creation

```python
from valoricore import MemoryClient, Node
from valoricore.kinds import NODE_DOCUMENT, NODE_CHUNK, EDGE_PARENT_OF, EDGE_REFERS_TO

client = MemoryClient(path="./my_db", dim=384)

# Insert the embedding AND create the node in a single call — no manual ID juggling
doc   = client.node(NODE_DOCUMENT)
chunk = client.node(NODE_CHUNK, vector=my_embedding)  # inserts + creates, returns Node

print(doc)    # Node(id=0, kind=5, record_id=None)
print(chunk)  # Node(id=1, kind=6, record_id=0)
```

#### Method-chaining with `link_to`

```python
# Create a directed edge from doc → chunk
doc.link_to(chunk, EDGE_PARENT_OF)

# Chain multiple edges in one line
c2 = client.node(NODE_CHUNK, vector=embedding_2)
c3 = client.node(NODE_CHUNK, vector=embedding_3)
doc.link_to([c2, c3], EDGE_PARENT_OF)   # link to a list at once

# Traverse back as Node objects
children = doc.children(EDGE_PARENT_OF)
# → [Node(id=1, ...), Node(id=2, ...), Node(id=3, ...)]
```

#### `build_document` context manager — the RAG pattern in 3 lines

```python
embeddings = [embed(chunk) for chunk in text_chunks]   # your embedding function

with client.build_document(title="Q4 Report") as builder:
    for emb in embeddings:
        builder.add_chunk(emb)   # inserts vector, creates NODE_CHUNK, wires EDGE_PARENT_OF

# After the block:
doc_node   = builder.document    # root Node object
chunk_rids = builder.record_ids  # [0, 1, 2, …]  — pass to search for RAG retrieval
```

#### Before vs After

```python
# ── BEFORE (low-level — works, but tedious) ──────────────────────────────────
rid1   = client._db.insert(emb1)
rid2   = client._db.insert(emb2)
doc_id = client.create_node(kind=NODE_DOCUMENT)
ch1    = client.create_node(kind=NODE_CHUNK, record_id=rid1)
ch2    = client.create_node(kind=NODE_CHUNK, record_id=rid2)
client.create_edge(from_id=doc_id, to_id=ch1, kind=EDGE_PARENT_OF)
client.create_edge(from_id=doc_id, to_id=ch2, kind=EDGE_PARENT_OF)

# ── AFTER (fluent — identical performance, far less code) ────────────────────
doc = client.node(NODE_DOCUMENT)
doc.link_to([
    client.node(NODE_CHUNK, vector=emb1),
    client.node(NODE_CHUNK, vector=emb2),
], EDGE_PARENT_OF)
```

#### Full agent memory example

```python
from valoricore.kinds import NODE_AGENT, NODE_DOCUMENT, EDGE_BY_AGENT

# Agent node (no vector — it's a logical entity)
agent = client.node(NODE_AGENT)

# Document node linked to an embedding
doc = client.node(NODE_DOCUMENT, vector=my_embedding)

# Wire the relationship
agent.link_to(doc, EDGE_BY_AGENT)

# Traversal — everything returned as Node objects
visited = agent.walk(max_depth=2)         # [Node, Node, …]
rids    = agent.record_ids(max_depth=2)   # [0, 1, …]  for vector lookup

# Delete cascade (removes node + all edges)
doc.delete()
```

#### Low-level API (still fully supported)

```python
# Raw integer IDs still work — the two styles mix freely
raw_nid = client.create_node(kind=NODE_DOCUMENT)
raw_eid = client.create_edge(from_id=raw_nid, to_id=chunk.id, kind=EDGE_PARENT_OF)

# db.edge() accepts Node objects OR raw ints
client.edge(doc, chunk, EDGE_REFERS_TO)    # Node objects
client.edge(3, 7, EDGE_REFERS_TO)          # raw ints
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

### Environment variables (cluster mode)

Set these to boot a node as a Raft cluster member instead of standalone.

| Variable | Description |
|---|---|
| `VALORI_CLUSTER_MEMBERS` | `id=raft_addr/api_addr,…` — presence activates cluster mode. Example: `1=10.0.0.1:3100/10.0.0.1:3000,2=10.0.0.2:3100/10.0.0.2:3000` |
| `VALORI_NODE_ID` | This node's integer ID (must appear in `VALORI_CLUSTER_MEMBERS`). |
| `VALORI_RAFT_BIND` | gRPC consensus listener address (default `0.0.0.0:3100`). |
| `VALORI_CLUSTER_INIT` | Set to `1` on exactly one node of a brand-new cluster to bootstrap it. |
| `VALORI_RAFT_LOG_PATH` | Path to the `redb` file for the persistent Raft log. When set, the state machine also persists `last_applied` and the latest snapshot so audit events are never replayed after a restart. |

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

#### Knowledge Graph — Fluent API *(recommended)*
| Method | Returns | Description |
|---|---|---|
| `node(kind, vector=None, tag=0)` | `Node` | Create a node; optionally insert a vector and link it in one call |
| `edge(from_node, to_node, kind)` | `int` | Create an edge; accepts `Node` objects or raw integer IDs |
| `build_document(title=None)` | `DocumentGraph` | Context manager: builds doc → chunk graph with no ID bookkeeping |

**`Node` object methods**
| Method | Returns | Description |
|---|---|---|
| `node.link_to(other, edge_kind)` | `self` | Create edge(s) from this node; `other` may be a `Node`, `int`, or list of either |
| `node.link_from(other, edge_kind)` | `self` | Create edge from `other` to this node |
| `node.children(edge_kind=None)` | `List[Node]` | Outgoing neighbours, optionally filtered by edge kind |
| `node.walk(max_depth=2)` | `List[Node]` | BFS traversal; returns visited `Node` objects |
| `node.record_ids(max_depth=2)` | `List[int]` | All reachable vector record IDs (for RAG retrieval) |
| `node.delete()` | `None` | Cascade-delete node and all incident edges |
| `int(node)` | `int` | Escape hatch to the raw integer ID |

**`DocumentGraph` context manager**
| Attribute / Method | Description |
|---|---|
| `builder.add_chunk(vector, tag=0, metadata=None)` | Insert vector, create `NODE_CHUNK`, wire `EDGE_PARENT_OF`; returns `Node` |
| `builder.document` | The root `NODE_DOCUMENT` `Node` |
| `builder.chunks` | Ordered list of chunk `Node` objects |
| `builder.record_ids` | List of vector record IDs in insertion order |

#### Knowledge Graph — Low-Level API *(still fully supported)*
| Method | Description |
|---|---|
| `create_node(kind, record_id)` | Create a graph node; returns integer node ID |
| `create_edge(from_id, to_id, kind)` | Create a directed edge; returns integer edge ID |
| `delete_node(node_id)` | Cascade-delete a node and all its incident edges |
| `delete_edge(edge_id)` | Delete a single edge |
| `get_node(node_id)` | Fetch node kind and attached record_id |
| `get_edges(node_id)` | Fetch all outgoing edges |
| `walk(start_node, max_depth)` | BFS traversal; returns visited node IDs |
| `expand(start_node, max_depth)` | BFS traversal; returns reachable record IDs |

### `SyncRemoteClient` / `AsyncRemoteClient`

These clients expose the full API surface when talking to a running node over HTTP.
`SyncRemoteClient` uses `requests`; `AsyncRemoteClient` uses `httpx` and must be
`await`ed. Both are cluster-aware (automatic leader redirect, retry with backoff).

#### Collections

| Method | Returns | Description |
|---|---|---|
| `create_collection(name)` | `{"name", "id", "created"}` | Create a namespace. Idempotent. |
| `list_collections()` | `[{"name", "id"}, …]` | List all namespaces. |
| `drop_collection(name)` | `None` | Drop a namespace and all its records. Raises `ValueError` for `"default"`. |

All data methods accept `collection: str = "default"`:

| Method | Collection-aware parameter |
|---|---|
| `insert(vector, tag, collection)` | ✅ |
| `insert_batch(batch, collection)` | ✅ |
| `search(query, k, filter_tag, consistency, collection)` | ✅ (also accepts `consistency="linearizable"\|"local"`) |

#### Cluster

| Method | Returns | Description |
|---|---|---|
| `cluster_status()` | `dict` | Leader node ID, term, log indices, membership table. |
| `cluster_health()` | `bool` | `True` when a leader is visible; `False` during election. |
| `get_state_hash()` | `str` | 64-char BLAKE3 hex digest of the current kernel state. |

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

MIT OR Apache-2.0 — see [LICENSE-MIT](https://github.com/varshith-Git/Valori-Kernel/blob/main/LICENSE-MIT).

You may use Valoricore in proprietary, commercial, and on-premise deployments without any copyleft obligations. For enterprise support, SLA agreements, or custom deployment assistance, contact: varshith.gudur17@gmail.com
