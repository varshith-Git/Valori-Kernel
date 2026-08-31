# Valori Python SDK

The official Python client for the **Valori Data Plane REST API v1** — a
deterministic vector + knowledge-graph store whose every mutation is
BLAKE3-chained and replayable.

> **This is not `valoricore`.** `valoricore` (in `python/`) is the *embedded*
> SDK: it binds the Rust kernel in-process through PyO3. `valori` (this
> package) is the *remote* SDK: it speaks HTTP to a running node. They are
> independent distributions and can be installed side by side.

| | |
|---|---|
| API contract | **1.0** (`api/openapi/valori-v1.yaml`, OpenAPI 3.1.0) |
| Operations covered | **74 / 74** — see [`api-coverage.yaml`](api-coverage.yaml) |
| Python | 3.9 – 3.13 |
| Transport | `httpx` |

## Install

```bash
pip install valori-client
```

## Quickstart

### 1. Cloud SaaS (`https://app.valori.systems`)

```python
from valori import ValoriClient

# Zero-config Cloud SaaS — passing api_key with no endpoint routes to https://app.valori.systems
client = ValoriClient(api_key="vlk_your_project_api_key")
```

### 2. Self-Hosted / Local Node

```python
from valori import ValoriClient

# Point to your local or self-hosted Valori node
client = ValoriClient(endpoint="http://localhost:3000")
```

### Basic Usage Example

```python
docs = client.collections.create("docs", dimension=384, metric="squared_l2")

docs.records.insert([0.1] * 384, metadata={"source": "paper.pdf"}, request_id="ins-1")

for hit in docs.search([0.1] * 384, k=5).hits:
    print(hit.id, hit.score)
```

Endpoint resolution, in order: the `endpoint` argument, then `VALORI_ENDPOINT`, then — only when an `api_key` was given and neither of those named an endpoint — Cloud SaaS. `api_key` falls back to `VALORI_API_KEY`. The key is automatically redacted from `repr()` and logs.

## Shape of the API

```
client.collections.create(name, dimension=…, metric=…)   client.operations.get(id) / .wait(id)
client.collections.list() / .get(name) / .delete(name)   client.index.config() / .rebuild()
client.collections["docs"]                               client.ingest.document(...) / .chunk(...) / .status(job)
                                                         client.tree.build(...) / .query(...) / .verify(...)
collection.records.insert / .insert_batch / .get         client.community.detect() / .search(...)
collection.records.delete / .soft_delete                 client.proof.event_log() / .state() / .timeline()
collection.records.update_metadata                       client.snapshots.save() / .restore(path)
collection.search(query, k)                              client.storage.manifest() / .upload_snapshot()
collection.graphrag(query_vector, k=…, depth=…)          client.cluster.status() / .health() / .role()
collection.index.build(type) / .status() / .wait()       client.meta.health() / .version() / .usage()
collection.graph.create_node / .create_edge / .query     client.crypto.key_status(key_id)
collection.memory.upsert / .search / .consolidate
```

Every one of these is a thin, human-written wrapper over a generated client
method. Nothing here invents an endpoint the contract does not have.

## Layers

```
valori/            handwritten — ergonomics, retry, error mapping, polling
    ↓
valori_generated/  machine output from api/openapi/valori-v1.yaml — DO NOT EDIT
    ↓
httpx
```

Regenerate the machine half with `sdk/python/scripts/generate.sh`. The arrow
never points the other way: generated code must not import `valori`.

If you need an operation before the ergonomic layer wraps it, use
`client.raw` — the generated client — rather than standing up a second HTTP
stack.

### `metadata` — one domain concept, three wire shapes

The split earns its keep on `metadata`. You always pass a plain `dict`; the SDK
encodes it into whichever form the contract requires, in `valori/_wire.py`:

| Operation | Wire type | Why |
|---|---|---|
| `POST /v1/records` | `list[int]` | opaque UTF-8 JSON **bytes**, committed inside the audit-chained `InsertRecord` event |
| `POST /v1/vectors/batch-insert` | `list[str \| None]` | UTF-8 JSON **strings**, one per vector (`None` = no metadata for that vector) |
| memory upsert · metadata sidecar · `metadata_filter` | JSON object | sent verbatim, not chained |

The first two are byte-committed to the BLAKE3 chain, which is why the node
takes bytes rather than a map: the encoding has to be the caller's, byte for
byte. The encoder is deliberately byte-identical to the TypeScript SDK's
(`JSON.stringify` semantics — no whitespace, real UTF-8, insertion order), so
the same metadata written from either SDK yields the same event bytes and the
same state hash. `sdk/metadata-wire-fixtures.json` pins that parity and is read
by both test suites.

```python
docs.records.insert([0.1] * 384, metadata={"author": "alice", "page": 4})
docs.records.insert_batch([v1, v2], metadata=[{"i": 0}, None])
```

Metadata is validated at this boundary: a non-mapping, a non-string key, a
value that is not JSON-serialisable, or `NaN`/`Infinity` raises `TypeError`
before any request is made.

> **Note on `metadata_filter` — known server bug.** `search(metadata_filter=…)`
> is part of the contract and the SDK sends the predicate verbatim. It currently
> matches **no** records: the node resolves the filter against the metadata
> *sidecar* only, keyed `rec:{id}`, so metadata written by `records.insert` or
> `records.update_metadata` is invisible to it. Confirmed with raw `curl`, with
> no SDK in the path — see
> [`docs/api/known-server-issues.md`](../../docs/api/known-server-issues.md) #1.
> The SDK deliberately does **not** work around this, because a client-side
> rewrite would change the endpoint's documented semantics.

> **Reading metadata back.** The write path takes a `dict`; the read path hands
> back the raw wire form (bytes for a record's committed metadata) or a
> generated model. Decode it yourself for now — making reads symmetric is a
> deliberate future API decision, not a silent change.

## Errors

Every 4xx/5xx becomes a typed exception carrying the full raw response:

```python
from valori import CollectionNotFoundError, ValoriAPIError

try:
    client.collections.get("nope")
except CollectionNotFoundError as exc:
    print(exc.status_code, exc.code, exc.message, exc.request_id, exc.body)
```

An error code this SDK does not recognise becomes a plain `ValoriAPIError`
with every field intact — an older SDK keeps working against a newer node.

## Retries

Disabled for unsafe writes by default, because a repeated `POST /v1/records`
without a `request_id` can double-insert.

```python
from valori import RetryPolicy, ValoriClient

client = ValoriClient(
    endpoint="http://localhost:3000",
    retry=RetryPolicy(max_attempts=5, backoff_initial=0.5),
)

# GET is always retryable; this write becomes retryable because it carries an
# idempotency key the node dedups on.
docs.records.insert(vec, request_id="ins-42")
```

`Retry-After` always wins over computed backoff.

## Long-running work

```python
op = client.operations.get(op_id)
op.wait(poll_interval=1.0, timeout=300)   # raises OperationFailedError / OperationTimeoutError

collection.index.build("hnsw")
collection.index.wait()                    # same ergonomics for index builds
```

## Development

```bash
pip install -e "sdk/python[dev]"
pytest sdk/python/tests                       # unit + wrapper + error + retry tests
VALORI_TEST_ENDPOINT=http://localhost:3000 \
  pytest sdk/python/tests -m integration      # against a real node
./sdk/python/scripts/generate.sh              # regenerate generated/
python3 scripts/sdk-coverage-check.py         # prove 74/74 coverage
```

## Licence

MIT OR Apache-2.0.
