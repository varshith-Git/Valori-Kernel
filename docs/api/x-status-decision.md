# `x-status` Decision — Valori API v1

## What `x-status` was

Hand-maintained iterations of the Valori OpenAPI document (pre-Phase API-3)
carried an `x-status` vendor extension on operations, with values in the
neighbourhood of `stable` / `experimental` / `deprecated`.

It never had a source in Rust. Nothing in `server.rs`, `cluster_server.rs`, or
`api_keys.rs` computed it, no handler declared it, and no test asserted it. It
was maintained by editing YAML. When the contract became code-first, the field
had nowhere to come from — which is precisely why it disappeared rather than
being ported.

Investigating what it actually meant across the historical documents, it was
carrying **two unrelated ideas at once**:

1. *Is this operation deprecated?* — a lifecycle fact.
2. *Is this operation part of the supported SDK surface?* — a boundary fact.

Both of those already have real, machine-derived homes in the current
pipeline. `x-status` was a third spelling of facts owned elsewhere.

## Decision: OPTION B — documentation-only, and removed

`x-status` is **not** part of the runtime contract, is **not** reintroduced,
and does **not** participate in SDK readiness.

It was not recreated in a post-generation script. A value invented by tooling
and then read back as though it were authoritative is exactly the failure mode
`docs/api/phase-api-3-recovery-audit.md` records; restoring `x-status` that way
would have re-committed it in miniature.

## What replaces each half

| Former use | Canonical mechanism | Source of truth |
|---|---|---|
| "this operation is deprecated" | OpenAPI's own `deprecated: true` | `#[utoipa::path(deprecated)]` on the handler |
| "this operation is in the SDK surface" | Route classification | `scripts/generate-route-manifest.py` → `docs/api/phase-api-3-route-manifest.json`; enforced by `scripts/verify-api-route-contract.py` |

Both are standard or verifiable. Neither requires a client to understand a
Valori-specific vendor extension to answer the question.

Note that the deprecated **routes** are not in the public document at all —
all 14 are classified `DEPRECATED` and excluded (see
`docs/api/non-public-routes.md`), so no operation in the current contract needs
`deprecated: true` yet. The mechanism is the one to use when a *public*
operation is eventually deprecated.

## Consequence for `x-sdk`

`VendorExtensionAddon` also writes `x-sdk` on every operation. Because the
document contains only public operations, that value is `true` on all 74 and
carries no information — it cannot distinguish anything, since an operation
that would be `false` is never emitted.

It is retained for now as a harmless explicit marker, but it is **not** a
readiness signal and no consumer should branch on it. The route manifest is
where the public/non-public boundary is actually decided. Removing `x-sdk`
outright is a candidate for a later phase; it is called out here so nobody
mistakes a constant for a contract.

## Invariants

- No script may inject `x-status` into `api/openapi/valori-v1.yaml`.
- Deprecation is expressed only via `deprecated: true`, set on the handler.
- The SDK boundary is decided only by route classification in the manifest.
