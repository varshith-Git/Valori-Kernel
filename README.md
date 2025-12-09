# Valori Kernel

**The Deterministic Memory Engine for AI Agents.**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()

**Valori** is a high-performance, strictly deterministic vector database and knowledge graph designed for AI agents, robotics, and mission-critical memory systems. Unlike traditional vector stores, Valori guarantees **bit-identical state across any architecture** (x86, ARM, WASM), making it the only choice for verifiable and reproducible AI behaviors.

---

## 🚀 Why Valori?

*   **🧠 Total Recall**: Combines vector semantic search (RAG) with a structured Knowledge Graph in a single request.
*   **🛡️ Deterministic by Design**: Uses fixed-point arithmetic (Q16.16) to ensure calculations are identical on a MacBook, a Linux Server, or a Raspberry Pi. No floating-point drift.
*   **⚡ Local-First, Cloud-Ready**:
    *   **Embed it directly** in your Python process (via FFI) for microsecond latency.
    *   **Run it as a Service** (via HTTP) for distributed, scalable deployments.
*   **📦 Light & Portable**: Written in `no_std` Rust. Tiny binary size.

---

## 🛠️ Quick Start

Valori provides a unified Python SDK that works in both embedded and remote modes.

### 1. Installation
```bash
pip install values
# or build from source if pre-release
```

### 2. Local Mode (Embedded)
Ideal for single-agent applications, scripts, and testing. The database lives inside your process RAM.

```python
from valori import ProtocolClient

# Define your embedding logic (e.g. OpenAI, HuggingFace)
def my_embedder(text):
    # return model.encode(text) -> list[float]
    return [0.1] * 16  # Dummy example

# Initialize (No server needed!)
client = ProtocolClient(embed=my_embedder)

# Store Memory
client.upsert_text("Valori ensures my agent's memory is reproducible.")

# Recall
print(client.search_text("Why use Valori?"))
```

### 3. Remote Mode (Production)
Ideal for cloud backends, multi-agent systems, and web apps. Connects to a dedicated `valori-node` server.

**Step A: Start the Server**
```bash
# Production build
cargo build -p valori-node --release
./target/release/valori-node
# Listening on 127.0.0.1:3000...
```

**Step B: Connect via Client**
```python
# Just add the 'remote' URL!
client = ProtocolClient(embed=my_embedder, remote="http://127.0.0.1:3000")

## Development

### Prerequisites

- Rust (stable)

### Running Tests

This crate is `no_std` but allows `std` for testing purposes.

```bash
cargo test
```

### Building

To build the library:

```bash
cargo build --release
```

## Usage

*Note: This kernel is currently in active development. Usage API is subject to change.*

### Basic Fixed-Point Math

```rust
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::fxp::ops::{fxp_add, fxp_mul};

// In a no_std context:
let a = FxpScalar(1 << 16); // 1.0 in Q16.16
let b = FxpScalar(2 << 16); // 2.0 in Q16.16

let sum = fxp_add(a, b);
// sum represents 3.0
```

## How to decide when to use which

**Use Local / FFI mode when:**
*   You’re in a single Python process.
*   You want zero overhead.
*   You’re doing research, prototyping, agents, notebooks.

**Use Remote / Node mode when:**
*   Multiple services need shared memory.
*   You want to deploy Valori as infra.
*   You’re building a SaaS or team-wide memory system.
*   You care about scaling, auth, monitoring.

## Architecture Phases

1.  **Skeleton + FXP Core**: Basics of fixed-point math. (Completed)
2.  **Vectors + Math**: Vector operations (Dot product, L2).
3.  **Storage**: Static record pool.
4.  **Index**: Brute-force deterministic search.
5.  **Graph**: Knowledge graph with adjacency lists.
6.  **State Machine**: Command processing and state management.
7.  **Snapshot**: Serialization and restoration.

## License

Valori is open-source software licensed under the [GNU Affero General Public License v3.0 (AGPLv3)](LICENSE).

### Commercial License
For proprietary use, embedding in closed-source devices, or SaaS hosting without open-sourcing your stack, a Commercial License is available. See [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md) for details.
