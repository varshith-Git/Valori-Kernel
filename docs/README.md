# Valori Documentation

The map of everything under `docs/`. Start with **Getting Started**, then branch
by what you're doing — running a cluster, integrating the SDK, or auditing a log.

## Start here

| Doc | What it covers |
|---|---|
| [getting-started.md](getting-started.md) | First insert and search, the 60-second tour |
| [core-concepts.md](core-concepts.md) | The model: events, commit barrier, deterministic state |
| [../README.md](../README.md) | Project overview, quick start, benchmarks |
| [../CHANGELOG.md](../CHANGELOG.md) | Release history |

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
| [phases/](phases/) | Per-phase design records (Phase 0 → 2.11) |
| [../architecture.md](../architecture.md) | The long-form architecture document |
