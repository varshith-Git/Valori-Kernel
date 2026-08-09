# Phase M0–M2 — Platform contracts: dependency enforcement, `valori-domain`, canonical `Project`

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** [`ARCHITECTURE_AUDIT.md`](../../ARCHITECTURE_AUDIT.md) (Stage 1)
**Status:** M0, M1, M2, M2.5 complete, plus the **M2.1 repair** (findings F1–F7
from [`docs/reviews/m2-project-review.md`](../reviews/m2-project-review.md)).
**Stopped before M3 by explicit instruction** — no existing `Project`
implementation has been deleted or migrated.

> **M2.1 addendum.** Post-M2 review found that `#[serde(transparent)]` bypassed
> every validated newtype's constructor (F1), that `ProjectName` was stricter
> than the daemon and would have made existing projects vanish from listings
> (F2), that legacy manifests minted a fresh `ProjectId` on every read (F3), and
> four silent-corruption paths in the adapters (F4–F7). All are fixed; +37 tests;
> combined suite 552 passing. Full detail in the review's "M2.1 outcome" section.

---

## Goal

Establish the smallest set of structural contracts that stop the next years of
Valori development from creating more duplicated implementations: make the
dependency graph mechanically enforceable (M0), give the platform a curated
shared vocabulary (M1), and settle what a `Project` *is* with explicit adapters
rather than a merged struct (M2).

---

## Delivered

### M0 — dependency-direction enforcement

**`crates/valori-node/tests/dependency_direction.rs`** (new, 6 tests). Parses
every `crates/*/Cargo.toml`, separates shipped from dev dependencies, and
enforces:

| Test | Rule |
|---|---|
| `parser_sees_the_workspace` | ≥20 crates found; 7 known edges present; the two intentional dev-only back-edges stay dev-only. Guards against a broken parser making everything else vacuously pass. |
| `shipped_dependency_graph_is_acyclic` | Hard rule 14 |
| `sealed_crates_depend_only_on_their_allowlist` | `valori-core` → nothing; `valori-kernel` → `valori-core`; `valori-domain` → `valori-core` |
| `determinism_crates_cannot_reach_valori_domain` | kernel, wire, storage, state, index, rag, verify must not reach `valori-domain`, **transitively**; failure prints the full path |
| `no_oss_crate_depends_on_cloud` | No workspace crate may depend on `valori-cloud-*` |
| `cloud_only_concepts_are_not_defined_in_oss_platform_core` | `OrganizationId`, `UserId`, `BillingAccountId`, `SubscriptionId`, `DeploymentId`, `WorkerId` may not be *defined* in core/kernel/domain (mentions in docs are fine) |

Dev-dependencies are excluded deliberately and the reason is documented in the
module header: `valori-state → valori-verify` and `valori-verify → valori-node`
are intentional test-only back-edges.

Runs in CI already — `ci.yml` executes `cargo test -p valori-kernel -p valori-node`.

### M1 — `valori-domain`

**`crates/valori-domain/`** (new crate; `valori-core` is its only workspace
dependency). Registered in workspace `members`, `default-members` and
`[workspace.dependencies]`.

| File | Contents |
|---|---|
| `src/id.rs` | `ProjectId`, `SessionId`, `InstallationId` (UUID); `ModelId` (slug); `SnapshotId` (opaque handle). Re-exports `CollectionId`, `NamespaceId`, `ExecutionId` from `valori-core` |
| `src/error.rs` | `DomainError` (8 variants), `Result<T>` |
| `src/validate.rs` | `validating_deserialize!` — routes `Deserialize` through each newtype's `parse()` (M2.1/F1) |
| `src/project.rs` | M2 — see below |
| `tests/wire_compat.rs` | 14 tests |
| `README.md` | Admission rule, deferred list, firewall rules |

**Shipped 5 of the 7 IDs in the approved plan. Two were deliberately not built:**

| Deferred | Why |
|---|---|
| `RuntimeId` | No runtime identity exists. `valori_daemon::Runtime` is keyed by `kind() -> &'static str`, has one implementor, and addresses nodes by project name. |
| `PipelineId` | No `Pipeline` platform primitive exists. `valori_ingest::PipelineConfig` / `PipelineResult` are ingest-local and never addressed by id. |

Both are documented in `id.rs` with the condition that unblocks them. This is
the "don't make it a dumping ground" rule applied literally.

**Representation is mixed, on purpose** — three shapes because they identify
three kinds of thing:

- UUID for ids Valori mints (`ProjectId`, `SessionId`, `InstallationId`)
- Slug for `ModelId` (`openai/text-embedding-3-small` is what users type; forcing
  a UUID would break every `ModelManifest.id` on disk)
- Opaque handle for `SnapshotId` (wraps a key owned by `valori-storage`, which is
  behind the domain firewall and cannot depend on this crate)

Every ID is `#[serde(transparent)]`, so it replaces today's raw `String` fields
without changing a byte of any existing manifest.

### M2 — canonical `Project` + adapters

**`crates/valori-domain/src/project.rs`** — `Project`, `ProjectName`,
`IndexKind`, `ProjectTopology`, `Timestamp`, `LocalProject`, `ApiProject`.

**`crates/valori-daemon/src/domain_adapter.rs`** (new, 8 tests) —
`manifest_to_domain`, `manifest_from_domain`, `ProjectAdapterError`.

**`crates/valori-metadata/src/domain_adapter.rs`** (new, 6 tests) —
`record_to_domain`, `record_from_domain`, `index_to_domain`, `index_from_domain`.

**`crates/valori-domain/tests/project_contract.rs`** — 16 tests pinning the API
wire shape and the identity semantics.

### M2.5 — ownership registry

**`docs/architecture/ownership.md`** (new) — the concept→owner table, the
admission rules, the domain/persistence/API/UI separation, the identity rule,
the single-execution-engine rule (with `PipelineEngine`/`WorkflowEngine`/
`JobEngine`/`TaskEngine` forbidden by name), the three-runtimes table, the
split-provider-trait direction for M4, and the deferred extension points.

---

## The seven M2 deliverables

### 1. Canonical `Project` model

```rust
pub struct Project {
    pub id: ProjectId,               // the identity — never changes
    pub name: ProjectName,           // validated, filesystem-safe, mutable
    pub dim: u32,
    pub index: IndexKind,
    pub topology: ProjectTopology,   // { replicas: NonZeroU8, shards: NonZeroU8 }
    pub created_at: Timestamp,       // unix seconds
    pub last_opened_at: Option<Timestamp>,
    pub record_count: Option<u64>,
}
```

Design decisions worth reviewing:

- **`ProjectTopology` replaces three spellings.** `cluster: Option<ClusterConfig>`
  (daemon), `node_count` + `mode` (metadata), `replication: 1|3` (TypeScript)
  collapse into `{ replicas, shards }`. `is_cluster()` is **derived**, so `mode`
  and `node_count` can no longer disagree. `NonZeroU8` makes zero replicas
  unrepresentable.
- **`replicas` is not constrained to 1 or 3.** The TypeScript union is, but
  RFC-0007 is not; constraining the domain model would make a legitimate 5-node
  cluster inexpressible. That is a wizard concern.
- **`ProjectName` is validated** against the **daemon's** contract (M2.1/F2 —
  it originally copied the stricter `projects.ts::isValidName` rule, which would
  have made existing `_scratch` / `-tmp` / 64-character projects unrepresentable
  and silently absent from listings). The stricter UI rule is a separate
  creation policy, `check_new_project_policy()`. Either way the character rule
  keeps `/`, `\` and `.` unrepresentable — this is a path-traversal guard, not
  formatting.
- **`Timestamp` is unix seconds**, matching two of the three Rust models. The
  legacy TypeScript ISO encoding is a property of that persistence format and
  is converted by its adapter — keeping this crate free of a date dependency.

### 2. Ownership decision — the identity question, answered

**`ProjectId` is the logical Valori identity. Nothing else is.**

```text
ProjectId
  ├── LocalProject { project, root: PathBuf }               OSS  (shipped)
  └── CloudProject { project, organization_id, region, … }  PRIVATE (not here)
```

- The **filesystem path is not the identity** — `root` lives on `LocalProject`,
  never on `Project`. A moved or restored directory is the same project.
- The **database row is not the identity** — the same project exists in
  `project.json`, in redb, and in a Cloud row.
- The **display name is not the identity** — mutable and non-unique across
  workspaces.

`CloudProject` is deliberately **not** in this repository: it composes
`OrganizationId` and `DeploymentId`, which `dependency_direction.rs` forbids in
OSS. Both types share `project.id` — which is what makes local↔cloud sync
expressible later without either side importing the other.

Three tests pin this: `moving_a_project_does_not_change_its_identity`,
`renaming_a_project_does_not_change_its_identity`,
`two_projects_may_share_a_name_but_never_an_identity`.

### 3. Adapters

Four boundaries, four representations, explicit conversions. No merged struct.

| Boundary | Adapter | Direction | Notes |
|---|---|---|---|
| daemon persistence | `valori_daemon::domain_adapter` | `manifest_to_domain` / `manifest_from_domain` | **Not** a `From` impl: a domain project lacks `workspace`, `restart_policy`, `embedding`, `storage`, so construction from scratch would silently default them. `manifest_from_domain` mutates an existing manifest instead. |
| control plane | `valori_metadata::domain_adapter` | `record_to_domain(record, id)` / `record_from_domain` | **Requires the caller to supply `ProjectId`** — the record has no id. Minting one internally would produce a different identity on every read. |
| HTTP API | `ApiProject` in `valori-domain` | `From<&Project>` / `TryFrom<ApiProject>` | Distinct type so the domain model can evolve without silently changing a public API |
| TypeScript UI | *documented mapping only* | — | Generation is M5; see the mapping table below |

The TS mapping M5 will generate from `ApiProject`:

| `ApiProject` | TS | Note |
|---|---|---|
| `id: ProjectId` | `id: string` | UUID |
| `name: ProjectName` | `name: string` | |
| `dim: u32` | `dim: number` | |
| `index: IndexKind` | `index: "brute"\|"hnsw"\|"ivf"\|"bq"\|"auto"` | matches today's union |
| `replicas: u8` | `replicas: number` | replaces `replication: 1\|3` |
| `shards: u8` | `shards: number` | replaces `shardCount` |
| `is_cluster: bool` | `isCluster: boolean` | derived; no client branching |
| `created_at: u64` | `createdAt: number` | **unix seconds, not ISO** |
| `last_opened_at: Option<u64>` | `lastOpenedAt?: number` | omitted, never null |
| `record_count: Option<u64>` | `recordCount?: number` | |

### 4. Migration plan (M3 — not executed)

1. Daemon resolves `name → ProjectId` and exposes it on its project routes.
2. `valori-metadata::Project` gains an `id` field, backfilled through the
   existing `m001_project_registry` migration pattern; `name` stays a unique
   secondary key for a compatibility window.
3. `valori-metadata` adopts `valori_domain::IndexKind`; its local enum and
   `index_to_domain`/`index_from_domain` are deleted together.
4. Node/daemon HTTP responses switch to `ApiProject`.
5. `ui/src/lib/server/projects.ts` becomes a thin `daemon.ts` client; the legacy
   ISO-timestamp manifest reader is retired.
6. Only then are the duplicate `Project` definitions removed.

Each step compiles and passes tests on its own. Deletion is last.

### 5. Compatibility impact

**Zero.** No existing file format, wire format or call site changed.

| Surface | Impact |
|---|---|
| `project.json` | Unchanged — no field added, removed or retyped |
| redb control-plane schema | Unchanged |
| Snapshot V6, event log V4, BLAKE3 chain | Untouched; the domain firewall prevents this crate from reaching them |
| HTTP API | Unchanged — `ApiProject` exists but no handler serves it yet |
| Python SDK / FFI | Unchanged |
| `valori-kernel` `no_std` / wasm | Verified: `cargo build -p valori-kernel --target wasm32-unknown-unknown` passes |
| New dependency edges | `valori-daemon → valori-domain`, `valori-metadata → valori-domain`. Both verified acyclic and outside the firewall. |

The `#[serde(transparent)]` decision is what makes this zero-impact: every new
ID has the same JSON representation as the `String` it will eventually replace.

### 6. Tests

| Suite | Tests | What it pins |
|---|---|---|
| `valori-node::dependency_direction` | 6 | Architecture rules; **verified to fail** on injected violations |
| `valori-domain::wire_compat` | 14 | ID transparency, round-trips, rejection paths, nominal typing |
| `valori-domain::project_contract` | 16 | `ApiProject` JSON shape, name validation incl. path traversal, topology legality, identity semantics |
| `valori-daemon::domain_adapter` | 8 | Round-trip preserves daemon-only fields and port allocations; malformed id/index/topology rejected not coerced |
| `valori-metadata::domain_adapter` | 6 | Identity supplied by caller; `mode` repaired from topology; index enums cannot drift |
| **Total new** | **50** | |

Full run across `valori-kernel`, `valori-node`, `valori-domain`, `valori-daemon`,
`valori-metadata`: **515 passed, 0 failed**.

`cargo fmt --all -- --check` clean. `cargo clippy --workspace --all-targets`
produces no new warnings.

**M0 was verified negatively**, which matters more than it passing: each rule
was broken on purpose and observed to fail, including transitive detection —
`valori-state → valori-storage → valori-wire → valori-domain` was reported as a
full path. The scratch crates were then removed.

### 7. Dependency graph after M2

```text
valori-core ─────────────────────────── (no_std, zero deps)
  │
  ├─▶ valori-kernel ─────────────────── (no_std; core only)
  │     ├─▶ valori-wire ─▶ valori-storage ─▶ valori-state
  │     ├─▶ valori-metadata ─▶ valori-planner ─▶ valori-effect
  │     ├─▶ valori-index · valori-rag · valori-verify
  │     └─▶ valori-consensus
  │
  └─▶ valori-domain  ★ NEW (std) ────── (core only; sealed)
        ├─▶ valori-daemon      ★ new edge
        └─▶ valori-metadata    ★ new edge

valori-engine ─▶ valori-node ─▶ valori-cli · valori-ffi · valori-mcp
```

Firewall verified: no path exists from `valori-kernel`, `valori-wire`,
`valori-storage`, `valori-state`, `valori-index`, `valori-rag` or
`valori-verify` to `valori-domain`.

---

## Findings

1. **The audit's §9 table had one error.** It recorded the daemon manifest as
   having no shard count. `ClusterConfig.shard_count` exists — nested inside the
   optional cluster block, as a `u32` where metadata uses `u8` and TypeScript
   uses `number`. A fourth divergence, not a missing field. The adapter rejects
   values above 255 rather than truncating.

2. **The TypeScript manifest stores a raw API key.** `ProjectEmbedConfig.apiKey`
   in `projects.ts` holds the key itself, while `daemon::EmbeddingConfig` stores
   `api_key_ref` — a reference. The daemon already got this right. This is why
   `embedding` was excluded from the canonical `Project`: a shared model that can
   carry a secret needs a secrets decision first. Worth treating as a security
   item independent of this refactor.

3. **`metadata::Project` can already contradict itself.** `mode` and `node_count`
   are stored independently and nothing keeps them consistent. Deriving
   `is_cluster()` from `replicas` removes the possibility; the adapter test
   `mode_is_recomputed_and_cannot_contradict_node_count` demonstrates the repair.

4. **`valori-daemon` had no `valori-core` dependency.** Its only workspace
   dependency was `valori-models`, which is why it re-invented `Project` rather
   than reusing anything. The new `valori-domain` edge is its first shared-
   vocabulary dependency.

5. **The workspace had no dependency-direction enforcement at all.**
   `architecture.rs` compares file paths only; `deny.toml` has no layer rule.
   Every architectural boundary in `layers.md` and RFC-0005 was documentation.

6. **Pre-existing CI failures from commit `e44b814`, unrelated to this work.**
   Both were found while validating and are **not** fixed here — they are
   outside this phase's scope, and touching them would mix unrelated churn into
   an architecture change.
   - **Clippy:** `crates/valori-node/tests/api_object_store.rs:118` uses
     `len() > 0`. CI runs `cargo clippy --workspace --all-targets -- -D warnings`,
     so this fails the build on its own. One-line fix: `!…is_empty()`.
   - **Formatting:** five files are not `rustfmt`-clean —
     `valori-node/src/server.rs`, `valori-node/src/cluster_server.rs`,
     `valori-node/tests/api_object_store.rs`,
     `valori-node/tests/dr_disaster_recovery.rs`,
     `valori-storage/src/object_store.rs`. A `cargo fmt --all` during this phase
     reformatted them; that was reverted so this change stays surgical, leaving
     them exactly as `e44b814` wrote them. Every file added or edited by this
     phase **is** fmt-clean.

---

## Validation

```bash
cargo test -p valori-kernel -p valori-node -p valori-domain -p valori-daemon -p valori-metadata
#  515 passed, 0 failed

cargo test -p valori-node --test dependency_direction   # 6 passed
cargo test -p valori-node --test route_parity           # 2 passed
cargo build -p valori-kernel --target wasm32-unknown-unknown   # ok
cargo fmt --all -- --check                              # clean
```

Manual smoke test — no runtime behaviour changed, so there is nothing to smoke
test at the process level. The adapters are pure functions with no I/O and are
not yet called by any handler. That is the intended M2 end state.

---

## Follow-ups

| Item | Owner phase |
|---|---|
| Migrate `Project` consumers; delete the duplicate definitions | **M3** — needs review before starting |
| Backfill `ProjectId` into the control-plane record | M3 |
| `valori-metadata` adopts `valori_domain::IndexKind`; delete its enum | M3 |
| Split provider traits: `InferenceProvider` (generate), `RerankerProvider` (rerank) | M4 |
| API contract generation from `ApiProject` | M5 |
| Retire `process-manager.ts`, `embed.ts`, `llm.ts`, `reranker.ts` | M6 |
| Common event envelope (excluding `KernelEvent`) | M7 |
| `@valori/ui` / `-data` / `-ai` packages | M8 |
| Secrets decision for `embedding` config before it enters any shared model | M3 or earlier |
| Fix pre-existing `api_object_store.rs:118` clippy warning | unrelated; blocks CI |
