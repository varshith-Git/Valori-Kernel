# TypeScript Contract Quality — Valori API v1

**Phase API-3.3, §13.** Every `unknown`, `any`, and `never` in
`ui/api-types/src/valori-v1.ts` — generated from `api/openapi/valori-v1.yaml`
by `openapi-typescript` — classified as **EXPECTED** or **BUG**.

The goal is not literally zero `unknown` in the file. `openapi-typescript`
emits structural boilerplate that has nothing to do with our contract, and a
genuinely empty response body *must* be `never` — saying otherwise would be a
lie in the other direction. The goal is **zero unexpected** occurrences in
request/response semantics.

Regenerate with `./scripts/generate-api-types.sh`; the counts below are
re-derived on every `./scripts/api-contract-gate.sh` run and printed under
`GENERATED CLIENT QUALITY`.

## Summary

| Marker | Count | Verdict |
|---|---:|---|
| `unknown` — response-headers index signature | 332 | EXPECTED |
| `unknown` — prose inside doc comments | 6 | EXPECTED (not code) |
| `unknown` — in a request or response type | **0** | — |
| `any` (any form) | **0** | — |
| `content?: never` | 2 | EXPECTED |
| `requestBody?: never` | 37 | EXPECTED |
| `query?: never` | 132 | EXPECTED |
| `parameters?: never` | 0 | — |
| Schemas whose only member is an index signature | **0** of 157 | — |

**Zero BUG-class occurrences.**

## Classification

### `unknown` × 332 — EXPECTED

Every one is `[name: string]: unknown` inside a response's `headers` block:

```ts
200: {
    headers: {
        [name: string]: unknown;   // ← all 332 are this
    };
    content: { "application/json": components["schemas"]["SearchResponse"]; };
};
```

This is how `openapi-typescript` models "arbitrary HTTP response headers may be
present". It is emitted for every response of every operation regardless of the
contract, describes headers rather than bodies, and cannot be removed by
anything we write in the OpenAPI document. It never reaches a body type.

### `unknown` × 6 — EXPECTED (prose, not code)

The literal word appears inside JSDoc text carried over from Rust doc comments —
for example `"Bad base64 payload, bad key_id, or unknown collection"` and the
`IndexBuildParameters` note explaining that unknown keys are ignored. These are
comment characters, not types.

### `any` × 0

No occurrence in any form. The generator does not emit `any`, and no schema
forces it.

### `content?: never` × 2 — EXPECTED

Both are responses whose handler genuinely sends no bytes:

| Operation | Status | Why |
|---|---|---|
| `DELETE /v1/namespaces/{name}` | 204 | `drop_collection` returns `StatusCode::NO_CONTENT`; RFC 9110 §15.3.5 forbids a body on 204. |
| `POST /v1/snapshot/upload` | 200 | `restore` is `async fn(..) -> Result<(), EngineError>`; the success arm is the unit type. |

Both are recorded in the `EMPTY_BODY_OK` allowlist in
`scripts/verify-api-route-contract.py` and in `EMPTY_SUCCESS_OK` in
`scripts/audit-public-api-operations.py`. Anything *not* on those lists that
declares an empty body fails the gate.

This count was higher before this phase. Sixteen error responses declared no
body while their handlers sent JSON — see the phase doc for the root cause.

### `requestBody?: never` × 37 — EXPECTED

74 public operations, 37 of which accept a body; the other 37 do not. The audit
verifies this in **both** directions against the Rust handler signature: an
operation gets `requestBody?: never` only if its handler has no `Json<..>`,
`Form<..>`, or `Bytes` extractor. A handler that takes a body while the contract
says `never` is reported as `[request]` and fails the gate.

### `query?: never` × 132 — EXPECTED

`openapi-typescript` emits a `parameters` block at both the **path** level and
the **operation** level, so the count spans two populations:

```
 71  path-level blocks   (one per path; 71 paths)
+61  operation-level     (74 operations − 13 that declare query parameters)
────
132
```

Exactly reconciled. As with `requestBody`, the audit cross-checks each against
the handler's `Query<..>` extractor in both directions, so a real query
parameter cannot hide behind a `never`.

### Untyped bag schemas × 0 of 157

No schema in the document consists solely of an index signature. Every one of
the 157 carries at least one named, typed field — so no SDK user is ever handed
a `Record<string, unknown>` where a real type was possible.

Three schemas *do* carry an open index signature **alongside** their named
fields, which is correct and deliberate:

| Location | Why open |
|---|---|
| `MetadataSetRequest.metadata` | Arbitrary caller metadata. Valori stores it verbatim and never interprets it. |
| `GraphRagHit.metadata` | Echo of the same caller-supplied metadata. |
| `OperationDetailResponse.proof` | Either a full `Receipt` or a reduced stand-in when no receipt was assembled. Documenting one schema would claim a shape that only sometimes holds. |

These render as `Record<string, unknown>` / `Dict[str, Any]` — which is the
honest type for "arbitrary JSON", and materially better than the bare
property-less `object` they produced before this phase.

## How this is enforced

`./scripts/api-contract-gate.sh` recomputes all of it per run:

```
 GENERATED CLIENT QUALITY (discovered):
   Unexpected unknown/any:      0
   Untyped bag schemas:         0
   content?: never:             2 (expected: genuinely empty bodies)
   requestBody?: never:         37 (expected: operations taking no body)
```

A non-zero value in either of the first two rows adds an SDK blocker and fails
the gate. The numbers are discovered from the generated file, never asserted
here — if this document and the gate disagree, the gate is right.
