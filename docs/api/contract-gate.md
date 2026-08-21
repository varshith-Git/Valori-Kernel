# Valori API Contract Gate & Contract Drift Governance Policy

## 1. Executive Summary

The **Valori API Contract Gate** is the single permanent executable entry point ([`scripts/api-contract-gate.sh`](file:///Users/as-mac-0272/Desktop/sass/Valori-Kernel/scripts/api-contract-gate.sh)) that enforces contract reproducibility, route parity, schema integrity, and client wire compatibility.

Before official SDK generation (Phase 4+) can be initiated, the Contract Gate must pass cleanly in CI.

---

## 2. Gate Verification Pipeline (8 Sequential Steps)

```text
=====================================
 VALORI API CONTRACT GATE
=====================================
 [1/8] Utoipa subset generation       PASS
 [2/8] Generated schema drift         PASS
 [3/8] OpenAPI lint                   PASS
 [4/8] Contract integration tests     PASS
 [5/8] Route parity                   PASS
 [6/8] TypeScript generation          PASS
 [7/8] Generated artifact drift       PASS
 [8/8] SDK compatibility              PASS
-------------------------------------
 RESULT: PASS
 SDK READY: NO
 BLOCKERS: 3
-------------------------------------
```

1. **`[1/8]` Utoipa Subset Generation**: Compiles `valori-node` with `--features utoipa` and runs the `valori-openapi` binary to emit the OpenAPI subset YAML.
2. **`[2/8]` Generated Schema Drift**: Executes `cargo test -p valori-node --features utoipa --test openapi_generated` to verify every Utoipa-annotated schema exists in `api/openapi/valori-v1.yaml`.
3. **`[3/8]` OpenAPI Lint**: Runs `npx @redocly/cli@latest lint api/openapi/valori-v1.yaml` to guarantee spec validity.
4. **`[4/8]` Contract Integration Tests**: Executes `cargo test -p valori-node --test api_contract` to enforce Standalone and Raft Cluster behavioral parity.
5. **`[5/8]` Route Parity**: Executes `cargo test -p valori-node --test route_parity` to ensure both routers expose identical paths and HTTP verbs.
6. **`[6/8]` TypeScript API Type Generation**: Executes `./scripts/generate-api-types.sh` to compile `@valori/api-types` (`ui/api-types/src/valori-v1.ts`) from the canonical OpenAPI spec.
7. **`[7/8]` Generated Artifact Drift**: Asserts `git diff --exit-code -- ui/api-types api/openapi` to guarantee that generated files produce zero uncommitted diff on clean rerun.
8. **`[8/8]` SDK Compatibility**: Runs `pytest python/tests/` to verify remote client wire compatibility.

---

## 3. Contract Drift Budget & Change Classification Policy

Every change affecting the public API contract must be classified according to the Contract Drift Budget:

| Change Category | Permitted in Minor Version? | Contract Gate Policy | Examples |
|-----------------|-----------------------------|----------------------|----------|
| **NON-BREAKING** | YES | Allowed automatically | Adding an optional response field, adding an optional query parameter, adding a new ErrorCode variant. |
| **BREAKING** | NO (Requires Major Version) | **REJECTED BY GATE** | Renaming request/response fields, removing an endpoint, removing an ErrorCode variant, changing distance metric score direction. |
| **DEPRECATION** | YES (with `deprecated: true`) | Allowed with explicit timeline | Marking a field or path as deprecated in OpenAPI spec while retaining runtime backward compatibility. |
| **INTERNAL** | YES | Allowed | Refactoring internal kernel/engine structs without altering HTTP wire format or status codes. |
