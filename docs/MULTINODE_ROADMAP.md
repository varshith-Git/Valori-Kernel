# Valori Multi-Node Roadmap

Single source of truth for the single-node → multi-node → enterprise evolution.
Decisions recorded here were made on the `multinode` planning track (2026-06).

**Standing rules (apply to every phase):**

- The kernel crate's diff stays ~zero. All distributed-systems risk lives in
  `valori-consensus`, feature-flagged, leaving standalone mode untouched.
- Single-node mode is never removed. One binary, mode chosen at startup.
- Merge per phase to `main` behind flags — no long-lived divergent branch.
- Every wire-format decision lands in Phase 1 (one header bump, not five).
- Hashes are computed over entry bytes, never file bytes — compression and
  storage layout can never affect verification.

---

## Phase 0 — Baseline ✅ DONE

- fsync-per-append durability + `crash_durability.rs` kill-test
- v2 hash-chained event log (`ChainedEntry`, per-entry BLAKE3 chain)
- `valori-verify` v2: chain validation, tamper localization, forensic JSON
- Capacity enforcement (HTTP 507) at all insert entry points
- Reverse edge index test coverage (`graph_cascade.rs`)
- Kill-9-a-real-server end-to-end validation

---

## Phase 1 — Foundations & Seams ✅ DONE

Goal: make every decision that *calcifies once production logs exist*
(wire format, hash domain, trust boundaries), restructure the repo, and
introduce the abstraction seams Raft will plug into.

### 1.1 Workspace restructure ✅
### 1.2 `valori-wire` crate — single source of wire truth ✅
### 1.3 `FxpFormat` seam — configurable precision ✅
### 1.4 Collections seam ✅
### 1.5 GDPR / crypto-shredding — design + schema reservation ✅
### 1.6 Security design doc ✅
### 1.7 Verifier hardening ✅
### 1.8 Storage policy ✅
### 1.9 `Committer` trait seam ✅
### 1.10 CI upgrades ✅
### 1.11 Docker + compose ✅

---

## Phase 2 — Cluster Mode via openraft ✅ DONE

Goal: `valori-node --mode cluster` — N-node HA, quorum-durable writes,
provably identical replicas.

### 2.1 `valori-consensus` crate — openraft type config, log store, state machine ✅
### 2.2 gRPC transport (tonic) + mTLS ✅
### 2.3 Modes & bootstrap (standalone → cluster upgrade path) ✅
### 2.4 Request dedup (replicated dedup table in state machine) ✅
### 2.5 Read path (linearizable via read-index; RwLock off hot path) ✅
### 2.6 Cluster management API (`/v1/cluster/*`) ✅
### 2.7 Snapshot transfer (late-joiner catch-up via `InstallSnapshot`) ✅
### 2.8 Fault-tolerance tests (leader crash, minority/majority kill) ✅
### 2.9 Admin audit events in chain ✅
### 2.10a Persistent Raft log (redb) ✅
### 2.10b mTLS (rustls + cluster CA) ✅
### 2.10c Metrics (Prometheus) ✅
### 2.10d Partition harness (asymmetric partition, BLAKE3 frozen-then-converges) ✅
### 2.11 Boot dispatch + cluster data plane v1 ✅

**Phase 2 exit:** 3-node cluster survives any single-node kill with zero
acknowledged-write loss; partition suite green; merged to main.

---

## Phase 3 — GA & Product (in progress)

Goal: turn the working cluster into a shippable product. Every sub-phase
is independently mergeable. Exit = SOC 2 evidence trail started, first
paying customer onboarded.

### 3.1 S3 object store (snapshot offload + WAL archival) ✅ DONE

S3/GCS/Azure Blob backend for sealed WAL segments and snapshots.
Object Lock (WORM) flag on sealed segments. `valori-verify` runs against
bucket contents. Cross-region replication as the DR story.

### 3.2 Rolling upgrades (zero-downtime version migration) ✅ DONE

Schema version gate in `ValoriStateMachine`. `vN` binary refuses entries
from `vN+1`; accepts `vN-1` with backward-compat decoders. Node-by-node
replacement procedure documented. Mixed-version window tested end-to-end.

### 3.3 Cluster-aware Python SDK

Leader discovery (poll `/v1/cluster/role`, cache result), automatic
retry-with-redirect on 307 `ForwardToLeader`, idempotency tokens on
every mutating call by default, configurable consistency tier per call.

**Deliverables:**
- `SyncRemoteClient` + `AsyncRemoteClient` — `leader_url()`, `with_leader()`,
  `insert(..., idempotency_key=uuid4())`, `search(..., consistency="local"|"linearizable")`
- `ClusterClient` wrapper: round-robins reads across replicas for local reads,
  routes writes to the leader
- Retry logic: exponential backoff (3 attempts), `RetryableError` vs `FatalError`
  distinction; 307 redirects followed exactly once
- Integration test: kill the leader mid-write, SDK retries, exactly-once delivery
  confirmed via `deduplicated` flag in response

### 3.4 As-of / point-in-time reads ← NEXT

Event sourcing makes this almost free: every committed event has a log index
and a wall-clock timestamp. "What did the AI know on March 3?" becomes a
range scan up to the last event at or before that timestamp.

**Deliverables:**
- `GET /v1/search?as_of=<ISO8601>` and `GET /v1/search?as_of_log_index=<u64>` —
  replay the kernel up to that point and search the resulting snapshot
- Efficient path: binary-search the WAL for the target index/timestamp, restore
  the nearest preceding snapshot, replay the tail, run the search, discard
- `GET /v1/timeline?collection=<name>&from=<ISO8601>&to=<ISO8601>` — returns
  the sequence of record inserts/deletes in that window with timestamps
- Proof receipt extension: `as_of_log_index`, `as_of_timestamp`, and the
  BLAKE3 hash of the state at that point — verifiable by any auditor
- Python SDK: `c.search(..., as_of="2026-03-03T00:00:00Z")`
- Phase doc + tests with WAL fixture that spans multiple timestamps

### 3.5 Per-tenant API keys + RBAC

Scoped credentials: read-only, read-write, admin. Keys are stored hashed
(Argon2id) in the replicated state machine so every node enforces the same
ACL. Key rotation is an audit event in the chain.

**Deliverables:**
- `POST /v1/keys` — create key with scope + collection filter
- `DELETE /v1/keys/{id}` — revoke (audit event written)
- `GET /v1/keys` — list keys (masked)
- Middleware in both `server.rs` and `cluster_server.rs` — bearer token lookup,
  scope check, 401/403 on mismatch
- Collection-scoped keys: a key can be locked to one collection (tenant isolation)
- `VALORI_AUTH_TOKEN` becomes legacy; new keys take precedence
- Admin key required to manage other keys

### 3.6 Crypto-shredding (GDPR erasure)

Implementation of the design from Phase 1.5. Per-record AES-256-GCM envelope.
Erase = destroy the DEK. The BLAKE3 chain stays intact — shredded records
appear as `"present, unrecoverable"` in the verifier output.

**Deliverables:**
- `EncryptionEnvelope` in `valori-kernel` — wraps vector + metadata with per-record DEK
- `KeyVault` trait — two impls: `InMemoryKeyVault` (test/standalone) and
  `ExternalKeyVault` (HTTP endpoint, compatible with AWS KMS / Hashicorp Vault)
- `DELETE /v1/records/{id}?shred=true` — destroys DEK, marks record `SHREDDED`
  in the log (audit event), vector becomes zero-filled unrecoverable ciphertext
- `valori-verify` output: `status: "shredded"` for erased records, chain intact
- GDPR erasure receipt: signed JSON with record ID, shred timestamp, log index

### 3.7 `valori-import` — provable migrations

Import from external vector stores. Every imported record is a normal
`KernelEvent` — provable from the moment of migration.

**Deliverables:**
- `valori import qdrant --url <> --collection <> --target-collection <>`
  (scroll API, page size 1000, Q16.16 quantization with honest precision doc)
- `valori import jsonl <file>` — `[{"vector": [...], "metadata": {...}}]` lines
- `valori import parquet <file>` — Arrow schema auto-detected
- Genesis-link: import job emits a `GenesisImport` event recording the source
  URL + content hash before the first record, so the provenance chain starts
  from migration day zero
- Progress: streaming `--progress` output; resumable (last imported record ID
  stored in a sidecar file, restart skips already-committed records via dedup)

### 3.8 Write-throughput regression gates in CI

Automated benchmark that fails the PR if p99 insert latency regresses > 15%
or throughput drops > 10% vs the baseline on `main`.

**Deliverables:**
- `benchmarks/write_regression.py` — inserts 10 000 records, measures p50/p99/throughput
- GitHub Actions job: runs on every PR that touches `crates/`, compares to stored
  baseline JSON in `benchmarks/baseline/`
- Baseline update workflow: `make benchmark-baseline` stamps new numbers after
  a deliberate perf improvement
- Alert comment on PR if regression detected (does not block merge, just warns)

### 3.9 Terraform modules (AWS + Azure)

BYOC deployment into customer VPCs — the compliance-friendly middle step
before hosted SaaS.

**Deliverables:**
- `terraform/aws/` — EKS cluster, EBS PVCs, ALB, IAM roles, S3 bucket with
  Object Lock, CloudWatch alarms for `state_hash_match` and replication lag
- `terraform/azure/` — AKS, managed disks, Azure Blob Storage, Key Vault
  integration for CMK (Phase 5)
- `docs/DEPLOY_AWS.md` and `docs/DEPLOY_AZURE.md` — operator runbooks
- Terratest smoke test: `terraform apply` → health check → `terraform destroy`

### 3.10 Signed releases + SBOM

**Deliverables:**
- `cosign` signatures on GHCR images and GitHub release binaries
- SPDX SBOM generated and attached to every release via `cargo-sbom`
- `cargo-deny` advisories check in CI (already have license scan; add vuln scan)
- SOC 2 Type II process started: evidence collection automation (log exports,
  access reviews, change management artifacts)

**Phase 3 exit:** first paying customer running in production; GDPR erasure
tested end-to-end; SOC 2 evidence trail ≥ 30 days.

---

## Phase 4 — Scale & Kubernetes

Goal: 10× the data (100M+ vectors), 10× the teams (per-collection Raft groups),
and fully automated operations via a Kubernetes operator.

### 4.1 Kubernetes operator

Automates everything in the Helm chart rung. Watches a `ValoriCluster` CRD,
drives `add-node` / `remove-node` as Kubernetes scales the StatefulSet up or
down. This is the foundation of the managed cloud data plane.

**Deliverables:**
- `ValoriCluster` CRD: replicas, storage class, resource requests, S3 backup config
- Controller: reconciles desired vs actual member list via `/v1/cluster/*` APIs
- Leader LB: updates a `Service` selector to always point to the current leader
- PVC lifecycle: provisions, retains on scale-down (data never auto-deleted),
  re-attaches on scale-up
- `operator/` directory, published to OperatorHub

### 4.2 Shard-by-collection (per-collection Raft group)

One Raft consensus group per collection. A stateless router maps requests to
the correct group. Cluster-wide proof = Merkle root over per-collection hashes.

**Design constraints:**
- NEVER shard intra-collection (cross-shard edges destroy O(degree) graph
  cascades and fragment the proof story)
- Each collection is a first-class Raft cluster: independent leader, log, and
  state hash
- Router is stateless: reads the group's membership from a shared etcd-style
  catalog (one tiny Raft group for the catalog itself)

**Deliverables:**
- `CollectionRouter` — maps `collection_id` → `RaftGroupHandle`
- Catalog Raft group: stores collection → member mapping, survives any node loss
- Cross-collection search: fan-out → merge → re-rank (no cross-shard consistency
  guarantee; documented)
- Proof API: `GET /v1/proof/cluster` — Merkle root over all active collection hashes

### 4.3 Disk-mode HNSW (demand-driven)

In-RAM HNSW (`HnswIndex`) is the right default — 1M × 384-dim fits in ~6 GB.
Disk mode is activated only when a paying customer arrives with a dataset that
does not fit in RAM.

**When to build:** first customer with > 10M vectors in a single collection.

**Design:**
- `usearch` FFI (Apache-2.0) or a custom mmap-backed adjacency graph on redb
- `VectorIndex` trait already exists — disk mode is a new impl, not a refactor
- Index file is separate from the WAL/snapshot; rebuild from snapshot on restart
  (no new serialization format risk to the audit chain)
- `VALORI_INDEX=hnsw-disk` env var activates it; default stays `hnsw` (in-RAM)

**Deliverables:**
- `DiskHnswIndex` implementing `VectorIndex`; all existing HNSW tests pass
- Benchmark: 50M-vector insertion + 1k QPS search, p99 < 50 ms on NVMe
- Memory ceiling test: confirm RSS stays < 2 GB during index traversal

### 4.4 Q8.8 / Q32.32 format activation

The `FxpFormat` seam (Phase 1.3) already exists. Activate on real demand:
- **Q8.8** — embedded / edge devices where RAM is < 256 MB
- **Q32.32** — finance / scientific workloads requiring > Q16.16 precision

**Deliverables:**
- `valori migrate-format --from q16.16 --to q8.8` — rewrites records with a
  provable genesis-link: new log's first event records old log's final state hash
- Format-mismatch rejection in cluster handshake (already gated by format ID)
- Precision loss doc: quantization error bounds vs. original float embeddings

### 4.5 Kafka / Pulsar edge connectors

Ingest buffer and CDC-out at the edge. Never the consensus core — external brokers
stay outside the trust boundary.

**Deliverables:**
- `valori-kafka-source` connector: consumes a Kafka topic, batches into `batch_insert`,
  commits offset only after Raft ack — exactly-once delivery into the audit chain
- `valori-kafka-sink` connector: streams committed `KernelEvent`s to a Kafka topic
  for downstream analytics (CDC out)
- Pulsar equivalents (same interface, different transport)
- `VALORI_KAFKA_BOOTSTRAP_SERVERS`, `VALORI_KAFKA_TOPIC` env vars

### 4.6 Managed cloud control plane (proprietary)

The open-core revenue layer. Data plane = same OSS `valori-node` clusters.
Control plane = proprietary provisioning, billing, and tenant management.

**Deliverables (internal, not open-sourced):**
- Tenant provisioning API: create/delete clusters, resize, set backup policy
- Usage metering: vectors stored, queries/second, egress
- Billing integration: Stripe, per-tenant invoicing
- SSO federation: customer IdP → control plane → per-cluster RBAC keys

### 4.7 Reproducible builds

- Deterministic binary build (pinned toolchain, pinned crate hashes, no build-time
  randomness)
- `cargo-auditable` embeds the full dependency tree in the binary for post-hoc SBOM
- Verify: two independent builds from the same source produce byte-identical binaries

**Phase 4 exit:** operator-managed cluster on EKS with 10M vectors in a single
collection; shard-by-collection tested with 3 collections × 3 Raft groups;
Kafka connector delivering exactly-once writes at 50k msg/s.

---

## Phase 5 — Enterprise & Compliance

Goal: SOC 2 Type II certified, FedRAMP-ready design, enterprise SSO, and
multi-region HA. This is the phase that unlocks regulated-industry customers
(fintech, healthcare, legal).

### 5.1 Verifiable AI: Proof-Carrying Answers + Compliance Pack ✅ DONE

Full proof receipt per answer: citation chain, BLAKE3 state hash at inference
time, model + version stamp, contradiction status. `GET /v1/proof` exports
the full compliance bundle. Already shipped.

### 5.2 SOC 2 Type II automation

Continuous evidence export for the five Trust Services Criteria.

**Deliverables:**
- Evidence pipeline: daily export of access logs, change events, availability
  metrics to a locked S3 bucket (Object Lock, 7-year retention)
- `POST /v1/audit/export?from=<date>&to=<date>` — signed NDJSON export of all
  `KernelEvent`s in a window with BLAKE3 chain proof
- Automated access review: quarterly report of all active API keys, last-used
  timestamps, scope; delivered to configured email
- `docs/SOC2_CONTROLS.md` — control-by-control mapping to Valori features

### 5.3 SAML / OIDC SSO + fine-grained RBAC

Enterprise identity integration. Row-level security: a key can be scoped to a
subset of collections AND a subset of record tags.

**Deliverables:**
- SAML 2.0 SP and OIDC RP in the control plane (Phase 4.6) — customer IdP issues
  short-lived session tokens that map to Valori API key scopes
- Row-level security: `tag` field on `KernelEvent::InsertRecord` (already in the
  schema) gates retrieval — keys with `tags: ["pii"]` cannot read records not
  carrying that tag
- `POST /v1/roles` — named role bundles (auditor = read-only + proof endpoints;
  ingestor = write-only; admin = all)
- Role assignment audit event in the BLAKE3 chain

### 5.4 Customer-managed encryption keys (CMK / BYOK)

Replaces the `InMemoryKeyVault` from Phase 3.6 with real KMS integration.
Customer holds the master key; Valori never sees it.

**Deliverables:**
- `AwsKmsKeyVault` — wraps DEKs with a customer CMK via AWS KMS `Encrypt` / `Decrypt`
- `AzureKeyVaultKeyVault` — same via Azure Key Vault
- Key rotation ceremony: new CMK wraps all DEKs, old CMK retired, rotation event
  in the audit chain; old CMK can be destroyed without data loss
- HSM support: `PKCS#11` interface for on-premise HSMs (Thales, SafeNet)

### 5.5 SIEM integration — audit log streaming

Pipe the BLAKE3-chained audit log to customer SIEM tools in real time.

**Deliverables:**
- `valori-siem` sidecar: tails `events.log`, verifies chain on the fly, forwards
  to configurable backends: Splunk HEC, Datadog Logs, AWS Security Hub, Elastic
- `VALORI_SIEM_BACKEND`, `VALORI_SIEM_URL`, `VALORI_SIEM_TOKEN` env vars
- Backpressure: if SIEM is unavailable, sidecar buffers up to 1 GB on disk and
  replays; never blocks the write path
- Each forwarded event carries the BLAKE3 chain hash so the SIEM can verify
  continuity independently

### 5.6 Multi-region active-active reads with geo-routing

Write-anywhere is hard and not yet needed. Read-anywhere is free from the
Raft follower model — followers serve `local` consistency reads today. This
phase adds geo-aware routing.

**Deliverables:**
- Region-tagged nodes: `VALORI_REGION=us-east-1` label propagated in membership
- `GET /v1/search?consistency=local` routed to the nearest region's follower
  by the load balancer (Envoy or Cloudflare Workers rule)
- Cross-region replication lag metric: `valori_replication_lag_seconds{region=}`
- Disaster recovery runbook: promote a follower in a secondary region to leader
  without data loss (RPO = 0 for committed writes)

### 5.7 FedRAMP-ready deployment topology

**Deliverables:**
- GovCloud deployment guide (AWS us-gov-west-1 / us-gov-east-1)
- FIPS 140-2 mode: swap BLAKE3 for SHA-256 in a compile-time feature flag
  (`--features fips`) — all existing tests run under both flags in CI
- Air-gapped install: `valori-node` + all images bundled in a tar archive
  deployable with no internet access
- `docs/FEDRAMP.md` — control mapping, boundary diagram, data flow diagram

**Phase 5 exit:** SOC 2 Type II report issued; at least one regulated-industry
customer in production; CMK rotation tested end-to-end.

---

## Phase 6 — AI-Native at Scale

Goal: Valori becomes the native memory layer for autonomous AI agents —
not just a vector store, but a verifiable, self-maintaining, multi-modal
long-term memory with a compliance receipt on every inference.

### 6.1 GPU-accelerated search

Activates when `dim > 512` or query rate > 10k QPS on a node with a GPU.

**Deliverables:**
- `cuVS` (NVIDIA, Apache-2.0) backend behind `VectorIndex` trait — exact same
  API, GPU detected at startup
- Mixed fleet: GPU nodes handle high-dim search, CPU nodes handle graph ops
  and Raft consensus (no mixing of trust domains)
- Benchmark: 1M × 1536-dim (OpenAI ada-002 size), p99 < 5 ms at 10k QPS
- BLAKE3 hashing stays on CPU — the proof chain is never on the GPU

### 6.2 Multi-modal embeddings

Image, audio, and structured data as first-class vector types alongside text.

**Deliverables:**
- `EmbeddingKind` field on `InsertRecord`: `Text | Image | Audio | Structured`
- Per-kind distance metric: cosine (text), L2 (image/audio), dot (structured)
- Ingest route: auto-detect MIME type, call the appropriate embedding model
  (CLIP for image, Whisper for audio)
- Mixed-modal search: embed the query in one modality, retrieve across all
  (late fusion with configurable weights per kind)

### 6.3 Native RAG connectors

First-class SDK integrations for the leading RAG frameworks.

**Deliverables:**
- **LangChain** (`langchain-valori`): `ValoriVectorStore` implementing
  `VectorStore` interface; `ValoriRetriever` with as-of support
- **LlamaIndex** (`llama-index-vector-stores-valori`): `ValoriVectorStore`
  node; `ValoriReader` for document ingest with contextual enrichment
- **Haystack** (`haystack-valori`): `ValoriDocumentStore` and `ValoriRetriever`
- Each connector exposes the proof receipt so the RAG pipeline can attach it
  to the final LLM response

### 6.4 MemoryOS — self-maintaining agent memory as a service

Productization of the Cortex stack (C0–C3). An agent's entire working memory,
long-term knowledge, and contradiction history as a managed API.

**Deliverables:**
- `POST /v1/memory/ingest` — document → chunk → context → embed → graph extract
  → contradiction check, all in one call with one proof receipt
- `GET /v1/memory/ask?q=<natural language>` — retrieval + reranking + citation
  chain + as-of capability, returned as a proof-carrying answer
- `GET /v1/memory/contradictions` — list, dismiss, supersede
- `GET /v1/memory/timeline?entity=<id>` — full belief history for a named entity
- Hosted plan: per-agent memory namespace, metered by vectors stored + queries

### 6.5 Streaming ingest (WebSocket + SSE)

Real-time append path for agents that produce continuous streams of observations.

**Deliverables:**
- `WS /v1/stream/ingest` — client pushes chunks; server returns per-chunk receipts
  as they commit through Raft; backpressure via flow control
- `GET /v1/stream/events` (SSE) — server-sent stream of committed `KernelEvent`s
  filtered by collection; enables real-time UI updates and CDC consumers
- Reconnect protocol: client sends last seen log index; server resumes from there

### 6.6 Real-time contradiction & drift detection at scale

Cortex C3 contradiction detection runs synchronously today (blocks ingest).
At scale it needs an async worker pool with bounded latency.

**Deliverables:**
- `ContradictionWorker` pool: configurable concurrency, priority queue by
  similarity score, dead-letter queue for failed checks
- `VALORI_CONTRADICTION_WORKERS` (default 4), `VALORI_CONTRADICTION_THRESHOLD`
  (default 0.92)
- Drift detection: weekly background job computes centroid drift per collection;
  flags collections where the distribution has shifted > σ from baseline
- Alert: `POST /v1/webhooks/contradictions` — fires on new pending contradictions

### 6.7 Open-core SaaS GA

Managed data plane + billing + self-serve onboarding.

**Deliverables:**
- Public cloud SaaS: `app.valori.dev` — create cluster, get a URL, start ingesting
- Free tier: 100k vectors, 1M queries/month, no GDPR erasure, no SLA
- Pro tier: 10M vectors, unlimited queries, GDPR erasure, 99.9% SLA
- Enterprise tier: BYOC (Phase 4 Terraform), CMK (Phase 5.4), SSO, SLA 99.99%
- Self-serve onboarding: OAuth sign-in → cluster provisioned < 60 s → SDK snippet

**Phase 6 exit:** 100+ agents using MemoryOS; multi-modal ingest in production;
LangChain connector downloaded > 10k/month; SaaS generating ARR.

---

## Appendix: Key design decisions (permanent record)

| Decision | Rationale |
|---|---|
| openraft over Kafka/Pulsar as consensus core | External broker = source of truth outside trust boundary; openraft is in-process, in Rust, auditable |
| Q16.16 fixed-point only in vector hot path | Bit-identical across x86/ARM; deterministic BLAKE3 hashes; no float non-determinism |
| Audit log written at APPLY time, never at commit | Raft truncates uncommitted tails; audit log must never truncate; ordering enforced by the state machine |
| No intra-collection sharding (Phase 4.2) | Cross-shard edges destroy O(degree) graph cascades and fragment the proof |
| Disk-mode HNSW deferred to Phase 4.3 | 1M × 384-dim fits in ~6 GB RAM; build it when a paying customer needs it, not on spec |
| FIPS mode = SHA-256 feature flag (Phase 5.7) | BLAKE3 is faster but not FIPS-certified; swap at compile time, same test suite |
| MemoryOS builds on Cortex C0–C3 (Phase 6.4) | Cortex is already shipped; Phase 6 is productization, not greenfield |
