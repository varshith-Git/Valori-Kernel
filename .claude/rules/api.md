# API rules

**Source of truth:** [`api/openapi/valori-v1.yaml`](../../api/openapi/valori-v1.yaml) — the canonical
contract, `utoipa`-generated from `crates/valori-node`'s route annotations. Not hand-maintained prose;
don't describe endpoints here that duplicate it.

## The contract is three-way verified — this is not optional review, it's a mechanical gate

`scripts/verify-api-route-contract.py` proves:
```
Rust registered public routes  ==  Utoipa generated operations  ==  committed OpenAPI YAML
```
It is **verification only** — it never generates or modifies the OpenAPI file. `tests/api_contract.rs`
(run via `cargo test`) diffs the runtime `ErrorCode` enum against the committed YAML on every CI run.
`scripts/api-contract-gate.sh` is the single entry point that runs contract reproducibility, three-way
route equality, schema integrity, and client compatibility together — run it before claiming an API
change is done:
```
./scripts/api-contract-gate.sh
```

## Error envelope (verified from `api/openapi/valori-v1.yaml`'s `ApiError` schema)

```json
{ "error": "human-readable message — do not parse", "code": "stable_machine_readable_code" }
```
`code` is the closed `ErrorCode` enum (e.g. `validation_error`, `unauthorized`, `forbidden`, `not_found`,
`collection_not_found`, `record_not_found`, `dimension_mismatch`, `invalid_metric`, `invalid_index`, …) —
branch on `code`, never parse `error`. The doc comment on the YAML schema is explicit: `EngineError`
produces this shape at runtime but lives in a crate without a `utoipa` dependency, so the schema is
declared separately in the OpenAPI file rather than re-exported — if you add a new error variant, add it
to **both** the Rust `ErrorCode` enum and the YAML `enum:` list, or `api_contract.rs` fails.

## Standalone vs. cluster — every endpoint needs both

See `.claude/rules/architecture.md` and `AGENTS.md`'s "MANDATORY: single-node AND multi-node" section.
`cargo test -p valori-node --test route_parity` mechanically diffs `server.rs` against `cluster_server.rs`
route declarations (paths *and* methods) — a route added to only one router fails this test, not a
runtime 404 someone finds later. Genuinely path-specific routes (index config/rebuild, proof/timeline
mechanics — see `AGENTS.md`) go in the `STANDALONE_ONLY`/`CLUSTER_ONLY` allowlist with a reason, not left
unregistered.

## SDK generation — the contract is the one source, SDKs are derived

```
api/openapi/valori-v1.yaml  ──openapi-typescript──▶  ui/api-types/src/valori-v1.ts   (generated — never hand-edit)
```
Regenerate via `scripts/generate-api-types.sh` after any contract change. `ui/api-types/src/index.ts` is
the one hand-written file in that pipeline (maps generated schema names to friendlier exports) — don't
hand-edit the generated file instead of the source contract.

`scripts/sdk-coverage-check.py` and `scripts/sdk-repro-check.sh` exist for the Python SDK side —
run them (or the equivalent workflow, `sdk-python.yml`/`sdk-typescript.yml`) after any endpoint change
that should be reachable from `python/valoricore/remote.py`'s `SyncRemoteClient`/`AsyncRemoteClient`.

## If a contract change is genuinely needed

1. Update the Rust handler + its `utoipa` annotations in `crates/valori-node`.
2. Regenerate/verify the OpenAPI YAML — `./scripts/api-contract-gate.sh`.
3. Regenerate TS types — `./scripts/generate-api-types.sh`.
4. Update `python/valoricore/remote.py` (both `SyncRemoteClient` and `AsyncRemoteClient` — `AGENTS.md`'s
   "Where to add things" table).
5. Add the endpoint to both `server.rs` and `cluster_server.rs` per the standalone/cluster rule above.
6. State the compatibility impact explicitly: is this additive (new optional field, new endpoint) or
   breaking (removed/renamed field, changed status code)? A breaking change to a *stable* contract needs
   the versioning discussion in `docs/architecture/layers.md`'s "Stable public contracts" /
   "Compatibility ownership" sections before landing.
