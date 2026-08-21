# Contract Conformance — Valori Data Plane API v1

**Contract**: `api/openapi/valori-v1.yaml` — OpenAPI 3.1.0, `info.version: 1.0.0`
**Last reconciled**: Phase API-3.1 (complete public utoipa coverage)
**Enforced by**: `crates/valori-node/tests/api_contract.rs`,
`crates/valori-node/tests/openapi_generated.rs`,
`crates/valori-node/tests/route_parity.rs`

This document answers one question: **where does the running node agree with
the published contract, and where does it not?** It is deliberately blunt
about the second half. Phase API-1 produced the contract; Phase API-2 made the
implementation meet it in the places that mattered most, and this file records
what is left.

---

## 1. How the contract is produced today

> **Superseded as of Phase API-3.1.** The contract is no longer hand-maintained.
> `api/openapi/valori-v1.yaml` is the byte-exact output of
> `cargo run -p valori-node --features utoipa --bin valori-openapi`, covering
> all 74 public routes, and
> `crates/valori-node/tests/openapi_generated.rs` fails if the committed file
> differs from the generator's output by a single byte. The prose below
> describes the API-2 arrangement and is kept for history; the current
> pipeline, its guarantees, and the OpenAPI 3.0.3 → 3.1.0 target change are
> documented in
> [`docs/phases/phase-api-3.1-utoipa-coverage.md`](../phases/phase-api-3.1-utoipa-coverage.md)
> and [`openapi-version-decision.md`](./openapi-version-decision.md).

The contract *was* **hand-maintained, machine-checked**.

```
crates/valori-node/src/api.rs          public DTOs, serde + optional ToSchema
        │
        ├─ #[cfg(feature = "utoipa")] ─▶ ValoriApi  (src/openapi.rs)
        │                                    │
        │                                    ├─▶ cargo run --features utoipa
        │                                    │     --bin valori-openapi  ▶ YAML
        │                                    │
        │                                    └─▶ tests/openapi_generated.rs
        │                                          diffs generated schema NAMES
        │                                          against the committed file
        │
        └─ serde ───────────────────────▶ the wire

api/openapi/valori-v1.yaml             the canonical, hand-maintained document
        │
        └─ scripts/generate-api-types.sh ─▶ ui/api-types/src/valori-v1.ts
```

There is **one** logical contract (§33). The utoipa document is not a second
model — it is a partial re-derivation of the same one, whose only job right
now is to fail a test when a Rust DTO and the contract disagree on the set of
public schema names.

### Generated vs hand-maintained

| | Count | Where |
|---|---|---|
| Schemas the Rust code generates | 16 (12 of which name a committed schema) | `crates/valori-node/src/openapi.rs` |
| Schemas in the committed YAML | 102 total, 90 hand-maintained only | `api/openapi/valori-v1.yaml` |
| Path items generated | 0 | — |
| Path items hand-maintained | 75 (79 operations) | `api/openapi/valori-v1.yaml` |

Generated today: `ApiError`, `CollectionInfo`, `CreateCollectionRequest`,
`CreateCollectionResponse`, `ErrorCode`, `InsertReceiptJson`,
`InsertRecordRequest`, `InsertRecordResponse`, `ListCollectionsResponse`,
`MultiSearchHit`, `MultiSearchRequest`, `MultiSearchResponse`,
`PartialSearchFailure`, `RequestId`, `SearchHit`, `SearchResponse`.

Four of those — `ApiError`, `InsertReceiptJson`, `CollectionInfo`,
`PartialSearchFailure` — are Rust-side names the contract spells differently
or inlines; the conformance test carries them in a short, justified allowlist.

Those are exactly the domains Phase API-2 converged: Collections, Records,
Search, Multi-Search, Errors. Nothing else is annotated, and the module says so
in its own doc comment. **Do not read `#[derive(OpenApi)]` in this repo as
evidence that the whole document is generated.** The end state — generation
owns the file — needs Indexes, Graph, GraphRAG, Memory, Ingest, Proof and
Snapshots annotated first, and needs a way to keep the prose descriptions and
`x-` extensions that make the hand-written document useful.

### Regenerating

```bash
# Emit the generated subset (for inspection / diffing)
cargo run -p valori-node --features utoipa --bin valori-openapi

# Fail the build if a Rust DTO drifted from the contract
cargo test -p valori-node --features utoipa --test openapi_generated

# Validate the committed contract
npx @redocly/cli@latest lint api/openapi/valori-v1.yaml

# Regenerate the TypeScript wire model consumed by ui/ and ui/studio
./scripts/generate-api-types.sh
```

The `utoipa` feature is **opt-in**, so the shipped node binary carries no
schema-generation code and the default `cargo test -p valori-node` run is
unaffected by it.

---

## 2. Implementation status by domain

| Domain | Contract ↔ impl | Standalone/cluster parity | Test |
|---|---|---|---|
| Collections — create/list/delete | ✅ conforms | ✅ proven | `api_contract.rs` (4 tests) |
| Records — insert | ✅ conforms | ✅ proven | `api_contract.rs` (5 tests) |
| Records — idempotency (`request_id`) | ✅ conforms | ✅ proven | `api_contract.rs` (4 tests) |
| Batch insert | ✅ conforms | ✅ proven | `api_batch_idempotency.rs` |
| Search | ✅ conforms | ✅ proven | `api_contract.rs`, `search_k_bounds.rs` |
| Multi-search | ✅ conforms | ✅ proven | `api_contract.rs`, `multi_collection_search.rs` |
| Errors — `code` on every body | ✅ conforms | ✅ proven | `api_contract.rs` (3 tests) |
| Scopes (`x-required-scope`) | ✅ conforms | n/a | `api_contract.rs` (2 tests) |
| Indexes | ⚠️ documented drift — see §4 row 9 | ✅ same schema both paths | `index_lifecycle.rs` |
| Graph | ✅ conforms | ✅ proven | `api_graph_*.rs`, `cluster_graph_*.rs` |
| GraphRAG | ✅ conforms | ✅ proven | `api_graphrag.rs` |
| `/health` | ❌ **open drift** — see §4 row 37 | ❌ two shapes | — |
| Object store | ⚠️ documented drift — see §4 row 27 | — | `api_object_store.rs` |
| Legacy aliases | ✅ conforms (headers intact) | ✅ proven | `api_contract.rs` |

---

## 3. Resolved divergences

Row numbers refer to `docs/api/current-vs-target.md`.

| Row | Was | Now |
|---|---|---|
| 11 | Two different insert request types and two different response types | One `api::InsertRecordRequest` / `InsertRecordResponse` deserialised by **both** routers. `values`, `collection`, `text`, `metadata`, `tag`, `request_id` are accepted and honoured on both paths; no field is silently dropped. |
| 12 | `request_id` was a no-op standalone — serde dropped it | Standalone dedup implemented in `valori-engine` (`dedup_lookup` / `dedup_record`), same 16-byte token and same bounded-FIFO semantics as the cluster state machine. Replay returns the original `record_id` and performs no second write. |
| 13 | `filter_tag` sent by the SDK, honoured by nothing | The SDK now **raises** instead of silently no-opping. `tag` is on the canonical insert DTO for both paths. |
| 14 | `k` required standalone, defaulted to 10 on cluster | `k` required on both. `as_of*` documented standalone-only, `consistency` cluster-only, both modelled explicitly. |
| 15 | Cluster returned `{results}` only | Both paths return the same `SearchResponse`; `as_of_*` omitted rather than differently-shaped. |
| 18 | Unknown Collection → 400 standalone / 404 cluster | **404 `collection_not_found`** on both, from one helper (`error_codes::collection_not_found`). The 400/500 "unconfigured collection" fork is unreachable since Phase 3.2. |
| 29 | `{"error": "…"}` only; 401/403 had **no body at all** | Every error response carries `{error, code}`. A response-layer middleware (`error_codes::attach_error_code`) guarantees this even for bodies produced by axum itself, so bare 401/403 are now parseable. |
| 30 | No machine-readable codes | `valori_engine::ErrorCode`, 16 variants, each mapped from a real `EngineError`/`KernelError` variant. `error` remains for backward compatibility; `code` is the field to branch on. |
| 34 | `/v1/cluster/add-node`, `remove-node`, `snapshot` needed only `read_write` | **`admin`**. Contract updated to match. |
| 35 | `/v1/search/multi` and `/v1/graphrag` needed `read_write` | **`read_only`** — they are pure reads that happen to carry a body. |
| 44 | Python SDK defaulted every method to `collection="default"` | Removed. `collection` is required wherever the server requires it; a zero-collection project no longer silently 404s behind a default argument. |
| 49 | Two duplicated hand-written `types/valori.ts` with fabricated fields | Both now consume `@valori/api-types`, generated from the contract by `scripts/generate-api-types.sh`. `ui/src/lib/valori-client.ts` (dead, with a broken `createCollection`) deleted. |
| — | `ui/src/app/api/ingest/route.ts` auto-created the target Collection with `{"name": collection}` and no dimension/metric | Fails fast with an actionable message naming the endpoint to call. No implicit creation anywhere. |
| — | `valori-cli import` derived dimension from `/health.dim` | Dimension comes from the source (Qdrant collection config, or the first JSONL vector). Existing target Collection is validated, never mutated. |
| — | `Daemon::create_collection` forwarded no dimension/metric | Fixed; every Collection-creation call site now supplies both. |

---

## 4. Open divergences

These are **known and unfixed**. Each is a decision, not an oversight.

| Row | Divergence | Why it is still open |
|---|---|---|
| 37 | **`/health` returns two structurally different objects** — standalone `EngineHealth` vs cluster `{status, leader, dim, embed_*}`. One client cannot parse both. | Real P0, out of Phase API-2's stated section list. Unifying it means picking a shape and breaking whichever consumer is on the other one; the UI, the CLI wizard, `docker-compose` healthchecks and the MCP server all read it. Needs its own phase with a migration window. |
| 36 | `ApiKeyRecord.collection` is stored and returned by `GET /v1/keys` but **never enforced** by either auth guard. | A field that looks like a tenancy boundary and is not. The fix is either enforcement (a real feature) or removal (a breaking response change). Both are out of scope for an API-stabilisation phase. |
| 27 | `/v1/storage/*` returns **400** when the object store is unconfigured; it should be **501** (the caller is not at fault). | Behaviour change to a shipped status code. The contract documents the current 400 and marks the intended 501 in prose. |
| 9 | `IndexStatusResponse.status` is a **derived** string, not the raw `IndexState`; `ready`/`retiring` surface only through the building-generation branch. | Exposing the raw generation list is a schema addition, not a stabilisation. Contract documents the derivation. |
| 7 | Two enums for one concept — `IndexKind` (creation: `brute\|hnsw\|ivf\|bq\|auto`) vs `IndexType` (lifecycle: `hnsw\|ivf\|bq\|null`). `auto` is accepted at creation and rejected on the lifecycle endpoint. | Converging them changes accepted input on a live endpoint. Both enums are modelled explicitly in the contract so no client is surprised. |
| 42 | Tree / community / entity-extraction endpoints take **`namespace`**; everything else takes **`collection`**. | Renaming is breaking. `namespace` is marked deprecated in favour of `collection` with the successor named; removal is a v2 item. |
| 45/46 | SDK `list_contradictions()` / `resolve_contradiction()` call a Next.js UI route; `set_index()` POSTs a value the endpoint ignores. | Already `DeprecationWarning`-flagged. Deleting public SDK methods is a breaking SDK change, sequenced with the SDK-generation phase. |
| 28 | `/v1/operations/:id` and `/v1/operations/:id/execution` serve **two different id spaces** on one path prefix. | Splitting them is a v2 path change. |
| 40 | `GET /v1/operations` and `GET /v1/timeline` are unbounded reads over the whole event log. | Adding pagination is a feature, and §38 forbids new features in this phase. Recorded as a scaling hazard. |
| 5 | The path segment is `/v1/namespaces`; every body, SDK and document says *Collection*. | Renaming the path is a v2 breaking change. The contract uses the real path with the `Collections` tag and `create_collection`/`list_collections`/`delete_collection` operationIds. |

---

## 5. Intentionally unsupported

Not gaps. Do not "fix" these.

* **Cross-collection graph edges.** Graph is Collection-scoped by design.
* **Per-collection snapshots.** Snapshots are node-scoped; the contract does
  not invent a per-Collection family.
* **Metrics other than `squared_l2`.** One metric exists; the enum has one
  member. No cosine/dot/inner-product placeholder.
* **`429 Too Many Requests`.** No rate limiting exists, so none is documented.
  A full record pool is `507`, which is unusual but honest.
* **Generic async operations.** Exactly two things are asynchronous (index
  build, `POST /v1/ingest?async=true`). `/v1/operations` is an audit-log
  reader, not a job registry.
* **`GET /metrics` and `GET /v1/replication/*` in any SDK.** Marked
  `x-sdk: false` — a Prometheus scrape target and an internal node-to-node
  protocol respectively.
* **The embedded PyO3 engine (`valoricore/local.py`).** The contract governs
  the **remote** HTTP path only. The two have never been proven
  feature-equivalent and the contract must not imply they are.

---

## 6. Related

* [`api/README.md`](../../api/README.md) — ownership, versioning, deprecation rules
* [`docs/api/current-vs-target.md`](./current-vs-target.md) — the 52-row gap analysis with the Phase API-2 resolution log
* [`docs/api/api-inventory.md`](./api-inventory.md) — every route in every router
* [`docs/api/ui-parity.md`](./ui-parity.md) — TypeScript-layer drift
* [`docs/phases/phase-api-contract-2-convergence.md`](../phases/phase-api-contract-2-convergence.md)
