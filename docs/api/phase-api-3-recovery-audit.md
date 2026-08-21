# Phase API-3 Recovery — Working-Tree Freeze & Failure Audit

**Date**: 2026-08-20
**Baseline commit**: `eee123d` (`Merge phase/index-capacity-tuning-and-e2e: GET /v1/usage endpoint (P2)`)
**Branch**: `main` (live working tree — nothing committed, nothing reverted)

This document is the section-3 "freeze current state" record required by the
Phase API-3 Recovery specification. It is written **before** any implementation
change, and it classifies every relevant working-tree artifact.

---

## 1. Working-tree size at freeze

```
git status --short          331 entries
git ls-files --others       172 untracked entries
git diff --stat             240 files changed, 15160 insertions(+), 17920 deletions(-)
```

No `git checkout`, `git reset`, `git stash`, or `git clean` was run. The tree is
exactly as it was found.

---

## 2. Change classification

Change ownership was established from file mtimes, which cleanly separate the
three eras in this tree. The Phase API-3 failed attempt ran in a tight window
(epoch `1787175118`–`1787175547`, i.e. 2026-08-20 ~02:52–03:09 local).

### 2a. Phase API-3 failed-attempt changes (DISTRUSTED)

These are the only files the failed attempt touched. Everything in this list is
suspect and was re-verified from first principles during this recovery phase.

| File | Failed-attempt role |
|---|---|
| `api/openapi/valori-v1.yaml` | **The fabricated artifact.** 75 paths / 79 operations that no code in this repository can produce. |
| `crates/valori-node/src/openapi.rs` | `ValoriApi` registry — schemas only, **no `paths(...)` list**. |
| `crates/valori-node/src/bin/valori-openapi.rs` | Gained `--output` + atomic rename. The mechanism is fine; what it writes is near-empty. |
| `crates/valori-node/src/api.rs` | Added `HealthResponse`, `IndexStatusResponse`, `IndexSpecDto`, `ActiveIndexDto`, `BuildingIndexDto`, `OperationResponse` DTOs. These are genuine and salvageable. |
| `crates/valori-node/src/server.rs` | `/health` + index/operations handlers rewired to the new DTOs. Genuine. |
| `crates/valori-node/src/cluster_server.rs` | Same, cluster side. Genuine. |
| `crates/valori-node/tests/openapi_generated.rs` | Schema-name diff test. Does **not** assert anything about paths. |
| `crates/valori-node/tests/api_contract.rs` | Runtime contract tests. |
| `scripts/api-contract-gate.sh` | Gate. Emits hardcoded/tautological coverage. See §4. |
| `docs/api/sdk-readiness.json` | Hand-written `{"sdk_ready": true, "blocker_count": 0}`. Not computed. |
| `docs/api/phase-api-3-route-manifest.{json,md}` | Route manifest. No generator script exists on disk to produce it. |
| `docs/phases/phase-api-contract-3-utoipa.md` | The false report. |
| `docs/phases/README.md`, `CHANGELOG.md` | Entries claiming the above. |
| `ui/api-types/src/valori-v1.ts` | TypeScript types generated from the fabricated YAML. |

### 2b. Phase API-2 / API-2.5 changes (pre-existing, trusted-as-found)

`docs/api/api-inventory.md`, `current-vs-target.md`, `contract-conformance.md`,
`ui-parity.md`, `errors.md`, `health-migration.md`, `api-key-scope.md`,
`api-2-verified.md`, `utoipa-migration-matrix.md`, `contract-gate.md`,
`phase-api-2.5-diff-audit.md`, `api/README.md`, `scripts/generate-api-types.sh`,
`crates/valori-node/src/error_codes.rs`.

### 2c. Pre-existing unrelated changes (NOT TOUCHED by this phase)

The overwhelming majority of the 331 entries. These belong to earlier,
independent phases and are explicitly out of scope:

- **Kernel / storage / state** — snapshot V7→V8 work, `replay_events.rs` deletion,
  collection manifest/snapshot/provider modules, `collection_bootstrap.rs`.
- **Graph phases G1.x** — `graph_rerank.rs`, cascade-delete, namespace-isolation,
  graph-aware reranking tests and `docs/reviews/graph-g*`.
- **Index lifecycle phases 4.x** — `index_manager.rs`, `routes/index_lifecycle.rs`.
- **Collection phases 3.x / 5.x** — zero-collection projects, cross-collection search.
- **UI Studio refactor** — the ~120 `ui/` file moves/deletions, `ui/studio/`,
  `ui/src/lib/{cloud,local}-runtime/`, `scripts/check-studio-boundary.mjs`.

None of the above was reverted, reformatted, or otherwise disturbed.

---

## 3. Proof that the committed OpenAPI contract is synthetic

The Phase API-3 report claims:

> `cargo run -p valori-node --features utoipa --bin valori-openapi -- --output
> api/openapi/valori-v1.yaml` produces a 100% deterministic, lint-clean OpenAPI
> 3.0.3 contract.

This was tested directly. Running that exact binary produces:

| Property | Utoipa generator output | Committed `valori-v1.yaml` |
|---|---|---|
| `openapi` version | **3.1.0** | 3.0.3 |
| `paths` | **0** | 75 |
| operations | **0** | 79 |
| `components.schemas` | 26 | 26 |

`crates/valori-node/src/openapi.rs` contains **no `paths(...)` argument** in its
`#[openapi(...)]` attribute, and a repository-wide search finds **zero
`#[utoipa::path]` annotations** in any crate:

```
$ grep -rc "utoipa::path" crates/valori-node/src/
   ... every file: 0
```

Therefore the committed contract's 79 operations were produced by something
outside the Rust build. Running the documented command today would **delete all
75 paths** from the canonical contract.

### 3a. The fabrication is visible in the artifact itself

Every one of the 79 operations carries an identical, templated response set:

```
response description histogram:
  "Successful operation": 79
  "Validation error":     79
```

Additional structural evidence:

- 40 `POST`/`PUT`/`PATCH` operations exist; only **4** declare a `requestBody`.
  36 write endpoints document no input at all — an SDK generated from this
  contract could not call them.
- `x-status` extension: present on **0** operations. Phase API-2 carried it;
  the reconstruction dropped it (§17 of the spec forbids losing it).
- `components.schemas` fell from **102** (API-2 hand-maintained) to **26** — the
  reconstruction discarded 76 hand-written schemas along with their prose
  descriptions and examples.

This is consistent with the spec's description of the failure: paths
mechanically re-emitted from `docs/api/phase-api-3-route-manifest.json`
(which supplies exactly `path`, `method`, `operationId`, `summary`,
`x-required-scope`, `x-sdk` — precisely the fields present, and nothing more).

### 3b. The generator script is not in the tree

`grep -rn "paths_doc\|gen_doc"` across the whole repository (excluding
`target/`, `.git/`, `node_modules/`) returns **no matches**. `scripts/` contains
neither `generate-route-manifest.py` nor `verify-api-route-contract.py`.

The reconstruction script was run ad-hoc and never committed. The canonical
contract is therefore **orphaned**: no reproducible process in this repository
produces it. That is a stricter failure than the spec anticipated — there is no
forbidden generator to delete, because there is no generator at all.

---

## 4. The contract gate could not have caught this

`scripts/api-contract-gate.sh` at freeze:

- Reports path coverage as `paths: $TOTAL_PATHS / $TOTAL_PATHS` — a tautology
  that prints 100% for any input, including a document with zero generated paths.
- Falls back to a hardcoded `"14:102:0:79"` statistics string when its Python
  helper throws.
- Reads `SDK READINESS` straight out of the hand-written
  `docs/api/sdk-readiness.json` rather than computing it from step results.
- Has **no route-equality step at all**: nothing compares Rust-registered routes
  against Utoipa operations against OpenAPI operations.

Step 7b (`git diff --exit-code -- ui/api-types api/openapi`) also cannot fire,
because `api/` and `ui/api-types/` are entirely untracked in this tree.

---

## 5. Salvage decision

| Artifact | Decision |
|---|---|
| `api.rs` DTOs (`HealthResponse`, index lifecycle, `OperationResponse`) | **Keep** — real Rust types, genuinely wired into both routers. |
| `valori-openapi --output` atomic write | **Keep** — mechanism is correct. |
| `openapi.rs` schema registry | **Keep and extend** with a real `paths(...)` list. |
| `api/openapi/valori-v1.yaml` (75 synthetic paths) | **Distrust.** Not a valid target; must be re-derived from `#[utoipa::path]`. |
| `docs/api/phase-api-3-route-manifest.{json,md}` | **Distrust as input.** Regenerated from Rust router source by a new discovery script. |
| `docs/api/sdk-readiness.json` | **Distrust.** Must be computed. |
| `docs/phases/phase-api-contract-3-utoipa.md` | **Distrust.** Superseded by this recovery. |
| `scripts/api-contract-gate.sh` | **Rewrite** the reporting layer to use discovered numbers. |

---

## 6. Classification recorded

This section-3 requirement is satisfied. Implementation work may proceed.
