# Phase API-4A — SDK Platform Foundation

**Date:** 2026-08-20
**Branch:** `main` (uncommitted working tree)
**Predecessor:** [API-3.3 — Public Operation Contract Completeness & SDK Preflight](./phase-api-contract-3.3-operation-completeness.md)

---

## Goal

Stand up the SDK platform architecture — generated + handwritten layers for
Python and TypeScript, pinned generation, coverage enforcement, CI, and package
builds — on top of the validated OpenAPI 3.1.0 contract API-3.3 produced.
Explicitly **not** to publish anything, and not to redesign the REST API.

---

## Delivered

### Generation toolchain (§2, §13)

| File | What it does |
|---|---|
| `sdk/generator.lock.json` | Pins every generator to an exact version. `openapi-python-client 0.26.2` + `ruff 0.13.3`; `swagger-typescript-api 13.12.6`. No `latest` anywhere in the pipeline. Also records the contract path, `openapi: 3.1.0`, `api_contract_version: 1.0` and `info.version: 1.0.0`. |
| `sdk/python/scripts/generate.sh` | Provisions the pinned toolchain into `.sdk-toolchain/python` (overridable via `VALORI_SDK_GEN_VENV`), generates with `--meta none` so packaging stays handwritten, then deletes the ruff cache and writes an empty `py.typed`. |
| `sdk/python/scripts/openapi-python-client.yaml` | `package_name_override: valori_generated` — the import line tells you which side of the boundary a symbol came from. |
| `sdk/typescript/scripts/generate.sh` | Generates with `--single-http-client --sort-routes --sort-types`, then strips the generator's unconditional `// @ts-nocheck` and prepends a "GENERATED FILE — DO NOT EDIT" banner. Both post-steps are fixed-string and idempotent, so byte-stability holds. |
| `scripts/sdk-repro-check.sh` | Regenerates each SDK twice; diffs run-vs-run and run-vs-committed. `--sdk <name>` to scope, `--write` to regenerate in place. Restores the committed tree on failure so a check never mutates the working tree. |

### Python SDK (§5–§10, §15)

```
sdk/python/
  generated/valori_generated/   276 files · 74 endpoint modules · 181 models
  handwritten/valori/
    client.py        ValoriClient — resource wiring, env fallback, redacting repr
    transport.py     the only place that touches the generated client
    errors.py        24 exception classes + error_for()
    retry.py         RetryPolicy + sync/async httpx transports + idempotency contextvar
    version.py       __version__, API_CONTRACT_VERSION, check_api_compatibility
    _models.py       wire-named kwargs → generated request models
    resources/       collections · records · index · graph · memory · operations · node
  tests/             8 files
  api-coverage.yaml  74/74
  pyproject.toml     hatchling; dist `valori`; Python 3.9–3.13
```

The developer API §5 asked for, verbatim:

```python
client.collections.create(...)      client.operations.get(id) / .wait(id)
client.collections.list() / .get("docs") / client.collections["docs"]
collection.records.insert / .get / .delete
collection.search(...)              collection.index.build(...) / .status() / .wait()
collection.graph.create_node / .create_edge
collection.graphrag(...)            collection.memory.*
```

* **Auth (§6).** `Authorization: Bearer <key>`, built in one place. `endpoint`
  falls back to `VALORI_ENDPOINT`, `api_key` to `VALORI_API_KEY`, so a key never
  has to be hardcoded. Redacted from `repr`/`str` on both the client and the
  transport; a test asserts the literal never appears.
* **Errors (§7).** The contract's closed 16-variant `ErrorCode` enum maps to
  exception classes; 429 maps by status (the contract has no `rate_limited`
  code) and carries `retry_after`. Unknown codes degrade to `ValoriAPIError`
  with status, code, message, `request_id`, headers and raw body all intact.
* **Retry (§8).** Installed as an httpx transport *under* the generated client.
  `GET`/`HEAD`/`OPTIONS` always eligible; writes only when they carry a
  `request_id`, surfaced as an `Idempotency-Key` header via a `ContextVar` so
  the key cannot leak into the next call. `Retry-After` wins over backoff and is
  clamped. Fully configurable and immutable (`RetryPolicy.evolve`).
* **Polling (§9).** `client.operations.get(id).wait()` and
  `collection.index.wait()`. Interval, deadline, terminal-state recognition and
  failure conversion all live in the handwritten layer; clock and sleep are
  injectable so no test sleeps.

### TypeScript SDK (§11, §16)

```
sdk/typescript/
  generated/valori-api.ts       74 methods, typechecked (no @ts-nocheck)
  src/
    client.ts      ValoriClient
    transport.ts   Transport + V1Data<K>/HealthData<K> return-type derivation
    errors.ts      24 error classes + errorFor()
    retry.ts       RetryPolicy + withRetry(fetch)
    resources/     collection.ts · node.ts · operations.ts
    version.ts
  tests/           8 files
  api-coverage.yaml  74/74
  package.json     @valori/sdk, ESM + CJS + .d.ts, Node ≥ 18
```

Same architecture, idiomatic idioms: promise-based, typed results, typed errors,
`client.collection("docs").records.insert(...)`.

One design note worth recording: wrapper return types are written as
`V1Data<"search">` rather than naming a `SearchResponse` interface. Several
operations have no named response interface in the generated file, and deriving
the type from the generated client means a wrapper's signature *cannot* drift
from the contract, because it is not a copy.

### Coverage manifests (§12)

`sdk/python/api-coverage.yaml`, `sdk/typescript/api-coverage.yaml`, and
`scripts/sdk-coverage-check.py`, which fails when: the contract has an operation
the manifest omits; the manifest names an operation the contract lacks; a
declared `http:` disagrees with the contract; a declared `wrapper:` does not
resolve to a real method in the sources; or the header totals disagree with the
entries.

Verified negatively — pointing one row at a non-existent method produces:

```
python: `get_cluster_health` claims wrapper `client.cluster.totallyMadeUp`,
        but no method named `totallyMadeUp` is defined in the handwritten sources
```

Current state: **74 wrapped, 0 generated-only, 74 contract operations**, both SDKs.

### CI (§1, §19, §21)

| Workflow | Contents |
|---|---|
| `.github/workflows/api-contract.yml` | Single invocation of `./scripts/api-contract-gate.sh`. No gate logic is duplicated — verified: no other workflow references the gate or its sub-scripts. Uploads `sdk-readiness.json` as an artifact and fails if the committed contract or generated wire types drifted. |
| `.github/workflows/sdk-python.yml` | `reproducibility` · `coverage` · `test` (3.9–3.13) · `integration` (real node) · `build` (wheel + sdist + `twine check` + clean-venv install-and-import, asserting the key does not leak into `repr`) · `publish` (tag-gated, environment `pypi`, OIDC). |
| `.github/workflows/sdk-typescript.yml` | Same shape: `reproducibility` · `coverage` · `test` (Node 18/20/22) · `integration` · `build` (`npm pack`) · `publish` (tag `sdk-ts-v*`, environment `npm`, `--provenance`). |

Both `publish` jobs reference GitHub environments that **do not exist**, so a
release tag runs everything and then stops at an approval gate nobody has
created yet. That is the intended end state for API-4A.

### Docs (§23)

`docs/sdk/sdk-architecture.md`, `docs/sdk/sdk-versioning.md`,
`docs/sdk/sdk-release-process.md`.

---

## Findings

1. **`GET /health` is legitimately unauthenticated, and the first TS auth test
   was wrong about it.** The contract declares `security: []` on that one
   operation, so the generated client does not invoke the security worker and no
   bearer is sent. The test now asserts that explicitly rather than being
   "fixed" by making the SDK send a header the contract says is not required.

2. **The generated TypeScript client discards error bodies on operations
   documented as bodiless.** `swagger-typescript-api` only populates
   `HttpResponse.error` when the operation has a response `format`. A failing
   `DELETE /v1/namespaces/{name}` therefore arrived with `error: null` even
   though the node had sent a real `ApiError` — the SDK would have reported
   "HTTP 404" with no code. Fixed in `Transport.#convert`, which now reads the
   unconsumed body via `response.clone()`. Caught by a wrapper test, not by
   inspection.

3. **The contract has no `collection_already_exists` code.** Creating a
   duplicate collection returns `conflict`. Rather than inventing a code or
   pretending the distinction does not matter, both SDKs raise
   `CollectionAlreadyExistsError` (a subclass of `ConflictError`) **only** from
   `collections.create`, where the operation makes the meaning unambiguous, and
   the class docstring says so.

4. **There is no `GET /v1/namespaces/{name}`.** `collections.get(name)` is
   therefore one `GET /v1/namespaces` plus a membership check, and
   `collections["docs"]` / `client.collection("docs")` is a zero-round-trip
   handle. Documented in both SDKs rather than papered over with an invented
   endpoint.

5. **`ruff` is load-bearing for Python reproducibility.** Without it in `PATH`
   the generator emits a warning and skips formatting, producing different bytes.
   It is now pinned in the lockfile alongside the generator, and its cache
   directory is deleted after each run because it would otherwise be the only
   thing that differs between two generations.

6. **`--meta none` skips the PEP 561 marker.** The generated Python code is
   fully annotated, but without `py.typed` an installed `valori_generated` is
   invisible to mypy. The generate script now writes an empty one — a
   deterministic post-step, so byte-stability is unaffected.

7. **`openapi-python-client` 0.26.2 needs Python ≥ 3.10**, while the SDK itself
   supports 3.9. The generate script prefers the newest interpreter it can find
   for *generation*; the generated code targets 3.9+. These are different
   requirements and are kept separate.

8. **Response-model strictness is a real ergonomic edge.** The generated
   `from_dict` raises `KeyError` on a missing required field. Several wrapper
   tests initially failed for this reason. They now answer `204 No Content`
   where the case only asserts on the request, rather than fabricating
   plausible-looking response fixtures a reader might mistake for contract
   truth. **This is a genuine open gap**: a node returning an unexpected body
   would surface a raw `KeyError` out of the SDK rather than a typed error.
   Logged as a follow-up.

---

## Validation

All numbers below are from actual runs on this working tree.

| Check | Command | Result |
|---|---|---|
| Python unit suite | `pytest sdk/python/tests -q` | **235 passed, 18 skipped** (skipped = integration, no node) |
| TypeScript suite | `npm test` (vitest) | **161 passed, 18 skipped** (skipped = integration) |
| TypeScript typecheck | `npx tsc --noEmit` | clean (src + generated + tests) |
| SDK coverage | `python3 scripts/sdk-coverage-check.py` | **PASS** — python 74/74, typescript 74/74 |
| Coverage gate negative test | manifest row pointed at a fake method | **FAIL**, exit 1, correct message |
| SDK reproducibility | `./scripts/sdk-repro-check.sh` | **PASS** — both SDKs byte-stable across two runs *and* matching the committed tree |
| Python build | `python -m build` | `valori-0.1.0-py3-none-any.whl` (329 KB) + `.tar.gz` (138 KB) |
| Python metadata | `twine check dist/*` | **PASSED** (both artifacts) |
| Wheel contents | `unzip -l` | two top-level packages, `valori/py.typed` present |
| TypeScript build | `npm run build` (tsup) | ESM 90 KB · CJS 94 KB · `.d.ts` 146 KB |
| TypeScript pack | `npm pack --dry-run` | `valori-sdk-0.1.0.tgz`, 227 KB, 10 files |
| Workflow syntax | `yaml.safe_load` on all three | valid; 1 + 6 + 6 jobs |
| Gate-logic duplication | grep for gate scripts across workflows | only `api-contract.yml` references them |

**Not run in this phase, and why:**

* `./scripts/api-contract-gate.sh` — not re-run. It requires a full
  `cargo build -p valori-node --features utoipa` plus network `npx` for Redocly;
  the contract is unchanged by this phase (no Rust file was touched), and
  `docs/api/sdk-readiness.json` from the API-3.3 run still reads
  `sdk_ready: true`, `gate_result: PASS`, `blocker_count: 0`. The CI workflow
  added here runs it on every API-affecting PR.
* `cargo test` — no Rust source was modified in this phase. `crates/` is
  byte-identical to how the phase started.
* `ui/` `tsc --noEmit` — no UI source was modified. `sdk/typescript` has its own
  independent `tsconfig.json` and was typechecked separately (clean).
* Integration suites — both are written, marked, and wired into CI, but need a
  running node. 18 Python and 18 TypeScript cases are skipped locally and will
  execute in the `integration` CI jobs, which build and start a real standalone
  node. §17 says "do not rely only on mocks"; the suites exist and are honest
  about not having run here.

### Manual smoke test

```bash
pip install -e "sdk/python[dev]"
python -c "
import valori
c = valori.ValoriClient('http://localhost:3000', api_key='secret-abc')
print(repr(c))                      # api_key='***' — the literal is absent
print(repr(c.collections['docs']))  # Collection(name='docs')
"
```

---

## Follow-ups

| Item | Owner phase |
|---|---|
| An unexpected response body raises a raw `KeyError` from the generated `from_dict` instead of a typed SDK error. Wrap model-parse failures in a `ValoriResponseError`. (Finding 8.) | API-4B |
| Publishing: create the `pypi`/`npm` GitHub environments, configure trusted publishing, and **reserve the names** — `valori` on PyPI and `@valori/sdk` on npm have not been checked for availability. | API-4C |
| Run the integration suites against a real cluster node (`VALORI_TEST_MODE=cluster`). The cases exist and are gated; only standalone is wired into CI so far. | API-4B |
| Async Python surface: `Transport.acall` and `AsyncRetryTransport` exist and the generated `asyncio_detailed` path is reachable, but no `AsyncValoriClient` resource layer is exposed and there are no async tests. | API-4B |
| Go and Java SDKs. Explicitly out of scope for API-4A. | later |
| Extract `sdk/python` and `sdk/typescript` into standalone repositories with history preservation. Deliberately **not** done (§22). | later |
