# Valori Function Reference & Deep Analysis

This document provides a deep dive into the core functions of the Valori system across its three layers: **Kernel (Rust)**, **Node (Service)**, and **Client (Python)**.

## 1. Core Kernel (Rust)
**Location**: `valori-kernel/src/state/kernel.rs`

The Kernel is the deterministic heart of the system. It is single-threaded, `no_std` compatible, and operates on fixed-point arithmetic.

### `KernelState::apply(&mut self, cmd: &Command)`
*   **Purpose**: The **only** way to mutate state. Transitions the system from `State_N` to `State_{N+1}` deterministically.
*   **Complexity**:
    *   `InsertRecord`: O(1) for storage + O(log N) or O(1) for Index update (depends on Index implementation).
    *   `DeleteRecord`: O(1).
    *   `CreateNode`/`CreateEdge`: O(1) (amortized).
*   **Determinism**: Guaranteed. Bit-identical results for the same sequence of commands.
*   **Side Effects**: Updates `version`, `records`, `graph`, and the `index`.

### `KernelState::search_l2(&self, query: &FxpVector, results: &mut [SearchResult]) -> usize`
*   **Purpose**: Performs a k-nearest neighbor search using L2 squared distance.
*   **Behavior**: Delegates to the configured `VectorIndex`.
*   **Default Implementation (`BruteForceIndex`)**:
    *   **Complexity**: O(N * D), where N is active records, D is dimensions.
    *   **Determinism**: Uses stable sorting by Score (primary) and RecordID (secondary) to break ties deterministically.

### `KernelState::snapshot(&self, buf: &mut [u8]) -> usize`
*   **Purpose**: Serializes the entire state (Records + Graph + Index + Version) into a binary buffer.
*   **Format**: Custom compact binary format (not standard serialization like JSON/Protobuf) for maximum density and speed.
*   **Complexity**: O(State Size).

---

## 2. Abstractions (Traits)
**Location**: `valori-kernel/src/index/mod.rs` & `src/quant/mod.rs`

### `VectorIndex` (Trait)
Defines how vectors are indexed for search.
*   **Functions**:
    *   `on_insert(id, vec)`: Hook called after storage insert.
    *   `on_delete(id)`: Hook called after storage delete.
    *   `search(pool, query, results)`: Execute retrieval.

### `Quantizer` (Trait)
Defines how vectors are compressed (lossy compression) to save memory/bandwidth.
*   **Functions**:
    *   `encode(vec) -> Code`: Compress a high-precision FXP vector.
    *   `decode(code) -> FxpVector`: Reconstruct approximation.

---

## 3. Valori Node (HTTP Engine)
**Location**: `valori-node/src/engine.rs` & `server.rs`

Wraps the Kernel in a `tokio` async runtime. Use `Arc<Mutex<Engine>>` for sharing.

### `Engine::new(config: NodeConfig)`
*   **Purpose**: Initializes a new Kernel with specified `IndexKind` (e.g., `Hnsw`) and `QuantizationKind`.
*   **Analysis**: This is where dependency injection happens. The specific `VectorIndex` implementation (e.g., BruteForce, HNSW) is selected here at compile time or runtime startup.

### `Engine::snapshot() -> Vec<u8>` & `restore(&[u8])`
*   **Purpose**: Manages the lifecycle of the entire system state.
*   **Behavior (Checkpointing)**:
    *   **Format**: Multipart binary (`[Header][Meta][Kernel][Metadata][Index][CRC]`).
    *   **HNSW**: Uses deterministic serialization (sorting internal HashMaps) to ensure bit-identical snapshots.
    *   **Safety**: Validates bounds and checksums before loading.
    *   **Fallback**: If the Index blob is missing or incompatible, the Engine rebuilds the index from the Kernel records.

### API Endpoints

#### Memory Protocol (V0)
*   **`POST /v1/memory/upsert_vector`**:
    *   **Logic**:
        1. Insert Vector -> Get `RecordId`.
        2. Create `Document` Node (if not reusing).
        3. Create `Chunk` Node linked to Record.
        4. Create Edge `ParentOf` (Doc -> Chunk).
    *   **Atomicity**: Not fully atomic over HTTP (multiple kernel commands). Future work: Batched Commands.
*   **`POST /v1/memory/search_vector`**:
    *   **Logic**: Calls `search_l2` and formats results with `memory_id` (`rec:{id}`).

#### Metadata (V1)
*   **`POST /v1/memory/meta/set`**: Key-Value metadata storage separate from the graph.
*   **`GET /v1/memory/meta/get`**: Retrieve metadata by ID.

#### Admin / Snapshot (V1)
*   **`POST /v1/snapshot/save`**: Trigger a manual snapshot to the configured path. Supports rotation (keeps `.prev`).
*   **`POST /v1/snapshot/restore`**: Load state from a specified file path. (Warning: Overwrites current state).

---

## 4. Python Client (Valori Memory Protocol)
**Location**: `python/valori/protocol.py` & `memory.py`

### `ProtocolClient` (Facade)
*   **Purpose**: The main entry point for developers. Handles the choice between **Local** and **Remote** execution.
*   **Initialization**: `ProtocolClient(remote="...")`
    *   If `remote` is None: Instantiates `MemoryClient` (FFI).
    *   If `remote` is URL: Instantiates `ProtocolRemoteClient` (HTTP).

#### `upsert_text(text, metadata)`
*   **Logic**:
    1. **Chunking**: Splits text into chunks locally (client-side).
    2. **Embedding**: Runs `EmbedFn` locally.
    3. **Transport**:
    *   **Local**: Direct memory write.
    *   **Remote**: Sends `POST /v1/memory/upsert_vector`.
    4. **Metadata**: Calls `set_metadata` automatically if metadata is provided.

#### `snapshot() / restore(bytes)`
*   **Local**: Calls Rust `KernelState::snapshot` directly.
*   **Remote**:
    *   `snapshot()`: Downloads the full binary DB from `POST /snapshot`.
    *   `restore()`: Uploads a binary blob to `POST /restore`.
*   **Use Case**: Migrating state from a local development environment to a production server, or for backups.

______________________________________________________________________
**Note on Performance vs. Correctness**:
Valori prioritizes **Correctness (Determinism)** > **Performance**.
*   **Fixed-Point**: All floating-point inputs are converted to Fixed-Point (Q16.16).
*   **Deterministic Indexing**: Even complex structures like HNSW are implemented to be bit-exact reproducible, sacrificing some parallelism for consistency if necessary (though current implementation is single-threaded).
