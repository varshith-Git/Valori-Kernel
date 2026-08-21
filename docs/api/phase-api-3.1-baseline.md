# Phase API-3.1 — Baseline (measured before any implementation change)

**Date**: 2026-08-20
**Baseline commit**: `eee123d`
**Branch**: `main` (live working tree — nothing committed, reverted, or stashed)

Every number below was produced by running the tooling that already exists in
the tree, before a single line of Phase API-3.1 implementation was written.

## Working tree at entry

```
git status --short          334 entries
```

No `git checkout`, `reset`, `stash`, `clean`, `commit`, or `push` was run at any
point in this phase.

## Measured baseline

Command: `python3 scripts/verify-api-route-contract.py`

| Metric | Value |
|---|---|
| Rust routes discovered | 100 |
| Rust public routes (`PUBLIC_SDK` + `PUBLIC_UNAUTH`) | 74 |
| Utoipa operations generated | 11 |
| Canonical OpenAPI operations (`api/openapi/valori-v1.yaml`) | 79 |
| Missing from Utoipa | 63 |
| Missing from canonical OpenAPI | 2 |
| Unexpected Utoipa | 0 |
| Unexpected OpenAPI (synthetic) | 0 |
| operationId mismatches | 7 |
| Classification errors (non-public leaking into contract) | 7 |
| **Total discrepancies** | **79** |

> The Phase API-3.1 brief quoted 83 discrepancies. The verifier as committed at
> the end of the recovery phase reports **79** — the recovery phase's final
> verifier revision de-duplicates routes that were being counted both as
> "unexpected OpenAPI" and as "classification error". 79 is the number this
> phase is measured against; it is what the script prints today.

### Missing from canonical OpenAPI (2)

```
POST /v1/memory/search_vector
POST /v1/memory/upsert_vector
```

### operationId mismatches (7)

```
DELETE /v1/namespaces/{name}  rust=drop_collection_handler    openapi=delete_collection
GET    /health                rust=health_check               openapi=get_health
GET    /v1/namespaces         rust=list_collections_handler   openapi=list_collections
GET    /v1/operations         rust=get_operations             openapi=list_operations
GET    /v1/operations/{id}    rust=get_operation_by_id        openapi=get_operation
POST   /v1/namespaces         rust=create_collection_handler  openapi=create_collection
POST   /v1/search/multi       rust=multi_search               openapi=search_multi
```

### Classification leaks (7)

```
DELETE /v1/crypto/shred/{key_id}   ADMIN
DELETE /v1/keys/{id}               ADMIN
GET    /v1/keys                    ADMIN
POST   /v1/cluster/add-node        ADMIN
POST   /v1/cluster/remove-node     ADMIN
POST   /v1/cluster/snapshot        ADMIN
POST   /v1/keys                    ADMIN
```

## OpenAPI version at entry

| Source | Version |
|---|---|
| `valori-openapi` generator output | **3.1.0** |
| Committed `api/openapi/valori-v1.yaml` | 3.0.3 |

`utoipa 5.5.0` is pinned in `crates/valori-node/Cargo.toml`. Its
`openapi::OpenApiVersion` enum has exactly one variant (`Version31`), so 3.0.x
output is not reachable by configuration on this dependency version. See
`docs/api/openapi-version-decision.md`.

## SDK readiness at entry

`docs/api/sdk-readiness.json`, as computed by `scripts/api-contract-gate.sh`:

```
sdk_ready     : false
gate_result   : FAIL
blocker_count : 6
```

Blockers:

1. Three-way route equality fails.
2. Generated schema set does not conform to the committed contract.
3. `ui/api-types` does not typecheck against the generated contract.
4. Utoipa generates 11 of 74 public operations; 63 have no `#[utoipa::path]`.
5. The committed contract carries 79 operations; the generator emits 11.
6. Generated document is OpenAPI 3.1.0; the recorded target was 3.0.3.

## Known instrumentation defect found while taking this baseline

`scripts/generate-route-manifest.py::find_utoipa_annotations()` matches only the
literal `#[utoipa::path(` form. Every annotation in this repository is written as
`#[cfg_attr(feature = "utoipa", utoipa::path(...))]`, so the manifest reported
`utoipa_registered = false` for all 74 public routes even though 11 were really
annotated and really generated. The manifest's own totals table therefore said
"Public routes with `#[utoipa::path]`: 0" while the generator emitted 11.

This is a §6 violation (`utoipa_registered` must reflect actual Rust
registration) and is fixed as the first task of this phase.
