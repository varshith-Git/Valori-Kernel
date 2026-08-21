# Valori SDK architecture

**Status:** Phase API-4A — platform foundation. Nothing is published yet.

This document is the constitution for `sdk/`. Read it before adding a method,
a generator, or a language.

---

## 1. What exists

```
sdk/
  generator.lock.json          pinned toolchain — no `latest` anywhere
  python/
    generated/valori_generated/   machine output   ← openapi-python-client 0.26.2
    handwritten/valori/           human code
    tests/
    api-coverage.yaml             74/74 operations, every claim CI-verified
    pyproject.toml                dist `valori`, import `valori` + `valori_generated`
  typescript/
    generated/valori-api.ts       machine output   ← swagger-typescript-api 13.12.6
    src/                          human code
    tests/
    api-coverage.yaml             74/74 operations
    package.json                  dist `@valori/sdk`
```

Both are generated from one source of truth:

```
api/openapi/valori-v1.yaml     OpenAPI 3.1.0 · 74 public operations · contract 1.0
```

which is itself emitted from `#[utoipa::path]` annotations on the axum handlers.
Nobody hand-writes the contract either.

## 2. The layering rule

```
handwritten   ergonomics · retry · error mapping · polling · auth
     ↓
generated     one function per operation, typed from the contract
     ↓
HTTP          httpx (Python) · fetch (TypeScript)
```

The arrow points one way, always.

* **`generated/` is machine-owned.** Never hand-edit it. To change it, change
  the Rust annotations, re-run the contract gate, then re-run the SDK's
  `scripts/generate.sh`.
* **Generated code must never import handwritten code.** Enforced by
  `sdk/python/tests/test_generated_contract.py::test_the_generated_package_does_not_import_the_handwritten_one`
  and by the equivalent assertion in
  `sdk/typescript/tests/generated-contract.test.ts`.
* **The handwritten layer must not open its own socket.** Retry, auth and
  idempotency are installed *underneath* the generated client — a custom
  `httpx` transport in Python, a `fetch` wrapper in TypeScript — so there is
  exactly one HTTP stack in each SDK.

### Why this split at all

A pure generated client is honest but unpleasant: `sync_detailed(client=…, body=CreateCollectionRequest(...))`
and a `Union[ApiError, CreateCollectionResponse]` you have to branch on by hand.
A pure handwritten client is pleasant but drifts from the contract the moment
someone forgets to update it.

Keeping both, with a one-way dependency, means the contract is enforced
mechanically and the ergonomics are still designed by a person.

## 3. What lives in the handwritten layer

Exactly the things a generator cannot decide:

| Concern | Where | Why it cannot be generated |
|---|---|---|
| Retry policy | `retry.py` / `retry.ts` | Whether a repeat is safe depends on idempotency semantics the contract does not express. |
| Error classes | `errors.py` / `errors.ts` | Which Python/TS class a `code` maps to is a taste decision; the generator only knows `ApiError`. |
| Polling | `resources/operations.*`, `index.wait` | Terminal states, intervals and deadlines are operational policy. |
| Resource shape | `resources/` | `client.collections["docs"].records.insert(...)` is a design, not a projection. |
| Auth | `transport.py` / `transport.ts` | One header, one place, never logged. |

## 4. What the handwritten layer must *not* do

* **Invent endpoints.** Every wrapper maps to an operationId in the contract.
  `Collections.get(name)` looks like a single-collection read but is
  documented and implemented as one `GET /v1/namespaces` plus a membership
  check, because the contract has no `GET /v1/namespaces/{name}`.
* **Hide a real distinction.** `memory_upsert` and `memory_upsert_vector` are
  separate operations on separate paths; both are wrapped, and neither is
  quietly aliased onto the other.
* **Retry a write blindly.** Without a `request_id` the SDK will not repeat a
  `POST /v1/records`, because a repeat can double-insert.
* **Swallow information.** Every typed error carries status, code, message,
  request id, headers and the raw body.

## 5. Coverage is a gate, not a document

`sdk/*/api-coverage.yaml` maps every operationId to how the SDK exposes it —
either a `wrapper:` (human-written ergonomic method) or `generated: true`
(reachable only through the generated client). `scripts/sdk-coverage-check.py`
fails the build when:

1. the contract has an operation the manifest does not name;
2. the manifest names an operation the contract does not have;
3. a manifest row declares an HTTP method/path that disagrees with the contract;
4. a declared `wrapper:` does not resolve to a real method in the sources;
5. the manifest's header totals disagree with its own entries.

Check 4 is the one that matters most: it is what stops the manifest from
becoming aspirational.

Today both SDKs are **74 wrapped / 0 generated-only / 74 operations**.

## 6. Reproducibility

`scripts/sdk-repro-check.sh` regenerates each SDK twice and diffs:

* generation *N* against generation *N+1* — proves no timestamps, no absolute
  paths, no random ordering;
* generation against the committed `generated/` — proves nobody hand-edited
  machine output and that the tree is not stale.

Two deterministic post-processing steps are applied by the generators and are
part of the reproducible output:

* Python: the ruff cache is deleted (machine-local state, not source) and an
  empty `py.typed` is written (`--meta none` skips the PEP 561 marker, and the
  generated code is fully annotated).
* TypeScript: `// @ts-nocheck` is stripped, so the generated surface is actually
  typechecked, and a "GENERATED FILE — DO NOT EDIT" banner is prepended.

## 7. Escape hatch

Every operation is wrapped today. If the contract grows one before the wrapper
lands, `client.raw` (Python) / `client.raw` (TypeScript) exposes the generated
client so nobody is tempted to stand up a second HTTP stack. Using it means
taking on the generated layer's raw error semantics.

## 8. Adding a language later

The bar for a third SDK is the same four artifacts: a pinned generator entry in
`sdk/generator.lock.json`, a `generated/` + handwritten split, an
`api-coverage.yaml` wired into `scripts/sdk-coverage-check.py`, and a workflow
that runs reproducibility · coverage · test · build. Go and Java are explicitly
out of scope for API-4A.

## 9. Relationship to `python/valoricore`

`python/valoricore` is the **embedded** SDK: it binds the Rust kernel in-process
through PyO3 and does not speak HTTP. `sdk/python` is the **remote** SDK. They
are independent distributions, can be installed side by side, and neither is a
replacement for the other. API-4A did not touch `valoricore`.
