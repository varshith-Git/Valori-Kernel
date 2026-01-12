# Valori FFI (Python Bindings)

This crate provides the **Python Foreign Function Interface (FFI)** for Valori.
It wraps the Rust `valori-kernel` and exposed functionality using `PyO3`, allowing Python code to interact with the deterministic core directly, without going through HTTP.

## 🏗 Architecture
*   **Type**: `cdylib` (Shared Library)
*   **Bridge**: `PyO3` (Rust <-> Python)
*   **Consumers**: The `valori` Python package (located in `../python/`).

## 🚀 Key Features
1.  **Direct In-Process Access**: Python interacts with Rust memory directly. No network latency.
2.  **ValoriEngine**: Exposes the `ConcreteEngine` (Kernel) as a Python Class.
3.  **Zero-Copy Optimizations**: Where possible, data is shared between Rust and Python without copying.

## 🛠 Build & Development
This crate is usually built automatically by `maturin` or `setuptools-rust` when installing the Python package.

### Manual Build
```bash
# Build the shared library (e.g., .so or .dylib)
cargo build --release --lib
```

The output will be in `../target/release/libvalori_ffi.dylib` (macOS) or `.so` (Linux).


## 📚 API Reference
These methods are methods of the `ValoriEngine` class exposed to Python.

### `__new__(path: str)`
Creates a new Valori Engine instance.
*   **path**: Directory where WAL/Logs will be stored (e.g., `./valori_db`).

### `insert(vector: list[float], tag: int) -> int`
Inserts a new vector.
*   **vector**: List of floats (Length must match `D=384`).
*   **tag**: 64-bit integer tag for filtering.
*   **Returns**: New Record ID (integer).

### `search(vector: list[float], k: int, filter_tag: int = None) -> list[tuple]`
Performs an L2 vector search.
*   **vector**: Query vector.
*   **k**: Number of results to return.
*   **filter_tag**: Optional tag to filter by.
*   **Returns**: List of tuples `(record_id, score)`.

### `create_node(kind: int, record_id: int = None) -> int`
Creates a graph node (e.g., DOCUMENT, CHUNK).
*   **kind**: Enum integer (1=Record, 2=Document, 3=Chunk).
*   **record_id**: Optional ID of the vector record this node effectively points to.
*   **Returns**: New Node ID.

### `create_edge(from_id: int, to_id: int, kind: int) -> int`
Creates a relationship between two nodes.
*   **from_id**: Source Node ID.
*   **to_id**: Target Node ID.
*   **kind**: Enum integer (1=ParentOf, 2=RefersTo).
*   **Returns**: New Edge ID.

### `save() -> str`
Saves a snapshot of the current in-memory state to disk.
*   **Returns**: Path to the saved snapshot file.

### `insert_batch(vectors: list[list[float]]) -> list[int]`
Atomically insert multiple vectors.
*   **vectors**: List of float lists.
*   **Returns**: List of assigned Record IDs.

### `get_metadata(record_id: int) -> bytes | None`
Get metadata for a record.

### `set_metadata(record_id: int, metadata: bytes)`
Set metadata for a record.

### `get_state_hash() -> str`
Get the cryptographic state hash (BLAKE3) of the kernel. Used for verifiable crash recovery.

### `record_count() -> int`
Get the total number of records.

### `restore(data: bytes)`
Restore kernel state from snapshot bytes.

### `soft_delete(record_id: int)`
Mark a record as deleted (tombstone). Excludes it from search results.
