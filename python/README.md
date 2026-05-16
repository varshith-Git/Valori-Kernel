# Valoricore 🛡️

**The Deterministic Knowledge Graph & Vector Engine with Bit-Exact Audit Trails**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](../LICENSE)
[![Python 3.8+](https://img.shields.io/badge/python-3.8+-blue.svg)](https://www.python.org/downloads/)
[![Rust](https://img.shields.io/badge/built%20with-Rust-orange)](https://www.rust-lang.org/)

`valoricore` is the official Python SDK for the Valoricore Kernel. It provides a high‑performance, async-capable interface for applications where **determinism** and **auditability** are absolute requirements.

---

## 🔒 What Makes Valoricore Different?

- **True Determinism** – Fixed‑point arithmetic ensures identical results on x86, ARM, and RISC‑V, forever.
- **Cryptographic Audit Trails** – Every insert, update, and delete is logged. The BLAKE3‑based Merkle root proves the exact state at any point in time.
- **Unified Graph + Vector** – Seamlessly combine semantic search with structured knowledge graph relationships (nodes and edges).
- **Embedded & Distributed** – Run as a lightweight, embedded engine via FFI or scale to a multi‑node cluster.
- **Zero‑Config** – Vector dimensions and pool capacities are auto‑detected. No manual tuning required.

---

## 📦 Installation

Valoricore ships with a pre‑compiled native extension for most platforms. A Rust compiler is **only** required when building from source.

```bash
pip install valoricore
```

---

## 🚀 Quick Start

### Local embedded engine
No server required – import and go.

```python
from valoricore import Valoricore

# Create or open a local database
db = Valoricore(path="./my_knowledge_base")

# Insert a vector
record_id = db.insert(
    vector=[0.1, 0.2, 0.3, 0.4],   # any dimension
    tag=101
)

# Set binary metadata (up to 64KB)
db.set_metadata(record_id, b"{\"title\": \"Project Alpha\"}")

# Semantic search (with optional tag filter)
results = db.search(query=[0.1, 0.2, 0.3, 0.5], k=5, filter_tag=101)

# Get a cryptographic proof of the full state
state_hash = db.get_state_hash()

# --- Offline Verification ---
# You can verify an embedding's integrity WITHOUT the database running
from valoricore import verify_embedding
is_valid = verify_embedding(vector=[0.1, 0.2, 0.3, 0.4], claimed_hash="a3f8c2d1...")

# --- Auto-Snapshots ---
# Automatically take and save a snapshot to the db directory every 1,000,000 inserts
db.snapshot(auto_interval=1_000_000)

# --- Restoring from a Snapshot ---
# Read the binary file into memory, then pass the bytes to restore()
with open("valoricore_db/auto_snapshot_1000000.snap", "rb") as f:
    snapshot_bytes = f.read()
    
db.restore(snapshot_bytes)
```

### Async support
For non‑blocking operations inside FastAPI or modern async applications, use the built‑in async factory:

```python
from valoricore import AsyncValoricore

# Use AsyncValoricore for high-performance non-blocking I/O
db = AsyncValoricore(remote="http://localhost:3033")
results = await db.search(query=[...], k=10)
```

> 💡 The async client uses `httpx` and is fully compatible with `asyncio` event loops.

---

## 🧠 Key Abstractions

- **Record** – A vector with an optional tag (integer) and arbitrary binary metadata.
- **Node** – A higher‑level entity (e.g., “Document”, “User”) that wraps one or more records.
- **Edge** – A directed relationship between nodes (e.g., “parentOf”, “authoredBy”).
- **Proof** – A BLAKE3‑based Merkle inclusion proof that verifies a record’s presence in a given global state.

---

## 🔐 Cryptographic Determinism

Valoricore’s core engine performs all distance calculations in **fixed‑point arithmetic (Q16.16)**. This guarantees that the same sequence of operations will produce the **exact same** score values, state hashes, and proofs—regardless of the hardware or operating system.

The global state hash is a single 32‑byte BLAKE3 Merkle root that represents the entire database.

---

## 📚 Documentation

- **[Getting Started Guide](docs/getting_started.md)** – Your first 5 minutes with Valoricore.
- **[API Reference](docs/api_reference.md)** – Complete method signatures, types, and error codes.

---

**Built with ❤️ by the Valoricore team** – integrity‑first AI infrastructure.
