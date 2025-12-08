# Valori Memory Protocol v0

## 1. Title and Overview

**Valori Memory Protocol v0**

Valori is a deterministic, fixed-point vector + knowledge graph engine. The **Valori Memory Protocol (VMP v0)** defines a logical protocol for how clients interact with Valori as a memory store.

v0 is deliberately minimal, focusing on **vector-level memory** and **semantic search**. Higher-level semantics (text parsing, document management) are primarily implemented in the Python client, while the kernel ensures deterministic math and graph consistency.

## 2. Design Goals

-   **Determinism**: Replayable behavior via the FXP kernel. Same inputs + same command sequence = identical bitwise state.
-   **Stability**: JSON shapes are stable and minimal.
-   **Separation of Concerns**:
    -   **Kernel**: Deterministic math and knowledge graph.
    -   **Protocol**: Defines abstract "Upsert" / "Search" operations.
    -   **Clients**: Handle text chunking, embedding, and rich semantics.

## 3. Core Concepts

| Concept | Definition |
| :--- | :--- |
| **Record** | A single vector in the kernel, identified by `record_id: u32`. |
| **Memory** | A protocol-level concept. In v0, a "Memory" is effectively a Record plus context (chunk/doc links). Canonical ID: `rec:<id>`. |
| **Document** | Client-side concept for a file/text. Mapped to `NODE_DOCUMENT` graph nodes. |
| **Chunk** | A slice of a Document. Mapped to `NODE_CHUNK` graph nodes linked to Records. |
| **Reserved** | `Actor`, `Session`, `Tags`, `Metadata` are reserved for future host-layer storage. |

## 4. Operations (Protocol-level)

These operations define the v0 API.

### 4.1 `mem.upsert_text`

Insert textual memory. The client is responsible for embedding. **Client-side only in v0**.

**Request:**
```json
{
  "text": "string",
  "doc_id": "optional-string",
  "actor_id": "optional-string",
  "tags": ["optional-tag-strings"],
  "metadata": { "optional": "json-blob" }
}
```

**Response:**
```json
{
  "memory_ids": ["rec:12", "rec:13"],
  "record_ids": [12, 13],
  "document_node_id": 101,
  "chunk_node_ids": [501, 502],
  "chunk_count": 2
}
```

### 4.2 `mem.upsert_vector`

Insert a pre-computed embedding vector. Maps 1:1 to a Record.

**Request:**
```json
{
  "vector": [0.0, 0.0, ...],      // length == D (16)
  "attach_to_document_node": 123, // optional u32
  "tags": ["optional"],
  "metadata": { "optional": "json-blob" }
}
```

**Response:**
```json
{
  "memory_id": "rec:12",
  "record_id": 12,
  "document_node_id": 123,
  "chunk_node_id": 45
}
```

### 4.3 `mem.search_vector`

Search for nearest neighbors by vector.

**Request:**
```json
{
  "query_vector": [0.0, 0.0, ...],
  "k": 5
}
```

**Response:**
```json
{
  "results": [
    { "memory_id": "rec:12", "record_id": 12, "score": 123456 },
    { "memory_id": "rec:3",  "record_id": 3,  "score": 234567 }
  ]
}
```

### 4.4 `mem.search_text`

Semantic search using client-side embedding. **Client-side only in v0**.

**Request:**
```json
{
  "query_text": "string",
  "k": 5
}
```

**Response:** Same as `mem.search_vector`.

### 4.5 `mem.snapshot` / `mem.restore`

Maps directly to kernel operations.
-   **snapshot**: Returns binary blob of full state.
-   **restore**: Replaces state from blob.

## 5. Error Model

Common errors (mapped to HTTP codes or Exceptions):
-   `INVALID_ARGUMENT` (e.g., malformed JSON)
-   `DIM_MISMATCH` (Vector length != D)
-   `CAPACITY_EXCEEDED` (Pool full)
-   `NOT_FOUND`
-   `INTERNAL_ERROR`

## 6. Determinism Guarantees

-   Given the **same sequence** of protocol operations and the **same embedding function**, the kernel state and results are bitwise identical.
-   Valori relies on the client/user to provide consistent embeddings.

## 7. Versioning

-   This is **VMP v0**.
-   Breaking changes will bump the version (e.g., `/v2/memory/`).
-   Future versions may add metadata storage, richer graph semantics (Episodes), and index tuning.
