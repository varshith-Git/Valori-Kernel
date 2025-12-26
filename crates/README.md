# Valori: The Flight Recorder for AI Memory

**Version:** 0.1.0-mvp | **License:** MIT | **Status:** Production Ready (Phase 9)

> "The only vector database that guarantees your AI behaves exactly the same way today as it did yesterday."

**Valori** is a **deterministic, forensic AI substrate**. Unlike standard vector databases (Pinecone, Qdrant) which prioritize speed and fuzzy search, Valori prioritizes **Truth** and **Reproducibility**. It captures the entire evolution of your AI's memory, allowing you to rewind time, replay decisions, and prove exactly why your agent's behavior changed.

---

## 🎯 Why Valori?

The modern AI stack is built on **Probabilistic Foundations** (Float32, Random Seeds, Approximate Nearest Neighbor). This makes it impossible to audit.

If your autonomous agent, trading bot, or retrieval system makes a different decision today than it did yesterday, you cannot know *why*. Was it the model? Was it a new vector? Was it a race condition in the database?

**Valori solves this by enforcing strict determinism:**
*   **Bit-for-Bit Reproducibility:** `Insert A` -> `Delete A` results in the exact same state as the beginning.
*   **Deterministic Math:** Uses Q16.16 Fixed Point arithmetic instead of floating point. `1.0` is always `1.0`.
*   **Proven Topology:** Uses a deterministic HNSW graph structure derived from data entropy, not random seeds.

---

## 🚀 Quick Start

### Installation

**From Source:**
```bash
# Clone the repo
git clone https://github.com/your-org/valori.git
cd valori

# Build the CLI
cargo install --path crates/cli

# Verify Installation
valori --version
```

### Basic Workflow

In this example, we simulate an AI system inserting memory vectors, and then perform a forensic investigation.

**1. Create a Database (Mock)**
*Assume you have a directory `data/` with `snapshot.val`, `events.log`, and `metadata.idx`.*

**2. Inspect the State**
```bash
valori inspect --dir ./data
```
*Output:*
```text
╔════════════════════════════════════════════╗
║         VALORI FORENSIC CLI v0.1.0-mvp        ║
╚════════════════════════════════════════════╝

Valori Status Report
--------------------
File      | Status | Details
----------|---------|------------------------------------------------
Snapshot | FOUND   | Format: V1, Magic: VALO, Ver: 1, Idx: 100
WAL       | FOUND   | 105 events
Index     | FOUND   | 3 labeled entries
```

**3. Rewind Time (Replay)**
Fast-forward the database to a specific point in the event log to see what the state looked like then.
```bash
valori replay-query --dir ./data --at 102 --query "[10, 20, 30]"
```

**4. The "Money" Feature: Semantic Diff**
Compare the search results between two different time points.
*Did a new vector enter the Top 10? Did the ranking shift?*
```bash
valori diff --dir ./data --from 100 --to 105 --query "[10, 20, 30]"
```
*Output:*
```text
State Comparison
----------------
Property     | Value
-------------|------------------
From Index   | 100
From Hash    | 0x1a2b3c...
To Index     | 105
To Hash      | 0x9f8e7d...
Status       | DRIFTED

Semantic Diff (Top-5)
--------------------
ID   | Change          | Detail
-----|-----------------|----------------------------------
102  | ~ Rank Change   | 1 -> 3
105  | + Entered Top-5 | Rank 4
```

---

## 🛠️ The Architecture

Valori is not a monolithic server. It is a **Workspace of Crates**:

### 1. `valori-kernel` (The Brain)
The `no_std` pure Rust library containing the AI logic.
*   **Math:** Q16.16 Fixed Point Arithmetic.
*   **Index:** Deterministic HNSW (Graph Structure).
*   **State:** `BTreeMap` storage for determinism.
*   **Philosophy:** Zero heap allocators (optional), zero floating points.

### 2. `valori-persistence` (The Storage)
The binary format layer.
*   **Format:** `snapshot.val` (Graph Topology) + `events.log` (Append-Only).
*   **Integrity:** CRC64 Checksums on every byte. Fail-closed validation.

### 3. `valori-cli` (The Flight Recorder Interface)
The command-line tool for engineers.
*   **Offline Forensics:** Reads disk directly. No daemon required.
*   **Time Travel:** `replay`, `diff`, `verify`.

---

## 📚 Commands Reference

### `valori inspect`
Inspect the health and metadata of a database volume.
*   **Usage:** `valori inspect --dir <path>`
*   **Output:** Snapshot version, WAL event counts, Integrity status.

### `valori verify`
Cryptographically verify a snapshot file.
*   **Usage:** `valori verify snapshot.val`
*   **Output:** `✅ VERIFIED` or `❌ CORRUPTED`.
*   **Use Case:** Validating backups before an incident response.

### `valori timeline`
List labeled checkpoints in the event log.
*   **Usage:** `valori timeline metadata.idx`
*   **Output:** Human-readable timeline of `ingest:batch_01`, `experiment:v2`, etc.

### `valori replay-query`
Replay the WAL to a specific event ID and execute a search.
*   **Usage:** `valori replay-query --at <event_id> --query "[...]"`
*   **Use Case:** "What did the top-5 neighbors look like *right before* the crash?"

### `valori diff`
Compare search results (Topology) between two points in time.
*   **Usage:** `valori diff --from <id_a> --to <id_b> --query "[...]"`
*   **Output:** Delta of neighbors (+ Entry, - Exit, ~ Rank Shift).

---

## 🧬 Technical Specifications

### Deterministic Math
Valori uses **Q16.16 Fixed Point** arithmetic instead of IEEE 754 Float32.
*   **Range:** [-32768.0, 32767.99998]
*   **Behavior:** No NaN, no Infinity, no `1.0 + 2.0 != 2.0 + 1.0`.
*   **Overflow:** Hard failure (Clamped/Rejected) rather than silent wrapping.

### Deterministic HNSW
The graph index is not stochastic.
*   **Entry Points:** Derived from `trailing_zeros(hash(id))`, creating a natural geometric distribution without RNG.
*   **Neighbor Selection:** Strict `Distance ASC -> ID ASC` sorting.
*   **Result:** The graph structure on an x86 server is **identical** to the graph on an ARM microcontroller.

### Serialization Format
*   **Header:** `VALO` + Version + EventIndex + Timestamp.
*   **Body:** Vectors + Graph Topology (Layers, Neighbors).
*   **Verification:** Body checksums must match header checksums.

---

## 🚧 Development

**Testing:**
```bash
# Run all unit and integration tests
cargo test --workspace

# Run with output
cargo test --workspace -- --nocapture
```

**Build:**
```bash
# Release build (optimized)
cargo build --release
```

---

## 🗺️ Roadmap

*   **v0.1.0 (Current):** MVP Release. CLI, Deterministic Kernel, Snapshotting.
*   **v0.2.0:** Performance Tuning. Neighbor Pruning, `ef_search` optimization.
*   **v0.3.0:** `valori-node`. HTTP Server & Network Layer.
*   **v0.4.0:** Distributed Consensus. "God Mode" state sync across nodes.

---

## ⚖️ License

MIT License - See LICENSE file for details.

**Valori.** *Operate on Truth.*
