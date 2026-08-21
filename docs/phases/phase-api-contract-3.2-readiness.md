# Phase API-3.2 — Final API Contract Readiness Gate

## Goal

Decide, on evidence rather than on the 74/74 route equality Phase API-3.1
achieved, whether `api/openapi/valori-v1.yaml` can safely be handed to an SDK
generator — and close the three items API-3.1 left open (`x-status`, the
`/health` Redocly warning, the 26 non-public routes).

**Verdict: SDK READY = NO, 2 blockers.** Route coverage was already correct; the
*content* of the operations was not.

## Delivered

### Contract generation

| File | Change |
|---|---|
| `crates/valori-node/src/openapi.rs` | New `AuthResponsesAddon` modifier: attaches `401` and `403` to every operation with a non-empty `security` requirement, from the one place that knows the middleware's behaviour. New closed-domain schema mirrors `MetricSchema`/`MetricInputSchema`/`IndexKindSchema`/`IndexKindInputSchema`, registered in `components(schemas(...))`. |
| `crates/valori-node/src/*.rs` (5 files) | Removed 70 hand-written `(status = 401, ..., body = ApiError)` annotation lines — a router-layer fact duplicated per handler, and stated wrongly. |
| `crates/valori-node/src/api.rs` | `CreateCollectionRequest.dimension` and `.metric` marked `schema(required = true)`; `metric`/`index` given `value_type` enum mirrors; `CollectionInfo.metric`/`.index` likewise; `IndexRebuildRequest.index` likewise. Stale "except `default`" wording corrected to match the Phase 3.3 runtime. |
| `crates/valori-node/src/ingest.rs` | `/v1/ingest` `202` given its real body (new `IngestAcceptedResponse`); `413` and `500` documented; `400` description corrected. `/v1/ingest/update` given `413`, `500`, `502`. All ingest error paths routed through `valori_engine::error_response`, and the `IngestErrorBody` raw-`Vec<u8>` responses (served as `application/octet-stream`) removed. |

### Policy and tooling

| File | Change |
|---|---|
| `redocly.yaml` | `operation-4xx-response` documented; kept at `warn` **only** because `openapi-typescript` shares this config and aborts codegen on `error` severity. |
| `.redocly.lint-ignore.yaml` | New. Pins exactly `#/paths/~1health/get/responses`. Lint is now clean with no rule disabled. |
| `scripts/verify-api-route-contract.py` | Enforces 4xx coverage at full strength (exempting only `GET /health`), and adds `check_empty_bodies` — a reviewed allowlist of responses that are legitimately empty; anything else is reported as a defect. |
| `scripts/api-contract-gate.sh` | Step 3 relabelled to match what it actually checks; surfaces an "Untyped JSON responses" count and raises it as a computed blocker. |

### Documentation

`docs/api/x-status-decision.md`, `security-contract.md`, `non-public-routes.md`
(regenerated from the manifest), `operation-id-policy.md`, `sdk-readiness.md`,
`openapi-version-decision.md`, `api/README.md`, `CHANGELOG.md`.

## Findings

1. **The contract documented a `401` body that has never existed.** 70
   annotations declared `401` with `body = ApiError`. `auth_guard_v2` returns
   `Err(StatusCode::UNAUTHORIZED)` — a bare status, which axum renders with an
   empty body. An SDK trusting the contract would try to parse an empty body as
   JSON on every expired token.

2. **`403` was reachable on all 73 authenticated operations and documented on
   none.** The middleware returns it whenever a key's scope does not satisfy
   `required_scope`. It appeared nowhere in the contract.

3. **Three published documents were wrong.** `non-public-routes.md` listed
   `/v1/storage/*`, `/v1/snapshot/*`, and `/v1/memory/*` as non-public when the
   machine-generated manifest proves all 13 are `PUBLIC_SDK`; it also missed the
   real `OPERATOR_INTERNAL` set (`/v1/cluster/read-index`, `/v1/replication/*`)
   and the real `DEPRECATED` set (the unprefixed `/graph/*`, `/records`,
   `/search`, `/version`, …). `security-contract.md` claimed public operations
   carry only `read_only`/`read_write`; ten carry `admin`.
   `operation-id-policy.md` cited a `drop_collection` operationId that does not
   exist (it is `delete_collection`). All three were prose asserting what the
   manifest could have answered; they are now generated from or checked against
   it.

4. **`x-required-scope` was already trustworthy.** `VendorExtensionAddon` reads
   `crate::api_keys::required_scope` — the same function the middleware calls —
   so the contract cannot claim a scope the server does not enforce. This was
   the one piece of security metadata that needed no correction.

5. **`dimension` and `metric` were optional in the schema** while
   `parse_collection_config` has required them for every name since Phase 3.3.
   The field docs still carried the removed `"default"` exception.

6. **Closed domains crossed the wire as bare strings.** The accepted set and the
   emitted set genuinely differ — `FromStr` takes `l2`, `l2sq`, `bruteforce`,
   `mstg`; `as_str` never emits them — so they are modelled as two schemas each,
   not one.

7. **The blocker: Valori serves errors in two shapes.** `ApiError`
   (`{error, code}`) is what the contract declares, but ~126 sites across
   `server.rs` and `cluster_server.rs` emit a bare `{error}` with no `code`, and
   `GET /v1/crypto/status/{key_id}` returns a plain string. Sixteen documented
   responses are contentless as a result. This is systemic, not a typo, and is
   too large to fix surgically inside a contract-readiness phase.

8. **`x-sdk` is a constant.** It is `true` on all 74 operations, because an
   operation that would be `false` is never emitted. Recorded in
   `x-status-decision.md` so nobody mistakes it for a signal.

9. **`POST /v1/ingest` defaults `collection` to `"default"`** server-side
   (`ingest.rs`), and eight comparable sites exist in `cluster_server.rs`. This
   contradicts the "no implicit default collection" invariant. Out of scope
   here; noted for follow-up.

## Validation

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo build --workspace` | ok |
| `cargo test -p valori-node --features utoipa` | **454 passed, 0 failed** |
| `cargo test -p valori-kernel` | **177 passed, 0 failed** |
| `cargo test -p valori-storage` | **78 passed, 0 failed** |
| `cargo test -p valori-consensus` | **76 passed, 0 failed** |
| `cargo test -p valori-state` | **24 passed, 0 failed** |
| `cargo test -p valori-engine` | **18 passed, 0 failed** |
| `cargo clippy --workspace --all-targets --all-features` | 0 errors; 10 warnings, all pre-existing in `valori-engine`/`e2e_recovery.rs`, none in touched files |
| `cargo build -p valori-kernel --target wasm32-unknown-unknown` | ok (`no_std` invariant holds) |
| Three-way route equality | 74 == 74 == 74; 0 missing, 0 unexpected, 0 operationId mismatches, 0 classification leaks |
| OpenAPI determinism | 3 consecutive generations byte-identical; no timestamps or env-specific values |
| TypeScript determinism | 2 consecutive generations byte-identical |
| `cd ui && npx tsc --noEmit` | clean |
| `redocly lint` | valid, 0 warnings, 1 explicitly-pinned exemption |
| Python remote/contract tests | 7 passed |
| Python sync/async parity | 93 API methods each (`session` is a sync-transport property, not an API method) |
| `./scripts/api-contract-gate.sh` | **FAIL** — 1 step, 2 computed blockers |

Contract shape: 74 operations, 143 schemas (was 138), OpenAPI 3.1.0.

### Breaking-change classification (§20)

Operation count is unchanged at 74 — no route was added or removed.

| Change | Class |
|---|---|
| `403` documented on 73 operations | NON_BREAKING — documents existing behaviour |
| `401` body corrected to empty | NON_BREAKING on the wire; **contract-surface correction**, since the documented body never existed |
| `/v1/ingest` `202` body, `413`, `500`; `/v1/ingest/update` `413`/`500`/`502` | NON_BREAKING — additive |
| Ingest error bodies gain `code`; ingest-update errors now `application/json` instead of `application/octet-stream` | NON_BREAKING — additive field; the content-type change is a bug fix |
| 5 new schemas | NON_BREAKING — additive |
| `dimension` + `metric` become `required` | **BREAKING (SDK surface only)** — the wire is unchanged and the server already rejected requests without them, but a regenerated client makes the arguments mandatory |
| `metric`/`index` string → enum | **BREAKING (SDK surface only)** — every previously-accepted value is still accepted, including the aliases; a regenerated client narrows `string` to a union |

Both breaking entries are deliberate corrections of a contract that was
under-specifying what the runtime already enforced. Neither changes what the
server accepts or returns.

## Follow-ups

| Item | Owner |
|---|---|
| Unify runtime error bodies on `ApiError` across ~126 sites in `server.rs` / `cluster_server.rs` (`ingest.rs` is the reference pattern), then document the 16 responses and empty the allowlist | **Phase API-3.3** — blocks API-4 |
| `GET /v1/crypto/status/{key_id}` returns a plain `String` body; convert to `ApiError` | Phase API-3.3 |
| Remove the implicit `collection = "default"` fallback in `ingest.rs` and the 8 sites in `cluster_server.rs` | Phase API-3.3 |
| Consider removing the constant `x-sdk` extension | Phase API-3.3 |
| Official SDK generation from the contract | **Phase API-4** — start only when `sdk-readiness.json` reports `sdk_ready: true` |
