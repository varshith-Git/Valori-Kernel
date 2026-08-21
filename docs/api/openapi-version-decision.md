# OpenAPI Version Decision — Valori API v1

## Standard Decision: OpenAPI 3.1.0

Valori API v1 canonically targets **OpenAPI 3.1.0**.

## Rationale
1. **Direct JSON Schema Alignment**: OpenAPI 3.1.0 aligns 100% with JSON Schema 2020-12, enabling native representation of polymorphic types, tuples, complex nullability (`type: ["string", "null"]`), and `oneOf`/`anyOf` discriminators.
2. **Native Utoipa 5.5 Support**: `utoipa 5.5` generates OpenAPI 3.1.0 documents natively from Rust type derives (`#[derive(utoipa::ToSchema)]`).
3. **Modern SDK Generator Tooling**: Modern client generators (including `openapi-typescript` 7+ and Python/Go/Java OpenAPI 3.1 generators) consume OpenAPI 3.1.0 natively without data loss or awkward `nullable: true` hacks.

## Rejected Alternatives
- **OpenAPI 3.0.3**: Rejected because `utoipa 5.5` derives emit 3.1.0 schema constructs (`type: ["string", "null"]`). Retrofitting 3.0.3 via custom post-processing introduced synthetic schema drift and destroyed Utoipa's native Rust type fidelity.

## Compatibility Matrix
- **Generator**: `utoipa` 5.5 (emits `openapi: 3.1.0`)
- **Linter**: `Redocly CLI` (validates `3.1.0` schema constructs)
- **TypeScript Generator**: `openapi-typescript` 7.13+ (compiles 3.1.0 directly to TypeScript wire types)
- **Contract Gate**: `./scripts/api-contract-gate.sh` enforces target version `3.1.0`

## SDK Implications

- **Nullability** is expressed as `type: ["string", "null"]`, not `nullable: true`.
  A 3.0-only generator will mis-handle it. Any SDK generator adopted in Phase
  API-4 must advertise OpenAPI 3.1 support.
- **`Option<T>` over a `$ref`** renders as `oneOf: [{type: null}, {$ref: ...}]`.
  Generators that flatten `oneOf` too eagerly will produce a union where an
  optional field was meant; check this on the first generated client.
- **Closed enums** (`ErrorCode`, `Metric`, `MetricInput`, `IndexKind`,
  `IndexKindInput`) generate as native enums/literal unions, which is the point
  of modelling them — see `docs/api/security-contract.md` and §8 of the Phase
  API-3.2 doc.
- **Empty-bodied responses** (`204`, and the middleware's `401`/`403`) generate
  as "no content" and are correct. Any *other* contentless response is a defect,
  not a convention — `scripts/verify-api-route-contract.py` enforces that
  distinction against a reviewed allowlist.

## Verification

`scripts/api-contract-gate.sh` fails if the emitted document's `openapi` field
is anything other than the `OPENAPI_TARGET_VERSION` constant it declares
(`3.1.0`). The version cannot drift silently.

Historical documents under `docs/phases/` that mention 3.0.3 are records of
what was true at the time and are deliberately left unedited.
