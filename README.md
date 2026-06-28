<div align="center">

<img src="assets/valori-logo.png" alt="Valori" width="72" />

# Valori

**The vector database that can mathematically prove it never lost your data.**

[![Version](https://img.shields.io/pypi/v/valoricore?style=flat-square&color=6c47ff&label=valoricore)](https://pypi.org/project/valoricore/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Build](https://img.shields.io/github/actions/workflow/status/varshith-Git/Valoricore-Kernel/ci.yml?style=flat-square&label=CI)](https://github.com/varshith-Git/Valoricore-Kernel/actions)
[![Determinism](https://img.shields.io/badge/determinism-multi--arch%20verified-brightgreen?style=flat-square)](.github/workflows/multi-arch-determinism.yml)
[![arXiv](https://img.shields.io/badge/arXiv-2512.22280-b31b1b?style=flat-square)](https://arxiv.org/abs/2512.22280)
[![Tests](https://img.shields.io/github/actions/workflow/status/varshith-Git/Valori-Kernel/test-count.yml?label=tests&style=flat-square)](https://github.com/varshith-Git/Valori-Kernel/actions/workflows/test-count.yml)

*Q16.16 fixed-point arithmetic · BLAKE3 hash-chained audit log · openraft consensus · offline verifiable proofs*

</div>

---

## The Problem

Every vector database makes a silent assumption: float arithmetic on one machine produces the same result on another. It does not. SIMD units, cloud hardware migrations, and IEEE 754 implementation variance mean replicas silently diverge — and you can never verify they haven't.

In AI systems this compounds: agent memory drifts between restarts, crash recovery is unverifiable, and an audit trail built on float results cannot be reproduced anywhere else.

**Valori eliminates all of this with one decision: integer-only vector math, provably identical on every machine.**

---

## Production Proof

```bash
# State hash before a forced restart
curl $VALORI_URL/v1/proof/state
# → {"final_state_hash": [174, 163, 169, 225, 123, 111, 34, 11, ...]}

# kill -9 — no graceful shutdown, no flush

# State hash after automatic recovery
curl $VALORI_URL/v1/proof/state
# → {"final_state_hash": [174, 163, 169, 225, 123, 111, 34, 11, ...]}
# identical — bit-perfect recovery, cryptographically verified
```

Every byte of state is recovered from the append-only, BLAKE3-chained event log and verified against the pre-crash root. No data loss. No manual intervention. No trust required.

---

## Where Valori Sits in Your Stack

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Your AI Application                            │
│   LangChain · LlamaIndex · OpenAI Agents · Custom Orchestrators    │
└────────────────────────┬────────────────────────────────────────────┘
                         │  Python SDK  /  HTTP  /  PyO3 FFI
┌────────────────────────▼────────────────────────────────────────────┐
│                         VALORI                                      │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────────┐   │
│  │  Vector      │   │  Knowledge   │   │  Cryptographic       │   │
│  │  Memory      │   │  Graph       │   │  Audit Trail         │   │
│  │  (HNSW/Brute)│   │  (same store)│   │  (BLAKE3 + replay)   │   │
│  └──────────────┘   └──────────────┘   └──────────────────────┘   │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │           Q16.16 Fixed-Point Kernel  (no_std / no_alloc)    │  │
│  │   bit-identical results on x86 · ARM · RISC-V · Cortex-M4  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────┐   ┌──────────────────────────────────┐  │
│  │   Standalone Node     │   │   3- or 5-Node Raft Cluster      │  │
│  └───────────────────────┘   └──────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Key Features

| | |
|---|---|
| **Determinism** | Q16.16 fixed-point — bit-identical across x86, ARM, RISC-V, Cortex-M4 |
| **Audit trail** | Append-only BLAKE3-chained event log; offline verifiable with no server |
| **Tamper detection** | Locates the exact altered event, byte offset, and commit timestamp |
| **Raft cluster** | 3/5-node consensus via openraft 0.9 + tonic/gRPC + mTLS |
| **GraphRAG** | Vector search + subgraph traversal in one call, one consistent snapshot |
| **Agent memory (MCP)** | `valori-mcp` — verifiable recall with BLAKE3 receipt; works with Claude Desktop |
| **Recency decay** | `decay_half_life_secs` fades older memories in ranking without touching the state hash |
| **Valori Reranker** | Server-side hybrid retrieval — vector top-K pooled then re-scored by term frequency; 90% accuracy on hard lexical queries, 0.4 s latency, no external dependency |
| **Built-in ingest** | `POST /v1/ingest` — chunk + embed + insert + graph + audit in one call; works in standalone and 3/5-node cluster; `VALORI_EMBED_PROVIDER=ollama\|openai\|custom`; `/v1/ingest/document` for chunking only |
| **Tree-RAG** | `POST /v1/tree/{build,query,verify}` — navigate a doc's table-of-contents to the right section with breadcrumb + line citations and a replayable BLAKE3 retrieval receipt; deterministic, no embeddings, catches tampering |
| **Self-maintaining memory** | `consolidate` (supersede a memory) and `contradict` (flag conflicts) commit `Supersedes`/`Contradicts` edges to the audit chain |
| **Multi-tenancy** | Up to 1 024 named collections; per-tenant API keys with RBAC |
| **Point-in-time reads** | Replay to any past state hash or log index |
| **GDPR erasure** | Crypto-shredding — DEK destruction = O(1) erasure, audit chain stays intact |
| **Embedded** | `no_std` / `no_alloc` kernel; runs on microcontrollers with no heap |
| **S3 offload** | Snapshot archival + WAL rotation to S3/MinIO/R2 |

→ [Full feature list and phase history](docs/phases/README.md)

---

## Get Started

> **New contributor?** `bash dev-setup.sh` — one script installs Rust, the wasm32 target, Python SDK, and UI deps with OS detection and version gates. See [Build from Source](#build-from-source) and [CONTRIBUTING.md](CONTRIBUTING.md).

**Which client should I use?**

| Client | Install / import | Use when |
|---|---|---|
| `MemoryClient` | `pip install "valoricore[local]"` · `from valoricore import MemoryClient` | No server — Rust kernel runs inside your Python process (offline, embedded, CI) |
| `SyncRemoteClient` | `pip install valoricore` · `from valoricore.remote import SyncRemoteClient` | `valori-node` is running and you want synchronous HTTP calls |
| `AsyncRemoteClient` | same · `from valoricore.remote import AsyncRemoteClient` | Same node, but in an `async`/`await` context (FastAPI, asyncio) |
| `ClusterClient` | same · `from valoricore.remote import ClusterClient` | 3/5-node Raft cluster — pass all node URLs, leader failover is automatic |

Everything else (`Valoricore`, `ValoricoreAdapter`, `LocalClient`) is an advanced wrapper or legacy alias — you don't need it to get started.

---

### Option 1 — Python SDK, embedded (~30 seconds, no server, no compile)

```bash
pip install valoricore
```

```python
# Copy-paste runnable — no server, no API key, no ellipses.
import math, os, shutil
from valoricore import MemoryClient

DIM = 16
DB = "./hello_valori"
if os.path.exists(DB): shutil.rmtree(DB)

def embed(text):
    s = sum(ord(c) for c in text)
    return [math.sin(s + i * 0.3) for i in range(DIM)]

db = MemoryClient(path=DB, dim=DIM)
db.add_document(text="Valori proves it never lost your data.", embed=embed)
db.add_document(text="Fixed-point math is bit-identical on every machine.", embed=embed)

hits = db.semantic_search("cryptographic proof", embed=embed, k=2)
for h in hits:
    print(f"score={h['score']:.4f}  {h.get('metadata','')[:60]}")

print(db.get_state_hash())  # run this on any machine → same 64-char hex
shutil.rmtree(DB)
```

Run it twice, run it on a different OS — the hash is always identical. That's the guarantee.

**See tamper detection in 10 more lines:**

```python
# Continue from the snippet above (before shutil.rmtree).
good_hash = db.get_state_hash()

# Flip one byte in the event log — simulating silent corruption or a malicious edit.
with open(f"{DB}/events.log", "r+b") as f:
    f.seek(64); b = f.read(1)[0]; f.seek(64); f.write(bytes([b ^ 0xFF]))

# Reload from the corrupted log.
db2 = MemoryClient(path=DB, dim=DIM)
corrupt_hash = db2.get_state_hash()

print(good_hash == corrupt_hash)   # False — one bit changed the entire hash
print(f"expected : {good_hash}")
print(f"replayed : {corrupt_hash}")
# An attacker cannot forge a matching hash without breaking BLAKE3.
```

Full demo with `valori-verify` exact-byte-offset detection: [`examples/tamper_demo.py`](examples/tamper_demo.py)

To use a real embedding model instead of the mock `embed()` function:

```bash
pip install "valoricore[local]"
```

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")  # downloads ~90 MB once
db = MemoryClient(path="./my_db", dim=384)
db.add_document(text="The patient presented with hypertension.", embed=embedder)
hits = db.semantic_search("blood pressure", embed=embedder, k=5)
print(db.get_state_hash())
```

The Rust kernel runs inside your Python process via PyO3 — no server, no Docker, no Rust toolchain needed.

### Option 2 — Docker (~60 seconds, prebuilt image)

```bash
docker compose up -d
curl http://localhost:3000/health   # → {"status":"ok",...}
```

```bash
pip install valoricore
```

```python
# Copy-paste runnable after `docker compose up -d`.
# docker-compose.yml sets VALORI_DIM=1536, so vectors must be length 1536.
import math
from valoricore.remote import SyncRemoteClient

db  = SyncRemoteClient("http://localhost:3000")
dim = 1536
vec = [math.sin(i * 0.01) for i in range(dim)]   # deterministic placeholder vector

db.insert(vec, text="Valori proves it never lost your data.")
db.insert([math.cos(i * 0.01) for i in range(dim)], text="Fixed-point math, bit-identical everywhere.")

hits = db.search(vec, k=2)
for h in hits:
    print(f"score={h['score']:.4f}  {h.get('metadata','')[:60]}")

print(db.get_state_hash())   # same hex on every replica
```

Other search modes (swap for any of the `db.search` calls above):

```python
hits = db.search(vec, k=5, query_text="my query")                   # hybrid rerank
hits = db.search(vec, k=5, decay_half_life_secs=86400)              # recency-aware
hits = db.search(vec, k=5, metadata_filter={"author": "Alice"})     # metadata filter
hits = db.search(vec, k=5, metadata_filter={"year": {"gte": 2020}}) # range filter
```

Edit `docker-compose.yml` to change `VALORI_DIM` (default: 1536), add auth, or mount S3.

### Option 3 — One-call document ingest (chunk + embed on-node)

Add an embedding provider to Docker so clients can POST raw text — no client-side embedding needed:

```bash
VALORI_EMBED_PROVIDER=ollama VALORI_EMBED_MODEL=nomic-embed-text \
VALORI_EMBED_URL=http://localhost:11434 VALORI_DIM=768 \
  docker compose up -d
```

```python
from valoricore.remote import SyncRemoteClient

db = SyncRemoteClient("http://localhost:3000")
result = db.ingest(text, source="paper.pdf", strategy="auto", collection="research")
print(f"{result['chunk_count']} chunks inserted, doc node {result['document_node_id']}")
```

**Tree-RAG — jump to the right section instead of similar text:**

```python
built = db.tree_build(handbook_markdown, doc_name="handbook")
ans   = db.tree_query(built["tree"], "how many sick days do I get?")
print(ans["answer"], "—", ans["citations"][0]["breadcrumb"])  # lands on "… > Sick Leave"
assert db.tree_verify(built["tree"], ans["receipt"])          # proves it wasn't altered
```

### Option 4 — 3-node cluster

```bash
cargo install --path crates/valori-cli
valori setup   # interactive wizard
```

→ [Cluster setup guide](docs/CLUSTER.md) · [Docker Compose](docker-compose.cluster.yml) · [Helm chart](deploy/helm/valori/) · [AWS/Azure Terraform](docs/DEPLOY_AWS.md)

### Option 5 — Agent memory via MCP

```bash
VALORI_URL=http://localhost:3000 valori-mcp
```

```json
{ "mcpServers": { "valori": {
  "command": "valori-mcp",
  "env": { "VALORI_URL": "http://localhost:3000" }
} } }
```

→ [`crates/valori-mcp/README.md`](crates/valori-mcp/README.md)

### Option 6 — Web dashboard with persistent projects

```bash
cd ui && npm install && npm run dev   # http://localhost:3001
```

Each **project** is an isolated, persistent workspace: its own node, port, and
data dir under `~/.valori/projects/<name>/`. The Home screen lists every project
(even when its node is stopped); opening one auto-starts its node and restores
state, and closing it writes a snapshot and locks the files at rest — they can
only be deleted from the UI. → [`docs/phases/phase-6-persistent-projects.md`](docs/phases/phase-6-persistent-projects.md)

---

## Build from Source

> **Note:** Options 1 and 2 above don't require this. Build from source when you want to modify the Rust code, run CI, or start the node without Docker.
>
> **`dev-setup.sh`** — run once after cloning. Detects macOS/Linux, checks OS version, installs Rust via `rustup`, adds the `wasm32-unknown-unknown` target, installs `maturin` and the Python SDK in editable mode (`pip install -e python/`), and installs UI npm deps. After it finishes you have a fully wired dev environment.

```bash
# One-time setup — run from repo root
bash dev-setup.sh

# Build
cargo build --release -p valori-node

# Run (first-time cold compile: ~3–5 min; subsequent builds: ~10 s)
VALORI_DIM=128 \
VALORI_EVENT_LOG_PATH=./data/events.log \
VALORI_SNAPSHOT_PATH=./data/snapshot.bin \
  ./target/release/valori-node

# Tests
cargo test -p valori-kernel -p valori-node
```

Requires Rust 1.80+. For the Python FFI extension: `pip install maturin && maturin develop`.

---

## Documentation

| Doc | What it covers |
|---|---|
| [docs/getting-started.md](docs/getting-started.md) | Full quickstart for all deployment modes |
| [docs/api-reference.md](docs/api-reference.md) | Complete HTTP API reference |
| [docs/python-reference.md](docs/python-reference.md) | Full Python SDK reference |
| [docs/CLUSTER.md](docs/CLUSTER.md) | Cluster setup, operations, failover |
| [docs/DR.md](docs/DR.md) | Backup, restore, cross-region DR runbook |
| [docs/CAPACITY.md](docs/CAPACITY.md) | Capacity planning — vectors/GB, WAL growth, S3 cost |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | Security model and BLAKE3 MAC analysis |
| [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) | Docker, Kubernetes, S3, Terraform |
| [docs/authentication.md](docs/authentication.md) | API keys, RBAC, mTLS |
| [docs/core-concepts.md](docs/core-concepts.md) | Fixed-point math, audit chain, determinism |
| [docs/phases/README.md](docs/phases/README.md) | Full build history and phase reports |
| [benchmarks/RESULTS.md](benchmarks/RESULTS.md) | Benchmarks and comparison vs Pinecone/Qdrant/Weaviate |

---

## Research

**Paper:** [Valori: A Deterministic Memory Substrate for AI Systems](https://arxiv.org/abs/2512.22280)

```bibtex
@article{gudur2025valori,
  title   = {Valori: A Deterministic Memory Substrate for AI Systems},
  author  = {Gudur, Varshith},
  journal = {arXiv preprint arXiv:2512.22280},
  year    = {2025}
}
```

---

## License

Dual-licensed under **MIT OR Apache-2.0** — free for commercial use.

**Contact:** varshith.gudur17@gmail.com

---

<div align="center">

*Built in Rust. Proven in production. Auditable by mathematics.*

If Valori is useful to you, a star helps others find the project.

[![Star History](https://api.star-history.com/svg?repos=varshith-Git/Valoricore-Kernel&type=Date)](https://star-history.com/#varshith-Git/Valoricore-Kernel&Date)

</div>
