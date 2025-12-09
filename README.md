# Valori Kernel

A deterministic, `no_std`, fixed-point vector + knowledge graph engine in Rust.

`valori-kernel` is designed to be a tiny, embeddable kernel that provides consistent behavior across different runtimes (Node.js, Cloud, Embedded) by strictly enforcing determinism and avoiding dynamic allocation in its core.

### 3. Usage (Python)

Valori supports two modes: **Local (Embedded)** and **Remote (Client-Server)**.

#### Local Mode (No Server Required)
Runs the logic inside your Python process using FFI. Fastest for single-process apps.
```python
from valori import ProtocolClient

# 1. Define an embedding function (e.g. using OpenAI, SentenceTransformers)
def my_embedder(text):
    return [0.1, 0.2, ...] # Must return list[float] of dimension D

# 2. Initialize Client (Local)
client = ProtocolClient(embed=my_embedder)

# 3. Remember & Search
client.upsert_text("Valori remembers everything.")
print(client.search_text("What does Valori do?"))
```

#### Remote Mode (Production)
Connects to a standalone `valori-node` server over HTTP. Use this for web apps, multi-process setups, or cloud deployments.
```python
# 1. Start the server (in terminal)
# ./valori-node

# 2. Connect via Python
client = ProtocolClient(embed=my_embedder, remote="http://127.0.0.1:3000")

# API is identical to Local Mode!
client.upsert_text("This is stored on the server.")
```

## Features

- **`no_std` Core**: Built for embedded and constrained environments. No standard library dependencies in the core kernel.
- **Determinism**: Bit-identical results across platforms. No floating-point arithmetic or randomness.
- **Fixed-Point Arithmetic**: Uses Q16.16 fixed-point math for all geometric operations (dot product, L2 distance).
- **Static Memory**: Operates on pre-allocated pools. No heap allocation during runtime execution.
- **Knowledge Graph**: Integrated graph structure linking vectors, records, and concepts.
- **Snapshot/Replay**: Full state serialization and deterministic replay via a command log.

## Installation

Add `valori-kernel` to your `Cargo.toml`:

```toml
[dependencies]
valori-kernel = { path = "." } # Or git/crates.io dependency
```

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
