# Valoricore Python API Reference 🛡️

This document covers all functions and classes exported in the top-level `valoricore` package.

## 🏭 Core Factories

### `Valoricore(remote=None, path="./valori_db")`
Synchronous factory for embedded or remote kernels.

### `AsyncValoricore(remote=None, path="./valori_db")`
Asynchronous factory. Recommended for **FastAPI**, **Starlette**, and high-concurrency applications.

- **`remote`** (Optional[str]): If provided, returns an `AsyncRemoteClient`.
- **`path`** (str): The local directory for database storage.

---

## 🧠 High-Level APIs

### `MemoryClient` (Sync) & `AsyncMemoryClient` (Async)
The primary APIs for managing semantic memory and Knowledge Graphs.

- **`add_document(text, embed, ...)`**: Automatically chunks, embeds, and stores documents.
- **`semantic_search(query, embed, k=5)`**: Encodes string query and performs vector search.
- **`upsert_vector(vector, ...)`**: Directly inserts a vector and links it to a Graph node.

> [!TIP]
> `AsyncMemoryClient` uses **Thread-Shielding**. It is safe to use in an `asyncio` loop even with the local embedded kernel.

---

## 🔌 Clients (`LocalClient`, `SyncRemoteClient`, `AsyncRemoteClient`)

### Vector Operations
- **`insert(vector, tag=0)`**: Returns `record_id`.
- **`insert_batch(vectors)`**: Returns list of `record_id`s.
- **`insert_with_proof(vector, tag=0)`**: Returns `(record_id, proof_bytes)`.
- **`delete(record_id)`**: Permanently remove a record.
- **`search(query_vector, k=5, filter_tag=None)`**: Nearest neighbor search with optional filtering.

### Metadata & Graph
- **`set_metadata(record_id, metadata: bytes)`** / **`get_metadata(record_id)`**
- **`create_node(kind, record_id)`** / **`create_edge(from, to, kind)`**

---

## ⚠️ Exceptions

- **`ValoricoreError`**: Base exception.
- **`ConnectionError`**: Remote node communication failure.
- **`ValidationError`**: Invalid dimensions or data.
- **`IntegrityError`**: Cryptographic proof verification failure.
- **`NotFoundError`**: Resource missing.

---

## 🔐 Cryptographic Proof Bridge

- **`ingest_embedding(floats) -> List[int]`**: Deterministic Q16.16 conversion.
- **`generate_proof(fixed_values) -> str`**: Generate BLAKE3 Merkle root hex.
- **`verify_embedding(floats, claimed_hash) -> bool`**: Bit-exact offline verification.
