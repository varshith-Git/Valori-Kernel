# Phase API-3.1 — Complete Public Utoipa Coverage & Contract Convergence

**Date**: 2026-08-20
**Baseline commit**: `eee123d`
**Branch**: `main` (live working tree; nothing committed, reverted, or stashed)
**Predecessor**: [Phase API-3 Recovery](./phase-api-3-recovery.md)

---

## 1. Goal

Finish the code-first OpenAPI migration the recovery phase scoped: annotate every
remaining public handler with a real `#[utoipa::path]` and real `ToSchema` DTOs,
register them on `ValoriApi`, and make `api/openapi/valori-v1.yaml` the
generator's byte-exact output — so that Rust public routes, utoipa operations,
and the canonical contract are provably the same set.

---

## 2. Delivered

### 2.1 Baseline, measured before any change

`docs/api/phase-api-3.1-baseline.md`. The verifier reported **79** discrepancies
at entry (the brief quoted 83; the recovery phase's final verifier revision
de-duplicates routes counted twice). 74 public routes, 11 utoipa operations,
63 unannotated.

### 2.2 The 63 annotations

Where the 74 annotations now live, counted from the regenerated manifest's
`utoipa_source_file` field — 11 pre-existed, **63 were added by this phase**:

| File | Public operations |
|---|---|
| `crates/valori-node/src/server.rs` | 63 (incl. 2 new alias wrapper handlers) |
| `crates/valori-node/src/cluster_api.rs` | 3 (`/v1/cluster/{status,role,health}`) |
| `crates/valori-node/src/ingest.rs` | 3 (`/v1/ingest`, `/update`, `/status/{job_id}`) |
| `crates/valori-rag/src/tree.rs` | 2 (`/v1/tree/verify`, `/chain-verify`) |
| `crates/valori-node/src/cluster_server.rs` | 1 (`/v1/cluster/proof`) |
| `crates/valori-node/src/routes/mod.rs` | 1 (`/v1/version`) |
| `crates/valori-ingest/src/handler.rs` | 1 (`/v1/ingest/document`) |
| **Total** | **74** |

All 74 are listed in `ValoriApi`'s `paths(...)`.

### 2.3 DTOs

* **34** existing `api.rs` DTOs gained `ToSchema`, plus `schema(value_type = …)`
  on 13 `serde_json::Value` fields so they render as objects rather than as
  nothing.
* **16 new public DTOs** in `api.rs`, for endpoints that previously answered
  with an untyped `serde_json::Value`: `RecordResponse`,
  `UpdateMetadataResponse`, `StateProofResponse`, `UsageResponse`,
  `UsageStorage`, `ShardRoutingResponse`, `ShardRoutingEntry`,
  `IndexRebuildRequest`, `IndexRebuildResponse`, `SubgraphResponse`,
  `GraphRagResponse`, `CommunityOverviewResponse`, `CommunityOverviewEntry`,
  plus the three §14 health mirrors `PoolStatsSchema`, `EngineHealthStats`,
  `ClusterHealthStats`.
* **Seven standalone handlers** were converted to return a DTO instead of a
  `json!` literal (`get_record_by_id`, `update_record_metadata`, `get_proof`,
  `usage_handler`, `shard_routing_handler`, `index_rebuild_handler`,
  `community_overview`), so the type system enforces the shape the contract
  promises rather than the contract merely describing a literal.
* **Three new cluster DTOs** — `ClusterRoleResponse`, `ClusterHealthResponse`,
  `ClusterProofResponse` — with their three handlers likewise converted off
  `serde_json::Value`; `StatusView` and `MemberView` gained `ToSchema`.

### 2.4 Cross-crate `utoipa` feature (§10 — no duplicated wire models)

Five workspace crates own public request/response types the data plane returns
verbatim. Rather than hand-copy them into `api.rs`, each gained an **optional,
default-off** `utoipa` feature, enabled transitively by `valori-node/utoipa`:

`valori-engine`, `valori-rag`, `valori-ingest`, `valori-models`,
`valori-storage`.

The contract therefore references the same Rust type the handler serialises. A
field change cannot drift the schema, because there is only one type.

`valori-kernel` was **not** touched. It gains no dependency and remains `no_std`
(`cargo build -p valori-kernel --target wasm32-unknown-unknown` verified clean).

### 2.5 The two missing routes (§7)

`POST /v1/memory/search_vector` and `POST /v1/memory/upsert_vector`. These are
not decorative aliases — `python/valoricore/remote.py` and `protocol.py` call
**only** these spellings, so they are load-bearing public surface. utoipa binds
one path per function, so each got a thin wrapper handler that delegates to the
canonical one and carries its own annotation. Both are now in the contract.

### 2.6 The seven classification leaks (§8)

`GET/POST /v1/keys`, `DELETE /v1/keys/{id}`, `DELETE /v1/crypto/shred/{key_id}`,
`POST /v1/cluster/{add-node,remove-node,snapshot}` are ADMIN. They are **still
served by the node** — no route was removed from any router. They are simply not
registered on `ValoriApi`, so they no longer appear in the public SDK contract.
`tests/api_contract.rs::contract_records_the_corrected_scopes` was inverted to
assert their absence; the scope they enforce is still pinned directly against
`required_scope`, one test above.

### 2.7 operationIds (§9)

The manifest generator no longer derives an id from the handler name. It reads
`operation_id` out of the handler's own `#[utoipa::path]`, so the canonical id is
declared exactly once, in Rust. That is also what lets one id serve both the
standalone and cluster registration of a path, whose handler functions have
different names.

All 24 ids that would otherwise have changed were set back to the identifiers the
previously published contract used (`get_health`, `list_collections`,
`create_collection`, `delete_collection`, `list_operations`, `get_operation`,
`search_multi`, `create_graph_node`, …). Net operationId churn versus the
previous contract: **0**.

One deviation from §9's suggested list is deliberate: `POST /v1/namespaces/{name}/index`
keeps the published `set_collection_index` rather than adopting
`create_collection_index`, because the endpoint creates, changes, **and** drops
an index — `set` describes it and `create` does not. `get_collection_index`
matches §9 exactly.

### 2.8 OpenAPI version (§17)

`docs/api/openapi-version-decision.md`. **3.1.0 is now the target.** utoipa
5.5.0's `OpenApiVersion` enum has one variant; 3.0.x is unreachable by
configuration, downgrading utoipa would mean deleting annotations this phase
depends on, and a 3.1→3.0 normaliser is a lossy rewrite §17 explicitly says not
to build first. `openapi-typescript@7` — the only generator this repo actually
runs — is 3.1-first. The implementation is one constant in the gate.

### 2.9 Vendor extensions (§19)

`VendorExtensionAddon`, a `Modify` pass, stamps `x-required-scope` and `x-sdk`
onto every generated operation. `x-required-scope` is read from
`crate::api_keys::required_scope` — the same function the auth middleware calls
at request time — so the contract cannot document a scope the server does not
enforce. `tests/api_contract.rs::every_operation_documents_the_scope_the_server_enforces`
checks all 74, not a sample. The pass adds metadata only; it has no code path
that could create a path, body, or response.

### 2.10 Deterministic generation (§23)

`to_yaml()` now renders through `serde_json::Value` before YAML. utoipa stores
extensions in a `HashMap` flattened into the operation object, and Rust's
`HashMap` iterates in per-process random order, so rendering straight from the
struct produced a semantically identical but byte-different file on every run.
`serde_json::Map` is a `BTreeMap` in this build, so every object is key-sorted
and the artifact is byte-reproducible across runs and machines.

### 2.11 Contract gate (§24)

`scripts/api-contract-gate.sh`: target version is a named variable, step 2 now
writes the canonical artifact (so a stale committed file is impossible rather
than merely detected), step 6a runs with `--features utoipa`, and the summary
prints the §24 diff counts — missing, unexpected, operationId mismatches,
classification leaks — all computed from the manifest and the two documents, none
hardcoded. Two new blocker conditions were added for opid mismatches and
classification leaks.

### 2.12 Files touched

```
crates/valori-node/src/{openapi.rs, api.rs, server.rs, cluster_server.rs,
                        cluster_api.rs, ingest.rs, execution_registry.rs,
                        routes/{mod.rs, graph.rs, explain.rs}, Cargo.toml}
crates/valori-node/src/bin/valori-openapi.rs
crates/valori-node/tests/{openapi_generated.rs, api_contract.rs}
crates/valori-engine/{Cargo.toml, src/index_manager.rs}
crates/valori-rag/{Cargo.toml, src/tree.rs, src/community.rs}
crates/valori-ingest/{Cargo.toml, src/handler.rs, src/chunker.rs, src/execution.rs}
crates/valori-models/{Cargo.toml, src/health.rs}
crates/valori-storage/{Cargo.toml, src/object_store.rs}
scripts/{generate-route-manifest.py, api-contract-gate.sh}
api/openapi/valori-v1.yaml            (regenerated, never hand-edited)
ui/api-types/src/{index.ts, valori-v1.ts}
docs/api/{phase-api-3.1-baseline.md, openapi-version-decision.md,
          phase-api-3-route-manifest.{json,md}, sdk-readiness.json}
```

---

## 3. Findings

**F1 — the route manifest could not see its own annotations.** `find_utoipa_annotations()`
matched only the literal `#[utoipa::path(` form, but every annotation in this
repository is written `#[cfg_attr(feature = "utoipa", utoipa::path(…))]`. The
manifest therefore reported "Public routes with `#[utoipa::path]`: 0" while the
generator was emitting 11. A §6 violation in the recovery phase's own
instrumentation, found while taking the baseline.

**F2 — `utoipa_registered` was measuring one link of a two-link chain.** An
annotation alone generates nothing; the handler must also appear in `ValoriApi`'s
`paths(…)`. The manifest now parses that block and reports
`utoipa_annotated`, `utoipa_registered_in_api`, and `utoipa_registered` (their
conjunction) separately, so a half-wired handler is visible rather than silently
counted as covered.

**F3 — five fabricated DTOs were live in the schema registry.** `api::IndexStatusResponse`,
`IndexSpecDto`, `ActiveIndexDto`, `BuildingIndexDto`, and `OperationResponse`
were added by the failed Phase API-3 and registered as schemas, but **no handler
anywhere constructs any of them**. They described an index lifecycle model
(`state`/`desired`/`active`/`building`/`failure`) that the implementation does
not have. The real model is `valori_engine::index_manager::IndexStatusResponse`
(`active_type`, `active_generation`, `desired_type`, `status`,
`building_generation`, `base_lsn`, `build_started_at`, `error`), which is what
both routers actually return. The five were deleted and the real type is now the
public contract — §15's "preserve the real lifecycle model" read literally.

**F4 — `StructureNode` made the generator recurse forever.** `Vec<StructureNode>`
inside `StructureNode` sent utoipa's schema builder into unbounded descent; even
a 256 MiB stack overflowed, so it was infinite, not merely deep. Fixed with
`#[schema(no_recursion)]` on that one field, which emits a `$ref` back to the
type. Worth recording because the failure mode is a stack overflow with no
diagnostic, and the fix is one attribute.

**F5 — cluster `/health` had silently lost `leader` and `dim`.** The pre-API-3
cluster handler emitted both at the top level; the Phase API-3 `HealthResponse`
dropped them. `ui/src/lib/hooks/useHealth.ts` still reads `data?.dim`, so this
was a live break that `tsc` surfaced only once the contract became real. Both
fields are restored at the top level (§14: legacy fields are not removed for
schema tidiness) **and** exposed inside the structured `cluster` object.

**F6 — `x-status` is gone and this phase did not restore it.** The API-2
hand-maintained contract carried a per-operation `x-status`. The synthetic Phase
API-3 document dropped it, and there is no Rust-side source of truth for it —
unlike `x-required-scope`, which has `required_scope()`. Inventing values would
be exactly the fabrication this workstream exists to stop. Recorded as a
follow-up, not silently skipped. See §5.

**F7 — `legacy_id` is not required.** §16 asks for `legacy_id: Option<u64>`
"where required for compatibility". Audited: operation identity is
`format!("op-{log_index}")` — a string in every version of this endpoint — and
`get_operation_by_id` already accepts both `op-7` and bare `7`. No consumer
(`remote.py`, the four UI proxy routes) has ever seen a numeric id field. There
is nothing to preserve, so no `legacy_id` field was added. Both URL spellings are
now pinned by `operation_urls_accept_both_id_forms`.

**F8 — `ui/api-types/src/index.ts` was the intended failure point, and it fired.**
Seven aliases (`Metric`, `IndexKind`, `IndexType`, `IndexState`, `Consistency`,
`InsertReceipt`, `EventLogProof`) referenced schemas the generated contract does
not have; `metric` and index-kind are plain strings on the wire and never had a
closed enum to alias. Nothing under `ui/` imported any of the seven, so they were
removed; the rest were repointed at the generated names. This is the file working
as designed — a renamed schema became a TypeScript error instead of silent drift.

---

## 4. Validation

### 4.1 Three-way route contract

```
$ python3 scripts/verify-api-route-contract.py
  Rust public routes:        74
  Utoipa operations:         74
  OpenAPI operations:        74
  Missing Utoipa:             0
  Missing OpenAPI:            0
  Unexpected Utoipa:          0
  Unexpected OpenAPI:         0
  OperationId mismatches:     0
  Classification errors:      0
ROUTE CONTRACT: PASS
```

Entry state was 79 discrepancies. Exit state is 0.

### 4.2 Contract gate

```
$ ./scripts/api-contract-gate.sh
   Route discovery from Rust router source              PASS
   Utoipa OpenAPI generation                            PASS
   Three-way route equality (Rust == Utoipa == OpenAPI) PASS
   Generated schema conformance                         PASS
   OpenAPI lint                                         PASS
   API contract integration suite                       PASS
   Standalone/cluster route parity                      PASS
   TypeScript wire types generation                     PASS
   Generator reproducibility                            PASS
   TypeScript wire types compile                        PASS
   Python remote API compatibility                      PASS

   Rust routes registered:       100
   Rust public routes:            74
   Utoipa operations:             74
   OpenAPI operations:            74
   Missing (Rust -> utoipa/doc):   0
   Unexpected (not in Rust):       0
   operationId mismatches:         0
   Classification leaks:           0
   Utoipa schemas:               138
   OpenAPI schemas:              138
   OPENAPI VERSION EMITTED:    3.1.0 (target 3.1.0)

 CONTRACT GATE: PASS
 SDK READINESS: YES
 SDK BLOCKERS (0):
```

### 4.3 Test counts

| Crate | Passed | Failed |
|---|---|---|
| `valori-node` | 446 | 0 |
| `valori-kernel` | 177 | 0 |
| `valori-ingest` | 110 | 0 |
| `valori-models` | 78 | 0 |
| `valori-storage` | 78 | 0 |
| `valori-rag` | 46 | 0 |
| `valori-engine` | 18 | 0 |

`cargo build --workspace` clean. `cargo clippy -p valori-node --features utoipa`
produces no new errors. `cargo build -p valori-kernel --target wasm32-unknown-unknown`
clean — the `no_std` invariant is intact.

New tests added in this phase (7):

* `openapi_generated::committed_contract_is_byte_identical_to_the_generator_output`
  — replaces the old subset-with-allowlist assertion, which a hand-written
  superset would have passed. This one would have caught the Phase API-3 failure.
* `openapi_generated::generated_document_declares_the_target_openapi_version`
* `openapi_generated::the_error_taxonomy_is_a_first_class_component` (§20)
* `openapi_generated::generation_is_deterministic`
* `api_contract::every_operation_documents_the_scope_the_server_enforces` (§19)
* `api_contract::health_subobjects_match_schema_mirrors` (§14)
* `api_contract::cluster_health_keeps_its_legacy_top_level_fields` (§14)
* `api_contract::operation_urls_accept_both_id_forms` (§16)
* `api_contract::generated_operation_ids_are_present_and_unique` (§12)

### 4.4 Contract shape

| Property | Before | After |
|---|---|---|
| `openapi` | 3.0.3 (synthetic) | 3.1.0 (generated) |
| operations | 79 | 74 |
| schemas | 26 | 138 |
| operations with a real `requestBody` | 4 of 40 | 37 of 38 |
| distinct response descriptions | 2 | 60+ |
| operations with `x-required-scope` | 0 | 74 |

The single write operation with no request body is
`POST /v1/storage/snapshots/upload`, which genuinely takes none.

### 4.5 Manual smoke test

```bash
cargo run -p valori-node --features utoipa --bin valori-openapi -- \
  --output api/openapi/valori-v1.yaml
cargo run -p valori-node --features utoipa --bin valori-openapi > /tmp/a.yaml
cargo run -p valori-node --features utoipa --bin valori-openapi > /tmp/b.yaml
cmp /tmp/a.yaml /tmp/b.yaml          # byte-identical
npx @redocly/cli lint api/openapi/valori-v1.yaml   # valid, 1 warning
./scripts/generate-api-types.sh && (cd ui && npx tsc --noEmit)
```

---

## 5. Follow-ups

| Item | Why it is deferred | Owner |
|---|---|---|
| `x-status` per operation (F6) | No Rust source of truth exists. Needs a deliberate per-operation stability classification (`stable`/`beta`/`deprecated`) declared in Rust, then stamped by the same `Modify` pass that already handles `x-required-scope`. Fabricating values now would repeat the Phase API-3 failure. | Phase API-4 |
| `/health` has no documented 4xx (Redocly warning) | Accurate: `/health` is unauthenticated and answers 200 or 503. The lint rule is a house-style rule, not a spec violation. Suppressing it needs a `redocly.yaml`, which is a separate decision about lint policy. | Phase API-4 |
| The 26 non-public routes have no contract | Deliberate (§8). If an admin/operator contract is ever wanted it should be a **separate document** (`valori-admin-v1.yaml`) with its own gate, not a mixing of surfaces. | unscheduled |
| Actual SDK generation | Explicitly out of scope (§2). The contract is now trustworthy enough to generate from; that is the whole point of this phase. | Phase API-4 |
| `/v1/proof/event-log` + `/v1/timeline` read shard 0 only | Pre-existing sharding gap, now *documented* in the contract's `get_timeline` description rather than silently wrong. | sharding backlog |
