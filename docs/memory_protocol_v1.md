# Memory Protocol V1 (Draft)

This specification extends v0 with Enterprise features: Pluggable Indexing, Quantization, and Host Metadata.

## 1. Concepts

### Indexing
Strategies to accelerate vector search.
*   `bruteforce`: O(N) scan. Exact. Deterministic. (Default)
*   `hnsw`: Graph-based. O(log N). Deterministic (seeded).
*   `flat`: Contiguous memory scan optimization.

### Quantization
Strategies to compress vector storage.
*   `none`: Full precision (Q16.16).
*   `scalar`: 8-bit per dimension (u8). lossy.
*   `product`: PQ coding (future).

### Metadata (Host-Level)
Key-Value storage attached to IDs. Not part of the deterministic kernel calculation but stored alongside it.
*   Scope: `RecordId`, `NodeId`, namespaced strings?
*   Format: JSON.

## 2. API Extensions

### Configuration (Startup)
When starting the node, configuration flags determine the Index/Quantization strategy.
(Currently compile-time or startup-config).

### Vector Operations
**POST** `/v1/memory/search_vector`
```json
{
  "query_vector": [0.1, ...],
  "k": 5,
  "index": "hnsw" // Optional override hint
}
```

### Metadata Operations

**POST** `/v1/memory/meta/set`
```json
{
  "target_id": "rec:123", // or "node:50"
  "metadata": {
    "author": "gwern",
    "timestamp": 123456789
  }
}
```

**GET** `/v1/memory/meta/get?target_id=rec:123`
Response:
```json
{
  "target_id": "rec:123",
  "metadata": { ... }
}
```

## 3. Persistence Model
*   The system maintains `data.bin` (Kernel Snapshot) and `meta.json` (Metadata).
*   **Auto-Save**: Configurable interval (e.g., 30s) or on-change.
*   **Determinism**: The `data.bin` is bit-perfect. `meta.json` is standard JSON.
