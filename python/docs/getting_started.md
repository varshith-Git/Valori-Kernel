# Getting Started with Valoricore 🛡️

This guide will walk you through the first 5 minutes of using the `valoricore` Python package, covering embedded vector memory, async queries, Knowledge Graph topology, and offline cryptographic verification.

---

## 1. Installation
Valoricore is distributed with pre-compiled Rust binaries. You can install it directly from PyPI (or your local build):

```bash
pip install valoricore
```

---

## 2. Basic Semantic Memory (Vectors)
The fastest way to get started is using the `MemoryClient`. You will need an embedding function (like one from `sentence-transformers` or OpenAI).

```python
from valoricore import MemoryClient

# 1. Initialize (Local Mode - Embedded FFI Engine)
client = MemoryClient(path="./my_valori_db")

# 2. Define a simple embedding function (Example using dummy data)
def my_embedder(text: str) -> list[float]:
    # Should return a list of floats matching your kernel's dimension (e.g., 384)
    return [0.1] * 384 

# 3. Add a Document
# This automatically chunks the text and securely inserts it into the RecordPool
result = client.add_document(
    text="Valoricore is a deterministic vector database.",
    embed=my_embedder,
    title="Valoricore Intro"
)

print(f"Stored Document Node ID: {result['document_node_id']}")

# 4. Search
hits = client.semantic_search("What is Valoricore?", embed=my_embedder, k=2)
for hit in hits:
    print(f"Match ID: {hit['id']}, Score: {hit['score']}")
```

---

## 3. High-Performance Async Search
For web applications (FastAPI, Starlette, etc.), use the `AsyncMemoryClient` to avoid blocking the asyncio event loop.

```python
import asyncio
from valoricore import AsyncMemoryClient

async def main():
    # Uses internal Thread-Shielding for safety with local FFI.
    # To connect to a scalable HTTP cluster, pass remote="http://address:3000"
    client = AsyncMemoryClient()
    
    def my_embedder(text: str) -> list[float]: return [0.1] * 384
    
    # Non-blocking deterministic search
    hits = await client.semantic_search("Async query", embed=my_embedder, k=5)
    print(f"Found {len(hits)} results asynchronously.")

asyncio.run(main())
```

---

## 4. Cryptographic Verification
Because Valoricore uses fixed-point math (Q16.16) and BLAKE3 hashing, you can cryptographically verify that an embedding exists in the database's exact state **entirely offline** without touching the database at all.

```python
from valoricore import verify_embedding, Valoricore

# 1. Get the current mathematical state of the database
db = Valoricore()
state_hash = db.get_state_hash()
print(f"Database State Signature: {state_hash}")

# 2. Perform offline verification
# This proves mathematically that the vector [0.1]*384 generates a valid Merkle node
my_vector = [0.1] * 384
is_valid = verify_embedding(floats=my_vector, claimed_hash=state_hash)

if is_valid:
    print("Verification Passed: The vector is authentic and untampered.")
else:
    print("Verification Failed: Integrity violation detected!")
```

---

## 5. Using the Hybrid Knowledge Graph

Valoricore natively bridges Semantic Vectors and Knowledge Graphs in the same memory space.
The **fluent API** lets you build the graph without ever touching raw integer IDs.

### 5a. Fluent API (recommended)

```python
from valoricore import MemoryClient, Node
from valoricore.kinds import NODE_DOCUMENT, NODE_CHUNK, NODE_AGENT, EDGE_PARENT_OF, EDGE_BY_AGENT

client = MemoryClient(path="./my_db")

# db.node() inserts the vector AND creates the graph node in a single call
doc   = client.node(NODE_DOCUMENT)
chunk = client.node(NODE_CHUNK, vector=[0.5]*384)  # returns a Node object

# node.link_to() creates the edge — returns self for chaining
doc.link_to(chunk, EDGE_PARENT_OF)

# Link to multiple nodes at once
c2 = client.node(NODE_CHUNK, vector=[-0.5]*384)
doc.link_to([chunk, c2], EDGE_PARENT_OF)

# Traverse as Node objects — no raw ID bookkeeping
children = doc.children(EDGE_PARENT_OF)
print(children)   # [Node(id=1, kind=6, record_id=0), Node(id=2, kind=6, record_id=1)]

# BFS traversal
visited   = doc.walk(max_depth=2)      # List[Node]
rids      = doc.record_ids(max_depth=2)  # [0, 1]  — pass to search() for RAG

# Cascade-delete node + all edges
chunk.delete()
```

### 5b. Build a full document → chunk graph (the RAG pattern)

```python
def my_embedder(text: str) -> list: return [0.1] * 384

text_chunks = ["Intro paragraph.", "Main content.", "Conclusion."]
embeddings  = [my_embedder(t) for t in text_chunks]

with client.build_document(title="My Article") as builder:
    for emb in embeddings:
        builder.add_chunk(emb)   # insert + create + link, all in one call

doc_node   = builder.document    # root Node
chunk_rids = builder.record_ids  # [0, 1, 2] — ready for search retrieval
print(f"Built doc node {doc_node.id} with {len(builder.chunks)} chunks")
```

### 5c. Low-level API (still fully supported)

```python
# Raw integer IDs still work — the two styles mix freely
vector_a_id  = client._db.insert([0.5]*384)
agent_node   = client.create_node(kind=NODE_AGENT)
doc_node_id  = client.create_node(kind=NODE_DOCUMENT, record_id=vector_a_id)
client.create_edge(from_id=agent_node, to_id=doc_node_id, kind=EDGE_BY_AGENT)

related_ids = client.expand(start_node=agent_node, max_depth=1)
print(f"Graph traversal yielded record IDs: {related_ids}")
```

---
**Next Steps**: Check out the [API Reference](api_reference.md) for full method signatures and configuration options.
