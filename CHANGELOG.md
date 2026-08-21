# Changelog

All notable changes to Valori are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Phase API-4D — Python SDK Parity, Type-Safety Hardening & Real-Node Validation (2026-08-21)

Fixed the Python `metadata` wire bug API-4C found by inspection, removed the casts
that made that class of bug possible, and validated **both** SDKs against a real
node from a reproducible disposable environment. Contract gate **PASS**,
`SDK READY = YES`, 74 operations. **Nothing published.**

#### Fixed
- **Python `metadata` was encoded wrong on both write paths** — the same bug
  API-4C fixed in TypeScript. `records.insert` sent a JSON map where
  `POST /v1/records` requires opaque UTF-8 JSON **bytes** (`list[int]`), and
  `records.insert_batch` sent maps where `POST /v1/vectors/batch-insert`
  requires UTF-8 JSON **strings**. The generated model's permissive `from_dict`
  accepted the mapping and passed it straight through. Now encoded at the
  domain→wire boundary by a new centralised `valori/_wire.py`, so callers still
  pass a plain dict.
- **Cross-SDK byte identity.** The Python encoder matches `JSON.stringify`
  exactly (no whitespace, real UTF-8, insertion order). `POST /v1/records`
  commits metadata bytes *inside* the `InsertRecord` event, so they are covered
  by the BLAKE3 audit chain — a divergence would give the same logical write two
  different state hashes depending on which SDK issued it.
- **Contract bug: scalar metadata values were unrepresentable.** Five schema
  fields were annotated `HashMap<String, Object>`, rendering as
  `additionalProperties: {type: object}` — "every value must be an object" —
  while the Rust type is `serde_json::Value` and the field's own doc comment
  claims generators should emit `Dict[str, Any]`. `update_metadata(7, {"a": 1})`
  raised `TypeError`. Fixed at the Rust source in `valori-node/src/api.rs` and
  `server.rs`; contract and both `generated/` trees regenerated.
- **Two TypeScript type holes**, exposed by removing the casts: `collections.create()`
  took `metric: string` / `index?: string` and `index.build()` took `type?: string`,
  where the contract has closed enums. `metric: "cosine"` — not a contract metric —
  compiled, and two committed tests were passing it.
- **Three long-broken Python integration tests**: `request_id` must be 32 hex
  characters, `CreateNodeResponse` exposes `node_id` (not `id`), and a 404 from
  `GET /v1/operations/{id}/execution` is contract-valid.
- **`.github/workflows/sdk-python.yml`** started its integration node without
  `VALORI_EVENT_LOG_PATH` — API-4C fixed this only on the TypeScript side, so the
  Python proof cases were testing an error path.

#### Changed
- **All 13 `as unknown as` casts removed** from `sdk/typescript/src/`. Public
  signatures now use string unions *derived from* the generated enums, so callers
  still write `"hnsw"` but `"hsnw"` is a compile error. TypeScript enums are
  nominal, so one bridge remains — isolated in a single documented `asEnum`
  helper with runtime membership validation that makes the assertion sound and
  throws before any request is made.
- Python `metadata_filter`, memory upsert/consolidate and both metadata-sidecar
  paths now route through the wire layer so they are validated at the boundary
  rather than coerced by a permissive `from_dict`.
- Both SDK CI workflows now start their node via one shared harness instead of
  each carrying an inline snippet.

#### Added
- `scripts/sdk-integration-node.sh` — disposable node for the integration suites:
  free port, throwaway storage root, every `VALORI_*` variable set explicitly,
  health wait, guaranteed teardown. Never touches a developer's own node.
- `sdk/metadata-wire-fixtures.json` — 12 canonical metadata cases with their exact
  wire forms, read by **both** test suites so cross-SDK parity is mechanical.
- `docs/api/known-server-issues.md` — **`metadata_filter` is broken in the server**
  (confirmed with raw curl, no SDK involved): it consults only the metadata
  sidecar keyed `rec:{id}`, so a predicate that exactly matches a record's
  committed metadata returns zero hits. Deliberately **not** worked around in
  either SDK; both integration suites pin the broken behaviour so a server fix
  cannot land silently.
- `docs/sdk/release-readiness.md` — npm/PyPI verification results and the exact
  trusted-publishing/OIDC configuration Phase API-4E must create.
- `sdk/python/tests/test_metadata_wire.py` (46 cases),
  `sdk/typescript/tests/enum-boundary.test.ts` (8),
  `sdk/typescript/tests/wire-parity.test.ts` (26), and metadata round-trip
  integration tests in both SDKs that read the value back and compare semantically
  rather than asserting HTTP 200.
- `publishConfig` on `@valori/sdk` (`access: public`, required for a scoped
  package; `provenance: true`).

#### Validation
Python **307 unit + 18 integration**; TypeScript **223 including integration**
(3 cluster-only skipped in each). Both suites were also run against the **same
node process** in one harness invocation. `tsc --noEmit` clean, 0
`as unknown as` remaining. Both packages build, install and typecheck from their
published artifacts; `twine check` PASSED. **Nothing published, no registry
credential created.**

### Phase API-4C — Official Valori TypeScript SDK (2026-08-21)

Validated the `@valori/sdk` TypeScript package against a **real running node** for
the first time and closed the remaining ergonomic gaps. No REST API change; the
contract remains 74 operations, gate PASS.

#### Fixed
- **`metadata` was encoded wrong on both write paths.** `Records.insert` sent a JSON
  map where `POST /v1/records` requires opaque UTF-8 JSON **bytes** (`number[]`), and
  `Records.insertBatch` sent an array of maps where
  `POST /v1/vectors/batch-insert` requires UTF-8 JSON **strings**
  (`(string | null)[]`). Both are rejected with a 422 by a live node. The ergonomic
  layer now encodes at the domain→wire boundary, so callers still pass a plain
  object. The bug was masked by the `as unknown as X` casts the wrapper bodies use
  to reach the generated types.
- **`sdk/typescript/README.md` documented an SDK that does not exist** — wrong
  constructor options (`baseUrl`/`token` vs `endpoint`/`apiKey`), wrong method shapes
  on every call in the quickstart, and a "Zero Hand-Written Types" claim that is the
  opposite of the architecture. Rewritten against the real surface.
- **`.github/workflows/sdk-typescript.yml`** started the integration node without
  `VALORI_EVENT_LOG_PATH`, so `GET /v1/proof/event-log` answers "Event log not
  enabled" and the proof case could never have passed. Now set.
- Two integration tests encoded false beliefs about the contract: `request_id` must
  be 32 hex characters (not a free-form string), and `GET /v1/operations/{id}/execution`
  correctly 404s for operations the planner never ran.

#### Added
- **§21 per-call abort and timeout** — `CallOptions.timeoutMs`, honoured by
  `Transport.params()`, and a trailing `CallOptions` argument on
  `Collection.search()` and `Collection.graphrag()`.
- **§22 pagination** — `Graph.listAllNodes()`, an async iterator over
  `GET /v1/graph/nodes`. A contract scan confirmed this is the only operation with
  true offset/limit pagination; nothing else got an iterator.
- **`sdk/typescript/examples/quickstart.ts`** — a runnable end-to-end tour, added to
  `tsconfig.json` `include` so it is typechecked rather than decorative.
- `tests/call-options.test.ts` (8 cases) and a batch-metadata encoding case.

#### Known issue
- The **Python SDK has the identical `metadata` bug**, confirmed by inspection in
  `sdk/python/handwritten/valori/resources/records.py`. Not fixed in this
  TypeScript-scoped phase; owned by the next one.

Validation: **186 tests pass against a live node** (171 unit + 18 integration, 3
cluster-only skipped), typecheck clean, coverage 74/74, generation byte-reproducible,
`npm run build` and `npm pack --dry-run` clean. **Not published.**

### Phase API-3.3 — Public Operation Contract Completeness & SDK Preflight (2026-08-20)

Phase API-3.2 proved the right 74 operations exist. This phase proves their HTTP
contracts are **complete** enough to generate a high-quality multi-language SDK.
**Verdict: `SDK READY = YES`, 0 computed blockers.** No route was added or
removed; the operation count is unchanged at 74.

#### Added
- **`scripts/audit-public-api-operations.py`** — audits every public operation
  for contract completeness, cross-checking the OpenAPI document against the
  **Rust handler signature**. Completeness is never inferred from path
  existence: a `requestBody` counts only if the contract declares one *and* the
  handler has a body extractor, and a `query?: never` only if the handler has no
  `Query<..>`. Emits `docs/api/public-operation-audit.{json,md}` and exits
  non-zero on any incomplete operation.
- **Step `3b/9` "Public operation completeness"** in
  `scripts/api-contract-gate.sh`, plus `OPERATION COMPLETENESS` (13 figures) and
  `GENERATED CLIENT QUALITY` (4 figures) reporting blocks. All discovered per
  run; a missing artifact reports `UNKNOWN` and blocks rather than defaulting.
- **Typed DTOs replacing `body = Object` / `Vec<Object>` placeholders**:
  `Receipt` + `ReceiptFragment` (the flagship proof artifact, previously an
  opaque blob in every SDK), `IngestJobStatusResponse` + `IngestJobState`,
  `GraphRagHit`, `SubgraphNode`, `SubgraphEdge`, `OperationOverview`,
  `OperationResults`, `OperationMetrics`, `OperationDetails`,
  `IndexBuildParameters`, `BuildableIndexKind`, `SnapshotBytes`.
- `docs/api/typescript-contract-quality.md` — every `unknown`/`any`/`never` in
  the generated TypeScript classified EXPECTED or BUG. Zero BUG-class.

#### Fixed
- **401/403 declared an empty body on all 73 authenticated operations — 146
  wrong responses.** Phase API-3.2 attached them bodyless, reasoning that axum
  renders a bare `StatusCode` with no body. True of the auth guard alone, false
  of the router: `attach_error_code` is the **outermost** layer on both routers
  and synthesises a full `ApiError` for any empty error body. An existing
  passing test already proved it. They now declare `body = ApiError`.
- **The 16 remaining bodyless error responses**, via a new `ErrorBodyAddon`
  `Modify` pass — the contract-side mirror of `attach_error_code`, so the two
  cannot drift. This also reached `/v1/tree/*` and `/v1/ingest/document`, which
  are annotated in `valori-rag` / `valori-ingest` and cannot name `ApiError`
  (it is declared in `valori-node`), making per-call-site edits impossible.
- **`GET /v1/crypto/status/{key_id}` returned `text/plain` on 400** — the one
  error in the public surface escaping `ApiError`, since the middleware passes
  non-JSON bodies through by design, and a fork from its cluster twin, which
  already answered JSON. Converged onto `error_response`.
- **`GET /health` and `GET /v1/cluster/health` had their 503 body corrupted at
  runtime.** Both answer 503 with a full typed health document; the error
  middleware mapped 503 → `Unavailable` and spliced `error` and `code` into the
  documented DTO, so the bytes did not match the advertised schema. Both paths
  are now exempt — a 503 health report is a status signal, not an error.
- **`GET /health` advertised `x-required-scope: read_only`** despite declaring
  `security: []`. The middleware never runs there, so the value was the scope
  function's default — telling every SDK that the one deliberately open endpoint
  needed a key. The extension is now emitted only for authenticated operations.
- **`/v1/snapshot/{download,upload}` typed binary payloads as
  `array<integer>`.** `body = Vec<u8>` rendered literally, so a generated Python
  client typed the download as `list[int]`. Now `type: string, format: binary`
  → `File` / `Blob`.
- **`IndexBuildRequest` was effectively undocumented**: `parameters` had no
  schema at all (the only genuinely untyped field in the surface) and `type` was
  a bare `string`. Now `IndexBuildParameters` (the five keys both routers
  actually read) and the closed `BuildableIndexKind` enum.
- `MetadataSetRequest.metadata`, `GraphRagHit.metadata`,
  `OperationDetailResponse.proof` and the PATCH-metadata body now emit
  `additionalProperties`, rendering as `Record<string, unknown>` /
  `Dict[str, Any]` instead of a property-less `object`.
- `scripts/verify-api-route-contract.py` no longer blanket-exempts `401`/`403`
  from the empty-body check — the exemption rested on the same false premise as
  the bug it should have caught.

#### Changed
- `api_contract.rs::every_operation_documents_the_scope_the_server_enforces` now
  asserts a conditional invariant: authenticated ⇒ scope matches
  `required_scope()`; unauthenticated ⇒ **no** scope declared.
- New `api_contract.rs::receipt_dto_matches_the_runtime_receipt` diffs the
  hand-written `ReceiptDto` against a serialised `valori_effect::Receipt` so the
  mirror cannot drift.

#### Validation
74/74 operations complete; 0 untyped parameters, 0 parameter mismatches, 0
untyped schema properties, 0 unexpected `unknown`/`any` in the generated
TypeScript. `cargo test -p valori-node` 449 passed; kernel 83, engine 18, state
24, storage 78, consensus 32; pytest 101 passed. Clippy 0 errors. wasm32 build
clean (`no_std` intact). Throwaway Python (`openapi-python-client`) and
TypeScript (`swagger-typescript-api`) clients generated and inspected — both
PASS; the TS client compiles under `tsc --strict`. Contract gate PASS, exit 0.

### Phase API-3.2 — Final API Contract Readiness Gate (2026-08-20)

Audits the *content* of the 74 operations Phase API-3.1 aligned, rather than
their coverage. **Verdict: `SDK READY = NO`, 2 computed blockers.** No route was
added or removed; the operation count is unchanged at 74.

#### Fixed
- **`401` documented a body that has never existed.** 70 handler annotations
  declared `401` with `body = ApiError`, but `auth_guard_v2` returns a bare
  `StatusCode::UNAUTHORIZED`, which axum renders with an empty body. A client
  trusting the contract would have tried to parse an empty body as JSON on every
  expired token.
- **`403` is now documented.** It is reachable on all 73 authenticated
  operations whenever a key's scope does not satisfy `x-required-scope`, and was
  documented on none of them.
  Both responses are now attached by a single `AuthResponsesAddon` modifier in
  `crates/valori-node/src/openapi.rs`, from the one place that knows the
  middleware's behaviour, instead of being restated per handler.
- **`POST /v1/ingest` `202` had no documented body** despite returning
  `job_id` — the value the entire async flow depends on. It now carries
  `IngestAcceptedResponse`. `413` and `500` are documented on `/v1/ingest` and
  `/v1/ingest/update`; the `400` description no longer claims to cover oversize
  text, which is a `413`.
- **Ingest errors now use the canonical `ApiError` shape** via
  `valori_engine::error_response`, gaining the `code` field (additive). The
  `/v1/ingest/update` error paths previously serialized a raw `Vec<u8>` with an
  empty header map, so they were served as `application/octet-stream` rather
  than JSON.
- **`redocly.yaml` no longer weakens `operation-4xx-response` for the whole
  document.** `GET /health` — the one operation that legitimately has no 4xx —
  is pinned in a new `.redocly.lint-ignore.yaml`, and the rule is enforced at
  full strength by `scripts/verify-api-route-contract.py`. Lint is clean with no
  fabricated `400`.

#### Changed
- **`CreateCollectionRequest` now requires `name`, `dimension`, and `metric`**
  in the schema, matching what `parse_collection_config` has enforced since
  Phase 3.3. The stale "required except for `default`" wording is gone —
  `"default"` has no exception.
- **`metric` and `index` are closed enums**, no longer bare strings. The
  accepted set and the emitted set are separate schemas
  (`MetricInput`/`Metric`, `IndexKindInput`/`IndexKind`) because `FromStr`
  accepts `l2`, `l2sq`, `bruteforce`, and `mstg` while `as_str` never emits
  them. Every previously-valid value remains valid; the wire is unchanged.
  Both of the above are breaking for a *regenerated SDK's* method signatures
  and non-breaking on the wire — corrections to a contract that under-specified
  what the server already enforced.
- Schemas: 138 → **143**.

#### Documentation
- `docs/api/non-public-routes.md` **regenerated from the route manifest.** The
  previous inventory was wrong: it listed `/v1/storage/*`, `/v1/snapshot/*`, and
  `/v1/memory/*` as non-public when all 13 are `PUBLIC_SDK`, and missed the real
  `OPERATOR_INTERNAL` and `DEPRECATED` sets.
- `docs/api/security-contract.md` corrected — ten *public* operations require
  `admin` scope (`/v1/snapshot/*`, `/v1/storage/*`), which the previous version
  denied.
- `docs/api/operation-id-policy.md` corrected (`delete_collection`, not
  `drop_collection`) and expanded with the naming rules.
- `docs/api/x-status-decision.md` rewritten: `x-status` is documentation-only
  and deliberately **not** reintroduced; the two facts it conflated are owned by
  `deprecated: true` and the route manifest. Also records that `x-sdk` is a
  constant `true` and carries no information.
- `docs/api/sdk-readiness.md`, `docs/api/openapi-version-decision.md`,
  `api/README.md` updated.

#### Known blockers (deferred to Phase API-3.3)
- **16 documented responses declare no body while the handler returns JSON**, so
  a generated SDK sees `never` and cannot surface the error message.
- **Root cause: two error shapes in the runtime.** ~126 sites across
  `server.rs` and `cluster_server.rs` emit a bare `{error}` with no `code`
  instead of `ApiError`, and `GET /v1/crypto/status/{key_id}` returns a plain
  string. `crates/valori-node/src/ingest.rs` was converged as the reference
  pattern; the rest is too large to fix surgically here.

#### Tooling
- `scripts/verify-api-route-contract.py` now also enforces 4xx coverage and
  response-body typing against a source-reviewed allowlist.
- `scripts/api-contract-gate.sh` surfaces an "Untyped JSON responses" count and
  raises it as a computed blocker. Readiness remains calculated, never
  hand-written: `docs/api/sdk-readiness.json` now reports
  `"sdk_ready": false, "blocker_count": 2`.

### Phase API-3.1 — Complete Public Utoipa Coverage & Contract Convergence (2026-08-20)

Finishes the code-first migration the recovery phase below scoped. The
canonical contract is now generated end-to-end from Rust.

- **All 74 public routes annotated.** The 63 handlers that had no
  `#[utoipa::path]` now carry one, with a real request body, per-status
  responses, and a `BearerAuth` requirement, and every one is registered on
  `ValoriApi`. Three-way equality holds: **Rust public routes (74) == utoipa
  operations (74) == OpenAPI operations (74)**, 0 discrepancies — down from 79
  at the start of the phase.
- **`api/openapi/valori-v1.yaml` is now the generator's byte-exact output.**
  `tests/openapi_generated.rs` asserts byte equality, replacing a
  subset-with-allowlist check that a hand-written superset could have passed.
  Schemas: 26 → **138**. Write operations documenting a request body: 4 of 40
  → **37 of 38**. Distinct response descriptions: 2 → 60+.
- **OpenAPI target is now 3.1.0**, not 3.0.3. utoipa 5.5.0 cannot emit 3.0.x by
  construction, and `openapi-typescript@7` — the generator this repo runs — is
  3.1-first. Rationale, alternatives, and the revisit condition:
  `docs/api/openapi-version-decision.md`. No HTTP-surface change.
- **Admin routes removed from the public contract.** `/v1/keys*`,
  `/v1/crypto/shred/{key_id}`, and `/v1/cluster/{add-node,remove-node,snapshot}`
  are still served by the node; they are simply not part of the SDK surface.
- **Two SDK paths added.** `POST /v1/memory/search_vector` and
  `/v1/memory/upsert_vector` — the spellings `python/valoricore` actually calls
  — were missing from the contract entirely.
- **operationIds are declared in Rust**, once, in the handler's own annotation;
  the route manifest reads them rather than deriving them from a function name.
  Net churn versus the previously published contract: **0**.
- **`x-required-scope` / `x-sdk` restored**, stamped by a Rust `Modify` pass
  that reads `api_keys::required_scope` — the same function the auth middleware
  calls, so the contract cannot document a scope the server does not enforce.
  A new test checks all 74, not a sample.
- **Generation is byte-reproducible.** Rendering through `serde_json::Value`
  removes the `HashMap`-ordering nondeterminism that made consecutive runs
  differ.
- **Cross-crate `utoipa` features** (optional, default-off) on `valori-engine`,
  `valori-rag`, `valori-ingest`, `valori-models`, `valori-storage`, so the
  contract references the same type the handler serialises instead of a mirror
  that can drift. `valori-kernel` is untouched and remains `no_std`.
- **Fixes found on the way.** Cluster `/health` had silently dropped the
  top-level `leader` and `dim` fields the UI still reads — restored. Five DTOs
  added by the retracted Phase API-3 described an index-lifecycle model no
  handler implements — deleted in favour of the real
  `valori_engine::index_manager::IndexStatusResponse`. A self-referential
  tree-RAG type sent the schema builder into unbounded recursion.
- **Contract gate: PASS, 11/11 steps. `SDK READY = YES`, 0 computed blockers**
  (`docs/api/sdk-readiness.json`, written by the gate, never by hand).
  SDK generation itself remains deliberately out of scope.


### Phase API-3 Recovery — Genuine Code-First Utoipa Architecture (2026-08-20)

Corrects the retracted Phase API-Contract-3 entry below.

- **Honest route discovery.** `scripts/generate-route-manifest.py` derives the
  route inventory from the axum router source in `server.rs`,
  `cluster_server.rs`, `cluster_api.rs`, and `routes/**`. It never reads
  `api/openapi/valori-v1.yaml`, never emits OpenAPI, and exits non-zero on any
  router construct it cannot resolve. Real surface: **100 routes** (74 public
  SDK, 14 deprecated, 7 admin, 5 operator-internal) — not the 75 previously
  claimed.
- **Three-way contract verifier.** `scripts/verify-api-route-contract.py`
  diffs Rust public routes vs live utoipa operations vs the committed contract
  on method, path, operationId, and classification. Verification only.
- **First real `#[utoipa::path]` annotations.** 11 operations across 10 paths on
  the registered handlers, each with a genuine request body, per-status
  responses, and a `BearerAuth` requirement. `ValoriApi` now has a real
  `paths(...)` list and a `SecurityAddon` modifier.
- **Contract gate reports measured numbers.** `scripts/api-contract-gate.sh`
  no longer prints a `$TOTAL/$TOTAL` tautology or a hardcoded fallback, and
  computes `docs/api/sdk-readiness.json` from step outcomes.
- **SDK READINESS: NO** — 5 computed blockers, chiefly that 63 of 74 public
  routes are still unannotated and the committed contract is not yet the
  generator's output.

### Phase API-Contract-3 — RETRACTED (2026-08-20)

This entry claimed 100% code-first generation and `SDK READINESS: YES`. Both
were false: the workspace contained zero `#[utoipa::path]` annotations, the
generator emitted zero paths, the contract was reconstructed by an uncommitted
script, and `sdk_ready` was hand-written. See
`docs/phases/phase-api-3-recovery.md`.

### Added (Phase API-2.5 — Conformance Diff Review & Pre-SDK Gate — 2026-08-20)

- **Official API Contract Gate (`scripts/api-contract-gate.sh`).** Single permanent executable pipeline enforcing Utoipa subset generation, OpenAPI linting, route parity, TypeScript type generation, zero git diff on generated artifacts, and Python SDK compatibility.
- **Working-Tree Forensics & Diff Audit (`docs/api/phase-api-2.5-diff-audit.md`).** Classified all 328 working-tree files relative to baseline `eee123d`. Category F ("Cannot determine") count reached 0. Deep-audited API-2 changes (31 files), dependencies (96 files), and isolated 11 suspicious/unrelated platform files (Index Manager, Snapshot v8, Graph/Storage manifests) without deletion or modification.
- **API-2 Claim Verification Matrix (`docs/api/api-2-verified.md`).** Re-verified all 11 claims of Phase API-2 against source implementation and test suite with explicit regression risk ratings.
- **Utoipa Migration Matrix & Generator Reproducibility (`docs/api/utoipa-migration-matrix.md`).** Documented Utoipa generated subset (14 schemas, 0 paths) vs canonical hand-maintained contract (102 schemas, 79 paths), metadata preservation strategy, and `@valori/api-types` wire model isolation.
- **Contract Governance Policy (`docs/api/contract-gate.md`).** Documented breaking vs non-breaking contract drift budget rules.
- **Domain SDK Readiness Matrix & Pre-SDK Gate Verdict (`docs/api/sdk-readiness.md`).** Issued formal **SDK READY = NO** verdict, detailing explicit blockers for Phase 3 and Phase 4.
- **Documentation-Only Analysis Items.** Created `docs/api/health-migration.md` (/health shape divergence) and `docs/api/api-key-scope.md` (ApiKeyRecord collection authorization) as analysis items without modifying runtime code.

### Added (Phase API-2 — API contract convergence — 2026-08-20)

- **Machine-readable error codes.** Every error response on every route now
  returns `{"error": "<human string>", "code": "<stable code>"}`. `code` is
  drawn from a closed 16-variant set (`validation_error`, `unauthorized`,
  `forbidden`, `not_found`, `collection_not_found`, `record_not_found`,
  `dimension_mismatch`, `invalid_metric`, `invalid_index`,
  `index_build_failed`, `conflict`, `capacity_exceeded`, `not_leader`,
  `unavailable`, `not_implemented`, `internal_error`), each mapped from a real
  `EngineError`/`KernelError` variant. `error` is unchanged — this is
  additive. **Branch on `code`, not on the message.** `401`/`403` now carry a
  parseable JSON body; previously they had none at all.
- **`request_id` idempotency on standalone.** `POST /v1/records` and
  `POST /v1/vectors/batch-insert` honour a client idempotency token on the
  standalone path, not only in cluster mode. Replaying a token returns the
  record the first request created and performs no second write. Both wire
  spellings are accepted — a 16-byte array or a 32-character hex string with
  optional UUID dashes. A malformed token is now an error rather than a
  silently ignored field.
- **`@valori/api-types`** — internal TypeScript workspace package generated
  from `api/openapi/valori-v1.yaml` by `scripts/generate-api-types.sh`.
  Consumed by both `ui/` and `ui/studio/`; neither keeps a hand-written copy
  of the wire model any more.
- **Code-first OpenAPI generation (partial).** `cargo run -p valori-node
  --features utoipa --bin valori-openapi` emits a generated document covering
  16 schemas (Collections, Records, Search, Multi-Search, Errors).
  `tests/openapi_generated.rs` fails the build if a Rust DTO drifts from the
  committed contract. 90 of the 102 committed schemas and all 79 path items are
  still hand-maintained — see `docs/api/contract-conformance.md`.
- **`docs/api/contract-conformance.md`** — per-domain implementation status,
  resolved and open divergences, and the intentionally-unsupported list.

### Changed (Phase API-2)

- **`POST /v1/records` accepts one canonical request body on both paths.**
  Standalone previously accepted `{values, collection, text}` and cluster
  `{values, collection, metadata, tag, request_id}`, each silently dropping
  the other's fields. All six are now accepted and honoured everywhere.
- **`k` is required on `POST /v1/search` in cluster mode.** It previously
  defaulted to 10 there and was required on standalone. **Potentially
  breaking** for a cluster client that omitted `k`.
- **Unknown Collection returns `404 collection_not_found`** on
  `POST /v1/search/multi` in standalone mode; it previously returned `400`
  while cluster returned `404`.
- **`POST /v1/cluster/add-node`, `/v1/cluster/remove-node` and
  `/v1/cluster/snapshot` now require an `admin` key.** They previously
  accepted `read_write`, so any writer key could reconfigure cluster
  membership. **Breaking** for automation using a read-write key.
- **`POST /v1/search/multi` and `POST /v1/graphrag` now require only
  `read_only`.** A read-only key previously could not run a cross-collection
  or GraphRAG query.
- **Python SDK: no implicit `collection="default"`.** Every
  `SyncRemoteClient`/`AsyncRemoteClient` method that targets a Collection now
  requires one. A new project has zero Collections, so the old default turned
  every call into a 404 behind an argument the caller never typed.
  **Breaking** for code relying on the default. `filter_tag` now raises
  instead of silently doing nothing — no server request type has ever carried
  it.
- **UI ingest no longer auto-creates the target Collection.** It fails fast
  with an actionable message instead of POSTing `{"name": collection}` with no
  dimension or metric.
- **`valori import` derives the target dimension from the source**, not from
  `/health.dim`. An existing target Collection is validated against it and
  never mutated.

### Removed (Phase API-2)

- `ui/src/lib/valori-client.ts` — dead code with a `createCollection` that
  sent no dimension or metric. It had no importers.

### Added (Phase API-1 — Canonical OpenAPI v1 contract + API audit — 2026-08-20)

- **`api/openapi/valori-v1.yaml`** — the canonical public REST contract for the
  Valori **data plane**. OpenAPI 3.0.3; 75 paths, 79 operations, 101 reusable
  schemas, one `bearerAuth` security scheme. Every operation carries
  `x-status` (all `current` — nothing aspirational is presented as live),
  `x-required-scope`, and `x-cluster-status` / `x-standalone-status` where a
  mode does not implement it. Validates clean under
  `npx @redocly/cli lint` (0 errors).
- **`api/README.md`** — contract ownership, validation commands, the
  control-plane/data-plane boundary, breaking vs non-breaking vs deprecation
  rules, the embedded-`.so`-vs-remote-SDK separation, and the *documented but
  not implemented* future SDK-generation and gRPC architecture.
- **`docs/api/api-inventory.md`** — every externally reachable route across
  `valori-node` (standalone + cluster), `cluster_api.rs` and `valori-daemon`,
  with auth scope, standalone/cluster support, request/response detail, real
  status codes, idempotency, consistency and pagination behaviour.
- **`docs/api/current-vs-target.md`** — 52-row severity-ranked gap analysis
  with a P0-first Phase-2 priority order.
- **`docs/api/ui-parity.md`** — TypeScript/UI drift against the contract.
- **`docs/phases/phase-api-1-contract-audit.md`** — phase report.

Audit-only: no route, request struct, response struct or status code was
changed; no SDK was generated; no protobuf or gRPC was introduced. The
deprecated legacy path aliases (`/records`, `/search`, `/graph/*`,
`/timeline`, `/operations`, `/version`, `/v1/vectors/batch_insert`), the
internal replication endpoints and `GET /metrics` are deliberately excluded
from the contract and inventoried separately.

### Added (Phase 5.4 — GraphRAG Graph-Aware Reranking + Traversal Budgeting — 2026-08-19)

- **`graph_weight` request field** on `POST /v1/graphrag`: β coefficient in the combined
  ranking formula `final_score = (1-β)×vector_relevance + β×graph_relevance`. Range [0.0, 1.0];
  default 0.3 (vector-dominant). At `graph_weight=1.0` the ranking is purely graph-based and
  graph-only candidates can outrank pure vector hits with no graph node.
- **`graph_score` hit field** on `POST /v1/graphrag`: normalised graph relevance ∈ [0, 1]
  present on every hit (`0.0` for no-graph vector hits, `1.0` for seeds at distance 0,
  `1/(1+N)` for graph-only candidates at hop N). Always numeric (never null).
- **`final_score` now always numeric**: Phase 5.3 left `final_score: null` for graph-only
  hits. Phase 5.4 computes `final_score = β × graph_relevance` for graph-only candidates,
  making all hits comparable in one sorted list.
- **Unified sorted hit list**: all candidates (vector and graph-only) are merged and sorted
  by `final_score` descending, `record_id` ascending as tie-breaker. Phase 5.3 separated
  vector hits and graph-only hits into two appended buckets; Phase 5.4 merges them.
- **`max_nodes` request field** on `POST /v1/graphrag`: halt BFS before visiting more than
  this many nodes (enforced inside `expand_subgraph_budgeted`). `None`/absent = unlimited.
- **`max_edges` request field** on `POST /v1/graphrag`: halt edge emission once this count
  is reached per BFS traversal. `None`/absent = unlimited.
- **`expand_subgraph_budgeted`** (`valori_rag::graph`): new function with `max_nodes` and
  `max_edges` parameters. `expand_subgraph` is now a wrapper calling it with `None, None`.
  No existing call sites changed.
- **`final_k` defaults to `retrieval_k`**: absent `final_k` now defaults to `retrieval_k`
  (was unlimited in Phase 5.3). A request with `retrieval_k=5` returns at most 5 hits
  unless `final_k` is explicitly set larger.

### Changed (Phase 5.4)

- **Hit sort order**: hits are now sorted by `final_score` DESC (higher = better), not by
  L2 distance ascending then graph distance ascending. The backward-compat `score` field
  still carries the raw L2 distance.
- **`final_k` default**: changed from unlimited (`None`) to `retrieval_k`. Callers that
  relied on graph-only candidates being returned beyond `retrieval_k` must now pass an
  explicit `final_k`.
- **Python SDK** (`graphrag` on all 4 clients): `max_nodes`, `max_edges`, `graph_weight`
  optional parameters added. `final_k` doc updated to note the new default behaviour.

### Added (Phase 5.3 — GraphRAG Semantic Hardening and Contract Finalization — 2026-08-19)

- **`retrieval_k` request field** on `POST /v1/graphrag`: explicit name for vector seed count
  (how many ANN candidates become graph expansion seeds). Legacy `k` field continues to work
  as an alias — existing clients require no changes.
- **`final_k` request field** on `POST /v1/graphrag`: optional cap on returned hits. When
  absent all candidates are returned (same as Phase 5.2 behaviour). Example: `retrieval_k=20,
  final_k=10` expands from 20 seeds but returns at most 10 results.
- **`max_graph_candidates` request field** on `POST /v1/graphrag`: optional budget on
  graph-only candidates before `final_k` is applied. Default 100. Sorted by `graph_distance`
  ascending before truncation so the closest graph neighbours are preferred.
- **`vector_score` and `final_score` hit fields** on `POST /v1/graphrag`: explicit type-safe
  names alongside backward-compat `score`. `vector_score` = L2 distance for vector hits,
  `null` for graph-only. `final_score` = `vector_score` until a reranker lands. `score`
  continues to carry the same value for backward compat.
- **Deterministic graph-only candidate ordering**: graph-only candidates are now sorted
  ascending by `graph_distance`, then ascending by `record_id` as a tie-breaker. Ordering
  is no longer dependent on HashMap iteration or BFS discovery order.
- **Minimum graph-distance guarantee**: when a record is referenced by multiple graph nodes
  at different hop counts, `graph_distance` now reports the shortest discovered path. Phase
  5.2 used `HashSet::insert` (first-seen wins) which could report a longer path when a
  shorter-distance node was encountered later; replaced with `HashMap<record_id, min_dist>`.
- **Python SDK** (`SyncRemoteClient`, `AsyncRemoteClient`, `SyncClusterClient`,
  `AsyncClusterClient`): `graphrag()` method gains optional `retrieval_k`, `final_k`, and
  `max_graph_candidates` parameters with full docstrings documenting Phase 5.3 semantics.

### Added (Phase 5.2 — GraphRAG Query Orchestration and Retrieval Composition — 2026-08-19)

- **`source` field on all `POST /v1/graphrag` hits**: `"vector"` (no graph node),
  `"vector_and_graph"` (vector hit with graph node), or `"graph"` (record reached only
  via graph expansion, not in top-k vector results). Additive — existing clients ignore
  the new field.
- **`graph_distance` field on all `POST /v1/graphrag` hits**: `0` for seed nodes,
  hop count `N` for graph-expanded records, `null` for vector-only hits with no graph node.
- **Graph-only candidates in `hits`**: records referenced by expanded subgraph nodes that
  were NOT in the vector top-k now appear in `hits` with `score: null`, `source: "graph"`,
  and `graph_distance: N`. Deduplication ensures each record appears at most once; a record
  that is both a vector hit and a graph neighbor keeps its vector provenance (`"vector_and_graph"`).
- **4 new GraphRAG Prometheus metrics**: `valori_graphrag_seed_count` (histogram — seeds per call),
  `valori_graphrag_expanded_nodes` (histogram — expanded nodes per call),
  `valori_graphrag_expanded_edges` (histogram — expanded edges per call),
  `valori_graphrag_no_graph_seed` (counter — vector hits present but no graph seeds).

### Fixed (Phase 5.2 — 2026-08-19)

- **UI GraphRAG sample body** (`PlaygroundView.tsx`): the "GraphRAG" playground panel
  was sending `query:` instead of the correct field name `query_vector:`. Requests from
  the playground would have failed with a 400 deserialization error.

### Added (Phase 5.1 — Graph Query Architecture Audit and Metrics — 2026-08-19)

- **7 new Prometheus metrics for graph operations** (both standalone + cluster):
  `valori_graph_node_create_total` (counter), `valori_graph_edge_create_total` (counter),
  `valori_graph_query_total` (counter), `valori_graph_traversal_nodes` (histogram),
  `valori_graph_traversal_edges` (histogram), `valori_graphrag_total` (counter),
  `valori_graph_rerank_total` (counter). Node/edge/query/subgraph metrics are added
  to the shared `routes/graph.rs` module (fires on both standalone and cluster paths
  automatically). GraphRAG and graph-rerank counters are added to the path-specific
  handlers in `server.rs` and `cluster_server.rs`.

### Added (Phase 5 — Cross-Collection (Orchestrated) Search — 2026-08-19)

- **`POST /v1/search/multi`** (standalone + cluster). New cross-collection
  vector search endpoint. Fans the query out to each listed Collection
  independently in parallel, then merges results globally by Squared L2
  (smaller = better). All Collections must share the same `dim` and `metric`;
  different index types are allowed within the same request.
- **`routes/query_planner.rs`** (new module). Pure orchestration helpers shared
  by both routers: `check_compatibility` (validates dim + metric across all
  Collections), `merge_top_k` (sort by score ascending + truncate), `CollectionHits`.
- **`MultiSearchRequest` / `MultiSearchHit` / `MultiSearchResponse` / `PartialSearchFailure`**
  added to `api.rs`. Hits carry a `collection: String` field. Partial runtime
  failures (one Collection's search fails at runtime) are surfaced in
  `partial_failures` without suppressing results from other Collections.
- **Metadata filter in multi-search**: the `metadata_filter` predicate is applied
  per-Collection after vector search, before the global merge.
- **Decay in multi-search**: `decay_half_life_secs` applies the C4.1 age-based
  re-ranking per-Collection; `decay_factor` and `age_secs` propagate to merged hits.
- **Python SDK `search_multi`** on `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase 4.4 — Cluster ANN Hardening — 2026-08-19)

- **Stale-build detection.** After a node-local ANN build completes (in
  `spawn_blocking`), the activation path re-reads the Raft-replicated desired
  generation before calling `mark_ready` + `activate`. If the desired generation
  has advanced (or the collection was deleted), the just-built index is silently
  discarded and `mark_failed` is called so the watcher can trigger a fresh build
  for the new generation. Prevents a slow gen-N build from overwriting a faster
  gen-(N+1) build.
- **FAILED retry debounce (60 s).** `ClusterCollectionIndex` now tracks
  `last_build_started_at: Option<Instant>`. Before retrying a FAILED generation,
  `trigger_local_build` checks the elapsed time; attempts within 60 s of the
  previous one are silently skipped. Prevents tight retry storms on persistent
  build failures.
- **Watcher drop-path fix.** When `SetMeta(null)` is committed (index dropped),
  `check_and_trigger_pending_builds` now clears both `building_generation` and
  `active_generation` in the local index state. Previously only the active pointer
  was cleared, leaving a stale `building_generation` that could confuse the next
  trigger call.
- **Single `list_namespaces()` call in watcher.** The cleanup and build-trigger
  loops within `check_and_trigger_pending_builds` now share one snapshot of the
  namespace set, halving the number of state-machine lock acquisitions per tick.
- **`GET /v1/namespaces/{name}/index` now reports `desired_type` from Raft state.**
  `IndexOps::get_index_state` always populates `state.desired` from
  `sm.get_meta_json` (authoritative replicated state) before returning. Followers
  that haven't built their local index yet correctly report the cluster's desired
  type (e.g. `"desired_type": "hnsw"`) instead of a blank value.
- **7 Prometheus metrics for cluster ANN lifecycle.** All emitted by
  `trigger_local_build` and the search fallback path:
  `valori_cluster_ann_build_started_total`,
  `valori_cluster_ann_build_completed_total`,
  `valori_cluster_ann_build_failed_total`,
  `valori_cluster_ann_build_duration_seconds`,
  `valori_cluster_ann_generation_active`,
  `valori_cluster_ann_stale_activation_skipped_total`,
  `valori_cluster_ann_search_fallback_total`.
- **9 cluster ANN hardening tests** (`tests/cluster_ann_hardening.rs`): watcher
  build trigger, ANN vs brute-force result agreement, search fallback when no
  index, drop clears state, status API reports Raft desired, build doesn't corrupt
  records, collection re-creation doesn't inherit old index, successive requests
  handled safely, graph state unaffected by index lifecycle.
- **`ClusterHandle::cluster_indexes` field.** The per-collection ANN index map is
  now initialised once at bootstrap (`Arc<RwLock<HashMap>>`) and cloned into every
  `DataPlaneState` built by `build_cluster_router_with_keys`. This means multiple
  router instances (common in tests) share the same index lifecycle state.

### Added (Phase 4.3 — Cluster ANN — 2026-08-19)

- **Cluster ANN indexes (HNSW, IVF, BQ).** `POST /v1/namespaces/{name}/index` and
  `GET /v1/namespaces/{name}/index` now work in cluster mode — the previous 501 is gone.
  The desired spec and generation are replicated through Raft via `KernelEvent::SetMeta`
  (`"__valori_idx_spec:{ns_id}"`) so all nodes agree on the logical generation id and
  parameters. Each node builds and activates its own local ANN index independently
  (node-local activation model). Search uses the local active ANN index and falls back
  to exact brute-force transparently while a build is in progress or if it fails.
- **Background index-propagation watcher.** A per-node background task polls every 5 s,
  reads the replicated desired spec for each collection, and triggers a local build on any
  node that is behind the committed generation. This ensures followers pick up new index
  requests automatically without any additional signalling.
- **`cluster_index_config` updated.** Now reports the actual `VALORI_INDEX` env-var
  setting with a note about per-collection endpoints, instead of hardcoding
  `"brute_force"`.

### Added (Phase 4.2 — Index Lifecycle UI — 2026-08-18)

- **`IndexLifecycleTab` in Studio.** The Analyze menu now includes an Index tab on every
  collection page (local and cloud). Shows all lifecycle states (none/building/active/failed)
  with inline Create, Change, and Remove action panels. HNSW and IVF parameter inputs wired
  through to the backend. Cluster nodes return 501 for build actions; the tab shows the backend
  message inline instead of hiding the limitation.
- **`useCollectionIndex` hook.** Polls `GET /v1/namespaces/{name}/index` at 3 s during transient
  states; stops for terminal states. Revalidates on window focus so a page return after navigation
  always shows the latest lifecycle state.
- **Collection header shows live index status.** `CollectionHeader` in ToolsWorkspace now reads
  `GET /v1/namespaces/{name}/index` (per-collection, live) instead of the node-wide `/health`
  `index` field.
- **"View details" → Index tab.** The header button now opens the Index lifecycle tab directly.
- **Python SDK: extended test coverage (21 tests).** Added error-scenario tests (409 conflict,
  501 cluster, 404 not found) and status-model validation tests (building with active generation,
  active, failed-with-error, none) for both sync and async clients.

### Fixed (Phase 4.2)

- **CollectionList "BRUTE INDEX" display bug.** Collections without an ANN index now show
  "No Index" (both grid and list view modes) instead of "BRUTE INDEX".

### Added (Phase 4.1 — Index Lifecycle Hardening — 2026-08-18)

- **Durable index artifacts.** After `finish_index_build`, the engine writes
  the index bytes as an immutable `StorageKey::IndexArtifact` blob via the
  configured `StorageProvider`. HNSW and IVF artifacts are full roundtrips
  (`snapshot()` / `restore()`). BQ skips artifact writing (rebuilds from
  records on restart; fast).
- **`CollectionManifest` index tracking.** Three new optional fields record
  which generation is active, its algorithm name, and the WAL position the
  artifact was written at (`active_index_generation`, `active_index_type`,
  `active_index_base_lsn`). Backward-compatible (`#[serde(default)]`).
- **Artifact-driven restart (no blind rebuild).** `try_recover()` now calls
  `try_restore_index_artifacts` instead of rebuilding every index from scratch.
  If the artifact's `base_lsn` equals `recovered_lsn`, the artifact is loaded
  directly (fast path). Stale or missing artifacts fall back to a synchronous
  rebuild from `KernelState` records.
- **`drop_collection_index` now durable.** Clears the manifest's index fields
  and deletes the artifact bytes from the provider.
- **IVF build parameters wired.** `POST /v1/namespaces/{name}/index` with
  `{"type":"ivf","parameters":{"n_list":64,"n_probe":8}}` now builds the IVF
  index with exactly those centroids. Without parameters, auto-scale heuristic
  (`sqrt(N)` centroids) is used as before.
- **HNSW build parameters wired.** `m`, `ef_construction`, `ef_search` in
  `parameters` override the library defaults. `m_max0` is automatically
  set to `2 * m` when `m` is provided.
- **7 Rust persistence integration tests** (`index_artifact_persistence.rs`):
  HNSW round-trip, IVF round-trip, missing-artifact fallback, stale-artifact
  rebuild, HNSW explicit params, IVF explicit params, drop clears manifest.
- **11 Python SDK tests** (`test_index_lifecycle.py`): payload/URL contracts
  for all four index lifecycle SDK methods (sync + async).

### Added (Phase 4 — Mutable Collection Index Lifecycle — 2026-08-18)

- **`POST /v1/namespaces/{name}/index`** — create, replace, or drop the ANN
  index for a collection. Returns `202 Accepted` immediately; the build runs
  in a background task. Supported types: `"hnsw"`, `"ivf"`, `"bq"`. Pass
  `{"type": null}` to drop the index and revert to exact search.
- **`GET /v1/namespaces/{name}/index`** — poll the index lifecycle status.
  Response fields: `collection`, `active_type`, `active_generation`,
  `desired_type`, `status` (`none`/`building`/`active`/`failed`/`retiring`),
  `building_generation`, `base_lsn`, `build_started_at`, `error`.
- **Background build with WAL catch-up.** Records inserted while a build is
  in-flight are caught up automatically before the new index is activated.
  The active index (if any) continues to serve searches until the new one is
  atomically swapped in.
- **Generation tracking.** Each built index gets a monotonically increasing
  collection-scoped generation id. A generation is immutable once created.
- **`409 Conflict` on concurrent build.** A second build request while a
  build is in-flight is rejected.
- **Cluster path returns honest `501 Not Implemented`** for build requests.
  Cluster nodes use exact brute-force search for linearizable consistency.
  The status endpoint returns `200 {"active_type":"none","status":"none"}`.
- **`valori-engine::index_manager` module** — `CollectionIndexState`,
  `IndexSpec`, `IndexGeneration`, `IndexState`, `IndexBuildRequest`,
  `IndexStatusResponse`.
- **Python SDK** — `collection_index_status(collection)`,
  `create_collection_index(collection, type, parameters=None)`,
  `change_collection_index(collection, type, parameters=None)`,
  `drop_collection_index(collection)` on both `SyncRemoteClient` and
  `AsyncRemoteClient`.

### Changed (Phase 3.3 — Zero-Collection Projects — 2026-08-18)

- **New projects have zero collections.** `"default"` is no longer
  auto-created; every collection must be explicitly created via
  `POST /v1/namespaces` with `dimension` + `metric` before any insert.
- **`Engine::create_collection(name)` deleted.** The only way to create a
  collection is `create_collection_with_config(name, dim, metric, index)`.
  This is a correctness boundary enforced at the type level.
- **`CollectionRegistry` no longer special-cases `"default"` or `None`.**
  `resolve(None)` → `None`. `resolve(Some("default"))` → `None` unless
  a collection named `"default"` was explicitly created. First allocated
  id is 1 (id 0 stays unallocated — the kernel's `DropNamespace` rejects
  `namespace_id == 0`, unrelated to naming).
- **Ingest routes require an existing collection.** Both standalone and
  cluster ingest paths (`POST /v1/ingest`, `POST /v1/ingest_update`) now
  return HTTP 400 with a clear message if the target collection does not
  exist, instead of silently creating an unconfigured namespace.
- **Document ingest UI returns a clear error** when the target collection
  does not exist instead of swallowing the failure.
- **`valori-cli import` determines dimension from source data** (Qdrant
  collection-info API or JSONL first record) instead of reading the
  dead `/health.dim` field.
- **Bug fix (kernel):** `InsertRecordEncrypted` was reading the legacy
  process-wide `self.dim` instead of `namespace_dim(namespace_id)`, causing
  500s for encrypted inserts into explicitly-configured namespaces that had
  no prior plain inserts. Fixed.
- **Python SDK contract tests (5 new):** confirm the `create_collection`
  wire payload shape including `dimension`/`metric` required, `index` absent
  when `None`, `"default"` treated identically to any other name.

### Changed (Phase 3.2 — Eliminate Implicit Unconfigured Collection Fallback — 2026-08-18)

- **`POST /v1/namespaces` now requires explicit `dimension` and `metric`
  for every Collection name except `"default"`.** Creating a Collection
  with only `{"name": "x"}` returns 400 — a Collection can no longer
  silently lock onto whatever dimension its first insert happens to use.
  `index` remains optional (absence means "no dedicated ANN index",
  not a fake `BruteForce` object). `"default"` keeps its zero-config
  behavior deliberately (disclosed, load-bearing exception) and now
  explicitly rejects being passed config at all.
- Fixed the `DimensionMismatch` HTTP error message, which used to tell
  callers to `set VALORI_DIM={expected}` — that env var stopped being
  read several phases ago. It now explains that a Collection's dimension
  is fixed at creation and points at creating a new Collection instead.
- No SDK or UI changes were required: the Python SDK's `create_collection`
  and the local-project UI's `useCollections.create()` already sent
  `dimension`/`metric` explicitly.
- **Found, not fixed (logged as follow-ups)**: `ui/`'s document-ingest
  flow (`api/ingest/route.ts`) silently fails to pre-create a new
  collection under the stricter contract (fails safely downstream with a
  confusing error, not a data-integrity issue); `valori-cli import`'s
  dimension lookup was already broken by an earlier phase's removal of
  `/health`'s `dim` field, unrelated to this change.

### Changed (Phase 3.0 — Remove Process-Wide Vector Config from Project Creation UI — 2026-08-17)

- **Project creation no longer asks for dimension, index, or embedding.**
  `CreateProjectDialog` (local wizard + sidebar) now only collects `name`,
  `replication`, and `shardCount`. An informational callout guides users to
  configure dimensions per-collection after project creation.
- **Collection creation now requires dimension.**
  `CreateCollectionDialog` has new required "Dimension" (integer, 1–65535) and
  optional "Index" (brute / auto / hnsw / ivf / bq) fields. Both are forwarded to
  `POST /v1/namespaces { dimension, index }`, matching the server-side
  `CreateCollectionRequest` schema added in the previous phase.
- `POST /api/projects` (Next.js route) no longer accepts or forwards `dim` / `index`
  from the UI client. The daemon manifest receives `dim = 0` as an internal sentinel
  (no process-wide default). Existing projects are unaffected.
- Cloud `CollectionsPanel` inline form gains a dimension number input alongside the
  name field, maintaining parity with the local collection-creation dialog.

### Added (Phase 2.4 — Complete Storage Coherence — 2026-08-17)

- **Graph Durability in Collection Snapshots**: Bumped collection snapshot schema to V3 (`COLLECTION_SNAPSHOT_SCHEMA_VERSION = 3`). Snapshots now serialize and restore collection-owned `GraphNode` and `GraphEdge` structures alongside records.
- **Multi-Collection Hole Filling**: `collection_snapshot::restore_project_into` restores records, graph nodes, and graph edges with monotonic slab allocator hole filling, advancing global slot counters up to each collection's ceiling without inter-collection corruption.
- **StorageProvider WAL Rotation Publishing**: `EventCommitter::maybe_rotate` and `rotate_log` publish sealed WAL segments directly to `StorageProvider` (`StorageKey::WalSegment`) as immutable artifacts upon rotation.
- **Streaming WAL Replay via StorageProvider**: `stream_events_from_provider` replays WAL tails across sealed segments and active WAL without whole-log heap allocations, enforcing namespace-specific `snapshot_base_lsn` filtering during recovery.
- **StorageProvider Recovery Decoupling**: `recover_project_from_storage` and `Engine::try_recover` recover full project state from `StorageProvider` abstractions rather than hardcoded raw filesystem paths.
- **Namespace-Scoped Graph Iterators in Kernel**: `KernelState::iter_nodes_in_ns`, `iter_edges`, and `iter_edges_in_ns` provide clean, zero-allocation iterators over graph entities for snapshot extraction.

### Added (Collection-Scoped Vector Configuration — 2026-08-16)

- **Architecture change**: vector dimension, metric, and index algorithm
  move from Project scope (one value shared by every namespace in a
  `valori-node` process) to Collection scope (one value per namespace).
  One node process still hosts every collection — no per-collection OS
  process was introduced, and Project/node/topology isolation is
  unchanged.
- `POST /v1/namespaces` gains optional `dimension`, `metric`
  (`"squared_l2"` only today), and `index` (`brute`/`hnsw`/`ivf`/`bq`/`auto`)
  fields. Omitting all three keeps a collection's exact pre-existing
  behavior: it inherits the project's legacy `VALORI_DIM`/`VALORI_INDEX`.
  `index` without `dimension` is rejected — an index is built for a
  specific dimension at creation time. `GET /v1/namespaces` now reports
  each collection's explicit config, when it has one.
- New `KernelEvent::ConfigureNamespace` (append-only, kernel snapshot
  `SCHEMA_VERSION` 7→8) makes collection config part of the replicated,
  audited, replayed state on both the standalone and cluster paths — the
  mechanism that keeps every Raft replica in agreement on a collection's
  dimension. V1–V7 snapshots continue to decode unchanged; a V8 snapshot
  with zero explicitly-configured collections is behaviorally identical
  to a V7 one.
- `valori-engine::Engine` gains a dedicated `dyn VectorIndex` per
  explicitly-configured collection (standalone path), replacing "one
  shared index, post-filtered by namespace" for anything that opts in.
  Existing single-collection projects are unaffected — proven by test.
- New `valori_domain::Metric` (`SquaredL2` only — the metric is now
  representable as data; no new distance calculation was introduced).
- **Known limitations, disclosed and tested, not silently shipped**:
  cluster-mode search for any collection currently uses the
  brute-force-equivalent path regardless of the requested `index`
  (cluster mode's Raft state machine applies directly to `KernelState`,
  with no `Engine` in that path); a collection whose dimension differs
  from every other namespace in the same snapshot does not yet survive
  a snapshot/restore cycle (the kernel's record section stores one
  vector byte-width per file). See
  [phase-collection-scoped-vector-config.md](docs/phases/phase-collection-scoped-vector-config.md)
  for the full breakdown, including what was deliberately deferred
  (Python SDK, UI, `Project.dim`/`.index` deprecation).
- `cargo test` clean across `valori-kernel`, `valori-domain`,
  `valori-metadata`, `valori-engine`, `valori-consensus`; 52/52
  pre-existing `valori-node` namespace/collection/search-isolation
  tests unaffected; `route_parity` 2/2; `wasm32-unknown-unknown` build
  verified after every kernel change.

### Fixed (G1.4.2 — Cluster Vector Search Namespace Isolation — 2026-08-13)

- **Critical**: cluster's `POST /search` ignored namespace/collection
  scoping entirely whenever more than one namespace mapped to the same
  shard — including the default `VALORI_SHARD_COUNT=1` deployment, where
  *every* namespace shares shard 0. It called `KernelState::search_l2`
  ("ALL records regardless of namespace — backward-compat, single-tenant")
  and relied solely on shard routing for isolation, which enforces nothing
  once two namespaces share a shard. Confirmed directly: two collections,
  colliding vectors, a search scoped to one collection returned both.
  Found while building G1.4.1, unrelated to and not caused by graph-aware
  reranking — the plain vector search path had the bug regardless.
- Fixed via a new `shard_search_ns()` helper in `cluster_server.rs`,
  mirroring standalone's existing `Engine::search_l2_ns` split: the exact,
  namespace-scoped kernel function
  (`KernelState::search_l2_ns`) for `BruteForce` (the only index variant
  cluster mode ever configures — confirmed, cluster never calls
  `set_index_kind()`), falling back to a global search + post-filter
  otherwise. Fixes every downstream search mode (plain, BM25 rerank, decay,
  metadata_filter, the new G1.4.1 `graph_rerank`) since they all operate on
  this one root candidate list.
- 7 new tests in `crates/valori-node/tests/cluster_search_namespace_isolation.rs`
  covering 1-shard/2-namespace isolation for every search mode, a
  3-shard/2-namespace sanity check (confirms the already-safe-by-routing
  case stays safe), soft-delete exclusion, and the default namespace.
  Revert-and-confirmed non-vacuous (4/7 fail when reverted to the old
  namespace-blind call). `cargo test -p valori-node` 363/363 passed (up
  from 356), fmt/clippy clean.

### Added (G1.4.1 — Graph-Aware Vector Reranking — 2026-08-13)

- `POST /search` gains an optional `graph_rerank` field
  (`{seed_count, weight, direction, max_depth}`, all defaulted) — reranks
  vector results by graph proximity to the search's own top hits. Seeds are
  resolved from the top `seed_count` hits' graph nodes (no separate
  seed-node lookup required); each candidate's graph distance is the
  minimum hop count (bounded BFS) across every live graph node referencing
  its record (a record may have several — G1.3.1). Formula:
  `adjusted = score × (1 + weight × distance)` — a multiplicative penalty,
  same shape as the existing decay re-ranker's `distance / factor`. Missing
  or unreachable graph data is neutral: never penalizes, never drops a
  candidate.
- Hits gain `graph_distance: Option<u32>` (present only when `graph_rerank`
  was requested).
- Composes with either the existing BM25 rerank or decay re-rank — runs as
  an independent final pass over whichever score they already produced.
  Absence of `graph_rerank` is a byte-identical no-op to pre-G1.4.1
  behavior.
- New `valori_rag::graph::graph_distances_from_seeds` — multi-source,
  bounded, direction-scoped BFS, deterministic and path-independent.
- New `valori_search::graph_rerank` module — pure scoring math, mirrors
  `decay`'s existing shape/conventions exactly.
- Read-time only: never mutates canonical state, never touches
  `KernelEvent`/snapshot/WAL format, never affects the BLAKE3 state hash.
- Standalone and cluster both wired (`server.rs`/`cluster_server.rs`);
  Python SDK's `search()` gained `graph_rerank=` on both
  `SyncRemoteClient` and `AsyncRemoteClient`.
- 34 new tests across `valori-rag`, `valori-search`, and two `valori-node`
  HTTP integration test files (standalone + a real single-node Raft
  cluster); revert-and-confirmed non-vacuous. `cargo test -p valori-node`
  356/356 passed (up from 338), `route_parity` clean, fmt/clippy clean.
- Design doc:
  [docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md](docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md).
  Implements "Option 1" from
  [docs/reviews/graph-g1.4-hybrid-retrieval-design.md](docs/reviews/graph-g1.4-hybrid-retrieval-design.md);
  graph-reachability pre-filtering and independent-signal/RRF fusion remain
  explicitly deferred.
- **Found, not fixed** (flagged as a separate task): a pre-existing bug
  where cluster's `/search` ignores namespace/collection scoping entirely
  when `VALORI_SHARD_COUNT=1` — unrelated to and not caused by this
  feature; see the phase doc's Findings section.

### Fixed (G1.3.1 — Record → GraphNode Cascade Fix — 2026-08-12)

- **Critical**: hard-deleting a record (`POST /v1/delete`) that still had one
  or more graph nodes referencing it left those nodes dangling
  (`node.record` pointing at a freed record slot) — the resulting state's
  own snapshot then failed to decode on restart. Reachable using only
  shipped endpoints (`/v1/memory/consolidate` and `/v1/memory/contradict`
  each create a fresh node per record with no reuse check, so a record with
  2+ live nodes is a normal outcome, not an edge case).
- `Engine::delete_record` now cascade-deletes every live node referencing
  the record (ascending `NodeId` order, via the new
  `valori_rag::graph::nodes_referencing_record` enumeration primitive) —
  and, because `delete_node` already frees a node's incident edges, those
  go too — before freeing the record itself. Fixes the pre-existing partial
  cascade (only the last-created node was ever cleaned up, via the removed
  `record_to_node` single-valued cache).
- `Engine::soft_delete_record` no longer touches the graph at all: the
  record row survives a soft delete, so any node referencing it stays
  valid — the pre-fix code's partial node cascade there was itself a bug.
- Cluster's `DataPlaneState::delete` previously performed **zero** cascade
  of any kind on record deletion; it now mirrors the standalone cascade via
  sequential `raft_write_data` `DeleteNode` writes before the record delete.
- Both `POST /v1/delete` and `POST /v1/soft-delete` now reject (404) a
  `collection` that doesn't own the target record, on both standalone and
  cluster paths — previously a record in namespace A could be deleted
  through a request scoped to namespace B.
- Hard delete now also calls `reranker.remove()` (previously only soft
  delete did, an asymmetry with no justification).
- Removed the now-fully-dead `record_to_node: HashMap<u32, u32>` cache from
  `Engine` — its only two consumers were the two delete paths fixed above.
- 14 new tests across `crates/valori-node/tests/graph_cascade_delete.rs`,
  `api_graph_cascade_delete.rs`, and `cluster_graph_cascade_delete.rs`
  (standalone/cluster parity, revert-and-confirmed non-vacuous against the
  pre-fix code). `cargo test -p valori-node` 338/338 passed (up from 324).
  See [docs/phases/phase-G1.3.1-record-graph-cascade-fix.md](docs/phases/phase-G1.3.1-record-graph-cascade-fix.md).

### Added (Cloud P2 — Usage & Quota Accounting, node-side — 2026-08-12)

- `GET /v1/usage` — new read-only endpoint (standalone `server.rs` +
  cluster `cluster_server.rs`) reporting `records`, `collections`, and
  `storage` (`event_log_bytes`/`snapshot_bytes`/`total_bytes`) for
  Cloud's plan/quota/usage accounting system. `valori-node` remains
  completely plan-agnostic — the endpoint returns raw counts only, no
  plan or billing context. Never mutates canonical state (read lock
  only) and never affects the BLAKE3 state hash — verified directly by
  a new test asserting `/v1/proof/state` is byte-identical whether or
  not `/v1/usage` was ever called.
- Storage accounting correctly sums the live event-log segment plus
  every rotated archive segment (`events.log`, `events.log.000001`,
  ...) — archived segments are never deleted on rotation, so a naive
  stat of only the live file would silently undercount after any
  rotation. Covered by a dedicated test.
- Cluster mode sums `records`/`storage` across every shard the node
  runs (records are genuinely partitioned by
  `namespace_id % shard_count`); `collections` is not summed — the
  namespace registry is a single logical registry maintained via shard
  0's Raft group alone, not duplicated per shard.
- Authenticated via the existing `auth_guard_v2`/`VALORI_AUTH_TOKEN`
  mechanism — no new auth code needed.
- 5 new tests in `crates/valori-node/tests/usage_endpoint_tests.rs`;
  `cargo test --workspace` 1194/1194 passed, clippy clean.
- Full design rationale (usage model, storage accounting inventory,
  stale-worker semantics, Cloud-side scheduler/schema) lives in the
  private `valori-ui` repo (`docs/architecture/plan-quota-p2-usage-accounting-plan.md`)
  — Cloud business logic stays out of this public repo per this
  project's own architecture boundary.

### Added (S11 — Index Tuning & Product Defaults — 2026-08-11)

- `BqConfig { pool_factor, min_candidates }` (`crates/valori-index/src/bq.rs`)
  — BQ's candidate-pool size, previously hardcoded (`POOL_FACTOR=10`,
  `MIN_CANDIDATES=200`), is now runtime-configurable. Defaults reproduce
  prior behavior exactly.
- `VALORI_BQ_POOL_FACTOR` / `VALORI_BQ_MIN_CANDIDATES` env vars, wired
  through `NodeConfig` → `EngineConfig` → `Engine::make_index`, following
  the same pattern as the existing `VALORI_IVF_N_LIST`/`_N_PROBE`.
- 2 new unit tests in `bq.rs` (`custom_config_changes_candidate_pool_without_error`,
  `default_config_matches_prior_constants`).
- `docs/reviews/index-tuning-audit.md` — full audit of BruteForce/HNSW/IVF/BQ
  covering memory model, build/insert/search cost, recall, restart/recovery
  behavior, and configuration surface. Found IVF and HNSW both have real
  `snapshot()`/`restore()` code that `Engine::try_recover()` never calls —
  recovery always rebuilds from the record pool from scratch, which is the
  root cause of both indices' expensive recovery times.
- Real Docker benchmarks (1GB/0.5vCPU, 384D, seed 42): IVF sweep (4 cells,
  n_list×n_probe) found search p50 flat at ~660-664ms regardless of
  configuration — no tested config achieves the 25% latency reduction
  needed to justify IVF as a default — but recovery time scales linearly
  with n_list (16.9s at n_list=64 vs 47.4s at auto-scale's n_list=223),
  independent of n_probe. BQ sweep (2 cells) found `min_candidates=10000`
  (~20% of N) reaches Recall@10=0.99 (up from the S10 default's 0.48) at
  comparable memory/recovery cost. HNSW (1 cell, 10K) found search latency
  statistically tied with BruteForce (115.95ms vs 118.29ms), 24-28x worse
  insert throughput, ~150x worse recovery (187.1s vs 1.23s) — does not
  clear its own "clearly outperforms" bar, so the mandated 50K follow-up
  point was not run.
- Fixed `benchmarks/capacity/scripts/bench_cell.py`'s restart-health-check
  timeout (60s → 300s) — the same class of bug already fixed in
  `bench_ivf_bq.py`, hit again on the first HNSW attempt this phase
  (`status: restart_failed` was a benchmark-timeout artifact, not a real
  integrity failure — confirmed by rerunning with the fixed timeout,
  BLAKE3 state hash matched).
- `benchmarks/capacity/scripts/bench_ivf_bq.py` — added
  `--n-list`/`--n-probe`/`--bq-pool-factor`/`--bq-min-candidates` CLI flags.
- **Recommendation (evidence-based, not applied to production)**: BruteForce
  remains the Free-tier default index — no tuned configuration of IVF, BQ,
  or HNSW beat it on the combined predictable-memory/predictable-latency/
  high-recall/fast-recovery/deterministic/simple criteria at Free-tier
  scale. Index choice is not exposed to Cloud users (Option B: Valori
  chooses automatically); the `VectorIndex` abstraction and all four
  implementations remain intact for future tiers. Documented a
  forward-compatible `{"index": {"type", "config"}}` API contract as the
  extension point for future index types/parameters, without building a
  generic config framework the current architecture doesn't yet need.
- `benchmarks/capacity/results/s11-summary.md`,
  `docs/phases/phase-S11-index-tuning.md` — full results and findings.
  No plan limits, provisioning defaults, pricing, or customer-facing
  quotas were modified this phase.

### Added (S10 — IVF/BQ Capacity Validation — 2026-08-11)

- `docs/reviews/index-capacity-audit.md`,
  `docs/phases/phase-S10-index-capacity.md`,
  `benchmarks/capacity/scripts/bench_ivf_bq.py`,
  `benchmarks/capacity/results/s10-summary.md`.
- Real Docker benchmarks (IVF@50K, IVF@100K, BQ@50K, 384D, 1GB/0.5vCPU)
  with recall@k measured against exact-L2 ground truth. Finding:
  **neither IVF nor BQ meaningfully improves on BruteForce capacity in
  the current implementation** — IVF's search latency is neutral-to-worse
  at scale (664ms→1332ms p50, worse tail latency) with a severe
  recovery-time penalty that scales as roughly N^1.5 (47s at 50K → 130s
  at 100K, since the index is rebuilt from scratch via k-means on every
  restart); BQ trades meaningful recall (Recall@10 ≈ 0.48 at 50K) for a
  ~5% latency improvement at higher memory cost than either BruteForce
  or IVF. All restart-hash checks passed (S8 fix holds for both index
  types). **RECOMMENDED, NOT APPLIED**: the Free-tier vector quota
  should continue to be derived from BruteForce's measured performance
  boundary (S9's ~25,000-30,000 vectors) rather than a higher
  index-specific number, until IVF's auto-scaled parameters are tuned
  to actually prune the search space (currently returns perfect recall,
  meaning it isn't pruning at all) or a different index is measured and
  shown to help. No production plan limits, provisioning defaults, or
  pricing were changed.

### Added (S9 — Resource Capacity & Plan Limits — 2026-08-11)

- `benchmarks/capacity/` — real Docker capacity benchmark harness
  (reuses the actual `cloud-worker-a` image, real `docker run
  --memory`/`--cpus` limits, real restart-hash verification per cell).
  19 measured result records across RAM boundary, index-type
  comparison, dimension sweep (384-1536), and multi-project contention.
- `docs/reviews/resource-capacity-audit.md`,
  `docs/phases/phase-S9-resource-capacity.md`.
- Key findings: the existing (never benchmark-validated) free-tier
  `max_records_per_project = 1,000,000` is ~34x higher than the
  measured, search-latency-bound safe limit (~29,300 vectors at 384D
  BruteForce on the planned 1GB/0.5vCPU free worker); HNSW recovery
  takes 188s for just 10K vectors (35-160x slower than
  brute/ivf/bq — real index is never persisted, always rebuilt from
  the record pool on restart); actual memory per vector measured at
  ~4.6-5.3x the raw Q16.16 byte size, not 1x. No production plan
  limits, provisioning defaults, or pricing were changed — this phase
  produced a **proposed**, evidence-based limits table for review.

### Fixed (S8 — Deterministic Restart Integrity — 2026-08-11)

Root-caused and fixed the state-hash-across-restart divergence flagged by
the Local Cloud E2E phase (vector data and search results were
byte-identical before/after a real container restart, but the BLAKE3
state hash differed). Isolated via a standalone Rust reproduction
comparing live-apply vs event-log-replay hashes directly (bypassing
Docker/HTTP entirely), confirmed against the real `valori-node` binary
both before and after the fix.

- **Root cause**: `Engine::create_collection()` and `Engine::drop_collection()`
  mutated `KernelState` directly via `state.apply_event_ns(...)`,
  bypassing the durability layer entirely (`commit_and_apply_ns`, the
  "log then apply" helper every other mutation uses). The
  `AutoCreateNamespace`/`DropNamespace` events are no-ops on
  records/nodes/edges, but still bump `state.version` — which IS hashed
  — so the live process's version counter ran ahead of anything the
  event log could ever replay.
- **Fixed**: both now route through `commit_and_apply_ns()`.
- **Second, related gap**: even logged, a single-event commit
  (`commit_event_ns`) only buffers in memory (`DEFAULT_WRITE_BUFFER_SIZE
  = 64`) — unlike a batch commit, which flushes unconditionally. The
  buffer previously only flushed via `Engine::drop()`, which isn't
  guaranteed to fire on graceful shutdown (`SharedEngine` is an
  `Arc<RwLock<Engine>>`; background tasks can hold clones past
  `with_graceful_shutdown`'s return). Added `Engine::flush_pending_events()`
  and wired it into `main.rs`'s `shutdown_signal`, under a write lock,
  before the snapshot save.
- Regression test: `crates/valori-node/tests/persistence_tests.rs::test_state_hash_survives_restart_after_collection_create`.
- Re-verified end-to-end against the real Docker container: state hash
  now matches identically before and after a genuine `docker stop`/`start`.

### Fixed (Local Cloud E2E verification — 2026-08-11)

Found and fixed by standing up the real Cloud stack (Postgres + real
migrations + PostgREST + two real `valori-node` workers + the real
Next.js Cloud API) in Docker and running the real Python SDK against it
end-to-end — see `e2e/cloud/` and `docs/architecture/project-api-v1.md`.

- `POST /api/projects/[id]/delete` (private `valori-ui` repo) never
  accepted `vlk_` API-key auth — 404'd for every external caller instead
  of authenticating. Fixed to match every sibling write route.
- `valoricore/__init__.py` unconditionally imported `adapter.py`, which
  unconditionally imported the compiled Rust FFI extension — `import
  valoricore` (and therefore `valoricore.remote`'s pure-HTTP `Valori`
  client) failed without a native build present. `adapter.py`'s FFI
  import is now guarded the same way `local.py`'s already was.
- `create_api_key()` (private `valori-ui` repo, Postgres function) had
  two ambiguous-column bugs — its own `RETURNS TABLE` column names
  (`id`, `project_id`) shadowed the same-named table columns inside two
  of its internal queries, breaking every call unconditionally. Never
  caught before real E2E against PostgREST.
- `proxyToNode()` (private `valori-ui` repo) hardcoded `method: 'POST'`
  on every non-GET request regardless of the caller's actual method,
  silently turning every `DELETE` into a `POST`; separately, it crashed
  constructing a `NextResponse` with a `204` status and a JSON body
  (spec-illegal). Both only surface on a route returning a real 204,
  which only real E2E exercises.
- `valori-node`'s `NodeConfig` printed its own `auth_token`/
  `embed_api_key` in plaintext at INFO level on every startup (derived
  `Debug`). Replaced with a hand-written `Debug` that redacts both.
- `Valori(url=..., api_key=...)` required `url` positionally with no
  default — `Valori(api_key="vlk_...")` (the documented production
  usage) would fail. `url` now defaults to `https://app.valori.systems`.

### Added (Project-Scoped API Keys — P1/P2 — 2026-08-10)

- Python SDK: `valoricore.Valori` — a thin `SyncRemoteClient` subclass for
  the Cloud project-scoped API key contract (`Valori(url=..., api_key=...)`).
  No new authorization logic; reuses existing Bearer-auth/retry/error
  mapping unchanged.
- (Private `valori-ui` repo, documented here for cross-repo visibility per
  this project's public/private split) Cloud API keys are now
  project-scoped by construction: `api_keys.project_id`/`expires_at`,
  `verify_api_key()` rewritten so a project-scoped key can never resolve
  to a project other than the one it's bound to (previously org-scoped —
  any key in an org could reach any project in that org), atomic
  project+Default-key creation, and a fix for API auth failures
  previously mapping to a misleading `404` (now correctly `401`/`403`).
  Legacy org-scoped keys (`project_id = null`) keep their exact
  pre-migration behavior, not backfilled or narrowed. See
  `docs/architecture/project-api-key-architecture.md` and
  `docs/phases/phase-project-api-key-P2.md`.

### Fixed (Project-Scoped API Keys hardening — P2.1 — 2026-08-10)

- Executed P2's security test suite for real (10/10 pass) against a real
  Postgres instance with the full real migration chain applied — it had
  only been written, not run, in P2. Found and fixed 3 real bugs surfaced
  only by real execution: an invalid `CREATE OR REPLACE VIEW` column
  reorder, a plpgsql parameter/output-column name collision, and two
  errors in the test fixtures themselves.
- (Private `valori-ui` repo) Fixed `Valori-Kernel/ui`'s stale
  `create_api_key` callers — the 3-arg pre-migration shape P2's report
  flagged, plus a second, previously-undiscovered call site
  (`ui/src/app/settings/page.tsx`). Fixed `duplicateProject()` to show its
  new project's Default key instead of creating it silently.
- Verified live: FK cascade deletes a project's keys on project deletion;
  a failed atomic project+key creation leaves no orphan rows; no raw key
  ever reaches logs, telemetry, or browser storage in any file this work
  touched.

### Added (Cloud → Worker authentication — P2.2 — 2026-08-10)

- (Private `valori-ui` repo) `VALORI_AUTH_TOKEN` defense-in-depth: every
  Cloud project now has an internal worker auth token, set in its
  deployed node's env by the Rust provisioner and attached by the Cloud
  proxy on every request it forwards — closing the "every Cloud node is
  unauthenticated at the node level" gap the original audit found. No
  `valori-kernel`/`valori-node` code changed — reuses the node's existing
  auth mechanism.
- Proved the full request chain over real HTTP (PostgREST + a real
  `valori-node` process, no simulation): worker-token enforcement,
  cross-project key isolation, and atomic project+key creation all
  verified live.
- Found and fixed a real security bug this live testing caught: a
  `REVOKE SELECT (column)` on top of a pre-existing broad table grant
  does not actually restrict access in Postgres (they're ORed, not
  layered) — the worker token was readable by any authenticated user
  until this was caught by an actual HTTP request and fixed the same way
  `api_keys.key_hash` is already protected elsewhere in this schema.

### Added / Fixed (NodeClient unification, real E2E, Python SDK — P2.3 — 2026-08-10)

- New `NodeClient` (`valori-ui/ui`) — the single way Cloud code talks to a
  node; a sweep found 14 files (not 2) making ad-hoc, un-authenticated node
  calls and converted all of them.
- Proved the full Python SDK → Cloud API → node chain over real HTTP
  (real Postgres, PostgREST, and a real node process) — found and fixed a
  serious bug this caught that nothing else could have: every
  project-scoped key's real default scope failed every real route's scope
  check due to an exact-string-match bug in `verify_api_key()` — **every
  newly-issued key would have 401'd on its first real request**. Fixed
  with a wildcard match, re-verified live.
- Corrected the Python SDK: P2.2's `Valori` class targeted the wrong route
  shape for real Cloud use (never actually tested against it). Added
  `client.collections.create()/upsert()/search()` and a new `GET /api/me`
  self-discovery endpoint, so the SDK never takes a `project_id` argument —
  proven live end-to-end, including cross-project 403 and revoked-key 401.
- A real safety incident during setup is disclosed in full in
  `docs/phases/phase-project-api-key-P2.3.md`: one test request briefly
  reached a real Supabase project due to an env-var precedence assumption
  that didn't hold. Assessed as harmless (a no-op key lookup, no data
  touched) and fixed by isolating the test environment properly for the
  remainder of testing.

### Changed (Legacy-key cleanup, SDK URL fix — P2.4 — 2026-08-10)

- Confirmed no real legacy org-scoped Cloud API keys exist anywhere (not
  deployed yet) — removed that entire compatibility path.
  `api_keys.project_id` is `not null`; `verify_api_key()` no longer takes
  a `target_project_id` parameter or returns `key_kind` — one code path,
  not two. SQL test suite updated and **re-run live: 9/9 pass**.
- Python SDK example/docstring URL corrected from `api.valori.systems`
  (never created, not needed) to `app.valori.systems` — the SDK's routes
  live inside the same Next.js app as the dashboard, not a separate
  deployment.

## [0.3.0] — 2026-08-09

Kernel/workspace and desktop app both bumped to 0.3.0 (minor — this
release adds real new capabilities: OS-keychain credential storage,
desktop filesystem consolidation, session retention, and the persistence
boundary cleanup below, not just fixes).

### Added (Phase Studio S7 — Persistence Boundary Cleanup — 2026-08-09)

Closes out S6's six follow-up items.

- `ui/src/lib/server/valori-home.ts` (new) — the one TypeScript-side
  `$VALORI_HOME` resolver, replacing three duplicated (agreeing) copies
  and fixing two that silently ignored an explicit override.
- `modelDir` preference now actually wired: overrides model artifact
  storage independently of `workspaceDir`
  (`ModelManager::new_with_models_dir`, `VALORI_MODELS_DIR` env var,
  `startDaemon(home, modelDir)`).
- `metadata.redb`'s dormancy formally documented and mechanically
  enforced (`dependency_direction.rs`'s new
  `metadata_db_open_stays_out_of_production_binaries`) — not wired, not
  deleted (its `Project`/`Collection` code is real, paused M3
  infrastructure).
- `logs/` and `crashes/` given real, bounded content: a `tracing-appender`
  file log sink (daily rotation, 7-day cleanup) and best-effort crash
  archival (30-day cleanup) — the live panic-hook marker path is
  unchanged.
- `tauri-plugin-store` fully removed (dependency, plugin registration,
  capability entry) — zero call sites existed.
- `valori:notifs` migrated off desktop `localStorage` onto
  `studio.redb`'s `preferences.notification_prefs` (web build unchanged).
- New consolidated architecture test
  (`desktop/src-tauri/tests/persistence_boundary_architecture.rs`, 8
  tests) mechanically prevents: UI writing desktop state to
  `localStorage` outside an explicit allowlist, UI touching the raw
  filesystem, any module minting its own `~/.valori` path outside an
  explicit allowlist, Studio storage depending on project internals, and
  a second embedded database engine appearing anywhere in the workspace.
- See `docs/phases/phase-studio-S7-persistence-boundary.md` for full
  validation.

### Added (Phase Studio S6 — Desktop Filesystem Consolidation — 2026-08-09)

Establishes one canonical, enforceable answer to "where does every desktop
file live, who owns it, how is it created/recovered/cleaned up" — see
`docs/reviews/studio-filesystem-audit.md` for the preceding read-only
audit.

- `crates/valori-studio-storage/src/path.rs` — new `StudioPaths`, the
  canonical path resolver: typed accessors for `studio_db`, `backups_dir`,
  `recovery_log_path`, `projects_dir`/`project_dir(name)`,
  `models_dir`/`model_dir(&ModelId)`, `logs_dir`, `crashes_dir`,
  `cache_dir`, `downloads_dir`, `temp_dir`. Pure path math — no
  filesystem access. Pre-existing free functions now delegate to it.
- `desktop/src-tauri/src/filesystem_service.rs` (new) — `FileSystemService`:
  safe operations (`create_dir`, `atomic_write`/`atomic_replace`,
  `read`/`remove`/`rename`/`copy`/`exists`, `clear_cache`,
  `cleanup_stale_temp_files`) plus `safe_join` (component-aware path-
  traversal + symlink-escape rejection). Wired into startup: stale
  temp-file cleanup (24h), never fatal.
- `crates/valori-daemon/src/project.rs` — `project.json`'s existing atomic
  write-then-rename gained an `fsync` before the rename, closing a narrow
  power-loss durability gap.
- New architecture tests (`desktop/src-tauri/tests/filesystem_architecture.rs`)
  prevent Studio storage/desktop from ever depending on
  kernel/storage/node/daemon crates, and prevent the browser UI/Cloud from
  touching the local filesystem directly.
- A project-safety test proves, with real production types against a real
  project fixture, that every Studio housekeeping operation (recovery,
  cache clear, temp cleanup, atomic writes) leaves project files
  byte-for-byte unchanged.
- See `docs/phases/phase-studio-S6-filesystem-management.md` for full
  validation, including a real 16-step desktop smoke test.

### Fixed (Phase Studio S5 — Session Retention — 2026-08-09)

Fixes the P0 finding from `docs/reviews/studio-persistence-consolidation-audit.md`:
`studio.redb`'s `sessions` table grew by one row per app launch, forever,
with no pruning.

- `crates/valori-studio-storage/src/session.rs` — new
  `SessionRetentionPolicy` (typed: keep newest 100 completed sessions;
  completed sessions beyond that floor prune once older than 90 days;
  crashed sessions prune once older than 180 days, no count cap) and
  `SessionStore::prune(current_session_id, &policy, now)`. Deterministic,
  oldest-first deletion; touches only the `sessions` table.
- `desktop/src-tauri/src/lib.rs` — startup reordered to DB open →
  installation identity → crash reconciliation → **prune** → start
  current session. Pruning failure is logged and never fatal to startup,
  never triggers `studio.redb` recovery.
- No schema version bump — purely additive, fully backward compatible.
- See `docs/phases/phase-studio-S5-session-retention.md` for full
  validation (13 new storage tests, 118 total; real desktop smoke test
  against a disposable `$VALORI_HOME` with byte-for-byte project-file
  verification).

### Security (Phase Studio S3 — Credential Security — 2026-08-09)

Implements the approved fix from `docs/reviews/studio-credentials-audit.md`:
provider API keys (OpenAI/Cohere/Groq/Together/custom) no longer persist as
plaintext in `localStorage` on the desktop build.

- **`CredentialRef`** (`crates/valori-domain`) — new UUID v4 opaque
  reference type, same `uuid_id!` pattern as `InstallationId`.
- **`CredentialService`** (`desktop/src-tauri/src/credential_service.rs`,
  new) — the sole wrapper around the `keyring` crate (macOS Keychain /
  Windows Credential Manager / Linux Secret Service). New Tauri commands:
  `credential_store`, `credential_get`, `credential_exists`,
  `credential_delete`.
- `useLLMConfig.ts`, `useEmbeddingConfig.ts`, and `SettingsModal.tsx`'s
  reranker config now persist `{ provider, model, credentialRef }` instead
  of `{ provider, model, apiKey }` in `localStorage` — desktop only. The
  web/Cloud build is unchanged (documented limitation — no OS keychain
  reachable from a browser tab).
- One-time, idempotent, verify-before-delete migration of existing
  plaintext credentials (`native.ts`'s `migrateLegacyProviderCredential`) —
  never deletes the legacy value before confirming the new one resolves.
- `studio.redb` was not and is not used for provider configuration; new
  architecture tests (`credential_security_architecture.rs`) lock in that
  secrets cannot enter `studio.redb`, telemetry, logs, or crash reports.
- See `docs/phases/phase-studio-S3-credentials.md` for full validation.

### Fixed (Phase Studio Installation Identity — 2026-08-09)

Implements the approved fix from `docs/reviews/installation-id-audit.md`:
`installation_id` was only ever generated as a side effect of the
telemetry send path, so any user who never opted into telemetry (the
default state) never received an installation identity at all.

- `desktop/src-tauri/src/lib.rs` — `setup()` now calls
  `StudioPreferencesService::get_or_init_installation_id()`
  **unconditionally**, before session start, independent of telemetry
  consent, Cloud login, or project state.
- `desktop/src-tauri/src/preferences_service.rs` —
  `get_or_init_installation_id` is now the sole canonical
  get-or-init implementation (returns typed `InstallationId`).
- `desktop/src-tauri/src/telemetry.rs` — the private `installation_id()`
  helper no longer duplicates get-or-init logic; it reads through the
  canonical service.
- New architecture test
  (`desktop/src-tauri/tests/installation_id_architecture.rs`, 4 tests)
  mechanically enforces exactly one generation site and no second desktop
  persistence location.
- Existing sessions recorded with `installation_id: None` (from installs
  that had telemetry off) are left as accurate historical records, not
  rewritten.
- See `docs/phases/phase-studio-installation-identity.md` for full
  validation (105 storage tests, 45 desktop tests, real desktop smoke
  test against a disposable `$VALORI_HOME`).

### Fixed (Phase Studio S2c — Privacy Boundary & Persistence Cleanup — 2026-08-08)

Implements exactly the two concrete issues found by
`docs/architecture/studio-persistence-audit.md` — no other persistence
feature touched.

- **Telemetry consent revocation now invalidates already-queued analytics
  events**, closing a real privacy gap: previously, disabling analytics
  stopped new events from queuing but did nothing to events already in
  `telemetry_queue` — `drain_queue` had no consent check of its own.
  - `crates/valori-studio-storage/src/telemetry.rs` — new
    `TelemetryCategory` enum (`Analytics`/`Crash`, matching
    `TelemetryConsent`'s two existing fields — no third category added,
    none was found to exist); `StudioTelemetryEvent` gained a `category`
    field (`#[serde(default)]` to `Analytics` for pre-existing rows); new
    `TelemetryQueue::discard_category(category)` bulk-delete primitive.
  - `desktop/src-tauri/src/telemetry.rs` — `analytics_consent` replaced
    by category-aware `consent_for_category`; `enqueue_telemetry_event`
    gained a `category` parameter; **`drain_queue` (the uploader
    boundary) now re-checks consent per event, per category, immediately
    before dispatching each HTTP request** — not just at enqueue time,
    not cached, not once per batch.
  - `desktop/src-tauri/src/preferences_service.rs` — new
    `discard_revoked_telemetry_categories`, called from
    `set_telemetry_consent_command` immediately after persisting revoked
    consent — the eager half of the invariant; the uploader-boundary
    recheck is the half that makes it safe even if this were skipped.
  - **Bug found and fixed in the same change**: crash events
    (`studio_crashed`) were silently gated by *analytics* consent instead
    of *crash* consent at enqueue time — a user with `crash: true,
    analytics: false` had crash reports dropped despite explicitly
    opting into them. `ui/src/lib/telemetry.ts`'s `send()` now passes an
    explicit category (`"crash"` for `studio_crashed`).
  - Independent consent categories preserved — `TelemetryConsent` is
    still exactly `{ analytics: bool, crash: bool }`, not collapsed or
    extended.
  - 4 new tests in `valori-studio-storage` (105 total), 10 new in
    `desktop/src-tauri` (35 total): existing-queued-event discard,
    multiple-events discard, re-enable-only-allows-new-events,
    independent-crash-consent, restart durability, repeated-drain-tick
    safety.
- **`ui/src/lib/theme.tsx` no longer dual-writes to `studio.redb` and
  `localStorage`** in the desktop app. Now branches on the existing
  `nativeAvailable()` check: desktop reads/writes `studio.redb` only;
  browser/web mode (Valori Cloud, `npm run dev` outside Tauri) keeps
  using `localStorage`, unchanged. One-time, idempotent, non-destructive
  migration for installations that only ever had a legacy `localStorage`
  theme value (backfills `studio.redb` once; never deletes the legacy
  key). No `studio.redb` schema change — `preferences.theme` already
  existed.
- **`docs/architecture/studio-storage.md`** — new §14.5 (Telemetry
  consent enforcement) and §18 (Theme persistence).
  **`docs/architecture/studio-persistence-audit.md`** — both fixed
  findings marked inline, cross-referenced; original audit text
  otherwise preserved as a point-in-time record.

### Added (Phase Studio DR — Database resilience & recovery — 2026-08-08)

Invariant established: `studio.redb` contains recoverable Studio
metadata. It must never be allowed to make Valori Studio permanently
unlaunchable, and corruption of `studio.redb` must never delete or modify
the user's actual Valori project data — verified structurally (the
dependency firewall makes recovery code physically unable to reach
project files) and by test (byte-for-byte hash checks, both in the
automated suite and a real desktop launch).

- **`crates/valori-studio-storage/src/recovery.rs`** (new) —
  `open_with_recovery(db_path, backups_dir, recovery_log_path)`: try
  current → preserve corrupt original (atomic `fs::rename` to
  `studio.redb.corrupt-<unix_ms>`, never deleted) → try bounded rolling
  backup generations (`$VALORI_HOME/backups/studio.redb.{1,2,3}`,
  newest-first, each validated read-only via `Database::open` before
  restoring) → fresh-database fallback. Never fails for a condition a
  fresh database can resolve.
  - `RecoveryOutcome` (`Healthy` / `RestoredFromBackup` /
    `FreshDatabaseCreated`) and `RecoveryState` (`Healthy`/
    `RecoveryRequired`/`RestoringBackup`/`Rebuilding`/`Recovered`/
    `RecoveryFailed`).
  - Backups taken before a schema migration (so a migration failure has
    the pre-migration state to fall back to) and at most once per 24h on
    a healthy open — never on a preference write, telemetry enqueue, or
    any other hot path.
  - `DatabaseAlreadyOpen` (another process/handle holding the file open)
    is explicitly never treated as corruption — recovering a database
    that's merely locked would be actively destructive.
  - Crash-safe and idempotent: a process killed between "preserve" and
    "restore" is detected and resumed correctly on the next launch, by
    deriving state purely from the filesystem (no separate lock file).
  - Append-only `$VALORI_HOME/studio-recovery.jsonl` — a **sibling** of
    `studio.redb`, so a corruption event that destroys the database can't
    also destroy the record that corruption happened. Never logs
    preference values, telemetry payloads, project content, or
    credentials.
  - Evidence-based rebuild classification documented in the module's own
    doc comment: `preferences` restores-from-backup-or-safe-defaults;
    `update_state`/`telemetry_queue`/`sessions` are trivially
    rebuildable/disposable; `sync_state` is Cloud-re-derivable; the
    `projects` registry is **not** auto-rebuilt (no `project.json` parser
    exists in this crate and adding one would violate the dependency
    firewall); WAL/snapshots/vectors/indexes are never touched — this
    crate has no code path to them.
- **`StudioDatabase::open_default_with_recovery()`** — the recovery-aware
  entry point at the default paths, alongside the existing plain
  `open`/`open_default`.
- **`desktop/src-tauri/src/studio_storage.rs`** — startup now calls
  `open_with_recovery` instead of the plain open; new
  `RecoveryStatusDto` (camelCase fields, snake_case `"kind"` tag — pinned
  by a dedicated wire-shape test) + `get_studio_recovery_status` Tauri
  command + a `studio-recovery` event emitted once during `setup()`.
- **`ui/src/lib/native.ts`** / **`AppShellGate.tsx`** — `StudioRecoveryStatus`
  binding; a non-blocking toast (reusing the existing toast system, no
  new UI component) on any non-healthy recovery outcome; silent on a
  healthy launch.
- **Tests** — 13 new in `crates/valori-studio-storage/tests/recovery.rs`
  (101 total in the crate) covering healthy/corrupt-with-backup/
  corrupt-no-backup/multiple-backups-mixed-validity/pre-migration-backup/
  idempotency/crash-resume/cross-process-lock-safety/project-data-integrity/
  recovery-log-content; 2 new in `desktop/src-tauri` (25 total). Plus a
  real desktop application launch against a disposable `$VALORI_HOME`
  (never the developer's production `~/.valori`) exercising all four
  scenarios (healthy, corrupt+no-backup, corrupt+valid-backup, healthy
  relaunch) with SHA-256-verified project-file integrity.
- **`docs/architecture/studio-storage.md`** — §10 rewritten (Corruption
  behavior and recovery), new §15 (Recovery UI), §16 (Logging), §17
  (Concurrency and recovery ordering); §13's startup diagram updated.

### Changed (Phase Studio S2b-2d & S2b-2d.1 — Telemetry Queue & Consent Boundary Migration — 2026-08-08)

- **`desktop/src-tauri/src/telemetry.rs`** — rewired `enqueue()` and
  `drain_queue()` from `events.jsonl` file I/O to `studio.redb`'s
  `telemetry_queue` table via `TelemetryQueue`. Removed `QUEUE_LOCK`,
  `queue_path()`, `QUEUE_FILE`, `MAX_QUEUE_LINES`. `events.jsonl` is now a
  read-only legacy artifact — this module never writes to it.
- **Canonical Consent Routing (S2b-2d.1)** — `analytics_consent()` now resolves
  consent exclusively through the managed `StudioPreferencesService` (`app.try_state::<StudioPreferencesService>()`).
  Telemetry no longer accesses `studio.redb`'s `preferences` table directly.
  Consent decisions are strictly separated from telemetry queue persistence:
  `Telemetry -> StudioPreferencesService -> StudioDatabase -> preferences` for consent decisions,
  `Telemetry -> TelemetryStore -> studio.redb` for telemetry queuing.
- **`desktop/src-tauri/src/lib.rs`** — registered `StudioPreferencesService` as
  managed Tauri state (`app.manage(StudioPreferencesService::new(studio_db.clone()))`) so
  all Rust-native consumers can access it without bypassing the service boundary.
- **Drain improvements** — `drain_queue()` now calls `mark_delivered()` on
  success and `increment_retry()` on failure (per-event retry metadata); a
  `prune_older_than(7 days)` backstop runs each tick, closing the gap the
  file-based sender had (no time-based eviction at all).
- **`installation_id` at drain time** — `installation_id` is no longer stored
  per queued event; it is read once per drain tick from the preferences table
  and stamped on all wire envelopes for that tick.
- **`build_wire_envelope()`** (new helper) — converts `StudioTelemetryEvent`
  → `TelemetryEnvelope` (wire format) by re-hydrating `schema`, `source`,
  `version`, `platform`, `arch` from constants + `get_app_info()` at send
  time. Wire format (`TelemetryEnvelope`, `TELEMETRY_ENDPOINT`, `SCHEMA`,
  `SOURCE`) is unchanged.
- **`DRAIN_BATCH_SIZE = 50`** — caps the in-memory batch per drain tick;
  sender loops every 60 s.
- **`lib.rs`** — updated comment on `spawn_sender(...)` to reflect the new
  queue backend.

### Added (Phase Studio S2b-2c — Session Store Runtime Migration — 2026-08-08)

- **`desktop/src-tauri/src/session_service.rs`** (new) — typed `SessionService`
  and Tauri commands (`session_get_current`, `session_list_recent`, `session_end_current`)
  backed by `studio.redb`'s `sessions` table using canonical `valori_domain::SessionId`.
- **Application Process Session Lifecycle** — active session started in Tauri `setup()` with
  version, platform, and installation ID. Clean application shutdown recorded in `shutdown_and_exit()`
  with duration calculation.
- **Crash Reconciliation** — next application startup scans for prior unended sessions
  (`ended_at.is_none()`), marking them as `crashed: true` with `ended_at` populated.
- **Idempotent Lifecycle** — `start()` is idempotent for React dev-mode remounts without
  generating duplicate session records or mutating `started_at`.
- **`crates/valori-studio-storage/tests/startup_integration.rs`** — added `session_runtime_lifecycle_and_crash_reconciliation`
  test proving startup session persistence, clean shutdown, crash flagging across restarts, and untouched `preferences.json`.
- **Telemetry Queue & Uploader Independence** — `events.jsonl`, `telemetry_queue`, and background sender
  remain independent and will be addressed in S2b-2d.

### Added (Phase Studio S2b-2b — Project & Recent Project Registry Migration — 2026-08-08)

- **`desktop/src-tauri/src/project_registry_service.rs`** (new) — typed `ProjectRegistryService`
  and Tauri commands (`registry_list_projects`, `registry_get_project`, `registry_recent_projects`,
  `registry_favorite_projects`, `registry_register_local_project`, `registry_register_cloud_project`,
  `registry_rename_project`, `registry_set_local_path`, `registry_set_favorite`,
  `registry_touch_last_opened`, `registry_unregister_project`, `registry_reconcile_legacy_names`)
  backed by `studio.redb`'s `projects` table using canonical `valori_domain::ProjectId`.
- **Registry vs Storage Separation** — `studio.redb` acts strictly as Studio's reference/index layer.
  Actual local project storage (`~/.valori/projects/<name>/` — vectors, WAL, snapshots, indexes, collections)
  remains owned by `valori-daemon` / `valori-metadata` / engine.
- **`ui/src/lib/native.ts`** — migrated `getRecentProjects`, `touchRecentProject`, `getLastOpenedProject`,
  `getFavoriteProjects`, `toggleFavoriteProject`, `forgetProject` to route through typed Tauri registry commands.
- **Identity & Availability Invariants** — renames and moves preserve `ProjectId`; missing local project directories
  report `available: false` without deleting registry records; recents are derived by `ORDER BY last_opened_at DESC`.
- **`crates/valori-studio-storage/tests/startup_integration.rs`** — added project registry runtime lifecycle test
  verifying canonical `ProjectId` preservation, legacy name reconciliation, and untouched legacy `preferences.json`.
- **Remaining consumers (sessions, telemetry queue uploader, sync, updates)** deferred to S2b-2c..e.

### Added (Phase Studio S2b-2a — Preferences Runtime Consumer Migration — 2026-08-08)

- **`desktop/src-tauri/src/preferences_service.rs`** (new) — typed `StudioPreferencesService`
  and Tauri commands (`get_preference`, `set_preference`, `get_all_preferences`, `get_installation_id_command`,
  `get_telemetry_consent_command`, `set_telemetry_consent_command`) backed by `studio.redb`'s `preferences` table.
- **`desktop/src-tauri/src/telemetry.rs`** — migrated `analytics_consent` and `installation_id`
  from `preferences.json` to `Arc<StudioDatabase>` (`studio.redb`), lazily generating a permanent UUID `installation_id`.
- **`ui/src/lib/native.ts` & `ui/src/lib/theme.tsx`** — replaced `tauri-plugin-store` / `LazyStore("preferences.json")`
  with typed Tauri preference commands, persisting theme, telemetry consent, onboarding status, and last page into `studio.redb`.
  **Legacy `preferences.json` is preserved byte-for-byte unmodified.**
- **`crates/valori-studio-storage/tests/startup_integration.rs`** — added runtime preference flow
  integration test verifying theme changes, telemetry consent updates, and permanent `installation_id` across restarts.
- **Remaining consumers (projects, sessions, telemetry queue uploader, sync, updates)** deferred to S2b-2b..e.

### Added (Phase Studio S2b-1 — Real Startup Migration Integration — 2026-08-08)

- **`desktop/src-tauri/src/studio_storage.rs`** (new) — wires `StudioDatabase`
  and the S2a migration engine into the real Tauri desktop startup lifecycle.
  Resolves real on-disk legacy paths via Tauri's `app.path().app_config_dir()`,
  opens/creates `$VALORI_HOME/studio.redb` (or `~/.valori/studio.redb`),
  runs legacy migration idempotently, logs non-sensitive progress diagnostics,
  and manages `Arc<StudioDatabase>` in Tauri application state.
  **Legacy files are never modified, deleted, or renamed.**
- **`crates/valori-studio-storage/tests/startup_integration.rs`** (new, 5 tests) —
  tests the full startup migration boundary with temporary fixtures: fresh install,
  existing install with legacy files, idempotency on restart, fail-safe behavior
  on corrupt legacy data, and non-destruction of unrelated metadata.redb/project files.
- **Runtime consumers are NOT yet migrated** — S2b-2 deferred.

### Added (Phase Studio S2a — Legacy Studio persistence migration engine — 2026-08-08)

Migration **engine** only, per explicit review instruction to split S2 into
S2a (migration) and S2b (application wiring), with a review checkpoint
between them. **Not wired into `desktop/src-tauri`. Legacy files are read
but never written, renamed, or deleted.** No runtime consumer changed.

- **`crates/valori-studio-storage/src/migration.rs`** (new) — one-time,
  idempotent, transactional import of `preferences.json` and
  `events.jsonl` into `studio.redb`. Five-step contract: detect (a `meta`
  flag short-circuits a second call) → validate (whole-file for
  preferences, per-line for telemetry — a malformed line is skipped and
  reported, not fatal) → import transactionally (data + completed-flag in
  one redb write transaction) → verify (a fresh read transaction confirms
  the write) → mark complete (the flag itself).
  - `preferences.json` fields (`onboardingVersion`, `telemetryConsent`,
    `installationId`, `lastPage`) **merge** onto any pre-existing
    `StudioPreferences` row — never a blind overwrite.
  - `recentProjects`/`favoriteProjects`/`lastOpenedProject` are name-only
    in the legacy source (no `ProjectId`) — preserved losslessly in
    `meta.legacy_project_names`, deliberately **not** written into the
    `ProjectId`-keyed `projects` table (minting a fresh id per name would
    create an identity the daemon's own `project.json` doesn't know
    about). A later phase reconciles these by name against the daemon's
    real project list.
  - `events.jsonl` envelopes import with RFC3339 `timestamp` → unix-ms
    `created_at`, `session_id` parsed as `valori_domain::SessionId`
    (invalid values are skipped, not fatal), and respect
    `TelemetryQueue::MAX_QUEUE_LEN` at import time — the newest 500 by
    timestamp survive, same policy live `enqueue()` already enforces.
  - Neither function ever writes to, renames, or deletes the legacy file —
    `std::fs::read` only, proven by a byte-for-byte before/after test.
  - No credential-shaped field is migrated — `preferences.json`'s real
    shape has none, and the typed-field deserialization silently drops
    anything not explicitly modeled (e.g. a hypothetical `apiKey`), rather
    than copying it through.
- **`StudioDatabase`** — new methods `migrate_legacy_preferences[_from_path]`,
  `migrate_legacy_telemetry_queue[_from_path]`, `run_legacy_migration`,
  `legacy_project_names`; new public types `LegacyStudioPaths`,
  `LegacyMigrationSummary`, `MigrationReport`, `SkippedRecord`,
  `LegacyProjectNames`.
- **`StudioPreferences`** — added `installation_id: Option<InstallationId>`
  (a genuine singleton fact, unlike the name-only project lists). Purely
  additive; no schema/table version change.
- **New dependency**: `chrono` (RFC3339 parsing in `migration.rs` only, no
  `"clock"` feature — this crate still never reads the system clock
  itself).
- **`docs/architecture/studio-storage.md`** — new §6.5 "Legacy data
  migration (S2a)"; §12 extended with the target `provider`/`model`/
  `credential_ref` + OS-keychain architecture for a future security phase
  (documented, not implemented); status and cross-references updated.
- **Tests** — 19 new (`crates/valori-studio-storage/tests/migration.rs`),
  77 total in the crate, 0 failed.

### Added (Phase Studio S1 — Durable Studio storage — 2026-08-08)

Storage foundation only, per `docs/architecture/studio-storage-audit.md` (the
read-only audit this phase implements). **No existing Studio persistence
touched or migrated** — `preferences.json`, `tauri-plugin-store`,
`events.jsonl`, `localStorage`, the existing telemetry sender, the existing
updater, and `desktop/src-tauri` itself are all unchanged; the new crate is
not yet consumed anywhere. See `docs/phases/phase-studio-S1-durable-storage.md`
for the full validation record.

- **`crates/valori-studio-storage`** (new crate) — durable Studio-local
  metadata store, `~/.valori/studio.redb` (override with `$VALORI_HOME`),
  entirely separate from `~/.valori/metadata.redb` and any Raft `redb`
  file. `StudioDatabase` is the single typed owner; no `redb::Database` is
  exposed publicly.
  - `preferences` — `StudioPreferences` (theme, language, accent color,
    onboarding version, telemetry consent, window state, last page).
  - `projects` — `StudioProjectRecord` (local path or cloud reference,
    favorite, last-opened, registered-at), keyed by `valori_domain::ProjectId`
    with identity-preserving upsert semantics (rename/path-change/
    re-registration never mint a new id or lose `favorite`/`registered_at`).
  - `project_cache` — disposable display cache, independent of `projects`;
    clearing it cannot affect the registry.
  - `sessions` — Studio **application** sessions (launch→exit), explicitly
    distinct from a Valori execution or Cloud deployment.
  - `telemetry_queue` — durable, bounded (`MAX_QUEUE_LEN = 500`) queue;
    delivered events are deleted, not flagged, so the table cannot grow
    into an unbounded history.
  - `sync_state` / `update_state` — Studio-side sync bookkeeping (Cloud
    stays authoritative) and updater state.
  - Explicit `meta.schema_version` (currently `1`) with a migration
    scaffold: opening a database from a newer schema version than the
    build supports fails clearly and leaves the file untouched; opening an
    older or pre-versioning one is additive-only, never destructive.
  - JSON (`serde_json`) serialization throughout, matching
    `valori-metadata::MetadataDb`'s existing convention; every stored
    struct is forward-compatible via `#[serde(default)]`.
  - Sealed in `crates/valori-node/tests/dependency_direction.rs`: may
    depend on `valori-domain` only, never `valori-daemon`/`valori-node`/
    `valori-metadata`/`valori-consensus`/any Cloud crate.
  - 58 tests: database lifecycle (fresh/reopen/schema version/unsupported
    future version/corrupt file/pre-versioning backward-compat fixture),
    per-store CRUD + reopen, and concurrency (concurrent writers/readers,
    panicking-transaction safety, reopen after concurrent load).
- **`docs/architecture/studio-storage.md`** (new) — the crate's contract:
  ownership, schema, serialization, versioning/migration, concurrency,
  durability, corruption behavior, backward compatibility, and what must
  never enter `studio.redb` (secrets; project/vector/WAL/snapshot data
  owned elsewhere).
- **`crates/valori-node/tests/dependency_direction.rs`** — added
  `valori-studio-storage` to `SEALED_CRATES` (allowlist: `valori-domain`
  only) and `OSS_PLATFORM_CORE` (Cloud-concept ban applies to it too), and
  its expected edge to `EXPECTED_EDGES`. No existing rule weakened.

### Added (Phase M0–M2 — Platform contracts — 2026-08-08)

Implements Stage 2 (steps M0–M2) of [`ARCHITECTURE_AUDIT.md`](ARCHITECTURE_AUDIT.md). **No existing behaviour, file format or wire format changed** — every addition is additive and no duplicate implementation was removed (that is step M3, deliberately not executed).

- **`crates/valori-node/tests/dependency_direction.rs`** — architecture tripwire that makes the crate dependency graph mechanically enforceable. Parses every `crates/*/Cargo.toml` (shipped deps only; dev-deps excluded with a documented reason) and asserts: the graph is acyclic; `valori-core`/`valori-kernel`/`valori-domain` depend only on their allowlists; the determinism-critical crates (kernel, wire, storage, state, index, rag, verify) cannot reach `valori-domain` even transitively; no crate depends on `valori-cloud-*`; and Cloud-only identity concepts (`OrganizationId`, `UserId`, `BillingAccountId`, `SubscriptionId`, `DeploymentId`, `WorkerId`) are not *defined* in the OSS platform core. Runs in the existing CI `cargo test -p valori-kernel -p valori-node` job.
- **`crates/valori-domain`** (new crate; `valori-core` is its only workspace dependency) — cross-boundary platform vocabulary, std-only, sealed and firewalled from the kernel.
  - `id` — `ProjectId`, `SessionId`, `InstallationId` (UUID-backed); `ModelId` (`provider/model-name` slug); `SnapshotId` (opaque handle over a storage-owned object key). Re-exports `CollectionId`, `NamespaceId`, `ExecutionId` from `valori-core` rather than redefining them. Every ID is `#[serde(transparent)]`, so it has the same JSON form as the `String` it will eventually replace.
  - `project` — canonical `Project`, plus `ProjectName` (validated filesystem-safe), `IndexKind`, `ProjectTopology` (`{ replicas, shards }` as `NonZeroU8`; cluster-ness derived, never stored), `Timestamp` (unix seconds), `LocalProject` (identity + location), and `ApiProject` (the HTTP wire contract).
  - `error` — `DomainError`, `Result<T>`.
  - `RuntimeId` and `PipelineId` are **deliberately not built** — neither has a real consumer; the conditions that would unblock them are documented in `id.rs`.
- **`crates/valori-daemon/src/domain_adapter.rs`** — `manifest_to_domain` / `manifest_from_domain` between `ProjectManifest` and the domain model. Intentionally not a `From` impl: `manifest_from_domain` mutates an existing manifest so `workspace`, `restart_policy`, `embedding`, `storage` and cluster port allocations cannot be silently defaulted away. Rejects malformed ids, unknown index kinds and shard counts above 255 rather than coercing them.
- **`crates/valori-metadata/src/domain_adapter.rs`** — `record_to_domain(record, id)` / `record_from_domain` between the redb control-plane record and the domain model. Requires the caller to supply the `ProjectId` because the record has none — making the `name → id` gap explicit instead of minting a fresh identity on every read. `mode` is recomputed from topology, so it can no longer contradict `node_count`.
- **`docs/architecture/ownership.md`** — the architecture constitution: concept→owner registry (OSS vs private Cloud, persistence/API/UI per concept), admission rules for `valori-domain`, the domain≠persistence≠API≠UI separation, the identity rule, the single-execution-engine rule (`PipelineEngine`/`WorkflowEngine`/`JobEngine`/`TaskEngine` forbidden by name — extend `OperationKind`/`TaskKind` instead), the three-runtimes table (process / AI-model / hosted inference stay separately named), the split-provider-trait direction for M4, and the deferred extension points.
- **`ARCHITECTURE_AUDIT.md`** — Stage 1 audit: current crate/desktop/Next.js architecture, duplicate concepts, the four divergent `Project` implementations, ID inventory, event models, API contract situation, capability-system scope, OSS/private boundary, missing abstractions, migration risks and the recommended target architecture and sequence.
- **Tests** — 50 new: 6 dependency-direction, 14 ID wire-compat, 16 `Project`/`ApiProject` contract, 8 daemon adapter, 6 metadata adapter.
- **Phase M2.1 — review repairs.** Post-M2 review (`docs/reviews/m2-project-review.md`) found and fixed seven defects before any consumer migration:
  - **F1 (critical)** — `#[serde(transparent)]` bypassed every validated newtype's constructor, so `serde_json::from_str::<ProjectName>("\"../../etc/passwd\"")` succeeded while `ProjectName::parse` correctly failed. New `crates/valori-domain/src/validate.rs` routes `Deserialize` through the canonical `parse()` for `ProjectName`, `ModelId` and `SnapshotId`; validation is defined once and cannot drift. `Serialize` is untouched, so emitted JSON is byte-identical. `ProjectId`/`SessionId`/`InstallationId` (via `Uuid`) and `ProjectTopology` (via `NonZeroU8`) were already safe, and tests now assert that rather than assuming it.
  - **F2** — `ProjectName` implemented the stricter UI rule rather than the daemon's, so daemon-created projects named `_scratch`, `-tmp` or 64 characters long could not be represented — and `ProjectStore::list()` would have silently dropped them. `ProjectName::parse` now implements the daemon contract (≤64 bytes, `[A-Za-z0-9_-]`), and the stricter rule became a separate creation policy, `ProjectName::check_new_project_policy()`. Path traversal remains unrepresentable.
  - **F3** — `ProjectManifest.id` defaulted to a freshly minted UUID, so a manifest written before the field existed produced a *different* id on every read. It now defaults to empty, and `JsonProjectStore::get()` backfills and persists one id exactly once. The id is random, never derived from name or path; existing ids are never reassigned; an unwritable manifest logs a warning rather than failing the listing.
  - **F4** — `manifest_from_domain` silently discarded cluster → standalone demotion, leaving a stale cluster block on disk. It now returns `Result` and rejects the transition with `UnsupportedTopologyChange`.
  - **F5** — `ApiProject.is_cluster` could contradict `replicas` and was silently ignored; `TryFrom<ApiProject>` now rejects inconsistent payloads.
  - **F6** — both adapters saturated `dim` across the `usize`/`u32`/`u16` width mismatch, silently rewriting a dimension that is immutable after first insert. Both now return `DimensionOutOfRange`.
  - **F7** — `index_from_domain` ended in `.unwrap_or_default()`, silently rewriting an unmatched variant to `Brute`. Now an exhaustive `match`, so enum drift is a compile error.
  - **Tests** — +37, including the new `crates/valori-domain/tests/invariants.rs` matrix (every type through constructor, serialize, deserialize, invalid input, persistence boundary and adapter boundary) and eight daemon identity-stability tests. Combined suite: **552 passing, 0 failing**.
  - `record_count` was **not** added back to the canonical model; public API field names were **not** changed; the `apiKey` vs `api_key_ref` credential divergence is documented as a security migration item, not implemented.

### Added (Phase P8 — CI hardening — 2026-07-16)

- **`.github/workflows/ci.yml`** — two new parallel jobs:
  - `coverage` — installs `cargo-llvm-cov` via `taiki-e/install-action` (prebuilt, no compile), runs `cargo llvm-cov --package valori-kernel --lcov`, uploads `lcov.info` as a 14-day artifact, writes a `--summary-only` table to `$GITHUB_STEP_SUMMARY`. Does not gate on a threshold (baseline tracked in K3 doc).
  - `miri` — nightly toolchain + `miri` component; runs `cargo miri test -p valori-kernel --test fxp` (Q16.16 arithmetic UB) and `--test proof` (Merkle root + InsertReceipt UB) with `MIRIFLAGS=-Zmiri-disable-isolation`. Blocks merge on Miri errors.
- **`.github/actions/rust-setup/action.yml`** — composite action extended with `toolchain` (default `stable`) and `components` inputs. Switches from `dtolnay/rust-toolchain@stable` to `@master` with the configurable channel. All existing callers are unaffected (they omit both new inputs and get the same stable/no-components behavior as before).

### Added (Phase P6 — InsertReceipt cryptographic receipts — 2026-07-16)

- **`valori-kernel/src/proof.rs`** — `InsertReceipt` struct: `{ record_id, old_root, new_root, proof, sequence, timestamp, state_hash }`. `build()` computes `proof` via `generate_proof_bytes` (Merkle root of Q16.16 FXP values) and `state_hash` as `BLAKE3("valori-insert-receipt-v1" ‖ fields)`. `verify()` recomputes the self-hash and returns `true` iff the receipt is unaltered.
- **`valori-node/src/api.rs`** — `InsertReceiptJson` (hex-string HTTP form) + `From<InsertReceipt>` impl; `InsertRecordResponse { id, receipt }` (backward-compatible: old clients that only read `id` are unaffected).
- **`valori-node/src/server.rs`** (standalone) — `POST /v1/records` now returns the full receipt: `old_root` captured before insert, FXP values converted from `payload.values`, `new_root` and `sequence` captured from a post-insert read lock.
- **`valori-node/src/cluster_server.rs`** (cluster) — same receipt in `InsertResponse`; `sequence` = `resp.log_index`, `new_root` = `resp.state_hash` from `ClientResponse`.
- **`python/valoricore/remote.py`** — `insert_with_receipt(vector, ...)` on both `SyncRemoteClient` and `AsyncRemoteClient`; returns the `receipt` dict from the HTTP response.
- **Tests** — 5 new `InsertReceipt` tests in `crates/valori-kernel/tests/proof.rs`: `verify_roundtrip`, `verify_detects_tampering` (record_id / sequence / new_root), `deterministic`, `proof_field_matches_generate_proof_bytes`, `state_hash_differs_from_roots`. Kernel test count: 153 (was 148).

### Added (Phase K4 — Snapshot version migration tests — 2026-07-16)

- **`crates/valori-kernel/tests/snapshot_version_migration.rs`** (new) — 10 tests covering every `schema_ver` 1–6 backward-compat branch in `decode_state`, which were previously untested dead code under the test suite (the encoder always writes the current version). Includes: `v1_decodes_correctly`..`v6_decodes_correctly` (per-version field assertions), `v1_hole_slot_decodes_as_absent_without_shifting_ids`, `cross_version_decode_reencode_chain_is_hash_stable` (decode → hash vs. reference → reencode → decode → fixed-point for every V1–V7), `v6_out_of_range_namespace_head_is_rejected`, `schema_version_zero_is_rejected`. Mutation-tested: disabling the V1–V3 incoming-edge reconstruction block in `decode.rs` causes exactly the right 4 failures; `v4/v5/v6` stay green. Kernel test count: 148 (was 138).

### Added (Phase D1.3 — Installers + clean-machine validation groundwork — 2026-07-13)

- **Fixed**: two API route handlers (`api/records/[id]/route.ts`, `.../metadata/route.ts`) used Next.js 14's synchronous `params` signature, which blocked `next build` outright on this repo's Next.js version. Fixed to `params: Promise<...>` + `await`, matching the convention already used elsewhere.
- **`ui/` bundled as a Node sidecar**: `desktop/scripts/prepare-ui-server.mjs` (new, packages `ui/`'s `next build --output standalone` as a Tauri bundle resource, including the manual `.next/static` copy Next's standalone output omits); `desktop/src-tauri/src/ui_server_manager.rs` (new, release-only — spawns the bundled `node` sidecar against it on a fixed loopback port, then navigates the main window from a "Starting Valori…" loading page to the real app once healthy). `tauri dev` is unaffected.
- **First real `tauri build` in this project** — produced and verified `Valori.app` + `Valori_0.1.0_aarch64.dmg` (checksum-verified via `hdiutil verify`). All 4 sidecars (`valori-desktop`, `valori-daemon`, `valori-node`, `node`) and the bundled `ui-server` resource confirmed correctly placed; confirmed via real launch that the bundled ui-server actually serves the app (not just that the build succeeded).
- **Fixed two real shutdown bugs found via launch-testing, not inspection**: (1) a raw SIGTERM (session logout, `killall`, force-quit) bypassed the graceful `ExitRequested` handler entirely, orphaning the bundled ui-server process and leaving its port held — fixed with a `#[cfg(unix)]` SIGTERM handler; (2) the existing `ExitRequested` handler's own call to `AppHandle::exit()` re-triggers `ExitRequested` (per Tauri's docs) — a real infinite-loop-on-quit risk that had never been exercised until now — fixed with a shared `Arc<AtomicBool>` shutdown guard.
- **`.github/workflows/desktop-build.yml`** (new) — macOS/Windows/Linux matrix build producing each platform's installer (`.dmg`/`.msi`/`.AppImage`) as a CI artifact. Signing/notarization explicitly deferred to Phase D1.4.
- **`docs/architecture/desktop-layout.md`** (new) — real app-bundle and workspace directory layout, startup sequence, fixed ports.
- **`docs/DESKTOP_RELEASE_CHECKLIST.md`** (new) — manual clean-machine smoke test steps, deliberately not automated this phase.

### Added (Phase D3.1 — Bundle the daemon and node as Tauri sidecars — 2026-07-13)

- **`desktop/scripts/prepare-sidecars.mjs`** (new) — resolves host target triple, locates/builds `valori-daemon` + `valori-node`, copies them into `src-tauri/binaries/<name>-<triple>[.exe]` per Tauri's `externalBin` naming convention. `--release` always rebuilds in release mode; dev mode reuses whatever's already built.
- **`desktop/scripts/dev.mjs`** (new) — new `beforeDevCommand`: preps sidecars synchronously, then starts `ui/`'s dev server. Required because Tauri's build script validates `externalBin` resource paths on every cargo build, not just `tauri build`.
- **`bundle.externalBin`** in `tauri.conf.json` — bundles both binaries into the app; `beforeBuildCommand` now runs `prepare-sidecars.mjs --release`.
- **`desktop/src-tauri/src/daemon_manager.rs`** — rewritten around exactly two code paths (per explicit user direction, no env-var override): dev-mode `target/{release,debug}` search vs. release-mode Tauri sidecar spawn (`tauri-plugin-shell`). Adds a version handshake (`GET /version` api-level check, `UnsupportedVersion` error on mismatch instead of a later mysterious failure) and `VALORI_NODE_BIN` wiring so the daemon sidecar can find its bundled `valori-node` sidecar with no Cargo/target-dir assumption on the end user's machine.
- `tauri-plugin-shell = "2"` added to `desktop/src-tauri/Cargo.toml`.

### Added (Phase D3 — Desktop launches and manages the daemon — 2026-07-13)

- **`POST /v1/shutdown`** (`valori-daemon`) — graceful, cross-platform daemon shutdown over HTTP; snapshots every running project before the process exits. Exists because OS signal semantics aren't uniform across macOS/Linux/Windows for a process spawned and supervised by another process (the desktop app).
- **Fixed:** `Runtime::stop_all()` previously hard-killed every supervised node with no snapshot on daemon shutdown (Ctrl-C or desktop close) — a real durability gap, since `stop()` for a single project always snapshotted first. Now `stop_all()` does the same snapshot-then-terminate for every node.
- **`desktop/src-tauri/src/daemon_manager.rs`** — desktop now supervises the `valori-daemon` process directly: `start_daemon` (spawns it with `VALORI_HOME` from the user's chosen workspace, polls `/health`, no-ops if already running), `stop_daemon` (calls `POST /v1/shutdown`, falls back to a hard kill if it doesn't exit), `daemon_status`. An `ExitRequested` hook calls the graceful shutdown before the desktop window is allowed to close.
- **`ui/src/lib/native.ts`** — `startDaemon`/`stopDaemon`/`daemonStatus` bridge functions.
- Welcome wizard's workspace folder choice now actually becomes `VALORI_HOME` (`Welcome.tsx` calls `startDaemon(workspaceDir)` on finish); returning users get the daemon started automatically against their persisted workspace on every launch (`AppShellGate.tsx`).
- **Fixed:** `crates/valori-daemon/tests/lifecycle.rs::supervisor_restarts_crashed_node` had a pre-existing race (asserted the crash would be visible on the very first `supervise_tick()` after `kill -9`, which isn't guaranteed) — now polls until the restart lands instead of asserting on a specific tick.

### Added (Phases M5–M6 — Package Store + Integrity Manager — 2026-07-13)

- **`PackageStore`** — on-disk package manager with `<root>/<task>/<sanitized-id>/manifest.json` layout; `register()` (remote/no-download), `install()` (atomic download + rename), `commit_staged()`, `remove()`, `repair()`, `list()`, `find_by_task()`, `disk_usage()`, `exists()`, `get()`, `acquired_lock()`.
- **`PackageManifest`** (M5.3) — versioned per-package manifest: `schema_version`, `package_version`, `created`, `updated`, `size`; wraps `ModelManifest`.
- **`InstallLock`** (M5.2) — RAII exclusive lock via `OpenOptions::create_new`; prevents concurrent installs from two processes; released on drop.
- Atomic install (M5.1): download → `.tmp/<timestamp>/model.bin` → SHA-256 verify → `fs::rename` → write `manifest.json`; stale `.tmp/` entries cleaned on `PackageStore::new()`.
- **`IntegrityManager`** (M6) — `verify(id)` + `verify_all()` → `Vec<IntegrityReport>` with `IntegrityStatus`: Verified / Remote / Missing / Unverified / Corrupted.
- **`repair_package(store, id)`** (M6) — returns `RepairAction`: AlreadyHealthy / SizeRepaired / NeedsReinstall { download_url }.
- **`RefCounter`** (M6.2) — in-memory model→project reference tracking; `add_ref`, `remove_ref`, `ref_count`, `can_delete`, `all_referenced_ids`, `referencing_projects`.
- **`GarbageCollector`** (M6.1) — `scan(&refs)` → `GcReport { unreferenced, reclaimable_bytes }`; `clean(&refs)` → removes all unreferenced; `safe_delete(id, &refs)` → errors if model in use.
- **`SystemHealth`** / **`PackageHealth`** (M6.3) — per-package health (Verified / Installed / Missing / Corrupted + size + ref_count); aggregate totals (total_installed, verified, corrupted, missing, disk_used_bytes, reclaimable_bytes).
- **`GET /v1/models/health`** — added to both standalone and cluster routers; reads `VALORI_MODELS_DIR` (default: `~/.valori/models`); returns `SystemHealth` JSON.
- `ModelError::InstallConflict` — new error variant for lock contention.
- `dirs = "5"` added to `valori-node` deps for home-dir resolution.

### Added (Phases M1–M4 — valori-models Package Manager — 2026-07-13)

- **`ModelManifest`** — replaces `InstalledModel` + `ModelSpec`; 15 typed fields: `provider: ProviderKind`, `task: ModelTask`, `format: ModelFormat`, `status: ManifestStatus`, `family`, `quantization`, `min_ram_mb`, `license`, `homepage`, `download_url`.
- **`ModelTask`** — `Embedding | Generation | Reranker | Vision | Speech`
- **`ModelFormat`** — `Onnx | Gguf | Safetensors | Remote`
- **`ProviderKind`** — `OpenAI | Ollama | Voyage | Anthropic | AzureOpenAI | Custom | Local | Dummy`; `as_str()` / `from_str()`.
- **`ManifestStatus`** — `Available | Queued | Downloading { progress_bytes, total_bytes } | Paused | Verifying | Installed | Failed { reason }`
- **`ProviderRegistry`** + **`ProviderFactory`** — eliminates all `match kind` dispatch; `register()`, `build()`, `build_from_manifest()`, `provider_kinds()`; pre-loaded with Ollama / OpenAI / Voyage / Custom / Dummy factories.
- **`Resolver`** — `resolve(task, dim?)` selects best installed model; `compatible_embedding_models(dim)`, `resolve_for_embedding(dim)`.
- **`DownloadJob`** + **`DownloadState`** + **`DownloadEvent`** — M4 download state machine with channel-based progress events and cancellation token.
- **`ModelStore::update()`** — in-place manifest update (status, path, sha256 after install).
- **Built-in catalog** enriched with `family`, `license`, `homepage`, `min_ram_mb`, `download_url` for all 11 entries.
- **`ModelManager`** gains: `all_manifests()`, `disk_usage_bytes()`, `catalog_json()`, `resolve()`, `resolve_for_collection(dim)`, `provider_for(id)`, `provider_from_config()`.
- `provider_from_config` now delegates to `ProviderRegistry` (backward compat shim for node env-var path).

### Added (Phase E4 — Ingest Pipeline Observability — 2026-07-13)

- **`CancellationToken`** — `Arc<AtomicBool>`-backed, `Clone`; `check()` returns `Err(IngestError::Cancelled)` when triggered; checked between each pipeline stage.
- **`RetryPolicy`** — `Never | Fixed { attempts, delay_ms } | Exponential { max_attempts, base_delay_ms, max_delay_ms }`; async `execute(FnMut() -> Fut)`; applied to the embedder stage.
- **`PipelineConfig`** — `{ batch_size, retry, timeout_secs }` with builder methods; default = original behavior (no retry, one batch, no timeout); `batch_size` enables streaming (embed+write N chunks before moving to the next N).
- **`PipelineHook`** — observer trait with 6 default no-op methods (`after_read`, `before_chunk`, `after_chunk`, `before_embed`, `after_embed`, `after_write`); multiple hooks stack; `NoopHook` for tests.
- **`ProgressEvent`** — typed channel events: `StageStarted`, `ChunkProgress { completed, total }`, `StageCompleted { stage, duration_ms }`, `Done`, `Failed`; optional `ProgressSender` passed to `run_observed`.
- **`StageMetrics` / `StageResult` / `PipelineResult`** — per-stage timing, counters, and warnings; `PipelineResult::summary()`, `stage()`, `all_warnings()`.
- **`IngestPipeline::run_observed()`** — full observable entry point; `run()` stays backward-compatible.
- **`WriteResult`** — added `Serialize/Deserialize` (required by `PipelineResult`).

### Added (Phases E3.1–E3.6 — Extractor Framework — 2026-07-13)

- **`Extractor` trait** — bytes-in / `Document`-out; synchronous (no I/O); separates parsing from file access.
- **Five `Extractor` impls** — `TextExtractor`, `MarkdownExtractor`, `HtmlExtractor`, `PdfExtractor`, `DocxExtractor` in `src/extractors/`.
- **`ExtractorRegistry`** — `extractor_for_extension`, `extractor_for_mime`, `extractor_for_path`, `extractor_for_bytes` (magic-byte MIME detection via `infer`), `all_capabilities()`.
- **`DocumentMetadata`** — typed struct replacing `metadata: Value` on `Document`; fields: `title`, `author`, `language`, `created_at`, `modified_at`, `page_count`. All readers updated.
- **`DocumentValidator`** — checks: empty, too-large, page limit, malformed UTF-8, protected PDF. Standalone; not yet wired into pipeline.
- **`DocumentSource`** — typed origin enum: `File`, `Url`, `Memory`, `GitHub { repo, branch, file }`, `S3 { bucket, key }`.
- **`ReaderCapabilities`** — `extensions`, `mime_types`, `supports_streaming/metadata/images`; exposed on every `Extractor` via `capabilities()` and aggregated by `ExtractorRegistry::all_capabilities()`.

### Added (Phase E3.5 — ReaderRegistry — 2026-07-13)

- **`ReaderRegistry`** — `reader_for_extension(ext)` and `reader_for_path(path)` return `Arc<dyn Reader>`; all extension-to-reader mapping lives in one place; unknown extension returns `IngestError::Reader`.

### Added (Phase E3 — Format Readers — 2026-07-13)

- **`MarkdownReader`** — CommonMark → plain text via `pulldown-cmark`; H1 heading promoted to `metadata.title`.
- **`HtmlReader`** — visible-text extraction via `scraper`; `<script>`/`<style>` subtrees pruned; `<title>` and `<meta name="author">` surfaced as metadata.
- **`PdfReader`** — file-path input; text via `pdf-extract`, page count via `lopdf`; runs in `spawn_blocking`.
- **`DocxReader`** — file-path input; unzips, parses `word/document.xml` `<w:t>` runs + `docProps/core.xml` core properties via `quick-xml`; runs in `spawn_blocking`.
- All four readers implement the existing `Reader` trait and return the existing `Document` type — no pipeline changes required.

### Changed (Phase E2.5 — KernelWriter wiring — 2026-07-13)

- `POST /v1/ingest` sync and async paths now delegate to `IngestPipeline::run()` + `KernelWriter`; ~200 lines of inline `embed_batch → insert_batch_ns → nodes/edges/metadata` orchestration removed from the handler.
- `KernelWriter` (in `valori-node`) implements `valori-ingest::Writer` — per-chunk vector insert, reranker index, chunk-node, parent edge, and chunk metadata in one place.
- `provider_from_config()` factory added to `valori-models` — builds `Box<dyn ModelProvider>` from raw env-var strings without the `InstalledModel` registry.
- HTTP API surface unchanged; `ingest_update` path untouched.

### Added (Phase E2 — Composable Ingest Pipeline — 2026-07-13)

- **`Document`** — shared data object (BLAKE3 id, source, mime_type, metadata, content) that flows through every ingest stage.
- **`trait Reader`** + `TextReader` — first stage; converts raw input to `Document`. Format changes are local to this stage.
- **`trait Chunker`** + `ValoriChunker` — wraps existing `chunk_document`; no logic changed.
- **`trait Embedder`** + `ModelProviderEmbedder` — delegates to `Box<dyn ModelProvider>` (from `valori-models`); no Ollama/OpenAI awareness in the stage itself.
- **`trait Writer`** + `NoopWriter` — final stage contract; `KernelWriter` implementation lives in `valori-node` (separate migration).
- **`IngestPipeline`** — `Reader → Chunker → Embedder → Writer`, returns one record ID per chunk. Named `IngestPipeline` to leave room for `QueryPipeline`, `SearchPipeline`, etc.
- `embed.rs` and `handler.rs` unchanged — existing node call sites unaffected.
- **Tightened (E2 exit checklist)**: `ValoriChunker` renamed to `DefaultChunker` (names describe behavior, not brand); stage boundaries use typed objects (`Chunk`, `Embedding`, `WriteResult`) not raw primitives; `IngestError` is an enum with stage variants (`Reader`/`Chunk`/`Embed`/`Writer`); `IngestPipeline::builder()` fluent API replaces positional constructor.
- 19 crate tests (was 13); `valori-node` still builds clean.

### Added (Phase E1.1 — `valori-models` Standalone Crate — 2026-07-13)

- **New `valori-models` crate** — shared model management subsystem used by the daemon, `valori-ingest`, the Python SDK, and the desktop without duplication.
- **`ModelProvider` trait** (`kind`, `model_name`, `dim`, `embed`, `health`) with provider implementations: `OllamaProvider` (batch + legacy fallback), `OpenAIProvider` (OpenAI-compatible), `VoyageProvider`, `DummyProvider` (zero vectors for tests). `build()` factory dispatches by provider string.
- **`ModelStore` DIP seam** — `JsonModelStore` backed by `<home>/models.json` (write-then-rename); `SqliteModelStore` drops in later with no change to `ModelManager`.
- **Built-in registry** of 11 models: OpenAI ×3, Ollama ×3, Voyage ×2, BGE-ONNX ×3.
- **`VerifyStatus`** (`Remote | Ok | Missing | Unverified | Corrupted`) + `verify_model()` for on-demand re-verification.
- Fixed workspace `Cargo.toml` — added 7 missing `[workspace.dependencies]` entries for crates added in prior N/E sessions.
- 5 crate tests: SHA-256 known-vector, dummy provider, storage CRUD, verifier (remote + local).
- **E1.2**: Deleted `valori-daemon/src/model.rs` (351 lines, fully duplicated); daemon now imports from `valori-models`. Removed `ModelStore` trait from `daemon/store.rs`. Added `From<ModelError> for DaemonError` bridge. Removed `sha2`/`futures-util` from daemon deps. Single source of truth established; 11 daemon unit tests pass.

### Added (Phase E1 lite — Model Manager — 2026-07-13)

- **Daemon model catalog**: `GET /v1/models` (installed + available from a curated registry + total disk usage), `POST /v1/models/install` `{id}`, `GET|DELETE /v1/models/*id`. Replaces the previous `501` stubs.
- **Two install paths**: remote-service models (OpenAI/Ollama/…) install by registering; local models (ONNX/…) stream-download to `<home>/models/<id>/` with **SHA-256 verification** (mismatch → delete + error) and disk accounting.
- **`ModelStore` DIP seam** (impl `JsonModelStore`, `<home>/models.json`) alongside the project/workspace stores — a `SqliteModelStore` drops in later with no daemon change. `DaemonDeps` now injects the model store too.
- Management only — the daemon orchestrates models; local inference is a future `ModelProvider` (E1-full). Each model's `provider` is the seam the document-pipeline embedder (E2) will dispatch on. New event: `model.installed` / `model.removed`.

### Added (Phase D2.2 — Restart Loop & Health FSM — 2026-07-13)

- **Self-healing supervision**: a background monitor detects crashed nodes and restarts them per an operator-set `RestartPolicy` (`never` (default) / `on_failure` / `always`) with capped exponential backoff (2→60s). Crash count and last crash reason are tracked and surfaced under `supervision` in project responses; `restart_policy` is settable on project create and persisted in the manifest.
- **Operational/runtime split** (review point 3): the `Runtime` detects exits (`poll_exits`, via a new non-blocking `RunningProcess::has_exited`) and executes start/stop; a separate operational `Supervisor` decides *whether* to restart (policy + backoff) and owns crash bookkeeping. The daemon's monitor tick wires them.
- **Richer `RuntimeState`**: adds `Recovering` (auto-restart after a crash — distinct from a fresh `Starting`, since Valori replays its event log on recovery), with the corresponding legal transitions.
- Lifecycle events now include `project.crashed`, `project.recovering`, `project.restarted`.

### Changed (Phase D2.1 — Dependency-Inversion Seams — 2026-07-13)

- **The daemon now runs entirely on injected trait objects** (Dependency Inversion). `Daemon` holds `Box<dyn ProjectStore>`, `Box<dyn WorkspaceStore>`, `Box<dyn Runtime>`, `Box<dyn EventStore>` and constructs nothing durable itself — a `DaemonDeps` struct + `with_deps()` inject everything; `new()` wires the defaults. Swapping to a SQLite store or Docker runtime needs no daemon change.
- **New seams**: `ProjectStore`/`WorkspaceStore` (impl `JsonProjectStore`/`JsonWorkspaceStore`), `EventStore` (impl `MemoryEventStore`), and `Launcher` + `RunningProcess` (impl `LocalLauncher`/`LocalProcess`). The `Runtime` now *orchestrates* (health, state, resources) while the `Launcher` *launches* — so a future `DockerLauncher` returns a container handle without the runtime touching `std::process`.
- **`RuntimeState` state machine**: node lifecycle is now `Stopped → Starting → Running → Stopping → Stopped` (plus `Failed`) with illegal transitions returning an error instead of corrupting state; `NodeInfo.status` is the typed state, not a bespoke enum.
- **`RestartPolicy` moved out of `runtime/`** to a top-level operational module — whether a node *should* exist is an operator decision, not the runtime's.

### Added (Phase D2 — Node Runtime — 2026-07-13)

- **`Runtime` trait + `LocalRuntime`**: the daemon now runs nodes through a pluggable `Box<dyn Runtime>` (async-trait) instead of a hard-coded supervisor, so `DockerRuntime` / `SshRuntime` / `RemoteRuntime` slot in later with no change to the daemon, API, or desktop. The monolithic `Supervisor` was decomposed (SRP) into focused components: `PortAllocator`, `ResourceMonitor`, `RestartPolicy`, plus health polling and log capture in `LocalRuntime`.
- **`GET /v1/events`**: Docker-style lifecycle event stream (in-memory ring buffer) — `project.created`, `project.started`, `project.stopped`, `workspace.created/deleted`. Poll today; SSE/WebSocket push later (same shape).
- **`GET /v1/projects/:name/runtime`**: live per-node resource stats (CPU %, resident MB, threads on Linux, uptime) sampled via `ps` — no platform crate.
- **Stable resource IDs**: projects and workspaces now carry a UUID `id` (names become mutable labels). `GET /v1/config` reports the runtime descriptor (`kind: "local"`, binary, port range).

### Added (Phase D1.1 — Stabilize the Daemon API — 2026-07-13)

- **System / discovery endpoints**: `GET /v1/system` (version, platform, daemon PID, uptime, and live counts of projects/running/workspaces/models — the endpoint every client calls first), `GET /version`, `GET /v1/config`. Whole API is versioned under `/v1` from day one.
- **Workspaces** — the grouping layer above projects (RFC-0006): `GET|POST /v1/workspaces`, `PATCH|DELETE /v1/workspaces/:name`. A `default` workspace always exists; deleting a workspace that still has projects is refused. Projects carry a `workspace` field (serde-defaulted, so older manifests still load).
- **Collections** proxied through the running node: `GET|POST /v1/projects/:name/collections`, `DELETE …/:collection` → the node's `/v1/namespaces`.
- **Node logs + uptime**: node stdout/stderr captured to `<project>/node.log`, exposed via `GET /v1/projects/:name/logs?tail=N`; node status now includes `uptime_secs`.
- **Model manager stubs** (D4 placeholder): `GET /v1/models` (empty), `POST /v1/models/install` and `DELETE /v1/models/:id` return `501`.

### Added (Phase D1 — Valori Daemon, Milestone 1 — 2026-07-13)

- **New crate `valori-daemon`** + `valori-daemon` binary: the control-plane daemon that owns project lifecycle and supervises `valori-node` instances (RFC-0006 "Docker Desktop for AI Memory"). Rust successor to the TypeScript process manager in `ui/src/lib/server/`.
- **Project lifecycle HTTP API** (Milestone 1): `GET /health`, `GET|POST /v1/projects`, `GET|DELETE /v1/projects/:name`, `POST /v1/projects/:name/{start,stop,restart}`. Projects are directories under `$VALORI_HOME/projects/<name>/` with a `project.json` manifest; one project → one supervised `valori-node`.
- **Process supervision**: internal port allocation (8100–8999, hidden from clients — projects are addressed by name), `/health`-gated startup, best-effort graceful stop (snapshot then terminate; hard kill is still safe via event-log replay), and "no delete while running" enforcement.
- **New crate `valori-daemon`** added to the workspace (members + default-members); the Tauri desktop shell (`desktop/`) is deliberately excluded from the Cargo workspace.
- **`desktop/`** — Tauri 2 scaffold (native control-plane shell), separate from `ui/` so `cd ui && npm run dev` is unaffected. Runs in dev against the Next.js UI; production bundling waits on the daemon absorbing `ui/`'s server API routes.
- **RFC-0006** (`rfcs/0006-desktop-daemon-architecture.md`): daemon architecture — three execution modes (embedded/supervised/remote), path-as-truth + project-scoped-token-as-sugar, workspace layer, collections-are-namespaces scaling model.
- **`_execution` observability block** extended with `operation_hash` + measured `duration_ms` (opt-in via `?explain=true` on `POST /v1/memory/search_vector`).

### Added (Phase N5 — valori-engine extraction — 2026-07-12)

- **New crate `valori-engine`**: the `Engine` struct (1 743-line engine.rs) and all supporting types extracted from `valori-node` into a standalone orchestration crate. Five modules: `config` (`IndexKind`, `QuantizationKind`, `EngineConfig`), `error` (`EngineError`, `CommitError`), `metadata` (`MetadataStore`), `persistence` (`Persistence` enum — Phase E1 durability funnel), `engine` (`Engine::with_config`, `RecoveryMode`, `EngineHealth`, `PoolStats`, `ExecutionResources`).
- **`EngineFromNodeConfig` extension trait**: defined in `valori-node/src/engine.rs`, bridges `NodeConfig → EngineConfig` so all existing `Engine::new(&cfg)` call sites in tests, main.rs, and examples keep compiling with one added `use valori_node::EngineFromNodeConfig;` per file.
- **Dependency Inversion**: `EngineConfig` injects `Arc<dyn KeyVault>` and `Option<Arc<ObjectStoreBackend>>`; `valori-engine` never constructs `AesGcmVault` or calls `ObjectStoreBackend::from_env()` — those remain in `valori-node`.
- **Re-export shims**: `valori-node/src/errors.rs`, `metadata.rs`, `commit/persistence.rs`, and `config.rs` (for `IndexKind`/`QuantizationKind`) now delegate to `valori-engine` via `pub use`, keeping all existing `crate::*` imports across server.rs, cluster_server.rs, routes/, etc. unchanged.

### Added (Phase N4 — valori-ingest extraction — 2026-07-12)

- **New crate `valori-ingest`**: embedding client and chunking logic extracted from `valori-node/src/embedder.rs` and `ingest.rs` into a standalone crate with zero `valori-*` dependencies. Three modules: `embed` (`EmbedConfig`, `embed_batch` supporting Ollama/OpenAI/custom), `chunker` (`chunk_document`, `chunk_content_hash`, 4 strategies + auto-detection, `MAX_INGEST_TEXT_BYTES`), `handler` (`ingest_document` stateless axum handler for `POST /v1/ingest/document`).
- **`embed_config_from_node` helper**: added as `pub(crate)` in `engine.rs` — constructs `valori_ingest::EmbedConfig` from `NodeConfig` without requiring `valori-ingest` to depend on `valori-node`.
- **Recursion bugfix in chunker**: tree strategy falling back to `"auto"` could infinite-recurse (auto re-detects tree → loop → SIGABRT). Fixed by falling back directly to `"fixed"` instead.

### Added (Phase N3 — valori-rag extraction — 2026-07-12)

- **New crate `valori-rag`**: GraphRAG, Tree-RAG, and Community Layer extracted from `valori-node` into a standalone crate. Three modules: `graph` (`resolve_seed_nodes`, `expand_subgraph`), `tree` (`TreeIndex`, `Receipt`, `verify_chain`, stateless axum handlers), `community` (Label Propagation, centroid ranking, request/response types). New `llm` module holds `LlmConfig` + `extract_entities_via_llm`, decoupled from `EmbedConfig` via a 4-field config struct.
- **`LlmConfig`**: minimal credentials struct in `valori_rag::llm` that breaks the circular dependency between entity extraction and `valori-node`'s `EmbedConfig`. Node constructs `LlmConfig` at the call site; `valori-rag` has no `valori-node` dependency.

### Added (Phase N2 — valori-index extraction — 2026-07-12)

- **New crate `valori-index`**: all vector index structures extracted from `valori-node/src/structure/` into a standalone crate behind a single `VectorIndex` trait. Includes `BruteForceIndex`, `HnswIndex`, `IvfIndex`, `BqIndex`, quantizers (`NoQuantizer`, `ScalarQuantizer`, `ProductQuantizer`), and `deterministic_kmeans`. NEON SIMD kernels and determinism guarantees preserved.
- **`VectorIndex` trait is now a public crate interface**: integration test files in `valori-node/tests/` and `engine.rs` import from `valori_index::` directly; the old `crate::structure::*` internal module is deleted.

### Added (Phase N1 — valori-search extraction — 2026-07-12)

- **New crate `valori-search`**: post-retrieval search primitives extracted from `valori-node` into a standalone crate with no kernel or node dependency. Three modules: `decay` (time-decay re-ranking), `reranker` (BM25 hybrid), `filter` (metadata predicate matching).
- **O(1) IDF lookup in `ValoriReranker`**: added `doc_freq: HashMap<String, usize>` inverted index updated incrementally on every `insert`/`remove`. Previous implementation scanned the full corpus per query term — O(|corpus| × |query_terms|).
- **`restore_corpus` is now deterministic**: rebuilds `doc_freq` from the restored corpus instead of trusting the snapshotted `total_tokens` value, which could be stale after tokeniser changes.


### Fixed (Phase A14 — valori-node audit bug fixes — 2026-07-10)

- **P0 — `RaftKernelCapability::state_hash()` always returned zeros**: Now uses `tokio::task::block_in_place` to call the async `ValoriStateMachine::with_state()` from a sync trait method, computing the real BLAKE3 hash per shard.
- **P0 — `cluster_snapshot_save` only saved shard 0 and read wrong field**: Handler now loops all shards `0..shard_count` and reads `"state_hash"` (not `"hash"`) from `SnapshotArtifactTask` output.
- **P0 — `/health` and `/metrics` gated behind `cluster_auth_guard`**: Middleware restructured so the public sub-router (health, metrics) is merged without auth; only the v1 protected sub-router gets the auth layer.
- **P1 — Namespace truncation in standalone shard routing**: `(ns as u8).wrapping_rem(shard_count)` silently truncated 16-bit namespace IDs before modulo, misrouting namespaces ≥ 256. Fixed to `((ns as u32) % (shard_count as u32).max(1)) as u8` at all 3 callsites in `server.rs`.
- **P1 — `cluster_community_search` hardcoded shard 0**: Handler now resolves `payload.namespace` via `s.sm.resolve_namespace()` and routes to the correct shard via `shard_for_namespace()`, matching `cluster_community_detect` behavior.
- **P1 — `cluster_community_detect` swallowed planner errors with `.ok()`**: Return type changed to `Result<Json<DetectResponse>, (StatusCode, Json<Value>)>`; planner errors now surface as 500 INTERNAL_SERVER_ERROR.
- **P1 — Decay sort inverted in `RaftKernelCapability::memory_search`**: `score * decay_factor` ascending ranked older records better; fixed to `score / decay_factor` ascending, matching the standalone `decay.rs::rerank` formula.

### Added (Phase A13.1 — cluster planner wiring — 2026-07-10)

- **`RaftKernelCapability` extended** with 8 new methods: `save_snapshot`, `graph_rag`, `memory_search`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid` — backed by `ValoriStateMachine` (`with_state()` / `with_state_and_timestamps()` / `get_meta_json()`).
- **`CapabilityRegistryBuilder::build_cluster()`** now takes `tree_cache` and `community_store` to pass shared state into the cluster capability.
- **7 cluster handlers wired through `run_graph_inline`**: `cluster_graphrag`, `cluster_snapshot_save`, `cluster_tree_build`, `cluster_tree_query`, `cluster_tree_hybrid`, `cluster_community_detect`, `cluster_community_search` in `cluster_server.rs`. Both execution paths now follow the identical `HTTP → ExecutionGraph → TaskRunner → KernelCapability → Response` contract.

### Added (Phase A13 — planner migration — 2026-07-10)

- **8 new `KernelCapability` default methods** in `valori-effect`: `save_snapshot`, `graph_rag`, `memory_search`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid` — all default to `CapabilityUnavailable`.
- **`EngineKernelCapability` overrides** for all 8 methods in `valori-node/src/capabilities.rs`: each delegates to the live engine subsystem (search, community, tree-RAG, snapshot).
- **5 new Task files** under `valori-effect/src/tasks/`: `snapshot.rs`, `graph_rag.rs`, `memory_search.rs`, `community.rs`, `tree_rag.rs` — 8 concrete `Task` implementations.
- **6 new `TaskKind` variants**: `MemorySearch`, `CommunityDetect`, `CommunitySearch`, `TreeBuild`, `TreeQuery`, `TreeHybrid` in `valori-planner`.
- **Standalone path wired**: `snapshot_save`, `graphrag`, `memory_search_vector`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid` in `server.rs` all dispatch through `run_graph_inline`. No behavior change — same outputs, same HTTP contract.
- **`Deserialize` added** to `HybridHit`, `HybridResponse` (tree_rag.rs), `CommunitySummary`, `DetectResponse`, `CommunityHit`, `SearchResponse` (community.rs), `MemorySearchHit`, `MemorySearchResponse` (api.rs) — needed for task output round-trip.

### Removed (valori-storage/state dead API pass — 2026-07-10)

- **`EventProof` struct and `generate_proof()`** deleted from `valori-storage::events::event_proof`. Both were superseded by `valori-verify` which owns the full audit path. `compute_event_log_hash()` (the only production caller, used by `/v1/proof/event-log`) is kept.
- **`read_event_log()`** deleted from `valori-storage::events::event_replay`. It dropped namespace information silently and was strictly weaker than `read_all_segments()`. Two `cluster_boot.rs` tests migrated to `read_all_segments()`.
- **`StateManifest`**, **`StateLifecycle`**, **`shutdown_snapshot()`** deleted from `valori-state`. None had any external callers; all were speculative scaffolding from an orchestration layer that was never built.
- **`bootstrap::{has_wal, has_event_log, load_snapshot, validate_snapshot, replay_wal}`** changed `pub` → `pub(crate)`. Retained as internal helpers for future bootstrap orchestration; removed from the public surface.

### Added (persistence contract corpus — 2026-07-10)

- **Snapshot compatibility corpus** (`valori-kernel/tests/snapshot_compat.rs`) — committed V7 binary fixtures (`snapshot_v7_empty.bin`, `snapshot_v7_single.bin`, `snapshot_v7_multi.bin`) paired with pinned state hashes. Four forever-decode tests lock the snapshot encoder, decoder, and `hash_state_blake3` contract against accidental format drift. A fifth test (`snapshot_v7_multi_can_continue_after_restore`) verifies that restored state produces the same hash as replay-from-scratch after a subsequent event.
- **WAL compatibility corpus** (`valori-storage/tests/wal_compat.rs`) — committed `wal_v1_inserts.wal` and `wal_v1_namespace.wal` fixtures with pinned `.hash` files. Two forever-replay tests lock `WalWriter` → `WalReader` → `apply_event_ns` → `hash_state_blake3` against format regressions.
- **Event-log end-to-end corpus** (`valori-state/tests/event_log_compat.rs`) — committed event log fixtures with TOML manifests pinning four independent invariants: `event_count`, `record_count`, `chain_head`, `state_hash`. Tests exercise both `recover_from_event_log` (bootstrap path) and `valori_verify::verify_log_file` (audit path). Three malformed-artifact tests (`bad_magic`, `truncated`, `chain_tampered`) assert that corrupted input is detected and handled, not panicked on.

### Refactored (valori-storage / valori-state cleanup — 2026-07-10)

- **`valori-storage::recovery` deleted** — the module was a dead duplicate of `valori-state::bootstrap` left behind when recovery orchestration was migrated in Phase A3. Zero external callers; `valori-node` and `valori-state` already routed through `valori_state::bootstrap`. `StorageError` preserved in a new `error.rs` module so the public path `valori_storage::StorageError` is unchanged and no callers required updating.
- **Crate responsibilities clarified** — `valori-storage` = persistence primitives (WAL, event log, object store); `valori-state` = recovery orchestration. Docs in `lib.rs`, `CLAUDE.md`, and `AGENTS.md` updated to match.
- **Dependency graph confirmed acyclic**: `valori-core → valori-kernel → valori-wire → valori-storage → valori-state → (valori-consensus, valori-node)` with no back edges.

### Fixed (valori-consensus cleanup — 2026-07-10)

- **`ShardId` deduplicated** — valori-consensus now re-exports the shared valori-core type (via valori-kernel) instead of defining a structurally identical local duplicate; wire encoding unchanged. Stale "namespace routing does not exist yet" doc replaced with shipped S3–S9 behavior.
- **Snapshot IDs derived, not counted** — `snapshot_id` is now `(last_applied index, state-hash prefix)`; the old in-memory counter reset on restart and could reissue a previous ID.
- **Dead `thiserror 1.x` dependency removed**; stale V5→V6 snapshot docs and the `created_at` "replicas agree" claim corrected; `serve_raft_single`/`serve_raft_tls_single` marked as test helpers; obsolete `placeholder.rs` deleted.

### Fixed (valori-wire audit — 2026-07-10)

- **Phantom hardening guards now real** — `METADATA_CAP` enforced at `encode_entry` (write-side; pre-cap logs stay readable), `MAX_ENTRIES_PER_SEGMENT` enforced in the valori-verify replay loop, `MAX_ENTRY_DECODE_BYTES` unified with the applied decode limit. `MAX_SEGMENT_DECOMPRESSED_BYTES` remains reserved for upcoming zstd support and its doc now says so honestly.
- **V4 evolution fixture added** — `segment_v4.bin` + forever-decode test; the current production write format previously had no CI fixture. `make_demo_log` now emits V4 per policy rule 4.
- **Wire cleanups** — stale "understands v2 and v3" error message fixed; `parse_header` V3/V4 arms collapsed; bincode limit errors matched by enum variant instead of display-string substring; `thiserror` 1.0→2.0; `encode_header_v3` marked legacy/fixture-only.

### Fixed (valori-core audit — 2026-07-10)

- **`ExecutionId::new_random()` collision bug (release blocker)** — the old time+stack-address scheme produced ~93% duplicate IDs under sequential calls (937,202 dups measured in 1M); planner operation IDs and async-ingest `job_id`s could collide across clients. Now uses OS RNG via `getrandom` (std-gated). Regression tests: 100k sequential, 80k cross-thread, and a `#[ignore]`d 1M stress test.
- **`ExecutionId: FromStr` added** — parses the 32-hex-digit `Display` form, so `job_<id>` strings round-trip.
- **valori-core dead API trimmed** — `CoreError` reduced to `InvalidInput`; unused `Version::is_compatible_with` removed (its exact-match policy contradicted the actual V5→V6 snapshot compatibility); `Version::next`/`ClusterEpoch::next` use `checked_add` for consistent overflow behavior; docs corrected from "zero-dependency" to "minimal-dependency".

### Internal (Command removal + ValoriKernel deletion — 2026-07-10)

- **`Command` enum deleted from `valori-kernel`** — no kernel code creates or processes `Command` anymore. `state/command.rs` is gone.
- **WAL format upgraded to v2** — `WalWriter` now writes `(KernelEvent, namespace_id)` bincode pairs (header version=2). `WalReader` handles both v1 (Command, backward compat) and v2 transparently; callers always receive `(KernelEvent, u16)`.
- **`LegacyWalCommand`** lives in `valori-storage/src/wal_compat.rs` (private to storage) as the only remaining Command-shaped type — used exclusively for reading pre-K2 WAL files.
- **`ValoriKernel` struct deleted** — the legacy HNSW prototype (`kernel.rs`) and its CRC64 `state_hash()` / binary-payload `apply_event(&[u8])` are gone. `crc64fast` dependency removed from `valori-kernel/Cargo.toml`.
- **Bench bins deleted** — `bench_filter`, `bench_ingest`, `bench_recall` all depended on `ValoriKernel`; removed from `valori-cli`. `bench_1m` and `bench_persistence` (which already used the production path) are retained.
- **`command_for()` deleted from `persistence.rs`** — `Persistence::Wal` arm now calls `w.append_event(event, namespace_id)` directly, no translation layer.

### Internal (coverage tests — 2026-07-10)

- **43 new tests for zero-coverage kernel modules** — `tests/fxp.rs` (22 tests: `fxp_add/sub/mul`, `from_f32`/`to_f32` with saturation, NaN, infinity), `tests/proof.rs` (12 tests: `merkle_root` empty/single/even/odd/order-sensitive, `generate_proof_bytes`, `DeterministicProof` bincode roundtrip), inline tests in `verify.rs` (5: `snapshot_hash`/`wal_hash` against `blake3::hash` directly), inline tests in `adapters/ivecs.rs` (4: single row, multi-row, empty file, zero-dim row). Total kernel tests: **134**.
- **Dead binary-protocol types deleted** — `InsertPayload`, `DeletePayload`, `CMD_INSERT`, `CMD_DELETE`, `FixedPointVector` removed from `types/mod.rs`; exclusively used by the deleted `ValoriKernel::apply_event(&[u8])`.

### Internal (coverage audit — 2026-07-10)

- **`cargo-tarpaulin 0.37.0` installed** — baseline coverage established for `valori-kernel`: **36.24%** (963/2657 lines). Zero-coverage modules ranked by risk: `hnsw.rs` (265L, untested), `proof.rs` (24L), `fxp/ops.rs` (21L), `types/mod.rs` (48L), `verify.rs` (4L), `adapters/ivecs.rs` (11L). Full audit in `docs/phases/phase-K3-coverage-audit.md`.

### Internal (replay unification — 2026-07-10)

- **`KernelEvent` → `apply_event_ns` is now the single authoritative mutation path** — eliminated the `Command` intermediate type from the kernel's internal apply loop. `apply_event_ns` directly contains the logic for every mutation; there is no translation layer.
- **`replay.rs` deleted** — `replay_and_hash` (legacy bincode-Command WAL replay) had zero external callers. `WalHeader` moved to `valori-storage/src/wal_reader.rs` where it belongs.
- **Version-bump omission fixed** — `UpdateRecordMetadata`, `SetMeta`, `InsertRecordEncrypted`, and `ShredKey` previously did not bump `KernelState::version` when applied via `apply_event_ns` directly (the cluster path). Fixed by a single version bump at the end of `apply_event_ns`.
- **`apply_raw_for_test` → `apply_event_for_test`** — engine test helper now takes `&KernelEvent` instead of `&Command`.
- **WAL recovery updated** — `valori-storage::recovery::replay_wal` and `valori-state::bootstrap::replay_wal` both translate legacy `Command` entries to `KernelEvent` before applying, keeping backward-compatible WAL recovery on the canonical path.

### Performance (HNSW wired into namespace search — 2026-07-08)

- **HNSW/IVF/BQ now applies to all named collections** — `Engine::search_l2_ns` previously always called the kernel's brute-force linked-list walk regardless of `VALORI_INDEX`. It now routes through the `VectorIndex` (HNSW, IVF, or BQ) when a non-brute index is active, with namespace post-filtering on the candidates. Measured speedup: 9× at N=1k, 43× at N=10k, 183× at N=50k (in-process, dim=384, k=10).
- **HNSW sort-order bug fixed** (`hnsw.rs`) — `BinaryHeap::into_sorted_vec()` on a MaxHeap returns descending (worst-first). Without `.reverse()`, `select_neighbors` was connecting every node to its M *farthest* neighbors, producing an inverted graph and O(N) traversal.
- **over_fetch reduced from k×20 to k** (`engine.rs`) — the previous `(k * 20).max(200)` multiplier forced ef=200 in HNSW, expanding the beam search to O(N) candidates. Using `k` directly lets ef fall to ef_search (default 50), keeping search sub-millisecond.
- **All records enter the global index** — inserts and `build_index` previously skipped non-default-namespace records. All namespaces now feed `self.index`, enabling the HNSW path above.
- **`drop_collection` cleans the global index** — records in a dropped namespace are now explicitly removed from `self.index`, preventing stale HNSW entries from polluting future searches.
- **`search_l2` delegates to `search_l2_ns(DEFAULT_NS)`** — removes code duplication and ensures the default-collection path also benefits from HNSW automatically.

### Performance (kernel SIMD + algorithmic fixes — 2026-07-08)

- **HNSW uses SIMD distance** — `hnsw.rs` was importing `dist::euclidean_distance_squared` (scalar, `saturating_mul`); now calls `math::l2::l2_sq_i32` which dispatches to NEON (aarch64), AVX2 or SSE4.1 (x86_64). All candidate comparisons in insert and search now run at 4–8× lane width.
- **`fxp_dot` SIMD implementation** — `math/dot.rs` added NEON (`vmull_s32` widening), AVX2, and SSE4.1 paths mirroring `math/l2.rs`. Cosine similarity (contradict, consolidate, memory search) now runs at SIMD speed.
- **HNSW `determine_level` fix** — was hashing the full 384-dim vector (1536 bytes) for deterministic level assignment; now hashes only the 8-byte record ID (~48× less data per insert).
- **Brute-force top-K: insertion sort → max-heap** — `BruteForceIndex::search` replaced O(k) insertion sort with `BinaryHeap` O(log k) per candidate. At k=100 this is ~7× fewer comparisons per candidate.
- **`dist.rs` deleted** — dead scalar-only distance file (`euclidean_distance_squared`, `dot_product`, `euclidean_distance_fxp`) removed. All call sites redirected to `math::l2` / `math::dot`. Prevents future regression to scalar paths.
- **HNSW startup allocation eliminated** — `Vec::with_capacity(1_000_000 × dim)` replaced with `Vec::new()`. Removes up to 1.5 GB of committed virtual memory at startup for dim=384.
- **HNSW `id_map`: `HashMap` → `FxHashMap`** — uses identity-like hashing for integer keys; ~5–15% insert throughput improvement.
- **`dist::dot_product` callers migrated** — `engine.rs` and `cluster_server.rs` cosine-similarity helpers now call `math::dot::dot_i32` (SIMD) instead of the deleted scalar function.

### Internal (engine decomposition — not user-facing)

- **ExecutionResources (E4)** — `tree_cache` and `community_store` extracted
  from `Engine` into `pub resources: ExecutionResources`; application-layer
  boundary is now explicit in the type.
- **Hide pub state (E3)** — `Engine.state` changed to `pub(crate)`; 10 public
  read accessor methods added. Stale pre-E1 dual-branch patterns in valori-ffi
  removed. FFI `create_node` now routes through `create_node_for_record`.
- **NamespaceRegistry → CollectionRegistry (E2)** — duplicate `NamespaceRegistry`
  struct deleted from engine.rs; `valori-metadata::CollectionRegistry` is the
  single implementation. `list()` added to `CollectionRegistry`.
- **Single persistence funnel (E1)** — `Engine` now owns one `Persistence` enum
  (`EventLog` / `Wal` / `Ephemeral`); every mutation flows through one
  `commit_and_apply_ns` path. Behavior fix: event-log batch inserts now run the
  auto-tier index check (previously WAL-only).
- **Dead storage-layer duplicates removed (E0)** — 10 stale files in
  `valori-node/src/` deleted; `tests/architecture.rs` tripwire added to prevent
  re-introduction.

### Added
- **Dual-path unification, all mechanical domains (Phase R2)** — graph
  (7 endpoints), record deletion, metadata sidecar, and version handlers now
  share one body in `valori-node/src/routes/` served by both routers. Two new
  endpoints fell out of the unification: `POST /v1/soft-delete` on standalone
  (the engine always supported it; the route was missing) and
  `DELETE /v1/graph/node/:id` on cluster (commits `KernelEvent::DeleteNode`
  via Raft). The parity test's METHOD_GAPS list is now empty. Also fixed by
  construction: cluster `GET /v1/graph/nodes` no longer lists every
  namespace's nodes when `collection` is absent (tenant-isolation leak — now
  scopes to "default" like standalone); invalid node/edge kinds are 400 on
  both paths (standalone silently coerced them before); cluster `meta/set`
  answers `{"success":true}` (was `{"ok":true}`); unknown collections are 404
  on graph/delete endpoints on both paths. See
  `docs/phases/phase-R2-dual-path-domains.md`.
- **Dual-path unification (Phase R1)** — new `valori-node/src/routes/` module:
  shared HTTP handler bodies served by BOTH the standalone and cluster routers,
  starting with the collection endpoints (`/v1/namespaces*`). A new
  `tests/route_parity.rs` guard asserts the two routers expose identical `/v1`
  route sets (paths and methods) modulo explicit, documented allowlists — an
  endpoint added to only one router is now a test failure instead of a silent
  404. See `docs/phases/phase-R1-dual-path-unification.md`.

### Changed
- **`DELETE /v1/namespaces/:name` on an unknown collection now returns 404 on
  both paths** (standalone previously returned 400 while cluster returned 404).
- **Cluster `POST /v1/namespaces` now enforces the same name validation as
  standalone** (non-empty, ≤64 chars, `[a-zA-Z0-9_-]` only) — previously the
  cluster path committed unvalidated names straight through Raft.

- **Snapshot autosave + cluster lifecycle hardening (Phase 6.2)** — UI-launched project
  nodes now pass `VALORI_SNAPSHOT_INTERVAL=60` so a periodic snapshot is written even if
  the node is killed without a graceful close (the WAL was always durable; this keeps the
  next open instant and survives WAL-file loss). The deprecation warning on
  `VALORI_SNAPSHOT_INTERVAL` was removed — the replacement knobs
  (`VALORI_SNAPSHOT_EVERY_EVENTS/BYTES`) were parsed but never implemented, so the
  interval knob is the supported cadence control. Cluster mode gained a graceful-shutdown
  handler (SIGTERM/Ctrl-C drains axum and lets redb close cleanly). The UI close route
  now records the final record count in the manifest so at-rest project cards stay accurate.
  Verified end-to-end: standalone and 3-node cluster projects survive
  create → insert → close → reopen with records, collections, and search intact.
  See `docs/phases/phase-6.2-snapshot-autosave.md`.

### Fixed
- **Cluster search returned raw Q16.16 fixed-point scores** — `/search` on the cluster
  path serialized `score` as the raw `i64` kernel distance (e.g. `42954916`) instead of
  the float conversion the standalone path applies (`0.0100…`). The cluster `SearchHit`
  now divides by SCALE², matching standalone byte-for-byte across the plain, reranked,
  and decay-ranked paths. One SDK client now sees identical score scales on both.
- **Effect bus wiring for `POST /v1/records` (Phase A12)** — the standalone insert handler
  now routes through `EffectBus → EngineKernelCapability → Engine` via `run_graph_inline`,
  making the effect/planner pipeline live for the first time. `CapabilityRegistry` and
  `TaskRegistry` are built at router startup and injected as axum Extensions.
  `EffectError::Capacity` added so HTTP 507 (pool full) is still propagated correctly.
  `capabilities.rs` updated to the final `apply_command(body: &KernelCommandBody) → serde_json::Value`
  signature across all three kernel capability impls (`EngineKernelCapability`,
  `RaftKernelCapability`, `NoRaftKernelCapability`).
  See `docs/phases/phase-A12-effect-bus-wiring.md`.
- **Cross-shard timeline ordering validation (Phase S19)** — `GET /v1/timeline` on a
  multi-shard cluster now tags every event with `shard_id`, merges all shards' logs
  with a deterministic composite sort key `(timestamp_unix, shard_id, log_index)`,
  and actively rejects any shard log whose `log_index` sequence is non-monotonic in
  the merged output (HTTP 500 with a descriptive error). Standalone path unchanged
  (single shard, `shard_id: 0`). Covered by 1 new integration test.
  See `docs/phases/phase-S19-cross-shard-ordering.md`.
- **V4 event-log format with per-entry CRC32 (Phase S18)** — closes the silent
  corruption window where a bit-flipped entry decoded as valid bincode and was applied
  silently. New `VERSION_V4` segment: `encode_entry` appends a 4-byte LE CRC32 of the
  bincode payload; `decode_entry` rejects on mismatch with a descriptive error before
  the entry reaches the kernel. Chain hash is unchanged (CRC is transport-only, not
  part of the BLAKE3 chain formula). V2/V3 segments decode unchanged. 6 new hardening
  tests cover clean roundtrip, payload bit-flip, CRC tamper, and truncation.
  See `docs/phases/phase-S18-v4-per-entry-crc32.md`.
- **CRTS/BCRP snapshot roundtrip tests (Phase S17)** — 5 new tests in
  `engine_snapshot_roundtrip.rs` covering: decay timestamps (`created_at`) survive
  snapshot/restore; BM25 reranker corpus survives; both sections forward-compatible with
  snapshots that predate them (silent skip, no panic). Added `Engine::reranker_corpus_len()`,
  `Engine::reranker_rerank()`, `ValoriReranker::corpus_len()`.
  See `docs/phases/phase-S17-crts-bcrp-snapshot-tests.md`.
- **Multi-shard audit surface (Phase S16)** — `/v1/proof/event-log` now returns
  BLAKE3 hashes for every shard under `shards: { "0": {...}, "1": {...} }` (top-level
  `event_log_hash` is shard 0 for backward compat); `/v1/timeline` reads and merges
  all shards' audit logs sorted by wall-clock time; root cause fixed:
  `DataPlaneState.event_log_path` (shard 0 only) replaced with
  `shard_event_log_paths: BTreeMap<ShardId, PathBuf>`.
  See `docs/phases/phase-S16-multi-shard-audit-surface.md`.
- **Real `OperationHash` + extended write coverage (Phase A11)** — receipt bridge now
  uses the canonical RFC-0003 `OperationHash = BLAKE3(kind_discriminant ‖ bincode(inputs) ‖
  bincode(policy))` — reproducible from planning parameters, no timestamps involved.
  New `OperationKind`/`OperationInputs` variants: `Delete` and `BatchInsert`.
  Receipt emission extended to `batch_insert`, `delete_record`, and `soft_delete_record`
  on both standalone and cluster paths; cluster `delete_record` and `soft_delete_record`
  switched to `raft_write_data` to capture `log_index` as `committed_height`.
  See `docs/phases/phase-A11-real-op-hash.md`.
- **Receipt bridge wired into live handlers (Phase A10)** — `GET /v1/proof/receipt` now
  returns real per-operation receipts from actual HTTP traffic:
  - New `receipt_bridge.rs` — `emit_write()` (mutating ops) and `emit_read()` (read-only
    ops); each assembles a `Receipt` via `ReceiptAssembler` and pushes it into `ReceiptStore`.
  - Standalone `insert_record` — captures `state_before`/`state_after` via `hash_state_blake3`
    while holding the write lock; emits receipt after every successful insert.
  - Standalone `search` — captures current state hash at entry; emits read receipt on both
    no-decay and decay exit paths.
  - Cluster `insert_record` — gets `state_before` from `sm.state_hash().await`; switches to
    `raft_write_data` to read `resp.state_hash` + `resp.log_index` from the committed
    `ClientResponse`; emits receipt with real Raft log index as `committed_height`.
  - Cluster `search` — emits read receipt with shard state hash after results are computed.
  See `docs/phases/phase-A10-receipt-bridge.md`.
- **`RaftKernelCapability` in `valori-node` (Phase A9)** — real cluster `KernelCapability`
  backed by `raft.client_write()`. `apply_command()` deserializes `event_json → KernelEvent`,
  wraps in `ClientRequest { CURRENT_SCHEMA_VERSION, namespace_id, event, request_id }`, submits
  via Raft, and returns the post-apply `state_hash` hex from `ValoriStateMachine::state_hash()`.
  `NoRaftKernelCapability` renamed to a test-only stub (`is_available = false`).
  See `docs/phases/phase-A9-node-cleanup.md`.
- **`ReceiptAssembler` + `/v1/proof/receipt` (Phase A8)** — unified RFC-0003 proof type:
  - `Receipt` — identity, what ran, execution contract, state transition, Merkle DAG.
  - `ReceiptHash = BLAKE3(op_hash ‖ graph_hash ‖ state_before ‖ state_after ‖ sorted(parent_hashes) ‖ shard_id ‖ committed_height)` — `produced_at` excluded for determinism.
  - `ReceiptAssembler` — collects `ReceiptFragment`s per execution, sorts by `task_index`, assembles the final `Receipt`.
  - `verify_receipt()` — offline verifier: recompute hash, check fragment state chain, outer consistency.
  - `ReceiptStore` — in-process last-256 cache; evicts oldest on overflow.
  - `GET /v1/proof/receipt` — latest assembled receipt (both standalone and cluster).
  - `GET /v1/proof/receipt/:id` — receipt by receipt_id (both standalone and cluster).
  - `ReceiptStore` injected as `axum::Extension` into both routers.
  See `docs/phases/phase-A8-receipt-assembler.md`.
- **TaskRunner + real capabilities in `valori-node` (Phase A7)** — wires the effect
  system into the live node:
  - `EngineKernelCapability` — implements `KernelCapability` against `SharedEngine`:
    deserializes `event_json → KernelEvent`, calls `apply_committed_event_ns()`,
    returns the BLAKE3 state hash. Non-blocking `state_hash()` via `try_read()`.
  - `HttpEmbedCapability` — implements `EmbedCapability` by delegating to the
    existing `embed_batch()` HTTP client (Ollama / OpenAI / custom).
  - `PassthroughHttpCapability` — implements `HttpCapability` for outbound fetches.
  - `CapabilityRegistryBuilder` — assembles a `CapabilityRegistry` for standalone mode.
  - `TaskRegistry` — maps all 12 `TaskKind`s to `Arc<dyn Task>` (Embed/InsertRecord/Search
    are real; remaining kinds use `NoOpTask` until A8).
  - `TaskRunner` — drives one `ExecutionGraph` in topological order: builds `TaskContext`,
    resolves predecessor outputs, retries `TaskFailed` up to `policy.retry_limit`, marks
    `ExecutionHandle` at each step.
  - `run_graph()` — spawns a `TaskRunner` on the tokio runtime, returns `ExecutionHandle`.
  3 unit tests; 0 failures. All prior tests unaffected.
  See `docs/phases/phase-A7-task-runner.md`.
- **`valori-effect` effect system crate (Phase A6)** — defines the single routing
  layer between task execution and subsystems.
  - `EffectId = BLAKE3(execution_id ‖ task_topological_index ‖ effect_index)` — stable
    across retries; the bus deduplicates by this id, preventing double-writes.
  - `EffectDurability`: `Durable` (bus awaits completion) vs `Ephemeral` (fire-and-forget).
  - `EffectPayload` variants: `KernelWrite`, `Receipt`, `Audit`, `Counter`, `Gauge`.
  - `EffectBus`: `dispatch()` (dedup-checked for Durable) + `dispatch_all()` (skips
    duplicates silently). Routes `KernelWrite` → `KernelCapability::apply_command`,
    `Receipt`/`Audit` → `ProofCapability::append_fragment`.
  - 7 capability traits: `KernelCapability`, `EmbedCapability`, `LlmCapability`,
    `StorageCapability`, `HttpCapability`, `ProofCapability`, `SchedulerCapability`.
  - `CapabilityRegistry` — optional capabilities return `Err(CapabilityUnavailable)`.
  - `Task` async trait + `TaskContext` (bus, capabilities, budget) + `TaskOutput`.
  - Concrete tasks: `EmbedTask`, `InsertRecordTask` (Durable KernelWrite), `SearchTask`
    (Durable ReceiptFragment, read-only proof), `NoOpTask`.
  - `NoOpKernelCapability` for tests. 9 tests; 0 failures.
  See `docs/phases/phase-A6-valori-effect.md`.
- **`valori-planner` execution planning crate (Phase A5)** — converts `Operation`
  + `PlanningContext` into a deterministic `ExecutionGraph` DAG.
  - `Operation` — immutable unit of user intent: `hash = BLAKE3(kind ‖ inputs ‖ policy)`.
    `OperationInputs` captures planning parameters only (k, collection, shard_id,
    rerank, embed flags) — not actual data — so two searches with the same config
    share the same cached graph.
  - `PlannerFingerprint` — `BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ schema_version)`.
    Changes when planner behavior changes.
  - `PlanningContext` — fully-typed (no HashMap), deterministically serializable.
    `PlanningContextHash = BLAKE3(bincode(context))`.
  - `ExecutionGraph` — DAG of `TaskSpec`s. `GraphHash = BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)`.
    Built with Kahn's topological sort; equal inputs always produce equal hash.
  - `ExecutionCache` — bounded in-process `RwLock<HashMap>` cache.
  - `ExecutionHandle` — `tokio::watch` channel wrapping `ExecutionStatus` lifecycle.
  - `ExecutionRegistry` — top-level cache + active-handle index with `retire()`.
  - `NoOpPlanner` + `IngestPlanner` — concrete `Planner` implementations.
  - `plan_with_cache()` — two-layer cache lookup (in-process → durable `MetadataDb`) before fresh planning.
  16 tests; 0 failures. See `docs/phases/phase-A5-valori-planner.md`.
- **`valori-metadata` control-plane crate (Phase A4)** — redb-backed persistent
  store for all control-plane types: `Project` (name, dir, port, dim, index,
  shard_count, node_count, mode), `Collection` + `CollectionRegistry` (elevated
  form of the node's inline `NamespaceRegistry`), `ShardTopology`, `SnapshotCatalog`
  with `prunable(keep)` policy enforcement, `ExecutionRecord` + `ExecutionRetentionPolicy`
  (stub), `PlannerCacheKey/Entry` (stub). `MetadataDb` uses 5 typed redb tables.
  `valori-metadata` has no dependency on `valori-kernel` or `valori-storage` —
  pure control-plane. 13 tests.
  See `docs/phases/phase-A4-valori-metadata.md`.
- **`valori-state` state lifecycle crate (Phase A3)** — corrects the Phase A2
  placement error (`recovery.rs` was in `valori-storage` but orchestrates state
  lifecycle, not raw I/O). New crate owns: `bootstrap` (crash recovery via event
  log, WAL, or snapshot), `manifest` (`StateManifest` — which files make up
  durable state), `lifecycle` (`StateLifecycle`: Recovering/Ready/Snapshotting),
  `shutdown` (`shutdown_snapshot` — synchronous snapshot-on-close). `StateError`
  wraps `StorageError` and `KernelError`. `valori-node` re-exports
  `valori_state::bootstrap as recovery` — zero call-site changes.
  See `docs/phases/phase-A3-valori-state.md`.
- **Architecture specification (RFC-0)** — six RFC documents freeze the Valori
  execution model before further crate creation:
  - `rfcs/0000-glossary.md` — 16 canonical terms (Operation, ExecutionGraph,
    Task, Effect, EffectBus, EffectDurability, KernelCommand, KernelEvent,
    KernelABI, Receipt, KernelSnapshot, ExecutionSnapshot, KnowledgeGraph,
    KernelState, ClusterState, PlannerFingerprint, PlanningContextHash,
    Collection, Shard) each with Definition, Owner, Lifetime, and Invariant.
  - `INVARIANTS.md` — 15 numbered system invariants (I-01 through I-15)
    covering immutability, content-addressing, determinism, apply protocol,
    task isolation, effect routing, shard atomicity, receipt assembly order,
    and `no_std` boundary. Each tagged with the crates it governs.
  - `COMPATIBILITY.md` — version policy for KernelABI, snapshot format (V5/V6),
    event log format (v2/v3), PlannerFingerprint, wire types, HTTP API, and
    rolling upgrade (two consecutive minor versions allowed simultaneously).
  - `rfcs/0001-operation-lifecycle.md` — Operation, PlanningContext,
    PlannerFingerprint, ExecutionGraph, ExecutionHandle, ExecutionRegistry
    (split into Cache + History + Analytics), planner cache, lifecycle diagram.
  - `rfcs/0002-kernel-contract.md` — KernelCommand, CommandId, exactly-once
    dedup, apply protocol (DEDUP→APPLY→AUDIT), namespace isolation (3 points),
    no_std boundary, one-Task-one-transaction, verifier contract, valori-state scope.
  - `rfcs/0003-receipt-spec.md` — unified Receipt schema (KernelABI +
    PlannerFingerprint + CapabilitySet + state_hash_before/after + Merkle DAG
    parent_receipts), ReceiptFragment, ReceiptAssembler (topological sort, not
    completion order), offline verification algorithm, migration path from
    EventProof / MCP receipt / Tree-RAG receipt.
  - `rfcs/0004-capability-model.md` — Capability trait hierarchy
    (Kernel/Embed/Llm/Storage/Http/Proof/Scheduler), Effect enum variants with
    EffectDurability, EffectBus (dispatch + dedup), Task trait + TaskContext,
    capability checking at plan time.
  - `rfcs/0005-crate-boundaries.md` — full dependency graph, per-crate ownership
    table, no_std boundary line, phase sequencing constraints (A3→A9),
    cargo-deny enforcement rules.
- **`valori-storage` durable storage crate (Phase A2)** — WAL, event log,
  event journal, crash recovery, and object store (S3/file) extracted from
  `valori-node` into a new `valori-storage` crate. All 2,400+ lines of
  storage code now live in one place with their own 23 tests. `valori-node`
  re-exports all modules via `pub use valori_storage::*` so no existing
  imports change. `StorageError` defined; `From<StorageError> for EngineError`
  added for ergonomic propagation. See `docs/phases/phase-A2-valori-storage.md`.
- **`valori-core` zero-dependency type crate (Phase A1)** — all platform
  identity types (`RecordId`, `NodeId`, `EdgeId`, `NamespaceId`,
  `CollectionId`, `ExecutionId`, `ShardId`, `ClusterEpoch`), domain enums
  (`NodeKind`, `EdgeKind`), `Version`, and `CoreError` extracted into a new
  `no_std` crate. `valori-kernel` re-exports from it; every other crate will
  follow in subsequent phases. `valori-core` builds for
  `wasm32-unknown-unknown` with no OS dependencies.
  See `docs/phases/phase-A1-valori-core.md`.
- **Document update with chunk-level diffing (Phase I8)** — new
  `POST /v1/ingest/update` endpoint accepts a `document_node_id` (from a
  prior `/v1/ingest` response) plus new text. Diffs old vs new chunks by
  BLAKE3 content hash: unchanged chunks are kept in place (no re-embed),
  removed chunks are soft-deleted (vector + graph node), and only genuinely
  new or changed chunks hit the embedding provider. The document graph
  node is reused so external edges remain valid. Works in both standalone
  and cluster mode (shard-routed, all writes via Raft). Python SDK:
  `ingest_update()` on both `SyncRemoteClient` and `AsyncRemoteClient`.
  See `docs/phases/phase-I8-document-update.md`.
- **Replication factor in the project-creation wizard (Phase 6.1)** — the
  UI's "New Project" dialog now offers "Single Node" or "3-Node Cluster"
  (Raft-replicated, tolerates 1 node down) as a first-class creation
  choice, instead of clustering living only on the separate `/launch`
  power-user page. Cluster projects get a `nodes[]` manifest entry (legacy
  single-port manifests migrate automatically), a dedicated 4010-4999 port
  range that never collides with single-node projects (3010-3999) or the
  Launcher (3000-3009), per-node data files under the same project dir,
  aggregate "2/3 running" status in the UI, and full open/close/delete
  lifecycle across all nodes (open waits for full quorum health; close
  snapshot-stops every node and re-locks files at rest). The two
  previously-divergent dimension option lists are unified into one shared
  module, and `/launch` now imports the same cluster-config helpers
  instead of maintaining its own copies. Verified live end to end,
  including leader election, follower reads, and close→reopen data
  persistence. See `docs/phases/phase-6.1-project-wizard-replication.md`.
- **Shard count in the project-creation wizard (Phase S14)** — the UI's
  first surface for horizontal scaling. Creating a 3-node-cluster project
  now offers a "Shards" control (1/2/4/8); the choice is persisted in the
  project manifest and threaded to `VALORI_SHARD_COUNT` on every spawned
  node (one process per replica still — all shards on a node share its
  HTTP port and gRPC listener). Cluster projects only; standalone
  projects have no shard concept and pin to 1. Verified live end to end:
  a 3-replica/2-shard project produced six independently chain-valid
  per-node-per-shard audit logs (`valori-verify` on each). Requires
  Phase S13 (below) — shard count was not safe to expose while shards
  ≥ 1 silently discarded their audit trail. Known gap, disclosed in the
  wizard itself: Proof/Timeline pages still read shard 0's log only.
- **Shard routing completed across the entire cluster HTTP surface (Phases
  S5-S9)** — every collection-aware endpoint now routes to the shard that
  actually owns its namespace's data, closing out the routing work started
  in S3/S4:
  - **S5** — `cluster_insert_encrypted` routes by namespace;
    `DELETE /v1/crypto/shred/:key_id` fans out to every shard this node
    runs (ciphertext for one key can land on multiple shards) and
    aggregates per-shard status into `{"shredded": bool, "shards": {...}}`.
  - **S6** — linearizable reads are shard-aware:
    `ensure_read_consistency(shard_id, ...)` and
    `GET /v1/cluster/read-index?shard=N`; `cluster_memory_search` gained a
    read-index check it never had before (previously always
    eventually-consistent regardless of the requested `consistency`).
  - **S7** — core CRUD (`/v1/records`, `/v1/search`, `/v1/delete`,
    `/v1/soft-delete`, `/v1/vectors/batch-insert`) gained a `collection`
    field and shard routing, matching the standalone server's existing
    contract.
  - **S8** — graph node/edge CRUD (`/v1/graph/*`), `/v1/graphrag`, and
    namespace-scoped `/v1/community/detect` now route to their collection's
    shard.
  - **S9** — `cluster_ingest` gained automated test coverage via an
    in-process mock embed server; `cluster_tree_hybrid`'s vector-search
    section now routes to the resolved namespace's shard (previously
    resolved the namespace correctly but scanned shard 0 regardless — a bug
    flagged back in S1 and never revisited until now).

  See `docs/phases/phase-S5-crypto-shredding-cross-shard.md` through
  `docs/phases/phase-S9-ingest-coverage-tree-hybrid.md`.

- **Namespace→shard routing (Phases S3+S4)** — deterministic
  `shard_for_namespace(namespace_id, shard_count)` (`namespace_id % shard_count`,
  no placement table needed) and a multi-shard-aware `DataPlaneState`.
  `cluster_memory_upsert`, `cluster_memory_consolidate`,
  `cluster_extract_entities`, and `cluster_ingest` (writes) plus
  `cluster_list_nodes` and `cluster_memory_search` (reads) now route to the
  shard that actually owns a namespace's data, instead of always shard 0 —
  every collection-aware write handler is now shard-routed. `cluster_extract_entities`
  also had a latent id-allocation race fixed as part of making its routing
  safe (was pre-reading "next id" from the wrong shard's counter). See
  `docs/phases/phase-S3-shard-routing-infrastructure.md` and
  `docs/phases/phase-S4-remaining-write-handlers.md`.

### Fixed
- **Documents in named collections vanished after close/reopen (Phase S15)**
  — the standalone audit log recorded events without a namespace, so on
  recovery every event replayed into the default collection and the named
  collection came back empty. Data was never lost (the events were all on
  disk), just re-shelved into the wrong collection on each restart. Added
  an append-only `LogEntry::EventNs` wire variant that records the
  namespace; commit, replay, and every log reader (`valori-verify`,
  timeline, inspect, the legacy replication stream) are now
  namespace-aware. Default-collection logs stay byte-identical to before,
  and pre-S15 logs replay unchanged. Note: writes made *before* this fix
  stay in the default collection (their log entries lack the namespace);
  point-in-time `as_of` search in a non-default collection remains a known
  gap (the journal is namespace-agnostic). See
  `docs/phases/phase-S15-namespaced-event-log.md`.
- **Shards ≥ 1 silently discarded their audit trail (Phase S13)** —
  `bootstrap_cluster()` only ever gave shard 0 a real audit sink; every
  other shard got a hardcoded `NullAuditSink` that discards events without
  writing them to disk. This was an intentional S1-era decision made when
  no HTTP traffic could reach shard ≥ 1 — invalidated once S3-S9 wired real
  namespace→shard HTTP routing to every shard, but never revisited. Writes
  to a non-zero shard were still correctly Raft-committed and applied to
  that shard's `KernelState`, but had no BLAKE3 chain on disk. Every shard
  now gets its own genuine `events-shardN.log` (unchanged filename at
  `shard_count == 1`). A failure to open shard 0's audit log remains fatal
  (unchanged); a failure on shards ≥ 1 falls back to `NullAuditSink` for
  that shard only, logged loudly, rather than aborting the whole node —
  new capability this phase adds, no prior "fatal" guarantee to preserve
  there. See `docs/phases/phase-S13-per-shard-audit-sinks.md`.
- **Cluster mode's `GET /v1/graph/node/:id` and `GET /v1/graph/edges/:id`
  returned different field names than the standalone server (Phase S12)**
  — e.g. `{"id","kind","record"}` vs standalone's
  `{"kind","record_id","namespace_id"}`. Harmless for callers reading raw
  JSON, but the Python SDK's `walk()`/`expand()`/`neighbors()` read
  specific keys (`record_id`, `to_node`) and threw `KeyError` against
  cluster nodes. Predates S1-S11 entirely; found while documenting S11.
  Cluster now emits the same shape as standalone. `GET /v1/graph/subgraph`
  and `/v1/graphrag` were unaffected — they already shared one function
  between both modes.
- **Python SDK graph methods had no `collection` support (Phase S11)** —
  `create_node()`, `get_node()`, `create_edge()`, `get_edges()`,
  `subgraph()`, and `neighbors()` on both `SyncRemoteClient` and
  `AsyncRemoteClient` always targeted the default collection — the server
  side has always supported `collection` on these endpoints (and the
  cluster path routes it correctly as of S8), but the SDK never exposed
  it. All six gained a `collection: str = "default"` parameter,
  backward-compatible with every existing call site.
- **`valoricore-ffi` did not compile (Phase S10)** — `get_timeline()`'s
  exhaustive `KernelEvent` match was missing arms for
  `AutoCreateNamespace`/`DropNamespace` (added in S2). Predates the S1-S9
  sharding work — confirmed present on `main` before any of it. Fixed and
  verified with a real `maturin build --release` (the crate's actual build
  path; a bare `cargo build -p valoricore-ffi` never links successfully by
  design — PyO3's `extension-module` feature omits `libpython`).
- **Python SDK `soft_delete()` permanently deleted records instead of
  soft-deleting them (Phase S7)** — `SyncRemoteClient.soft_delete()` and
  `AsyncRemoteClient.soft_delete()` posted to `/v1/delete` (hard delete)
  instead of `/v1/soft-delete`, on both standalone and cluster targets.
  Fixed both methods to hit the correct endpoint; `crates/valori-node/README.md`'s
  API table had the same mislabeling, corrected, and `/v1/soft-delete`
  (previously undocumented) added as its own row. `delete()`/`soft_delete()`
  also gained an optional `collection` parameter on both clients (and their
  `ClusterClient`/`AsyncClusterClient` wrappers) — previously always scoped
  to the default collection regardless of where the record actually lived.
- **Collections/namespaces for graph data (nodes/edges) and vector-record
  writes were non-functional in cluster mode (Phase S3a)** —
  `ValoriStateMachine::apply()`'s generic dispatch always applied
  `AutoInsertRecord`/`AutoCreateNode`/`AutoCreateEdge` to namespace 0
  regardless of which collection a handler resolved (`cluster_memory_upsert`/
  `cluster_memory_consolidate` resolved a namespace id and then discarded
  it). Only the crypto-shredding path
  (`InsertRecordEncrypted`/`AutoInsertRecordEncrypted`) was genuinely
  namespace-scoped. Fixed by adding `namespace_id` to `ClientRequest`
  (`#[serde(default)]`, backward compatible) and threading it through
  `apply()`'s generic dispatch. Verified live: writes to two different
  collections now correctly land in their own namespaces (and, combined
  with the routing above, their own shards).
- **Cluster-mode collection creation was not Raft-replicated (Phase S2)** —
  `POST /v1/namespaces` mutated a private, per-node, in-memory registry
  directly. Two nodes could silently assign different `NamespaceId`s to the
  same collection name (or the same id to different names), and a follower
  would happily "succeed" against its own out-of-sync copy instead of
  redirecting to the leader. Now goes through Raft like every other write
  (`KernelEvent::AutoCreateNamespace`/`DropNamespace`); every node ends up
  with the identical, durable mapping, and a follower correctly
  307-redirects. See `docs/phases/phase-S2-namespace-replication.md`.
- **Snapshot `CapacityExceeded` at scale** — `encode_state` rewritten from a
  fixed `&mut [u8]` buffer to a growable `&mut Vec<u8>`. Snapshots above ~250K
  records (any dimension) previously failed with `Kernel(CapacityExceeded)`
  because the V6 schema added 10 bytes/record that the buffer-size formula did
  not account for. Verified end-to-end at 1M records (515 MB snapshot in 1.2 s).
  The encoder is now structurally incapable of this error. Stays `no_std`.
- **WAL loss on clean teardown** — added `impl Drop for Engine` and
  `impl Drop for EventCommitter` to flush the batched write buffer on scope
  exit. A clean shutdown could previously lose up to `flush_every` buffered
  events; recovery tests found 0 events after a simulated crash.

### Added
- **Multi-Raft consensus skeleton (Phase S1)** — a cluster process can now run
  multiple independent Raft groups ("shards") sharing one gRPC listener, each
  with its own persistent redb log, state machine, and leader election.
  New `VALORI_SHARD_COUNT` env var (default `1`, byte-identical to prior
  single-Raft-group behavior). Foundation for future namespace-sharded
  horizontal scaling — namespace→shard routing and HTTP-layer wiring are not
  part of this phase. See `docs/phases/phase-S1-multi-raft-skeleton.md`.
- **IVF centroid auto-scaling** (`n_list = max(16, sqrt(N))`, `n_probe = max(1, sqrt(n_list))`) — fixes a 153× QPS regression from 10K to 1M records. Centroids now scale with dataset size so average bucket size stays O(sqrt(N)) and scan cost is O(sqrt(N)) not O(N). Manual override via `VALORI_IVF_N_LIST` / `VALORI_IVF_N_PROBE` disables auto-scaling. Added `IvfIndex::needs_rebuild(count)` hook (returns true when online inserts exceed 2× the build size).
- **`encode_capacity_hint(state)`** — V6-correct pre-allocation estimate so the
  snapshot `Vec` avoids repeated reallocation on the hot path.
- **SIMD L2 distance** (`l2_sq_i32`) — NEON (aarch64) + AVX2 (x86_64) paths with
  scalar fallback; identical integer result on every path (determinism
  preserved), purely a speedup.
- **Benchmark suite** — `benchmarks/local_perf.py` (B1–B7) + `RESULTS_1M.md`,
  with a full performance section and HNSW-above-50K / small-batch warnings in
  the root `README.md`.

## [0.2.3] — 2026-06-29

### Security
- **SEC-2** `SyncRemoteClient` — bearer token was stored in `session.headers`
  (visible in `dict(session.headers)`, Python logging, and tracebacks). Ported
  the `_BearerAuth(requests.auth.AuthBase)` redaction pattern from
  `protocol.py`; token now injected per-request via `__call__`, never stored
  in the headers dict. `_BearerAuth.__repr__` returns `[REDACTED]`.
- **SEC-3** `ProtocolRemoteClient.set_metadata()` / `get_metadata()` — both
  called `session.post/get` without `auth=self._auth`, bypassing authentication
  even when an API key was configured. Fixed; all HTTP calls in
  `ProtocolRemoteClient` are now authenticated.
- **SEC-4** `set_metadata` — `metadata.decode(errors='replace')` silently
  corrupted binary metadata on round-trip (`b'\xff\xfe'` → garbage). Resolved
  by unifying the metadata type to `Dict[str, Any]` with a JSON codec; the
  corrupt decode path is gone entirely.

### Fixed
- **BUG-2** `ProtocolRemoteClient.upsert_text()` crashed with `KeyError` on
  every call — `res["proof_hash"]` hard-access on a field the server does not
  return. Changed to `res.get("proof_hash", "")`.
- **BUG-3** `test_batch_verify.py` called `exit(1)` at module scope when
  `VALORI_URL` was not set, killing the entire pytest process. Replaced with
  `pytest.skip()` inside the test function.
- **BUG-4** `record_count()` always returned 0 — `resp.json().get("record_count", 0)`
  but `/health` returns `{"records": {"live": N}}`. Fixed to
  `resp.json().get("records", {}).get("live", 0)` on both sync and async clients.
- **BUG-5** Duplicate, incompatible exception hierarchies — `protocol.py`
  defined its own `ValoricoreError`, `ValidationError`, `AuthError`,
  `ProtocolError` as separate classes from `exceptions.py`. `except
  valoricore.ValidationError` would not catch a `protocol.ValidationError`.
  Deleted the four duplicates from `protocol.py`; all now imported from
  `exceptions.py`. `ValidationError` now also inherits `ValueError`.
  `AuthError` kept as a backward-compat alias for `AuthenticationError`.
- **#3** `record_count()` — same as BUG-4 above (sync + async).
- **#4** `factory.py` — `Valoricore(remote=…, token=…)` silently dropped the
  token; `SyncRemoteClient` was constructed with no auth. Fixed by forwarding
  `token=token` in both `Valoricore` and `AsyncValoricore`.
- **#5** `ValoriClient` ABC added — shared interface for `LocalClient` and
  `SyncRemoteClient`. `LocalClient` methods widened to accept
  `collection/text/consistency/metadata_filter` kwargs (ignored with annotation)
  so factory-swapped code never raises `TypeError`.
- **#6** Metadata types unified — `insert_batch` now accepts
  `List[Optional[Dict[str, Any]]]` (SDK serialises each dict to a JSON string);
  `get_metadata`/`set_metadata` use `Dict[str, Any]` on all clients with JSON
  encode/decode. `LocalClient` stores as UTF-8 JSON bytes internally.
- **#7** `AsyncRemoteClient` timeout — constructor now accepts
  `timeout: float = 10.0` forwarded to `httpx.AsyncClient`; `AsyncValoricore`
  factory passes it through.
- **#8** BFS O(n²) — all three `walk()` implementations (`LocalClient`,
  `SyncRemoteClient`, `AsyncRemoteClient`) replaced `list.pop(0)` with
  `collections.deque` + `popleft()`.
- **#9** `EXPECTED_DIM = 384` removed from `memory.py`; dead imports cleaned
  from `protocol.py` and `async_memory.py`. `MemoryClient` already used
  `self._dim` for validation; the constant had no effect.
- **#10** Context-manager support — `SyncRemoteClient` gains `close()` /
  `__enter__` / `__exit__`; `AsyncRemoteClient` and both `ClusterClient`
  variants gain `__aenter__` / `__aexit__`.
- **#11** `__init__.py` module docstring — moved to first statement so
  `__doc__` is populated; RST grid table replaced with plain text readable in
  `help()` and `pydoc`.
- **#12** `ClusterClient.close()` — closes all N underlying `requests.Session`
  pools; adds `__enter__` / `__exit__`.
- **#13** `__version__` fallback — `except Exception` narrowed to
  `except PackageNotFoundError`; fallback changed from `"0.0.0"` to `"dev"` to
  distinguish an unregistered editable install from a real release.
- Test suite — 42 offline test failures resolved; `conftest.py` added with
  auto-skip for integration tests, env-var cleanup, and shared fixtures.
  `addopts = "-m 'not integration'"` means `pytest` on a clean checkout runs
  73 tests with 0 failures.

### Added (Phase I7 — Metadata filtering)
- **`metadata_filter` on `POST /search`** — optional JSON predicate that restricts
  results to records whose stored metadata satisfies all specified key-value conditions.
  Supports exact equality for strings/booleans/null and range operators (`gt`, `gte`,
  `lt`, `lte`, `eq`) for numeric fields. Example:
  `{"author": "Alice", "year": {"gte": 2020}}`. Both standalone and cluster paths
  are covered. When a filter is present the server over-fetches `k×10` candidates
  (capped at 5000) before post-filtering to ensure `k` results are returned.
- **Python SDK** — `SyncRemoteClient.search()` and `AsyncRemoteClient.search()` both
  accept `metadata_filter: Optional[Dict[str, Any]] = None`. `ClusterClient` and
  `AsyncClusterClient` inherit via `**kwargs`.

### Added (Phase I6 — Community layer: global sensemaking + entity extraction)
- **`POST /v1/community/detect`** — Label Propagation on the existing GraphNode
  adjacency list (pure Rust, zero LLM). Assigns every node a `community_id`,
  computes an f32 centroid vector per community (average of member FxpVectors),
  and emits a BLAKE3 receipt over the sorted `(node_id, community_id)` map —
  a tamper-evident proof of community structure at that point in time.
  Community store cached in-process; accessible by subsequent search calls.
- **`POST /v1/community/search`** — Cosine-similarity search over community
  centroids. Returns top-k communities ranked best-first with `member_count`
  and a `sample_node_ids` list. Answers "what are the themes across all
  documents?" — the global-sensemaking query that vector RAG cannot handle.
- **`POST /v1/ingest/extract-entities`** — Sends text to the configured LLM
  (reuses `VALORI_EMBED_PROVIDER` credentials — no new env vars). Parses
  `(entity, type, description)` tuples and `(source, target, description,
  strength)` relationships. Embeds entity descriptions and inserts them as
  `Concept` graph nodes with `Relation` edges — bridges a document graph into
  a true entity knowledge graph.
- All three endpoints exist in both **standalone** (`server.rs`) and **cluster**
  (`cluster_server.rs`) paths, following the mandatory dual-path rule.
- `valori-kernel`: added `incoming_edges()` on `KernelState` so Label
  Propagation can traverse both directions of the adjacency list.
- Python SDK: `community_detect()`, `community_search()`, `extract_entities()`
  on both `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I5 — Tree-RAG: hierarchical retrieval with provable receipts)
- **`POST /v1/tree/build`** — parse a structured/markdown document into a
  navigable table-of-contents tree (sections, parent/child, line ranges).
  Deterministic, zero-LLM, zero-embedding. Returns `{node_count, structure_map, tree}`.
- **`POST /v1/tree/query`** — navigate the tree to the *right section* and answer
  with a breadcrumb + line-range citation and a BLAKE3-chained **retrieval receipt**.
  Distinguishes vocabulary-overlapping sections (e.g. "sick days" → *Sick Leave*,
  not *Annual Leave*) where plain vector search fails. Supports `prev_hash` to
  chain receipts.
- **`POST /v1/tree/verify`** — replay a receipt against the tree; `valid: false`
  proves the stored content was altered after retrieval (tamper detection).
- All three are stateless handlers — identical in standalone and cluster mode.
- Python SDK: `tree_build` / `tree_query` / `tree_verify` on both
  `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I5 gap-fill — server-side tree cache + hybrid retrieval)
- **Server-side tree cache** — `Engine` (standalone) and `DataPlaneState` (cluster) now
  hold a `HashMap<String, TreeIndex>` keyed by `BLAKE3(text)`. `/v1/tree/build` stores the
  parsed tree and returns `cache_key` in the response. Subsequent `/v1/tree/query` and
  `/v1/tree/hybrid` calls accept `cache_key` instead of re-transmitting the full tree.
- **`POST /v1/tree/hybrid`** — single-call hybrid retrieval fusing tree-RAG section scores
  (term-frequency, normalized to [0,1]) with vector-search similarity scores (if
  `VALORI_EMBED_PROVIDER` is set). Configurable `tree_weight` (default 0.6). Returns merged,
  re-ranked hits with per-hit `source` tag (`"tree"` or `"vector"`), BLAKE3 receipt for the
  tree path, and a human-readable `reasoning` string. Available on both standalone and cluster.
- **`/v1/tree/build` and `/v1/tree/query`** are now stateful (take engine state for cache
  read/write); `/v1/tree/verify` remains stateless (no cache dependency).
- Python SDK: `tree_hybrid()` added to both `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I4.1 — replicated metadata sidecar)
- **`KernelEvent::SetMeta { key, value }`** — new kernel event storing a
  replicated `meta` map on `KernelState`. Cluster ingest now writes the chunk/
  document metadata sidecar via `raft.client_write(SetMeta)` so **all** peers
  share it (previously node-local on the ingesting node only).
- **`/v1/memory/meta/set` + `/v1/memory/meta/get`** added to the cluster router,
  reading/writing through the kernel (`sm.with_state`) instead of a node-local map.

### Added (Phase I1/I2/I3 — Built-in ingest pipeline)
- **`POST /v1/ingest/document`** — server-side document chunking with five strategies:
  `auto` (sniffs text), `tree` (section headers), `conversation` (Q&A boundaries),
  `sentence` (sentence-window with ±2 context), `fixed` (overlapping windows).
  Returns `{strategy_used, chunk_count, chunks: [{index, title, text}]}`.
  Works in both standalone and cluster mode (stateless handler).
- **`POST /v1/ingest`** — full one-call pipeline: chunk + embed + insert + graph nodes +
  metadata sidecar. Requires `VALORI_EMBED_PROVIDER` (ollama / openai / custom).
  Supports `VALORI_EMBED_MODEL`, `VALORI_EMBED_URL`, `VALORI_EMBED_API_KEY`.
  Returns `{document_node_id, chunk_count, record_ids, strategy_used}`.
- **`/health`** now includes `embed_enabled: bool` and `embed_provider: string?` so
  clients can probe node capability before deciding on a pipeline.
- **`crates/valori-node/src/embedder.rs`** — HTTP embed client with Ollama fallback
  (`/api/embed` → `/api/embeddings`) and OpenAI-compatible batching.
- **Python SDK** — `SyncRemoteClient.chunk_document()`, `ingest()`,
  `AsyncRemoteClient.chunk_document()`, `ingest()`.
- **UI** — DocumentUploadTab probes node on mount; shows "Server-side pipeline active ⚡"
  banner and routes upload through `/v1/ingest` when embed is configured;
  falls back transparently to client-side pipeline otherwise.
- **Phase I4 — cluster ingest**: `POST /v1/ingest` now works in 3/5-node cluster mode.
  Vectors and graph nodes/edges go through `raft.client_write()` and are replicated to
  all peers. `DataPlaneState` gains `embed_config` and node-local `metadata` sidecar.
  `build_cluster_router` auto-reads `VALORI_EMBED_*` env vars. Cluster `/health`
  now exposes `embed_enabled` + `embed_provider`.

### Added (Phase C5 — Valori Reranker)
- **Valori Reranker** (`crates/valori-node/src/valori_reranker.rs`) — server-side hybrid
  retrieval that runs inside the node with no external dependency. Records inserted with a
  `text` field are tokenised and indexed. At search time, `query_text` triggers a two-stage
  pipeline: the kernel returns `k × POOL_FACTOR` candidates by vector similarity, the
  reranker blends vector and term-frequency scores (50 / 50), and the top-k are returned.
  Achieves **90 % accuracy** on hard lexical queries vs 60 % for LLM-based navigation, at
  **0.4 s** latency.
- `/records` and `/v1/vectors/batch_insert` accept `text` / `texts` fields for reranker
  indexing. `/search` accepts `rerank: bool` (default `true`) and `query_text: string`.
- `SyncRemoteClient` and `AsyncRemoteClient` updated: `insert(text=)`,
  `insert_batch(texts=)`, `search(rerank=True, query_text=)`, and new `health()` method.
- Cluster path: `ValoriStateMachine` stores raw texts in `text_corpus`; `cluster_server`
  builds a transient reranker per query from the corpus via `with_text_corpus()`.
- `KernelState::iter_records_in_ns(namespace_id)` — public iterator over records in a
  namespace, used by `drop_collection` to clean up the reranker on collection drop.

### Added (Phase 6 — Persistent, isolated projects in the UI)
- **Each UI project is now its own persistent, isolated workspace.** A project maps to one
  `valori-node` process with its own data dir, port, and WAL/snapshot under
  `~/.valori/projects/<name>/` (manifest at `~/.valori/ui-projects.json`, kept separate from
  the CLI wizard's `projects.json`). Home is now a project picker that lists every project
  from disk — even when all nodes are stopped — and one click resumes a session.
- **Auto-start on open / snapshot-on-close.** Opening a project boots its node and points the
  UI at it; closing writes a final snapshot, stops the node, and re-locks the files at rest.
- **Files are deletable only through the UI.** Data files carry the macOS immutable flag
  (`chflags uchg`; Linux falls back to read-only perms) while a project is at rest — Finder
  and `rm` refuse to remove them. The UI delete path clears the flag first.
- **Node graceful-shutdown snapshot.** Standalone `valori-node` now serves with a
  `SIGTERM`/`Ctrl-C` handler that writes a final snapshot to `VALORI_SNAPSHOT_PATH` before
  exiting — a durable backstop on top of the always-on WAL.
- New UI API routes `GET/POST /api/projects`, `DELETE /api/projects/[name]`, and
  `POST /api/projects/[name]/{open,close}`. The Launcher's defaults moved off `/tmp` to
  `~/.valori/cluster`.

### Changed (Python SDK — full endpoint coverage)
- **The Python SDK now wraps every product endpoint (40/40).** Newly added to `SyncRemoteClient` and `AsyncRemoteClient`:
  - **Agent-memory primitives** — `memory_upsert()` (`/v1/memory/upsert_vector`: store vector + document→chunk graph, returns `memory_id`) and `memory_search()` (`/v1/memory/search_vector`: hits carry `memory_id`, `metadata`, and decay fields). Previously only the lower-level `insert`/`search` (which return `{id, score}` with no `memory_id`/metadata) were exposed.
  - **Proof / provenance** — `event_log_proof()` (`/v1/proof/event-log`: the receipt primitive — event-log hash, state hash, committed height). Also on `ClusterClient`/`AsyncClusterClient`.
  - **Graph / introspection** — `list_nodes()` (`/graph/nodes`), `get_version()` (`/version`).
  - **Snapshot / object-store offload** — `save_snapshot()`, `restore_snapshot()`, `list_remote_snapshots()`, `upload_snapshot_to_store()`, `restore_from_store()`, `list_remote_wal()`, `archive_wal_segment()`.
- **Deprecated** `list_contradictions()` / `resolve_contradiction()` — legacy C3 methods that called the Next.js UI layer (`ui_url`), not the node, and returned whatever that layer held (historically `[]`). They now emit `DeprecationWarning` pointing to the node-native, audited `contradict()` / `consolidate()`. Scheduled for removal.

### Added (Phase C4.3 — Contradiction detection: self-maintaining memory, pillar 3)
- **`POST /v1/memory/contradict`** — given two record ids, computes cosine similarity between their Q16.16 vectors and, if it meets `threshold` (default 0.85), commits an `AutoCreateEdge(record_a → record_b, Contradicts)` to the BLAKE3 audit chain. Request `{ record_a, record_b, threshold?, collection? }`; response `{ record_a, record_b, similarity, contradicts, edge_id?, state_hash }` (`edge_id` only when `contradicts`). On both standalone and cluster data planes.
- **`EdgeKind::Contradicts = 8`** — new kernel edge kind (no_std-safe); the verdict is a first-class hashed event, not mutable metadata.
- **Python SDK** — `contradict(record_a, record_b, threshold=, collection=)` on all four clients; cluster variants route to the leader.
- **v1 boundary (documented):** "contradiction" is currently a structural proxy — cosine similarity ≥ threshold, which detects near-duplicates, *not* semantic NLI. The hashed `Contradicts` event path is signal-agnostic: a real entailment model can replace the cosine gate at the node layer with zero kernel change. See `docs/phases/phase-C4.3-contradiction.md`.

### Added (Phase C4.2 — Memory consolidation: self-maintaining memory, pillar 2)
- **`POST /v1/memory/consolidate`** — replace a memory in one auditable operation: commits `SoftDeleteRecord(old)` → `AutoInsertRecord(new)` → `AutoCreateEdge(new → old, Supersedes)` to the audit chain. Request `{ old_record_id, new_vector, collection?, metadata? }`; response `{ old_record_id, new_record_id, supersedes_edge_id, state_hash }`. On both standalone and cluster data planes.
- **`EdgeKind::Supersedes = 7`** — new kernel edge kind (no_std-safe) linking a replacement to the memory it retired, so a reader can trace why a record was soft-deleted.
- **Python SDK** — `consolidate(old_record_id, new_vector, collection=, metadata=)` on all four clients; cluster variants route to the leader.
- **Atomicity:** standalone is atomic (single engine write lock across all three events). Cluster commits the events as a sequence of Raft entries — each chain-valid and replicated, but not a single transaction; a mid-sequence leader crash can leave a partial result (follow-up: multi-event `ClientRequest`). See `docs/phases/phase-C4.2-consolidation.md`.

### Added (Phase C4.1b — Cluster decay + state-machine creation timestamps)
- **Cluster `/search` now honours `decay_half_life_secs`.** In C4.1 the cluster endpoint accepted the field but ignored it; now the consensus state machine tracks per-record creation timestamps (`StateMachineInner.created_at`, stamped at `AutoInsertRecord` apply time) and the cluster search path runs the same over-fetch → `decay::rerank` → top-k pipeline as standalone. One SDK call now behaves identically against both node types.
- **`ValoriStateMachine::record_created_at` / `with_state_and_timestamps`** — read accessors exposing creation time to the search path under one lock.
- **Determinism preserved** — `created_at` is a derived, non-hashed, non-replicated side map (same design as standalone `Engine.created_at`), so the BLAKE3 state hash is unchanged. Known boundary: a node that restarts or installs a snapshot loses timestamps and ranks pre-event records neutrally until re-stamped — durable WAL timestamps are deferred to **C4.1c**. See `docs/phases/phase-C4.1b-cluster-decay.md`.
- **Internal:** new `raft_write_data` helper returns the committed `ClientResponse` so cluster multi-step writes (consolidate/contradict) read allocated record/node/edge IDs from the apply response instead of pre-reading them — closing a TOCTOU race against concurrent writers.

### Added (Phase C4.1 — Kernel-native time decay: self-maintaining memory, pillar 1)
- **`decay_half_life_secs`** on `POST /search` and `POST /v1/memory/search_vector` — recency-aware re-ranking. When set (> 0), older records decay: a record one half-life old has its L2 distance doubled, so a fresh near-match can overtake a stale better one. Each hit gains `decay_factor` (∈ (0,1]) and `age_secs`; `score` stays the true, undecayed distance. Absent/`0` → byte-identical to the prior response.
- **`VALORI_DECAY_HALF_LIFE_SECS`** — optional server-default half-life; a per-request value wins (incl. an explicit `0` to disable).
- **Determinism preserved** — decay is a read-time re-rank: it never mutates kernel state, emits no event, and does not change the BLAKE3 state hash (regression-tested). Creation time lives in a derived, non-hashed `Engine.created_at` map stamped on live inserts only.
- **Python SDK** — `search(..., decay_half_life_secs=…)` on all four clients (`Sync`/`Async` `RemoteClient`, `ClusterClient` via `**kwargs`).
- **MCP** — `memory_recall` accepts `decay_half_life_secs` for recency-aware agent recall; the receipt still verifies over the decayed result set.
- **Supersedes the UI-only Phase C3** "self-maintaining memory," which shipped no decay and lived outside the audit chain. See `docs/phases/phase-C4.1-decay.md`.
- Known boundaries (v1): cluster decay is accepted-but-neutral (creation time isn't tracked in the consensus state machine yet — C4.1b); `created_at` is in-memory, so recovered records rank neutrally until re-stamped (durable WAL timestamps — C4.1b).

### Added (Phase 3.15 — Native GraphRAG: one-call retrieval)
- **`POST /v1/graphrag`** — retrieve the K nearest vectors **and** the connected knowledge subgraph around them in a single call, from one consistent kernel snapshot. Request `{ query_vector, k, depth, collection? }`; response `{ hits, seed_nodes, subgraph: { nodes, edges } }`. Added to both standalone and cluster data planes (cluster also honours `consistency`).
- **`memory_graph_recall` MCP tool** — GraphRAG with a receipt that binds **both** the hits and the returned subgraph (`receipt.subgraph = { node_ids, edge_ids }`, sorted). valori-mcp now exposes 7 tools.
- **Shared `graph_rag` module** (`expand_subgraph`, `resolve_seed_nodes`) — one BFS implementation reused by `/v1/graphrag`, `/graph/subgraph`, and the cluster equivalents, so the traversal stays identical across paths.
- **Python SDK** — `graphrag(query_vector, k, depth, collection, consistency)` on `SyncRemoteClient`, `AsyncRemoteClient`, `ClusterClient`, and `AsyncClusterClient` (cluster variants route to a read replica).
- Plain `memory_recall` receipts are unchanged on the wire (the new optional `subgraph` field is omitted when absent).

### Added (Phase 3.14 — MCP server: verifiable agent memory)
- **New crate `valori-mcp`** — a Model Context Protocol server (stdio, protocol `2024-11-05`) exposing a Valori node as verifiable, deterministic long-term memory for agents. New binary `valori-mcp`.
- **Six MCP tools** — `memory_write`, `memory_recall`, `memory_why`, `memory_timeline`, `memory_forget`, `memory_fork` — each a thin composition over existing node endpoints.
- **Retrieval receipts** — `memory_recall` returns a `receipt`: `receipt_digest = BLAKE3(canonical_json(body))` binding the exact result set to the committed `state_hash`, `event_log_hash`, and `committed_height` at recall time. Independently recomputable offline by any client, in any language.
- **`VALORI_URL` / `VALORI_AUTH_TOKEN`** (and `--url` / `--auth-token`) configure the node the MCP server talks to.
- **`examples/mcp_agent_memory.py`** — runnable end-to-end demo that boots a node, drives the MCP handshake, and re-derives the receipt digest in Python to prove cross-language verification. **`examples/claude_desktop_config.json`** — copy-paste client config.

### Added (Phase 3.13 — HNSW parameter exposure)
- **`VALORI_HNSW_M`** — sets max edges per node per layer; `m_max0` and `lambda` are derived automatically (`m_max0 = 2*M`, `lambda = 1/ln(M)`).
- **`VALORI_HNSW_EF_CONSTRUCTION`** — sets beam width during index build (default 100). Higher = better recall at the cost of insert throughput.
- **`VALORI_HNSW_EF_SEARCH`** — sets beam width floor during queries (default 50). Higher = better recall at the cost of query latency.
- **`GET /v1/index/config`** — new endpoint returning active index type and current HNSW parameters. Returns `{"index_type":"hnsw","hnsw":{"m":…,"m_max0":…,"ef_construction":…,"ef_search":…}}` for HNSW or `{"index_type":"brute_force","hnsw":null}` for brute-force.
- **Python SDK** — `SyncRemoteClient.get_index_config()` and `AsyncRemoteClient.get_index_config()` wrap the new endpoint.
- `HnswIndex::new_with_config(config: HnswConfig)` constructor; `HnswConfig` gains `ef_search` field.
- `Engine` stores `hnsw_config: HnswConfig` so `rebuild_index()` preserves operator-supplied parameters across crash recovery.

### Added (Phase 3.10 — Signed releases + SBOM)
- **cosign keyless signing** — every release binary and Docker image is signed
  using GitHub Actions OIDC → Sigstore transparency log. No private key to
  manage. Verify with `cosign verify-blob --certificate ... --signature ...`.
- **SPDX 2.3 SBOM** — `valori-sbom.spdx.json` generated via `cargo-sbom` on
  every release tag and attached to the GitHub Release with its own cosign
  signature.
- **Multi-platform binaries** — `linux/amd64`, `linux/arm64`, `darwin/amd64`,
  `darwin/arm64` in every GitHub Release alongside SHA-256 checksums.
- **SOC 2 evidence collection** — `scripts/soc2/collect_evidence.py` hits
  `/v1/proof/*`, `/v1/keys`, `/v1/cluster/status`, `/v1/storage/snapshots`
  and writes an evidence bundle with control-family mappings (CC6.6, CC7.2, A1.1, CC9).
- **Weekly evidence workflow** — `.github/workflows/soc2-evidence.yml` collects
  and uploads a 90-day-retained artifact bundle every Sunday at 02:00 UTC.

### Added (Phase 3.9 — Terraform modules)
- **`terraform/aws/`** — EKS cluster, VPC (3 AZs), S3 Object Lock bucket (KMS
  encrypted), IAM IRSA role for pod-level S3 access, ALB controller role,
  CloudWatch alarms for `state_hash_match` and replication lag.
- **`terraform/azure/`** — AKS cluster, Azure Blob Storage (ZRS, versioning,
  lifecycle policy), Key Vault (purge-protected, Premium SKU for Phase 5 CMK),
  Log Analytics workspace (90-day retention), Monitor alerts.
- **`docs/DEPLOY_AWS.md`** — Quick-start, variables, Helm deploy, cost estimate (~$575/mo).
- **`docs/DEPLOY_AZURE.md`** — Quick-start, SOC 2 KQL queries, CMK upgrade path, cost estimate (~$636/mo).

### Added (Phase 3.8 — Write-throughput regression gates)
- **`benchmarks/write_regression.py`** — Measures p50/p99 single-insert latency
  and batch throughput; compares against `benchmarks/baseline/write_regression_baseline.json`.
  Exit 1 if p99 grows > 15% or throughput drops > 10%.
- **`.github/workflows/write-regression.yml`** — Runs on every PR touching `crates/`.
  Builds release binary, starts node, runs benchmark, posts a warning comment on
  regression. Does not block merge (`continue-on-error: true`).
- **`benchmarks/baseline/write_regression_baseline.json`** — Seed baseline
  (p99 = 8 ms, throughput = 3 000 rps). Update via `--save-baseline` after
  deliberate perf improvements.

### Added (Phase 3.12 — Batch insert per-item idempotency)
- **Per-item `request_ids`** in `POST /v1/vectors/batch_insert` — each slot in
  the batch may carry an optional 32-hex idempotency key. A duplicate key is
  detected server-side (O(1) in-memory `FxHashMap`) and the previously assigned
  record ID is returned instead of creating a new record.
- **Mixed batches supported** — deduped and new items may be interleaved at
  arbitrary positions; the response `ids` array preserves original order.
- **Capacity guard accounts for deduped items** — a fully-deduped batch never
  trips the capacity limit.
- **Python SDK** — `insert_batch()` on all four client classes gains
  `request_ids: Optional[List[Optional[str]]] = None`.
- **4 new integration tests** in `tests/api_batch_idempotency.rs`.

### Changed (Phase 3.11 — Concurrent reads via RwLock engine)
- `SharedEngine` type changed from `Arc<Mutex<Engine>>` to `Arc<RwLock<Engine>>`;
  18+ read-only HTTP handlers now acquire a shared read lock, allowing concurrent
  search, proof, health, and timeline requests without serializing behind a global
  write lock. Write handlers (insert, delete, restore, crypto-shred, etc.) retain
  the exclusive write lock.
- `main.rs` auto-snapshot task uses `.read().await` (snapshot is read-only).
- Replication hash-checker and start-offset reads use `.read().await`.

### Added (Phase 3.6 — Crypto-shredding / GDPR erasure)
- **AES-256-GCM per-record encryption** — `POST /v1/records/encrypted` encrypts
  a binary payload before storing; the vector slot is zeroed (not searchable).
  Returns `{"id": int, "key_id": str}`. Group multiple records under one
  `key_id` to shred them atomically.
- **Cryptographic key destruction** — `DELETE /v1/crypto/shred/:key_id` destroys
  the DEK; all records encrypted under that key become permanently unrecoverable
  (GDPR Article 17 "right to erasure" via key destruction, not log truncation).
- **Key existence check** — `GET /v1/crypto/status/:key_id` returns
  `{"exists": bool}`.
- **`VALORI_SHRED_LOG_PATH`** — optional env var; shredded key_ids are appended
  to this file so they remain unrecoverable across restarts.
- **Python SDK** — `insert_encrypted()`, `shred_key()`, `shred_key_status()`
  added to both `SyncRemoteClient` and `AsyncRemoteClient`.
- **Kernel invariants** — `FLAG_ENCRYPTED` (0x02) and `FLAG_SHREDDED` (0x04)
  now fully implemented; `is_searchable()` added to `Record`; shredded records
  are excluded from search, iteration, and index rebuild.
- **Audit chain preserved** — encrypted/shredded record slots remain in the
  BLAKE3 hash chain; the flags byte proves shredding happened without exposing
  plaintext.
- **5 new integration tests** in `tests/api_crypto_shred.rs`.

### Added (Phase 3.7 — `valori import` — provable migrations)
- **`valori import qdrant`** — imports from a Qdrant collection via the scroll
  API. Detects source dimension automatically and aborts with a clear error if
  it mismatches the Valori node's `VALORI_DIM`. Cursor-based pagination;
  per-record idempotency keys ensure exactly-once delivery even on retry.
  Supports `--resume` via a `.valori-import-qdrant-<collection>.json` sidecar
  (tracks `last_offset` + import count across interruptions). Progress bar via
  `indicatif`; state hash printed on completion.
- **`valori import jsonl`** — imports from a JSONL file
  (`{"vector": [...], "metadata": "...", "tag": 0}` per line). Accepts aliases
  `embedding`/`values` for the vector field and `text`/`content`/`payload` for
  metadata. Skips malformed or wrong-dimension lines with a warning; does not
  abort the whole import.
- **Dim validation before any data write** — both subcommands call
  `GET /health` and compare the node's declared `dim` to the source before
  touching any data.
- **Auto-create target collection** — if the target collection doesn't exist,
  it is created before the first insert (idempotent; `400 Already Exists` is
  swallowed).
- **No new dependencies** — uses `ureq` + `indicatif` + `chrono` already in
  `valori-cli`'s dep tree.

### Added (Phase 3.5 — Per-tenant API Keys + RBAC)
- **`POST /v1/keys`** — create a scoped API key (`read_only`, `read_write`, or
  `admin`). Returns the plain-text token once; thereafter only the BLAKE3 hash
  is stored. Accepts optional `collection` lock and `description`.
- **`GET /v1/keys`** — list all keys (masked — `prefix` + metadata, no raw token).
  Requires `admin` scope.
- **`DELETE /v1/keys/:id`** — revoke a key. Audit-safe: key is removed from the
  store immediately; the `events.log` is not affected.
- **`VALORI_KEYS_PATH`** — new env var (JSON file); key store survives restarts
  when set. Absent = in-memory only.
- **`VALORI_AUTH_TOKEN` legacy fallback** — existing static tokens continue to
  work; the new key store is checked first, then the static token as a fallback
  (treated as admin scope).
- **`build_router_with_keys()`** / **`build_cluster_router_with_keys()`** — new
  router builders used by `main.rs`; existing `build_router()` unchanged
  (in-memory key store, no breaking change for tests).
- **Scope enforcement at middleware layer** — routes auto-classified as
  read-only, read-write, or admin by path + method without per-handler changes.
- **8 new integration tests** in `crates/valori-node/tests/api_keys.rs`.

### Added (Phase 3.3 — Cluster-aware Python SDK)
- **`ClusterClient`** — new sync multi-node client. Takes a list of node URLs;
  routes writes to the discovered leader, round-robins local reads across all
  replicas, and upgrades to linearizable reads on request. Leader is discovered
  from the first 307 redirect and cached; failover resets the cache and
  self-heals on the next call.
- **`AsyncClusterClient`** — async mirror backed by `AsyncRemoteClient`.
  `cluster_health()` fans out with `asyncio.gather`. `close()` shuts down all
  underlying httpx clients.
- **`SyncRemoteClient.insert()`** — now auto-generates a UUID4 idempotency key
  and sends it as `"request_id": [u8; 16]` in the JSON body on every call.
  The key is identical across all retry attempts, enabling server-side dedup
  when a write was applied before a connection reset. Pass `idempotency_key=`
  to supply your own token.
- **`SyncRemoteClient.delete()` / `soft_delete()`** — same idempotency key
  handling.
- **`SyncRemoteClient.leader_url()`** — expose the cached leader base URL.
- **`SyncRemoteClient.get_cluster_role()`** / **`AsyncRemoteClient.get_cluster_role()`**
  — `GET /v1/cluster/role` → `"leader"` | `"follower"`.
- **`AsyncRemoteClient.timeline()`** — replaced `aiohttp` with the existing
  `httpx.AsyncClient` (`self.client`); eliminates the mixed-client inconsistency.
- `ClusterClient` and `AsyncClusterClient` exported from `valoricore` package.

### Added (Phase 3.4 — As-of / Point-in-Time Reads)
- **`POST /search`** — new optional fields `as_of` (ISO 8601 UTC string) and
  `as_of_log_index` (u64). When either is set the server replays committed
  events up to the target, searches the resulting state, and returns
  `as_of_log_index`, `as_of_timestamp_iso`, and `as_of_state_hash` (BLAKE3
  hex) alongside the hit list. Requires `VALORI_EVENT_LOG_PATH`.
- **`GET /v1/timeline`** — upgraded from a raw string list to structured JSON
  (`TimelineResponse`). Accepts `from=<ISO8601>` and `to=<ISO8601>` query
  params for timestamp range filtering. Each entry includes `log_index`,
  `timestamp_unix`, `timestamp_iso`, `event_type`, and per-event IDs.
- **`EventJournal`** — now stamps each committed event with a wall-clock
  unix-second timestamp. New methods: `committed_with_timestamps()`,
  `find_log_index_at_or_before()`, `event_timestamp()`.
- **Python SDK** — `SyncRemoteClient.search()` and `AsyncRemoteClient.search()`
  gain `as_of` and `as_of_log_index` params. New `timeline()` method on both.
- **6 new integration tests** in `crates/valori-node/tests/api_as_of.rs`.

### Added (Phase 2.10d — Partition Harness)
- **`crates/valori-consensus/tests/partition_scenarios.rs`** — three new
  integration tests for the in-process partition harness:
  - `asymmetric_partition_lagging_node_catches_up` — one-directional link block
    (leader → follower); 2/3 quorum commits; lagging node catches up and all
    three BLAKE3 hashes converge.
  - `blake3_chain_consistent_across_partition_and_heal` — full compliance proof:
    isolated-leader's hash is frozen during a symmetric partition, and after heal
    all 3 replicas share the same BLAKE3 state hash over all 6 records.
  - `isolated_node_hash_frozen_then_converges` — confirms an isolated follower
    cannot fork the audit chain; hash is frozen during isolation and adopts the
    majority chain after heal.
- All 3 new tests pass (0.73 s); full `valori-consensus` suite clean.

### Added (C3 — Self-Maintaining Memory)
- **Global entity registry** (`ui/src/app/api/ingest/route.ts`) — before creating a
  Concept node, checks `entity:<collection>:<normalized_label>` in the metadata sidecar.
  Existing nodes are reused across documents and ingest sessions so the same real-world
  entity converges to a single graph node.
- **Content dedup** — per-chunk SHA-256 computed before embedding. Exact duplicates
  (`content:<collection>:<sha>` already registered) skip the vector insert entirely.
  `dedup_skipped` count returned in ingest response; `dedup: true` flag per chunk.
  `content_sha256` stored in sidecar for external verification.
- **Contradiction detection** — after each ingest, `detectContradictions()` runs
  async (fire-and-forget). Similarity > 0.92 with a different source document queues
  a `contradiction:<id>` entry with `status: "pending"`.
- **`GET /api/contradictions`** — lists pending/dismissed/superseded contradictions
  for a collection with chunk text preview.
- **`POST /api/contradictions`** — resolve: `dismiss` (both valid) or `supersede_b`
  (marks `record_b` sidecar as `superseded: true`).
- **Supersession filter in `/api/why`** — chunks with `metadata.superseded === true`
  are excluded from vector search results. Kernel record is immutable (audit trail
  preserved); only retrieval is suppressed.

### Added (C2 — Audited Entity Graph + Provenance Receipt)
- **`GET /graph/subgraph?root=<id>&depth=<d>`** — bounded BFS (depth capped at 4)
  returning all reachable nodes and edges. Added to both `server.rs` (standalone)
  and `cluster_server.rs` (cluster, respects readiness gate).
- **Entity extraction at ingest** (`ui/src/app/api/ingest/route.ts`) — when
  contextual enrichment is enabled, extracts up to 8 named entities per chunk via
  the configured LLM. Creates `NodeKind::Concept` nodes + `EdgeKind::Mentions`
  edges (chunk → concept), deduplicated within the ingest session via a
  `entityNodeMap`. Entity labels are stored in the metadata sidecar.
- **Provenance subgraph in receipt** (`ui/src/app/api/why/route.ts`) — after
  graph expansion, calls `/graph/subgraph?depth=1` for each top-5 chunk node and
  collects traversed nodes + edges. Entity labels fetched for Concept nodes.
- **Receipt schema** (`ui/src/lib/receipts.ts`) — `ReceiptGraphNode` and
  `ReceiptGraphEdge` interfaces added. `ServerReceiptPart` and `AnswerReceipt`
  gain `provenance_nodes` and `provenance_edges` arrays.
- **Bug fix**: `Document→Chunk` edge kind corrected from `0` (Relation) to `6`
  (ParentOf) in the ingest route.

### Added (C1 — Contextual Retrieval + Audited Enrichment)
- **Audited context sentences** — `BatchInsertRequest` now accepts
  `metadata: Option<Vec<Option<String>>>`. Per-vector UTF-8 metadata blobs are
  committed into `KernelEvent::InsertRecord.metadata` / `AutoInsertRecord.metadata`,
  included in the BLAKE3 audit chain, and replicated through Raft. The cluster ingest
  path (`cluster_server.rs`) previously always passed `metadata: None` — fixed.
- **Contextual enrichment at ingest** (`ui/src/app/api/ingest/route.ts`) — when
  enabled, generates a one-sentence LLM context per chunk before embedding and
  commits it as `{"doc","n","total","ctx"}` JSON in the audited metadata field.
  Concurrency limit: 6 parallel LLM calls via `Promise.allSettled`. Failure is
  graceful (ingest continues without enrichment, `enriched: false` in receipt).
- **Tier-2 reranker** (`ui/src/app/api/why/route.ts`) — optional cross-encoder
  reranker (Cohere or custom endpoint) applied after vector search. Failure is
  silent. `rerank_score: number | null` per chunk + `reranked: boolean` flag are
  written into the proof receipt so non-determinism is documented, not hidden.
- **Receipt schema** (`ui/src/lib/receipts.ts`) — `ReceiptChunkRef` gains
  `rerank_score: number | null` and `enriched: boolean`. Both additive, no version
  bump needed within `"1.0"`.
- **Settings → Tier-2 Reranker** (`ui/src/app/settings/page.tsx`) — Disabled /
  Cohere / Custom endpoint toggle persisted in `localStorage["valori:reranker_config"]`.
- **DocumentUploadTab** (`ui/src/components/ingestion/DocumentUploadTab.tsx`) — adds
  per-upload contextual enrichment toggle that passes LLM params to the ingest route.
- **AskTab** (`ui/src/components/collections/AskTab.tsx`) — loads reranker config
  from localStorage and passes it to `/api/why` on each question.

### Added (C0 — Eval Harness)
- **`scripts/eval/eval.py`** — Python eval harness with three subcommands: `probe`
  (health check, no embedding needed), `seed-eval` (seeds 10 records, embeds,
  searches, measures recall@k + provenance integrity; CI gate exits 1 if
  recall@1 < 0.8 or citation_existence < 1.0), `verify` (verifies
  `content_sha256` in saved receipt JSON files against a live node).
- **`scripts/eval/qa_sets/bootstrap.jsonl`** — 10 bootstrap QA entries labeled
  `[bootstrap]`. Not for external claims; replaced with real corpus when available.
- **`ui/src/lib/receipts.ts`** — receipt schema frozen at `version: "1.0"`.
  Breaking changes must bump `RECEIPT_VERSION`.
- **`docs/phases/phase-C0-cortex-plan.md`** — full converged Cortex plan (5
  contradiction cycles, 34 items, 4-point moat statement).

### Fixed (B13 — Startup Readiness Gate)
- **Partial-state-on-restart bug fixed** (`valori-node`) — cluster nodes no longer
  serve `Local`-consistency reads during the openraft log-replay catch-up window that
  follows a restart. Reads now return HTTP 503 (`Retry-After: 1`) until the node has
  replayed all entries committed before shutdown.
- **`ReadinessGate`** added to `cluster_server.rs` — atomic latch initialized from
  `startup_committed_index` (read from the redb `KEY_COMMITTED` entry before Raft
  opens). Latch opens permanently once `applied_index >= startup_committed_index`;
  fresh/in-memory nodes get `target=0` and are immediately ready.
- **Explicit snapshot cadence** (`cluster.rs`) — `SnapshotPolicy::LogsSinceLast(n)`
  now explicitly configured (default 5000, overridable via
  `VALORI_SNAPSHOT_EVERY_EVENTS`) instead of relying on openraft's implicit default,
  bounding the maximum catch-up window after restart.

### Added (B13 — env vars)
- `VALORI_SNAPSHOT_EVERY_EVENTS` — trigger a Raft snapshot every N applied entries
  (default 5000). Lower values reduce restart catch-up latency at the cost of more
  frequent snapshot I/O.
- `VALORI_RAFT_SNAPSHOT_KEEP` — log entries to retain after snapshot for followers
  that are slightly behind (default 1000).

### Added (Phase 3.2 — Rolling Upgrades)
- **`schema_version` field on `ClientRequest`** (`valori-consensus`) — the
  leader stamps `CURRENT_SCHEMA_VERSION` (currently `0`) on every proposal. Old
  nodes decode the field as `0` via `#[serde(default)]`.
- **`CURRENT_SCHEMA_VERSION: u8 = 0`** constant (`valori-consensus::types`) —
  single source of truth for the cluster wire version. Bump when a new
  `KernelEvent` variant or breaking field change requires newer followers.
- **Schema version gate in `ValoriStateMachine::apply()`** — followers reject
  entries with `schema_version > CURRENT_SCHEMA_VERSION` with `StorageError`
  (halts replication on that node; cluster continues through remaining quorum).
  State and audit log are untouched on rejection.
- **`valori cluster upgrade --url … --target-version …`** CLI command — interactive
  guided rolling upgrade: discovers topology, upgrades non-leaders first then
  leader, polls `/health` after each restart, waits for re-election before
  declaring the leader step complete.
- **`docs/COMPATIBILITY.md`** — schema version history, rolling-window rules,
  coexistence matrix, and the procedure for bumping `CURRENT_SCHEMA_VERSION`.

### Fixed (Phase 3.2)
- `corrupted_snapshot_payload_is_refused_and_state_kept` snapshot corruption
  test was flipping byte `bytes.len() / 2` which, for V6 snapshots (8318 bytes),
  lands in the namespace sentinel region not covered by `hash_state_blake3`.
  Fixed to corrupt `bytes.last_mut()` (last byte of the `state_hash` tail),
  which always triggers the hash mismatch check regardless of format version.

---

## [0.2.1] — 2026-06-19

### Added
- **Multi-tenant collections** — up to 1 024 named namespaces per node.
  `POST /v1/namespaces`, `GET /v1/namespaces`, `DELETE /v1/namespaces/:name`.
  All data endpoints accept an optional `"collection"` field. Records are
  isolated at the kernel level via intrusive per-namespace linked lists enforced
  at three independent points (event-commit, WAL replay, `build_index`).
- **`AutoCreateNode` / `AutoCreateEdge` kernel events** — graph mutations with
  IDs assigned at apply time for deterministic cluster-mode graph operations.
- **Persistent Raft state machine** — when `VALORI_RAFT_LOG_PATH` is set, the
  state machine shares the redb file and persists `last_applied`, membership,
  and the latest snapshot, preventing duplicate audit-log writes on restart.
- **Replay suppression** — `replay_until` suppresses already-written audit
  entries when openraft replays committed log entries after a restart.
- **`GET /v1/cluster/role`** — current node's Raft role for load-balancer routing.
- **`state_hash_match` Prometheus gauge** — cluster-wide hash-convergence metric.
- **Snapshot V6 format** — per-record `namespace_id` + linked-list pointers,
  2 × 1 024 × 4 = 8 KB namespace heads arrays, and a backward-compatible NSRG
  section (namespace registry as JSON, detected by `"NSRG"` magic tag).
- **Python SDK collection API** — `create_collection`, `list_collections`,
  `drop_collection` on both `SyncRemoteClient` and `AsyncRemoteClient`;
  `collection` parameter on all data methods; `consistency` parameter on search.
- **Threat model** (`docs/THREAT_MODEL.md`).
- **Capacity planning** (`docs/CAPACITY.md`).
- **DR & multi-region runbook** (`docs/DR.md`).
- **Multi-arch hash benchmark** (`benchmarks/multi_arch_hash.py`).
- **Q16.16 precision benchmark** (`benchmarks/q16_precision.py`).
- **Helm snapshot CronJob** (`deploy/helm/valori/templates/snapshot-cronjob.yaml`).
- **CI test-count workflow** (`.github/workflows/test-count.yml`).

### Fixed
- `LeaderClient::get_proof()` wire-format mismatch — server returns
  `{"final_state_hash":"<hex>"}` but client expected `[u8; 32]`. Added
  `LeaderProof { final_state_hash: String }` and updated hex comparison in replication.
- Snapshot buffer too small for V6 in `format.rs` and `snapshot_roundtrip.rs`
  (4 KB → 16 KB).
- `spawn_state_hash_watcher` held `Arc<Database>` indefinitely, blocking redb
  file re-open on restart. Now returns `JoinHandle`, stored in `ClusterHandle`,
  aborted and awaited before shutdown.
- arXiv paper title corrected from *"Deterministic Memory: A Substrate for
  Verifiable AI Agents"* to *"Valori: A Deterministic Memory Substrate for
  AI Systems"* in README and BibTeX.
- Hardcoded test count badge (271) replaced with CI-driven workflow badge.
- Python SDK version badge corrected from v0.1.11 to v0.2.1.
- Apply-vs-audit ordering invariant now explicitly documented with crash-window
  analysis in `valori-consensus/README.md`.
- Comparison table "No" cells now cite competitor documentation.

### `valori_raft_state_hash_match` Prometheus gauge — a background task on
  each cluster node periodically calls `/v1/proof/state` on every peer and
  publishes `1` when all reachable nodes agree on the BLAKE3 state hash, `0`
  when any peer diverges. Mismatches are also logged at `ERROR` level and
  counted by `valori_raft_divergence_detections_total`. Configurable via
  `VALORI_STATE_HASH_CHECK_SECS` (default 30 s; `0` disables).
- **`GET /v1/cluster/role`** endpoint — returns `{"role":"leader"|"follower",
  "node_id":N,"current_leader":N}` on any node. Designed for load-balancer
  health-check routing: steer writes at the pod that answers `"leader"` to
  avoid 307 redirect round trips on every write.
- **Proptest event-sequence fuzz** (`crates/valori-consensus/tests/proptest_event_fuzz.rs`)
  — 32 randomly generated insert/soft-delete/delete sequences applied through
  a 3-node in-process cluster, asserting all nodes converge to the same BLAKE3
  state hash after each sequence. Shrinks failing cases automatically.
- **Helm chart** (`deploy/helm/valori/`) — production StatefulSet with
  PersistentVolumeClaims for `events.log` and `raft.redb`, headless service
  for stable pod DNS, client service, and configurable liveness/readiness
  probes pointing at `/v1/cluster/health` and `/health`. Topology spread
  anti-affinity keeps pods on separate availability zones by default.

- **Automatic `events.log` rotation** on both write paths — the standalone
  `EventCommitter` and the cluster `EventLogAuditSink` seal the live segment to
  `events.log.NNNNNN` once it passes `VALORI_EVENT_LOG_ROTATION_BYTES` (default
  256 MiB; `0` disables), opening a fresh segment that splices from the sealed
  one's chain head.
- **Multi-segment recovery** — recovery now discovers and replays every local
  segment (sealed archives + live file) in sequence order and verifies each
  splice point.

- **Linearizable reads via the read-index protocol** (now the default read
  consistency). The leader serves through openraft's `ensure_linearizable()`;
  a follower fetches the leader's read index from the new
  `GET /v1/cluster/read-index` endpoint, then waits for its own apply to catch
  up before scanning local state. Clients can opt into a faster,
  eventually-consistent read with `consistency: "local"` (Python SDK:
  `search(..., consistency="local")`).

### Fixed
- Rotated logs previously recovered **only the live segment**, silently dropping
  all pre-rotation history; recovery is now multi-segment and lossless.
- Archive segments are named by monotonic segment sequence instead of a
  wall-clock timestamp, so two rotations within the same second no longer
  collide and clobber an earlier archive.

## [0.2.0] — 2026-06-13

The multi-node release. Valori graduates from a single standalone node to a
Raft-replicated cluster with verifiable, crash-symmetric state on every replica.

### Added
- **Raft consensus layer** (`valori-consensus`) over openraft 0.9: replicated
  log store (in-memory + persistent `redb`), `KernelState` state machine with
  the audit-log write at apply time, and a tonic/gRPC peer transport.
- **Cluster mode** for `valori-node`: boot-time dispatch on
  `VALORI_CLUSTER_MEMBERS`, leader-redirect (`307 + Location`) for writes,
  local reads on any replica, and a `/v1/cluster/*` management plane
  (status, health, add-node, remove-node).
- **Mutual TLS** on the Raft channel (`VALORI_TLS_*`), enforced at the
  handshake against a shared cluster CA.
- **Persistent Raft log** via embedded `redb` (`VALORI_RAFT_LOG_PATH`) — the
  log and vote survive process restarts.
- **Raft metrics** exported on `/metrics` (term, leader, log/apply lag,
  snapshot/purge indexes).
- **State-machine ID allocation** (`KernelEvent::AutoInsertRecord`): record IDs
  are assigned deterministically at apply time, removing the per-node insert
  mutex and retry loop.
- **Cluster data-plane endpoints**: `/v1/delete`, `/v1/soft-delete`,
  `/v1/vectors/batch_insert`, `/v1/proof/state`.
- **Interactive setup wizard** (`valori setup`): pick architecture and node
  count, start an in-process cluster, and drive inserts/search/membership from
  a live menu. Projects persist to `~/.valori/projects.json`.
- **`valori cluster` CLI**: operate a running cluster (status, health,
  add-node, remove-node) against any node's HTTP API.
- **Docker deployment**: distroless multi-stage `Dockerfile` with a built-in
  `--health-check` TCP probe, and a 3-node `docker-compose.yml`.
- **Partition harness**: in-memory switchable-transport test suite covering
  leader isolation, re-election, partition heal/convergence, and the
  minority-cannot-commit invariant.

### Changed
- Cluster search now uses the kernel's maintained index via `search_l2`
  instead of an ad-hoc record-pool scan.
- Workspace versioning unified at `0.2.0` via `[workspace.package]`; all crates
  inherit version, edition, and license.

### Fixed
- `Dockerfile` now copies all workspace member manifests so workspace
  resolution succeeds; healthcheck no longer references a non-existent flag.

### Repository
- Removed scratch and stale top-level files; relocated manual/e2e/benchmark
  scripts under `scripts/`.
- Tightened `.gitignore` for runtime database directories and caches.

[Unreleased]: https://github.com/valori-db/valori-kernel/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/valori-db/valori-kernel/releases/tag/v0.2.0
