# Valoricore Python API Reference 🛡️

This document covers all functions, classes, and exceptions exported in the top-level `valoricore` package. Valoricore is designed with a layered abstraction: **Factories** handle initialization, **MemoryClients** handle high-level AI concepts, and **Base Clients** handle raw vector math and Knowledge Graph topology.

---

## 🏭 Core Factories

The easiest way to initialize Valoricore. These factories auto-detect whether you want an embedded FFI kernel or an HTTP connection to a remote cluster.

### `Valoricore(remote=None, path="./valori_db")` -> `LocalClient` | `SyncRemoteClient`
Synchronous factory for embedded or remote kernels.
- **`remote`** *(str, optional)*: The HTTP address of a `valori-node` (e.g., `http://127.0.0.1:3000`). If provided, returns a `SyncRemoteClient`.
- **`path`** *(str, optional)*: The local directory for database storage (used only if `remote` is None). Defaults to `./valori_db`.

### `AsyncValoricore(remote=None, path="./valori_db")` -> `AsyncMemoryClient` | `AsyncRemoteClient`
Asynchronous factory. Recommended for **FastAPI**, **Starlette**, and high-concurrency applications.
- **`remote`** *(str, optional)*: If provided, returns an `AsyncRemoteClient`.
- **`path`** *(str, optional)*: The local directory for database storage. If `remote` is None, returns an `AsyncMemoryClient` (which wraps the FFI in a thread-shielded executor pool).

---

## 🧠 High-Level APIs (`MemoryClient`, `AsyncMemoryClient`)

These clients wrap the base clients to provide seamless handling of text, embeddings, and Knowledge Graph automated topology.

### `MemoryClient` / `AsyncMemoryClient`
**Methods:**
- **`add_document(text: str, embed: Callable[[str], List[float]], title: str = "") -> dict`**
  - **Description**: Chunks the text (currently by paragraph), embeds each chunk using the provided `embed` callable, and creates a `Document` Node in the Knowledge Graph connected to `Chunk` Nodes via `PARENT_OF` edges.
  - **Returns**: `{"document_node_id": int, "chunks_added": int}`

- **`semantic_search(query: str, embed: Callable[[str], List[float]], k: int = 5) -> List[dict]`**
  - **Description**: Embeds the query and performs a deterministic L2 nearest neighbor search.
  - **Returns**: A list of dictionaries: `[{"id": int, "score": float, "metadata": dict}]`.

- **`upsert_vector(vector: List[float], metadata: dict = None) -> int`**
  - **Description**: Directly inserts a vector and attaches the provided dictionary as JSON metadata.
  - **Returns**: The new `record_id`.

---

## 🔌 Base Clients (`LocalClient`, `SyncRemoteClient`, `AsyncRemoteClient`)

These clients expose the raw power of the Valori Kernel. All methods on `SyncRemoteClient` are identical to `LocalClient`. `AsyncRemoteClient` has the exact same methods, but they are `async`.

### Vector Operations
- **`insert(vector: List[float], tag: int = 0) -> int`**
  - Inserts a dense vector.
- **`insert_batch(vectors: List[List[float]]) -> List[int]`**
  - Inserts multiple vectors in a single transaction.
- **`insert_with_proof(vector: List[float], tag: int = 0) -> Tuple[int, str]`**
  - Inserts a vector and returns the `record_id` along with the cryptographically secure BLAKE3 Merkle Proof (hex string).
- **`delete(record_id: int) -> None`**
  - Permanently and physically removes a record from the vector index and the RecordPool.
- **`search(query_vector: List[float], k: int = 5, filter_tag: Optional[int] = None) -> List[dict]`**
  - Performs an exhaustive, deterministic L2 nearest neighbor search.

### Knowledge Graph Operations
- **`create_node(kind: int, record_id: Optional[int] = None) -> int`**
  - Creates a Node. `kind` is a user-defined integer representing the node type.
- **`create_edge(from_id: int, to_id: int, kind: int) -> int`**
  - Creates a directional Edge between two nodes.
- **`get_node(node_id: int) -> Optional[dict]`**
  - Retrieves a Node's structure (its `kind` and its underlying `record_id` if present).
- **`get_edges(node_id: int) -> List[dict]`**
  - Retrieves all outgoing edges for a node.
- **`expand(start_node: int, max_depth: int = 2) -> List[int]`**
  - Performs a breadth-first search across the Knowledge Graph starting from `start_node`. Returns a list of all unique `record_id`s found attached to any node in the traversal path.

### Snapshots & Audit Trails
- **`get_state_hash() -> str`**
  - Returns the 64-character BLAKE3 hex string representing the exact mathematical state of the entire database.
- **`snapshot(auto_interval: Optional[int] = None) -> None`**
  - Serializes the entire database state to disk.
- **`restore(snapshot_bytes: bytes) -> None`**
  - Completely replaces the current kernel state with the provided binary snapshot data.
- **`get_timeline() -> List[str]`**
  - Parses the immutable Event Log (Phase 23) and returns a human-readable chronological timeline of every transaction.

---

## 🔐 Cryptographic Helpers

These functions are completely decoupled from the database engine and can be run totally offline.

- **`ingest_embedding(floats: List[float]) -> List[int]`**
  - Converts a standard float array into Valoricore's internal deterministic Q16.16 fixed-point representation.
- **`generate_proof(fixed_values: List[int]) -> str`**
  - Generates the BLAKE3 Merkle root hex string for a given fixed-point array.
- **`verify_embedding(floats: List[float], claimed_hash: str) -> bool`**
  - Converts the floats to fixed-point, calculates the proof, and safely compares it against the `claimed_hash` to prove that the vector has not been tampered with.

---

## ⚠️ Exceptions

- **`ValoricoreError`**: Base exception class for all errors.
- **`ConnectionError`**: Thrown when the remote node is unreachable or drops the connection.
- **`ValidationError`**: Thrown when vector dimensions mismatch the initialized engine, or if floats exceed the fixed-point saturation boundaries (±32767.0).
- **`IntegrityError`**: Cryptographic proof verification failure.
- **`NotFoundError`**: The requested record, node, or edge ID does not exist in the active state.
