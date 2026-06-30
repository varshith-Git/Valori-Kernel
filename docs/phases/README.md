# Phase Reports

One report per delivered phase of the multi-node roadmap
([docs/MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md)). Each report records
what shipped, what was found along the way, and the validation evidence —
so the history of *why* the codebase looks the way it does survives the
people and sessions that built it.

## Status

| Phase | Report | Commit | Status |
|---|---|---|---|
| 0 — Baseline durability & verifier | [phase-0-baseline.md](phase-0-baseline.md) | merged via PR #3 (`57da43e`) | ✅ done |
| 1.1 — Workspace restructure | [phase-1.1-workspace-restructure.md](phase-1.1-workspace-restructure.md) | `2bd793d` | ✅ done |
| 1.1b — Per-crate test layout + kernel fixes | [phase-1.1b-per-crate-tests.md](phase-1.1b-per-crate-tests.md) | `1db62c9` | ✅ done |
| 1.2 — valori-wire + segment format v3 | [phase-1.2-valori-wire-v3.md](phase-1.2-valori-wire-v3.md) | `b4ac53b` | ✅ done |
| 1.3 — FxpFormat seam (configurable precision) | [phase-1.3-fxpformat-seam.md](phase-1.3-fxpformat-seam.md) | `22f600b` | ✅ done |
| 1.4 — Collections seam | [phase-1.4-collections-seam.md](phase-1.4-collections-seam.md) | `41fe5b6` | ✅ done |
| 1.5 — Crypto-shredding design (GDPR) | [phase-1.5-crypto-shredding.md](phase-1.5-crypto-shredding.md) | `003ce7e` | ✅ done |
| 1.6 — Security design doc | [phase-1.6-security-model.md](phase-1.6-security-model.md) | see git log | ✅ done |
| 1.7 — Verifier hardening (limits + fuzzing) | [phase-1.7-verifier-hardening.md](phase-1.7-verifier-hardening.md) | see git log | ✅ done |
| 1.8 — Storage policy (snapshot cadence, zstd, disk-full) | [phase-1.8-storage-policy.md](phase-1.8-storage-policy.md) | see git log | ✅ done |
| 1.9 — Committer trait seam | [phase-1.9-committer-trait.md](phase-1.9-committer-trait.md) | see git log | ✅ done |
| 1.10 — CI upgrades (multi-arch hash equality, cargo-deny) | [phase-1.10-ci-upgrades.md](phase-1.10-ci-upgrades.md) | see git log | ✅ done |
| 1.11 — Docker + compose | [phase-1.11-docker-compose.md](phase-1.11-docker-compose.md) | see git log | ✅ done |
| 2.1 — openraft type config | [phase-2.1-openraft-types.md](phase-2.1-openraft-types.md) | see git log | ✅ done |
| 2.2 — Raft log store | [phase-2.2-raft-log-store.md](phase-2.2-raft-log-store.md) | see git log | ✅ done |
| 2.3 — Raft state machine (kernel + audit) | [phase-2.3-raft-state-machine.md](phase-2.3-raft-state-machine.md) | see git log | ✅ done |
| 2.4 — gRPC transport (tonic) | [phase-2.4-grpc-transport.md](phase-2.4-grpc-transport.md) | see git log | ✅ done |
| 2.5 — RaftCommitter + cluster bootstrap | [phase-2.5-raft-committer.md](phase-2.5-raft-committer.md) | see git log | ✅ done |
| 2.6 — Cluster management API | [phase-2.6-cluster-api.md](phase-2.6-cluster-api.md) | see git log | ✅ done |
| 2.7 — Snapshot transfer | [phase-2.7-snapshot-transfer.md](phase-2.7-snapshot-transfer.md) | see git log | ✅ done |
| 2.8 — Fault-tolerance tests | [phase-2.8-fault-tolerance.md](phase-2.8-fault-tolerance.md) | see git log | ✅ done |
| 2.9 — Admin audit events in chain | [phase-2.9-admin-audit-events.md](phase-2.9-admin-audit-events.md) | see git log | ✅ done |
| 2.10a — Persistent Raft log (redb) | [phase-2.10a-persistent-raft-log.md](phase-2.10a-persistent-raft-log.md) | see git log | ✅ done |
| 2.10b — mTLS (rustls + cluster CA) | [phase-2.10b-mtls.md](phase-2.10b-mtls.md) | see git log | ✅ done |
| 2.10c — Metrics (Prometheus) | [phase-2.10c-raft-metrics.md](phase-2.10c-raft-metrics.md) | see git log | ✅ done |
| 2.10d — Partition harness | [phase-2.10d-partition-harness.md](phase-2.10d-partition-harness.md) | `multinode` | ✅ done |
| 2.11 — Boot dispatch + cluster data plane v1 | [phase-2.11-cluster-boot-dispatch.md](phase-2.11-cluster-boot-dispatch.md) | see git log | ✅ done |
| 3.1 — S3 object store (snapshot offload + WAL archival) | [phase-3.1-s3-object-store.md](phase-3.1-s3-object-store.md) | `multinode` | ✅ done |
| 3.2 — Rolling upgrades (zero-downtime version migration) | [phase-3.2-rolling-upgrades.md](phase-3.2-rolling-upgrades.md) | `multinode` | ✅ done |
| 3.3 — Cluster-aware Python SDK | [phase-3.3-cluster-sdk.md](phase-3.3-cluster-sdk.md) | `multinode` | ✅ done |
| 3.5 — Per-tenant API keys + RBAC | [phase-3.5-api-keys-rbac.md](phase-3.5-api-keys-rbac.md) | `multinode` | ✅ done |
| 3.6 — Crypto-shredding (GDPR erasure) | [phase-3.6-crypto-shredding.md](phase-3.6-crypto-shredding.md) | `multinode` | ✅ done |
| 3.7 — `valori import` (Qdrant + JSONL migration) | [phase-3.7-valori-import.md](phase-3.7-valori-import.md) | `multinode` | ✅ done |
| 3.4 — As-of / point-in-time reads | [phase-3.4-as-of-reads.md](phase-3.4-as-of-reads.md) | `multinode` | ✅ done |
| 5.1 — Verifiable AI: Proof-Carrying Answers + Compliance Pack | [phase-5.1-verifiable-ai.md](phase-5.1-verifiable-ai.md) | `multinode` | ✅ done |
| B13 — Snapshot cadence + startup readiness gate | [phase-B13-snapshot-readiness.md](phase-B13-snapshot-readiness.md) | `multinode` | ✅ done |
| C0 — Eval harness (recall@k, citation, provenance) | [phase-C0-eval-harness.md](phase-C0-eval-harness.md) | `multinode` | ✅ done |
| C0 plan — Cortex converged build plan | [phase-C0-cortex-plan.md](phase-C0-cortex-plan.md) | `multinode` | 📋 plan |
| C1 — Contextual retrieval + audited enrichment | [phase-C1-contextual-retrieval.md](phase-C1-contextual-retrieval.md) | `multinode` | ✅ done |
| C2 — Audited entity graph + provenance receipt | [phase-C2-entity-graph.md](phase-C2-entity-graph.md) | `multinode` | ✅ done |
| C3 — Self-maintaining memory (UI-only; **superseded by C4**) | [phase-C3-self-maintaining-memory.md](phase-C3-self-maintaining-memory.md) | `multinode` | ⚠️ superseded |
| C4.1 — Kernel-native time decay (self-maintaining pillar 1) | [phase-C4.1-decay.md](phase-C4.1-decay.md) | `multinode` | ✅ done |
| C4.1b — Cluster decay + state-machine creation timestamps | [phase-C4.1b-cluster-decay.md](phase-C4.1b-cluster-decay.md) | `main` | ✅ done |
| C4.2 — Memory consolidation (self-maintaining pillar 2) | [phase-C4.2-consolidation.md](phase-C4.2-consolidation.md) | `main` | ✅ done |
| C4.3 — Contradiction detection (self-maintaining pillar 3) | [phase-C4.3-contradiction.md](phase-C4.3-contradiction.md) | `main` | ✅ done |
| 3.8 — Write-throughput regression gates | [phase-3.8-write-regression.md](phase-3.8-write-regression.md) | `multinode` | ✅ done |
| 3.9 — Terraform modules (AWS + Azure) | [phase-3.9-terraform.md](phase-3.9-terraform.md) | `multinode` | ✅ done |
| 3.10 — Signed releases + SBOM | [phase-3.10-signed-releases.md](phase-3.10-signed-releases.md) | `multinode` | ✅ done |
| 3.11 — Concurrent reads via RwLock engine | [phase-3.11-rwlock-engine.md](phase-3.11-rwlock-engine.md) | `multinode` | ✅ done |
| 3.12 — Batch insert per-item idempotency | [phase-3.12-batch-idempotency.md](phase-3.12-batch-idempotency.md) | `multinode` | ✅ done |
| 3.13 — HNSW parameter exposure | [phase-3.13-hnsw-params.md](phase-3.13-hnsw-params.md) | `multinode` | ✅ done |
| 3.14 — MCP server (verifiable agent memory) | [phase-3.14-mcp-server.md](phase-3.14-mcp-server.md) | `multinode` | ✅ done |
| 3.15 — Native GraphRAG (one-call retrieval) | [phase-3.15-graphrag.md](phase-3.15-graphrag.md) | `multinode` | ✅ done |
| 6 — Persistent, isolated projects (UI workspace) | [phase-6-persistent-projects.md](phase-6-persistent-projects.md) | `main` | ✅ done |
| C5 — Valori Reranker (hybrid retrieval) | [phase-C5-valori-reranker.md](phase-C5-valori-reranker.md) | `main` | ✅ done |
| I1 — Server-side document chunking (`/v1/ingest/document`) | [phase-I1-server-chunking.md](phase-I1-server-chunking.md) | `main` | ✅ done |
| I2 — On-node embedding + full pipeline (`/v1/ingest`) | [phase-I2-on-node-embedding.md](phase-I2-on-node-embedding.md) | `main` | ✅ done |
| I3 — UI wired through server pipeline with auto-fallback | [phase-I3-ui-server-pipeline.md](phase-I3-ui-server-pipeline.md) | `main` | ✅ done |
| I4 — `/v1/ingest` wired into cluster mode (Raft path) | [phase-I4-cluster-ingest.md](phase-I4-cluster-ingest.md) | `main` | ✅ done |
| I5 — Tree-RAG: hierarchical retrieval + citations + replayable receipts | [phase-I5-tree-rag.md](phase-I5-tree-rag.md) | `main` | ✅ done |
| I6 — Community layer: Label Propagation + centroid search + entity extraction | [phase-I6-community-layer.md](phase-I6-community-layer.md) | `main` | ✅ done |
| I7 — Metadata filtering: JSON predicate post-filter on `/search` (both paths) | [phase-I7-metadata-filter.md](phase-I7-metadata-filter.md) | `main` | ✅ done |
| P1 — Million-scale performance: growable-Vec snapshots (fixes `CapacityExceeded` at 1M), WAL flush-on-drop, SIMD L2, benchmark suite | [phase-P1-million-scale-performance.md](phase-P1-million-scale-performance.md) | `main` | ✅ done |
| P2 — IVF centroid auto-scaling (k = sqrt(N)); `needs_rebuild()` hook; `VALORI_IVF_N_LIST`/`VALORI_IVF_N_PROBE` overrides | [phase-P2-ivf-centroid-scaling.md](phase-P2-ivf-centroid-scaling.md) | `main` | ✅ done |

## Report template

Every report answers five questions:

1. **Goal** — what this phase was supposed to achieve (1–2 sentences)
2. **Delivered** — what actually landed, file by file where it matters
3. **Findings** — bugs and design gaps discovered during the work
   (often the most valuable section)
4. **Validation** — the evidence: test counts, demos, end-to-end runs
5. **Follow-ups** — anything consciously deferred, and to which phase
