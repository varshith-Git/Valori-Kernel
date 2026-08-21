# Known server issues

Defects in the **Valori node itself** that are visible through the public REST
API. These are *not* SDK bugs, and the SDKs deliberately do not work around
them: a client-side workaround would change the documented semantics of the
endpoint and hide the defect from everyone who is not using that SDK.

Each entry records a reproduction that does not involve any SDK, so the
distinction is verifiable rather than asserted.

| # | Issue | Endpoints | Status |
|---|---|---|---|
| 1 | [`metadata_filter` matches only the metadata sidecar](#1-metadata_filter-matches-only-the-metadata-sidecar) | `/v1/search`, `/v1/search/multi`, `/v1/memory/search*` | Open |

---

## 1. `metadata_filter` matches only the metadata sidecar

**Discovered:** Phase API-4C (raw `curl`, no SDK involved).
**Characterised:** Phase API-4D.
**Severity:** High — the documented primary path returns zero results.
**Classification:** Server bug. Not an SDK bug.

### Summary

`metadata_filter` post-filters search hits by looking each record up in the
**metadata sidecar store**, under the key `rec:{id}`. It never consults the
record's own committed metadata — the field written by `POST /v1/records` and
returned by `GET /v1/records/{id}`.

The result is that filtering on insert-time metadata — the path the SDKs,
the README and `CLAUDE.md` all present as the normal one — always returns an
empty result set, even when the predicate matches the stored value exactly.

### Reproduction (no SDK)

Against a standalone node with `VALORI_DIM=8`:

```bash
E=http://127.0.0.1:3000
V='[0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1]'

curl -s -X POST "$E/v1/namespaces" -H 'content-type: application/json' \
  -d '{"name":"mf","dimension":8,"metric":"squared_l2"}'

# {"author":"alice"} as UTF-8 JSON bytes, per the contract.
META='[123,34,97,117,116,104,111,114,34,58,34,97,108,105,99,101,34,125]'
curl -s -X POST "$E/v1/records" -H 'content-type: application/json' \
  -d "{\"values\":$V,\"collection\":\"mf\",\"metadata\":$META}"

curl -s "$E/v1/records/0?collection=mf"
# → {"id":0,...,"metadata":{"author":"alice"},"tag":0}     ← stored correctly

curl -s -X POST "$E/v1/search" -H 'content-type: application/json' \
  -d "{\"query\":$V,\"k\":5,\"collection\":\"mf\"}"
# → {"results":[{"id":0,"score":0.0}]}                     ← found without filter

curl -s -X POST "$E/v1/search" -H 'content-type: application/json' \
  -d "{\"query\":$V,\"k\":5,\"collection\":\"mf\",\"metadata_filter\":{\"author\":\"alice\"}}"
# → {"results":[]}                                         ← BUG: exact match, no hits
```

### Which write paths the filter can and cannot see

Measured in Phase API-4D against a live standalone node:

| How the metadata was written | Readable via `GET /v1/records/{id}` | Matched by `metadata_filter` |
|---|:--:|:--:|
| `POST /v1/records` with `metadata` (insert-time) | yes | **no** |
| `PATCH /v1/records/{id}/metadata` | yes | **no** |
| `POST /v1/memory/meta/set` with `target_id: "0"` | — | **no** |
| `POST /v1/memory/meta/set` with `target_id: "rec:0"` | — | yes |

Only the last row works, and it requires the caller to know an internal key
prefix (`rec:`) that appears nowhere in the OpenAPI contract or the docs.

### Expected vs actual

**Expected.** A record whose metadata satisfies the predicate is returned by a
search with that `metadata_filter`, regardless of which documented endpoint
wrote the metadata.

**Actual.** Only records with a metadata-sidecar entry stored under the exact
key `rec:{id}` are ever returned. Everything else is filtered out, so the
common case yields an empty result set with no error and no warning.

### Root cause

`apply_metadata_filter` in `crates/valori-node/src/server.rs` (and the
equivalent closures in `cluster_server.rs` and `capabilities.rs`) resolve
metadata as:

```rust
let key = format!("rec:{id}");
match meta_store.get(&key) {
    Some(meta) => valori_search::matches_metadata_filter(&meta, f),
    None => false,          // ← no sidecar entry ⇒ hit is dropped
}
```

Two independent problems:

1. **Wrong store.** Insert-time metadata is committed inside the `InsertRecord`
   event and lives with the record, not in `MetadataStore`. The filter never
   looks there.
2. **Missing means excluded.** `None => false` drops any record without a
   sidecar entry. Even once (1) is fixed, this decision needs to be deliberate:
   "no metadata at all" and "metadata that does not match" are different, and
   only the second obviously warrants exclusion.

`PATCH /v1/records/{id}/metadata` writes to a third place again, which is why
it is also invisible to the filter.

### Why this was not fixed in Phase API-4D

§8 of the phase brief permits a narrowly-scoped Rust fix if the change is
trivial. This one is not:

* it spans **both execution paths** (`server.rs` and `cluster_server.rs`, ~8
  call sites, plus `capabilities.rs`) — per `CLAUDE.md`, missing one is a bug,
  not a follow-up;
* it needs a **precedence decision** that is a product question, not a
  mechanical one: when a record has both committed metadata and a sidecar
  entry, which wins, or are they merged;
* it needs the `None` case decided explicitly (see above);
* `PATCH /v1/records/{id}/metadata` has to be brought into whichever store the
  filter reads, or it stays silently unfilterable.

That is a design pass with its own test matrix across both routers, not a
one-line change, so it is recorded here rather than rushed.

### SDK impact

None of the SDKs work around this, by design.

* Python and TypeScript both send `metadata_filter` **verbatim** as the JSON
  object the contract specifies. The wire encoding on the client side is
  correct and is covered by unit tests in both suites.
* Neither SDK rewrites the predicate, injects a `rec:` prefix, or silently
  redirects the caller to the sidecar. Doing any of those would make the SDKs
  disagree with the documented API and with every non-SDK client.
* The real-node integration suites therefore **do not assert that a
  `metadata_filter` search returns hits**. They assert that the request is
  accepted and well-formed, with a comment pointing at this document. When the
  server is fixed, those assertions should be tightened.

### Suggested fix

Resolve a record's metadata for filtering as: the record's own committed
metadata, overlaid by the sidecar entry if one exists; treat "no metadata"
as not matching any non-empty predicate. Apply identically in `server.rs`,
`cluster_server.rs` and `capabilities.rs`, and make
`PATCH /v1/records/{id}/metadata` write where the filter reads. Add a
route-parity-style test so the two paths cannot drift.
