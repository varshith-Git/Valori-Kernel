# Valori Kernel

**The Deterministic Memory Engine for AI Agents.**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()

**Valori** is a `no_std` Rust kernel providing a strictly deterministic vector database and knowledge graph. It guarantees **bit-identical state across any architecture** (x86, ARM, WASM), enabling verifiable and reproducible AI memory.

---

## ⚡ Technical Highlights

### 1. Bit-Identical Determinism
Unlike standard vector stores using `f32` (which varies by CPU/Compiler), Valori uses a custom **Q16.16 Fixed-Point Arithmetic** engine.
- **Guarantee**: `State + Command_Log = Hash` is identical on a MacBook `M3`, `Intel` Server, or `WASM` runtime.
- **Safety**: Inputs are strictly validated to `[-32768.0, 32767.0]` to prevent overflow.

### 2. Hybrid-Native Architecture
One kernel, two modes of operation:
- **Embedded (FFI)**: Links directly into your Python process via `pyo3`. Microsecond latency, zero network overhead.
- **Remote (Node)**: The exact same kernel wrapped in `axum`/`tokio` for horizontal scaling.
- **Transition**: Move from local dev to distributed prod by changing **1 line of code**.

### 3. "Git for Memory"
- **Atomic Snapshots**: State is serialized into a verifiable format: `[Header][Kernel][Meta][Index]`.
- **Instant Restore**: Checkpoint low-level state and restore instantly.

---

## 🚀 Quick Start

### Installation

```bash
pip install valori
```

### Mode A: Embedded (Local Research/Dev)
**Transport: FFI (Zero-Copy Memory Access)**
Runs inside your Python process. No server required.

```python
from valori import ProtocolClient

# 1. Initialize (Zero-config)
client = ProtocolClient(embed=my_embedding_fn)

# 2. Upsert (Text -> Chunk -> Embed -> Store)
# Automatically handles chunking and linking nodes.
local_ref = client.upsert_text(
    "Valori uses Q16.16 fixed-point math for determinism.",
    metadata={"source": "readme"}
)

# 3. Search
print(client.search_text("Why is it deterministic?"))
```

### Mode B: Remote (Production/Cloud)
**Transport: HTTP/HTTPS (JSON over TCP)**
Connects to a high-performance Rust server (`valori-node`).

**1. Start the Server**
```bash
# Optimized Release Build
cargo run --release -p valori-node
# > Listening on 0.0.0.0:3000
```

**2. Client Connection**
```python
# exact same API, just add 'remote' URL
# Local Dev:
client = ProtocolClient(embed=my_embedder, remote="http://localhost:3000")

# Production (HTTPS supported!):
client = ProtocolClient(embed=my_embedder, remote="https://testing.com")

# All operations form JSON-RPC calls automatically
client.upsert_text("This data lives in the cloud now.")
```

---

## 🛠️ Architecture

```mermaid
graph LR
    Kernel[Core Kernel<br/>(no_std Rust)] -->|FFI| Python[Local Python]
    Kernel -->|Axum| Node[Valori Node]
    
    subgraph "Deterministic Core"
        BF[BruteForce Index]
        FXP[Q16.16 Math]
        Graph[Knowledge Graph]
    end
    
    Kernel --- BF
    Kernel --- FXP
    Kernel --- Graph
```

**Core Components:**
- **`valori-kernel`**: The pure state machine. No IO, No Alloc (mostly).
- **`valori-node`**: HTTP Service layer with Persistence (Disk/S3) and HNSW Indexing.
- **`valori` (Python)**: Unified Client implementing the `Memory Protocol`.

## 📦 Performance

- **Latencies**: `<500µs` for raw vector search (Local Mode).
- **Throughput**: Handles thousands of concurrent readers in Node mode (Tokio async).
- **Size**: Core kernel compiles to `<1MB`.

---

## License
**AGPLv3**. [Read more](LICENSE).
For commercial use, embedding in proprietary devices, or managed hosting, contact us for a [Commercial License](COMMERCIAL_LICENSE.md).
