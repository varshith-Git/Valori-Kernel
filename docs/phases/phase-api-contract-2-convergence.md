# Phase API-2 — Contract Convergence

**Predecessor**: [Phase API-1 — Deep API Audit](./phase-api-1-contract-audit.md)
**Branch**: `main` (unstaged working tree)
**Type**: implementation phase — Rust, Python, TypeScript, CLI

---

## Goal

Make the running node match `api/openapi/valori-v1.yaml`. Phase API-1 was
audit-only and produced a 52-row gap list with nine P0s; this phase closes the
P0s and as many P1s as fit, fixes the implementation rather than weakening the
contract wherever the two disagree, and leaves every remaining divergence
written down with the reason it is still open.

Three binding product decisions framed the work and were not re-litigated:
stable machine-readable error codes are introduced now; ingest must never
auto-create a Collection; and standalone `request_id` idempotency is
implemented for real rather than documented away.

---

## Delivered

### Error contract — `{error, code}` everywhere

| File | What landed |
|---|---|
| `crates/valori-engine/src/error.rs` | `ErrorCode` enum, 16 variants, each mapped from a real `EngineError`/`KernelError` variant — nothing invented. `as_str()` is the exact wire spelling. |
| `crates/valori-node/src/error_codes.rs` *(new)* | `attach_error_code` — a response-layer middleware installed on **both** routers. It rewrites any error response that lacks a `code`, including bodies axum produced itself, so the bare 401/403 that previously had **no body at all** are now parseable JSON. Also holds `collection_not_found()`, the one canonical "this Collection does not exist" response that six handlers used to build by hand with three different messages and two different status codes. |
| `crates/valori-node/src/errors.rs` | `error_response(status, code, message)` — the single constructor. |
| `api/openapi/valori-v1.yaml` | `Error` schema gains `code`; the `ErrorCode` enum moves from `x-status: target` to current. |

`error` (the human string) is unchanged and still emitted — this is additive,
not a break. `code` is the field clients should branch on.

### Record API — one canonical request model

`crates/valori-node/src/api.rs` now owns `InsertRecordRequest`,
`InsertRecordResponse`, `InsertReceiptJson` and `RequestId`, and **both**
routers deserialise them. Before this phase standalone accepted
`{values, collection, text}` and cluster accepted
`{values, collection, metadata, tag, request_id}`, each silently dropping the
other's fields. The union — `values`, `collection`, `text`, `metadata`, `tag`,
`request_id` — is accepted and honoured on both paths.

`RequestId` normalises the two spellings that already existed on the wire: a
16-byte array (cluster `POST /v1/records`, what the Python SDK sends) and a
32-char hex string with optional UUID dashes (`/v1/vectors/batch-insert`
`request_ids`). Anything else — wrong length, non-hex, wrong JSON type — is a
hard deserialisation error. A malformed idempotency token is never silently
ignored.

### Standalone idempotency

`crates/valori-engine/src/engine.rs` gains `dedup_lookup(&[u8;16]) -> Option<u32>`
and `dedup_record([u8;16], u32)`, deliberately mirroring
`valori-consensus`'s `StateMachineInner::dedup_map` — same 16-byte token, same
bounded-FIFO eviction, same semantics. First token → record created; replayed
token → the original `record_id` returned with no second write; new token →
new insert; malformed token → 4xx. Batch insert reuses the same table, so a
`request_ids` list with a repeat resolves to the already-created id rather
than a duplicate.

### Status-code and scope convergence

* Unknown Collection is **404 `collection_not_found`** on both paths
  (was 400 standalone / 404 cluster on multi-search).
* `k` is required on `POST /v1/search` on both paths — the cluster's hidden
  default of 10 is gone.
* `POST /v1/cluster/add-node`, `/remove-node`, `/snapshot` now require
  **`admin`** (any read-write key could previously reconfigure cluster
  membership). `POST /v1/search/multi` and `POST /v1/graphrag` now require
  **`read_only`** (a read-only key previously could not run a cross-collection
  or GraphRAG query, because neither path literally ends in `/search`).
  `api_keys::required_scope()` and the contract's `x-required-scope` were both
  corrected — the implementation was wrong in both directions, not the
  contract.

### Python SDK

`python/valoricore/remote.py` — the implicit `collection="default"` default is
removed from every method where the server requires a Collection, on both
`SyncRemoteClient` and `AsyncRemoteClient`. A zero-collection project (the
normal state of a new project since Phase 3.3) no longer 404s behind a default
argument the caller never typed. `filter_tag`, which no server request type
has ever carried, now raises instead of silently no-opping. The embedded
`.so`/FFI path was not touched.

### TypeScript / UI

* `ui/api-types/` *(new)* — internal workspace package `@valori/api-types`.
  `src/valori-v1.ts` is machine output from
  `scripts/generate-api-types.sh` (`openapi-typescript@7`);
  `src/index.ts` is the hand-written alias layer mapping
  `components["schemas"][…]` onto the short names the UI uses, so a renamed
  schema becomes a TypeScript error rather than silent drift.
* `ui/src/types/valori.ts` and `ui/studio/src/types/valori.ts` now re-export
  from it instead of redeclaring the wire model. Each keeps only its UI-only
  view types, with a header naming every field that is app-derived rather than
  wire: `SearchResponse.state_hash` (never emitted), `queried_at` (produced by
  the app's own BFF route), `ClusterStatus.converged` (derived in `useCluster`).
* `ui/src/lib/valori-client.ts` **deleted**. Every import was checked first;
  it had none. Its `createCollection({name})`, which sent no dimension or
  metric, died with it.
* `ui/src/app/api/ingest/route.ts` no longer POSTs `{"name": collection}` to
  auto-create the target Collection. It resolves the Collection first and
  fails fast with a message naming the endpoint to call, and cross-checks the
  embedding dimension against the Collection's own.

### CLI

`crates/valori-cli/src/commands/import.rs` no longer reads `/health.dim` —
a node-wide dimension that stopped existing when Collections became the unit
of vector configuration. Dimension now comes from the source (Qdrant
collection config, or the first JSONL vector). An existing target Collection
is validated against it and **never** mutated; a missing one is created with
an explicit dimension and metric.

`crates/valori-daemon/src/daemon.rs` — `create_collection` now takes
`dimension`, `metric` and an optional `index` and forwards them.

### Utoipa (§32) — partial, and labelled as such

* `crates/valori-node/Cargo.toml` — `utoipa 5.5` behind an **opt-in**
  `utoipa` feature, so the shipped binary carries no schema-generation code.
* `crates/valori-node/src/api.rs` — public DTOs carry
  `#[cfg_attr(feature = "utoipa", derive(ToSchema))]`. `RequestId`'s schema is
  hand-written (`PartialSchema`) because it accepts two JSON shapes, which a
  derive over a newtype cannot express.
* `crates/valori-node/src/openapi.rs` *(new)* — `ValoriApi`, the
  `#[derive(OpenApi)]` root, plus `ApiError`/`ErrorCodeSchema` as the
  translation layer §36 asks for (a DTO in the node crate, not a re-export of
  `EngineError` from a crate that does not depend on `utoipa`).
* `crates/valori-node/src/bin/valori-openapi.rs` *(new)* — the generation
  entrypoint: `cargo run -p valori-node --features utoipa --bin valori-openapi`.

**Coverage is 16 schemas and zero path items** — Collections, Records, Search,
Multi-Search, Errors, exactly the domains this phase converged. Of the 102 schemas in the committed file, 90 are hand-maintained
only, and all 79 operations are. The module doc comment,
`api/README.md` §5b and `docs/api/contract-conformance.md` all say so
explicitly; nothing in the repo implies the document is fully generated.

### Documentation

* `docs/api/contract-conformance.md` *(new)* — the standing answer to "where
  does the node agree with the contract": generation status, per-domain
  implementation status, resolved divergences, open divergences with reasons,
  and the intentionally-unsupported list.
* `docs/api/current-vs-target.md` — the 52-row audit table is left **intact**
  and a Phase API-2 resolution log appended, marking each row RESOLVED or OPEN
  with what changed. Rows were not deleted; the reasoning that justified each
  fix stays readable.
* `docs/api/ui-parity.md` — §5 rewritten as an outcome list.
* `docs/api/api-inventory.md` — a Phase API-2 note recording that the route set
  is unchanged but the behaviour behind five routes is not.
* `api/README.md` — new §5b on generation status and the sync process, plus
  code-side conformance commands in §5.

---

## Findings

1. **Route parity was a false comfort.** `route_parity.rs` proves both routers
   declare the same paths and methods, and every single P0 in the audit lived
   in the space that test does not cover — request fields, response fields,
   defaults, status codes, error bodies. Path-and-method parity is necessary
   and nowhere near sufficient. `api_contract.rs` exists because of this.

2. **Silent field-dropping is the worst failure mode in the codebase.** Serde
   ignoring an unknown field means a client can send `request_id` on every
   insert, believe it has idempotency, and have none. Nothing errors, nothing
   logs, no test fails. The canonical DTO is the structural fix; the guard test
   that fails if either router reintroduces a private insert struct is what
   keeps it fixed.

3. **Prefix-derived authorization drifts in both directions.**
   `required_scope()` deriving scope from `(method, path)` produced one
   privilege escalation (cluster membership on a read-write key) and one
   false denial (read-only keys locked out of multi-search) from the same
   rule. Scope belongs next to the route, not in a prefix table — recorded as
   a follow-up.

4. **The contract was right more often than the code.** Of the mismatches
   examined, the contract described the intended behaviour and the
   implementation had drifted, not the reverse. The one rule that mattered —
   never fix a mismatch by weakening the contract — cost nothing to follow.

5. **`/health` is the hardest remaining P0 precisely because it is trivial.**
   Two structurally different bodies, and the UI, CLI wizard, compose
   healthchecks and MCP server all parse one of them. There is no way to
   converge it inside a phase that also promises not to break clients.

6. **Utoipa cannot carry prose.** Demanding byte-equality between the
   generated and committed documents would mean deleting the descriptions,
   examples and `x-` extensions that make the contract worth reading. Diffing
   schema *names* catches the drift that matters (a DTO renamed without a
   contract edit) at none of that cost.

---

## Validation

All commands run for real; counts are actual.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo build --workspace --all-features` | ✅ |
| `cargo build -p valori-kernel --target wasm32-unknown-unknown` | ✅ (`no_std` invariant intact) |
| `cargo clippy --workspace --all-targets --all-features` | 0 errors. Warnings are pre-existing in `valori-engine` (9); the one warning this phase introduced (`items after a test module` in `error_codes.rs`) was fixed. |
| `cargo test -p valori-kernel` | **177 passed**, 0 failed, 1 ignored |
| `cargo test -p valori-engine` | **18 passed**, 0 failed |
| `cargo test -p valori-state` | **24 passed**, 0 failed, 1 ignored |
| `cargo test -p valori-storage` | **78 passed**, 0 failed, 1 ignored |
| `cargo test -p valori-consensus` | **76 passed**, 0 failed, 1 ignored |
| `cargo test -p valori-node --features utoipa` | **447 passed**, 0 failed, 1 ignored |
| **Rust total** | **820 passed, 0 failed** |
| `python3 -m pytest python/tests -q` | **101 passed**, 8 skipped, 27 deselected |
| `cd ui && npx tsc --noEmit` | clean (exit 0) |
| `npx @redocly/cli@latest lint api/openapi/valori-v1.yaml` | **valid**, 4 intentional warnings (documented in `api/README.md` §5) |

### New tests

`crates/valori-node/tests/api_contract.rs` — 21 tests. It drives the **same
request** through a real standalone router and a real single-node Raft cluster
router and compares what comes back.

| Test | Proves |
|---|---|
| `insert_accepts_the_full_canonical_field_set_on_both_paths` | No field is dropped on either path |
| `both_routers_share_the_canonical_insert_dto` | A router-private insert struct cannot be reintroduced |
| `request_id_accepts_both_wire_spellings_on_both_paths` | Byte-array and hex tokens both parse |
| `invalid_request_id_is_rejected_not_ignored_on_both_paths` | Malformed tokens error, never silently drop |
| `standalone_request_id_deduplicates` | The P0 fix, standalone |
| `cluster_request_id_deduplicates_to_the_same_record_id` | Same semantics on cluster |
| `search_response_shape_agrees_on_both_paths` | One `SearchResponse` |
| `search_k_bounds_agree_on_both_paths` | `k` required and bounded identically |
| `unknown_collection_is_404_collection_not_found_on_both_paths` | Status-code fork closed |
| `multi_search_error_statuses_agree_on_both_paths` | 404 / 400 / 400, both paths |
| `multi_search_hits_always_carry_collection_identity` | `record_id` alone is ambiguous |
| `collection_creation_requires_dimension_and_metric_on_both_paths` | No config-free creation |
| `collection_create_idempotency_agrees_on_both_paths` | Same config → existing; different → conflict |
| `default_has_no_implicit_behaviour_on_both_paths` | `"default"` is just a name |
| `every_error_response_carries_a_code_on_both_paths` | The error middleware works |
| `unauthorized_has_a_parseable_json_body_with_a_code` | 401/403 are no longer bodiless |
| `error_code_enum_matches_the_openapi_contract` | Rust enum ↔ committed YAML |
| `both_routers_install_the_error_code_middleware` | Structural guard |
| `legacy_aliases_still_work_and_announce_their_deprecation` | `Deprecation` + `Link` headers intact |
| `scope_derivation_matches_the_documented_contract` | The §17 scope fixes |
| `contract_records_the_corrected_scopes` | Contract not weakened to match code |

`crates/valori-node/tests/openapi_generated.rs` — 2 tests (feature-gated).
Asserts the generated document is valid YAML carrying the error taxonomy, and
that every generated schema name exists in the committed contract, with a
short justified allowlist for the four DTOs the contract names differently.

### Test matrix — standalone vs cluster (§39)

`✅` = proven by a test that drives the same request through both routers and
compares. `—` = not applicable (standalone has no Raft).

| Operation | SA | CL | Request parity | Response parity | Status parity | Auth parity |
|---|---|---|---|---|---|---|
| Create Collection | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| List Collections | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_only` |
| Delete Collection | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| Insert record | ✅ | ✅ | ✅ one DTO | ✅ (`log_index`/`deduplicated` omitted on SA) | ✅ | ✅ `read_write` |
| Batch insert | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| Get record | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_only` |
| Update metadata | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| Delete record | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| Search | ✅ | ✅ | ✅ (`k` required both; `as_of*` SA-only, `consistency` CL-only — modelled, not ignored) | ✅ | ✅ | ✅ `read_only` |
| Multi-search | ✅ | ✅ | ✅ | ✅ (`collection` on every hit) | ✅ 404/400/400 | ✅ `read_only` (was `read_write`) |
| Index build | ✅ | ✅ | ✅ | ✅ | ✅ 202 + poll | ✅ `read_write` |
| Index status | ✅ | ✅ | ✅ | ⚠️ `status` is derived, not raw `IndexState` — documented drift | ✅ | ✅ `read_only` |
| Index drop | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| Graph node create/delete | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_write` |
| Graph edge create/list | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Graph query / subgraph | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ `read_only` |
| GraphRAG | ✅ | ✅ | ✅ (+ `consistency` CL-only) | ✅ | ✅ | ✅ `read_only` (was `read_write`) |
| Cluster add/remove node | — | ✅ | — | — | — | ✅ `admin` (was `read_write`) |
| **`/health`** | ✅ | ✅ | ✅ | ❌ **two shapes — open** | ✅ | ✅ unauthenticated |

Intentional differences, all modelled in the contract rather than silently
forked: `log_index`/`deduplicated` are omitted on standalone; `as_of` /
`as_of_log_index` are standalone-only (no Raft log to address); `consistency`
is cluster-only (nothing to linearize against standalone); `307 + Location`
leader redirects exist only on cluster.

### Python SDK matrix (§40)

`python/tests/test_create_collection_contract.py`,
`test_index_lifecycle.py`, `test_python_remote.py`, `test_protocol_remote.py`.

| Capability | Sync | Async | No implicit `"default"` |
|---|---|---|---|
| Create / list / delete Collection | ✅ | ✅ | ✅ dimension + metric required |
| Insert / batch insert | ✅ | ✅ | ✅ |
| Search / multi-search | ✅ | ✅ | ✅ |
| Index build / status / drop | ✅ | ✅ | ✅ |
| Graph / GraphRAG | ✅ | ✅ | ✅ |
| Error parsing (`code`) | ✅ | ✅ | — |
| `filter_tag` | raises | raises | — |

`SyncRemoteClient` and `AsyncRemoteClient` were changed in lockstep; every
signature change landed on both.

### UI matrix (§41)

`cd ui && npx tsc --noEmit` — clean. Types come from `@valori/api-types`, so
field names are the contract's by construction: a UI file that invents a
response property, or misspells one, no longer compiles.

| Flow | Uses contract field names | No invented response properties |
|---|---|---|
| Create Collection | ✅ | ✅ |
| Insert | ✅ | ✅ |
| Search | ✅ | ✅ (`state_hash`/`queried_at` now typed as app-derived) |
| Index lifecycle | ✅ | ✅ |
| Graph / GraphRAG | ✅ | ✅ |
| Cluster status | ✅ | ✅ (`converged` typed as derived) |

There is no UI unit-test runner in this repo; `tsc --noEmit` plus the
generated types are the enforcement.

### Manual smoke

```bash
cargo run -p valori-node --features utoipa --bin valori-openapi | head -40
./scripts/generate-api-types.sh && cd ui && npx tsc --noEmit
```

---

## Follow-ups

Ordered by severity. Each is a real decision that did not fit inside an
API-stabilisation phase; none is an oversight.

1. **`/health` convergence** (P0, audit row 37). Two structurally different
   bodies; one client cannot parse both. Needs its own phase with a migration
   window because the UI, CLI wizard, compose healthchecks and MCP server all
   read it.
2. **Per-key `collection` lock** (P0, row 36). Stored, returned by
   `GET /v1/keys`, enforced by nothing. Either implement tenancy enforcement
   or remove the field — the first is a feature, the second is breaking.
3. **Move scope declaration next to the route** (finding 3). Prefix derivation
   produced errors in both directions. A per-route attribute, checked against
   `x-required-scope` for all 79 operations rather than the 11 pinned today.
4. **Object store `400 → 501`** (row 27). A 400 wrongly blames the caller for
   a deployment setting.
5. **Index enum convergence** (row 7) and raw `IndexState` exposure (row 9).
6. **Finish utoipa coverage** (§32 remainder): Indexes, Graph, GraphRAG,
   Memory, Ingest, Proof, Snapshots — then decide whether generation owns the
   file, which requires a way to carry prose descriptions through.
7. **Pagination on `/v1/operations` and `/v1/timeline`** (row 40). Unbounded
   reads over the whole event log.
8. **Remove the dead SDK methods** — `list_contradictions`,
   `resolve_contradiction`, `set_index` (rows 45, 46) — and decide the fate of
   `/v1/memory/meta/list`, which `ui/src/app/api/contradictions/route.ts`
   still calls and no node route serves.
9. **`namespace` → `collection` rename** (row 42) and the `/v1/namespaces`
   path rename (row 5) — both v2.
10. **SDK generation from the contract.** Explicitly **not started**. It
    should follow API convergence, not race it; generating clients against a
    contract the server does not yet meet would bake the drift into every
    language binding.
