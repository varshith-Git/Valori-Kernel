# Phase API-3 Recovery — Genuine Code-First Utoipa Architecture

**Supersedes**: [`phase-api-contract-3-utoipa.md`](./phase-api-contract-3-utoipa.md) — **that report is false and is retained only as evidence.**
**Predecessor**: [Phase API-2.5 — Conformance Diff Review](./phase-api-contract-2.5-review.md)
**Baseline commit**: `eee123d`
**Type**: Correction / architectural recovery
**Status**: Architecture corrected; contract migration **incomplete**. `SDK READY = NO`.

---

## 1. Goal

Replace the fabricated Phase API-3 OpenAPI pipeline with a genuinely code-first
one — Rust router registrations → annotated handlers → `utoipa` → YAML — and
make every coverage number in the repository a *measured* value rather than an
asserted one.

---

## 2. Delivered

### Freeze & failure audit
- [`docs/api/phase-api-3-recovery-audit.md`](../api/phase-api-3-recovery-audit.md) —
  section-3 working-tree freeze, three-way change classification (failed-attempt /
  API-2 / unrelated), and the proof that the committed contract is synthetic.

### Honest route discovery (§4, §5, §6, §20)
- **`scripts/generate-route-manifest.py`** (new) — parses the axum router
  construction in `server.rs`, `cluster_server.rs`, `cluster_api.rs` and
  `routes/**/*.rs`. It resolves `Router::new()` chains, `let` bindings,
  function-shaped builders, delegating tail calls, and `.merge()` graphs, with
  **file-scoped** name resolution. It never reads `api/openapi/valori-v1.yaml`
  and never emits OpenAPI. On any construct it cannot resolve it prints the
  file, line, and reason and **exits non-zero** rather than under-reporting.
- Regenerated [`phase-api-3-route-manifest.json`](../api/phase-api-3-route-manifest.json)
  and [`.md`](../api/phase-api-3-route-manifest.md) with a `utoipa_registered`
  link field.

### Three-way verifier (§7, §23)
- **`scripts/verify-api-route-contract.py`** (new) — verification only; it
  cannot write OpenAPI. Diffs Rust-registered public routes against live utoipa
  output against the committed contract on method, path, operationId, and
  classification.

### Real `#[utoipa::path]` annotations (§9, §14)
- 11 operations across 10 paths annotated on their **registered** handlers in
  `server.rs`, each with a real request body, real per-status responses, and a
  `BearerAuth` security requirement:
  `health_check`, `create_collection_handler`, `list_collections_handler`,
  `drop_collection_handler`, `insert_record`, `delete_record`,
  `soft_delete_record`, `search`, `multi_search`, `get_operations`,
  `get_operation_by_id`.
- `crates/valori-node/src/openapi.rs` — `ValoriApi` now carries a real
  `paths(...)` list and a `SecurityAddon` modifier declaring `BearerAuth`.
  The module doc no longer overstates coverage.
- `OperationSummary` / `OperationsListResponse` / `OperationDetailResponse`
  gained `ToSchema`.

### Honest contract gate (§22, §26)
- `scripts/api-contract-gate.sh` rewritten. Every printed figure is discovered
  from this run; the `$TOTAL/$TOTAL` tautology and the hardcoded `"14:102:0:79"`
  fallback are gone. SDK readiness is **computed** from step outcomes and
  written to `docs/api/sdk-readiness.json` — never read back from a file.

---

## 3. Findings

1. **The previous contract was orphaned, not merely wrong.** No script in the
   repository could regenerate `api/openapi/valori-v1.yaml`; the reconstruction
   ran ad-hoc and was never committed. Running the command the previous report
   documented would have deleted all 75 paths.
2. **Zero `#[utoipa::path]` annotations existed** anywhere in the workspace, and
   `#[openapi(...)]` had no `paths(...)` argument at all.
3. **The fabrication is legible in the artifact.** All 79 operations carried the
   same two responses (`Successful operation` / `Validation error`); only 4 of
   40 write operations declared a request body; `x-status` was dropped entirely;
   `components.schemas` fell from 102 to 26.
4. **Real route count is 100, not 75.** 74 are public-SDK; the rest are 14
   deprecated aliases, 7 admin, 5 operator-internal.
5. **Genuine parity gaps surfaced** by honest discovery: `GET /graph/nodes` and
   `GET /timeline` are standalone-only; `POST /v1/snapshot/upload` is
   standalone-only.
6. **Two real routes are missing from the contract**: `POST /v1/memory/search_vector`
   and `POST /v1/memory/upsert_vector`.
7. **Seven admin routes leak into the SDK contract** (`/v1/keys*`,
   `/v1/cluster/{add-node,remove-node,snapshot}`, `/v1/crypto/shred/{key_id}`).
8. **Blocker 3b was never actually resolved.** `OperationResponse` (with
   `id` + `legacy_id`) was added to `api.rs` but **no handler returns it** —
   `get_operations` still returns `OperationsListResponse`/`OperationSummary`.
   The dual-ID model exists as a type, not as behaviour.
9. **utoipa 5.5.0 cannot emit OpenAPI 3.0.3.** Its `OpenApiVersion` enum has a
   single variant, `Version31`. §16's target is unreachable without a
   normalization step or a different generator version.
10. **`cargo fmt --check` was failing** in five files the failed attempt touched,
    despite that attempt reporting full validation success.
11. **operationId policy is undecided.** The synthetic contract used SDK-friendly
    names (`get_health`, `create_collection`); Rust handler names are
    `health_check`, `create_collection_handler`. Seven mismatches. Choosing
    either direction is a deliberate decision, not a mechanical fix.
12. **`cd ui && npx tsc --noEmit` was already broken** — 14 errors, all in
    `ui/api-types/src/index.ts`, which aliases schemas the failed attempt
    deleted from the contract (`Error`, `Metric`, `IndexKind`, `IndexType`,
    `IndexState`, `Consistency`, `Collection`, `InsertReceipt`, `Health`,
    `ClusterStatus`, `IndexStatus`, `EventLogProof`, `PoolStats`). The previous
    report claimed this step passed. The aliases were **not** deleted to make
    the check green (§31) — they are the record of what the contract lost. A
    `tsc` step was added to the gate so this cannot pass unnoticed again.

---

## 4. Validation

Run against the live working tree. Real output, not asserted:

| Command | Result |
|---|---|
| `python3 scripts/generate-route-manifest.py` | PASS — 100 routes discovered |
| `cargo fmt --check` | PASS (after fixing 5 files the failed attempt left unformatted) |
| `cargo build --workspace` | PASS |
| `cargo build -p valori-kernel --target wasm32-unknown-unknown` | PASS — `no_std` intact |
| `cargo test -p valori-node` | PASS — 286 passed, 0 failed, 1 ignored |
| `cargo test -p valori-engine` | PASS — 18 passed |
| `cargo test -p valori-kernel` | PASS — 177 passed, 1 ignored |
| `cargo test -p valori-state` | PASS — 24 passed, 1 ignored |
| `cargo test -p valori-storage` | PASS — 78 passed, 1 ignored |
| `cargo test -p valori-consensus` | PASS — 66 passed, 1 ignored |
| `cargo clippy --workspace --all-targets --all-features` | PASS — 0 errors; 10 pre-existing warnings, none new |
| `cargo run … --bin valori-openapi` | PASS — 11 real operations, 29 schemas |
| `python3 scripts/verify-api-route-contract.py` | **FAIL** (expected — 63 routes unannotated) |
| `npx @redocly/cli lint api/openapi/valori-v1.yaml` | PASS |
| `./scripts/generate-api-types.sh` | PASS |
| `cd ui && npx tsc --noEmit` | **FAIL** — 14 pre-existing errors (finding 12) |
| `python3 -m pytest python/tests/` | PASS — 101 passed, 8 skipped |
| `cargo test -p valori-node --features utoipa --test openapi_generated` | **FAIL** (expected — see §5) |
| `./scripts/api-contract-gate.sh` | **FAIL** — 6 blockers, `SDK READY = NO` |

The three failures are **correct**: they are the contract telling the truth
about an incomplete migration. Per §31 no test was weakened, no route was
dropped from the manifest, and no contract was edited to make a check pass.

---

## 5. Deliberate non-actions

- **`api/openapi/valori-v1.yaml` was NOT overwritten.** Emitting the current
  utoipa document would replace 79 operations with 11, destroying the contract
  the UI's `@valori/api-types` and the Python SDK depend on. Overwriting is
  correct only once annotation coverage is complete. The file remains in the
  tree, flagged as distrusted, and the gate fails loudly because of it.
- **`tests/openapi_generated.rs` was NOT relaxed** to accept the three new
  operation schemas. Its failure is the accurate signal that the committed
  contract is not the generator's output.
- **No unrelated working-tree change was reverted or reformatted.**

---

## 6. Follow-ups

Owned by **Phase API-3.1 — Complete the Annotation Sweep** (must precede any SDK phase):

1. Annotate the remaining **63** public routes with real DTOs and real response
   codes. Several handlers return `Json<serde_json::Value>` and need public DTOs
   defined first.
2. Decide the operationId policy (SDK-friendly vs handler-name) and apply it to
   the manifest generator and the annotations together.
3. Reclassify or remove the 7 admin routes currently in the SDK contract.
4. Add `POST /v1/memory/{search_vector,upsert_vector}` to the contract, or
   retire them as aliases.
5. Wire `OperationResponse`'s `id` + `legacy_id` into the operations handlers so
   §13's dual-ID model is behaviour, not just a type.
6. Resolve the OpenAPI 3.0.3 target: upgrade/downgrade the generator, or add a
   deterministic Rust-side 3.1→3.0.3 normalizer that changes representation only.
7. Preserve `x-status` / `x-required-scope` / `x-sdk` through a deterministic
   Rust-side enrichment layer that operates only on already-generated operations.
8. Close the standalone/cluster registration gaps (`/graph/nodes`, `/timeline`,
   `/v1/snapshot/upload`).
9. Only then overwrite `api/openapi/valori-v1.yaml` from the generator and
   regenerate `@valori/api-types`.
