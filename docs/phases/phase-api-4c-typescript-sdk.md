# Phase API-4C — Official Valori TypeScript SDK

**Branch:** `main` (uncommitted)
**Contract:** `api/openapi/valori-v1.yaml` — OpenAPI 3.1.0, 74 public operations, gate PASS, `sdk_ready: true`
**Package:** `@valori/sdk` 0.1.0

---

## Goal

Finish the official TypeScript developer experience on top of the frozen 74-operation
contract: verify the layered SDK that Phase API-4A scaffolded actually works against a
**real node**, close the ergonomic gaps the spec names (§21 abort/timeout, §22
pagination, §2 examples), and correct anything the first pass got wrong.

No REST API was redesigned, no generated file was hand-edited, and no route was added
or removed. The operation count is unchanged at 74.

---

## Delivered

Phase API-4A had already built substantially more of `sdk/typescript/` than its own
report implied — the two-layer split, transport, auth, retry, error mapping, polling,
all 74 wrappers, 161 unit tests and the CI workflow were present and green. This phase
did **not** redo that work. What follows is only what changed.

### Fixed — `metadata` was encoded wrong on two write paths

`Records.insert` and `Records.insertBatch` both declared `metadata` as a JSON object
and passed it straight through. The contract says otherwise, and the node rejects it:

| Operation | Wire type | Old SDK behaviour | Result |
|---|---|---|---|
| `POST /v1/records` | `number[]` — opaque UTF-8 JSON **bytes** | sent a JSON map | **422** `invalid type: map, expected a sequence` |
| `POST /v1/vectors/batch-insert` | `(string \| null)[]` — UTF-8 JSON **strings** | sent an array of maps | would have failed the same way |
| memory upsert · metadata set | genuine JSON object | correct | ok |

The bug was masked by the `as unknown as SomeRequest` casts the wrapper bodies use to
reach the generated types, which defeat exactly the checking that would have caught it.

Fixed in `src/resources/collection.ts` with two encoders at the domain→wire boundary
(§24), so callers still pass a plain object:

```ts
encodeMetadataBytes(m)  // → Array.from(new TextEncoder().encode(JSON.stringify(m)))
encodeMetadataString(m) // → JSON.stringify(m) | null
```

The node takes bytes rather than a map because this metadata is committed *inside* the
audit-chained `InsertRecord` event — the encoding has to be the caller's, byte for
byte. Encoding in the SDK preserves that while keeping the ergonomics.

Verified against a live node: the corrected byte array is accepted and returns a
receipt.

### Added — §21 per-call abort and timeout

- `CallOptions` gained `timeoutMs`, which overrides the client-wide value for one call.
  An explicit `signal` still wins.
- `Transport.params()` now honours it.
- `Collection.search()` and `Collection.graphrag()` take a trailing `CallOptions`:
  `docs.search(vec, 5, { queryText }, { signal, timeoutMs: 10_000 })`.

### Added — §22 pagination

`Graph.listAllNodes()` — an `AsyncGenerator` that walks `GET /v1/graph/nodes` by
offset and stops on a short page. Both forms are exposed: the raw page
(`graph.listNodes({ offset, limit })`) and the ergonomic walk.

A contract scan found this is the **only** endpoint with true pagination
(`offset` + `limit`); `graph_query` and `get_timeline` have a bare `limit`, which is a
cap, not a cursor. Nothing else got an iterator — §22 forbids inventing pagination the
server does not have.

### Rewritten — `README.md`

The committed README documented an SDK that **does not exist**. Every call in its
quickstart was wrong:

| README claimed | Actual |
|---|---|
| `new ValoriClient({ baseUrl, token })` | `{ endpoint, apiKey }` |
| `client.collections.create({ name, dimension, metric })` | `create(name, { dimension, metric })` |
| `client.insert({ collection, values })` | `docs.records.insert(values, …)` |
| `client.search({ collection, query, k })` | `docs.search(query, k, …)` |
| "Zero Hand-Written Types" | the entire ergonomic layer is handwritten |

Replaced with a quickstart checked against the real surface, plus sections on
collection-scoped resources, async index builds, multi-collection search, pagination,
abort/timeout, errors, retries, the wire-vs-domain `metadata` table, and the
`requestId` format constraint.

### Added — `examples/quickstart.ts` (§2)

A runnable end-to-end tour against a live node: health, collection lifecycle, insert
with metadata + `requestId`, batch insert, search, graph nodes/edges,
`listAllNodes()`, subgraph, graphrag, state proof, cleanup. Every call is a handwritten
wrapper — nothing reaches for `client.raw`.

`examples/**/*` was added to `tsconfig.json` `include`, so the example is typechecked
rather than decorative. That immediately caught four wrong field accesses (`node.id`
vs the real `node_id`) before the example was ever run.

### Fixed — CI would have failed the proof test

`.github/workflows/sdk-typescript.yml` started the integration node without
`VALORI_EVENT_LOG_PATH`, so `GET /v1/proof/event-log` answers *"Event log not
enabled"* and the proof case fails. The workflow now sets it to
`${RUNNER_TEMP}/events.log`.

### Fixed — two integration tests were wrong about the contract

- `request_id` was `it-${uniq()}`. `RequestId` is 32 hex characters; the node rejects
  anything else with a 422 *before* the write. Replaced with a valid generator, and the
  constraint is now documented in the README.
- `operations.execution(id)` was asserted to always return data. An execution record
  exists only for operations the **planner** ran; a plain kernel mutation has none and
  the node correctly answers 404. The test now accepts either typed data or a typed
  `NotFoundError`.

### Fixed — non-JSON error bodies lost the server's message (§19)

axum's own extractor rejections are `text/plain`, not `ApiError` JSON. The generated
client's `JSON.parse` fails on those and stores the resulting `SyntaxError` in
`response.error`; `Transport.#convert` accepted it as a real body, so the caller saw
`ValoriAPIError: request failed` with `code: undefined` and a `SyntaxError` as the
body. The actual server message — *"Failed to deserialize the JSON body: metadata:
invalid type…"* — was discarded, which is exactly what §19 forbids.

`#convert` now also falls back to `#readBody()` when `response.error instanceof Error`,
recovering the text. This is how the `metadata` bug was diagnosed at all: the fix makes
the next such bug legible instead of opaque.

### Housekeeping

`*.tgz` added to `sdk/typescript/.gitignore` — CI and local build verification both run
`npm pack`.

---

## Findings

1. **`metadata` has three different wire shapes for one domain concept.** Bytes on
   single insert, JSON strings on batch insert, a real object on the memory/metadata
   endpoints. Defensible — the first two are BLAKE3-chained and the third is a sidecar —
   but it is a genuine contract wart, and it is exactly what a domain layer is for.

2. **The `as unknown as X` casts in the wrapper bodies are load-bearing and dangerous.**
   They exist because `omitUndefined` erases optionality, but they disable the type
   checking that would have caught finding #1 at compile time. This is the single
   biggest structural weakness in the handwritten layer.

3. **A committed README can be confidently, comprehensively wrong.** Nothing in the
   build checks prose. The tests, typecheck, coverage manifest, reproducibility check
   and CI were all green while the README documented a fictional API.

4. **Typechecking the examples directory paid for itself immediately** — four real
   errors, found before the file was ever executed.

5. **Integration tests that never run are not tests.** All 18 were skipped locally and
   in the default CI job; two encoded false beliefs about the contract, and one of the
   two hid a real SDK bug. The `metadata` bug had been sitting in a green build.

6. **The CI integration job could not have passed** in its committed form, for an
   environment reason (`VALORI_EVENT_LOG_PATH`) unrelated to the SDK.

7. **`metadata_filter` on `POST /v1/search` matched nothing — node-side, not SDK.**
   Verified with raw `curl` against a standalone v0.3.0 node, bypassing the SDK
   entirely: a record whose metadata was written via `POST /v1/records` is returned by
   an unfiltered search and *not* by the same search with
   `metadata_filter: {"author":"Alice"}`. Writing the same key through the sidecar
   (`PATCH /v1/records/{id}/metadata`, which returns `200 {"ok":true}`) does not change
   the result. Both metadata paths are therefore unfilterable on this build. **Out of
   scope for this phase** — the SDK maps the field faithfully — but it is a documented
   contract feature that does not appear to work, and the README now says so rather
   than implying it does. Needs a node-side phase to confirm and fix.

8. **`DELETE /v1/namespaces/{name}` returns the generic `not_found` code**, not the
   more specific `collection_not_found` the `ErrorCode` enum defines. Callers must
   catch `NotFoundError`, not `CollectionNotFoundError`. Minor, but it makes the
   specific error class unreachable on the one operation you would most expect it on.

9. **The Python SDK has the same `metadata` bug** — verified by inspection during this
   phase, not assumed. Both SDKs were written from the same mistaken reading of the
   contract, and both had green builds. A bug class that survives in two independent
   implementations is a contract-clarity problem, not two coding slips.

---

## Validation

Everything below was actually executed, not inferred.

| Check | Command | Result |
|---|---|---|
| Typecheck | `npm run typecheck` | **clean** — src + generated + tests + examples |
| Unit tests | `npm test` | **171 passed, 18 skipped** (skipped = integration) |
| Full suite vs. real node | `VALORI_TEST_ENDPOINT=… npm test` | **186 passed, 3 skipped** (skipped = cluster-only) |
| Coverage manifest | `scripts/sdk-coverage-check.py --sdk typescript` | **PASS — 74 wrapped + 0 generated-only / 74** |
| Generated reproducibility | `scripts/sdk-repro-check.sh --sdk typescript` | **PASS** — byte-stable across two runs; committed output matches |
| Build | `npm run build` | ESM 91.1 KB · CJS 95.0 KB · `.d.ts` 146.7 KB |
| Pack | `npm pack --dry-run` | `valori-sdk-0.1.0.tgz` — 233.5 KB, 10 files |

Unit tests went 161 → 171: +9 in the new `tests/call-options.test.ts` (§21 abort,
per-call timeout, graphrag call options; §22 page walking, offset advance, short-page
stop, collection scoping) and +1 for batch metadata encoding. One existing test was
corrected — it had asserted the buggy `metadata` behaviour.

### Manual smoke test

```bash
VALORI_DIM=8 VALORI_BIND=127.0.0.1:3999 \
  VALORI_EVENT_LOG_PATH=/tmp/events.log ./target/debug/valori-node &
curl -sf http://127.0.0.1:3999/health

cd sdk/typescript
VALORI_TEST_ENDPOINT=http://127.0.0.1:3999 VALORI_TEST_DIM=8 npm test
npx tsx examples/quickstart.ts
```

The `metadata` fix was confirmed directly against the running node: the map form
returns 422, the encoded byte array returns a receipt.

---

## Follow-ups

| Item | Owner |
|---|---|
| Remove the `as unknown as X` casts from the wrapper bodies — type `omitUndefined` so it preserves optionality, or build bodies without it. Finding #2; this is what let the `metadata` bug through. | next SDK phase |
| Run the cluster integration cases. 3 remain skipped; they need a live 3-node cluster in CI. | CI phase |
| **Fix the identical `metadata` bug in the Python SDK — confirmed present, not suspected.** `sdk/python/handwritten/valori/resources/records.py` types `metadata` as `Optional[Mapping[str, Any]]` on `insert` (l.42/57) and `Optional[Sequence[Mapping[str, Any]]]` on `insert_batch` (l.68/77), passing both straight through. Both will 422 against a real node, exactly as TypeScript did. Left unfixed here only because this phase is TypeScript-scoped. | immediate next phase |
| Publish `@valori/sdk`. Blocked on the `npm` GitHub environment, npm-org ownership of the `@valori` scope, and a human release review. | release phase |
| **Investigate `metadata_filter` returning no matches** for either metadata write path (finding #7). Node-side; reproduced with raw `curl`, so it is not an SDK defect. | node phase |
| Decide whether `DELETE /v1/namespaces/{name}` should return `collection_not_found` rather than `not_found` (finding #8). | node phase |
| Consider a docs check that executes README code blocks, so finding #3 cannot recur. | later |

---

## Status

TypeScript SDK is **feature-complete and validated against a real node**. All 74
operations wrapped, 185 tests passing live, build and pack clean, generation
reproducible. **Not published** — see Follow-ups.
