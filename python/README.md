# Valori Python SDK

The `valori` package provides two levels of access to the Valori system:
1.  **Core Client** (`Valori`): Raw access to the kernel (vectors, graph nodes).
2.  **Protocol Client** (`ProtocolClient`): High-level "Memory" features (text chunking, metadata, memory IDs).

## 📦 Installation
```bash
pip install .
# or
pip install values
```

## 1. Protocol Client (Recommended for Agents)
Handles text embedding, chunking, metadata, and memory IDs (`rec:0`).

### Initialization
```python
from valori import ProtocolClient

# Define an embedding function (e.g., OpenAI, HuggingFace)
def my_embedder(text: str) -> list[float]:
    # return [0.1, 0.2, ...] (Must match dim=16 or configured dim)
    return [0.0] * 16

client = ProtocolClient(
    remote="https://your-node.koyeb.app",
    embed=my_embedder
)
```

### Methods

#### `upsert_text(text, metadata=...)`
Chunks text, embeds it, and stores it as a Document + Chunks.
```python
res = client.upsert_text(
    text="Valori is a deterministic memory kernel.",
    metadata={"source": "documentation", "author": "Varshith"}
)
print(res["memory_ids"]) # ['rec:10', 'rec:11'...]
```

#### `search_text(query, k=5)`
Embeds query and finds similar memories.
```python
results = client.search_text("What is Valori?", k=3)
for hit in results["results"]:
    print(hit["memory_id"], hit["score"])
```

#### `upsert_vector(vector, metadata=...)`
Directly store a vector with metadata.
```python
vec = [0.1] * 16
client.upsert_vector(vector=vec, metadata={"type": "raw_embedding"})
```

#### `get_metadata(target_id)` / `set_metadata(target_id, metadata)`
Read/Write metadata for any ID (`rec:0`, `node:100`).
```python
meta = client.get_metadata("rec:10")
client.set_metadata("rec:10", {"status": "archived"})
```

---

## 2. Core Client (`Valori`)
Direct access to `valori-node` endpoints. Useful for raw vector ops or graph management.

### Initialization
```python
from valori import Valori

# Connects to Node
client = Valori(remote="https://your-node.koyeb.app")

# OR Local (In-Memory FFI)
# client = Valori(remote=None)
```

### Methods

#### `insert(vector)` / `insert_batch(vectors)`
Insert single or multiple vectors atomically.
```python
# Single
rid = client.insert([0.1, ...])

# Batch (Atomic)
vectors = [[0.1]*16, [0.2]*16, [0.3]*16]
ids = client.insert_batch(vectors) # [0, 1, 2]
```

#### `search(query, k)`
Raw search. Returns IDs and scores.
```python
hits = client.search([0.1, ...], k=5)
# [{'id': 10, 'score': 12345}, ...]
```

#### `get_metadata(id)` / `set_metadata(id, data)`
Read/Write metadata (for both HTTP and Local modes).
```python
client.set_metadata(10, b"user:123")
meta = client.get_metadata(10)
```

#### `get_state_hash()`
**Verifiable AI**: Get cryptographic proof of current database state.
```python
hash1 = client.get_state_hash()
# 1a2b3c... (BLAKE3 hash)
```

#### `record_count()`
Get total number of records.
```python
count = client.record_count()
```

#### `soft_delete(id)`
Mark record as deleted (excluded from search).
```python
client.soft_delete(10)
```

#### `snapshot()` / `restore(data)`
Backup and recovery.
```python
# Save to bytes
data = client.snapshot()

# Restore (Local mode only)
client.restore(data)
```
