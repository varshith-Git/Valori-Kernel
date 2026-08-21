# Phase API-Contract-2.5 — Conformance Diff Review, Working-Tree Isolation, Utoipa/OpenAPI Integrity, and Pre-SDK Gate

**Predecessor**: [Phase API-2 — Contract Convergence](./phase-api-contract-2-convergence.md)
**Baseline Commit**: `eee123dd0485941252da9e1ff8438a478e34b3a2`
**Type**: Audit, Governance, Isolation & Reproducibility Phase

---

## 1. Goal

Establish a trustworthy boundary between Phase API-2 changes, pre-existing working-tree modifications, and intentional follow-up work; make the current API contract pipeline reproducible and auditable; and issue an evidence-backed Pre-SDK Gate decision before multi-language SDK generation.

---

## 2. Delivered

### Working-Tree Forensics & Isolation
- **Classification Audit**: Executed forensic audit across all 328 working-tree files relative to baseline `eee123d`. Classified every file into categories A–E with **Category F ("Cannot determine") reaching exactly 0**.
- **Special Boundary Review**: Performed explicit 6-question boundary audit for 11 suspicious/unrelated platform files (`index_manager.rs`, snapshot v8 fixtures, kernel internals, graph/storage manifests), confirming isolation without modification or deletion.
- Delivered: [`docs/api/phase-api-2.5-diff-audit.md`](../api/phase-api-2.5-diff-audit.md).

### API-2 Claim Verification & Error Taxonomy
- **Source & Test Verification**: Verified all 11 primary claims of Phase API-2 directly against Rust server source, Python SDK, and integration test assertions.
- **Error Code Invariant**: Confirmed compile-time exhaustive matching in `EngineError::parts()` -> `ErrorCode` with zero wildcard `_ => ...` fallback branches.
- **Request ID Deduplication Model**: Audited process-scoped (Standalone) and Raft-replicated (Cluster) `request_id` deduplication (`idempotent within retained request-id window`).
- Delivered: [`docs/api/api-2-verified.md`](../api/api-2-verified.md) and [`docs/api/errors.md`](../api/errors.md).

### Utoipa & TypeScript Reproducibility
- **Migration Matrix**: Documented Utoipa subset (14 generated schemas, 0 paths) vs canonical hand-maintained YAML (102 schemas, 79 paths).
- **TypeScript Wire Model**: Enforced `@valori/api-types` generation via `./scripts/generate-api-types.sh` -> `ui/api-types/src/valori-v1.ts` containing strictly wire models (`components["schemas"]`).
- **Clean Two-Run Reproducibility**: Verified zero diff on consecutive generation runs (`cmp "$TMP1" "$TMP2"` and `git diff --exit-code -- ui/api-types api/openapi`).
- **Structural Schema Conformance**: Added structural schema property assertions in `crates/valori-node/tests/openapi_generated.rs`.
- Delivered: [`docs/api/utoipa-migration-matrix.md`](../api/utoipa-migration-matrix.md).

### Contract Gate & SDK Readiness Matrix
- **Single Executable Contract Gate**: Created [`scripts/api-contract-gate.sh`](../../scripts/api-contract-gate.sh) with 3 conceptual layers (Contract Generation, Runtime Conformance, Client Compatibility) running an 8-step pipeline with failure log capture.
- **Contract Drift Governance**: Documented breaking vs non-breaking drift policy in [`docs/api/contract-gate.md`](../api/contract-gate.md).
- **Machine-Readable SDK Readiness**: Created [`docs/api/sdk-readiness.json`](../api/sdk-readiness.json) and evaluated 10 API domains in [`docs/api/sdk-readiness.md`](../api/sdk-readiness.md).
- **Pre-SDK Gate Verdict**: Issued formal decision **CONTRACT GATE: PASS**, **SDK READY = NO (3 BLOCKERS)**, establishing explicit blockers for Phase 3 and Phase 4.

### Documentation Analysis Items (No Code Changes)
- Delivered: [`docs/api/health-migration.md`](../api/health-migration.md) (standalone vs cluster `/health` shape divergence & migration plan).
- Delivered: [`docs/api/api-key-scope.md`](../api/api-key-scope.md) (`ApiKeyRecord.collection` authorization analysis & security recommendations).

---

## 3. Findings

1. **Working-Tree Composition**: Out of 328 modified/untracked files, 31 belong directly to API-2 convergence (Category A), 96 are server/state dependencies (Category C), 21 are isolated platform/snapshot v8 features (Category D), 136 are pre-existing workspace files (Category B), and 44 are documentation/generated contract artifacts (Category E).
2. **Contract Reproducibility**: Utoipa subset generation (14 schemas) and TypeScript wire model generation are 100% reproducible and deterministic across clean consecutive runs. The full OpenAPI contract remains hybrid/manual until Phase 3.
3. **Pre-SDK Gate Blockers**: Official SDK generation must remain blocked until Phase 3 resolves `/health` response shape divergence, path annotations (0/79 paths annotated), and background index lifecycle status representation.

---

## 4. Validation

- Executed Contract Gate script: `./scripts/api-contract-gate.sh` (Passed 8/8 steps).
- Category F count: **0** unclassified files.
- Two-run generator diff check: `cmp "$TMP1" "$TMP2"` clean.
- Rust test suite, Clippy, WASM build, and Python SDK tests green.

---

## 5. Follow-ups

- **Phase API-3 (Full Utoipa Migration)**: Annotate all 79 path items and remaining schemas in `valori-node` using Utoipa, unifying `/health` response shapes.
- **Phase API-4 (Official Python SDK)**: Generate official Python client package after Phase API-3 contract gate passes.
- **Phase API-5 (Official TypeScript SDK)**: Generate official TypeScript SDK package.
