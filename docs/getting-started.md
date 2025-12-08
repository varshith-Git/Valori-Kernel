# Getting Started with Valori Kernel

Valori Kernel is a deterministic, `no_std` vector database and knowledge graph engine. It allows you to store vectors, form relationships between concepts, and replay execution state identically across any platform.

## 1. Choose Your Interface

Valori is designed to be embedded. You rarely interact with the "Kernel" directly unless you are writing Rust. Instead, you use one of the two primary interfaces:

| Interface | Use Case |
| :--- | :--- |
| **Python FFI (`valori-ffi`)** | **Local / Research**: Single-process scripts, Jupyter notebooks, agents, prototyping. Minimal overhead, direct memory access. |
| **Node.js Service (`valori-node`)** | **Remote / Infrastructure**: Microservices, shared memory across apps, SaaS deployment. Accessible via HTTP/REST. |

---

## 2. Installation

### Rust (Core)
If you are building a Rust application:
```toml
[dependencies]
valori-kernel = { git = "https://github.com/varshith-Git/Valori-Kernel" }
```

### Python FFI
*Requires Rust toolchain installed.*

```bash
cd ffi
maturin develop
# or
pip install .
```

### Node.js (HTTP Server)
*Requires Rust toolchain.*

```bash
cd node
cargo run --release
# Server starts on 127.0.0.1:3000
```

---

## 3. Quick Start (Python)

```python
import valori_ffi

# Initialize a fresh kernel (in-memory)
db = valori_ffi.PyKernel()

# 1. Insert a vector (16 dimensions)
vec = [0.1] * 16
id_1 = db.insert(vec)
print(f"Inserted record with ID: {id_1}")

# 2. Search
hits = db.search(vec, k=5)
print(f"Nearest neighbors: {hits}")

# 3. Create Graph Nodes
node_id = db.create_node(kind=1, record_id=id_1)
```

## 4. Quick Start (HTTP / Node)

Start the server:
```bash
cargo run --bin valori-node
```

Interact via curl or HTTP client:

```bash
# Insert Record
curl -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"values": [0.1, 0.1, ...]}'

# Search
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.1, ...], "k": 5}'
```

## Next Steps

*   [Core Concepts](./core-concepts.md) - Learn about Determinism, Fixed-Point Math, and Snapshots.
*   [Python Reference](./python-reference.md) - Detailed PyKernel API documentation.
*   [Node.js Reference](./node-reference.md) - API endpoints and schemas.
