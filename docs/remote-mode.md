# Valori Remote Mode Guide

In remote mode, `valori-node` runs as a standalone HTTP server and your application
talks to it over JSON/HTTP via `SyncRemoteClient` or `AsyncRemoteClient`.

---

## Architecture comparison

| | Embedded (`MemoryClient`) | Remote (`SyncRemoteClient`) |
|---|---|---|
| **Where it runs** | Inside your Python process (PyO3 FFI) | Separate process / container |
| **Latency** | Zero (direct memory) | Network RTT |
| **Concurrency** | Single process | Multi-client |
| **Best for** | Scripts, CLI tools, offline | Web apps, multi-agent, cloud |

---

## 1. Start the server

```bash
VALORI_DIM=128 cargo run --release -p valori-node
# Listening on http://0.0.0.0:3000
```

Key environment variables:

| Var | Default | Purpose |
|---|---|---|
| `VALORI_DIM` | 128 | Vector dimension (immutable after first insert) |
| `VALORI_MAX_RECORDS` | 1 000 000 | Slab capacity |
| `VALORI_BIND` | 0.0.0.0:3000 | HTTP listen address |
| `VALORI_EVENT_LOG_PATH` | — | Audit log path (omit = no persistence) |
| `VALORI_SNAPSHOT_PATH` | — | Snapshot file path |
| `VALORI_AUTH_TOKEN` | — | Bearer token (omit = no auth) |

See [CLAUDE.md](../CLAUDE.md) for the full env var reference.

---

## 2. Connect the client

```bash
pip install valoricore
```

```python
from valoricore.remote import SyncRemoteClient

client = SyncRemoteClient("http://localhost:3000")
print(client.health())   # → "ok"

# Insert (vector length must equal VALORI_DIM)
rid = client.insert([0.1, 0.2, 0.3], text="some content to index for reranking")

# Search
hits = client.search([0.1, 0.2, 0.3], k=5)
# [{"id": 0, "score": 0.0, "metadata": "some content..."}]

# Cryptographic proof — same hex on every replica
print(client.get_state_hash())
```

---

## 3. Async client

```python
import asyncio
from valoricore.remote import AsyncRemoteClient

async def main():
    async with AsyncRemoteClient("http://localhost:3000") as client:
        rid = await client.insert([0.1, 0.2, 0.3])
        hits = await client.search([0.1, 0.2, 0.3], k=5)

asyncio.run(main())
```

---

## 4. Authenticated connection

```python
client = SyncRemoteClient("http://localhost:3000", token="your-secret-token")
```

See [authentication.md](./authentication.md) for how to set `VALORI_AUTH_TOKEN` server-side.

---

## 5. Multi-agent shared memory

Multiple processes can connect to the same node simultaneously:

- **Agent A** inserts memories via `http://valori:3000`.
- **Agent B** reads from the same address.
- The BLAKE3 state hash verifies both see identical data — no sync code required.

For write-heavy multi-agent workloads, use a 3-node cluster instead. See [CLUSTER.md](./CLUSTER.md).
