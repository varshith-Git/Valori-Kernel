# Contributing to Valori

This guide gets you from a fresh clone to a running node in under 10 minutes.

---

## Prerequisites

| Tool | Minimum version | How to install |
|---|---|---|
| **Rust + cargo** | 1.80 stable | `curl https://sh.rustup.rs -sSf \| sh` |
| **Python** | 3.9+ | `brew install python` or system package manager |
| **Node.js** | 18+ (UI only) | `brew install node` or [nodejs.org](https://nodejs.org) |
| **Docker** | any recent (cluster only) | [docker.com](https://www.docker.com) |

Verify your Rust install:

```bash
rustc --version    # should print 1.80 or newer
cargo --version
```

> **wasm32 target** — only needed if you touch `crates/valori-kernel`:
> ```bash
> rustup target add wasm32-unknown-unknown
> ```

---

## 1. Clone and build

```bash
git clone https://github.com/varshith-Git/Valori-Kernel.git
cd Valori-Kernel

# Build every crate in the workspace
cargo build --workspace

# Run the core test suite
cargo test -p valori-kernel -p valori-node
```

All tests should pass. On Linux, install `build-essential` / `gcc` if you hit a linker error.

---

## 2. Run a standalone node (the simplest start)

```bash
# In-memory, no persistence (fastest for dev)
VALORI_DIM=128 cargo run -p valori-node

# With WAL + snapshot (survives restarts — recommended)
VALORI_DIM=128 \
VALORI_EVENT_LOG_PATH=/tmp/valori.log \
VALORI_SNAPSHOT_PATH=/tmp/valori.snap \
cargo run -p valori-node
```

The node listens on **`http://localhost:3000`** by default. Smoke-test it:

```bash
curl http://localhost:3000/health
# → "ok"

# Insert a record
curl -s -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]}'

# Search
curl -s -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8], "k": 5}' | jq .
```

> **`VALORI_DIM` is immutable after the first insert.** Set it once and never change it for the same data directory.

---

## 3. Python SDK

```bash
# Install the remote client (pure Python, no compilation needed)
pip install -e python/

python3 - <<'EOF'
from valoricore.remote import SyncRemoteClient
c = SyncRemoteClient("http://localhost:3000")
print(c.health())                       # ok
rid = c.insert([0.1]*8)
print(c.search([0.1]*8, k=3))
EOF
```

The **in-process `LocalClient`** (PyO3 FFI, no server needed) requires compiling the native extension:

```bash
pip install maturin
cd crates/valori-ffi
maturin develop        # compiles and installs into your active Python env
```

---

## 4. Web dashboard (Next.js UI)

```bash
# Node.js 18+ required
cd ui
npm install
npm run dev      # starts at http://localhost:4000
```

The UI reverse-proxies API calls to the node, so **start the node first** (step 2). No extra config needed for local dev.

For a production build:

```bash
npm run build
npm start
```

---

## 5. Local 3-node cluster (Docker)

```bash
# Start 3 nodes + bootstrap automatically
docker compose up -d

# Wait ~3 seconds for the leader election, then check
curl http://localhost:3001/health
curl http://localhost:3001/v1/cluster/status | jq .
```

| Node | HTTP port | Raft gRPC port |
|---|---|---|
| node-1 | 3001 | 3101 |
| node-2 | 3002 | 3102 |
| node-3 | 3003 | 3103 |

Tear down and wipe volumes:

```bash
docker compose down -v
```

---

## 6. Key environment variables

### Standalone node

| Variable | Default | What it does |
|---|---|---|
| `VALORI_DIM` | `128` | Vector dimension — immutable after first insert |
| `VALORI_BIND` | `0.0.0.0:3000` | HTTP listen address |
| `VALORI_EVENT_LOG_PATH` | _(none)_ | WAL path; omit for in-memory only |
| `VALORI_SNAPSHOT_PATH` | _(none)_ | Snapshot file path |
| `VALORI_AUTH_TOKEN` | _(none)_ | Bearer token for all endpoints; omit to disable auth |
| `VALORI_INDEX` | `brute` | `brute` or `hnsw` |
| `VALORI_MAX_RECORDS` | `1000000` | Record slab capacity |
| `VALORI_EMBED_PROVIDER` | _(none)_ | `ollama` / `openai` / `custom`; enables `POST /v1/ingest` |
| `VALORI_EMBED_MODEL` | provider default | e.g. `nomic-embed-text`, `text-embedding-3-small` |
| `VALORI_EMBED_URL` | provider default | e.g. `http://localhost:11434` for Ollama |
| `VALORI_EMBED_API_KEY` | _(none)_ | API key for OpenAI / custom providers |
| `VALORI_DECAY_HALF_LIFE_SECS` | _(none)_ | Default recency decay half-life for search |

### Cluster additions

| Variable | What it does |
|---|---|
| `VALORI_NODE_ID` | Integer ID for this node (1, 2, 3, …) |
| `VALORI_CLUSTER_MEMBERS` | Full topology: `1=host:3100/host:3000,2=…` |
| `VALORI_CLUSTER_INIT` | Set to `1` on the **bootstrap node only**, first boot only |
| `VALORI_RAFT_LOG_PATH` | Persistent redb file for Raft log and vote |
| `VALORI_RAFT_BIND` | gRPC listen address (default `0.0.0.0:3100`) |

Copy `.env.example` → `.env` and fill in values for your deployment.

---

## 7. Ingest pipeline (optional — needs an embed provider)

With an embed provider configured you can POST raw text and let the node chunk + embed + insert:

```bash
VALORI_DIM=768 \
VALORI_EMBED_PROVIDER=ollama \
VALORI_EMBED_MODEL=nomic-embed-text \
VALORI_EMBED_URL=http://localhost:11434 \
  cargo run -p valori-node
```

```bash
curl -s -X POST http://localhost:3000/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"text": "Your document text here...", "source": "my-doc.pdf"}' | jq .
```

---

## 8. MCP server (Claude Desktop integration)

```bash
cargo build --release -p valori-mcp

# Add to ~/Library/Application Support/Claude/claude_desktop_config.json
# (see examples/claude_desktop_config.json for the full snippet)
VALORI_URL=http://localhost:3000 ./target/release/valori-mcp
```

---

## 9. Architecture rules — read before touching code

These are enforced invariants, not style preferences:

1. **`valori-kernel` is `no_std`** — never add `use std::` inside `crates/valori-kernel/src/`. Use `core::` or `alloc::` instead. After any kernel change, verify:
   ```bash
   cargo build -p valori-kernel --target wasm32-unknown-unknown
   ```

2. **Every new HTTP endpoint goes in BOTH `server.rs` AND `cluster_server.rs`** — standalone-only endpoints silently 404 in cluster mode. No compile error catches this.

3. **No floats in the kernel hot path** — all vector ops use `FxpScalar` (Q16.16, backed by `i32`). `f32`/`f64` only appear in the HTTP JSON layer for I/O.

4. **Apply → then audit** — `DEDUP CHECK → KERNEL APPLY → AUDIT WRITE`. Never write a BLAKE3 audit entry for a rejected or duplicate event.

5. **Python SDK: always update both clients** — `SyncRemoteClient` AND `AsyncRemoteClient` in `python/valoricore/remote.py`. `ClusterClient`/`AsyncClusterClient` inherit via `**kwargs`, so you usually only touch the two base classes.

---

## 10. Running specific tests

```bash
# Full kernel test suite
cargo test -p valori-kernel

# Full node test suite
cargo test -p valori-node

# One test with stdout
cargo test -p valori-node test_collections_isolation -- --nocapture

# Verify kernel stays wasm-compatible after your change
cargo build -p valori-kernel --target wasm32-unknown-unknown

# Python SDK tests
cd python && python -m pytest tests/ -v
```

---

## 11. Project layout at a glance

```
crates/
  valori-kernel/      # no_std deterministic core: vector store, graph, BLAKE3 audit chain
  valori-consensus/   # Raft state machine (openraft 0.9 + tonic/gRPC)
  valori-node/        # axum HTTP server + cluster orchestration + metadata store
  valori-cli/         # `valori` binary: setup wizard, import, verify, timeline
  valori-mcp/         # Model Context Protocol server for Claude Desktop
  valori-verify/      # Standalone offline BLAKE3 verifier binary
  valori-ffi/         # PyO3 bindings — in-process Python SDK
  valori-wire/        # Shared serde types (node ↔ SDK ↔ CLI)
python/valoricore/    # Python SDK: SyncRemoteClient, AsyncRemoteClient, local.py (FFI)
ui/                   # Next.js dashboard
examples/             # Quickstart scripts (LangChain, LlamaIndex, MCP, cluster)
deploy/               # Kubernetes Helm chart, Terraform modules
docs/                 # Architecture docs, API reference, phase build history
```

---

## 12. PR checklist

Before opening a pull request:

- [ ] `cargo test -p valori-kernel -p valori-node` passes
- [ ] If you touched `valori-kernel`: `cargo build -p valori-kernel --target wasm32-unknown-unknown` passes
- [ ] New endpoint added to **both** `server.rs` and `cluster_server.rs`
- [ ] Python SDK updated (both `SyncRemoteClient` and `AsyncRemoteClient`) if the API surface changed
- [ ] UI: no hardcoded dark colors — use semantic CSS tokens (`--background`, `--foreground`, `--border`, `--v-accent`, etc.)
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] If a new phase: phase doc created in `docs/phases/` and status table in `docs/phases/README.md` updated

---

## Contribution philosophy

Valori's core guarantee is **bit-identical results on every architecture**. Contributions that trade determinism for raw performance will not be accepted. Everything else — new retrieval features, SDK improvements, deployment tooling, documentation — is welcome.

Open an issue first for changes that touch the kernel wire format, snapshot version, or BLAKE3 chain structure so the approach can be agreed before you write code.

**Contact:** varshith.gudur17@gmail.com
