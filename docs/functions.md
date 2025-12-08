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
*   **Purpose**: Initializes a new Kernel with specified `IndexKind` and `QuantizationKind`.
*   **Analysis**: This is where dependency injection happens. The specific `VectorIndex` implementation (e.g., BruteForce) is selected here at compile time or runtime startup.

### API Endpoints
*   **`POST /v1/memory/upsert_vector`**:
    *   **Logic**:
        1. Insert Vector -> Get `RecordId`.
        2. Create `Document` Node (if not reusing).
        3. Create `Chunk` Node linked to Record.
        4. Create Edge `ParentOf` (Doc -> Chunk).
    *   **Atomicity**: Not fully atomic over HTTP (multiple kernel commands). Future work: Batched Commands.
*   **`POST /v1/memory/search_vector`**:
    *   **Logic**: Calls `search_l2` and formats results with `memory_id` (`rec:{id}`).

---

## 4. Python Client (Valori Memory Protocol)
**Location**: `python/valori/protocol.py` & `memory.py`

### `ProtocolClient.upsert_text(text, metadata)`
*   **Purpose**: High-level convenience for "RAG" (Retrieval Augmented Generation).
*   **Flow**:
    1. **Chunking**: Splits text into sentences/chunks (deterministic sliding window).
    2. **Embedding**: Calls user-provided `EmbedFn` to get vectors.
    3. **Upsert**: Calls `upsert_vector` for each chunk.
*   **Analysis**: This function orchestrates the "ETL" pipeline on the client side.

### `MemoryClient.search_memory(query_vector, k)`
*   **Purpose**: Type-safe wrapper around the HTTP API or FFI.
*   **Abstraction**: Hides whether the backend is local (FFI) or remote (HTTP). Returns standardized `MemorySearchHit` objects.

______________________________________________________________________
**Note on Performance vs. Correctness**:
Valori prioritizes **Correctness (Determinism)** > **Performance**.
*   All floating-point inputs are converted to Fixed-Point (Q16.16) at the boundary (Client/Node).
*   No floating-point math occurs inside the Kernel.
