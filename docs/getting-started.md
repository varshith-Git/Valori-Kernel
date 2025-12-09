# Getting Started with Valori Kernel

Valori Kernel is a deterministic, `no_std` vector database and knowledge graph engine. It allows you to store vectors, form relationships between concepts, and replay execution state identically across any platform.

## 1. Choose Your Interface

Valori is designed to be embedded. You rarely interact with the "Kernel" directly unless you are writing Rust. Instead, you use one of the two primary interfaces:

| Interface | Use Case |
| :--- | :--- |
| **Python FFI (`valori-ffi`)** | **Local / Research**: Single-process scripts, Jupyter notebooks, agents, prototyping. Minimal overhead, direct memory access. |
| **Node.js Service (`valori-node`)** | **Remote / Infrastructure**: Microservices, shared memory across apps, SaaS deployment. Accessible via HTTP/REST. |

---

## Getting Started with Valori

This guide will take you from zero to running your first Deterministic Memory Engine.

## Prerequisites
*   **Python 3.8+**
*   **Rust** (only if compiling from source)

---

## 1. Installation

### From PyPI (Recommended)
```bash
pip install valori
```

### From Source (For Contributors)
```bash
git clone https://github.com/varshith-Git/Valori-Kernel
cd Valori-Kernel

# Build the Python bindings
cd ffi
maturin develop --release
```

---

## 2. Your First Memory (Local Mode)

Create a file `memory_test.py`:

```python
from valori import ProtocolClient

# 1. Define a dummy embedder (In real apps, use OpenAI/SentenceTransformers)
def my_embed(text):
    # Returns a 16-dim zero vector for demo
    return [0.0] * 16

# 2. Init Client
client = ProtocolClient(embed=my_embed)

# 3. Upsert
print("Storing memory...")
client.upsert_text("My contact email is varshith.gudur17@gmail.com")

# 4. Search
print("Searching...")
hits = client.search_text("email")
print(f"Found: {hits}")
```

Run it:
```bash
python memory_test.py
```

---

## 3. Moving to Production (Remote Mode)

When you are ready to scale, run the Valori Node server.

1.  **Start the Server**:
    ```bash
    cargo run -p valori-node --release
    # Server running on http://127.0.0.1:3000
    ```

2.  **Update your Script**:
    Change one line:
    ```python
    client = ProtocolClient(embed=my_embed, remote="http://127.0.0.1:3000")
    ```

3.  **Run**:
    Only the `client` logic changes. The data now lives in the `valori-node` process!
 Insert a vector (16 dimensions)
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
