# SDK Readiness — Valori API v1

**Status as of Phase API-3.3: READY. 0 blockers.**

The machine-computed verdict lives in `docs/api/sdk-readiness.json`, written by
`scripts/api-contract-gate.sh` from that run's step outcomes. It is never
hand-edited, and this document never overrides it. If the two disagree, the
JSON is right and this file is stale.

## The question

> Can a third-party SDK be generated from `api/openapi/valori-v1.yaml` without
> hidden contract ambiguity?

**Yes.** Route coverage was settled in Phase API-3.2; Phase API-3.3 closed the
operation-completeness gap behind it and validated the result by generating and
inspecting throwaway Python and TypeScript clients.

## What is settled

| Criterion | State |
|---|---|
| Rust public routes == utoipa == OpenAPI | 74 == 74 == 74, computed |
| operationId uniqueness / stability | 74 unique, all `snake_case`, policy documented |
| Non-public boundary | 26 routes, each explicitly classified; leaks fail the gate |
| **Operation completeness** | **74 / 74 complete**, cross-checked against Rust handler signatures |
| **Request coverage** | 37 of 74 take a body; all typed. Verified in both directions against the handler's extractors |
| **Response coverage** | Every operation has a typed success body, or a documented genuinely-empty one |
| **Parameter coverage** | 0 untyped parameters, 0 handler/contract mismatches |
| **Error coverage** | Every `>= 400` response carries `ApiError`, except two deliberate typed health documents |
| Security schemes | `BearerAuth` declared; 73 authenticated, 1 deliberately open |
| `x-required-scope` | Generated from the middleware's own `required_scope`, and **only** for authenticated operations |
| `401`/`403` | Documented on all 73 authenticated ops, with the correct `ApiError` body |
| Closed enums | `ErrorCode`, `Metric`, `MetricInput`, `IndexKind`, `IndexKindInput`, `BuildableIndexKind`, `IngestJobState` |
| Collection requiredness | `name` + `dimension` + `metric`, no `"default"` exception |
| Async lifecycle | `202` → `IngestAcceptedResponse` → poll `IngestJobStatusResponse` / `IngestJobState`; typed end to end |
| **Untyped schema properties** | **0** of 157 schemas |
| **Unexpected `unknown` / `any` / `never`** | **0** — see `typescript-contract-quality.md` |
| 4xx coverage | Every operation except `GET /health`, enforced by the verifier |
| OpenAPI version | 3.1.0 everywhere — generator, lint, codegen |
| OpenAPI determinism | Byte-identical across repeated runs |
| TypeScript determinism | Byte-identical; `tsc --noEmit` clean |
| Redocly lint | Clean, with one precisely-pinned documented exemption |
| Python remote client | Contract tests pass; no implicit `"default"` collection |
| Throwaway Python client | **PASS** — 74 typed methods, zero `Dict[str, Any]` in the operation layer |
| Throwaway TypeScript client | **PASS** — 74 typed methods, compiles under `tsc --strict`, zero `any` in the API surface |

## Blockers

None.

The two Phase API-3.2 blockers are closed. Both had the same root cause, and it
was **contract lag, not a runtime defect**: `attach_error_code` is the outermost
layer on both routers and already rewrote every error response into `ApiError`,
including the bare-status 401/403. The document described a runtime that had not
existed since Phase API-2.

- The 16 bodyless responses are now `ApiError`, filled structurally by
  `ErrorBodyAddon` — the contract-side mirror of the middleware, so the two
  cannot drift again.
- A further **146** responses (401/403 × 73 authenticated operations) were wrong
  in the same way and were invisible to the old verifier, which allowlisted them
  on the same false premise. That allowlist is gone.
- `GET /v1/crypto/status/{key_id}` was the one genuine escape — a `text/plain`
  body, which the middleware passes through by design. Converged onto
  `error_response`.

See `docs/phases/phase-api-contract-3.3-operation-completeness.md`.

## Not blockers

Verified by reading the handlers, and allowlisted in the audit scripts:

- `DELETE /v1/namespaces/{name}` → `204`. Returns `StatusCode::NO_CONTENT`; a
  204 must not carry a body.
- `POST /v1/snapshot/upload` → `200`. The handler is
  `-> Result<(), EngineError>`; the success arm is genuinely empty.
- `POST /v1/storage/snapshots/upload` has no `requestBody`. The handler takes
  only `State` — it consumes nothing.
- `GET /health` and `GET /v1/cluster/health` answer `503` with their full typed
  health document rather than `ApiError`. The status code signals load-balancer
  action; the payload is a status report, not an error. `attach_error_code`
  exempts exactly these two paths so the bytes match the schema. The audit
  reports any third such operation rather than letting it pass.
- Three schemas carry an open `additionalProperties` alongside their named
  fields (`MetadataSetRequest.metadata`, `GraphRagHit.metadata`,
  `OperationDetailResponse.proof`) because those payloads are genuinely
  free-form.

## Gate

```bash
./scripts/api-contract-gate.sh
```

Readiness is computed from the run, not asserted. `SDK READY = YES` requires
every step to pass **and** the blocker list to be empty. The gate now runs nine
stages and prints 13 operation-completeness figures plus 4 generated-client
quality figures, all discovered from the current repository.

## Next

**Phase API-4 — Official SDK Platform** is unblocked.
