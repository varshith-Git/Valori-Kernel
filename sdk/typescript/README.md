# `@valori/sdk` — Official TypeScript SDK

The official, fully-typed TypeScript client for the **Valori Data Plane API** —
a deterministic vector + knowledge-graph store with BLAKE3-verifiable receipts.

Every one of the contract's **74 operations** is reachable through a
handwritten, ergonomic wrapper (see [`api-coverage.yaml`](./api-coverage.yaml)).

---

## Installation

```bash
npm install @valori/sdk
```

Requires **Node.js ≥ 18** (for global `fetch` and `AbortSignal.timeout`).
Works in browsers and in any runtime with a WHATWG `fetch`; no Node built-ins
are imported at runtime.

---

## Quickstart

```ts
import { ValoriClient } from "@valori/sdk";

// Local Node (self-hosted):
const local = new ValoriClient({ endpoint: "http://localhost:3000" });

// Cloud SaaS (https://app.valori.systems) — apiKey with no endpoint routes there:
const client = new ValoriClient({ apiKey: "vlk_your_project_api_key" });

await client.health(); // → { status: "ok", mode: "standalone", ... }

// 1. Create a collection. dimension + metric are always required; there is no
//    implicit "default" collection and nothing is ever created silently.
const docs = await client.collections.create("tenant-acme", {
  dimension: 384,
  metric: "squared_l2",
});

// 2. Insert. Pass requestId to make the write dedupable *and* retryable.
//    requestId is a 32-hex-character UUID (dashes optional) — the node rejects
//    free-form strings with a 422 before attempting the write.
const { id } = await docs.records.insert(vector, {
  text: "Section 3.1 Training — AdamW optimizer",
  metadata: { author: "Alice", year: 2024 },
  requestId: crypto.randomUUID(),
});

// 3. Search. Ergonomic camelCase in, wire snake_case on the wire.
const hits = await docs.search(vector, 5, {
  queryText: "what optimizer is used?", // term-frequency hybrid rerank
  decayHalfLifeSecs: 86_400,
});
```

### Collection-scoped resources

`client.collections.get(name)` costs one list request and throws
`CollectionNotFoundError` if it is absent. `client.collection(name)` is the
unchecked handle and costs nothing — the contract has no
`GET /v1/namespaces/{name}`, so no endpoint was invented to make this nicer.

```ts
const docs = client.collection("tenant-acme");

docs.records; // insert · insertBatch · insertEncrypted · get · delete · softDelete · updateMetadata
docs.search; // POST /v1/search
docs.graphrag; // POST /v1/graphrag
docs.index; // build · status · wait
docs.graph; // createNode · createEdge · subgraph · query · listNodes · listAllNodes · …
docs.memory; // upsert · search · consolidate · contradict · get/setMetadata
```

Node-level resources hang off the client: `client.operations`, `client.ingest`,
`client.tree`, `client.community`, `client.proof`, `client.snapshots`,
`client.storage`, `client.cluster`, `client.crypto`, `client.meta`,
`client.index`.

### Async index builds

The handwritten layer owns polling; nothing busy-loops.

```ts
await docs.index.build("hnsw");
const status = await docs.index.wait({ pollIntervalMs: 500, timeoutMs: 120_000 });
// status.state is one of "active" | "failed" | "none"
```

Operations use the same primitive:

```ts
const op = await client.operations.get(operationId);
await op.wait(); // throws OperationFailedError on a failed terminal state
```

### Multi-collection search

```ts
await client.collections.searchMulti(vector, 10, ["docs", "notes"]);
```

Collections with an incompatible `dimension` or `metric` surface the server's
canonical error. The SDK never silently transforms a vector to make a search
succeed.

### Pagination

`GET /v1/graph/nodes` is the only offset/limit endpoint in the contract, so it
is the only one with an iterator. Both forms are available:

```ts
// raw page
const page = await docs.graph.listNodes({ offset: 0, limit: 100 });

// ergonomic walk
for await (const node of docs.graph.listAllNodes({ pageSize: 100 })) {
  console.log(node.id);
}
```

### Abort and timeout

Every wrapper takes an optional trailing call-options argument:

```ts
await docs.search(vector, 5, { queryText: "…" }, { signal, timeoutMs: 10_000 });
```

`timeoutMs: 0` disables the SDK-side timeout. An explicit `signal` always wins
over the timeout.

### Errors

Every failure is a `ValoriError`. HTTP failures are `ValoriAPIError` subclasses
carrying `code`, `status`, `requestId` and the raw body; an unrecognised code
maps to `ValoriAPIError` without losing anything.

```ts
import { CollectionNotFoundError, DimensionMismatchError, ValoriError } from "@valori/sdk";

try {
  await docs.search(vector, 5);
} catch (err) {
  if (err instanceof DimensionMismatchError) { /* … */ }
  else if (err instanceof CollectionNotFoundError) { /* … */ }
  else if (err instanceof ValoriError) console.error(err.code, err.status);
}
```

### Retries

Off by default for unsafe writes. `GET`/`HEAD` retry transient network and 5xx
failures; a write retries **only** when it carries a `requestId`, because that
is what makes the node dedup it. `429` respects `Retry-After`. Infinite retry is
never the default.

```ts
const client = new ValoriClient({
  endpoint,
  retry: { maxAttempts: 5, backoffInitialMs: 250 },
});
```

### The API key is never logged

`console.log(client)`, `JSON.stringify(client)` and `String(client)` all render
the key as `***`.

---

## Architecture

```
api/openapi/valori-v1.yaml     ← canonical contract, 74 operations
        ↓  swagger-typescript-api (pinned in sdk/generator.lock.json)
generated/valori-api.ts        ← machine-owned, disposable, DO NOT EDIT
        ↓
src/                           ← handwritten: transport · auth · retry · errors · polling · resources
        ↓
you
```

The arrow points one way: `generated/` never imports `src/`. The generated layer
is the **wire** representation; `src/` is the **domain** representation, which is
why the ergonomic layer takes `decayHalfLifeSecs` while the body on the wire
carries `decay_half_life_secs`.

The split earns its keep on `metadata`, which has three wire shapes for one
domain concept — and the SDK hides all three behind a plain JSON object:

| Operation | Wire type | Why |
|---|---|---|
| `POST /v1/records` | `number[]` | opaque UTF-8 JSON **bytes**, committed inside the audit-chained `InsertRecord` event |
| `POST /v1/vectors/batch-insert` | `(string \| null)[]` | UTF-8 JSON **strings**, one per vector |
| memory upsert · metadata set | JSON object | a metadata sidecar, not chained |

The first two are byte-committed to the BLAKE3 chain, which is why the node
takes bytes rather than a map: the encoding has to be the caller's, byte for
byte. The SDK does that encoding for you.

> **Note on `metadataFilter` — known server bug.** `search({ metadataFilter })`
> is part of the contract and the SDK maps it faithfully to `metadata_filter`,
> sending the predicate verbatim. It currently matches **no** records: the node
> resolves the filter against the metadata *sidecar* only, keyed `rec:{id}`, so
> metadata written by `records.insert` or `records.updateMetadata` is invisible
> to it. Confirmed with raw `curl`, with no SDK in the path — see
> [`docs/api/known-server-issues.md`](../../docs/api/known-server-issues.md) #1.
> The SDK deliberately does **not** work around this, because a client-side
> rewrite would change the endpoint's documented semantics.

### Enum-valued options

`metric`, `index` and the buildable index `type` are closed enums in the
contract, and the SDK types them as string unions derived from the generated
enums — so you write `"hnsw"`, but `"hsnw"` is a compile error, and an invalid
value passed from untyped JavaScript throws a `ValoriConfigError` before any
request is made.

```ts
await client.collections.create("docs", { dimension: 384, metric: "squared_l2" });
await client.collection("docs").index.build("hnsw");   // "hnsw" | "ivf" | "bq"
```

Regenerate with `npm run generate`. CI regenerates twice and diffs both runs
against the committed file, so a hand-edit under `generated/` fails the build.

---

## Development

```bash
npm ci
npm run typecheck   # tsc --noEmit, covers generated output too
npm test            # vitest
npm run build       # tsup → dist/ (ESM + CJS + .d.ts)
npm run generate    # regenerate generated/ from the contract
```

Integration tests run against a real node and skip without one:

```bash
VALORI_TEST_ENDPOINT=http://127.0.0.1:3000 VALORI_TEST_DIM=8 npm test
```

---

## License

MIT OR Apache-2.0.
