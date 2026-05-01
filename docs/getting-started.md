# Getting Started with Valori

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

# 2. Your First Memory (Local Mode)

Valori is now **Zero-Config**. You don't need to declare vector dimensions or pool sizes upfront—the kernel auto-detects them on the first insertion.

Create a file `memory_test.py`:

```python
from valori import ProtocolClient

# 1. Define an embedder (SentenceTransformers example)
def my_embed(text):
    # This returns 384 dimensions. Valori adapts automatically.
    return [0.0] * 384 

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

## Next Steps

*   [Core Concepts](./core-concepts.md) - Learn about Determinism, Fixed-Point Math, and Snapshots.
*   [Remote Mode Guide](./remote-mode.md) - Detailed production guide.
*   [API Reference](./api-reference.md) - HTTP endpoints for the Node server.
