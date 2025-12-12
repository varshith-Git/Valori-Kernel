# Valori Remote Mode Guide

Valori is designed to be **"Local First, Remote Ready"**. This means you can write your application code *once* using the `ProtocolClient`, and switch between embedded execution and client-server architecture just by modifying the configuration.

---

## 🏗️ Architecture Comparison

| Feature | Local Mode (FFI) | Remote Mode (HTTP) |
| :--- | :--- | :--- |
| **Where it runs** | Inside your Python process (as a C library) | In a separate process / server / docker container |
| **Communication** | Direct Memory Access (Zero latency) | HTTP / JSON over Network |
| **Concurrency** | Single Process Lock | Multi-Client Concurrent Access |
| **Best For** | Scripts, CLI tools, Embedded Agents | Web Apps, Cloud Backends, Multi-Agent Swarms |

---

## 1. Starting the Server

The `valori-node` binary is a high-performance HTTP server powered by **Axum** (Rust).

### Build & Run
```bash
# Production build
cargo build -p valori-node --release

# Run (default port 3000)
./target/release/valori-node
```

### Configuration
You can configure the node via environment variables:
*   `VALORI_MAX_RECORDS`: Max vector capacity (default: 1024).
*   `VALORI_SNAPSHOT_INTERVAL`: Seconds between auto-saves (default: `None` (Disabled)).
*   `VALORI_AUTH_TOKEN`: Bearer Token for Security (default: `None` (Public)).
*   `VALORI_BIND`: Address to listen on (default: `127.0.0.1:3000`).

## Authentication

When `VALORI_AUTH_TOKEN` is set, clients must send `Authorization: Bearer <token>`.

Example:
```bash
VALORI_BIND=0.0.0.0:8080 VALORI_MAX_RECORDS=100000 ./valori-node
```

---

## 2. Connecting the Client

Install the Python package:
```bash
pip install valori
```

Usage:

```python
from valori import ProtocolClient

def dummy_embed(text):
    return [0.0] * 16  # Replace with real embedding logic

# Connect to the remote server
client = ProtocolClient(embed=dummy_embed, remote="http://localhost:3000")

# Now just use the standard API
# Text is chunked LOCALLY, then vectors are sent to the server.
client.upsert_text("Hello from the client!")

# Search
hits = client.search_text("Hello")
print(hits)
```

---

## 3. Remote Capabilities

### 💾 Snapshot & Restore
You can download the entire database state as a binary blob and save it (e.g., to S3).

```python
# Download snapshot (binary)
backup_bytes = client.snapshot() 
with open("backup.bin", "wb") as f:
    f.write(backup_bytes)

# Restore snapshot to a fresh server
client.restore(backup_bytes)
```

### 🧠 Distributed Memory
Multiple agents can share the same memory:
*   **Agent A** writes to `http://valori-cloud:3000`.
*   **Agent B** reads from `http://valori-cloud:3000`.
*   They instantly share context without any sync code.
