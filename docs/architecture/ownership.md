# Valori — concept ownership registry

**Status:** Normative. This document and [`layers.md`](layers.md) together form the
architecture constitution.

`layers.md` answers *"which crate may depend on which"*. This page answers the
question that has to be settled first:

> **Which layer owns this concept?**

Ask it before adding a type, a table, an endpoint or a React prop. Most of the
duplication catalogued in [`ARCHITECTURE_AUDIT.md`](../../ARCHITECTURE_AUDIT.md)
— four `Project`s, three embedding dispatchers, four event models — happened
because that question had no written answer.

---

## How to use this page

1. Find the concept in the table. If it is there, its owner is the **only** place
   a new field, variant or rule for it may be added.
2. If it is not there, decide its owner using the [admission rules](#admission-rules)
   below, then **add a row in the same change**.
3. Never add a second representation of an owned concept without an adapter and
   a row explaining why the adapter exists.

---

## The registry

| Concept | Owner | OSS / Private | Persistence | API | UI |
|---|---|---|---|---|---|
| **Project** | `valori-domain` | OSS | daemon `project.json`, metadata redb, Cloud table — via adapters | `ApiProject` | Studio + Cloud |
| **ProjectId / SnapshotId / ModelId / SessionId / InstallationId** | `valori-domain` | OSS | embedded in each persistence model | transparent strings | both |
| **Collection / Namespace** | `valori-core` (`NamespaceId`), `valori-kernel` (state) | OSS | kernel snapshot + NSRG | node HTTP | both |
| **Record, Node, Edge, Shard, ClusterEpoch** | `valori-core` + `valori-kernel` | OSS | kernel snapshot V6 | node HTTP | both |
| **Snapshot (artifact)** | `valori-storage` (`object_store`) | OSS | S3 / B2 / file object keys | node HTTP | both |
| **Event log / audit chain** | `valori-wire` (format), `valori-storage` (IO) | OSS | `events.log` V4 | `/v1/proof/*`, `/v1/timeline` | both |
| **Model (metadata, artifact, installed)** | `valori-models` | OSS | model package store on disk | daemon + node HTTP | both |
| **Process runtime** (where a node process runs) | `valori-daemon` (`Runtime`) | OSS | none — runtime state | daemon `/v1/config`, `/v1/projects/*` | Studio |
| **AI / model runtime** (embed, generate, rerank) | `valori-models` (provider layer) | OSS | none | node embed/ingest endpoints | both |
| **Hosted inference runtime** | Cloud control plane | **Private** | Cloud DB | Cloud API | Cloud |
| **Operation / Planner / ExecutionGraph / Executor** | `valori-planner` + `valori-effect` | OSS | `MetadataDb` execution + planner cache | `/v1/operations/*` | both |
| **Execution status / history** | `valori-planner` (`ExecutionStatus`), `valori-metadata` (`ExecutionRecord`) | OSS | metadata redb | node HTTP | both |
| **Receipt / proof** | `valori-effect` (`receipt.rs`) | OSS | receipt store | `/v1/proof/receipt` | both |
| **Workspace** (project grouping) | `valori-daemon` | OSS | daemon store | daemon HTTP | Studio |
| **Daemon lifecycle events** | `valori-daemon` (`EventStore`) | OSS | in-memory ring today | daemon `/v1/events` | Studio |
| **Telemetry events** | `desktop/src-tauri/telemetry.rs` | OSS emitter, **private** sink | `events.jsonl` queue | Cloud telemetry ingest | — |
| **Organization / User / Team / Invitation** | Cloud control plane | **Private** | Supabase | Cloud API | Cloud |
| **Billing / Subscription** | Cloud control plane | **Private** | Cloud DB | Cloud API | Cloud |
| **Deployment / Worker / Scheduler** | Cloud control plane | **Private** | Cloud DB | Cloud API | Cloud |
| **API keys / service accounts / IP allowlist** | Cloud control plane | **Private** | Supabase | Cloud API | Cloud |
| **Design tokens / UI primitives** | `ui/` today, `@valori/ui*` after M8 | OSS | — | — | both |
| **Product pages / information architecture** | each application | Studio OSS, Cloud pages OSS with no commercial logic | — | — | separate per product |

---

## Admission rules

### When a concept belongs in `valori-domain`

All three must hold:

1. It is **already represented in two or more systems**, incompatibly.
2. It is meaningful **without Cloud** — a user running Valori offline still has one.
3. It is stable enough that changing it would be a compatibility event, not a refactor.

One consumer means the concept belongs to that consumer's crate. Zero consumers
means it is not built.

### When a concept belongs to Cloud (private)

Any one of these is sufficient:

- It presupposes an account, an organization, or a tenant.
- It is commercial (billing, subscription, quota enforcement, marketplace listing).
- It describes hosted infrastructure (deployments, GPU workers, schedulers).

These may **not** appear in `valori-core`, `valori-kernel` or `valori-domain`.
`crates/valori-node/tests/dependency_direction.rs` fails the build if
`OrganizationId`, `UserId`, `BillingAccountId`, `SubscriptionId`, `DeploymentId`
or `WorkerId` is defined in the OSS platform core.

### When a concept stays in the kernel

If the deterministic core needs it to replay an event log and produce a
byte-identical state hash, it belongs to `valori-core` / `valori-kernel` and
must stay `no_std`. Product vocabulary must never influence those bytes.

---

## Domain ≠ persistence ≠ API ≠ UI

One meaning, four representations. Never one struct with four sets of serde
attributes.

```text
                     Domain model          "what it means"
                          │
      ┌───────────────────┼───────────────────┬──────────────────┐
      ▼                   ▼                   ▼                  ▼
 Persistence model   Control-plane model   API model         UI model
 "how it is stored"  "how it is indexed"   "how it travels"  "how it is shown"
```

A field belongs in the domain model only if **every** representation needs it.
A restart policy, a port allocation and a filesystem path each belong to exactly
one representation, and putting them in the domain model would force the other
three to carry meaningless nulls.

Worked example — `Project`, implemented in step M2:

| Field | Domain | daemon persistence | metadata | API |
|---|---|---|---|---|
| `id` | ✅ owns | ✅ | ✗ (keyed on name — the M3 gap) | ✅ |
| `name`, `dim`, `index`, topology | ✅ owns | ✅ | ✅ | ✅ |
| `dir` / path | ✗ → `LocalProject` | ✅ | ✅ | ✗ never |
| `port`, `nodes[]` | ✗ runtime | ✅ | ✅ | ✗ never |
| `workspace`, `restart_policy` | ✗ | ✅ only | ✗ | ✗ |
| `record_count` | ✅ owns | ✗ | ✅ | ✅ |

---

## Identity is not location, storage, or display

This distinction becomes load-bearing the moment local projects, Cloud projects,
sync and migration coexist:

```text
ProjectId                    ← the identity. Stable forever.
  ├── LocalProject  { project, root: PathBuf }              OSS
  └── CloudProject  { project, organization_id, region, … }  PRIVATE
```

- The **filesystem path is not the identity** — a project can be moved or restored elsewhere.
- The **database row is not the identity** — the same project exists in several stores.
- The **display name is definitely not the identity** — names are mutable and non-unique.

`CloudProject` deliberately lives in the private Cloud repository, because it
composes `OrganizationId` and `DeploymentId`, which may not appear in OSS. Both
types share `project.id`, and that shared id is exactly what makes local↔cloud
correlation expressible later.

---

## There is one execution engine

Valori already has a universal execution model. **Do not build a second one.**

```text
        User-facing work  (ingest, RAG, sync, inference, eval, scheduled jobs)
                    │
                    ▼
              Operation            valori-planner — content-addressed, cacheable
                    │
                    ▼
               Planner             plans + 2-layer cache (in-process → MetadataDb)
                    │
                    ▼
           ExecutionGraph          DAG of TaskSpec, Kahn topo-sort, GraphHash
                    │
                    ▼
              Executor             TaskRunner — retry, predecessor threading
                    │
                    ▼
      EffectBus → CapabilityRegistry → kernel / raft / embed / llm / storage
```

A future `Pipeline` is **a way of authoring `Operation`s**, not a parallel
runtime. It compiles down to this graph.

Forbidden, by name: `PipelineEngine`, `WorkflowEngine`, `JobEngine`,
`TaskEngine`, or any scheduler that owns its own DAG, retry policy and state
machine. If the existing planner cannot express a workload, extend
`OperationKind` and `TaskKind` — that is what they are for.

Primitives still missing from this model (`Pipeline`, `PipelineStep`,
`Checkpoint`, `RetryPolicy` as a declarative type, `ResourceRequirements`,
`ExecutionEvent`) are **deferred, not rejected**. Each is added to
`valori-planner` when a second workload needs it.

---

## There are three runtimes, and they keep separate names

Conflating these is the most likely naming mistake in the next year of work.

| Runtime | Question it answers | Owner | Status |
|---|---|---|---|
| **Process runtime** | Where does a `valori-node` OS process run? | `valori_daemon::Runtime` | ✅ exists — `LocalRuntime`; Docker/SSH pluggable. **Keep this name.** |
| **AI / model runtime** | How is an embed / generate / rerank fulfilled? | `valori_models` provider layer | 🟡 embed only — `generate` and `rerank` arrive in M4 |
| **Hosted inference runtime** | Which GPU serves this request, and who pays? | Cloud control plane | ❌ does not exist; **private** when it does |

`valori_daemon::Runtime` must not be renamed to make room for the AI runtime.
The AI runtime gets capability-shaped traits of its own.

## Provider traits are split by capability, not merged

When M4 lands, the AI runtime is **not** one omnibus trait. An embedding-only
provider must not have to stub `transcribe`.

```text
EmbeddingProvider   ← exists today as ModelProvider (embed, dim, health)
InferenceProvider   ← generate
RerankerProvider    ← rerank
VisionProvider      ⏸ deferred
SpeechProvider      ⏸ deferred
```

Capability **discovery** sits above them, so callers ask what a provider can do
rather than which vendor it is. Provider-name conditionals
(`if provider == "ollama"`) are forbidden outside the one factory function that
constructs providers.

---

## Deferred extension points

Documented so nobody builds them speculatively, and so nobody re-derives the
reasoning. Each is unblocked by a **concrete second implementation**, not by a
roadmap date.

| Extension point | Unblocked when |
|---|---|
| `MemoryProvider` + `MemoryCapability` discovery | A second memory backend (Qdrant, pgvector, …) is actually being built |
| Connector API (S3, GitHub, Notion, Postgres, …) | Two connectors exist and their shapes can be compared |
| Plugin API + sandbox | A plugin loader exists and a third-party extension needs it |
| Marketplace runtime | Commercial requirement is real; **private** |
| `Pipeline` / `Checkpoint` / `ResourceRequirements` | A second workload needs them — built on the planner above |
| `RuntimeId`, `PipelineId` | The things they would identify become individually addressable |

Until then these are **not** in the backlog, not stubbed, and not abstracted
over. An interface with one implementation is a rename, not an abstraction.

---

## Enforcement

| Rule | Enforced by |
|---|---|
| Dependency direction, sealed crates, domain firewall, no OSS→Cloud, no Cloud IDs in OSS core | `crates/valori-node/tests/dependency_direction.rs` |
| No duplicate source files across extracted crates | `crates/valori-node/tests/architecture.rs` |
| Standalone / cluster route parity | `crates/valori-node/tests/route_parity.rs` |
| `Project` wire compatibility | `crates/valori-domain/tests/project_contract.rs`, `crates/valori-daemon/src/domain_adapter.rs` tests |
| ID wire compatibility | `crates/valori-domain/tests/wire_compat.rs` |
| Kernel portability | `cargo build -p valori-kernel --target wasm32-unknown-unknown` |

A rule in this document that is not in that table is an aspiration. Prefer
making it mechanical.
