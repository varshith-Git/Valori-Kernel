# G2.0 — Project & Collection Domain Model: Design Audit

**DESIGN ONLY. No source code, migrations, schemas, APIs, UI, tests, or
configuration were modified.** This document builds directly on
[project-collection-lifecycle-audit.md](project-collection-lifecycle-audit.md)
(the factual current-state audit) and
[graph-g1.4.3-cluster-index-capability-audit.md](graph-g1.4.3-cluster-index-capability-audit.md)
(the index-capability audit) — both re-verified, not re-derived from
scratch, per instruction. Every claim carrying a fresh citation was
independently re-checked against current source; claims reused from the
prior audits are marked as such rather than re-cited line-by-line.

---

## 1. Executive summary

Valori's current architecture already draws the boundary this document
ultimately recommends keeping: **one deployed node process is one
vector-configuration domain** (one dimension, one metric, one active
index), and "Collection" beneath it is a lightweight logical partition
(a `NamespaceId`), not an independently-configured vector space. This is
not a limitation to design away — it is the direct, structural
consequence of `KernelState`'s canonical fields (`dim: Option<usize>`,
one `ActiveIndex`) being scalars, not per-namespace maps, and of Cloud's
own provisioning model (one Project → its own dedicated node/cluster).

The central recommendation (§2, §18) is **Model A — project-level vector
configuration, collections share it** — chosen on evidence, not
convention: it matches what the canonical state already structurally
enforces, it avoids multiplying the G1.4.3-documented HNSW recovery-time
problem by collection count, it matches what the shipped Python SDK
already tells users, and every competitor surveyed (§16) that supports
one-config-per-index still makes that config **immutable** after
creation — the industry consensus is not "let each logical partition pick
its own embedding space," it's "pick your embedding space once per
physical index." Valori's Project (which owns exactly one physical
node's worth of state) is the correct level for that decision, not
Collection.

**Verdict: NOT READY FOR IMPLEMENTATION** (§20) — this document resolves
the Model A/B question and most of the immutability matrix with high
confidence, but four decisions remain genuinely open and are listed
explicitly rather than guessed at: the Cloud/Kernel collection-registry
ownership model (§12 Option A/B/C/D), whether collection deletion should
be soft or hard at the kernel level (no precedent exists — namespaces are
currently never deleted in practice, only *droppable* by name), the exact
shape of a future per-collection description/status field (real product
need vs. speculative surface), and the collection-level index-type
override question flagged in §2 as a deliberately incomplete edge of
Model A.

---

## 2. Current architecture (re-verified)

Re-checked directly against source (not trusted from the prior audit
alone) immediately before writing this document — confirmed unchanged:

```rust
// crates/valori-engine/src/engine.rs:64,120,129,136 (re-verified)
pub struct Engine {
    pub dim: usize,                        // ONE value, whole process
    pub index_kind: IndexKind,              // ONE value, whole process
    pub dim: usize,                         // (KernelState mirror, below)
    pub namespaces: CollectionRegistry,      // name -> NamespaceId map only
    ...
}
```
```rust
// crates/valori-kernel/src/state/kernel.rs:566-571 (re-verified)
KernelEvent::AutoCreateNamespace { name: _ } => {
    // The name is not stored in KernelState — namespaces are pure integer ids here.
    ...
}
```

No drift since the prior audit — same commit last touched these files
(`c592d1a`, predates this session's G-phase work; this session's edits to
`engine.rs` in G1.3.1 removed `record_to_node` and changed delete-cascade
logic, neither of which touches `dim`/`index_kind`/`namespaces`).

**Traced chains, both re-confirmed**:

```
Cloud:  Organization → Project (Postgres row) → Provisioning (Rust backend,
        valori-ui/backend/apps/api) → Node instance(s) (Docker container(s),
        tracked in infra.instances, a SEPARATE Postgres schema from
        Supabase's projects table)

Data plane:  Project [Cloud identity only — invisible below this line]
             → Node (one Engine, one dim, one index)
             → Collection/Namespace (NamespaceId(u16), name only)
             → Records/Vectors (RecordPool, canonical, Q16.16 fixed-point)
             → GraphNode/GraphEdge (NodePool/EdgePool, canonical,
                optional 0..N per record)
             → Vector indexes (derived, ActiveIndex/Box<dyn VectorIndex>,
                one per node) / graph traversal (pure functions over
                canonical state, no persisted index at all)
```

**Canonical / persisted / derived / runtime-only / dormant classification
for every load-bearing symbol touched by this design**, re-confirmed:

| Symbol | Classification | Evidence |
|---|---|---|
| `KernelState.dim: Option<usize>` | Canonical (in snapshot; only indirectly in BLAKE3 hash via vector-length bytes, per the G1.4.3-adjacent audit finding) | `kernel.rs:21`; `snapshot/encode.rs:69,97,282` |
| `KernelState.records/nodes/edges` | Canonical | established G0/G0.1/G0.2, unchanged |
| `Engine.namespaces: CollectionRegistry` | **Persisted but non-canonical** — a JSON sidecar, not the snapshot/event-log/WAL | `Engine::flush_namespaces`, `engine.rs:421-438` |
| `KernelEvent::AutoCreateNamespace{name}` | Canonical **event**, but the kernel-level apply is a near-no-op that discards `name` — only the integer id allocation is canonical | `kernel.rs:566-571` |
| `Engine.index: Box<dyn VectorIndex>` / `KernelState.index: ActiveIndex` | Derived | established, re-confirmed unchanged this pass |
| `valori-metadata::Collection`/`CollectionRegistry`/`MetadataDb` | **Dormant** — coded, tested, never opened by any production binary | re-confirmed via `valori-metadata/src/lib.rs:10-32`'s own in-code statement, unchanged |
| Cloud `projects.dim`/`projects.index_type` | Cloud-control-plane-only, Postgres, immutable in practice (no UPDATE path found) | prior audit §3, re-confirmed no new migration exists |

---

## 3. Current Project model

Restated precisely from the prior audit (§2 there), not re-derived: a
Project is **a Postgres row in Supabase that is the
identity/authorization/billing anchor for zero-or-more provisioned node
instances**, tracked in a *second*, separate Postgres schema
(`infra.instances`, owned by the Rust backend, a different service than
the one Supabase's RLS governs). The two are linked only by an ID and an
eventually-consistent PATCH-back (`mark_project_active`), not a
transaction or foreign key across databases.

---

## 4. Current Collection model

Restated precisely: **Collection = `NamespaceId(u16)` + a name**, held in
`Engine.namespaces: CollectionRegistry`, persisted only to a JSON
sidecar file outside the canonical snapshot/event-log. No dimension,
metric, index, description, status, timestamps, or record count exists
on it anywhere live (a richer, dormant struct exists in `valori-metadata`
but is never opened — §2 above, re-confirmed).

---

## 5. Current limitations (evidence-based, not assumed)

1. **Collections cannot have independent vector semantics** — not a bug,
   a direct consequence of `dim`/`index` being scalar fields on `Engine`/
   `KernelState`. Any redesign that wants per-collection dimension/metric
   must change these from scalars to per-namespace maps, which touches
   the canonical snapshot format (§14) — not a config-layer change.
2. **Collection name is not canonical** — a Raft-replicated cluster
   commits the *id allocation* through consensus but the *name* only
   through each node's own JSON sidecar (`flush_namespaces`), meaning a
   name could theoretically desync across replicas in a way the BLAKE3
   state hash would never catch (the hash never covers the name). **Not
   independently re-verified in this pass whether this has ever caused an
   observed bug** — flagged as a structural risk, not a confirmed
   incident.
3. **No collection-level lifecycle state** — a collection cannot be
   "creating," "ready," or "deleting" today; creation is synchronous and
   there is no delete-in-progress state (namespaces are droppable by
   name, `DropNamespace` event, but this session's audits did not trace
   what "drop" does to the records still inside it — flagged as an open
   question in §20, not assumed either way).
4. **No Cloud-side visibility into a project's actual collections** —
   confirmed in the prior audit: no schema link exists; Cloud would have
   to query the live node over HTTP to know what collections exist.
5. **The one already-attempted migration toward richer collections
   (`valori-metadata::Collection`) was never finished** — it sat dormant
   long enough that its own crate doc now states this as a known,
   accepted fact, not a work-in-progress.

---

## 6. Project ownership model

Evaluating exactly which of the prompt's candidate boundaries Project
should own, against evidence (not assumption):

| Candidate boundary | Should Project own it? | Evidence |
|---|---|---|
| Billing boundary | **Yes** | Cloud's `subscriptions` table is per-`org_id`, but per-project usage is tracked (`project_usage_snapshots`, prior audit §4) — billing *attribution* is project-level even though the *plan* itself is org-level |
| Authorization boundary | **Yes** | `api_keys.project_id` FK, `verify_api_key()` resolves the project from the key alone — confirmed the primary authz unit today |
| Deployment boundary | **Yes** | `infra.instances.project_id` — confirmed, this is literally what "provisioning" produces |
| Database boundary | **Partially** — Project maps to one node's canonical state, but that state lives in the node's own storage (event log/snapshot files or Raft-replicated redb), not in a Cloud-visible database at all | prior audit §2/§5 |
| Compute boundary | **Yes** | `replication` field, `infra.instances` rows — a project's compute footprint is 1..N node instances |
| Vector configuration boundary | **Yes — this is the central recommendation of §2/§7** | `projects.dim`/`index_type` already exist as Cloud-declared config passed to the node at provisioning time; the node's own `Engine`/`KernelState` scalars structurally enforce this is a whole-node property |
| Isolation boundary | **Yes, at the deployment level** — each project gets its own dedicated node(s), never shared with another project's data | confirmed by the provisioning trace (prior audit §2) |
| Collection container | **Yes** | trivially — collections only exist inside a running node, and a node belongs to exactly one project |
| API-key boundary | **Yes** | `api_keys.project_id` FK, confirmed |
| Resource-management boundary | **Yes** | `max_records`, replication, plan-derived limits all resolve at the project/org level |

**What Project should NOT own**: per-collection metadata (name,
description, per-collection lifecycle state) — these are data-plane
concerns that live inside the node, not Cloud metadata about the node.
Project also should not own *individual record/vector data* — that stays
entirely inside the node's canonical state, consistent with the
project's role as "identity + compute boundary," not "data store."

**Formal definition**:

> **Project = the Cloud-side authorization, billing, and deployment
> identity for a dedicated compute allocation (1..N replicated node
> instances) sharing one vector-configuration domain (dimension, metric,
> default index) and containing zero-or-more Collections.**

---

## 7. Collection ownership model

Should Collection become a real first-class resource beyond `NamespaceId
+ name`? **Yes, but narrowly** — evaluated field by field, not by
defaulting to "add everything a real resource might have":

| Field | Classification | Reasoning |
|---|---|---|
| `id` | CANONICAL | already is (`NamespaceId`) |
| `name` | PERSISTED BUT NON-CANONICAL, should become canonical | currently a sidecar-only mapping (§5.2) — this is the one field this document recommends *upgrading* to canonical, closing the desync risk in §5 |
| `project_id` | Not needed inside the kernel at all — a collection's project is implicit (it's whatever project owns the node it lives in); redundant to store per-collection | the kernel has no Project concept and should not gain one (preserves the canonical/derived boundary — Project is a Cloud concept, not kernel data) |
| `dimension` | NOT per-collection (Model A, §2) — remains node-level | see §2/§9 |
| `metric` | NOT per-collection — remains node-level | see §2/§9 |
| `index_type` | NOT per-collection in the general case; see §9's flagged open edge for whether a *type-only* override should exist | see §9 |
| `index_config` (M/ef/nlist/nprobe) | NOT per-collection — node-level, since index_type itself is node-level | follows from index_type |
| `created_at` | PERSISTED BUT NON-CANONICAL — genuinely useful (already exists on the dormant `valori-metadata::Collection`), no reason to hash it | low-risk, real product value (sortable collection lists) |
| `updated_at` | RUNTIME ONLY / not needed — nothing about a collection's own row changes after creation in the proposed model (name becomes canonical-immutable, dimension/index are node-level) | no mutable collection-level field exists to justify tracking this |
| `status` | RUNTIME ONLY, if a lifecycle is adopted (§10); DERIVED otherwise (computable from "does this namespace have any live nodes/edges/records + is a drop in progress") | see §10 — do not add speculatively if no real transition needs it |
| `record_count` | DERIVED, computed on demand from the record pool's per-namespace linked list, never stored | already trivially derivable — CLAUDE.md's own kernel invariants make this a cheap live query, storing it duplicately risks drift |
| `storage_size` | DERIVED, computable but expensive (would require summing serialized bytes) — CLOUD CONTROL-PLANE ONLY if surfaced at all, likely approximated, not exact | not a kernel canonical concern |
| `description` | CLOUD CONTROL-PLANE ONLY or PERSISTED-NON-CANONICAL, genuinely optional — flagged as an open product question in §20, not decided here (no code evidence either way about whether users need this) | no current evidence of demand; do not add speculatively |
| `metadata` (arbitrary key/value on the collection itself, distinct from per-record metadata which already exists) | NOT NEEDED — no evidence of demand, and `Record.metadata` already covers the per-item case | avoid speculative surface per rule 5 |
| `embedding model` | NOT per-collection in Model A (tied to Project's single dimension/metric, same embedding-model-per-project assumption embedded in `VALORI_EMBED_MODEL` today, which is a node-level env var) | see §6/§9 |
| `distance metric` | Duplicate of `metric` above — NOT per-collection |
| `quantization` | Tied to index_type (BQ specifically) — NOT per-collection, follows index_type |
| `replication` | NOT a collection concept at all — replication is a Project/deployment property (§6), collections inside a replicated project are replicated as a side effect of the whole node being replicated | no evidence collections need independent replication factors |
| `retention` | NOT NEEDED today — no retention mechanism exists anywhere in the current codebase (confirmed by absence, not by an explicit negative search in this pass — flagged as unconfirmed-absence, treat as "not established from current code" rather than a hard claim) | speculative; do not add without a real requirement |
| `schema` (a payload/metadata schema constraint) | NOT NEEDED — Valori's `Record.metadata` is already schemaless JSON (`metadata_filter`'s Phase I7 design assumes arbitrary JSON), and no evidence of demand for enforcement | avoid speculative surface |
| `graph configuration` | See §13 — collections should NOT own a separate "graph config" field; graph presence is a property of what's actually inserted, not a declared collection setting |

**Formal definition**:

> **Collection = a named, canonical partition (`NamespaceId`) of a
> Project's shared vector-configuration domain, scoping which Records,
> GraphNodes, and GraphEdges belong together for isolation, search, and
> traversal purposes — owning its own identity and name, but no
> independent dimension, metric, index, or embedding-model
> configuration.**

---

## 8. Immutable vs. mutable matrix (the core deliverable)

| Property | Owner | Immutable? | Can change online? | Requires rebuild? | Requires migration? | Requires restart? | Why |
|---|---|---|---|---|---|---|---|
| `project_id` | Cloud | Yes | No | No | No | No | Identity, PK |
| `organization` (`org_id`) | Cloud | Yes (no UPDATE path exists today) | No | No | No | No | Ownership transfer is a distinct, unimplemented feature, not a config edit |
| `name` (project) | Cloud | **No** | **Yes** | No | No | No | Confirmed live UPDATE path exists (prior audit §3); pure display/identity string, no semantic weight |
| `slug` (project) | Cloud | Effectively yes today (no UPDATE path found) | No | No | No | No | Used in routing/URLs; changing it would be a real feature (redirects) not implemented |
| `region` | Cloud | Yes | No | No | Yes (would mean re-provisioning in a new region) | N/A (whole redeploy) | Region determines physical host; "changing" it is really "migrate," a distinct operation |
| `plan` | Cloud (org-level, not project) | No — genuinely mutable, that's the point of a subscription | Yes | No | No | No | Billing-tier change, no data-plane impact at all |
| `replication` | Cloud/deployment | Effectively yes today (no UPDATE path found), but **should become mutable** in the target model (scale 1→3 replicas) | Should be: yes, additive (adding replicas) | No (new replicas bootstrap via snapshot+replay) | No | No (rolling) | Adding replicas is a pure infrastructure operation; canonical state doesn't change |
| `dimension` (project-level, Model A) | Project | **Yes — hard, semantic** | No | N/A (not a rebuild, a re-embed) | Yes (see §11) | Yes (kernel dim locks on first insert, in-process) | Changing dimension makes every existing vector meaningless — not a rebuild question, a data-validity question |
| `metric` (project-level, Model A) | Project | **Yes — hard, semantic** | No | N/A (see above) | Yes | Yes | Same reasoning — an L2-trained space and a cosine-trained space are not the same data |
| `default index` (project-level) | Project | **No — soft, implementation-only** | Standalone: yes, via `/v1/index/rebuild` (already exists); Cluster: not currently, per G1.4.3 | **Yes**, always | No | No (standalone); N/A (cluster has no equivalent yet) | Index is derived from canonical vectors — changing it never invalidates data, only costs a rebuild |
| `limits` (`max_records` etc.) | Cloud | Effectively yes today (no UPDATE path found), no reason it must remain so | Should be: yes | No | No | No | Pure quota, no data-plane semantic weight |
| `deployment topology` (host/provider) | Cloud | De facto yes (single provisioner active at a time, per prior audit §5) | No | No | Yes | Yes | Changing providers means re-provisioning, not editing |
| `collection_id` | Kernel | Yes | No | No | No | No | Identity |
| `name` (collection) | Kernel | **No — should become mutable AND canonical** (§7) | Yes, once canonical (would need a new `KernelEvent::RenameNamespace` or similar — not proposed as an implementation here, just as a property classification) | No | Yes (canonical format addition, §11) | No | Purely a label; renaming has zero effect on stored vectors/graph |
| `namespace_id` | Kernel | Yes | No | No | No | No | Identity |
| `dimension` (if ever made per-collection, Model B) | N/A in the recommended model | would be Yes, same reasoning as project-level dim | N/A | N/A | N/A | N/A | Not recommended (§2) |
| `metric` (per-collection) | N/A in the recommended model | same as above | N/A | N/A | N/A | N/A | Not recommended |
| `index_type` (per-collection) | N/A in the recommended model, EXCEPT the flagged open edge (§9) | Would be soft/derived if ever added | Conditionally | Yes | Possibly | No | Genuinely open question, not resolved here |
| HNSW `M` | Node (Model A) | **No — soft/derived config** | Only via full rebuild (no incremental M-change exists in the code, per G1.4.3) | **Yes, always** | No | No | Structural graph parameter — changing it without rebuilding produces an inconsistent graph, but doesn't invalidate canonical vectors |
| HNSW `ef_construct` | Node | Soft/derived, same as `M` | Same | Yes | No | No | Same reasoning |
| HNSW `ef_search` | Node | **No — pure runtime/search-time knob** | **Yes, immediately**, no rebuild | **No** | No | No | Confirmed a search-time-only parameter (G1.4.3 §7 audit), doesn't touch the built graph at all |
| IVF `n_list` | Node | Soft/derived | Only via rebuild | Yes | No | No | Determines centroid count — a structural build parameter |
| IVF `n_probe` | Node | Pure runtime/search-time knob | Yes, immediately | No | No | No | Confirmed search-time-only (probes an already-built index), G1.4.3 |
| BQ configuration (`pool_factor`/`min_candidates`) | Node | Pure runtime/search-time knob | Yes, immediately | No | No | No | Confirmed search-time candidate-pool sizing only, G1.4.3 |
| embedding model | Project (implicit, via `VALORI_EMBED_MODEL`) | **Yes — hard, semantic**, same class as dimension/metric | No | N/A | Yes (re-embed) | No (env var, but changing it mid-life is a data-validity issue, not a restart issue) | A collection's vectors are only meaningful relative to the model that produced them |
| metadata schema | N/A — not adopted (§7) | — | — | — | — | — | Not adding this field |
| graph settings | N/A — not adopted as a collection field (§13) | — | — | — | — | — | Graph presence is emergent from usage, not declared config |

**The three-way distinction the prompt asked for, applied precisely**:

- **IMMUTABLE** (changing invalidates existing canonical data): dimension,
  metric, embedding model. These three travel together — they define
  what a vector *means*, and changing any one silently corrupts every
  vector inserted under the old value without any way for the system to
  detect the corruption after the fact (nothing in the canonical state
  records "which model produced this vector").
- **MUTABLE** (changing has zero effect on canonical data validity):
  project name, plan, limits, replication factor (additive), collection
  name (once made canonical).
- **DERIVED CONFIG** (changing requires rebuilding a derived index, never
  rewriting canonical vectors): index type, HNSW `M`/`ef_construct`, IVF
  `n_list`. A strict sub-category exists within this — **pure search-time
  knobs** (`ef_search`, `n_probe`, BQ pool sizing) that need **no rebuild
  at all**, confirmed by the G1.4.3 audit's own configuration table.

---

## 9. Vector semantics

**Dimension, metric, and embedding model are fundamentally tied to the
vector itself — confirmed by direct reasoning about what a vector *is*,
not by convention**: a vector is only meaningful as "the output of
embedding model X, compared under metric Y, in Z-dimensional space." Any
one of these three changing means every previously-stored vector is now
being compared against a differently-shaped or differently-meaning query
vector. This is not a performance concern (like index type) — it's a
**correctness** concern: search results become silently, undetectably
wrong, not merely slower.

**Can 384D-model-X, 768D-model-Y, and 1536D-model-Z vectors coexist
inside one Collection?** **No, structurally cannot today** (single
`dim: Option<usize>` locks on first insert — a 768D insert into a
384D-locked kernel is rejected outright, `DimensionMismatch`, confirmed
in the prior audit §7). Even if the kernel *could* accept mixed
dimensions (it categorically cannot), doing so would still be a product
error — L2 distance between vectors from different embedding spaces is
meaningless regardless of whether the byte lengths happen to match.

**Can different metrics coexist inside one Collection?** No — there is
only ever one active index per node (`Engine.index`), computing one
metric (L2 today, confirmed the only implemented metric anywhere, prior
audit §8) against every vector in every namespace on that node uniformly.

**Direct answers to the prompt's specific question — "if a Collection
contains 1M vectors, can the user change dimension / metric / embedding
model?"**:

- **Dimension: No.** Attempting it either (a) fails outright (kernel
  rejects the mismatched insert) or (b) if somehow forced past validation,
  silently corrupts search — there is no code path that makes this safe.
- **Metric: No**, for the same structural reason (one process-wide index).
- **Embedding model: No**, for the semantic reason above — even though
  nothing in the kernel *enforces* this (the kernel has no concept of
  "embedding model" at all — it only sees float arrays), doing so
  produces silently wrong search results with zero detection mechanism.

**If yes, what happens? If no, why?** Answered above — the "if no, why"
is the load-bearing case for all three. This directly informs §11.

---

## 10. Index semantics — vector semantics vs. search implementation

Cleanly separated, per the prompt's own framing, and confirmed by the
G1.4.3 audit's independent findings:

**Semantic properties (immutable)**: dimension, metric, embedding model.

**Search implementation choices (mutable/rebuildable)**: BruteForce,
HNSW, IVF, BQ, and every one of their sub-parameters.

**Transition-by-transition analysis** (BruteForce↔HNSW↔IVF↔BQ, and
parameter changes within one index type), against the ten questions
posed:

| Transition | Canonical data changes? | Snapshot format changes? | Event log changes? | BLAKE3 hash changes? | Index rebuilds? | Reads continue during rebuild? | Old index kept until new ready? | Crash-during-rebuild behavior | Restart recovery | Cluster handling |
|---|---|---|---|---|---|---|---|---|---|---|
| BruteForce→HNSW (standalone) | No | No | No | No | Yes, always | **No** — `/v1/index/rebuild` takes a full write lock for the duration (confirmed, G1.4.3 §8) | **No** — discarded in place, then rebuilt (confirmed, G1.4.3 §4/§8: "immediately discards the current index ... and rebuilds") | Next boot rebuilds from scratch again — no resumability found (G1.4.3 §9) | `try_recover()`'s event-log branch always calls `rebuild_index()` unconditionally (G1.4.3 §2/§4) — full rebuild every restart regardless of this transition ever having happened | **Not applicable today** — cluster has no non-BruteForce kernel index at all (G1.4.3 §4); the transition itself cannot be attempted on cluster |
| HNSW→IVF / IVF→BQ / any pair (standalone) | No | No | No | No | Yes, always | No, same as above | No, same as above | Same as above | Same as above | Same as above |
| HNSW `M`/`ef_construct` change | No | No | No | No | Yes, always (structural build parameter) | No | No | Same | Same | N/A |
| HNSW `ef_search` change | No | No | No | No | **No** — search-time only | **Yes**, trivially (no write lock needed at all) | N/A — nothing to keep, no rebuild happens | N/A | N/A | N/A (if HNSW existed cluster-side) |
| IVF `n_list` change | No | No | No | No | Yes, always | No | No | Same | Same | N/A |
| IVF `n_probe` change | No | No | No | No | **No** — search-time only | Yes, trivially | N/A | N/A | N/A | N/A |
| BQ `pool_factor`/`min_candidates` | No | No | No | No | **No** — search-time only | Yes, trivially | N/A | N/A | N/A | N/A |

**The one property that should hold and currently does hold**: *changing
the derived index never changes canonical vector state* — confirmed true
in every transition above, by construction (index construction reads
only vectors from `RecordPool`, never mutates it — G1.4.3 §8's
independent confirmation of the same fact for HNSW specifically applies
identically to every index type, since all four implement the same
`VectorIndex::rebuild(pool: &RecordPool)`-shaped trait, read-only against
the pool).

**Should users be able to perform every transition listed above?**
**Yes, for standalone** (already possible via `/v1/index/rebuild`, which
this document does not propose changing). **Not yet meaningfully
possible for cluster** (no non-BruteForce kernel index exists — this is
the G1.4.3 audit's own Option C recommendation, not re-litigated here).

---

## 11. Design the lifecycle

**Project.** The current seven-state machine (`creating, active, stopped,
error, deleted, archived, suspended`, prior audit §4) is **kept almost
as-is** — evidence does not support removing any of these; each has a
real, distinct trigger and consequence already traced in the prior audit.
**One addition is justified by this document's own findings**: no
`region`-migration or `dimension`-change state exists because those
operations don't exist yet (§8) — if either is ever implemented, it
would need its own transient state (e.g. `migrating`), but **this is not
proposed for implementation now**, only flagged as the natural extension
point if §20's open questions resolve toward supporting project
migration.

```
Project lifecycle (kept from current, no removals justified by evidence):

  creating → active → { stopped ↔ active, suspended → active,
                         active/stopped → archived → stopped }
  any → deleted (soft; row survives, per prior audit §4)
```

**Collection.** The prompt's example (`CREATING → READY → UPDATING_INDEX
→ READY → DELETING → DELETED`) is **evaluated, not blindly adopted**.
Evidence-based trim:

- `CREATING`/`READY` split: **not justified today** — collection creation
  is synchronous, sub-millisecond (a `HashMap` insert + one canonical
  event), per §2/§6.1 of the prior audit. There is no meaningful "still
  creating" window to represent. **Do not add these two states.**
- `UPDATING_INDEX`: **not justified as a *collection*-level state** —
  index rebuild is a **node-level** operation (§2/§10), affecting every
  collection on that node simultaneously, not one collection
  independently. A per-collection `UPDATING_INDEX` state would misrepresent
  reality (implying other collections are unaffected, when they are not,
  under Model A). If surfaced at all, this belongs on the **node/Project**
  resource, not Collection.
- `DELETING`/`DELETED`: **justified, conditionally** — only if collection
  deletion is ever made asynchronous (e.g., large namespaces requiring
  bulk record cleanup). Today's `DropNamespace` behavior on the record
  contents inside a dropped namespace was **not traced in this pass**
  (flagged in §5.3 and §20 as an open question) — until that's confirmed,
  this document cannot responsibly assert whether an async delete state
  is even meaningful.

**Recommended collection lifecycle, evidence-trimmed**:

```
  (create — synchronous, no transient state)
        ↓
      ACTIVE   ← the only steady state that exists today, renamed from
                  implicit-and-unnamed to explicit for clarity
        ↓
  (drop — synchronous today; if made async later, ADD exactly
   one transient state, DELETING, at that time — not now)
        ↓
      gone (no tombstone row — matches today's behavior, where a
            dropped namespace's id is not tracked as "was here")
```

This is deliberately minimal — the prompt's own instruction ("don't add
states just because they sound useful") is followed literally here: zero
new collection states are recommended without a traced, real transition
to justify them.

---

## 12. Collection creation — future API contract (conceptual, not implemented)

**What must be specified at creation?** Only `name` — exactly what's
required today (`CreateCollectionRequest{name}`, prior audit §6.3). Under
Model A, there is nothing else *to* specify — dimension/metric/index are
already fixed at the project level before any collection exists.

**What should have safe defaults?** N/A under Model A — there's nothing
left over after `name` that needs a default.

**What should never be configurable through the UI?** `namespace_id`
(system-allocated, sequential, never user-chosen — already true today).

**What should be API-only vs. Cloud-project configuration?**
`name` — collection-level, API/SDK. `dimension`/`metric`/`index` —
**Cloud-project configuration**, set once at project provisioning time
(already the case — `CreateProjectDialog.tsx`'s `dim`/`index_type`
fields, prior audit §2), never at the collection-creation API layer at
all. This is a direct consequence of the recommended Model A boundary,
not a new design choice — it's the current reality, formalized as the
recommended contract rather than an implicit one.

**Conceptual creation contract** (illustrative only, not proposed as a
literal implementation):
```
POST /v1/namespaces
{ "name": "documents" }
→ { "name": "documents", "id": 3, "created": true }
```
**Deliberately identical in shape to today's actual request/response**
(prior audit §6.3) — the evidence does not support adding fields here.

---

## 13. Existing data migration (per the prompt's five scenarios)

For a Collection with 1M vectors, 384D, HNSW, distinguishing the six
listed response categories precisely — **not blurred together**:

| User wants | Response | Why |
|---|---|---|
| 768 dimensions instead of 384 | **D — create a new collection** (never A/B/C/E as the *only* step; F possible as an interim UX affordance, see below) | Dimension is IMMUTABLE (§8) — there is no "in-place" option. A brand-new collection (new `NamespaceId`, same or different name) with the new dimension is the only structurally valid target. Re-embedding the existing 1M vectors (C) is a *client-side, application-level* operation that happens to populate the new collection — Valori itself performs D (accept inserts into a fresh namespace); C is what the user's own pipeline does *before* those inserts, not something Valori performs on their behalf. |
| cosine instead of L2 | **D**, same reasoning | Metric is IMMUTABLE for the same structural/semantic reason as dimension — no code path anywhere implements in-place metric conversion, nor could one produce meaningful results (cosine and L2 rankings are not a deterministic function of each other for arbitrary vector sets) |
| IVF instead of HNSW | **B — rebuild index** (already possible today, standalone, via `/v1/index/rebuild`, confirmed G1.4.3) | Index type is DERIVED CONFIG, not semantic — canonical vectors are untouched, only the search structure changes |
| Migrate 384D/HNSW data into a new 768D/cosine/IVF collection, keeping both available during the transition | **E + F combined** — create the new collection (E), optionally keep the old one live and queryable (F) while the client re-embeds and re-inserts into the new one, then the client (not Valori) decides when to cut over and drop the old collection | This is the realistic product flow for a "dimension migration" — Valori's job is exactly (D): make creating the new collection cheap and safe; the re-embedding (C, client-side) and cutover timing are the user's application's responsibility, not the vector database's |
| Reject any of the above outright | **A — applies only to attempting an in-place dimension/metric change**, e.g. a hypothetical `PATCH /v1/namespaces/:name {dimension: 768}` — this document recommends such an endpoint **never be built**, i.e. reject-by-non-existence, not reject-by-runtime-error | Building an endpoint that always 400s is worse UX than not building it; the correct "rejection" is that the API surface simply doesn't offer the invalid operation |

**Never proposed**: an endpoint that silently reinterprets a
dimension/metric change as "wipe and rebuild the same collection in
place" — this would silently discard 1M vectors' worth of user data with
a config PATCH, the single most dangerous possible API shape given
everything established in §9.

---

## 14. Snapshot / recovery implications (tied directly to G0→G1.4)

For every property this document has discussed, classified against
where it belongs, using the established canonical/derived vocabulary:

| Property | Should live in |
|---|---|
| Project dimension/metric (Model A) | **Cloud Postgres** (`projects.dim`/`.index_type`, already there) as the *declared intent*, **plus** `KernelState.dim` (already canonical) as the *locked, enforced reality* — these are two representations of the same fact by design (declaration vs. enforcement), not a duplication to eliminate |
| Default index type/config | **Node startup config** (env vars, as today) — NOT canonical, NOT in the snapshot's k_data section, correctly already living only in the optional `i_data` section when persisted (G1.4.3 §5) |
| Collection name | **Should move from "separate metadata file" (JSON sidecar, today) to canonical snapshot** — this is the one concrete "move something" recommendation in this document (not implemented here), because it closes the desync risk in §5.2 and because it's cheap: names are small, bounded (64 chars, ≤1024 namespaces), and the kernel already has the exact right place for it — a namespace-name section analogous to the existing NSRG section pattern used for `CollectionRegistry` serialization at the *engine* level today. **This would be a new snapshot format field — a real, if small, migration (§16), not free.** |
| Collection lifecycle state | **RUNTIME ONLY / derived** (§11's minimal lifecycle needs no persisted state — "active" is just "the id is allocated," "gone" is just "the id is not") — nothing to persist beyond what already exists |
| `CollectionRegistry` itself | Stays exactly where it is (`Engine`, JSON sidecar for the id↔name direction) **except** the name should additionally become canonical per the point above — the sidecar can remain as a fast-lookup cache rebuilt from canonical state at startup, the same pattern already used for `record_to_node`-style caches before G1.3.1 removed the unsafe version of that pattern |

**Nothing in this design moves index-specific bytes into the canonical
snapshot, event log, or BLAKE3 hash** — every derived-index property
discussed in §8/§10 stays exactly where G0.2's "hash semantic state, not
reconstruction topology" principle already places it: outside the
commitment surface, in the optional, explicitly-separate `i_data`
section (standalone) or not persisted at all (cluster, per G1.4.3's
finding that this is currently harmless only because cluster indexes are
stateless-on-search).

---

## 15. Restart performance — architectural constraints only (no implementation)

Directly connecting to the [recovery-hnsw-startup-breakdown.md](recovery-hnsw-startup-breakdown.md)
findings (this session): that document established index rebuild
dominates recovery time (~98.8% of measured recovery-path time at 10K
vectors/384D) and that the persisted-index (`i_data`) fast path already
exists but is structurally bypassed whenever an event log is configured.

**What Collection configuration should allow us to optimize later**:
under Model A, there is exactly **one** index to rebuild per node
restart, regardless of how many collections exist — this is a direct,
positive consequence of rejecting Model B (§2): a redesign that gave each
collection its own index would multiply this already-serious cost by
collection count, which no evidence in this document or the prior
performance audit supports as acceptable.

**Architectural constraints this document establishes, without
implementing any of them**:
- Derived indexes **can** safely be snapshot-persisted (already true,
  standalone, `i_data` — confirmed working, G1.4.3/recovery-breakdown
  audits).
- Derived indexes **can** be validated against canonical state (not done
  today — no code path checks that a restored `i_data` blob's implied
  vector set matches the canonical `RecordPool` after restore; this is a
  real, identified gap but fixing it is implementation, not design, and
  is explicitly not proposed here).
- Derived indexes **can, in principle, be rebuilt asynchronously/lazily/
  in the background while BruteForce serves queries** — this is
  architecturally sound *given* the canonical/derived boundary already
  holding cleanly (§14): nothing about serving search from BruteForce
  while HNSW builds in the background would touch canonical state. This
  is **not implemented anywhere today** (recovery is synchronous,
  blocking, full-rebuild, per the recovery-breakdown audit) and this
  document does not propose implementing it — only confirms the
  architecture doesn't forbid it.
- **The specific optimization the recovery-breakdown audit already
  identified** (fixing `try_recover()`'s branch order so the existing
  `i_data` fast path isn't bypassed whenever an event log is configured)
  remains the highest-leverage, lowest-risk next step, and requires **no**
  Project/Collection model change at all — it's orthogonal to everything
  in this document.

---

## 16. Graph ownership model

**Does Graph belong to Collection?** **Yes, exactly at the level it
already does** — `GraphNode.namespace_id`/`GraphEdge` (via endpoint
namespace) already scope graph data to a collection identically to how
records are scoped (prior audit §13, kernel-enforced invariant,
`kernel.rs:389-393`). No change recommended.

**Should graph configuration be Collection-level?** **No — because no
such "graph configuration" exists to be leveled anywhere.** There are no
graph-specific settings anywhere in the current design (no configurable
traversal depth limits, no per-collection edge-kind vocabularies) — graph
behavior is entirely a function of what nodes/edges get inserted, not a
declared collection property. Recommending "graph config belongs to
Collection" would be inventing a feature surface with zero present
justification, contrary to rule 5/10.

**Should graph nodes/edges be independent of vectors?** **Already are,
partially, and this should be preserved, not changed**: `GraphNode.record:
Option<RecordId>` is already optional (prior audit §13) — a node can
exist with no backing vector at all (a pure "structural" or "concept"
node). This is real, shipped behavior, not proposed here.

**Should graph-only collections exist?** **Yes — already possible today,
with zero further design needed.** A collection where every
`GraphNode.record` is `None` is already a valid, legal state under the
current kernel invariants. Nothing prevents a user from using a
collection purely as a graph namespace today.

**Should a collection be able to contain vectors only / graph only /
vectors+graph?** **Yes to all three — already true today**, confirmed by
the invariant structure alone (no code path forces a collection to have
both, or either).

**Net finding for this section**: no change is recommended. The current
resource-ownership boundary (namespace-scoped, optional-record-per-node)
already correctly supports every configuration the prompt asks about.

---

## 17. Cloud / data-plane boundary

Evaluating the four options against the prior audit's own confirmed
failure cases:

**A. Cloud is authoritative.** Cloud would need to push collection
create/drop commands to the node and treat its own Postgres row as
ground truth. **Failure case — "Cloud says Collection A exists but node
says it doesn't"**: under this model, this is a **real, dangerous**
failure — any client trusting Cloud's answer would attempt operations
against a namespace the node will reject, with no way for Cloud to detect
the drift without polling the node anyway (defeating the purpose of being
authoritative).

**B. Node is authoritative.** Cloud never stores collection state at
all, always queries the node live. **Failure case — "Node contains
Collection A but Cloud doesn't know about it"**: **not a failure at all**
under this model — Cloud simply doesn't track it until asked, which is
consistent by construction (there's nothing for Cloud's copy to be wrong
*about*, because Cloud has no copy).

**C. Cloud metadata + node canonical state** (a cache, not authority).
Cloud stores a *best-effort mirror* purely for UI/listing convenience
(e.g., populating a collections dropdown without a live round trip),
explicitly documented as non-authoritative, always re-validated against
the node before any destructive operation. **Failure case handling**:
"Cloud says A exists, node doesn't" → Cloud's UI shows a stale entry,
but any actual operation (insert, search, drop) goes to the node and
fails/succeeds based on the node's real state — the stale Cloud entry is
a UX inconvenience (refresh needed), never a correctness hazard.

**D. Eventually consistent mirror** (Cloud subscribes to node change
events, e.g. via polling or a webhook, to keep its mirror fresh).
Same failure-mode profile as C, with lower staleness window, at the cost
of new infrastructure (a sync mechanism) with no current precedent
anywhere in either codebase.

**Recommendation: Option B for correctness-critical operations
(collection existence, for anything destructive or data-affecting),
Option C as a pure UI/listing convenience layer if product needs a
collections dashboard without per-view live round trips.** This directly
mirrors the *already-existing, already-correct* pattern the prior audit
found for Projects themselves: Cloud's `node_url`/`worker_auth_token` are
already treated as "what Cloud believes," always subject to being wrong
relative to what's actually running, with every actual data-plane
operation routed live through the node, never trusted from a Cloud-side
cache. Extending the same pattern to collections is consistent with
existing precedent, not a new risk being introduced. **Option A is
explicitly rejected** — it would introduce a new, worse failure mode
(false confidence) that neither Project nor Collection currently exhibits
anywhere in the audited code.

---

## 18. Cluster / multi-node semantics

**Where collection metadata should be replicated**: the id allocation
already goes through Raft (`AutoCreateNamespace`/`DropNamespace` via
`raft_write_data`, prior audit §6.1, confirmed) — this is correct and
should be preserved unchanged. **The name, if promoted to canonical
(§14), would then also be Raft-replicated for free** — it becomes part of
the same canonical snapshot every replica already receives, closing the
desync risk identified in §5.2 as a direct side effect, not a separate
mechanism to build.

**Collection creation** across replicas: already correct — Raft consensus
before any replica applies the id allocation (confirmed, prior audit
§6.1's cluster trace).

**Collection deletion**: same mechanism (`DropNamespace` via Raft) — not
independently re-traced for its effect on contained records in this pass
(§5.3/§20 open question), but the *replication* mechanism itself is
already sound regardless of what the drop semantics turn out to be.

**Index rebuild**: **not currently a cluster concept at all** — cluster
has no non-BruteForce kernel index (G1.4.3, re-confirmed unchanged this
session). If G1.4.3's Option C (cluster BQ) is ever implemented, rebuild
would need to happen **independently, per-replica**, since (per the
G1.4.3 determinism audit) BQ's derived structure is confirmed
order-independent and bit-identical across replicas building from
identical canonical state — making independent per-replica rebuild safe
for BQ specifically, but this document does not extend that safety claim
to HNSW/IVF, which G1.4.3 already flagged as NOT safely
independently-reconstructible across replicas.

**Configuration change** (index type/params): standalone-only capability
today (§10); cluster has no equivalent mechanism to design around yet.

**Node replacement / new replica joining**: already traced by G1.4.3 —
receives only canonical state via Raft snapshot, index (when one exists)
is not transferred and not rebuilt automatically on install (G1.4.3's
own flagged latent risk, item 1 in that audit's §12). **This document
does not re-resolve that gap** — it is G1.4.3's finding, reused here per
rule 7, not re-derived.

**Snapshot restore** (any node, standalone or cluster): per §14, nothing
about this design changes what's in the snapshot beyond the proposed
collection-name addition — restore behavior is otherwise unchanged from
today's established mechanics.

---

## 19. Compatibility (design, not implementation)

| Concern | Current state | Target state | Migration shape (design-level only) |
|---|---|---|---|
| Database (Cloud) | `projects.dim`/`.index_type` already exist, immutable in practice | No schema change needed — Model A already matches the existing Cloud schema exactly | **None required** — this is the strongest point in favor of Model A: it needs zero Cloud migration |
| Collection name canonicalization | JSON sidecar only | Canonical snapshot field | New snapshot version (analogous to the V5→V6→V7 additive pattern already established, G0.1/G0.2 precedent) — additive, backward-compatible in the same style those versions used |
| API | `POST /v1/namespaces {name}` | Unchanged shape (§12) | **None required** |
| SDK | Passthrough, no local state (prior audit §6.4) | Unchanged | **None required** |
| Existing projects | `dim`/`index_type` already set at creation | No change | **None required** |
| Existing collections | Names in JSON sidecar | Names become canonical | **Required**: a one-time migration reading the JSON sidecar and emitting it into the new canonical field on next snapshot write — additive, no data loss, no downtime beyond a normal restart, but a real, non-trivial piece of work, not "free" |
| Existing snapshots | V7 format (current) | V8 (hypothetical, if name-canonicalization proceeds) | Same backward-compat pattern already proven (V5 snapshots still restore correctly into V7 readers, per CLAUDE.md's documented policy) — this is a solved problem pattern, not a new risk |
| Existing event logs | No `name` in `AutoCreateNamespace`'s *applied* effect (name is currently discarded at kernel level, §2) | Would need the kernel to start honoring the name field it already receives but discards | **Required**: a kernel-level change (not proposed as implemented here) to actually store the name from `AutoCreateNamespace{name}` instead of discarding it — the event *already carries the field*, per `event.rs:145`'s struct definition (only the *application* discards it), so this is a smaller change than it might first appear |
| Existing nodes (already running) | Sidecar-only names | Canonical names | Requires a restart to pick up the new snapshot format — consistent with every prior snapshot-version bump in this project's history |
| Existing HNSW/IVF/BQ indexes | `i_data` section, standalone only | Unchanged by this design | **None required** — this document does not touch index persistence format |

**No breaking change is required for Cloud, API, or SDK compatibility**
under the recommended Model A — this is a direct, evidence-based
consequence of Model A already matching what Cloud's schema and the
SDK's own documentation already assume, confirmed independently in the
prior audit (§9's SDK docstring quote). The only real migration burden is
the optional, incremental collection-name-canonicalization work, which is
additive and non-breaking by the same pattern every prior snapshot
version bump in this project used.

---

## 20. Explicit unresolved questions (blocking implementation)

1. **What does `DropNamespace` actually do to the records/nodes/edges
   still inside a dropped namespace?** Not traced in this design pass.
   This directly affects whether a Collection lifecycle needs a
   `DELETING` transient state (§11) and whether collection deletion
   should be exposed as a Cloud-visible operation with its own
   confirmation UX. **Must be resolved by source inspection before any
   collection-deletion-lifecycle work begins.**
2. **Should collection name become canonical (§14/§19)?** This document
   recommends yes, with a concrete migration shape, but this is a real
   trade (new snapshot version, kernel-level event-application change) not
   yet weighed against how *actually* costly the current desync risk
   (§5.2) has been in practice — **no evidence was gathered on whether
   this desync has ever caused a real incident**; the recommendation
   rests on architectural cleanliness, not a fire being put out.
3. **The collection-level index-type override edge case**: should a
   Project ever be allowed to run *more than one* index type across its
   collections (e.g., a small "scratch" collection on BruteForce
   alongside a large "prod" collection on HNSW, within the same project)?
   Model A as recommended says no (§2/§7) — but this is the single
   softest part of the recommendation, because nothing in the current
   architecture actually *forbids* per-namespace index instances the way
   it hard-forbids per-namespace dimension (index is derived, not
   canonical) — it would "merely" require `Engine.index` to become a map,
   at real but bounded cost (multiplied rebuild time per distinct index
   in use, not per collection). **This document does not resolve this
   question** — it is flagged as the one place Model A's boundary is a
   product choice, not a structural necessity, unlike dimension/metric
   which are structurally forced to be project-level.
4. **Real product demand for `description`/per-collection metadata**: no
   evidence gathered either way in this pass — genuinely unknown whether
   this is a real user need or speculative surface. **Requires product
   input, not further code archaeology.**
5. **Cloud/Kernel collection-registry ownership (§17)**: this document
   recommends Option B (node-authoritative) for correctness-critical
   paths, but whether Cloud needs *any* mirror (Option C) at all is a
   product/UX decision (does the dashboard need to list collections
   without a live round trip?) — **not resolved here**, correctly left to
   product judgment.

---

## 21. Competitor comparison (researched only after codebase understanding, per instruction)

| Aspect | Qdrant | Milvus | Pinecone | Weaviate | pgvector | LanceDB | **Valori (current, re-confirmed)** | **Valori (recommended)** |
|---|---|---|---|---|---|---|---|---|
| Dimension mutability | **Immutable** — "Distance and vector size are immutable. Once a collection is created with size=384, Cosine, you cannot promote it to 768." | Set at collection creation via schema; no in-place alter documented for existing data | Set at index creation, must match embedding model; no in-place resize | Set at collection creation | Column type-fixed (`vector(n)`); changing requires the documented add-column/drop-column/rename dance | Dimension change requires the same add/drop/rename column pattern — "cannot be cast in-place" | **Immutable** (kernel-locked on first insert, project-declared at provisioning) | **Immutable**, unchanged — matches every competitor surveyed |
| Metric mutability | **Immutable**, same citation as dimension | Settable at creation ("metric_type"); no in-place change documented | Immutable, fixed at index creation | Weaviate's own GitHub issue #3177: silently *allowed* to change today but explicitly flagged by their own engineers as a bug — "the graph already built will no longer be valid," recommendation is to make it immutable | Operator/index-class must match at query time; changing effectively means rebuilding the index with a new operator class | N/A (LanceDB search didn't surface a direct claim either way) | **Immutable** (single global metric, hardcoded L2) | **Immutable**, unchanged |
| Index type mutability | Configurable per collection at creation; rebuild-to-change is the norm industry-wide | `createIndex()`/rebuild pattern, not in-place | Pod vs. serverless is an infra choice, not really an "index type" the way HNSW/IVF are | Per-collection `vectorIndexConfig`, changeable via rebuild | HNSW/IVF both supported, switching means building a new index alongside/replacing the old (`REINDEX`) | Index rebuild supported, explicitly documented as a maintenance operation after heavy updates | **Mutable, standalone only** (`/v1/index/rebuild`), full rebuild, blocking | **Mutable**, unchanged recommendation — matches every competitor's rebuild-to-change norm |
| Config granularity | **Per-collection** (Qdrant's "collection" already includes dim+metric+index — closer to Valori's *Project*, not Valori's *Collection*) | **Per-collection**, same note | **Per-index** (Pinecone's "index" ≈ Valori's *Project*-level vector-config domain) | **Per-collection**, with an explicit multi-vector escape hatch ("named vectors" — multiple independently-configured vector spaces *within* one collection, for cases needing more than one embedding model on the same objects) | **Per-table/column** | **Per-table** | **Per-Project (node)**, current | **Per-Project (node)**, recommended — see terminology note below |
| Multi-tenancy | Payload-based filtering within one collection, or separate collections per tenant — both patterns documented | Partition key within a collection is the documented multi-tenancy primitive | Namespaces within an index — closely analogous to Valori's Collection-within-Project | Multi-tenancy via a dedicated per-tenant sharding feature | Row-level (a `tenant_id` column), same database | N/A (not deeply researched) | **Namespace (Collection) within a node (Project)** — structurally closest to Pinecone's namespace-within-index and Weaviate's per-tenant sharding | Unchanged |
| Snapshot/recovery | Managed, not user-facing in the same way | Managed | Fully managed, no user-facing recovery concept at all (serverless) | Backup/restore documented as a distinct operation | Standard Postgres backup/restore (WAL, pg_dump) | Versioned via MVCC, "atomic and versioned" | **User/operator-facing, explicit** (event log + snapshot + rebuild) — this is a genuine Valori differentiator, not a gap, since forensic verifiability (the CLI's own stated purpose, prior audit §1.4) requires this to be explicit, unlike every managed competitor | Unchanged — this document does not propose making recovery implicit/managed-away, since that would conflict with Valori's own stated forensic/deterministic-replay identity |

**Terminology note, important for internal clarity**: what Qdrant/
Pinecone call a "collection"/"index" (the thing that owns dim+metric+
index-type) is structurally closer to what Valori calls a **Project**,
not what Valori calls a **Collection**. What Qdrant achieves via payload
filtering or Pinecone via "namespaces" within one index is structurally
closer to what Valori already calls **Collection**. **This document's
recommendation (Model A) is therefore not "different from the
industry" — it is the same shape as Qdrant/Pinecone's model, expressed
with Valori's own two-level naming** (Project ≈ their collection/index;
Collection ≈ their namespace/tenant-partition). Weaviate's "named
vectors" escape hatch is the one competitor feature that maps to this
document's §20 item 3 (the collection-level index-type/embedding-model
override question) — flagged there as unresolved, not adopted, precisely
because Weaviate's own engineers flagged their unconstrained version of
this as a bug (the GitHub issue cited above), which is instructive: *if*
Valori ever adds a per-collection override, it should be a deliberately
constrained, immutable-once-set escape hatch, not a freely-mutable field
— consistent with every competitor's actual (not aspirational) behavior.

**CURRENT VALORI FACT / COMPETITOR FACT / RECOMMENDATION, explicitly
separated, summary**:
- CURRENT VALORI FACT: dimension/metric are project-level, immutable,
  confirmed by direct source inspection (§2, §9).
- COMPETITOR FACT: every surveyed system treats dimension/metric as
  immutable at whatever level owns them (their "collection"/"index");
  Weaviate's own team considers their current mutable-metric behavior a
  bug to be fixed, not a feature.
- RECOMMENDATION: keep Valori's current immutability stance, keep the
  configuration at the Project level (not Collection), and treat any
  future per-collection override as a Weaviate-named-vectors-style
  constrained escape hatch, never a freely mutable field — **not
  implemented now**, flagged in §20.

---

## 22. Decision log

| Decision | Options considered | Chosen | Reason | Consequence |
|---|---|---|---|---|
| Project vs. Collection vector-config ownership | Model A (project-level) / Model B (collection-level) | **Model A** | Matches current structural reality (`Engine`/`KernelState` scalars); avoids multiplying HNSW recovery cost by collection count (§15); matches shipped SDK docstring; matches every competitor's actual (not aspirational) behavior once terminology is normalized (§21) | Zero Cloud/API/SDK migration required (§19); collections remain lightweight; the one open edge (§20 item 3) is deferred, not resolved |
| Dimension/metric/embedding-model mutability | Immutable / mutable-with-warning / mutable-with-silent-reinterpretation | **Immutable, no exceptions** | Changing any of the three produces silently, undetectably wrong search results with no code-level way to catch the corruption after the fact (§9) | Migration to a new dimension/metric is always "create new collection," never "edit in place" (§13) |
| Index type/config mutability | Immutable / mutable-with-rebuild / mutable-without-rebuild | **Mutable, always requires rebuild** (except confirmed search-time-only params) | Index is derived, not canonical — G0.2's own principle already establishes this; already the standalone-implemented behavior | No new canonical-state risk from allowing index changes |
| Collection name: sidecar-only vs. canonical | Keep as sidecar / promote to canonical | **Promote to canonical** (recommended, not implemented) | Closes a real desync risk under Raft replication (§5.2, §18) | Requires a new, additive snapshot version and a kernel-level change to stop discarding the name field `AutoCreateNamespace` already carries (§19) |
| Collection lifecycle states | Full prompt-example state machine / minimal evidence-trimmed set | **Minimal** — no `CREATING`/`READY`/`UPDATING_INDEX` states added | Creation is already synchronous and sub-millisecond; index rebuild is node-level, not collection-level, so a per-collection "updating" state would misrepresent reality | Simpler mental model; revisit only if async deletion is later justified (§20 item 1) |
| Cloud/Kernel collection registry authority | A (Cloud authoritative) / B (Node authoritative) / C (Cloud cache) / D (eventually-consistent mirror) | **B for correctness-critical paths, C optionally for UI convenience** | Mirrors the already-existing, already-correct pattern for Project's own `node_url`/`worker_auth_token` fields — Cloud is never trusted as ground truth for live node state anywhere in the current design | No new failure mode introduced; consistent with existing architecture |
| Graph configuration as a collection field | Add a declared graph-config field / leave emergent | **Leave emergent — no field added** | No such configuration exists anywhere in the current codebase to formalize; adding one would be speculative (§16) | Zero design/migration burden; graph-only, vector-only, and mixed collections all already work today with no change |

---

## 23. Recommended target architecture

```
Cloud:

  Organization
  └── Project
      ├── Project configuration (IMMUTABLE): dimension, metric,
      │     embedding model (implicit via VALORI_EMBED_MODEL), default
      │     index type + config
      ├── Project configuration (MUTABLE): name, plan (org-level, joined),
      │     limits, replication factor (additive)
      └── Collections  ← Cloud-side view is NEVER authoritative for
                          existence (§17) — this is a convenience listing
                          only, always re-validated against the live node
                          before any destructive operation

Data plane:

  Project
  └── Node / Cluster (one vector-configuration domain, per Model A)
      └── Collection
          ├── immutable semantic configuration: NONE — this document's
          │     central finding is that Collection owns no semantic
          │     configuration at all under Model A; dimension/metric/
          │     embedding-model live one level up, at Project/Node
          ├── mutable metadata: name (recommended to become canonical,
          │     §14/§19), created_at (if adopted)
          ├── canonical state: NamespaceId, its Records (RecordPool
          │     slice), its GraphNodes/GraphEdges (NodePool/EdgePool
          │     slices) — unchanged from today
          └── derived indexes: NONE owned per-collection — the single
                node-level ActiveIndex/boxed VectorIndex serves every
                collection on that node uniformly (Model A)
```

### Immutable properties
Project: dimension, metric, embedding model, region (de facto), org_id,
slug. Collection: id, namespace_id.

### Mutable properties
Project: name, plan, limits (recommended, currently no UPDATE path but no
structural reason blocks it), replication (additive, recommended).
Collection: name (once canonicalized).

### Rebuildable properties (derived config)
Node-level: index type, HNSW `M`/`ef_construct`, IVF `n_list` — always
requires a rebuild, never touches canonical data. Pure search-time knobs
requiring no rebuild at all: HNSW `ef_search`, IVF `n_probe`, BQ
`pool_factor`/`min_candidates`.

### Derived properties
`record_count`, `storage_size` (per collection, computed on demand, never
stored) — both cheap to derive from canonical state, storing them
duplicately risks drift with zero benefit.

### Cloud-only properties
`worker_auth_token`, `node_url`, `pinned_image`, `org_id`, `region`,
`plan`, `replication` — none of these have (or should have) any
representation inside the kernel/node at all; they are provisioning and
billing facts, not data-plane state.

---

## 24. Implementation roadmap (derived from this analysis, not templated)

Given every "None required" finding in §19, the roadmap is deliberately
front-loaded with the one real structural change (name canonicalization)
and back-loaded with genuinely optional/product-gated work:

- **G2.1 — Collection name canonicalization**: promote collection name
  from JSON-sidecar-only to a canonical snapshot field; stop discarding
  the `name` field `AutoCreateNamespace` already carries. New, additive
  snapshot version. Directly closes §5.2/§18's desync risk. Self-contained,
  no dependency on any other item below.
- **G2.2 — Formalize the Project↔Collection Cloud-side relationship
  (Option B+C, §17)**: build the "convenience mirror" (Option C) only if
  product confirms real UX need (§20 item 5) — otherwise this phase is
  "confirm and document Option B as already-correct, no new code."
- **G2.3 — `DropNamespace` semantics audit**: a dedicated, focused
  follow-up audit (not a redesign) tracing exactly what happens to
  records/nodes/edges inside a dropped namespace today — this must
  complete *before* any collection-deletion-lifecycle or Cloud-visible
  delete-collection UX work, per §20 item 1.
- **G2.4 — (Product-gated, no committed scope) Collection-level
  index-type override** — only if product confirms real demand for
  running mixed index types within one project (§20 item 3); this phase
  does not exist unless that product decision is made first.
- **G2.5 — (Product-gated, no committed scope) Per-collection
  description/metadata field** — only if product confirms real demand
  (§20 item 4); otherwise never built.
- **Independent, already-identified, NOT gated on any of the above**:
  the recovery-breakdown audit's own highest-leverage finding (fixing
  `try_recover()`'s branch order so the `i_data` fast path isn't
  bypassed) — this is orthogonal to the entire Project/Collection domain
  model and could proceed on its own schedule without waiting for G2.x.

---

## G2.0 VERDICT

# NOT READY FOR IMPLEMENTATION

Blocking, in priority order:

1. **§20 item 1** — `DropNamespace`'s actual effect on contained
   records/nodes/edges is untraced. This blocks any collection-lifecycle
   or collection-deletion design commitment, including the otherwise
   self-contained G2.1 work if it's ever bundled with lifecycle changes
   (it should not be — G2.1 as scoped above does not depend on this, but
   any *broader* collection-lifecycle phase does).
2. **§20 item 2** — whether collection-name canonicalization (G2.1) is
   worth its migration cost has no evidence of actual incident history to
   weigh against the architectural-cleanliness argument. Recommended to
   proceed on cleanliness grounds alone (the cost is small and additive),
   but this is a judgment call this document flags rather than makes
   unilaterally.
3. **§20 item 3** — the collection-level index-type override question is
   the one place this document's central recommendation (Model A) has a
   soft edge rather than a hard structural justification. Needs an
   explicit product decision before G2.4 can even be scoped, let alone
   built.
4. **§20 items 4/5** — real product demand for collection description/
   metadata and for a Cloud-side collection mirror are both genuinely
   unknown from code alone and require product input, not further
   architecture analysis.

None of these block *starting* G2.1 specifically (name
canonicalization, §24) — that item is self-contained and does not depend
on any of the four open questions above. But the *overall* domain model
this document was asked to finalize is not fully closed, per the
explicit instruction to list exact blocking decisions rather than declare
readiness prematurely.
