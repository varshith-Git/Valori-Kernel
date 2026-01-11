# Valori Kernel Core

The **Valori Kernel** is the deterministic, event-sourced heart of the system.
It is a `no_std` compatible Rust library that manages vector storage, graph relationships, and indexing.

## 🏛 Architecture

### 1. Event Sourcing (`event/`)
All state mutations happen via **Kernel Events**.
*   `InsertRecord`: Adds a vector.
*   `DeleteRecord`: Soft-deletes a record (tombstone).
*   `CreateNode` / `CreateEdge`: Modifies the semantic graph.
*   **Determinism**: Replaying the same sequence of events **always** results in the exact same state hash.

### 2. Fixed-Point Math (`fxp/`)
Floating point math is non-deterministic across architectures (x86 vs ARM vs WASM).
Valori uses **Q16.16 Fixed Point** (`i32` wrapper) for all vector operations to ensure bit-perfect consistency everywhere.

### 3. State Machine (`state/`)
The `KernelState` struct holds the in-memory database.
*   **Records**: Dense storage of vectors.
*   **Graph**: Adjacency list for Nodes and Edges.
*   **Index**: Pluggable vector index (BruteForce or HNSW).

## 🧩 Modules
*   **`kernel.rs`**: The main entry point (`ValoriKernel`).
*   **`index/`**: Vector indexing algorithms.
*   **`storage/`**: Memory pools for records and nodes.
*   **`proof/`**: Merkle tree and hashing for state verification.
*   **`replay/`**: Logic to rebuild state from an Event Log.

## 🛠 Key Functions

### `ValoriKernel`
*   `apply_event(event)`: The **ONLY** way to mutate state.
*   `search(query, k)`: Read-only index query.
*   `snapshot()`: Serializes the entire state to a binary blob.
*   `restore(data)`: Replaces state from a binary blob.

## ⚠️ Professional Requirements
1.  **NO Floating Point**: Do not use `f32` or `f64` in core logic. Use `FxpScalar`.
2.  **NO System Time**: Time logic must be supplied externally via events.
3.  **NO Randomness**: Use deterministic PRNG seeded from the Event Log if needed.
4.  **Crash Safety**: The Kernel relies on the `Application Layer` (Node) to persist the Event Log (WAL).
