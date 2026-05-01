# Getting Started with Valoricore 🛡️

This guide will walk you through the first 5 minutes of using the `valoricore` Python package.

## 1. Installation
First, ensure you are in the `python/` directory of the repository and install the package:

```bash
cd python
pip install -e .
```

## 2. Basic Semantic Memory
The fastest way to get started is using the `MemoryClient`. You will need an embedding function (like one from `sentence-transformers` or OpenAI).

```python
from valoricore import MemoryClient

# 1. Initialize (Local Mode)
client = MemoryClient()

# 2. Define a simple embedding function (Example using fake data)
def my_embedder(text):
    # Should return a list of floats matching your kernel's dimension (e.g., 384)
    return [0.1] * 384 

# 3. Add a Document
# This will automatically chunk the text and create Knowledge Graph nodes
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

## 3. High-Performance Async Search
For web applications (FastAPI, etc.), use the `AsyncMemoryClient` to avoid blocking the event loop.

```python
import asyncio
from valoricore import AsyncMemoryClient

async def main():
    # Uses internal Thread-Shielding for safety with local FFI
    client = AsyncMemoryClient()
    
    def my_embedder(text): return [0.1] * 384
    
    # Non-blocking search
    hits = await client.semantic_search("Async query", embed=my_embedder, k=5)
    print(f"Found {len(hits)} results asynchronously.")

asyncio.run(main())
```

## 4. Cryptographic Verification

## 4. Using the Knowledge Graph
If you want to manually link entities:

```python
from valoricore import Valoricore

db = Valoricore()

# Create nodes
person_id = db.create_node(kind=1) # E.g., Person
company_id = db.create_node(kind=2) # E.g., Company

# Create a relationship (Edge)
db.create_edge(from_id=person_id, to_id=company_id, kind=100) # E.g., WORKS_AT
```
