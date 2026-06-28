# Valori Documentation

> **New here?** Pick your path below — then come back to the reference tables when you need them.
> The [phase docs](phases/) are internal design records; you don't need them to use Valori.

---

## Choose your path

### "I don't write code — I want a UI"
1. `docker compose up -d` — starts the node
2. `cd ui && npm ci && npm run dev` — opens at http://localhost:3001
3. Create a project → the UI auto-starts a dedicated node, restores state on every open, and writes a snapshot on close

No configuration needed. The UI proxies to the node server-side — nothing to expose or hardcode.

---

### "I want to try it in 60 seconds — no server"
1. `pip install valoricore`
2. Run [`examples/hello_world.py`](../examples/hello_world.py) — inserts, searches, prints a BLAKE3 state hash
3. Run [`examples/tamper_demo.py`](../examples/tamper_demo.py) — flip a byte, watch the hash change

→ Deep dive: [embedded-quickstart.md](embedded-quickstart.md)

---

### "I'm building a Python app against a running node"
1. `docker compose up -d` (or `pip install valoricore` + `MemoryClient` for no-server)
2. Read [getting-started.md](getting-started.md) — covers insert, search, collections, auth
3. Read [python-reference.md](python-reference.md) — full SDK method reference

→ HTTP API surface: [api-reference.md](api-reference.md) · All endpoints are under `/v1/`

---

### "I'm deploying a production node / cluster"
1. [DEPLOYMENT.md](DEPLOYMENT.md) — Docker, EC2, env vars
2. [authentication.md](authentication.md) — bearer tokens and API keys
3. [CLUSTER.md](CLUSTER.md) — 3/5-node Raft setup, wizard, grow/shrink
4. [DR.md](DR.md) — snapshot-to-S3, restore, cross-region runbook
5. [THREAT_MODEL.md](THREAT_MODEL.md) — what Valori protects against

---

### "I need to verify an audit log / prove state hasn't changed"
1. [determinism-guarantees.md](determinism-guarantees.md) — what the guarantee means
2. [deterministic-proof.md](deterministic-proof.md) — how to produce and verify a proof
3. [crash-recovery-proof.md](crash-recovery-proof.md) — crash-symmetric recovery
4. Run `valori-verify events.log` — the standalone Rust verifier replays the chain

→ See also: [`examples/tamper_demo.py`](../examples/tamper_demo.py)

---

### "I'm contributing to the codebase"
1. [`CONTRIBUTING.md`](../CONTRIBUTING.md) — setup, conventions, PR checklist
2. `bash dev-setup.sh` — one-script dev environment
3. [core-concepts.md](core-concepts.md) — invariants and architecture before touching kernel code
4. [phases/README.md](phases/README.md) — design history (what was built and why)

---

## Full reference index

### Start here (if you skipped the paths above)

## Running Valori

| Doc | What it covers |
|---|---|
| [CLUSTER.md](CLUSTER.md) | **Multi-node operations** — wizard, `valori cluster` CLI, endpoints, grow/recover |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Deployment topologies, Docker, EC2, single-node and cluster |
| [MULTINODE_ROADMAP.md](MULTINODE_ROADMAP.md) | The Phase-2 cluster roadmap and what each phase delivered |
| [SHARDING.md](SHARDING.md) | **Design / roadmap** — horizontal scale via per-shard chains + Merkle-root proof (not yet implemented) |
| [authentication.md](authentication.md) | Bearer-token auth on the HTTP API |
| [remote-mode.md](remote-mode.md) | Talking to a node over HTTP vs the embedded engine |

## Python SDK

| Doc | What it covers |
|---|---|
| [python-usage-guide.md](python-usage-guide.md) | The `valoricore` package end to end |
| [python-reference.md](python-reference.md) | API reference for the Python client |
| [embedded-quickstart.md](embedded-quickstart.md) | The no-server embedded engine |
| [publishing-pypi.md](publishing-pypi.md) | Building and publishing the wheel |
| [api-reference.md](api-reference.md) | HTTP API endpoint reference |
| [functions.md](functions.md) | Function-level catalogue |

## Determinism & verification

| Doc | What it covers |
|---|---|
| [determinism-guarantees.md](determinism-guarantees.md) | What "deterministic" means here, and its limits |
| [deterministic-proof.md](deterministic-proof.md) | The cryptographic proof construction |
| [multi-arch-determinism.md](multi-arch-determinism.md) | Bit-identical state across CPU architectures |
| [build-determinism.md](build-determinism.md) | Reproducible builds |
| [verifiable-replication.md](verifiable-replication.md) | Verifying a replica matches the leader |
| [crash-recovery-proof.md](crash-recovery-proof.md) | Crash-symmetric recovery, proven |
| [wal-replay-guarantees.md](wal-replay-guarantees.md) | Write-ahead log replay semantics |
| [verification_report.md](verification_report.md) | A worked verification report |

## Formats & protocols

| Doc | What it covers |
|---|---|
| [SNAPSHOT_FORMAT.md](SNAPSHOT_FORMAT.md) | The V5 snapshot binary format |
| [memory_protocol_v1.md](memory_protocol_v1.md) | Memory protocol (current) |
| [memory_protocol_v0.md](memory_protocol_v0.md) | Memory protocol (legacy) |
| [adapter-improvements.md](adapter-improvements.md) | Embedding-adapter notes |

## Operations & security

| Doc | What it covers |
|---|---|
| [THREAT_MODEL.md](THREAT_MODEL.md) | What Valori protects against, what it doesn't, keyed BLAKE3 MAC analysis |
| [CAPACITY.md](CAPACITY.md) | Vectors/GB by dimension, RAM/1 M vectors, node sizing, WAL growth rates |
| [DR.md](DR.md) | Snapshot-to-S3, full cluster restore, cross-region active-passive, verification checklist |

## Internals

| Doc | What it covers |
|---|---|
| [architecture/](architecture/) | Cluster write-flow diagram and architecture notes |
| [../architecture.md](../architecture.md) | The long-form architecture document |

## Internal — phase design records

> These are engineering design journals, not user documentation. Each file records what a phase set out to do, what landed, and what was deferred. You don't need to read these to use or deploy Valori.

[phases/README.md](phases/README.md) — status table with links to all 64 phase docs.
