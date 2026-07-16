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

### Node Health

- **`health() -> str`**
  - Returns the node health status string, e.g. `"ok"`.
  - Useful for liveness checks before starting inserts.

  ```python
  assert client.health() == "ok"
  ```

### Vector Operations

- **`insert(vector: List[float], tag: int = 0, collection: str = "default", text: Optional[str] = None, idempotency_key: Optional[bytes] = None) -> int`**
  - Inserts a dense vector and returns the new `record_id`.
  - **`text`** *(str, optional)*: Raw text to index alongside the vector. When provided, the server tokenises and stores it so that future `search()` calls with `query_text` can re-score results by term frequency (Valori Reranker, Phase C5). Pass the section title + body for document chunks.

  ```python
  rid = client.insert([0.1, 0.2, 0.3])
  rid = client.insert([0.1, 0.2, 0.3], text="Section 3.1 Training — AdamW optimizer lr=1e-4")
  rid = client.insert([0.1, 0.2, 0.3], collection="my-tenant", text="chunk body")
  ```

- **`insert_batch(batch: List[List[float]], collection: str = "default", metadata: Optional[List[Optional[str]]] = None, request_ids: Optional[List[Optional[str]]] = None, texts: Optional[List[Optional[str]]] = None) -> List[int]`**
  - Inserts multiple vectors in one round-trip. Returns a list of `record_id`s in insertion order.
  - **`texts`** *(List[str | None], optional)*: Per-vector text strings for the Valori Reranker. Must be the same length as `batch`. Use `None` entries to skip indexing for specific vectors.
  - **`metadata`**: Per-vector JSON strings committed into the BLAKE3 audit chain.
  - **`request_ids`**: Per-vector idempotency keys (hex strings) — a duplicate key is skipped and the existing ID is returned.

  ```python
  ids = client.insert_batch(vectors)
  ids = client.insert_batch(
      vectors,
      texts=["Section 3.1 Training", "Section 4.2 Agent Behavior", None],
      collection="composer2",
  )
  ```

- **`insert_with_proof(vector: List[float], tag: int = 0, collection: str = "default") -> Tuple[int, bytes]`**
  - Inserts a vector and returns `(record_id, proof_bytes)` — the BLAKE3 Merkle proof for the vector.

- **`soft_delete(record_id: int, collection: str = "default") -> None`**
  - Marks a record inactive without physically removing it. The record is excluded from search results and its text is removed from the Valori Reranker index.

- **`delete(record_id: int, collection: str = "default") -> None`**
  - Permanently removes a record from the vector index and the RecordPool.
  - **`collection`**: record ids are only unique within their own collection (each collection's data may live on its own shard in cluster mode) — pass the same `collection` the record was inserted into.

- **`search(query: List[float], k: int, filter_tag: Optional[int] = None, consistency: Optional[str] = None, collection: str = "default", as_of: Optional[str] = None, as_of_log_index: Optional[int] = None, decay_half_life_secs: Optional[int] = None, rerank: bool = True, query_text: Optional[str] = None) -> List[dict]`**
  - K-nearest-neighbour search. Returns `[{"id": int, "score": float}, ...]`.
  - **`rerank`** *(bool, default `True`)*: Enable the Valori Reranker. When `True` and `query_text` is set, the server fetches a wider candidate pool and re-ranks by a blend of vector similarity + term-frequency score before returning the top-k. Set to `False` for pure vector ranking.
  - **`query_text`** *(str, optional)*: The human-readable query string used for term-frequency scoring. Required for the Valori Reranker to activate. Pass the same string you would show to the user.
  - **`decay_half_life_secs`** *(int, optional)*: Recency-aware ranking (Phase C4.1). A record one half-life old has its distance doubled, so fresh near-matches rise above stale ones. Each hit gains `decay_factor` and `age_secs` fields. Ignored for `as_of` queries.
  - **`consistency`** *(str, optional)*: Cluster mode only — `"linearizable"` (default, reads through the leader) or `"local"` (fast, may lag).
  - **`as_of`** *(str, optional)*: ISO 8601 UTC timestamp — search the vector state as it existed at that moment. Returns the full response dict including `as_of_log_index`, `as_of_timestamp_iso`, `as_of_state_hash`.
  - **`as_of_log_index`** *(int, optional)*: Search after exactly this many committed events. Takes precedence over `as_of`.

  ```python
  # Pure vector search
  hits = client.search(query_vec, k=5)

  # Valori Reranker — hybrid vector + term-frequency (recommended for document RAG)
  hits = client.search(query_vec, k=5, query_text="what optimizer is used?")
  # → re-ranks top-20 vector candidates by term frequency, returns best 5

  # Disable reranker explicitly
  hits = client.search(query_vec, k=5, rerank=False)

  # Recency-aware
  hits = client.search(query_vec, k=5, query_text="optimizer", decay_half_life_secs=86400)

  # Point-in-time
  resp = client.search(query_vec, k=5, as_of="2026-01-01T00:00:00Z")
  # → {"results": [...], "as_of_log_index": 42, "as_of_state_hash": "..."}
  ```

### Knowledge Graph — Fluent API *(recommended)*

These methods return Python **`Node` objects** instead of raw integers, so you never need to
track IDs manually.

- **`node(kind: int, vector: List[float] = None, tag: int = 0) -> Node`**
  - One-liner: optionally insert the vector, create the node, and return a `Node` object.
    Replaces the previous `insert` + `create_node` two-step.
- **`edge(from_node, to_node, kind: int) -> int`**
  - Create a directed edge. Both `Node` objects and raw `int` IDs are accepted.
- **`build_document(title: str = None) -> DocumentGraph`**
  - Returns a context manager. Inside the `with` block call `builder.add_chunk(vector)` for
    each chunk — it inserts the vector, creates a `NODE_CHUNK` node, and wires a
    `EDGE_PARENT_OF` edge automatically.

#### `Node` — the object returned by `db.node()`

| Attribute | Type | Description |
|---|---|---|
| `node.id` | `int` | Raw integer node ID (low-level escape hatch) |
| `node.kind` | `int` | Node kind (matches `NODE_*` constants) |
| `node.record_id` | `int \| None` | Attached vector record ID, or `None` |

| Method | Returns | Description |
|---|---|---|
| `node.link_to(other, edge_kind)` | `self` | Create edge(s) from this node. `other` can be a `Node`, `int`, or list of either. |
| `node.link_from(other, edge_kind)` | `self` | Create edge from `other` into this node. |
| `node.children(edge_kind=None)` | `List[Node]` | Outgoing neighbours, optionally filtered by edge kind. |
| `node.walk(max_depth=2)` | `List[Node]` | BFS traversal; returns visited nodes as `Node` objects. |
| `node.record_ids(max_depth=2)` | `List[int]` | All reachable vector record IDs (use with `search()` for RAG). |
| `node.delete()` | `None` | Cascade-delete this node and all its incident edges. |
| `int(node)` | `int` | Escape hatch — retrieve the raw integer ID. |

#### `DocumentGraph` — the context manager returned by `build_document()`

| Attribute / Method | Description |
|---|---|
| `builder.add_chunk(vector, tag=0, metadata=None)` | Insert vector, create `NODE_CHUNK`, wire `EDGE_PARENT_OF`. Returns the new `Node`. |
| `builder.document` | The root `NODE_DOCUMENT` `Node`. |
| `builder.chunks` | Ordered list of chunk `Node` objects. |
| `builder.record_ids` | List of vector record IDs in insertion order. |

### Knowledge Graph — Low-Level API *(still fully supported)*

`collection` on every method below defaults to `"default"`. It is
**remote-client-only** (`SyncRemoteClient`/`AsyncRemoteClient`) — the
embedded `LocalClient` is single-tenant and has no collection concept, so
its graph methods take no `collection` parameter at all. Node/edge ids are
only unique within their own collection (each collection's data may live
on its own shard in cluster mode), so always pass the same `collection`
a node/edge was created in when looking it up again.

- **`create_node(kind: int, record_id: Optional[int] = None, collection: str = "default") -> int`**
  - Creates a Node. Returns a raw integer node ID.
- **`create_edge(from_id: int, to_id: int, kind: int, collection: str = "default") -> int`**
  - Creates a directional Edge. Returns a raw integer edge ID.
- **`delete_node(node_id: int) -> None`**
  - Cascade-deletes a node and all its incident edges.
- **`delete_edge(edge_id: int) -> None`**
  - Deletes a single directed edge.
- **`get_node(node_id: int, collection: str = "default") -> Optional[dict]`**
  - Returns `{"kind": int, "record_id": int | None}`.
- **`get_edges(node_id: int, collection: str = "default") -> List[dict]`**
  - Returns `[{"edge_id": int, "to_node": int, "kind": int}, …]`.
- **`neighbors(node_id: int, collection: str = "default") -> List[int]`**
  - Returns the raw `to_node` ids from `get_edges()` — the immediate neighbours.
- **`walk(start_node: int, max_depth: int = 2, collection: str = "default") -> List[int]`**
  - Client-side BFS traversal (one `get_edges()` round trip per node); returns integer node IDs.
- **`expand(start_node: int, max_depth: int = 2, collection: str = "default") -> List[int]`**
  - Client-side BFS via `walk()`; returns all reachable vector record IDs.
- **`subgraph(root_node: int, depth: int = 2, collection: str = "default") -> dict`** *(remote client only)*
  - Server-side bounded BFS from `root_node` (depth capped at 4 server-side) — one round trip
    instead of `walk()`'s N round trips. Returns `{"nodes": [...], "edges": [...]}` where each
    node has `id`, `kind`, `record`, and each edge has `id`, `from`, `to`, `kind`.

  ```python
  # Everything below targets a named collection instead of "default".
  n1 = client.create_node(kind=NODE_DOCUMENT, collection="tenant-acme")
  n2 = client.create_node(kind=NODE_CHUNK, collection="tenant-acme")
  client.create_edge(n1, n2, kind=EDGE_PARENT_OF, collection="tenant-acme")

  client.get_node(n1, collection="tenant-acme")
  client.subgraph(n1, depth=2, collection="tenant-acme")
  ```

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
