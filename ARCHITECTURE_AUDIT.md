# Valori — Architecture Audit

**Status:** Stage 1 deliverable — audit only, no code changed
**Date:** 2026-08-08
**Branch audited:** `feat/node-object-store-durability-and-metrics` @ `e44b814`
**Scope:** Rust workspace (22 crates), `desktop/` (Tauri), `ui/` (Next.js — Studio + Cloud frontends)
**Implementation status:** this document records the repository as of the audit.
Steps M0, M1, M2 and M2.5 of §22 have since landed — see
[`docs/phases/phase-M0-M2-platform-contracts.md`](docs/phases/phase-M0-M2-platform-contracts.md)
and [`docs/architecture/ownership.md`](docs/architecture/ownership.md). M3 onward is not started;
the duplications catalogued below still exist, by design, until their consumers migrate.

---

## How to read this document

This is a description of **what the repository actually contains today**, derived by
reading source files, `Cargo.toml` dependency edges, and route declarations. It is not
a description of what the RFCs specify.

Where an RFC describes something that is not implemented, this document says so
explicitly. `rfcs/0004-capability-model.md` and `rfcs/0005-crate-boundaries.md` are
design intent; only the parts traced to source below are counted as existing.

Every proposed abstraction carries one of four statuses:

| Status | Meaning |
|---|---|
| ✅ **Already exists** | Implemented in source, in use by at least one consumer |
| 🟡 **Partially exists** | Implemented for a subset of cases, or implemented at the wrong layer |
| ❌ **Missing** | No implementation anywhere in the repository |
| ⏸ **Intentionally deferred** | Deliberately not built — no second implementation or consumer exists yet |

---

# 1. Current repository / workspace architecture

One git repository containing three deployable products plus a Python SDK.

```
Valori-Kernel/                     (this repo — public/OSS)
├── crates/            22 Rust crates, cargo workspace
├── desktop/           Tauri 2 shell (Valori Studio)
│   ├── src-tauri/     1,585 LOC Rust
│   └── node-launcher/
├── ui/                Next.js 16 / React 19 — 42,507 LOC TS/TSX
│                      Serves BOTH Studio (local) and Cloud frontends
├── python/valoricore/ Python SDK (sync + async remote, embedded via FFI)
├── embedded/          Cortex-M firmware (excluded from workspace)
├── rfcs/              0000–0007 design specs
└── docs/              architecture/, phases/ (60+ phase reports)
```

**Workspace members:** 21 crates. `default-members` is a 13-crate subset;
`valori-daemon`, `valori-models`, `valori-engine`, `valori-search`, `valori-index`,
`valori-rag`, `valori-ingest` are workspace members but **not** default members.
`valori-ffi` and `embedded/` are excluded from default builds for documented
link-time reasons.

**Size distribution (source LOC, excluding tests):**

| Component | LOC |
|---|---|
| `valori-node` | 18,514 |
| `ui/src` (TS/TSX) | 42,507 |
| `valori-kernel` | 5,471 |
| `valori-models` | 3,914 |
| `valori-daemon` | 3,518 |
| `valori-effect` | 2,454 |
| `valori-engine` | 2,359 |
| `valori-planner` | 1,685 |
| `desktop/src-tauri` | 1,585 |
| `valori-core` | 450 |

**Observation.** The two largest components — `valori-node` and `ui/src` — are also
the two least layered. `ui/src` is larger than the entire Rust workspace minus
`valori-node`. That ratio is the core finding of this audit and drives §16.

---

# 2. Current crate dependency graph

Extracted mechanically from each `[dependencies]` section (dev-dependencies shown
separately — they do not ship).

```
valori-core ─────────────────────────────────────── (zero valori deps, no_std)
  │
  ├─▶ valori-kernel ─────────────────────────────── (no_std; deps: valori-core)
  │     │
  │     ├─▶ valori-wire
  │     │     ├─▶ valori-storage   (+ valori-core)
  │     │     │     └─▶ valori-state   (+ valori-core, valori-kernel)
  │     │     └─▶ valori-metadata  (+ valori-core)
  │     │           └─▶ valori-planner  (+ valori-core)
  │     │                 └─▶ valori-effect  (+ valori-core)
  │     │
  │     ├─▶ valori-index
  │     ├─▶ valori-rag
  │     ├─▶ valori-verify   (+ valori-wire)
  │     └─▶ valori-consensus (+ valori-metadata)
  │
  ├─▶ valori-engine  ── kernel, index, search, ingest, rag, metadata, storage, state
  │     └─▶ valori-node   ── (15 crates; the convergence point)
  │           ├─▶ valori-cli
  │           ├─▶ valori-ffi   (+ kernel, verify)
  │           └─▶ valori-mcp   (dev-dep only)
  │
  ├─ valori-search   ── ZERO valori deps (operates on plain types)
  ├─ valori-models   ── ZERO valori deps
  │     ├─▶ valori-ingest
  │     └─▶ valori-daemon      ← only valori dep the daemon has
  │
  └─ desktop/src-tauri ── ZERO valori deps (HTTP only)
```

**Dev-dependency back-edges (do not ship, but worth knowing):**
- `valori-state` → `valori-verify` (dev)
- `valori-verify` → `valori-node` (dev, deliberate: wire-compat test)
- `valori-mcp` → `valori-node` (dev)

**Findings:**

| # | Finding | Status |
|---|---|---|
| 2.1 | Graph is **acyclic** in shipped dependencies. The `state→verify→node` chain exists only in dev-deps. | ✅ healthy |
| 2.2 | `valori-kernel` depends only on `valori-core`. No Cloud, Studio, or std leakage. | ✅ healthy |
| 2.3 | `valori-daemon` depends on **`valori-models` only** — not on `valori-core`. It therefore re-invents `Project` from scratch (§9). | 🟡 the key structural gap |
| 2.4 | `desktop/src-tauri` has **zero** Valori crate dependencies. All communication is HTTP to daemon/node. | ✅ already a thin adapter |
| 2.5 | `valori-search` and `valori-models` have zero intra-workspace deps — they are already reusable leaves. | ✅ |
| 2.6 | `valori-node` has 15 direct valori deps and 18.5k LOC. It is the convergence point *and* the god-crate. | 🟡 known, out of scope here |
| 2.7 | Enforcement: `crates/valori-node/tests/architecture.rs` only detects **duplicate source files** across crate boundaries. There is **no automated dependency-direction test**. `deny.toml` has no layer rule. | ❌ missing guard |

---

# 3. Current desktop architecture

```
desktop/src-tauri/           1,585 LOC
├── lib.rs                     531  — 12 Tauri commands + setup/tray/updater
├── telemetry.rs               493  — queue-first event pipeline (events.jsonl)
├── daemon_manager.rs          415  — spawn/supervise the valori-daemon binary
├── ui_server_manager.rs       140  — serve the bundled Next.js build
└── main.rs                      6
```

**The 12 registered commands** (`lib.rs:334`):
`node_health`, `start_daemon`, `stop_daemon`, `daemon_status`, `add_recent_document`,
`install_update`, `open_cloud_login`, `get_app_info`, `get_session_id`,
`get_rust_start_ms`, `enqueue_telemetry_event`, `check_and_clear_crash_marker`.

**Assessment.** Every one of these is either (a) native-only OS concern (recent
documents, updater, deep-link login, crash marker), (b) process supervision of the
daemon, or (c) telemetry plumbing. **None contain domain business logic.**

| Concern | Status |
|---|---|
| Tauri as thin adapter (§15 of the brief, hard rule 5) | ✅ **already satisfied** |
| Studio Rust services layer behind Tauri | 🟡 exists as `valori-daemon`, but reached over HTTP, not as a linked crate |
| Local AI/model execution in Studio | ❌ missing (`valori-models` exists but is not linked into desktop) |

**Note for Stage 3.** The brief assumes Tauri commands are fat. They are not. The
actual Stage-3 work is §16 (TypeScript → Rust), not §15.

---

# 4. Current Next.js / server architecture

One Next.js app serves both products:

```
ui/src/app/
├── (Studio routes)   projects/ collections* search/ graph* proof/ snapshots/
│                     operations/ metrics/ logs/ audit/ cluster/ playground/
│                     launch/ settings/ help/ onboarding
├── cloud/            Cloud product — projects, archived, settings/{team,
│                     api-keys, security, developer}, projects/[id]/{metrics,proof,…}
├── auth/ login/ forgot-password/ reset-password/ desktop-handoff/
└── api/              88 route.ts files
```

**Layering as documented in `docs/architecture/control-plane.md`:**

```
Tauri  →  ui/ (Next.js pages)  →  ui/src/app/api/*  →  { valori-daemon | valori-node }
```

React pages never call the daemon or node directly; `/api/*` is the compatibility
layer. That rule is real and is followed.

**Server-side TypeScript (`ui/src/lib/server/`, 2,034 LOC, 17 files):**

| File | LOC | What it does |
|---|---|---|
| `projects.ts` | 436 | Project manifest read/write/migrate + port allocation — a **fourth** `Project` implementation |
| `process-manager.ts` | 427 | **Spawns and supervises `valori-node` processes** for 3-node cluster projects |
| `daemon.ts` | 193 | HTTP client for `valori-daemon` |
| `embed.ts` | 139 | Embedding provider dispatch |
| `project.ts` | 127 | Project resolution helpers |
| `llm.ts` | 102 | LLM provider dispatch |
| `api-client.ts` | 89 | Mode-aware (local vs cloud) client |
| `project-adapter.ts` | 84 | Adapter between daemon shape and UI shape |
| `connection.ts` / `nodeProxy.ts` / `http.ts` | 184 | Node connection + proxying |
| `extract-text.ts` | 80 | PDF/DOCX text extraction |
| `cluster-config.ts` | 55 | Cluster topology helpers |
| `reranker.ts` | 54 | Reranker dispatch |
| others | 64 | mfa, content-filter, app-url |

**Cloud data access.** Cloud pages use Supabase directly from Next.js server actions
(49 files reference supabase). Tables observed: `projects`, `org_members`,
`org_invitations`, `subscriptions`, `api_keys_public`, `personal_access_tokens_public`,
`service_accounts`, `ip_allowlist_rules`, `login_history`.

There is **no Rust cloud control plane in this repository**. Cloud business logic
today lives in TypeScript server actions against Supabase.

---

# 5. Existing Planner → ExecutionGraph → Executor architecture

**This is the strongest asset in the codebase and must not be replaced.**

```
HTTP handler
   ↓  builds
Operation { kind: OperationKind, inputs: OperationInputs, hash: OperationHash }
   ↓  Planner::plan(op, PlanningContext) — content-addressed, 2-layer cached
ExecutionGraph { nodes: Vec<TaskSpec>, edges }  — Kahn topo-sort, GraphHash
   ↓  TaskRunner (valori-node) / run_graph_inline
Task impls (valori-effect)  →  EffectBus  →  CapabilityRegistry  →  engine / raft
```

**What exists in source:**

| Primitive | Where | Status |
|---|---|---|
| `Operation`, `OperationHash` (BLAKE3 content address) | `valori-planner/src/operation.rs` | ✅ |
| `OperationKind` — **16 variants** | `operation.rs:57` | 🟡 8 wired, 8 declared-not-wired |
| `OperationInputs` — planning params only, no vectors/text, deterministic | `operation.rs:95` | ✅ |
| `ExecutionGraph`, `GraphHash`, Kahn topological sort | `valori-planner/src/graph.rs` (374 LOC) | ✅ |
| `TaskSpec`, `TaskKind` — **18 variants** | `graph.rs:46` | ✅ |
| `ExecutionStatus` {Pending, Running{completed,total}, Succeeded, Failed{reason}, Cancelled} | `valori-planner/src/registry.rs:22` | ✅ |
| `ExecutionHandle` (tokio watch), `ExecutionRegistry` | `registry.rs` | ✅ |
| `PlanningContext`, `PlannerFingerprint`, `PlanningContextHash` | `context.rs` | ✅ |
| Two-layer planner cache (in-process + `MetadataDb`) | `planner.rs`, `valori-metadata/src/planner_cache.rs` | ✅ |
| `EffectBus`, `EffectId`, `EffectDurability`, `EffectPayload` | `valori-effect/src/{bus,effect}.rs` | ✅ |
| Concrete Tasks — 10 files | `valori-effect/src/tasks/` | ✅ |
| `Receipt`, `ReceiptAssembler`, `verify_receipt` | `valori-effect/src/receipt.rs` (468 LOC) | ✅ |
| Task retry | `TaskRunner` (valori-node) | 🟡 exists in runner, no declarative `RetryPolicy` type |

**`OperationKind` — 16 variants, wiring status per source comments:**

- **Wired** (endpoint → Operation → Planner → Graph → Executor): `Ingest`, `Search`,
  `MemoryUpsert`, `Consolidate`, `Contradict`, `HealthCheck`, `Delete`, `BatchInsert`
- **Declared but handler still calls logic directly**: `GraphRag`, `MemorySearch`,
  `CommunityDetect`, `CommunitySearch`, `TreeBuild`, `TreeQuery`, `TreeHybrid`,
  `Snapshot`

  Phase A13 / A13.1 routed 8 standalone and 7 cluster handlers through
  `run_graph_inline`; the source comment listing these as "planned" is stale relative
  to A13. **Treat the wiring status as needing re-verification, not as documentation.**

**Missing execution primitives (§11 of the brief):**

| Primitive | Status | Note |
|---|---|---|
| `Pipeline`, `PipelineStep` as first-class types | ❌ missing | `valori-ingest/src/config.rs` has a `PipelineConfig` and `execution.rs` a `PipelineResult`, but these are ingest-local, not a platform primitive |
| `ExecutionEvent` (stream of state changes) | ❌ missing | only `ExecutionStatus` snapshots via watch channel |
| `Checkpoint` | ❌ missing | |
| `RetryPolicy` as a declarative type | 🟡 partial | retry behavior is in `TaskRunner`, not a spec'd type on `TaskSpec` |
| `ResourceRequirements` | ❌ missing | |

**Conclusion.** Hard rule 9 ("do not build a second workflow engine") is already
satisfied by construction. The gap is *coverage* (more `OperationKind`s) and four
missing primitives — not architecture.

---

# 6. Existing model / runtime architecture

Three distinct things in this repo are called or could be called "runtime". They are
**not** the same concept and must not be merged (constraint D).

### 6a. Process runtime — ✅ already exists, keep the name

`crates/valori-daemon/src/runtime/` (1,300 LOC)

```rust
pub trait Runtime: Send + Sync { /* runtime/mod.rs:77 */ }
pub struct NodeInfo  { /* mod.rs:38 */ }
pub struct NodeExit  { /* mod.rs:69 */ }
```
Modules: `local.rs` (530 — the only impl), `state.rs`, `launcher.rs`, `port.rs`,
`resource.rs`.

This abstracts **"where does a `valori-node` OS process run"** — LocalRuntime today,
Docker/SSH later. It is well designed and correctly scoped. **Do not rename it.**

### 6b. AI / model runtime — 🟡 embedding only

`crates/valori-models/src/provider/mod.rs:18`

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn kind(&self) -> &'static str;
    fn model_name(&self) -> &str;
    fn dim(&self) -> usize;
    async fn embed(&self, texts: &[String]) -> ModelResult<Vec<Vec<f32>>>;
    async fn health(&self) -> ModelResult<()>;
}
```

Impls: `ollama.rs`, `openai.rs`, `voyage.rs`, `dummy.rs`. Plus `ProviderRegistry`
(282 LOC) with factory registration and `build_from_manifest`.

| Capability | Status |
|---|---|
| `embed` | ✅ |
| `generate` | ❌ missing from the trait (LLM calls go through `valori-rag/src/llm.rs` and `ui/src/lib/server/llm.ts` instead) |
| `rerank` | ❌ missing from the trait (`valori-search/src/reranker.rs` + `ui/src/lib/server/reranker.ts`) |
| `transcribe`, `vision` | ❌ missing (declared in `ModelTask` enum, no execution path) |

`ModelTask` already enumerates `{Embedding, Generation, Reranker, Vision, Speech}` —
the taxonomy exists, the execution interface covers only one of the five.

### 6c. Hosted inference runtime — ❌ missing

No GPU scheduler, no hosted-inference dispatch, no `DeploymentId`. This is Cloud
territory and belongs in the private repo.

---

# 7. Existing storage / state architecture

```
valori-kernel     in-memory KernelState; RecordPool slab; graph; V6 snapshot
                  encode/decode; Q16.16 fxp; BLAKE3 chain      (no_std)
      │
valori-wire       V2/V3/V4 event-log format — encode/decode/chain
      │
valori-storage    wal_writer.rs, wal_reader.rs, wal_compat.rs, events/,
                  object_store.rs (S3 / B2 / file)
      │
valori-state      bootstrap.rs  — snapshot restore + WAL replay orchestration
      │
valori-metadata   redb control plane: project.rs, collection.rs,
                  planner_cache.rs, db.rs
```

**Status: ✅ clean and well layered.** This is the part of the codebase where the
domain/persistence separation the brief asks for already exists in spirit:
`KernelState` (domain) is separate from the WAL/snapshot encoding (persistence),
separated again from `MetadataDb` (control plane).

The layering here is the model to copy for §9, not to change.

One boundary note: `valori-metadata` owns a `Project` (§9) — control-plane
persistence — while `valori-daemon` owns a different `ProjectManifest` for the same
concept. Two persistence models, no shared domain model above them.

---

# 8. Duplicate domain concepts

| Concept | Implementations found | Status |
|---|---|---|
| **Project** | 4 — `valori-daemon::ProjectManifest`, `valori-metadata::Project`, `ui/src/lib/server/projects.ts::ProjectEntry`, Supabase `projects` table | ❌ worst offender — §9 |
| **Embedding dispatch** | 3 — `valori-models::ModelProvider` (Rust), `valori-node` `EmbedConfig` path, `ui/src/lib/server/embed.ts` (139 LOC TS) | 🟡 |
| **LLM dispatch** | 2 — `valori-rag/src/llm.rs`, `ui/src/lib/server/llm.ts` (102 LOC TS) | 🟡 |
| **Reranker** | 2 — `valori-search/src/reranker.rs`, `ui/src/lib/server/reranker.ts` (54 LOC TS) | 🟡 |
| **Node process supervision** | 2 — `valori-daemon::LocalRuntime` (530 LOC Rust), `ui/src/lib/server/process-manager.ts` (427 LOC TS) | 🟡 documented exception, cluster projects only |
| **Cluster topology config** | 3 — `daemon::ClusterConfig`, `metadata::ClusterNodeConfig`, `ui/.../cluster-config.ts` | 🟡 |
| **Event / telemetry model** | 4 — see §11 | ❌ |
| **Collection / namespace** | 2 — `valori-core::NamespaceId`/`CollectionId` (aliased), `valori-metadata::CollectionRegistry` | ✅ acceptable — alias is deliberate, registry is persistence |
| **Index kind** | 3 — `valori-metadata::IndexKind` (enum), `daemon::ProjectManifest.index` (`String`), TS string-literal union | 🟡 |

---

# 9. Duplicate `Project` implementations

The single highest-priority finding. Four representations of one concept, with
**divergent field names, divergent types, and divergent primary keys**.

### The four

| # | Location | Role today |
|---|---|---|
| 1 | `crates/valori-daemon/src/project.rs:107` `ProjectManifest` (+ `Project` at :149 = manifest + resolved paths) | On-disk `project.json`, one per project dir. RFC-0006 authority. |
| 2 | `crates/valori-metadata/src/project.rs:62` `Project` | redb control-plane record |
| 3 | `ui/src/lib/server/projects.ts:62` `ProjectEntry` | Legacy TS manifest reader/writer, still live for cluster projects |
| 4 | Supabase `projects` table (Cloud) | Cloud persistence, schema not in this repo |

### Field-level divergence

| Field | daemon `ProjectManifest` | metadata `Project` | TS `ProjectEntry` |
|---|---|---|---|
| identity | `id: String` (UUID) + `name: String` | **no id** — keyed on `name: String` | **no id** — keyed on `name: string` |
| directory | `dir: PathBuf` (on `Project`, not manifest) | `dir: PathBuf` | `dir: string` |
| dimension | `dim: usize` | `dim: u16` | `dim: number` |
| index | `index: String` | `index: IndexKind` (enum) | `"brute"\|"hnsw"\|"ivf"\|"bq"\|"auto"` |
| replication | `cluster: Option<ClusterConfig>` | `node_count: u8` + `mode: ProjectMode` | `replication: 1 \| 3` |
| shards | **absent** | `shard_count: u8` | `shardCount: number` |
| port | *(in `ClusterConfig`)* | `port: u16` | `port: number` + `nodes[].httpPort` |
| workspace | `workspace: String` | **absent** | **absent** |
| restart policy | `restart_policy: RestartPolicy` | **absent** | **absent** |
| embedding | `embedding: EmbeddingConfig` | **absent** | `embed?: ProjectEmbedConfig` |
| storage | `storage: StorageConfig` | **absent** | **absent** |
| max records | **absent** | **absent** | `maxRecords: number` |
| record count | **absent** | `record_count: Option<u64>` | `records?: number` |
| collections | **absent** | **absent** | `collections?: string[]` |
| created | `created_at: u64` (unix) | `created_at: u64` (unix) | `createdAt: string` (ISO) |
| last opened | `last_opened_at: Option<u64>` | `last_opened_at: Option<u64>` | `lastOpenedAt?: string` |

Three primary-key strategies, three time encodings, three casings, three type
systems, and three different names for "how many replicas".

### Load-bearing coupling

`valori-daemon/src/project.rs` contains an explicit comment that
`node_event_log_path()` must match `projectNodePaths()` in `projects.ts`
**byte-for-byte**, because cluster projects created through the pre-daemon path
already have data at those paths. There is a one-time migration
(`migration/m001_project_registry.rs`, 390 LOC) bridging the TS manifest to the
daemon registry.

**This coupling is a migration risk (§20) and must be preserved through any
unification.**

### Proposed canonical model (design only — not implemented)

Per constraint B, the canonical model is a **domain** model with **explicit adapters**,
not a merged struct. Proposed home: a **new `valori-domain` crate** (see §21 — not
`valori-core`, to keep the kernel free of platform concepts).

```
                    valori-domain::Project          ← canonical domain model
                    (id, name, workspace, dim, index: IndexKind,
                     topology: Topology { replicas, shards },
                     embedding, storage, created_at: Timestamp,
                     last_opened_at: Option<Timestamp>)
                              │
        ┌──────────────┬──────┴───────┬────────────────┐
        ▼              ▼              ▼                ▼
 PersistenceProject  MetadataProject  ApiProject   (TS) UiProject
 daemon project.json  redb record     HTTP JSON    generated from ApiProject
 — keeps `id`,        — keeps         — stable      — camelCase, ISO dates
   restart_policy,      record_count,   wire names,   derived, never hand-written
   on-disk layout       shard topo      versioned
```

Four adapters, four `From`/`TryFrom` boundaries, one meaning. Explicitly **not** one
struct with 20 `Option` fields and four serde attribute sets.

Open questions to resolve before implementing:
- **Primary key.** Domain model should carry a `ProjectId` newtype. `metadata` and TS
  key on `name` today; unification requires either backfilling ids or keeping `name`
  as a unique secondary key. → recommend: `ProjectId` in domain, `name` retained as a
  unique mutable label, adapters resolve by name during a compatibility window.
- **Cloud `projects` table.** Schema is not in this repo. The `ApiProject` adapter is
  the contract Cloud must implement; do not assume the Supabase columns match.
- **`replication` vs `node_count` vs `cluster: Option<_>`.** These encode the same
  fact three ways with different null semantics. Domain model should use an explicit
  `Topology` enum, not a nullable struct.

---

# 10. Existing ID types and raw-string IDs

### Strongly typed today — `crates/valori-core/src/id.rs` (257 LOC)

| Type | Repr | Status |
|---|---|---|
| `RecordId` | `u32` | ✅ |
| `NodeId` | `u32` | ✅ |
| `EdgeId` | `u32` | ✅ |
| `NamespaceId` | `u16` | ✅ |
| `CollectionId` | `= NamespaceId` (alias) | ✅ deliberate |
| `ShardId` | `u32` | ✅ |
| `ExecutionId` | struct (`id.rs:98`) | ✅ |
| `ClusterEpoch` | — | ✅ |
| `OperationId` | `valori-planner/src/operation.rs:35` — newtype over `ExecutionId` | ✅ |

Constants: `DEFAULT_NS`, `NS_LIST_NIL`, `MAX_NAMESPACES`.

### Raw strings today

| Concept | Where | Current repr |
|---|---|---|
| Project id | `daemon::ProjectManifest.id` | `String` (UUID v4) |
| Project key | `metadata::Project.name`, TS `ProjectEntry.name` | `String` — used as PK *and* directory name |
| Workspace | `daemon::ProjectManifest.workspace` | `String` |
| Session id | `desktop/src-tauri/src/telemetry.rs` | `String` |
| Installation id | `telemetry.rs` | `String` |
| Event id | `telemetry.rs` `event_id` | `String` (UUID) |
| Model id | `valori-models` — models keyed by name/manifest | `String` |
| Provider kind | `ProviderKind` enum ✅ then `.as_str()` at every boundary | enum → `String` |
| Snapshot id | object-store key | `String` |
| Org / user / API key | Supabase columns, TS only | `string` |

### Classification: which IDs belong where (constraint A)

**This is the answer to "do not pollute the kernel".**

| ID | Home | Rationale |
|---|---|---|
| `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`/`CollectionId`, `ShardId`, `ClusterEpoch` | `valori-core` (already there) | Kernel data-plane concepts. `no_std`. |
| `ExecutionId`, `OperationId` | `valori-core` / `valori-planner` (already there) | Execution model is OSS core. |
| `ProjectId` | **`valori-domain` (new, OSS)** | A local Studio project is an OSS concept — Studio works fully offline. **Not** `valori-core`: the kernel has no notion of a project and must not gain one. |
| `SnapshotId`, `PipelineId` | `valori-domain` (OSS) | Same reasoning. |
| `ModelId`, `RuntimeId` | `valori-domain` (OSS) | Local model management is OSS (`valori-models` ships in the workspace). |
| `SessionId`, `InstallationId` | `valori-domain` (OSS) — **or** Studio-local | Desktop telemetry concepts. Defensible either way; recommend `valori-domain` since Cloud emits the same events. |
| `OrganizationId`, `UserId` | **Private Cloud crate** | No OSS consumer exists. A local Studio project has no org and no user. |
| `BillingAccountId`, `SubscriptionId` | **Private Cloud crate** | Commercial. Hard rule 8. |
| `DeploymentId`, `WorkerId` | **Private Cloud crate** | Hosted-inference / GPU-scheduler concepts; no OSS implementation exists or is planned. |
| `ConnectorId`, `PluginId` | ⏸ **deferred entirely** | No implementation, no second consumer (constraint G). |

**Explicit rejection of the brief's §3 list.** The brief lists `OrganizationId`,
`UserId`, `DeploymentId`, `WorkerId` among the primitives to standardize. Per
constraint A and hard rule 4, these must **not** enter `valori-core` or any crate the
kernel can reach. They belong to the private Cloud control plane. Doing otherwise
would make the OSS kernel carry commercial concepts for zero OSS benefit.

---

# 11. Existing event models

Four unrelated event models. None share a field, an ID type, or a schema.

| # | Model | Location | Shape | Purpose |
|---|---|---|---|---|
| 1 | `valori_daemon::Event` | `daemon/src/events.rs:18` | `{ time: u64, kind: String, resource: Option<String> }` behind an `EventStore` trait; only impl is a 1,000-entry `MemoryEventStore` ring buffer | Daemon lifecycle (`project.started`, `workspace.created`) |
| 2 | `TelemetryEnvelope` | `desktop/src-tauri/src/telemetry.rs` | `{ event_id, session_id, installation_id, event: String, properties: Value, app: AppInfo, ... }`, queued to `events.jsonl`, POSTed to `https://api.valori.systems/v1/telemetry/events` | Product telemetry |
| 3 | `KernelEvent` | `valori-kernel/src/event.rs` | Typed mutation enum, BLAKE3-chained into `events.log` | **Audit / determinism** — replayable proof chain |
| 4 | `ExecutionRecord` / `ExecutionStatus` | `valori-metadata`, `valori-planner/src/registry.rs` | Execution history in redb + tokio watch channel | Operation observability |

Plus a TS mirror: `ui/src/lib/telemetry.ts` (thin wrapper over command #2) and
`ui/src/lib/startupMarks.ts`, `ui/src/lib/event-types.ts`.

**Status of the §12 common event contract: ❌ missing.**

**Important caveat.** These four should **not** be collapsed into one type.
`KernelEvent` is the determinism substrate — it is BLAKE3-chained, replayed, and
version-locked by `COMPATIBILITY.md`. Adding telemetry fields to it would break the
audit chain. The correct target is a **common envelope for #1, #2, #4** (observability
and audit-of-control-plane), with `KernelEvent` explicitly excluded and left alone.

The existing `EventStore` trait (#1) is the right seam to grow into that envelope.

---

# 12. Existing API contract situation

**Status: ❌ no contract infrastructure exists.**

Verified absent from every `Cargo.toml` in the workspace: `utoipa`, `schemars`,
`ts-rs`, `typeshare`, `okapi`, `paperclip`. No OpenAPI document, no JSON Schema, no
generated client anywhere in the repo.

**What exists instead:**

| Surface | Count | Contract |
|---|---|---|
| `valori-node` standalone routes (`server.rs`) | 83 `.route()` calls | Hand-written axum handlers + serde structs |
| `valori-node` cluster routes (`cluster_server.rs`) | 77 `.route()` calls | Ditto, kept in sync manually |
| `valori-daemon` routes (`http.rs`, 440 LOC) | — | Hand-written |
| Next.js API routes | **88** `route.ts` files | Hand-written TS, hand-written types |
| TS types mirroring Rust | `ui/src/types/valori.ts` (67 LOC) + inline interfaces across `ui/src/lib/` | Hand-written, unenforced |
| Python SDK | `python/valoricore/remote.py` | Hand-written, ~40 endpoints |

**One mechanical guard does exist:** `crates/valori-node/tests/route_parity.rs` diffs
standalone vs cluster route declarations (paths and methods) and fails when they
diverge, with a `STANDALONE_ONLY` / `CLUSTER_ONLY` allowlist. That is
Rust-internal parity only — it says nothing about the Rust↔TypeScript↔Python contract.

**Historical evidence that drift is real, not hypothetical:** Phase S12 fixed a
standalone/cluster wire mismatch (`record` vs `record_id`) that made the Python SDK
throw `KeyError` against cluster nodes. That class of bug is undetectable today
across the Rust→TS boundary.

**Per constraint B, this work must come *after* §9.** Generating contracts from four
divergent `Project` shapes would freeze the divergence into generated code.

---

# 13. Existing capability system and its actual scope

`crates/valori-effect/src/capability.rs` (343 LOC), spec'd in `rfcs/0004`.

```rust
pub trait Capability: Send + Sync + 'static {           // :17
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
}
pub trait KernelCapability:    Capability { ... }       // :33
pub trait EmbedCapability:     Capability { ... }       // :157
pub trait LlmCapability:       Capability { ... }       // :167
pub trait StorageCapability:   Capability { ... }       // :176
pub trait HttpCapability:      Capability { ... }       // :187
pub trait ProofCapability:     Capability { ... }       // :195
pub trait SchedulerCapability: Capability { ... }       // :206
pub struct CapabilityRegistry { ... }                   // :223
pub struct NoOpKernelCapability { ... }                 // :274
```

Real implementations in `valori-node`: `EngineKernelCapability` (standalone),
`RaftKernelCapability` (cluster, A9/A13.1 — 8 additional methods),
`HttpEmbedCapability`, `PassthroughHttpCapability`, plus `CapabilityRegistryBuilder`.

### Actual scope — this is the critical distinction

These are **node-internal subsystem-authorization capabilities**: "is this node
allowed to talk to the kernel / an embedder / an LLM / object storage". They are
checked at task-dispatch time inside `valori-node`.

They are **not** the provider-capability-discovery model the brief's §6 describes.

| §6 requirement | Status |
|---|---|
| `Capability` trait + registry, runtime-queryable | ✅ exists |
| `is_available()` runtime check instead of compile-time gating | ✅ exists |
| `MemoryCapability::{VectorSearch, HybridSearch, Graph, Snapshots, Filtering, DeterministicReplay}` enum | ❌ missing — no such enum anywhere |
| `provider.capabilities().contains(...)` discovery API | ❌ missing |
| Avoidance of `if provider == "name"` conditionals | 🟡 — `ProviderKind::as_str()` + string matching in `ProviderRegistry::build()` is exactly this pattern, though contained to one factory function |

**Assessment.** The capability *mechanism* is real and good. The capability
*discovery* model for pluggable providers does not exist — and per constraint G it
should stay deferred until a second memory provider actually exists.

---

# 14. Existing model-management system

`crates/valori-models` — 3,914 LOC, **19 source files**, zero valori dependencies.

| Module | LOC | Contains |
|---|---|---|
| `package_store.rs` | 612 | Installed-package store (M5/M6) |
| `gc.rs` | 344 | Garbage collection of unused artifacts |
| `lib.rs` | 319 | `ModelManager` (:76) |
| `integrity.rs` | 289 | Checksum / integrity verification |
| `provider/registry.rs` | 282 | `ProviderRegistry`, factory registration |
| `downloader/mod.rs` | 274 | Artifact download |
| `registry/mod.rs` | 262 | Model registry |
| `types.rs` | 215 | `ModelTask`, `ModelFormat`, `ProviderKind`, `ManifestStatus` |
| `resolver.rs` | 201 | Name → manifest resolution |
| `health.rs` | 197 | Provider liveness |
| `storage/mod.rs` | 164 | On-disk layout |
| `manifest.rs` | 162 | Model manifest |
| `verifier/mod.rs` | 157 | Signature/hash verification |
| `provider/{ollama,openai,voyage,dummy}.rs` | 360 | 4 impls |

Phase docs: `phase-M1-M4-model-manager.md`, `phase-M5-M6-package-store.md`.

### Against the brief's §8 model primitives

| Primitive | Status |
|---|---|
| `ModelType` (as `ModelTask`) | ✅ `{Embedding, Generation, Reranker, Vision, Speech}` |
| `Provider` (as `ProviderKind`) | ✅ 8 variants |
| `Quantization` / format (as `ModelFormat`) | 🟡 `{Onnx, Gguf, Safetensors, Remote}` — format yes, quantization no |
| `Checksum`, `Size`, `Artifact` | ✅ `integrity.rs`, `manifest.rs`, `downloader/` |
| `License` | ❌ missing |
| `ModelId` (typed) | ❌ missing — string-keyed |
| `ModelVersion` | ❌ missing |
| `Architecture` | ❌ missing |
| `RuntimeCompatibility` | ❌ missing |
| `ContextLength` | ❌ missing |
| `EmbeddingDimension` | 🟡 `ModelProvider::dim()` — runtime value, not manifest metadata |
| `Capabilities` (per-model) | 🟡 `ModelTask` is a single value, not a set |
| **Separation: metadata / artifact / installed / hosted** | 🟡 `manifest.rs` (metadata) and `package_store.rs` (installed) are separated ✅; "hosted deployment" does not exist ❌ |

**Assessment.** This subsystem is substantially further along than the brief assumes.
It is not consumed by `desktop/src-tauri` (zero Valori deps) — so local model
execution in Studio is wired to nothing yet.

**Consumers today:** `valori-daemon` (its only valori dep), `valori-ingest`,
`valori-node`.

---

# 15. Current Tauri boundary

Covered in §3. Summary:

| Check (hard rule 5) | Verdict |
|---|---|
| Business logic inside `#[tauri::command]` | **None found.** 12 commands, all delegating to `daemon_manager`, `telemetry`, or OS APIs |
| Tauri as thin adapter | ✅ already true |
| Same Rust logic reusable by CLI/worker/tests | 🟡 partially — the logic lives in `valori-daemon`, reachable over HTTP by any client, but desktop reaches it via a spawned subprocess rather than a linked crate |

**The one architectural question here** is whether `desktop/src-tauri` should keep
talking to `valori-daemon` over HTTP (current: spawn + supervise + HTTP) or link it as
a crate. Current design is defensible — it keeps one daemon serving both Tauri and the
Next.js API routes, and matches `control-plane.md`. **Recommend: no change.**

---

# 16. TypeScript business logic that should move behind Rust services

`ui/src/lib/server/` — **2,034 LOC across 17 files**. Migration targets, in priority
order. **None of these are migrated in this stage.**

### Priority 1 — `process-manager.ts` (427 LOC)

Spawns and supervises `valori-node` OS processes for 3-node cluster projects,
including port allocation (4010–4999 range, raft = http + 100) and lifecycle.

- **Duplicates:** `valori-daemon::LocalRuntime` (530 LOC), `runtime/port.rs`
- **Why it still exists:** documented in `control-plane.md` as a deliberate exception —
  the daemon can *persist* cluster topology (`ProjectManifest.cluster`) but cannot
  *launch* a cluster (no Raft-join, no multi-node health aggregation)
- **Rust service boundary it should call:** `valori-daemon` needs a
  `Runtime::start_cluster(project, topology) -> Vec<NodeInfo>` capability plus
  multi-node health aggregation. **Define this trait extension before migrating.**
- **Risk:** High. Path layout is byte-compatible with existing on-disk cluster data.

### Priority 2 — `projects.ts` (436 LOC)

Manifest read/write/migrate, legacy-entry migration (`migrateEntry`), name validation,
port allocation, `projectNodePaths()`.

- **Duplicates:** `valori-daemon/src/project.rs` + `migration/m001_project_registry.rs`
- **Rust service boundary:** already exists — `ProjectStore` trait (`daemon/src/store.rs:14`)
  and `WorkspaceStore` (:31). This file should become a thin client of `daemon.ts`.
- **Blocked by:** §9 canonical Project model
- **Risk:** High — `node_event_log_path()` byte-compatibility comment

### Priority 3 — `embed.ts` (139), `llm.ts` (102), `reranker.ts` (54) — 295 LOC total

Provider dispatch for embedding, completion, and reranking from the Next.js server.

- **Duplicates:** `valori-models::ModelProvider` + `ProviderRegistry` (embed),
  `valori-rag/src/llm.rs` (llm), `valori-search/src/reranker.rs` (rerank)
- **Rust service boundary:** the AI-runtime trait from §6b, extended with `generate`
  and `rerank`. **This trait does not exist yet — define it first (§21 step M4).**
- **Risk:** Medium. These paths have UI fallback behavior (Phase I3 "auto-fallback")
  that must be preserved.

### Priority 4 — supporting files

`project.ts` (127), `project-adapter.ts` (84), `cluster-config.ts` (55),
`connection.ts` (81), `nodeProxy.ts` (58) — resolve naturally once 1–3 land.

### Explicitly **not** migration targets

`extract-text.ts` (80) — PDF/DOCX parsing via `pdf-parse`/`mammoth`; browser-adjacent,
fine in TS. `content-filter.ts`, `app-url.ts`, `mfa.ts`, `http.ts`, `api-client.ts` —
presentation/transport concerns, correctly placed.

---

# 17. OSS vs private Cloud boundary

### Currently OSS (this repo)

Core kernel, storage, state, metadata, consensus, planner, effect/capabilities,
engine, node HTTP surface, CLI, MCP server, verifier, FFI, Python SDK, model
management, daemon (project lifecycle), Tauri shell, **and the entire Next.js app —
including the Cloud product frontend.**

### Currently private (separate repo, per project memory)

Rust control plane, provisioning, workers, scheduler, billing.
**Not present in this repository.**

### The boundary violation

`ui/src/app/cloud/**` — Cloud product pages, org/team management, API keys, service
accounts, IP allowlists, subscriptions, login history — **live in the OSS repository**
and query Supabase directly from Next.js server actions.

Tables reached from OSS code: `projects`, `org_members`, `org_invitations`,
`subscriptions`, `api_keys_public`, `personal_access_tokens_public`,
`service_accounts`, `ip_allowlist_rules`, `login_history`.

`subscriptions` is commercial. Hard rule 8 says commercial infrastructure must not be
exposed in OSS.

**Assessment: 🟡 needs a decision, not an immediate move.**

Two defensible positions:
1. Cloud UI *pages* stay OSS (they're a frontend for a paid service; the service
   itself is private). Cloud *business logic* — currently in server actions — moves to
   the private control plane behind an API.
2. Cloud pages move to the private repo entirely; only shared UI packages stay OSS.

Position 1 is cheaper and consistent with the brief's §14 ("do not share complete
pages" — pages can live in different repos or the same repo, what matters is they're
not shared). **Recommend position 1**, with the constraint that no OSS code may
contain billing/provisioning logic. The migration is: server actions → private Cloud
API calls.

This decision blocks Stage 4 and should be made explicitly by the owner.

### Recommended target boundary

| Layer | Home |
|---|---|
| `valori-core`, `valori-kernel`, storage/state/wire/metadata/planner/effect/index/search/rag/ingest/engine/consensus/verify | OSS |
| `valori-domain` (new — platform IDs + canonical domain models + adapters) | **OSS** |
| `valori-node`, `valori-daemon`, `valori-models`, `valori-cli`, `valori-mcp`, `valori-ffi` | OSS |
| Runtime/provider *interfaces* | OSS |
| `@valori/ui`, `@valori/ui-data`, `@valori/ui-ai`, design tokens | OSS |
| Studio pages (`ui/src/app/**` minus `cloud/`) | OSS |
| Cloud pages | OSS (position 1) — but **zero** billing/provisioning logic |
| Cloud control plane, provisioning, scheduler, billing, hosted inference, `OrganizationId`/`UserId`/`SubscriptionId`/`DeploymentId`/`WorkerId` | **Private** |

---

# 18. Current UI / component architecture

```
ui/src/
├── app/          Studio routes + cloud/ routes + 88 api/ route.ts
├── components/   77 .tsx files in 12 domain folders:
│                 cluster, codegen, collections, graph, home, ingestion,
│                 layout, onboarding, operations, projects, proof, settings, ui
├── lib/          hooks/, server/ (2,034 LOC), embeddings/, valori-client.ts,
│                 theme.tsx, telemetry.ts, receipts.ts, proof.ts, ...
└── types/        valori.ts (67 LOC — the only shared type file)
```

**Primitives already extracted** — `ui/src/components/ui/` (17 files):
`badge`, `button`, `card`, `copy-btn`, `dialog`, `empty-state`, `input`,
`metric-card`, `page-header`, `separator`, `skeleton`, `status-badge`,
`status-panel`, `table`, `tabs`, `textarea`, `toaster`.

Built on `@base-ui/react` + Tailwind 4 + `class-variance-authority` + shadcn
conventions. `@xyflow/react` for graphs, `recharts` for charts.

**Design tokens** — `ui/src/app/globals.css` defines **174 CSS custom properties**
across `.dark` / `.light` blocks: semantic tokens (`--background`, `--foreground`,
`--border`, `--card`, `--muted-foreground`) plus Valori accents (`--v-accent`,
`--v-accent-muted`, `--v-accent-ring`, `--v-heatmap-empty`). Light mode is a
documented hard requirement in `CLAUDE.md`.

### Against the brief's §14 three-package model

| Package | Status |
|---|---|
| `@valori/ui` (primitives) | 🟡 **content exists, packaging does not.** 14 of the 17 files in `components/ui/` are generic primitives. No `packages/` directory; no npm workspace. |
| `@valori/ui-data` (infra components) | 🟡 seeds exist — `metric-card`, `status-badge`, `status-panel`, `table`. `DataTable`, `LogViewer`, `Timeline`, `UsageChart`, `EventList` are spread through domain folders or absent. |
| `@valori/ui-ai` (Valori-specific) | 🟡 seeds exist — `components/graph/`, `components/operations/`, `components/proof/`, `components/codegen/`. Not isolated, not named, coupled to Studio data fetching. |
| Centralized design tokens (§15) | 🟡 tokens exist and are disciplined, but live inside the app's `globals.css`, not a shared package |

**Critical caveat (constraint F).** There is currently **one** Next.js app serving both
products. Extracting three packages requires two consumers. Until Cloud is a separate
app, package extraction creates build complexity for zero decoupling benefit.
**Recommend: last in sequence**, and only once Cloud UI is genuinely separate.

---

# 19. Missing abstractions

Consolidated, with the four-way status and — per hard rule 2 — the **named real
consumer** that justifies each.

### Build (real consumer exists today)

| Abstraction | Status | Real consumer |
|---|---|---|
| `valori-domain` crate (platform IDs + canonical domain models) | ❌ missing | daemon, metadata, node, Cloud, TS UI — 4 divergent `Project`s |
| Canonical `Project` + 4 adapters | ❌ missing | §9 |
| `ProjectId`, `SnapshotId`, `ModelId`, `SessionId`, `InstallationId` newtypes | ❌ missing | §10 raw strings |
| API contract generation (Rust → schema → TS) | ❌ missing | 88 TS routes + Python SDK + S12-class bugs |
| AI-runtime trait with `generate` / `rerank` | 🟡 partial (embed only) | `llm.ts`, `reranker.ts`, Studio local models |
| Cluster launch in `valori-daemon` | ❌ missing | `process-manager.ts` (427 LOC TS) |
| Common event envelope for daemon/telemetry/execution | ❌ missing | 3 unrelated models; **excludes `KernelEvent`** |
| Dependency-direction enforcement test | ❌ missing | hard rule 14; no guard exists |

### Build later (consumer is near but not present)

| Abstraction | Status | Blocked on |
|---|---|---|
| `Pipeline` / `PipelineStep` platform primitives | ❌ missing | needs a second pipeline type beyond ingest |
| `Checkpoint`, `ResourceRequirements`, declarative `RetryPolicy` | ❌ missing | needs long-running/scheduled executions |
| `ExecutionEvent` stream | ❌ missing | needs a UI consuming live execution streams |
| `@valori/ui` / `-data` / `-ai` packages | 🟡 content exists | needs Cloud as a separate app (§18) |
| Model metadata gaps (`ModelVersion`, `RuntimeCompatibility`, `ContextLength`, `License`) | ❌ missing | Studio local model execution |
| Hosted inference runtime | ❌ missing | private Cloud |

### ⏸ Intentionally deferred (constraint G — no second implementation exists)

| Abstraction | Why deferred |
|---|---|
| `MemoryProvider` trait + Qdrant / Pinecone / Weaviate / Milvus / pgvector impls | Exactly one memory implementation exists (Valori). An interface with one implementation is a rename, not an abstraction. |
| `MemoryCapability::*` discovery enum | Only meaningful once ≥2 memory providers exist |
| Connector API (S3/GCS/GitHub/Notion/Postgres/...) | Zero occurrences of "connector" in any crate. No implementation to abstract over. |
| Plugin API + capability model | No plugin loader, no sandbox, no third-party extension exists |
| Marketplace runtime | Commercial + speculative |
| `OrganizationId`, `UserId`, `BillingAccountId`, `SubscriptionId`, `DeploymentId`, `WorkerId` in OSS | Constraint A — Cloud concepts, private repo |

These are **documented future extension points**, not backlog items. Each should be
revisited only when its second implementation is actually being built.

---

# 20. Migration risks

| # | Risk | Severity | Evidence | Mitigation |
|---|---|---|---|---|
| R1 | **On-disk path compatibility.** `daemon::Project::node_event_log_path()` must match `projectNodePaths()` in `projects.ts` byte-for-byte, or existing cluster-project data is silently orphaned | **Critical** | Explicit load-bearing comment at `crates/valori-daemon/src/project.rs` | Any `Project` unification keeps a path-compatibility test as an acceptance gate. Never derive paths from the new domain model without a fixture test against legacy layouts. |
| R2 | **Project primary-key change.** `metadata` and TS key on `name`; daemon has a UUID `id`. Introducing `ProjectId` as the canonical key risks orphaning records | **Critical** | §9 table | Adapters resolve by `name` during a compatibility window; `id` backfilled by the existing `m001_project_registry` migration pattern |
| R3 | **Manifest schema migration.** `m001_project_registry.rs` (390 LOC) already bridges TS→daemon. A second migration layered on a half-migrated fleet | **High** | `crates/valori-daemon/src/migration/` | One migration per release; never two in flight. Test against real legacy manifests. |
| R4 | **Dual-path (standalone/cluster) divergence.** Every node change must land in both `server.rs` (83 routes) and `cluster_server.rs` (77 routes) | **High** | `CLAUDE.md` mandate; `tests/route_parity.rs`; Phase S12/S13 were both caused by missing one path | `route_parity.rs` already guards routes. Extend the same discipline to any new contract. |
| R5 | **Wire-format compatibility.** Snapshot V6, event log V4, `KernelEvent` chain are version-locked by `COMPATIBILITY.md`. A domain-model refactor must not touch them | **Critical** | `COMPATIBILITY.md`, `INVARIANTS.md` | Firewall: `valori-domain` must **not** be a dependency of kernel/wire/storage. Enforced by the dep test (R9). |
| R6 | **`valori-kernel` `no_std`.** Non-negotiable invariant #7 | **Critical** | `CLAUDE.md`; `#![cfg_attr(not(feature="std"), no_std)]` | `valori-domain` is std-only and sits *beside*, never below, the kernel. Keep verifying `cargo build -p valori-kernel --target wasm32-unknown-unknown`. |
| R7 | **Cloud is a separate repo.** Contracts defined here cannot be compile-verified against Cloud | **High** | Cloud control plane absent from this repo | Version the `ApiProject` contract explicitly; treat Cloud as an external consumer with a compat policy, not an in-tree crate |
| R8 | **Supabase schema is not in this repo.** Column names/types unknown to this audit | **Medium** | No `.sql` file exists in the repo | Do not design `ApiProject` around assumed Supabase columns. Get the schema first. |
| R9 | **No dependency-direction enforcement.** `architecture.rs` only checks duplicate files; `deny.toml` has no layer rule | **Medium** | §2.7 | Add the enforcement test **before** adding `valori-domain`, not after |
| R10 | **Stale in-source wiring comments.** `OperationKind`'s "Planned:" comment predates A13/A13.1 | **Medium** | `operation.rs:70-73` vs `phase-A13*.md` | Re-verify actual wiring by reading handlers, not comments, before extending the planner |
| R11 | **UI light-mode requirement.** Every UI change must work in both themes | **Medium** | `CLAUDE.md` UI section | Applies to any token/package extraction in the last stage |
| R12 | **Python SDK + FFI are downstream consumers.** ~40 endpoints, hand-written | **Medium** | `python/valoricore/remote.py`, `valori-ffi` | Include both in the API-contract stage; S10 showed FFI breaks silently on enum changes |

---

# 21. Recommended target architecture

### Guiding decisions

1. **`valori-core` stays kernel-only.** No platform or Cloud concepts.
2. **New `valori-domain` crate** holds platform IDs and canonical domain models. It
   depends on `valori-core`; the kernel never depends on it.
3. **Domain ≠ persistence ≠ API ≠ UI.** Four representations, explicit adapters.
4. **One execution engine.** Everything future builds on
   `Operation → Planner → ExecutionGraph → Executor`.
5. **Three runtimes, three names.** Process runtime (`valori_daemon::Runtime` —
   keep as is), AI/model runtime (extend `ModelProvider`), hosted inference runtime
   (private Cloud, does not exist yet).
6. **Cloud-only IDs never enter OSS.**

### Target dependency graph

```
┌─────────────────────────── OPEN SOURCE ────────────────────────────┐
│                                                                     │
│  valori-core  (no_std, zero deps)                                   │
│   │  RecordId NodeId EdgeId NamespaceId/CollectionId ShardId         │
│   │  ExecutionId ClusterEpoch  ·  NodeKind EdgeKind  ·  Version      │
│   │                                                                  │
│   ├──▶ valori-kernel (no_std) ──────────────────────────────────┐    │
│   │      │  KernelState · RecordPool · graph · V6 snapshot       │    │
│   │      │  Q16.16 fxp · BLAKE3 chain                            │    │
│   │      ├──▶ valori-wire ──▶ valori-storage ──▶ valori-state    │    │
│   │      ├──▶ valori-metadata ──▶ valori-planner ──▶ valori-effect   │
│   │      ├──▶ valori-index · valori-rag · valori-verify          │    │
│   │      └──▶ valori-consensus                                   │    │
│   │                                                              │    │
│   └──▶ valori-domain  ★ NEW ─────────────────────────────────────┘    │
│          │  ProjectId SnapshotId PipelineId ModelId RuntimeId         │
│          │  SessionId InstallationId                                  │
│          │  Project (canonical) · Collection · Snapshot · Model       │
│          │  Adapters: Persistence ↔ Domain ↔ Api                      │
│          │  ✗ never depended on by kernel/wire/storage/state          │
│          │                                                            │
│          ├──▶ valori-models   (AI runtime: embed ✅ +generate +rerank) │
│          │      └──▶ valori-ingest                                    │
│          │                                                            │
│          ├──▶ valori-daemon   (process Runtime · ProjectStore ·       │
│          │      │              WorkspaceStore · EventStore ·          │
│          │      │              + cluster launch ★)                    │
│          │      └──▶ desktop/src-tauri  (HTTP; thin adapter ✅)        │
│          │                                                            │
│          └──▶ valori-engine ──▶ valori-node ──▶ valori-cli            │
│                                    ├──▶ valori-ffi ──▶ python SDK     │
│                                    └──▶ valori-mcp                    │
│                                                                       │
│  API contract:  valori-domain + node API types                        │
│                        ↓ schema generation                            │
│                   OpenAPI / JSON Schema  ← single source of truth      │
│                        ↓                    ↓                          │
│                 generated TS client   Python SDK types                │
│                                                                       │
│  UI (later):  @valori/ui → @valori/ui-data → @valori/ui-ai            │
│               + design tokens package                                 │
└───────────────────────────────────────────────────────────────────────┘

┌────────────────────────── PRIVATE CLOUD (separate repo) ──────────────┐
│  valori-cloud-*  ──depends on──▶  valori-domain, valori-node (client)  │
│    OrganizationId UserId BillingAccountId SubscriptionId               │
│    DeploymentId WorkerId                                               │
│    provisioning · scheduler · billing · hosted inference · marketplace  │
│    ✗ never depended on by any OSS crate                                │
└────────────────────────────────────────────────────────────────────────┘

Applications:
   Studio:  Next.js → Tauri → valori-daemon → valori-node → engine/planner/kernel
   Cloud:   Next.js → Cloud API (private) → valori-node → engine/planner/kernel
   CLI:     valori-cli → valori-node
   Python:  SDK → HTTP → valori-node   |   embedded → valori-ffi → kernel
```

### Forbidden edges (to be enforced by test, §M0)

```
valori-kernel   → valori-domain        ✗
valori-kernel   → anything Cloud       ✗
valori-core     → valori-domain        ✗
valori-wire/storage/state → valori-domain  ✗   (protects wire compat, R5)
any OSS crate   → valori-cloud-*       ✗
valori-domain   → valori-node/daemon   ✗   (domain must stay a leaf-ward crate)
Studio          → Cloud internals      ✗
Cloud           → Studio internals     ✗
```

---

# 22. Recommended migration sequence

Nine steps. **Every step compiles and passes `cargo test -p valori-kernel -p valori-node`
before the next begins.** No step is a rewrite; each introduces an interface, adapts the
existing implementation behind it, migrates consumers, then removes the duplicate.

| Step | Name | Depends on | Risk | Why here |
|---|---|---|---|---|
| **M0** | **Dependency-direction enforcement test** | — | Low | Must exist *before* `valori-domain` is added, or the first violation ships unnoticed (R9). Extends `crates/valori-node/tests/architecture.rs` with a `Cargo.toml`-graph assertion covering the forbidden-edge list above. Also add the OSS/Cloud-ID ban. |
| **M1** | **`valori-domain` crate — IDs only** | M0 | Low | New crate, `valori-core` dep, no consumers switched. Ships `ProjectId`, `SnapshotId`, `PipelineId`, `ModelId`, `RuntimeId`, `SessionId`, `InstallationId` as serializable newtypes with documented contracts (§23 of brief). Purely additive — nothing can break. |
| **M2** | **Canonical `Project` + adapters** | M1 | **High** | The priority per constraint B. Adds `valori_domain::Project` + `From`/`TryFrom` adapters for daemon-persistence, metadata, and API shapes. Existing structs stay; adapters are added around them. Acceptance gates: path-compatibility fixture test (R1), legacy-manifest migration test (R3), name→id resolution window (R2). Get the Supabase schema before writing `ApiProject` (R8). |
| **M3** | **Migrate `Project` consumers, delete duplicates** | M2 | **High** | daemon and metadata read/write through adapters; `ui/src/lib/server/projects.ts` becomes a thin `daemon.ts` client. Removes duplicate #3. Cloud adapter published as a versioned contract, not consumed in-tree (R7). |
| **M4** | **AI-runtime trait: `generate` + `rerank`** | M1 | Medium | Extends `valori-models`' provider layer (embed already ✅) so the three TS dispatchers have a Rust target. **Does not touch `valori_daemon::Runtime`** (constraint D). Naming: `ProcessRuntime` stays `Runtime` in daemon; the model side keeps `ModelProvider`/`ModelRuntime`; hosted inference is not named yet because it doesn't exist. |
| **M5** | **API contract generation** | M3 | Medium | Only now — after the domain model is canonical (constraint B). Pick one tool (`utoipa` → OpenAPI, or `ts-rs`), generate from `valori-domain` + node API types, replace hand-written TS interfaces. Must cover both routers (R4) and the Python SDK/FFI (R12). |
| **M6** | **Daemon cluster launch; retire `process-manager.ts`** | M3, M4 | **High** | Extends `Runtime` with cluster start/stop + multi-node health aggregation, closing the documented `control-plane.md` exception. Then migrate `embed.ts`/`llm.ts`/`reranker.ts` onto M4's trait, preserving Phase-I3 fallback behavior. Removes ~1,150 LOC of TS. |
| **M7** | **Common event envelope** | M1 | Low | Unifies daemon `Event`, desktop telemetry, and execution records behind the existing `EventStore` seam, carrying `ProjectId`/`SessionId`/`ExecutionId`. **`KernelEvent` is explicitly excluded** — touching the audit chain breaks determinism (§11, R5). |
| **M8** | **Shared UI packages** | Cloud separated | Low | Last, per constraint F. `@valori/ui` (14 primitives already exist) → `@valori/ui-data` → `@valori/ui-ai` → design-token package. **No redesign.** Pages and information architecture stay product-specific. Light-mode parity is an acceptance gate (R11). |

### Explicitly out of this sequence

- `MemoryProvider` + Qdrant/Pinecone — ⏸ until a second memory implementation is being built
- Connector API — ⏸ until ≥2 connectors are being built
- Plugin API, marketplace runtime — ⏸ until a plugin loader exists
- `Pipeline`/`PipelineStep`/`Checkpoint`/`ResourceRequirements` — ⏸ until a second
  pipeline workload exists beyond ingest
- Splitting `valori-node` (18.5k LOC) — separate initiative, not this refactor
- Any change to snapshot V6, event log V4, or `KernelEvent` — forbidden by R5

### Suggested first cut

**M0 → M1 → M2.** M0 and M1 are low-risk and purely additive. M2 is where the real
decisions land (primary key, path compatibility, Supabase schema) and should be
reviewed as a design before implementation, not after.

---

## Appendix — audit method

Every claim traces to one of:
- `Cargo.toml` `[dependencies]` sections, parsed programmatically (dev-deps separated)
- Source files read directly (paths and line numbers cited inline)
- `grep` over `crates/`, `desktop/`, `ui/src` for named types and traits
- `wc -l` for size figures
- Route counts from `.route(` occurrences in `server.rs` / `cluster_server.rs`, and
  `find ui/src/app/api -name route.ts | wc -l`

Absence claims (`utoipa`, `schemars`, `ts-rs`, `typeshare`, "connector", `MemoryProvider`,
`packages/`, `*.sql`, `ARCHITECTURE_AUDIT.md`) were verified by repository-wide search
returning zero results.

RFC documents were read for intent but **not** counted as evidence of implementation.
