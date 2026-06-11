# Valori Multi-Node Roadmap

Single source of truth for the single-node → multi-node evolution.
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

## Phase 0 — Baseline (DONE)

- fsync-per-append durability + `crash_durability.rs` kill-test
- v2 hash-chained event log (`ChainedEntry`, per-entry BLAKE3 chain)
- `valori-verify` v2: chain validation, tamper localization, forensic JSON
- Capacity enforcement (HTTP 507) at all insert entry points
- Reverse edge index test coverage (`graph_cascade.rs`)
- Kill-9-a-real-server end-to-end validation

---

## Phase 1 — Foundations & Seams (~2–3 weeks, behavior-neutral)

Goal: make every decision that *calcifies once production logs exist*
(wire format, hash domain, trust boundaries), restructure the repo, and
introduce the abstraction seams Raft will plug into. Main stays releasable
at every commit.

### 1.1 Workspace restructure
- Move root `src/` → `crates/valori-kernel/` (no_std, untouched logic)
- New layout: `crates/{valori-kernel, valori-wire, valori-node,
  valori-consensus(empty), valori-verify, valori-cli, valori-ffi}`
- Delete dead code: `src/tests/graph_tests.rs` (old const-generic API,
  never compiled into the build)
- Repo hygiene: gitignore `graphify-out/`, remove `my_report*.json`,
  decide `dev` branch policy
- **Accept:** full workspace builds; all 21+ test binaries green; CI paths fixed.

### 1.2 `valori-wire` crate — single source of wire truth
- Move `LogEntry`, `ChainedEntry`, header codec, `chain_advance` here.
  Deps: serde + bincode + blake3 ONLY (auditor-readable).
- `valori-node` and `valori-verify` both consume it; delete the duplicate
  in `verify/src/wire.rs` (it already drifted once, v1→v2).
- **Header v3 — one bump, all fields:**
  - `format_id: u8` — arithmetic format (Q16.16 = 1)
  - `segment_seq: u32` + `prev_segment_chain_head: [u8;32]` — fixes the
    cross-segment gap: `rotate()` currently resets the chain to zeros, so
    nothing binds segment N to N−1; a whole segment could be deleted or
    substituted undetected. New segments must splice to the predecessor's
    final chain head.
  - `request_id` envelope field on events — idempotency/dedup schema now,
    dedup implementation in Phase 2.
- **Event schema evolution policy (doc + CI):** enum variants append-only,
  never reorder; version-gated; CI test replays committed v2-era log
  fixtures forever.
- **Accept:** node writes v3, verifier reads v3 (and refuses v1/v2 loudly
  with guidance); fixture-replay CI job exists.

### 1.3 `FxpFormat` seam — configurable precision
- Trait: `{ type Repr; type Wide; const FRAC_BITS; const FORMAT_ID }`.
  `Wide` is the accumulator type (dot products: Q16.16 needs i64,
  Q32.32 will need i128).
- Kernel generic over format; **instantiate Q16.16 only** — Q8.8/Q32.32
  are ~50 lines later, activated on real customer demand, not on spec.
- Format ID mixed into the state-hash domain (a Q8.8 state can never
  masquerade as Q16.16) + header + snapshot header.
- FFI/Python take `format=` at DB creation (only `q16.16` accepted for now).
- **Accept:** state hashes unchanged for existing Q16.16 data ONLY if the
  domain-separation change is versioned; otherwise document the one-time
  hash change here and in the README.

### 1.4 Collections seam
- `collection` concept in the API surface (single default collection).
  No multi-collection engine yet — API shape only, so multi-tenancy and
  shard-by-collection (Phase 4) are not API breaks.

### 1.5 GDPR / crypto-shredding — design + schema reservation
- Per-record payload encryption envelope; key vault interface trait;
  erase = key destruction. Log/chain/replay stay intact; shredded records
  verify as "present, unrecoverable."
- Phase 1 delivers: design doc + reserved schema fields. Implementation
  in Phase 3. (Retrofit after production logs exist = format migration —
  that is why the schema lands now.)

### 1.6 Security design doc
- Threat model; inter-node mTLS (Phase 2); per-tenant API keys + RBAC
  (Phase 3); encryption at rest; admin-action audit events (cluster
  membership changes, key rotations recorded in the log itself).

### 1.7 Verifier hardening (it parses attacker-controlled input)
- bincode `with_limit` decode caps; dim/entry-size sanity bounds
- `cargo-fuzz` target + short CI fuzz smoke run
- Multi-segment verification: follow `prev_segment_chain_head` splices
  across archived segments; transparent zstd read support
- **Accept:** fuzzer runs clean for N minutes in CI; crafted oversized-
  allocation log files are rejected, not OOM.

### 1.8 Storage policy
- `VALORI_SNAPSHOT_EVERY` cadence knob (bytes and/or events). Recovery =
  latest snapshot (hash-verified against its checkpoint) + tail replay.
  Fallback chain: previous snapshot → genesis replay (audit path only).
- zstd-compress sealed segments + snapshots (active tail never compressed).
- Defined disk-full behavior: degraded read-only mode, clear error,
  heartbeats keep flowing; tested.
- **Accept:** recovery-time test proves bounded-by-cadence restart, not
  bounded-by-history.

### 1.9 `Committer` trait seam
- `Engine` owns `Box<dyn Committer>`; `StandaloneCommitter` wraps the
  existing shadow-exec → fsync → apply path verbatim.
- Capacity checks (507s) move inside shadow execution against state, so
  they are deterministic and replicated-state-ready.

### 1.10 CI upgrades
- Multi-arch determinism job: same log replayed on x86 + ARM runners,
  assert hash equality (mechanical enforcement of the core invariant)
- fsync/write-throughput benchmark tracked across commits
- `cargo-deny` license scan (no AGPL transitive deps, ever)

### 1.11 Deploy rung 1
- Multi-stage distroless `Dockerfile` + `docker-compose.yml` for a local
  3-node topology (single-node useful today; becomes the Phase 2 dev rig).

**Phase 1 exit:** all green, behavior-neutral except documented v3 header,
design docs merged (erasure, security, schema evolution), main releasable.

---

## Phase 2 — Cluster Mode via openraft (~6–10 weeks, ships as beta)

Goal: `valori-node --mode cluster` — N-node HA, quorum-durable writes,
provably identical replicas. Kafka/Pulsar rejected as core (external broker
= operational burden + the source of truth leaves our trust boundary);
openraft is the replicated log *and* the consensus, in-process, in Rust.

### 2.1 `valori-consensus` crate
- openraft `RaftTypeConfig`; log store; vote/membership store (redb or
  plain fsync'd files); state-machine adapter over `KernelState::apply_event`;
  snapshot adapter over the existing V4 snapshot build/restore.
- **The storage split (key design decision):** Raft requires truncating
  uncommitted tail entries on leader change; the audit log must never
  truncate. Therefore:
  ```
  data_dir/
    raft/vote, raft/log/segment-*.bin   ← truncatable tail, purged after snapshot
    audit/events.log                    ← v3 chained log, append-only forever,
                                          written at APPLY time (committed-only;
                                          Raft never un-commits → never truncates)
    snapshots/state-*.snap
  ```
  Double sequential write per commit = the price of an honest audit
  guarantee; group commit amortizes it.
- Group-commit batching: one quorum fsync covers concurrent requests.
- Log compaction = snapshot + purge Raft segments below snapshot index
  (aligned with the existing 256 MiB rotation concept).

### 2.2 Networking
- tonic/gRPC transport for Raft RPCs over private network
- rustls mTLS between nodes (a malicious peer joining Raft is game over)
- Connection handshake: binary version + wire version + format ID —
  refuse mismatched clusters until the rolling-upgrade protocol (Phase 3).

### 2.3 Modes & upgrade path
- `--mode standalone` (default, today's path) | `--mode cluster`
  with `--node-id`, `--peers` (user decides N; docs: odd numbers, 3 or 5).
- **Standalone → cluster:** existing data dir seeds a single-member
  cluster (same state hash, provable), then `add-node` grows it. No
  export/import, no re-ingestion, no separate download.

### 2.4 Request dedup
- Client request IDs (schema from 1.2) → dedup table INSIDE the
  replicated state machine (must be replicated state or replicas disagree
  on what is a duplicate). Retried inserts commit exactly once.

### 2.5 Read path
- Engine lock: reads move off the global mutex (RwLock / snapshot reads)
  so a slow query can never stall Raft heartbeats into a spurious election.
- Leader reads linearizable via read-index. (Full tiered-read UX → Phase 3.)

### 2.6 Testing (the phase lives or dies here)
- `turmoil` simulated-network suite: partitions, leader churn, message
  loss/reorder — invariant: all surviving nodes hash-identical
- `proptest` event-sequence fuzz across simulated 3-node cluster
- Real-process 3-node `kill -9` test (extends `crash_durability.rs`):
  zero acked-write loss with any single node killed
- Divergence injection: corrupt a follower's state → `state_hash_match`
  gauge flips + healing path (snapshot re-install) recovers it
- Mixed-arch validation: x86 leader + ARM followers, identical hashes
  (Graviton cost story + the demo nobody else can run)

### 2.7 Observability
- Metrics: `raft_term`, `commit_index`, `applied_index`, replication lag,
  `state_hash_match` (the signature gauge)
- Shipped Prometheus alert rules + a written runbook ("hash mismatch at
  3am: do X")

### 2.8 Deploy rung 2
- Helm chart: StatefulSet (stable identities = node IDs), headless
  service (peer DNS), PVC per pod, PodDisruptionBudget, topology spread
  across 3 AZs; `/v1/cluster/role` endpoint for LB leader-routing
- compose demo: `docker compose up` → 3 nodes → kill one → still serving,
  hashes still equal

**Phase 2 exit:** 3-node cluster survives any single-node kill with zero
acknowledged-write loss; turmoil suite green; merged to main behind
`--features cluster`, documented as beta.

---

## Phase 3 — GA & Product (~4–6 weeks)

- Read tiers as API: linearizable | verified-stale (hash + log-height
  stamped responses) | local
- `valori cluster add-node / remove-node` (openraft joint consensus) —
  these APIs are also the future autoscaling hooks
- Rolling-upgrade protocol (vN reads vN−1) + mixed-version handshake relax
- Cluster-aware Python SDK: leader discovery, retry-with-redirect on
  failover, idempotency tokens by default
- `valori-import`: Qdrant (scroll API) first, Pinecone second, JSONL/
  Parquet always; imports are "genesis import" events — provable from the
  moment of migration; document Q16.16 quantization honestly
- Crypto-shredding implementation (design from 1.5) — GDPR erasure that
  preserves chain verification
- Per-tenant API keys; encryption at rest
- S3/Blob archival of sealed segments with Object Lock (WORM) +
  `valori-verify` against bucket contents; cross-region bucket replication
  as the DR story
- As-of / point-in-time reads ("what did the AI know on March 3") —
  event sourcing gives this nearly free; productize it
- Terraform modules (AWS, then Azure) — BYOC deployments into customer
  accounts (the compliance-friendly middle step before hosted SaaS)
- Signed releases + SBOM; SOC 2 Type II process started
- Write-throughput regression gates in CI

---

## Phase 4 — Scale & Cloud (demand-driven, design docs only until then)

- Kubernetes operator: automates Helm rung, drives add/remove-node =
  autoscaling; foundation of the managed cloud data plane
- Managed cloud control plane (provisioning, billing, tenants) —
  proprietary, the open-core revenue line; data plane = same OSS clusters
- Shard-by-collection: one Raft group per collection, stateless router,
  Merkle root over per-collection hashes for a cluster-wide proof.
  NEVER intra-graph sharding (cross-shard edges destroy O(degree)
  cascades and fragment the proof story)
- Q8.8 / Q32.32 activation (seam exists from 1.3) when a real embedded /
  finance customer demands it; `valori migrate-format` with provable
  genesis-link (new log's genesis records old log's final state hash)
- Kafka/Pulsar edge connectors (ingest buffer / CDC out) — edge only,
  never the consensus core
- Reproducible builds (deterministic database, deterministic binary)
