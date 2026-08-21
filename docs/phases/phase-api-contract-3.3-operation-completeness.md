# Phase API-3.3 — Public Operation Contract Completeness & SDK Preflight

## Goal

Phase API-3.2 proved the **right** 74 public operations exist (route, method,
`operationId` agreement across Rust, utoipa, and the committed contract). This
phase proves their **HTTP contracts are complete** — that every one can become a
high-quality Python / TypeScript / Go / Java / Rust SDK method without losing
request types, response types, errors, security, enums, parameters, or async
lifecycle semantics. No new routes; no API redesign.

## Delivered

### New tooling

| File | What it does |
|---|---|
| `scripts/audit-public-api-operations.py` | **New.** Audits every public operation for contract completeness, cross-checking the OpenAPI document against the **Rust handler signature** located via the route manifest. Emits `docs/api/public-operation-audit.{json,md}`; exits non-zero on any incomplete operation or untyped schema property. |

Completeness is never inferred from path existence. A `requestBody` is complete
only when the contract declares one *and* the handler has a body extractor; a
`query?: never` is valid only when the handler has no `Query<..>`. Both
directions are checked, so a hole cannot hide behind either side.

The script resolves handlers across the workspace (`/v1/ingest*` is defined in
`valori-ingest`, `/v1/tree/*` in `valori-rag`), follows module qualification
(`crate::ingest::ingest`) and import aliases (`use crate::routes::version as
version_handler`), and skips `self`-receiving trait methods that share a name
with the real handler (`create_node`).

### Contract fixes — error bodies (the two Phase 3.2 blockers)

Both blockers had the **same root cause, and it was a documentation lag, not a
runtime defect**. `crate::error_codes::attach_error_code` is installed as the
**outermost** layer on both routers and rewrites every error response —
handler-built JSON, a bare `StatusCode`, an empty body — into the canonical
`{error, code}` `ApiError`. The ~126 bare `json!({"error": ...})` call sites were
already normalised by it at runtime. The contract simply did not say so.

| Change | Effect |
|---|---|
| `openapi.rs::AuthResponsesAddon` now declares `body = ApiError` on 401/403 | Fixed **146** responses (2 × 73 authenticated operations). They were documented as empty-bodied on the reasoning that "axum renders a bare `StatusCode` with an empty body" — true of the guard alone, false of the router. `tests/api_contract.rs::unauthorized_has_a_parseable_json_body_with_a_code` already proved otherwise and passed. |
| `openapi.rs::ErrorBodyAddon` — **new** `Modify` pass | The contract-side mirror of `attach_error_code`: fills `ApiError` into any `>= 400` response left bodyless, never overriding one already declared. Fixed the remaining 16, including `/v1/tree/*` and `/v1/ingest/document`, which are annotated in `valori-rag` / `valori-ingest` and **cannot name `ApiError`** — it is declared in `valori-node`, so hand-editing those call sites was not merely lossy, it was impossible. |
| `server.rs::crypto_status_handler` converged onto `error_response` | Was `Result<_, (StatusCode, String)>` — a `text/plain` body, which `attach_error_code` deliberately passes through untouched. It was the one error in the entire public surface escaping `ApiError`, and it forked from its cluster twin, which already answered in JSON. |
| `scripts/verify-api-route-contract.py` — removed the blanket `401`/`403` empty-body exemption | The exemption rested on the same wrong premise and would have masked a regression. Both statuses are now checked like every other. |

### Contract fixes — untyped bodies and schemas

Found by the new audit, each traced to a concrete Rust type that already existed:

| Was | Now | Why it mattered |
|---|---|---|
| `GET /v1/proof/receipt{,/{id}}` → `body = Object` | `ReceiptDto` + `ReceiptFragmentDto` (16 + 5 fields) | The receipt is the product's flagship proof artifact and reached every SDK as an opaque blob. The handler serialises a fully concrete `valori_effect::Receipt`. |
| `GET /v1/ingest/status/{job_id}` → `body = Object` | `IngestJobStatusResponse` + `IngestJobState` enum | §11 async lifecycle. A client polling an async ingest had no typed way to learn whether the job finished. `IngestJobState` is the real closed set both routers write: `processing` / `completed` / `failed`. |
| `IndexBuildRequest.parameters` — schema with no `type` at all | `IndexBuildParameters` (`m`, `ef_construction`, `ef_search`, `n_list`, `n_probe`) | The only genuinely untyped field in the surface — `unknown` in TS, `Any` in Python. Both routers read exactly these five `u64` keys; the knobs were undiscoverable. |
| `IndexBuildRequest.type` — bare `string` | `BuildableIndexKind` enum (`hnsw`/`ivf`/`bq`) | §8/§10 closed enums. The build task matches on exactly these three; its `_` arm errors. Deliberately narrower than project-wide `IndexKindInput` — `brute` and `auto` are not buildable per-collection ANN structures. |
| `SubgraphResponse.{nodes,edges}` — `Vec<Object>` | `SubgraphNode` / `SubgraphEdge` | Keys are `id`/`record`/`from`/`to`, **not** the `node_id`/`record_id` of `NodeInfo` — genuinely different shapes that must not be conflated. |
| `GraphRagResponse.hits` — `Vec<Object>` | `GraphRagHit` (10 fields) | GraphRAG is a headline retrieval feature returning `object[]`. Nullable `score`/`vector_score` are correct: a graph-only hit has no vector distance. |
| `OperationDetailResponse.{overview,results,metrics}`, `OperationSummary.details` | `OperationOverview`, `OperationResults`, `OperationMetrics`, `OperationDetails` | §11 typed list/detail responses. |
| `/v1/snapshot/{download,upload}` — `body = Vec<u8>` | `SnapshotBytes` (`type: string, format: binary`) | utoipa rendered `Vec<u8>` literally as `array<integer>`; the Python client typed the download as `list[int]`, so restoring a multi-megabyte snapshot meant a Python integer list. Now `File` / `Blob`. |
| `MetadataSetRequest.metadata`, `OperationDetailResponse.proof`, `GraphRagHit.metadata`, PATCH metadata body — bare `type: object` | `additionalProperties` via `HashMap<String, Object>` | Genuinely free-form, but now renders as `Record<string, unknown>` / `Dict[str, Any]` rather than a property-less `object` that says nothing. |

> Note: utoipa 5's `schema(value_type = Object, additional_properties = true)`
> silently ignores the flag. `value_type = std::collections::HashMap<String, Object>`
> is what actually emits `additionalProperties`.

### Contract fix — security

`VendorExtensionAddon` stamped `x-required-scope` on **every** operation,
including `GET /health`, which declares `security: []`. The auth middleware
never runs there and `required_scope` is never consulted, so the value was the
function's default (`read_only`) — telling every SDK that the one deliberately
unauthenticated endpoint required a key. It is now emitted only for
authenticated operations.

### Runtime fix — health is a status report, not an error

`GET /health` and `GET /v1/cluster/health` answer `503` with their **full typed
health document** when a pool is at 100 %. `attach_error_code` mapped 503 →
`Unavailable`, saw a JSON object without `code`, and spliced `error` and `code`
into a documented DTO — so the bytes on the wire did not match the schema the
contract advertised, and a strict SDK would reject its own health probe. Both
paths are now exempt via `STATUS_REPORT_PATHS`. These are the only two `>= 400`
responses in the surface with a typed non-`ApiError` body; the audit reports any
third one rather than letting it pass.

### Gate extension (§17)

`scripts/api-contract-gate.sh` gains step **3b/9 "Public operation
completeness"** and two discovered-number blocks — `OPERATION COMPLETENESS`
(13 figures) and `GENERATED CLIENT QUALITY` (4 figures). No hardcoded counts;
a missing artifact reports `UNKNOWN` and blocks rather than defaulting to zero.

### Tests

| Test | Pins |
|---|---|
| `api_contract.rs::receipt_dto_matches_the_runtime_receipt` | **New.** Serialises a real `valori_effect::Receipt` and the DTO and diffs their key sets, so the hand-written mirror cannot drift. |
| `api_contract.rs::every_operation_documents_the_scope_the_server_enforces` | **Updated.** Invariant is now conditional on `security`: authenticated ⇒ scope matches `required_scope()`; unauthenticated ⇒ **no** scope. |

## Findings

1. **Both Phase 3.2 blockers were contract lag, not runtime bugs.** The
   middleware already guaranteed `ApiError` everywhere. The instruction to
   "converge ~126 call sites onto `error_response()`" would have been
   redundant work; the actual defect was that the contract described a runtime
   that had not existed since Phase API-2. Only one call site
   (`crypto_status_handler`) genuinely escaped, because `text/plain` is passed
   through by design.

2. **The largest defect was invisible to the old gate.** 146 wrong 401/403
   responses dwarfed the 16 the verifier reported — and the verifier could not
   see them, because it *allowlisted* 401/403 on the same false premise. A
   check and the bug it should catch shared a wrong assumption.

3. **`/health` 503 was actively corrupted at runtime.** Not a documentation
   issue: the middleware mutated a typed DTO in flight. Found only by
   cross-checking contract against middleware behaviour.

4. **Two audit iterations were needed before the audit was trustworthy.** The
   first pass produced 13 findings, of which 10 were audit bugs (non-JSON media
   types read as untyped; trait methods shadowing handlers; unresolved
   cross-crate and aliased handlers). A negative control over five mutation
   classes now proves the audit has teeth, and `type: object` with no
   properties — how the receipt slipped through — is now a finding.

5. **`admin` scope on 10 PUBLIC_SDK operations is consistent**, not a §7
   violation: the contract's value is generated from the middleware's own
   `required_scope`, and the updated test diffs the two per operation.

6. **Tooling friction, worked around, not papered over.** `openapi-generator-cli`
   needs a JRE (absent); `@hey-api/openapi-ts` crashes on Node 25. Validation
   used `openapi-python-client` 0.26.2 and `swagger-typescript-api` 13 instead.

## Validation

All commands run from a clean tree at phase end.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo build --workspace` | clean |
| `cargo clippy --workspace --all-targets --all-features` | **0 errors.** 10 pre-existing warnings, all in `valori-engine/src/engine.rs` and `valori-node/tests/e2e_recovery.rs` — files this phase did not touch. |
| `cargo build -p valori-kernel --target wasm32-unknown-unknown` | clean (`no_std` invariant intact) |
| `cargo test -p valori-node` | **449 passed, 0 failed** |
| `cargo test -p valori-node --features utoipa` (`api_contract`, `openapi_generated`) | 27 + 4 passed |
| `cargo test -p valori-kernel` | 83 passed |
| `cargo test -p valori-engine` | 18 passed |
| `cargo test -p valori-state` | 24 passed, 1 ignored |
| `cargo test -p valori-storage` | 78 passed, 1 ignored |
| `cargo test -p valori-consensus` | 32 passed |
| `python3 -m pytest python/tests/` | 101 passed, 8 skipped |
| `npx @redocly/cli lint` | valid, 1 explicitly-ignored problem |
| `cd ui && npx tsc --noEmit` | clean |
| `python3 scripts/verify-api-route-contract.py` | PASS |
| `python3 scripts/audit-public-api-operations.py` | PASS — 74/74 complete |
| `./scripts/api-contract-gate.sh` | **PASS, exit 0, 0 blockers** |

### Operation completeness (discovered)

```
   Public operations:           74      Untyped parameters:          0
   Complete:                    74      Parameter mismatches:        0
   Incomplete:                   0      Errors with no body:         0
   Complete requests:           37      Errors not ApiError:         2  (deliberate)
   Incomplete requests:          0      Security mismatches:         0
   Complete responses:          74      Untyped schema properties:   0
   Incomplete responses:         0
```

### Throwaway SDK generation (§15) — both PASS

Generated from the final contract, inspected, then discarded. Nothing published.

**Python** (`openapi-python-client` 0.26.2) — 74 operation modules across 17
domain packages, 180 models. Return-type distribution over all 74:

| Shape | Ops | Verdict |
|---|---:|---|
| `Union[ApiError, <TypedModel>]` | 70 | typed both channels |
| `Optional[HealthResponse]` | 1 | `/health`, unauthenticated, typed 200 **and** 503 |
| `Union[ApiError, str]` | 1 | `/v1/version`, `text/plain` |
| `Union[Any, ApiError]` | 2 | `delete_collection` (204), `upload_snapshot` (empty 200) |

Zero `Dict[str, Any]` in the operation layer. Spot checks:
`search() → Union[ApiError, SearchResponse]`,
`get_latest_receipt() → Union[ApiError, Receipt]`,
`get_ingest_status() → Union[ApiError, IngestJobStatusResponse]`,
`download_snapshot() → Union[ApiError, File]`,
`ingest_document() → Union[ApiError, IngestAcceptedResponse, IngestResponse]`
(the 202 async lifecycle is exposed, so polling is typed end to end).
Enums generated as real enums: `IngestJobState`, `BuildableIndexKind`.
The generator's only warning was `ruff is not in PATH` — environmental.

**TypeScript** (`swagger-typescript-api` 13) — 74 methods, all
`this.request<Typed, ApiError>`. **Compiles clean under `tsc --strict`.**
Zero `any` in the API surface (the 8 in the file are the generator's own
`encodeQueryParam` / `contentFormatters` runtime). Only two methods return
`void` — `deleteCollection` and `uploadSnapshot`, the two genuinely empty
bodies. `uploadSnapshot(data: SnapshotBytes)` and `downloadSnapshot() → Blob`
confirm the binary fix.

## Follow-ups

| Item | Owner |
|---|---|
| **Phase API-4 — Official SDK Platform.** `sdk_ready` is now `true`; this is unblocked. | API-4 |
| `OperationDetailResponse.proof` is an open object because it is either a full `Receipt` or a reduced stand-in. A `oneOf` would be more precise; it needs the runtime to commit to one shape first. | API-4 |
| `GET /v1/proof/event-log` and `GET /v1/timeline` still read shard 0's log only (pre-existing, from the sharding initiative). Not a contract defect — the contract honestly describes what the handlers do. | sharding |
| `ReceiptHash` crosses the wire as a 32-integer array, not the hex string `to_hex()` produces. Documented as-is because that is what the runtime sends; changing it is a wire-breaking change. | future |
| CI does not yet run `scripts/api-contract-gate.sh`. The gate exists and passes; wiring it into `.github/workflows/ci.yml` would make regressions blocking. | API-4 |
