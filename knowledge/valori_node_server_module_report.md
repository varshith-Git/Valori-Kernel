# Valori Kernel: Module Analysis - Node Server & Orchestration

Having explored the deterministic inner workings of the `valori-kernel`, we now step outwards into the **Node Server Layer** (`node/src/`). 

While the kernel is a pure, stateless mathematical engine that knows nothing about the outside world, the `valori_node` crate acts as its container. It provides the HTTP API, background asynchronous tasks, disk persistence, and network replication.

---

## 1. The Orchestrator: `Engine`

**Location**: `node/src/engine.rs`

The `Engine` struct is the primary wrapper around the `KernelState`. It translates high-level requests into kernel mutations.
- **Components**: It holds the `KernelState`, a `Quantizer` (like Scalar or PQ compression), the `EventCommitter` (for writing to the WAL/Event log), and an Index abstraction (like HNSW or IVF) which provides faster approximate searches than the kernel's native Brute Force.
- **The Commit Pipeline**: When a `Command` is received, the Engine executes the 4-step durability pipeline:
  1. Synchronously `fsync`s the event to the disk event log.
  2. Applies the event to a clone of the state (shadow verification).
  3. If verification passes, commits it to the live memory state.
  4. Updates the HNSW search index.

---

## 2. Server Routing & HTTP API

**Location**: `node/src/server.rs` and `node/src/main.rs`

Valori uses `tokio` for async execution and `axum` for HTTP routing.
- **Shared State**: The entire `Engine` is wrapped in an `Arc<Mutex<Engine>>` (`SharedEngine`), making it accessible safely across asynchronous HTTP workers.
- **Endpoints**: Exposes REST endpoints to translate HTTP JSON into binary `KernelEvents`:
  - `POST /insert`: Submits vector embeddings.
  - `POST /search`: Queries the HNSW index.
  - `GET /proof`: Retrieves the cryptographic BLAKE3 receipt of the current kernel state.
  - `GET /snapshot`: Downloads the binary memory dump.

---

## 3. Crash Recovery

**Location**: `node/src/recovery.rs`

When the server starts (`main.rs`), it must recreate the memory state deterministically.
1. **Snapshot Loading**: It looks for a `snapshot.bin` file. If found, it reads the bytes and passes them to `decode_state` (bypassing the need to replay all historical events).
2. **Event Replay (`recover_from_events`)**: If there are events in the `.log` file that occurred *after* the snapshot was taken, it streams them, converting each binary payload into a `Command`, and re-applying them to the memory pools sequentially.
3. **Snapshot Validation**: It can take the replayed state, hash it, and compare it against the snapshot's recorded hash to verify no disk corruption occurred.

---

## 4. Network Replication (Leader/Follower)

**Location**: `node/src/replication.rs`

Valori implements a Master-Slave replication topology designed for distributed reading and failover.

### `run_follower_loop(state, leader_url)`
If a node is started in Follower mode, it spawns a background tokio thread that actively polls the Leader.
1. **Hash Synchronization**: Every 5 seconds, the follower asks the Leader for its `/proof`. It compares the Leader's `final_state_hash` with its own local hash.
2. **Streaming Events**: If the hashes diverge, the follower connects to the Leader's event stream. The leader pushes any new `LogEntry::Event` over the network as newline-delimited JSON.
3. **Commit & Apply**: The follower writes these incoming events to its own WAL and applies them to its `KernelState`, mathematically catching up to the Leader.
4. **Bootstrapping**: If the follower is completely empty, it calls `bootstrap_from_leader`. It downloads the entire binary snapshot from the Leader, overwrites its memory, wipes its WAL, and writes a `Checkpoint` to start fresh from the Leader's height.

---

### Summary of Module Capabilities
1. **Separation of Concerns**: The network layer is entirely decoupled from the mathematical layer. `axum` handles TCP/HTTP, but the actual state transitions are executed by the `no_std` kernel.
2. **Eventual Consistency**: Replication relies heavily on the `final_state_hash`. Because the engine is strictly deterministic, if the Follower's hash matches the Leader's hash, it is mathematically guaranteed that their internal Memory Pools, HNSW indices, and Graph structures are identical down to the byte.
