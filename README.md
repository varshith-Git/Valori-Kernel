<div align="center">

<img src="assets/valori-logo.png" alt="Valori" width="72" />

# Valori

**The vector database that can mathematically prove it never lost your data.**

[![Version](https://img.shields.io/badge/version-0.2.1-6c47ff?style=flat-square&logo=rust)](Cargo.toml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Build](https://img.shields.io/github/actions/workflow/status/varshith-Git/Valoricore-Kernel/ci.yml?style=flat-square&label=CI)](https://github.com/varshith-Git/Valoricore-Kernel/actions)
[![Determinism](https://img.shields.io/badge/determinism-multi--arch%20verified-brightgreen?style=flat-square)](.github/workflows/multi-arch-determinism.yml)
[![arXiv](https://img.shields.io/badge/arXiv-2512.22280-b31b1b?style=flat-square)](https://arxiv.org/abs/2512.22280)
[![Tests](https://img.shields.io/github/actions/workflow/status/varshith-Git/Valori-Kernel/test-count.yml?label=tests&style=flat-square)](https://github.com/varshith-Git/Valori-Kernel/actions/workflows/test-count.yml)

*Q16.16 fixed-point arithmetic · BLAKE3 hash-chained audit log · openraft consensus · offline verifiable proofs*

</div>

---

## The Problem with Every Vector Database

Every vector database in production makes one silent assumption: floating-point arithmetic on one machine produces the same result on another. It does not. IEEE 754 allows implementation variance. SIMD units introduce rounding differences. Cloud vendors migrate workloads to new hardware without telling you.

The consequences compound in AI systems:

- Two replicas of the "same" database produce different state hashes — you cannot verify consistency.
- Crash recovery is unverifiable — you trust the vendor's dashboard, not math.
- An audit trail grounded in float results cannot be reproduced on different hardware.
- AI agent memory that drifts silently between calls is worse than no memory at all.

**Valori eliminates all of these failure modes with one architectural decision: integer-only vector math, provably identical on every machine.**

---

## Production Proof

```bash
# State hash before a forced restart on Koyeb
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

## Where Valori Sits in Your AI Stack

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Your AI Application                            │
│   LangChain · LlamaIndex · OpenAI Agents · Custom Orchestrators    │
└────────────────────────┬────────────────────────────────────────────┘
                         │  Python SDK  /  HTTP  /  PyO3 FFI
┌────────────────────────▼────────────────────────────────────────────┐
│                         VALORI                                      │
│                                                                     │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────────┐   │
│  │  Vector      │   │  Knowledge   │   │  Cryptographic       │   │
│  │  Memory      │   │  Graph       │   │  Audit Trail         │   │
│  │  (HNSW/IVF)  │   │  (same store)│   │  (BLAKE3 + replay)   │   │
│  └──────────────┘   └──────────────┘   └──────────────────────┘   │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │           Q16.16 Fixed-Point Kernel  (no_std / no_alloc)    │  │
│  │   bit-identical results on x86 · ARM · RISC-V · Cortex-M4  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  ┌───────────────────────┐   ┌──────────────────────────────────┐  │
│  │   Standalone Node     │   │   3- or 5-Node Raft Cluster      │  │
│  │   events.log          │   │   events.log (per node)          │  │
│  │   snapshot.bin        │   │   raft.redb (per node)           │  │
│  │   wal (legacy)        │   │   openraft 0.9 + tonic/gRPC      │  │
│  └───────────────────────┘   └──────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
   Local disk /                  Kubernetes / EC2
   Docker volume             PersistentVolumeClaim + S3 (Phase 3)
```

Valori is the **memory layer** of your AI stack — the place where embedding vectors, knowledge-graph relationships, and the cryptographic proof that they were never corrupted all live together. It is not a managed cloud service. It is the foundation you own, audit, and verify.

---

## Features

### Shipped (v0.2.1)

| Category | Feature |
|---|---|
| **Determinism** | Q16.16 fixed-point arithmetic — bit-identical across x86, ARM, RISC-V |
| **Determinism** | Multi-architecture CI determinism test (`multi-arch-determinism.yml`) |
| **Audit** | Append-only, BLAKE3 hash-chained event log (wire format v3) |
| **Audit** | Cross-segment chain — deleting an archived segment breaks the chain |
| **Audit** | `valori-verify` offline auditor — no server, no trust required |
| **Audit** | Tamper localization — names the exact altered event, byte offset, timestamp |
| **Index** | BruteForce (exact, ≤ 50k), HNSW (approximate, millions), IVF (batch) |
| **Graph** | Knowledge graph (nodes + edges) in the same memory space as vectors |
| **Graph** | Cascade delete — removes all node edges in O(degree), no full scan |
| **Persistence** | Snapshot: full kernel state with CRC32 header, `.prev` rotation |
| **Persistence** | Event log rotation at configurable size (default 256 MiB), segment sequence named |
| **Persistence** | Multi-segment recovery — replays sealed archives + live in sequence order |
| **Persistence** | Splice verification — a missing or substituted archive is caught, not skipped |
| **Cluster** | 3- or 5-node Raft cluster via openraft 0.9 + tonic/gRPC transport |
| **Cluster** | Persistent Raft log via redb (`VALORI_RAFT_LOG_PATH`) |
| **Cluster** | Mutual TLS on the Raft channel (wrong-CA peers refused at handshake) |
| **Cluster** | Linearizable reads by default (read-index protocol) |
| **Cluster** | Follower → leader write redirect (307); SDK follows automatically |
| **Cluster** | Snapshot-based catch-up for late joiners (openraft `InstallSnapshot`) |
| **Cluster** | Active divergence detection — `valori_raft_state_hash_match` Prometheus gauge |
| **Cluster** | `GET /v1/cluster/role` — load-balancer leader-routing endpoint |
| **Cluster** | Interactive setup wizard (`valori setup`) |
| **Cluster** | Helm chart (`deploy/helm/valori/`) — StatefulSet + PVCs |
| **SDK** | Python: `SyncRemoteClient`, `AsyncRemoteClient`, `MemoryClient` |
| **SDK** | `ClusterClient` / `AsyncClusterClient` — round-robin reads, leader-routed writes, 307-redirect self-heal |
| **SDK** | Leader-redirect caching, retry/backoff, per-write UUID idempotency keys |
| **SDK** | LangChain + LlamaIndex integration |
| **SDK** | API key management: `create_key()`, `list_keys()`, `revoke_key()` |
| **Agent memory** | `valori-mcp` — Model Context Protocol server; plug Valori into Claude Desktop or any agent as long-term memory |
| **Agent memory** | Verifiable recall — `memory_recall` returns a BLAKE3 **receipt** binding the result set to the committed state hash; recomputable offline by any client |
| **GraphRAG** | `POST /v1/graphrag` — K nearest vectors **+** the connected subgraph in one call, one consistent snapshot; no Neo4j+vector-DB two-system stack |
| **GraphRAG** | `memory_graph_recall` MCP tool — GraphRAG with a receipt binding both the hits and the returned subgraph |
| **Self-maintaining memory** | Recency decay — `decay_half_life_secs` on search fades older memories in ranking; a read-time re-rank that never touches the state hash (`memory_recall` supports it too) |
| **Multi-tenancy** | Named collections (namespaces) — up to 1 024 per node, fully isolated vector search |
| **Multi-tenancy** | Per-tenant API keys (`/v1/keys`) — `read_only`, `read_write`, `admin` scopes; BLAKE3-hashed token store |
| **Point-in-time** | `search(query, as_of="2026-01-01T00:00:00Z")` or `as_of_log_index=N` — replays event log to target point |
| **Migration** | `valori import qdrant` — scroll API, dim-validated, resumable, per-record idempotency |
| **Migration** | `valori import jsonl` — streaming JSONL ingestion with alias-aware field names |
| **Embedded** | `no_std` / `no_alloc` kernel — validated on ARM Cortex-M4 @ 168 MHz |
| **Observability** | Prometheus metrics at `/metrics` (`valori_raft_*`, commit latency, replay duration) |
| **CLI** | `valori inspect`, `verify`, `timeline`, `replay-query`, `diff`, `cluster`, `import` |
| **Tests** | Unit, integration, openraft compliance suite, proptest fuzz — count auto-updated by CI |

### Shipped — Phase 3 complete

| Feature | What it unlocks |
|---|---|
| **Crypto-shredding (3.6)** | Per-record AES-256-GCM; DEK destruction = GDPR erasure in O(1) |
| **Import (3.7)** | `valori import` — Qdrant + JSONL migration, one command |
| **As-of reads (3.4)** | Point-in-time search against any past state hash |
| **Write-throughput CI gates (3.8)** | PR fails if p99 insert latency regresses > 15% vs `main` baseline |
| **Terraform modules (3.9)** | AWS / Azure one-command deployments |
| **Signed releases + SBOM (3.10)** | Cosign signatures, CycloneDX SBOM, SLSA Level 2 provenance |
| **Concurrent reads (3.11)** | `RwLock` engine — reads no longer block each other |
| **Batch idempotency (3.12)** | Per-item dedup in batch insert; safe to replay on failure |
| **HNSW parameter exposure (3.13)** | `ef_construction`, `M`, `ef_search` configurable at runtime |
| **MCP server (3.14)** | `valori-mcp` — verifiable agent memory for Claude Desktop and any MCP client |
| **Native GraphRAG (3.15)** | `POST /v1/graphrag` — K nearest + subgraph traversal, one call, one consistent snapshot |

### Coming next — Cortex (self-maintaining memory)

| Feature | Phase |
|---|---|
| **Cluster decay** — durable creation timestamps in `ValoriStateMachine` | C4.1b |
| **Consolidation** — scheduled merge via `SoftDeleteRecord` + `AutoInsertRecord` + `RefersTo` edges, all committed | C4.2 |
| **Contradiction detection** — claim-level NLI, verdict committed as a `RefersTo` edge event | C4.3 |

---

## Get Started

### Option 1 — Python SDK, no server (embedded local engine)

```bash
pip install valoricore                   # core only
pip install "valoricore[local]"          # + SentenceTransformer embeddings
pip install "valoricore[all]"            # + OpenAI, Cohere, LangChain, LlamaIndex
```

```python
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")

db = MemoryClient(path="./my_db", index_kind="hnsw")

# Store a memory
db.add_document(
    text  = "The patient presented with stage-2 hypertension on 2026-01-12.",
    embed = embedder,
    title = "clinical-note-001",
)

# Recall
hits = db.semantic_search("blood pressure diagnosis", embed=embedder, k=5)
for h in hits:
    print(h["id"], h["score"], h["metadata"])

# Cryptographic proof — same hash on any machine
print(db.get_state_hash())  # 64-char BLAKE3 hex
```

### Option 2 — HTTP server (standalone node)

```bash
cargo install --path crates/valori-node   # or: cargo build --release -p valori-node

VALORI_DIM=1536 \
VALORI_INDEX=hnsw \
VALORI_EVENT_LOG_PATH=./data/events.log \
VALORI_SNAPSHOT_PATH=./data/snapshot.bin \
  valori-node
# Listening on 127.0.0.1:3000
```

Connect from Python:

```python
from valoricore import SyncRemoteClient

db = SyncRemoteClient("http://localhost:3000")
rid = db.insert([0.1, 0.2, ...])          # returns record id
hits = db.search([0.1, 0.2, ...], k=5)   # [{"id": 0, "score": 0.99}, ...]
```

### Option 3 — 3-node cluster (interactive wizard)

```bash
cargo install --path crates/valori-cli

valori setup          # interactive wizard: choose Multi-node → 3 nodes → done
# For a server or EC2 where clients connect from outside:
valori setup --bind 0.0.0.0
```

The wizard starts all three nodes in one process on ports `51000–51002` (API) and `51100–51102` (Raft), persists the project to `~/.valori/projects.json`, and drops you into a live menu.

### Option 4 — Docker Compose (production-equivalent 3-node cluster)

```bash
docker compose up -d --build
docker compose ps     # wait for all 3 healthy (~30 s)

curl http://localhost:3001/health
curl -X POST http://localhost:3001/records \
  -H 'Content-Type: application/json' \
  -d '{"values": [0.1, 0.2, 0.3]}'
```

Tear down: `docker compose down -v`

### Option 5 — Kubernetes (Helm)

```bash
helm install valori ./deploy/helm/valori \
  --set replicaCount=3 \
  --set image.tag=0.2.1 \
  --set persistence.events.size=50Gi

kubectl get pods -l app=valori   # 3/3 Running
```

See [`deploy/helm/valori/values.yaml`](deploy/helm/valori/values.yaml) for storage classes, resource limits, mTLS, anti-affinity, and probe configuration.

### Option 6 — Manual cluster (full control)

```bash
# Node 1 — bootstraps the cluster (VALORI_CLUSTER_INIT=1)
VALORI_NODE_ID=1 \
VALORI_CLUSTER_INIT=1 \
VALORI_CLUSTER_MEMBERS="1=10.0.0.1:3100/10.0.0.1:3000,2=10.0.0.2:3100/10.0.0.2:3000,3=10.0.0.3:3100/10.0.0.3:3000" \
VALORI_BIND=0.0.0.0:3000 \
VALORI_RAFT_BIND=0.0.0.0:3100 \
VALORI_EVENT_LOG_PATH=/data/events.log \
VALORI_RAFT_LOG_PATH=/data/raft.redb \
  valori-node

# Nodes 2 and 3 — same env, VALORI_NODE_ID=2/3, NO VALORI_CLUSTER_INIT
```

### Option 7 — Agent memory via MCP (Claude Desktop / any agent)

Give an agent verifiable long-term memory. `valori-mcp` is a Model Context
Protocol server that fronts a running node; `memory_recall` returns a BLAKE3
**receipt** proving exactly what was recalled against the committed state.

```bash
cargo build -p valori-node -p valori-mcp

# 1. start a node (event log on → receipts carry an event-log hash)
VALORI_DIM=8 VALORI_EVENT_LOG_PATH=./data/events.log valori-node &

# 2. point the MCP server at it
VALORI_URL=http://localhost:3000 valori-mcp
```

Wire it into Claude Desktop (`claude_desktop_config.json`):

```json
{ "mcpServers": { "valori": {
  "command": "valori-mcp",
  "env": { "VALORI_URL": "http://localhost:3000" }
} } }
```

Runnable demo with cross-language receipt verification:
`python3 examples/mcp_agent_memory.py`. Details in
[`crates/valori-mcp/README.md`](crates/valori-mcp/README.md).

---

## Architecture

### Single-Node: The Commit Barrier

No mutation reaches the in-memory kernel without first being fsynced to the append-only event log. Every write follows a strict four-phase protocol:

```
[Client Write]
      │
      ▼
[Shadow Execute] ── clone of live state; validates the event safely
      │
      ├─ rejected ──► return error  (log is never written)
      │
      ▼
[fsync to events.log] ── durable on disk before any live state changes
      │
      ▼
[Apply to KernelState] ── update in-memory vectors + index + graph
      │
      ▼
[BLAKE3 state root updated] ── always consistent with the log on disk
```

If the process dies at any point — even mid-write — recovery replays the event log. The final state hash is guaranteed to match the pre-crash hash. A kill-test in the suite (`tests/crash_durability.rs`) proves events acknowledged with HTTP 200 survive `SIGKILL`.

Batch inserts amortize to one fsync per batch without weakening the guarantee.

---

### Multi-Node: Raft Cluster

A Valori cluster is an odd number of nodes (3 in practice, 5 for two-fault tolerance) running Raft consensus via **openraft 0.9**.

```
                    ┌─────────────────────────────────────────────┐
                    │               CLIENT                        │
                    └──────────┬──────────────┬───────────────────┘
                               │ write        │ read (any node)
                    ┌──────────▼──────────┐   │
                    │      LEADER          │◄──┘
                    │  (Node 1)           │
                    │  raft.redb          │──── heartbeat / AppendEntries ────►┐
                    │  events.log         │                                    │
                    └─────────────────────┘                                    │
                           │ AppendEntries (gRPC/mTLS)                        │
                    ┌──────▼──────────────┐  ┌───────────────────────────────▼─┐
                    │    FOLLOWER          │  │    FOLLOWER                     │
                    │    (Node 2)          │  │    (Node 3)                     │
                    │    raft.redb         │  │    raft.redb                    │
                    │    events.log        │  │    events.log                   │
                    └─────────────────────┘  └─────────────────────────────────┘
```

**What the leader does that followers do not:**
- Accepts writes (followers answer `307 Temporary Redirect` with the leader's address)
- Drives Raft log replication (`AppendEntries` RPC)
- Runs elections when heartbeats stop

**What every node does independently:**
- Applies committed entries to its own `KernelState` (deterministic → byte-identical on all nodes)
- Appends committed events to its own `events.log` (after apply, at apply, exactly once)
- Serves reads (linearizable by default via read-index; eventually-consistent opt-in)
- Runs the divergence-detection watcher

#### Leader election

The leader sends periodic heartbeats. If a follower goes `election_timeout_min` (800 ms) without a heartbeat it starts a vote. The first node to gather a majority of votes becomes leader. Election timeout is randomized between `election_timeout_min` and `election_timeout_max` (1600 ms) to avoid split votes.

#### Linearizable reads (read-index protocol)

A follower does not serve a read until it catches up to the leader's commit index:

```
Client → Follower: search(query, k=5)
Follower → Leader: GET /v1/cluster/read-index          # "what is your commit index?"
Leader → (quorum heartbeat) → confirms still leader
Leader → Follower: commit_index = C
Follower: wait until applied_index >= C
Follower → Client: search results  (reflect every write committed before this read)
```

The leader serves reads via openraft's `ensure_linearizable()` (one quorum heartbeat). Opt into fast local reads with `consistency: "local"` — no round trip, eventually consistent.

#### Fault tolerance

| Scenario | Outcome |
|---|---|
| Follower down | No impact — leader + remaining followers still form quorum |
| Leader down | Remaining nodes elect a new leader (< 2 s); writes auto-resume |
| Network partition | Majority side continues; minority **stalls** (does not fork) |
| Rejoining node | Catches up via log replay or snapshot install, whichever is shorter |

---

### The Two Logs — Never Conflated

Every cluster node maintains exactly two files:

```
raft.redb
  └─ the Raft consensus scratchpad
     - entries being voted on
     - this node's current ballot (voted-for + term)
     - truncated on leader conflict resolution
     - purged after snapshot compaction
     - NEVER shown to auditors
     - stays small (a few thousand entries at most)

events.log  (+ events.log.000001, events.log.000002, ...)
  └─ the cryptographic audit diary
     - committed events ONLY, written AFTER quorum, AFTER apply
     - append-only, never truncated, never purged from the chain
     - BLAKE3 hash-chained entry-by-entry
     - cross-segment chain: removing an archived segment breaks the chain
     - this is what valori-verify audits
     - this is what recovery replays
```

**The Raft log can be rebuilt; the audit log is evidence.** Never treat them as the same thing.

---

### Snapshot: Two Jobs

A periodic snapshot of `KernelState` does double duty.

#### Job A — Raft catch-up (`InstallSnapshot`)

When a node joins late, or a follower falls so far behind the leader has already trimmed the Raft entries it needs, the leader ships the kernel snapshot via the `InstallSnapshot` RPC. The joiner installs it (jumping to snapshot index `S`), then replays the remaining tail `S+1…C` through normal `AppendEntries`. openraft drives this automatically.

```
New node joins
      │
      ▼
Leader: raft log already compacted past the joiner's position?
      │
      ├─ yes ──► InstallSnapshot RPC ──► joiner installs ──► replay tail
      │
      └─ no  ──► AppendEntries from gap ──► caught up
```

#### Job B — Event log rotation

"Append-only forever" is correct for audit but unbounded on disk. Once the live `events.log` passes `VALORI_EVENT_LOG_ROTATION_BYTES` (default 256 MiB):

```
events.log  (live, 256 MiB)
      │
      ▼ rotation triggered
events.log.000001  (sealed, archived, BLAKE3-finalized)
events.log          (fresh segment, chain continues from sealed segment's final hash)
```

Segments are named by monotone sequence number, never a timestamp — two rotations in the same second can't collide. **Recovery replays every local segment in sequence order**, verifying each splice. A missing or substituted archive breaks the splice and is reported, not silently skipped.

#### Snapshot self-verification

The `SnapshotPayload` carries the expected BLAKE3 state hash. On `install_snapshot` the receiver decodes the kernel, recomputes the hash, and **refuses the snapshot if it doesn't match**. This catches corruption the V5 decode format cannot see (a flipped byte mid-payload decodes "successfully" into corrupt state).

---

### Active Divergence Detection

Each node runs a background watcher (default: every 30 s, `VALORI_STATE_HASH_CHECK_SECS`) that calls `/v1/proof/state` on every peer and compares hashes:

```
Node 1 watcher → GET /v1/proof/state (Node 2) → compare
Node 1 watcher → GET /v1/proof/state (Node 3) → compare
                         │
              all match? ──► valori_raft_state_hash_match = 1
              any mismatch? ──► valori_raft_state_hash_match = 0
                                + ERROR log + divergence counter
```

Alert on:
```promql
valori_raft_state_hash_match == 0
```

Unreachable peers (rolling restart) are not counted as mismatches — only a hash mismatch fires the gauge.

---

## Event Log Deep Dive

### Wire format v3

```
┌─────────────────────────────────────────────────────┐
│  48-byte header                                      │
│  ├─ version: u32       (= 3)                        │
│  ├─ dim: u32           embedding dimension          │
│  ├─ format_id: u8      (1 = Q16.16)                 │
│  ├─ reserved: 3 bytes                               │
│  ├─ segment_seq: u32   (0 = genesis)                │
│  └─ prev_segment_chain_head: [u8; 32]               │
│     BLAKE3 final hash of the previous segment        │
└─────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────┐
│  Entry[0]                                            │
│  ├─ wall_time_secs: u64                             │
│  ├─ request_id: Option<[u8; 16]>  idempotency token │
│  ├─ event: KernelEvent  (bincode)                   │
│  └─ chain_hash[0] = BLAKE3(                         │
│         prev_segment_chain_head                     │
│       ║ bincode((wall_time, request_id, event))     │
│     )                                               │
└─────────────────────────────────────────────────────┘
  ...
┌─────────────────────────────────────────────────────┐
│  Entry[N]                                            │
│  └─ chain_hash[N] = BLAKE3(chain_hash[N-1] ║ ...)  │
└─────────────────────────────────────────────────────┘
```

Every entry carries its predecessor's hash. An in-place edit to entry `i` breaks the chain at entry `i+1`. `valori-verify` locates the exact event, decodes its contents, and reports the byte offset and commit timestamp — without access to the running server.

### Rotation and cross-segment chain

When the segment rotates, the new segment's header records the sealed segment's final `chain_hash`. Verification spans all segments:

```
events.log.000001  (sealed)
  final_chain_hash = 0xABCD...

events.log.000002  (next segment)
  header.prev_segment_chain_head = 0xABCD...   ← must match
  chain_hash[0] = BLAKE3(0xABCD... ║ entry[0])
```

Deleting `events.log.000001` from the archive makes `events.log.000002` fail the splice check at load time — whole-segment removal is detectable, unlike v2 where every segment restarted from zeros.

---

## Python SDK

### Single-node remote client

```python
from valoricore import SyncRemoteClient

# Point at any node — the SDK follows 307 leader-redirects automatically
db = SyncRemoteClient(
    "http://10.0.0.1:3000",
    max_retries    = 5,
    retry_backoff  = 0.2,   # seconds
)

# Insert (UUID idempotency key auto-generated; safe to retry on connection reset)
record_id = db.insert([0.1, 0.2, 0.3, ...])

# Batch insert
ids = db.batch_insert([[0.1, ...], [0.2, ...]])

# Named collections (multi-tenancy)
db.create_collection("tenant-acme")
db.insert([0.1, 0.2, ...], collection="tenant-acme")
hits = db.search([0.1, 0.2, ...], k=5, collection="tenant-acme")

# Search (linearizable by default)
hits = db.search([0.1, 0.2, ...], k=10)
# → [{"id": 0, "score": 0.997}, {"id": 7, "score": 0.841}, ...]

# Point-in-time search (as-of reads) — requires VALORI_EVENT_LOG_PATH on server
hits = db.search([0.1, 0.2, ...], k=5, as_of="2026-01-01T00:00:00Z")
hits = db.search([0.1, 0.2, ...], k=5, as_of_log_index=500)
# → {"results": [...], "as_of_state_hash": "a3f2...", "as_of_log_index": 500}

# Fast local search (eventually consistent, no leader round-trip)
hits = db.search([0.1, ...], k=10, consistency="local")

# GraphRAG — K nearest vectors + the connected subgraph, in one call
g = db.graphrag([0.1, 0.2, ...], k=5, depth=2)
# → {"hits": [...], "seed_nodes": [...], "subgraph": {"nodes": [...], "edges": [...]}}

# Recency-aware search (Phase C4.1) — older records fade in ranking.
# Each hit gains decay_factor + age_secs; score stays the true distance.
hits = db.search([0.1, ...], k=5, decay_half_life_secs=86400)  # 1-day half-life

# Cluster health
print(db.cluster_status())
# → {"leader": 1, "term": 3, "members": [...]}

# Cryptographic state proof
print(db.get_state_hash())  # same 64-char hex on all three nodes
```

### Multi-node cluster client (Phase 3.3)

```python
from valoricore import ClusterClient

# All three node URLs — client discovers the leader automatically via 307
cluster = ClusterClient([
    "http://10.0.0.1:3000",
    "http://10.0.0.2:3000",
    "http://10.0.0.3:3000",
])

# Writes go to the leader; idempotency key ensures exactly-once on retry
rid = cluster.insert([0.1, 0.2, ...], collection="tenant-acme")

# Round-robin reads across all nodes (high throughput, eventually consistent)
hits = cluster.search([0.1, 0.2, ...], k=5, consistency="local")

# Linearizable reads go to the leader (default)
hits = cluster.search([0.1, 0.2, ...], k=5, consistency="linearizable")

# Cluster inspection
print(cluster.leader_url())       # → "http://10.0.0.2:3000"
print(cluster.get_cluster_role()) # → "leader" or "follower"
print(cluster.cluster_health())   # → True / False
```

### API Key management (Phase 3.5)

```python
from valoricore import SyncRemoteClient

admin = SyncRemoteClient("http://localhost:3000", token="<admin-token>")

# Create a scoped key
key = admin.create_key(scope="read_write", description="acme pipeline")
print(key["token"])   # vk_<64 hex> — shown ONCE, store it now
print(key["id"])      # key_a3f2...

# List keys (token masked)
keys = admin.list_keys()

# Revoke a key
admin.revoke_key(key["id"])

# Use the new key
tenant_client = SyncRemoteClient("http://localhost:3000", token=key["token"])
tenant_client.insert([0.1, 0.2, ...], collection="tenant-acme")
```

### Async client

```python
from valoricore import AsyncRemoteClient

async with AsyncRemoteClient("http://10.0.0.1:3000") as db:
    rid   = await db.insert([0.1, 0.2, ...])
    hits  = await db.search([0.1, 0.2, ...], k=5)
    state = await db.get_state_hash()
```

### Embedded local client (no server)

```python
from valoricore import MemoryClient

db = MemoryClient(
    path       = "./my_db",
    index_kind = "hnsw",   # "bruteforce" | "hnsw" | "ivf"
    dim        = 1536,
)

db.add_document(text="...", embed=embedder)
hits = db.semantic_search("query", embed=embedder, k=5)
```

### LangChain / LlamaIndex

```python
# LangChain vector store
from valoricore.langchain import ValoriVectorStore
vectorstore = ValoriVectorStore.from_documents(docs, embeddings)

# LlamaIndex
from valoricore.llamaindex import ValoriIndex
index = ValoriIndex.from_documents(documents)
```

---

## CLI Reference

Install:

```bash
cargo install --path crates/valori-cli
```

### Inspect a database directory

```bash
valori inspect --dir ./my_valori_db
# prints: record count, index type, graph stats, snapshot status, event log health
```

### Verify an event log (offline, no server)

```bash
valori verify events.log
valori verify events.log --expected-hash 2dfad476977709f3...
valori verify events.log --expected-hash <HEX> --report forensics.json

# Exit codes: 0 = VERIFIED, 1 = TAMPERED
```

### Forensic timeline

```bash
valori timeline events.log
# prints every event: index, type, timestamp, chain hash
```

### Point-in-time replay

```bash
# Replay to event #200 and run a search
valori replay-query \
  --snapshot snapshot.bin \
  --log events.log \
  --at 200 \
  --query '[0.1, -0.5, 0.8]' \
  --top-k 5
```

### Diff between two moments

```bash
valori diff \
  --snapshot snapshot.bin \
  --log events.log \
  --from 150 --to 200 \
  --query '[0.1, -0.5, 0.8]'
# shows which records entered/left the top-5 between events 150 and 200
```

### Cluster management

```bash
valori cluster status  --url http://10.0.0.1:3000
valori cluster health  --url http://10.0.0.1:3000   # exits 0 if leader exists

valori cluster add-node --url http://10.0.0.1:3000 \
  --id 4 \
  --raft-addr 10.0.0.4:3100 \
  --api-addr  10.0.0.4:3000

valori cluster remove-node --url http://10.0.0.1:3000 --id 4

# Rolling upgrade — drains and restarts nodes one at a time, leader last.
valori cluster upgrade --url http://10.0.0.1:3000 --target-version 0.3.0
```

`add-node` does the two-step openraft dance: adds as learner (catch-up without affecting quorum), then promotes to voter. The new node must already be running.

`upgrade` is interactive: it prints the upgrade plan (non-leaders first, leader last), waits for the operator to cycle each node, and polls `/health` before moving to the next. See [`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md) for the rolling-window rules and schema version policy.

### Migrate from another vector database (Phase 3.7)

The `valori import` command imports into a **running** Valori node. It validates
that the source dimension matches the target's `VALORI_DIM` before writing
anything — a mismatch aborts with the exact env var to set and retry.

**From Qdrant:**

```bash
# Start Valori with the right dimension first
VALORI_DIM=384 VALORI_EVENT_LOG_PATH=./events.log valori-node

# Import — dim-checked, cursor-resumable, progress bar
valori import qdrant \
  --url http://localhost:6333 \
  --collection my-embeddings \
  --target-url http://localhost:3000 \
  --target-collection my-embeddings \
  --batch-size 500

# If the run was interrupted, pick up where it left off
valori import qdrant \
  --url http://localhost:6333 \
  --collection my-embeddings \
  --target-url http://localhost:3000 \
  --target-collection my-embeddings \
  --resume
```

**From JSONL (universal format):**

```bash
# Line format: {"vector": [...], "metadata": "...", "tag": 0}
# Aliases: "embedding"/"values" for vector, "text"/"content"/"payload" for metadata
valori import jsonl ./my-vectors.jsonl \
  --target-url http://localhost:3000 \
  --target-collection tenant-acme
```

On completion, the final BLAKE3 state hash is printed — import provenance is in
the audit chain from the first inserted record.

**Dimension migration workflow** (changing embedding model — e.g. 384-dim → 1536-dim):

```bash
# 1. Create a new Valori instance (or collection) with the new dimension
VALORI_DIM=1536 valori-node --port 3001

# 2. Re-embed your corpus with the new model and export to JSONL
python3 reembed.py --model text-embedding-3-small --out new-embeddings.jsonl

# 3. Import the re-embedded data
valori import jsonl new-embeddings.jsonl \
  --target-url http://localhost:3001 \
  --target-collection my-embeddings

# 4. Swap at the application layer
```

---

## Crates

```
crates/
├── valori-kernel      Pure deterministic engine — no_std, no I/O, no time
├── valori-wire        Single source of truth for the events.log on-disk format
├── valori-node        HTTP server — standalone and cluster boot, persistence, API
├── valori-consensus   openraft adapter — Raft state machine, log store, gRPC transport
├── valori-verify      Offline event-log auditor — ~400 lines, auditor-readable
├── valori-cli         Inspector, verifier, timeline, replay, diff, cluster CLI
└── valori-ffi         PyO3 bridge for embedded Python (valoricore pip package)
```

### `valori-kernel`

The `no_std` deterministic heart. Contains:
- **Q16.16 arithmetic** (`fxp/`) — all vector math, no `f32`/`f64` in core
- **KernelState** — `RecordPool` (vectors), `NodePool` + `EdgePool` (graph), pluggable index
- **Pluggable indexes** — `BruteForceIndex`, `HnswIndex`, `IvfIndex`
- **Event sourcing** (`event/`) — `KernelEvent` enum; `apply_event` is the only mutation path
- **Snapshot codec** (`snapshot/`) — encode/decode V5 format with BLAKE3 hash-domain byte
- **Proof** (`proof/`) — BLAKE3 Merkle root over integer vectors

No filesystem, no async, no system clock. Suitable for embedded (`thumbv7em-none-eabihf`).

### `valori-wire`

Defines `LogEntry`, `EntryV2`, `EntryV3`, and the v2/v3 segment headers. All three consumer crates (`valori-node`, `valori-verify`, `valori-cli`) import from here. Format drift impossible. Committed test fixtures in `tests/fixtures/*.bin` guard backward compatibility forever — breaking a fixture means the refactor is wrong.

### `valori-node`

The production binary. Layers:
- **Engine** — wraps `KernelState`, snapshot save/load, WAL (legacy), event log recovery
- **Server** — axum HTTP router, auth middleware, Prometheus metrics
- **Events** — `EventCommitter` (standalone write path), `EventLogAuditSink` (cluster write path), `EventJournal`, rotation, multi-segment recovery
- **Cluster** — `bootstrap_cluster`, `ClusterHandle`, `ClusterConfig`, setup wizard, state-hash watcher
- **Commit** — `RaftCommitter` over the openraft handle; `EventLogAuditSink` plugged into `AuditSink`
- **Persistence** — `SnapshotManager` (save/load with CRC32 + BLAKE3)
- **Recovery** — `recover_from_events` → replay all segments; `validate_snapshot`

### `valori-consensus`

openraft 0.9 integration:
- **`ValoriStateMachine`** — adapts `KernelState` to `RaftStateMachine`: dedup → kernel apply → audit sink. The `AuditSink` trait is the single audit-log write seam.
- **`ValoriLogStore`** — in-memory `BTreeMap`-backed Raft log (in-process tests). `RedbLogStore` — persistent redb backend (`VALORI_RAFT_LOG_PATH`).
- **`network`** — tonic/gRPC transport: `AppendEntries`, `Vote`, `InstallSnapshot`. Protobuf is framing; openraft's types are the schema. mTLS support.
- **`partition_harness`** — switchable in-process transport for fault-injection tests

Passes the **official openraft storage compliance suite**.

### `valori-verify`

~400 lines. Two layers of defense:
1. **Hash chain** — catches in-place edits without any external information
2. **State hash** (`--expected-hash`) — catches even a sophisticated attacker who rewrote the log and recomputed the entire chain

Depends only on `valori-kernel` + `valori-wire` + serde + blake3. Deliberately tiny — an auditor reads the source in one sitting.

### `valori-ffi`

PyO3 extension module. Calls `valori-kernel` directly from Python — zero HTTP, zero serialization, microsecond-range inserts. Built with maturin (`pip install ./python`).

---

## Backup and Recovery

### Taking a snapshot

```bash
# Via HTTP
curl -X POST http://localhost:3000/v1/snapshot/save \
  -H 'Content-Type: application/json' \
  -d '{"path": "/backups/snapshot-2026-06-17.bin"}'

# Via CLI (from main.rs auto-snapshot or manual)
VALORI_SNAPSHOT_PATH=/backups/snapshot.bin valori-node
# auto-saves on shutdown
```

### Snapshot format

```
[MAGIC 4B][SCHEMA_VER 4B][META_LEN 4B][META_JSON][KERNEL_BYTES][METADATA_BYTES][INDEX_BYTES][CRC32 4B]
```

The `.prev` rotation keeps the last good snapshot — write failure during save cannot corrupt the current good copy.

### Recovery priority order (standalone node)

```
1. Event log (events.log + all sealed segments)  ← canonical truth
   └─ read_all_segments() → replay in sequence order → verify BLAKE3 splices
2. Snapshot  ← fallback if no event log
3. WAL (legacy)  ← fallback for pre-v2 data
```

### Recovery in a cluster

A restarting node:
1. Reloads `raft.redb` — recovers its vote and any un-applied log entries
2. Reconnects to the cluster — receives missing entries via `AppendEntries` or a fresh `InstallSnapshot` if the log was already compacted
3. Resumes applying events and writing to `events.log`

No manual recovery steps. The cluster is self-healing within one election timeout.

### Offline audit after recovery

```bash
# Pull the hash from the live node
HASH=$(curl -s http://localhost:3000/v1/proof/state \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print(''.join(f'{b:02x}' for b in d['final_state_hash']))")

# Verify from the log file alone — anywhere, no server needed
valori-verify events.log --expected-hash $HASH
# ✅  VERIFIED — N events replayed deterministically; hash chain intact.
```

---

## S3 Object Storage *(Phase 3)*

Sealed segment archives are destined for S3 (or any S3-compatible store — MinIO, GCS, R2). When wired:

```
events.log.000001  (sealed, 256 MiB)
      │
      ▼  archive_pusher uploads
S3 bucket: s3://your-bucket/valori/node-1/events.log.000001
      │
      ▼  local file deleted
Local disk: only events.log (live segment) + snapshot.bin

Recovery: download latest snapshot from S3
          + fetch missing sealed segments
          + replay live segment
```

**Why this matters:** local disk requirement drops from O(total history) to O(live segment size + one snapshot). A node with 5 years of history in S3 restarts in seconds.

Configure with: `VALORI_S3_BUCKET`, `VALORI_S3_PREFIX`, `VALORI_S3_ENDPOINT` (for MinIO/R2).

---

## HTTP API Reference

### Data plane (any node; writes redirect to leader)

| Method | Route | Body / params | Purpose |
|---|---|---|---|
| `POST` | `/records` | `{"values":[…], "collection":"…", "request_id":[…]}` | Insert a vector → `{"id": N}` |
| `POST` | `/v1/vectors/batch_insert` | `{"batch":[[…],…], "collection":"…"}` | Insert many → `{"ids": [...]}` |
| `POST` | `/search` | `{"query":[…], "k":5, "collection":"…", "as_of":"ISO8601", "consistency":"local"}` | k-NN search |
| `POST` | `/v1/delete` | `{"id": N}` | Hard-delete a record |
| `POST` | `/v1/soft-delete` | `{"id": N}` | Tombstone (soft-delete) a record |
| `POST` | `/graph/node` | `{"kind":"…", "record_id":N}` | Create a graph node |
| `POST` | `/graph/edge` | `{"from":N, "to":N, "kind":"…"}` | Create an edge |
| `GET`  | `/v1/proof/state` | — | BLAKE3 state root hex |
| `GET`  | `/v1/timeline` | `?from=ISO8601&to=ISO8601` | Structured event timeline |
| `GET`  | `/health` | — | `{"status":"ok","dim":N}` |
| `GET`  | `/metrics` | — | Prometheus exposition |
| `GET`  | `/version` | — | `{"version":"0.2.1"}` |

### Collections / multi-tenancy (`/v1/namespaces/*`)

| Method | Route | Purpose |
|---|---|---|
| `POST`   | `/v1/namespaces` | Create collection `{"name":"…"}` — idempotent |
| `GET`    | `/v1/namespaces` | List all collections + numeric IDs |
| `DELETE` | `/v1/namespaces/:name` | Drop collection and all its records |

### API Key management (`/v1/keys/*`) — Phase 3.5

Requires an admin-scope credential (`VALORI_AUTH_TOKEN` or an admin API key).

| Method | Route | Scope required | Purpose |
|---|---|---|---|
| `POST`   | `/v1/keys` | admin | Create a key. Body: `{"scope":"read_write","description":"…"}` → token shown once |
| `GET`    | `/v1/keys` | admin | List keys (masked — prefix + metadata, no raw token) |
| `DELETE` | `/v1/keys/:id` | admin | Revoke a key immediately |

Three scope tiers: `read_only` (GET + search only) < `read_write` (insert, delete, collections) < `admin` (keys, snapshots, cluster ops).

### Cluster management plane (`/v1/cluster/*`)

| Method | Route | Purpose |
|---|---|---|
| `GET`  | `/v1/cluster/status` | Leader, term, log indexes, member table |
| `GET`  | `/v1/cluster/health` | `200` leader present, `503` no leader |
| `GET`  | `/v1/cluster/role` | `{"role": "leader"\|"follower", "node_id": N}` |
| `GET`  | `/v1/cluster/read-index` | Leader commit index (for linearizable reads) |
| `POST` | `/v1/cluster/add-node` | Add a member (learner → voter). Leader-only. |
| `POST` | `/v1/cluster/remove-node` | Remove a voter. Leader-only. |

### Snapshot API

| Method | Route | Purpose |
|---|---|---|
| `POST` | `/v1/snapshot/save` | Save state to disk |
| `POST` | `/v1/snapshot/restore` | Restore from disk file |
| `GET`  | `/v1/snapshot/download` | Download snapshot binary |
| `POST` | `/v1/snapshot/upload` | Upload and restore a snapshot |

---

## Configuration Reference

### Standalone node

| Variable | Default | Description |
|---|---|---|
| `VALORI_DIM` | `16` | Embedding dimension — **fixed at startup; must match your model** |
| `VALORI_INDEX` | `bruteforce` | `bruteforce` · `hnsw` · `ivf` |
| `VALORI_BIND` | `127.0.0.1:3000` | HTTP listener |
| `VALORI_EVENT_LOG_PATH` | — | Append-only audit log path. Required for point-in-time reads. |
| `VALORI_EVENT_LOG_ROTATION_BYTES` | `268435456` (256 MiB) | Seal live segment after this many bytes (`0` disables) |
| `VALORI_SNAPSHOT_PATH` | — | Snapshot file path |
| `VALORI_AUTH_TOKEN` | — | Legacy bearer token (admin scope). Use `VALORI_KEYS_PATH` for per-tenant keys. |
| `VALORI_KEYS_PATH` | — | JSON file for persisting API keys across restarts. Absent = in-memory only. |

### Cluster node (additional)

| Variable | Required | Description |
|---|---|---|
| `VALORI_CLUSTER_MEMBERS` | yes | `id=raft_addr/api_addr,...` — presence switches cluster mode on |
| `VALORI_NODE_ID` | yes | This node's numeric id |
| `VALORI_CLUSTER_INIT` | one node | `1` on exactly one node of a **new** cluster. Never on a joiner. |
| `VALORI_RAFT_BIND` | no | gRPC Raft listener. Default `0.0.0.0:3100` |
| `VALORI_RAFT_LOG_PATH` | recommended | redb path for persistent Raft log. Omit for in-memory. |
| `VALORI_TLS_CA` | no | All three TLS vars required together → mTLS on Raft channel |
| `VALORI_TLS_CERT` | no | |
| `VALORI_TLS_KEY` | no | |
| `VALORI_STATE_HASH_CHECK_SECS` | no | Divergence-detection interval (default 30; `0` disables) |

---

## Observability

Prometheus metrics at `/metrics`:

| Metric | Type | Description |
|---|---|---|
| `valoricore_events_committed_total` | Counter | Events persisted to the audit log |
| `valoricore_batch_commit_duration_seconds` | Histogram | Commit latency per batch |
| `valori_replay_duration_seconds` | Histogram | Recovery replay time |
| `valori_raft_current_term` | Gauge | Raft term |
| `valori_raft_last_log_index` | Gauge | Last entry in this node's Raft log |
| `valori_raft_last_applied` | Gauge | Last entry applied to the kernel |
| `valori_raft_snapshot_index` | Gauge | Log index of the last installed snapshot |
| `valori_raft_state_hash_match` | Gauge | `1` = all peers agree on BLAKE3 state hash, `0` = mismatch |
| `valori_raft_divergence_detections_total` | Counter | Count of state-hash mismatches detected |

Recommended alert:

```yaml
- alert: ValoriStateDivergence
  expr: valori_raft_state_hash_match == 0
  for: 1m
  severity: critical
```

---

## Benchmarks

*MacBook Air M2, SIFT1M dataset.*

| Operation | Result |
|---|---|
| Single insert (local FFI) | ~20 µs |
| Batch insert — 1K vectors | ~15 ms |
| L2 search — 10K × 384-dim (BruteForce) | ~8 ms |
| L2 search — 100K × 384-dim (BruteForce) | ~80 ms |
| Snapshot — 10K records | ~45 ms |
| BLAKE3 state hash computation | < 1 µs |

| Recall metric | Result | Target |
|---|---|---|
| Recall@1 | 99.00% | > 90% |
| Recall@10 | 99.00% | > 95% |
| Tag filter accuracy | 100.00% | 100% |
| Search latency (p50) | 0.47 ms | < 1.0 ms |

Fixed-point arithmetic overhead relative to `f32` is negligible. Determinism is free.

```bash
cargo run --release --bin bench_recall
cargo run --release --bin bench_ingest
cargo run --release --bin bench_filter
cargo run --release --bin bench_persistence
```

---

## Comparison

| Capability | Pinecone | Weaviate | Qdrant | **Valori** |
|---|---|---|---|---|
| Crash recovery | Claimed¹ | Claimed² | Claimed³ | **Mathematically proven** |
| Cross-hardware bit-identical results | No⁴ | No⁴ | No⁴ | **Yes — Q16.16 fixed-point** |
| Per-record cryptographic proof | No | No | No | **Yes — BLAKE3 Merkle root** |
| Offline proof verification | No | No | No | **Yes — no server required** |
| Tamper localization | No | No | No | **Yes — exact event + byte offset** |
| Full forensic event replay | No | No | No | **Yes — audit log is the canonical truth** |
| Knowledge graph (same store) | No | Yes | No | **Yes** |
| Linearizable cluster reads | No⁵ | No⁶ | Yes | **Yes — read-index protocol** |
| Embedded `no_std` deployment | No | No | No | **Yes — ARM Cortex-M4** |
| Open source | No | Yes | Yes | **Yes — MIT OR Apache-2.0** |

**Sources for "No" cells:**

¹ Pinecone does not publish a crash-recovery proof or WAL specification. Their
  [architecture docs](https://docs.pinecone.io/guides/get-started/overview)
  describe eventual consistency without mathematical guarantees.

² Weaviate uses RocksDB for persistence; crash recovery is WAL-based but the
  exact recovery guarantee is not formally specified in their
  [storage documentation](https://weaviate.io/developers/weaviate/concepts/storage).

³ Qdrant's [on-disk storage](https://qdrant.tech/documentation/concepts/storage/)
  is WAL-backed, but replay correctness depends on float32 arithmetic, which is
  non-deterministic across platforms (see ⁴).

⁴ All three use float32 (IEEE 754 single precision). Float32 addition is
  non-associative: result depends on summation order, which varies by SIMD
  width (SSE2 vs AVX-512 vs NEON). The same vectors produce different distances
  on different hardware. See `benchmarks/RESULTS.md §3` for a measured example
  showing 100% of 276 scores differing at the bit level across four summation
  orders, with ranking flips on near-equal pairs.

⁵ Pinecone's read consistency is documented as
  [eventually consistent](https://docs.pinecone.io/guides/data/query-data) —
  freshly written vectors may not appear in queries immediately.

⁶ Weaviate's [replication](https://weaviate.io/developers/weaviate/concepts/replication-architecture)
  is leaderless (quorum reads) — not linearizable by the Herlihy definition.

---

## Building from Source

```bash
git clone https://github.com/varshith-Git/Valoricore-Kernel.git
cd Valoricore-Kernel

# Build all default crates
cargo build --release --workspace

# Run all tests
cargo test --workspace

# Targeted test suites
cargo test -p valori-node      --test proof_e2e_tests
cargo test -p valori-node      --test crash_durability   # kill-test
cargo test -p valori-node      --test graph_cascade
cargo test -p valori-consensus                           # openraft compliance + cluster tests
cargo test -p valori-consensus --test proptest_event_fuzz

# Offline verifier + tamper demo
cargo build -p valori-verify --release
./crates/valori-verify/tamper_demo.sh              # generates 2k events, flips bytes, catches both attacks
./crates/valori-verify/tamper_demo.sh 50000        # larger log

# Python FFI
cd python && pip install -e ".[dev]"
python test_valoricore_integrated.py
```

### Toolchain

```
rustup toolchain install stable        # Rust stable
rustup target add thumbv7em-none-eabihf  # for embedded only
cargo install maturin                  # for Python FFI only
```

---

## Who Should Use Valori

**Valori is the right choice when:**

- You build AI for **healthcare, finance, legal, or defence** and need a verifiable, reproducible audit trail that stands up in court or under regulatory review.
- You operate on **multiple hardware architectures** (x86 EC2, ARM Graviton, edge ARM) and cannot tolerate silent float divergence between replicas.
- You need to **forensically replay** the exact state of your AI system at any point in history.
- You want **offline proof verification** — an auditor should not need access to your production cluster to verify your data has not been tampered with.
- You need a vector database that runs on **resource-constrained hardware** (IoT, embedded, microcontrollers) without a heap allocator.
- You care that your AI agent's **memory cannot silently drift** between calls, machines, or restarts.

**Consider alternatives when:**

- Your primary constraint is raw throughput at billion-vector scale — managed services like Pinecone are optimised for that use case.
- You have no audit, reproducibility, or compliance requirements — you're paying for guarantees you don't need.

---

## Documentation

| | |
|---|---|
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | Security model — what Valori protects against, keyed BLAKE3 MAC analysis |
| [docs/CAPACITY.md](docs/CAPACITY.md) | Capacity planning — vectors/GB, RAM per 1M vectors, WAL growth, S3 costs |
| [docs/DR.md](docs/DR.md) | DR runbook — snapshot-to-S3, full cluster restore, cross-region active-passive |
| [docs/CLUSTER.md](docs/CLUSTER.md) | Cluster operations — setup wizard, endpoints, grow/recover |
| [docs/README.md](docs/README.md) | Full documentation index |

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

Valori is dual-licensed under **MIT OR Apache-2.0** — completely free and permissive for commercial embedding. The core engine is free forever.

Managed cloud, multi-tenant control plane, and enterprise features (SSO, RBAC, per-tenant encryption) will be available as a separate commercial offering.

**Contact:** varshith.gudur17@gmail.com

---

<div align="center">

*Built in Rust. Proven in production. Auditable by mathematics.*

If Valori is useful to you, a star helps others find the project.

[![Star History](https://api.star-history.com/svg?repos=varshith-Git/Valoricore-Kernel&type=Date)](https://star-history.com/#varshith-Git/Valoricore-Kernel&Date)

</div>
