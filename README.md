<div align="center">

<img src="assets/valori-logo.png" alt="Valori" width="72" />

# Valori

**The vector database that can mathematically prove it never lost your data.**

[![Version](https://img.shields.io/badge/version-0.2.2-6c47ff?style=flat-square&logo=rust)](Cargo.toml)
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
| **Self-maintaining memory** | `consolidate` (supersede a memory) and `contradict` (flag conflicts) commit `Supersedes`/`Contradicts` edges to the audit chain |
| **Multi-tenancy** | Up to 1 024 named collections; per-tenant API keys with RBAC |
| **Point-in-time reads** | Replay to any past state hash or log index |
| **GDPR erasure** | Crypto-shredding — DEK destruction = O(1) erasure, audit chain stays intact |
| **Embedded** | `no_std` / `no_alloc` kernel; runs on microcontrollers with no heap |
| **S3 offload** | Snapshot archival + WAL rotation to S3/MinIO/R2 |

→ [Full feature list and phase history](docs/phases/README.md)

---

## Get Started

### Option 1 — Python SDK, embedded (no server)

```bash
pip install valoricore
pip install "valoricore[local]"   # + SentenceTransformer embeddings
```

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
db = MemoryClient(path="./my_db", dim=384, index_kind="hnsw")

db.add_document(text="The patient presented with hypertension.", embed=embedder)
hits = db.semantic_search("blood pressure", embed=embedder, k=5)
print(db.get_state_hash())   # 64-char BLAKE3 hex — same on any machine
```

### Option 2 — HTTP server (standalone node)

```bash
VALORI_DIM=1536 \
VALORI_EVENT_LOG_PATH=./data/events.log \
VALORI_SNAPSHOT_PATH=./data/snapshot.bin \
  cargo run --release -p valori-node
```

```python
from valoricore import SyncRemoteClient
db = SyncRemoteClient("http://localhost:3000")
db.insert([0.1, 0.2, ...])
hits = db.search([0.1, 0.2, ...], k=5)
hits = db.search([0.1, 0.2, ...], k=5, decay_half_life_secs=86400)  # recency-aware
```

### Option 3 — 3-node cluster

```bash
cargo install --path crates/valori-cli
valori setup   # interactive wizard
```

→ [Cluster setup guide](docs/CLUSTER.md) · [Docker Compose](docker-compose.cluster.yml) · [Helm chart](deploy/helm/valori/) · [AWS/Azure Terraform](docs/DEPLOY_AWS.md)

### Option 4 — Agent memory via MCP

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

---

## Build from Source

```bash
cargo build --release --workspace
cargo test -p valori-kernel -p valori-node
cd python && pip install -e ".[dev]"
```

Requires Rust stable. For Python FFI: `cargo install maturin`.

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
