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
Valoricore natively bridges Semantic Vectors and Knowledge Graphs in the exact same memory space. You can manually link entities to form complex contextual relationships.

```python
from valoricore import Valoricore

db = Valoricore()

# Insert raw vectors into the RecordPool
vector_a_id = db.insert([0.5]*384)
vector_b_id = db.insert([-0.5]*384)

# Create nodes mapping to those records
# Let's assume Kind '1' is 'Person', and Kind '2' is 'Company'
person_node = db.create_node(kind=1, record_id=vector_a_id)
company_node = db.create_node(kind=2, record_id=vector_b_id)

# Create a directional relationship (Edge)
# Let's assume Kind '100' is 'WORKS_AT'
db.create_edge(from_id=person_node, to_id=company_node, kind=100)

# Traverse the Graph!
# We can expand outward from the person_node to find all associated vectors
related_record_ids = db.expand(start_node=person_node, max_depth=1)
print(f"Graph traversal yielded vector IDs: {related_record_ids}")
```

---
**Next Steps**: Check out the [API Reference](api_reference.md) for full method signatures and configuration options.
