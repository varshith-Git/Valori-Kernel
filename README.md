# Valori Kernel

**The Deterministic Memory Engine for AI Agents with Crash Recovery.**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()
[![Determinism: Verified](https://img.shields.io/badge/determinism-verified-brightgreen)](.github/workflows/multi-arch-determinism.yml)

**Valori** is a `no_std` Rust kernel providing a strictly deterministic vector database and knowledge graph. It guarantees **bit-identical state across any architecture** (x86, ARM, WASM) with **crash recovery** and verifiable memory for AI agents.

---

## ⚡ Key Features

### 1. Bit-Identical Determinism (CI-Verified)
Unlike standard vector stores using `f32` (which varies by CPU/compiler), Valori uses **Q16.16 Fixed-Point Arithmetic**.

- ✅ **Guarantee**: Same operations = Same hash on **any** architecture
- ✅ **Automated Proof**: [CI validates](docs/multi-arch-determinism.md) x86, ARM, WASM every commit
- ✅ **Safety**: Inputs validated to `[-32768.0, 32767.0]` range
- ✅ **Contract**: [Build determinism guarantees](docs/build-determinism.md)

**Example**:
```python
# Insert on ARM device
kernel_arm.insert(vector)
hash_arm = kernel_arm.get_state_hash()

# Replay on x86 server
kernel_x86.restore_from_wal(commands)
hash_x86 = kernel_x86.get_state_hash()

assert hash_arm == hash_x86  # ✅ Cryptographically identical!
```

### 2. Crash Recovery via WAL
Deterministic Write-Ahead Log enables bit-perfect recovery.

- ✅ **Durable**: fsync guarantees after each write
- ✅ **Deterministic Replay**: Snapshot + WAL = identical state
- ✅ **Cross-Platform**: ARM device → x86 cloud replay works perfectly
- ✅ **Restart Symmetric**: Resume interrupted operations seamlessly

**Example**:
```rust
// Normal operation - writes go to WAL
engine.insert_record(embedding)?;
engine.save_snapshot()?;

// After crash - automatic recovery
engine.restore_with_wal_replay(snapshot,  wal_path)?;
// ✅ State restored perfectly!
```

See: [WAL Replay Guarantees](docs/wal-replay-guarantees.md)

### 3. `no_std` Embedded Support
Run on microcontrollers without an operating system.

- ✅ **ARM Cortex-M** ready
- ✅ **No heap allocation** (stack/static only)
- ✅ **~4KB RAM** (256 records, 16-dim)
- ✅ **~5µs** insert latency

Perfect for: robotics, drones, autonomous systems, edge AI.

See: [Embedded Quickstart](docs/embedded-quickstart.md)

### 4. Hybrid-Native Architecture
One kernel, two deployment modes:

- **Embedded (FFI)**: Direct in-process linking via `pyo3` - microsecond latency
- **Remote (HTTP)**: Same kernel wrapped in `axum`/`tokio` - horizontal scaling
- **Switch**: Change 1 line of code to go from local dev → production

### 5. "Git for Memory"
Snapshot and restore your entire AI memory state.

- ✅ **Atomic Snapshots**: `[Header][Kernel][Meta][Index]`
- ✅ **Instant Restore**: Checkpoint and resume
- ✅ **Cryptographic Proofs**: Export state hashes for verification
- ✅ **Version Control**: Track memory evolution over time

---

## 🚀 Quick Start

### Python (Easiest)

```bash
pip install valori
```

```python
from valori import EmbeddedKernel

# Create kernel
kernel = EmbeddedKernel(max_records=1024, dim=16)

# Insert embeddings
embedding = model.encode("Hello, world!")
kernel.insert(embedding.tolist())

# Save snapshot
snapshot = kernel.save_snapshot()
hash = kernel.get_state_hash()

# Restore on any device/architecture
kernel2 = EmbeddedKernel(max_records=1024, dim=16)
kernel2.restore_snapshot(snapshot)
assert kernel2.get_state_hash() == hash  # ✅ Identical!
```

### Rust (Embedded)

```toml
[dependencies]
valori-kernel = { version = "0.1", default-features = false }
valori-embedded = "0.1"
```

```rust
#![no_std]

use valori_kernel::state::kernel::KernelState;

const MAX_RECORDS: usize = 256;
const DIM: usize = 16;

fn main() {
    let mut kernel = KernelState::<MAX_RECORDS, DIM, 0, 0>::new();
    
    // Insert vectors from sensors
    // ... your application logic ...
    
    // Export for verification
    let hash = kernel_state_hash(&kernel);
    transmit_to_cloud(hash);
}
```

See: [Embedded Quickstart](docs/embedded-quickstart.md)

### HTTP Server (Production)

```bash
cargo run --release -p valori-node
```

```python
from valori import KernelClient

# Remote mode
client = KernelClient(url="http://localhost:3000")
client.insert([0.1, 0.2, ...])
results = client.search([0.15, 0.25, ...], k=5)
```

---

## 📐 Architecture

Valori uses a **strict layered architecture** ensuring the deterministic kernel remains pure while enabling production durability and multiple deployment modes.

```mermaid
graph TB
    subgraph Clients["🖥️ CLIENT APPLICATIONS"]
        PythonApp["Python Scripts"]
        RustApp["Rust Applications"]
        HTTPClient["HTTP Clients"]
        Embedded["Embedded Devices<br/>(ARM Cortex-M)"]
    end

    subgraph Interface["💻 INTERFACE LAYER (std)"]
        direction LR
        FFI["Python FFI (pyo3)<br/>EmbeddedKernel<br/>• Direct in-process<br/>• Microsecond latency"]
        HTTP["HTTP Server (axum)<br/>REST API<br/>• /v1/memory/*<br/>• Multi-client"]
    end

    subgraph Durability["💾 DURABILITY LAYER (std)"]
        direction TB
        Engine["Engine Coordinator"]
        
        subgraph Persistence["Persistence Components"]
            WALWriter["WAL Writer<br/>• bincode serialize<br/>• fsync() durability<br/>• Length-prefixed framing"]
            WALReader["WAL Reader<br/>• Deserialize commands<br/>• Iterator API<br/>• replay_wal()"]
            SnapshotMgr["Snapshot Manager<br/>• encode_state()<br/>• decode_state()<br/>• BLAKE3 hashing"]
        end
        
        subgraph Storage["📁 Persistent Storage"]
            WALFile["commands.wal<br/>[version:u8]<br/>[length:u32]<br/>[command:bytes]"]
            SnapshotFile["state.snapshot<br/>[Header]<br/>[Kernel]<br/>[Metadata]<br/>[Index]"]
        end
    end

    subgraph Kernel["⚙️ VALORI KERNEL (no_std, pure Rust)"]
        direction TB
        KernelState["KernelState&lt;R,D,N,E&gt;<br/>Deterministic State Machine"]
        
        subgraph CoreComponents["Core Components"]
            direction LR
            VectorStorage["📊 Vector Storage<br/>RecordPool[R]<br/>FxpVector&lt;D&gt;<br/>• insert()<br/>• delete()<br/>• get()"]
            Graph["🕸️ Knowledge Graph<br/>NodePool[N]<br/>EdgePool[E]<br/>AdjacencyList<br/>• create_node()<br/>• create_edge()"]
            FXP["🔢 Fixed-Point Math<br/>Q16.16 (i32)<br/>• add, sub, mul, div<br/>• l2_distance()<br/>• normalize()"]
        end
        
        Verify["🔐 Cryptographic Verification<br/>kernel_state_hash() → [u8;32]<br/>BLAKE3 deterministic hashing"]
    end

    %% Client connections
    PythonApp --> FFI
    RustApp --> FFI
    HTTPClient --> HTTP
    Embedded -.->|Direct Link| KernelState

    %% Interface to Durability
    FFI --> Engine
    HTTP --> Engine

    %% Durability components
    Engine --> WALWriter
    Engine --> WALReader
    Engine --> SnapshotMgr
    
    WALWriter -->|Write| WALFile
    WALReader -->|Read| WALFile
    SnapshotMgr -->|Save/Load| SnapshotFile

    %% Recovery flow
    WALReader -.->|Replay| KernelState
    SnapshotMgr -.->|Restore| KernelState

    %% Durability to Kernel
    Engine --> KernelState

    %% Kernel internals
    KernelState --> VectorStorage
    KernelState --> Graph
    KernelState --> FXP
    KernelState --> Verify

    %% Styling
    classDef clientStyle fill:#e1f5fe,stroke:#01579b,stroke-width:2px,color:#000
    classDef interfaceStyle fill:#e8f5e9,stroke:#2e7d32,stroke-width:2px,color:#000
    classDef durabilityStyle fill:#fce4ec,stroke:#c2185b,stroke-width:2px,color:#000
    classDef kernelStyle fill:#fff3e0,stroke:#e65100,stroke-width:3px,color:#000
    classDef storageStyle fill:#f3e5f5,stroke:#7b1fa2,stroke-width:2px,color:#000

    class PythonApp,RustApp,HTTPClient,Embedded clientStyle
    class FFI,HTTP interfaceStyle
    class Engine,WALWriter,WALReader,SnapshotMgr durabilityStyle
    class KernelState,VectorStorage,Graph,FXP,Verify kernelStyle
    class WALFile,SnapshotFile storageStyle
```

### 🔄 Crash Recovery Flow

```mermaid
sequenceDiagram
    participant S as Snapshot File
    participant W as WAL File
    participant R as WAL Reader
    participant K as Kernel
    participant V as Verifier

    Note over S,V: System Restart After Crash

    S->>K: 1. Load snapshot (State S₀)
    activate K
    Note over K: Kernel at snapshot state

    W->>R: 2. Read WAL commands
    activate R
    
    loop For each command
        R->>K: 3. Replay command
        Note over K: Apply deterministically
    end
    deactivate R

    K->>K: 4. Compute state hash
    K->>V: 5. Verify hash
    activate V
    
    alt Hash matches expected
        V-->>K: ✅ Recovery successful
        Note over K: State Sₙ (bit-identical)
    else Hash mismatch
        V-->>K: ❌ Recovery failed
        Note over K: Corruption detected
    end
    deactivate V
    deactivate K
```

### 🎯 Key Properties

| Layer | Characteristics | Guarantees |
|-------|----------------|------------|
| **Kernel** | `no_std`, pure functions, Q16.16 fixed-point | Bit-identical across x86/ARM/WASM |
| **Durability** | WAL + Snapshots, bincode serialization | Crash recovery, deterministic replay |
| **Interface** | HTTP (axum) or FFI (pyo3) | Flexible deployment, same kernel |
| **Storage** | Length-prefixed WAL, structured snapshots | Durability, atomicity |

**Separation of Concerns**: Core kernel stays pure (no I/O) → Durability wrapped outside → Flexible interfaces

See [Architecture Details](architecture.md) for deep dive.

---

## 🎯 Use Cases

### Robotics & Autonomous Systems
- **Problem**: Robot fleet needs shared, verifiable memory
- **Solution**: Deterministic snapshots replicate perfectly across devices
- **Benefit**: ARM robot → x86 cloud → different ARM robot = identical state

### Edge AI with Verification
- **Problem**: Cannot trust device-generated embeddings
- **Solution**: Export cryptographic proof of memory state
- **Benefit**: Cloud can verify computation happened correctly

### Safety-Critical Applications
- **Problem**: Need reproducible AI behavior for certification
- **Solution**: Bit-identical determinism + audit trail via WAL
- **Benefit**: Every decision is reproducible and verifiable

### Multi-Device Coordination
- **Problem**: Drones/robots need synchronized context
- **Solution**: WAL streaming + deterministic replay
- **Benefit**: All devices converge to identical memory state

---

## 📚 Documentation

- **Getting Started**:
  - [Embedded Quickstart](docs/embedded-quickstart.md) - ARM Cortex-M in 10 minutes
  - [Python Guide](docs/python-client.md) - FFI and remote modes
  - [HTTP API](docs/api.md) - REST endpoints

- **Core Concepts**:
  - [Architecture](architecture.md) - System design
  - [Determinism Guarantees](docs/determinism-guarantees.md) - Formal specification
  - [Fixed-Point Arithmetic](docs/core-concepts.md) - Why FXP?

- **Advanced**:
  - [WAL Replay Guarantees](docs/wal-replay-guarantees.md) - Crash recovery
  - [Multi-Arch Validation](docs/multi-arch-determinism.md) - CI proof
  - [Performance Benchmarks](docs/benchmarks.md) - Speed & memory

---

## 🔬 Proof of Determinism

### The Problem: Floating Point Non-Reproducibility

The same embedding model + same input = **different results** on different CPUs:

```python
# x86 output
[0xbd8276f8, 0x3d6bb481, 0x3d1dcdf1, ...]

# ARM output  
[0xbd8276fc, 0x3d6bb470, 0x3d1dcdf9, ...]
      ↑↑           ↑↑           ↑↑
   Different!   Different!   Different!
```

This is **IEEE-754 compliant** but breaks reproducibility.

### Our Solution: Fixed-Point Arithmetic

Valori uses Q16.16 fixed-point (32-bit integers):
- ✅ Bit-identical across **all** architectures
- ✅ Validated in CI: x86 = ARM = WASM
- ✅ No floating point unit required

**Automated proof**: Our CI runs identical tests on 3 architectures and compares cryptographic hashes. If hashes diverge, build fails.

See: [Multi-Architecture Determinism](docs/multi-arch-determinism.md)

---

## 🛠️ Development

```bash
# Build kernel (no_std)
cargo build --lib --release

# Build node server
cargo build --release -p valori-node

# Run tests
cargo test --all-features

# Run determinism validation
cargo test -p valori-node --test multi_arch_determinism --release

# Start server
cargo run --release -p valori-node
```

---

## 📊 Performance

| Operation | Latency | Memory |
|-----------|---------|--------|
| Insert (16-dim) | ~5µs | ~64 bytes |
| L2 Distance | ~2µs | - |
| Snapshot (256 records) | ~100µs | ~4KB |
| WAL Replay (100 cmds) | ~600µs | - |

**Platform**: ARM Cortex-M4 @ 168MHz

---

## 🤝 Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md).

**Key areas**:
- Embedded platform testing
- Performance optimization
- Documentation improvements

---

## 📄 License

AGPL-3.0 - See [LICENSE](LICENSE) for details.

---

## 🌟 Why Valori?

Most vector databases sacrifice **reproducibility** for performance. Valori proves you can have both:

✅ **Deterministic** - Bit-identical across any platform  
✅ **Verifiable** - Cryptographic proofs of state  
✅ **Durable** - Crash recovery via WAL  
✅ **Embedded** - Runs on ARM Cortex-M  
✅ **Fast** - Microsecond latencies  
✅ **Proven** - Automated CI validation  

Perfect for robotics, autonomous systems, edge AI, and any application where reproducibility matters.

---

**Ready to build verifiable AI memory?** → [Get Started](docs/embedded-quickstart.md)
