# Valori Remote Mode Guide

Valori is designed to be **"Local First, Remote Ready"**. This means you can write your application code *once* using the `ProtocolClient`, and switch between embedded execution and client-server architecture just by changing a config string.

## Architecture

*   **Local Mode**: The Python library (`valori`) loads the Rust Kernel (`valori-node`) as a shared library (`.so` / `.dll`). Memory is stored in your process's RAM.
*   **Remote Mode**: You act as a client. The `valori-node` binary runs as a standalone HTTP server. Your Python code uses `requests` to send vectors to the node.

## 1. Starting the Server

Build and run the node binary:

```bash
# Production build
cargo build -p valori-node --release

# Run (default port 3000)
./target/release/valori-node
```

You can configure it via environment variables:
*   `VALORI_MAX_RECORDS`: Max capacity (default 1024)
*   `VALORI_BIND`: Address to listen on (e.g., `0.0.0.0:8080`)

## 2. Using the Client

Install the Python package:
```bash
pip install valori
```

In your code:

```python
from valori import ProtocolClient

def dummy_embed(text):
    return [0.0] * 16  # Replace with real embedding logic

# Connect to the remote server
client = ProtocolClient(embed=dummy_embed, remote="http://localhost:3000")

# Now just use the standard API
client.upsert_text("Hello from the client!")
hits = client.search_text("Hello")
```

## 3. Why use Remote Mode?

1.  **Persistence**: The Server can be configured to save snapshots periodically (coming soon) or you can trigger `client.snapshot()` to get the entire database state as bytes and save it to S3.
2.  **Concurrency**: Multiple Python scripts (e.g., a web scraper and a chatbot) can read/write to the same memory.
3.  **Language Agnostic**: Since the server speaks HTTP + JSON, you can interact with it using curl, Node.js, or Go (though we only provide a Python SDK currently).

## API Reference

The server exposes the following generic endpoints for vectors:

*   `POST /v1/memory/upsert_vector`: Store a raw vector.
*   `POST /v1/memory/search_vector`: Query nearest neighbors.
*   `POST /snapshot`: Download full DB state.
*   `POST /restore`: Upload full DB state.
