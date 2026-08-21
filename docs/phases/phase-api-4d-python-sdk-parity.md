# Phase API-4D — Python SDK Parity, Type-Safety Hardening & Real-Node Validation

## Goal

Fix the confirmed `metadata` wire-encoding bug in the Python SDK (the same one
API-4C found and fixed in TypeScript), remove the unsafe casts that let that
class of bug compile in the first place, and validate **both** SDKs against a
real Valori node from a reproducible, disposable test environment.

Nothing published. Nothing committed.

---

## Delivered

### Python — the metadata bug and its fix

* **`sdk/python/handwritten/valori/_wire.py`** (new) — the single place a
  developer-facing mapping becomes something other than itself on the wire.
  Five encoders, one per wire shape the contract actually uses:
  `encode_metadata_bytes` (`list[int]`, `POST /v1/records`),
  `encode_metadata_string` / `encode_metadata_string_list` (`list[str|None]`,
  batch insert), `encode_metadata_object` and `encode_metadata_filter`
  (verbatim JSON objects, validated at the boundary).

  Serialisation is deliberately byte-identical to JavaScript's
  `JSON.stringify`: `separators=(",", ":")`, `ensure_ascii=False`,
  `sort_keys=False`, `allow_nan=False`. This is not cosmetic — `POST /v1/records`
  commits those bytes *inside* the `InsertRecord` event, so they are covered by
  the BLAKE3 audit chain. Python's default `json.dumps` would emit `", "`
  separators and `\uXXXX` escapes, so the same logical write from the two SDKs
  would produce two different state hashes.

* **`resources/records.py`** — `insert` now encodes to bytes, `insert_batch` to
  JSON strings (preserving `None` entries, which are meaningful), and
  `update_metadata` routes through the object encoder instead of an unchecked
  `dict(metadata)` conversion. `insert_batch`'s signature was corrected to
  `Sequence[Optional[Mapping[...]]]` to match the contract's nullable items.
* **`resources/memory.py`** — `upsert`, `consolidate`, `set_metadata` and both
  search paths route through the wire layer.
* **`resources/collections.py`** — `search` and `search_multi` `metadata_filter`
  route through `encode_metadata_filter`.

### Cross-SDK wire contract (§7)

* **`sdk/metadata-wire-fixtures.json`** (new) — 12 canonical cases (scalars,
  nested objects, arrays, unicode, empty, key-order) with their exact `json`
  and `bytes` forms. Read by **both** test suites, so parity is enforced
  mechanically rather than asserted twice by hand. A future SDK adds a reader,
  not a second table of expectations.

### TypeScript — cast elimination (§5)

All **13** `as unknown as` casts removed from `sdk/typescript/src/`. Replacing
them with `satisfies` proved 11 were pure noise and 2 were hiding a real
weakening: `create()` took `metric: string` / `index?: string` and
`index.build()` took `type?: string`, where the contract has closed enums.

* Public signatures now use `MetricValue` / `IndexKindValue` /
  `BuildableIndexKindValue` — template-literal unions **derived from the
  generated enums**, so callers still write `"hnsw"` but `"hsnw"` is a compile
  error, and adding a contract value widens them automatically.
* TypeScript enums are nominal, so one bridge is unavoidable. It is isolated in
  a single documented `asEnum` helper with **runtime membership validation**
  that makes the assertion sound — an invalid value throws before any request
  is made rather than being cast into a lie. This is the §5-sanctioned form.
* The two `metadata as Record<string, object>` casts were also removed, made
  unnecessary by the contract fix below.

### Contract fix (§12)

`MetadataSetRequest.metadata`, `UpdateRecordMetadataBody` and three sibling
fields were annotated `HashMap<String, Object>`, which utoipa renders as
`additionalProperties: {type: object}` — "every value must be an object". The
Rust type is `serde_json::Value` (any JSON) and the doc comment on the field
explicitly claims the generators should emit `Dict[str, Any]`. The schema was
narrower than both the server and its own documentation.

Consequence: `openapi-python-client` generated a per-value wrapper model, and
`client.records.update_metadata(7, {"a": 1})` raised
`TypeError: 'int' object is not iterable`. Scalar metadata values were
unrepresentable.

Fixed at the source — `value_type = HashMap<String, serde_json::Value>` in
`crates/valori-node/src/api.rs` (4 sites) and `server.rs` (1 site) — which
emits `additionalProperties: {}`. The contract was regenerated and both
`generated/` trees regenerated with their pinned generators. The contract diff
is exactly those 5 lines and nothing else.

### Real-node environment (§4/§9)

* **`scripts/sdk-integration-node.sh`** (new) — builds the node, picks a **free
  port**, creates a throwaway storage root, sets every `VALORI_*` variable
  **explicitly** (including `VALORI_EVENT_LOG_PATH`, `VALORI_SNAPSHOT_PATH` and
  the capacity knobs), waits for `/health`, runs the caller's command, then
  tears the node down and deletes the state — on success, failure or interrupt.
  It never touches a developer's existing node on `:3000`.
* Both `sdk-python.yml` and `sdk-typescript.yml` now call this one script
  instead of each carrying its own inline node-startup snippet. The Python
  workflow was missing `VALORI_EVENT_LOG_PATH` entirely — API-4C fixed that only
  on the TypeScript side, so the Python proof cases were testing an error path.

### Tests

| Suite | Before | After |
|---|---|---|
| Python unit | 235 | **307** |
| Python integration (real node) | 12 pass / 3 fail | **18 pass, 3 skipped** |
| TypeScript unit | 171 | **202** |
| TypeScript integration (real node) | 18 (3 skipped) | **21 (3 skipped)** |

New files: `sdk/python/tests/test_metadata_wire.py` (46 cases),
`sdk/typescript/tests/enum-boundary.test.ts` (8),
`sdk/typescript/tests/wire-parity.test.ts` (26).

### Documentation

* `docs/api/known-server-issues.md` (new) — the `metadata_filter` server bug.
* `docs/sdk/release-readiness.md` (new) — npm/PyPI verification results and the
  exact OIDC/trusted-publishing configuration API-4E must create.

---

## Findings

1. **The Python metadata bug was real and is fixed.** Reproduced directly:
   `InsertRecordRequest.from_dict({"metadata": {"a": 1}}).to_dict()` returned
   the dict unchanged, because the generated model's `from_dict` is permissive.
   The wrapper handed it a mapping and nothing objected.

2. **An existing test encoded the bug.** `test_resources.py` asserted
   `rec.body["metadata"] == {"a": 1}` — i.e. it asserted the wrong wire shape.
   Corrected to decode the bytes.

3. **The contract had a genuine bug** (see above): `additionalProperties:
   {type: object}` made scalar metadata values unrepresentable in the Python
   SDK. Fixed at the Rust source, not by patching generated code.

4. **Removing the TypeScript casts found a live type hole.** Two of the 13 were
   load-bearing: enum-valued fields were typed `string`. The SDK accepted
   `metric: "cosine"` — which is *not* a contract metric — and two committed
   tests were passing it. Note that `CLAUDE.md`'s SDK quick-reference examples
   also use `metric="cosine"`; those describe the separate `valoricore` FFI SDK
   and were left alone, but they are worth auditing.

5. **Three Python integration tests were wrong** and had been failing
   unnoticed because the suite is not part of the default run:
   * `request_id=f"it-{uuid4().hex[:8]}"` is not 32 hex characters — the node
     422s it, so the dedup path was never exercised.
   * `create_node(1).id` — `CreateNodeResponse` has no `id`; the contract field
     is `node_id`. The TypeScript suite already had this right.
   * `operations.execution()` was asserted non-null, but a 404 is a
     contract-valid answer for an operation with no execution record; the test
     depended on which operation happened to sort first.

6. **`metadata_filter` is broken in the server** — confirmed, characterised,
   documented, and *not* worked around. See below.

7. **Read-path asymmetry (not fixed).** Writes take a plain dict; reads hand
   back raw wire bytes or a generated attrs model, so callers must decode
   themselves. Both integration suites now carry a `decodeStoredMetadata`
   helper with a comment. Making reads symmetric is an API-4E API decision, not
   a silent change here.

8. **A stale-build trap cost real time.** Restoring a file with `mv file.bak
   file` preserves the *older* mtime, so cargo considered the crate unchanged
   and kept serving a stale `valori-openapi` binary. The contract gate failed
   with a divergence that did not exist in the source. `touch` on the sources
   resolved it. Worth knowing before trusting a "the generator disagrees with
   itself" failure.

---

## `metadata_filter` — server bug, not fixed

Fully characterised in `docs/api/known-server-issues.md`. Confirmed with raw
`curl`, no SDK in the path:

* A record inserted with `metadata` is stored correctly and readable via
  `GET /v1/records/{id}`.
* It is returned by an unfiltered search.
* It is **not** returned by a search whose `metadata_filter` matches it exactly.

Root cause: `apply_metadata_filter` resolves metadata as
`meta_store.get("rec:{id}")` — the metadata **sidecar** — and returns `false`
when absent. Insert-time metadata lives with the record, not in that store, and
`PATCH /v1/records/{id}/metadata` writes to a third place. The only path that
works is `POST /v1/memory/meta/set` with `target_id: "rec:0"`, which requires
knowing an internal key prefix that appears nowhere in the contract.

**Not fixed here** because it is not the trivial change §8 allows for: it spans
both execution paths (~8 call sites across `server.rs`, `cluster_server.rs` and
`capabilities.rs`), and it needs a deliberate precedence decision (record
metadata vs sidecar; "no metadata" vs "no match") plus a test matrix on both
routers.

Neither SDK works around it. Both send `metadata_filter` verbatim as the
contract specifies. Both integration suites **pin the broken behaviour** with an
assertion whose failure message says the server appears to be fixed and the test
must be tightened — so a fix cannot land silently.

---

## Validation

Everything below was run, not assumed.

| Check | Result |
|---|---|
| `./scripts/api-contract-gate.sh` | **PASS**, `SDK READY = YES`, 0 blockers, 74/74 operations |
| `cargo test -p valori-node --features utoipa --test openapi_generated` | 4/4 pass (byte-identical contract) |
| Python unit | **307 passed** |
| Python integration vs real node | **18 passed, 3 skipped** (cluster-only) |
| TypeScript `npx tsc --noEmit` | 0 errors |
| TypeScript unit + integration vs real node | **223 passed, 3 skipped** |
| `as unknown as` in `sdk/typescript/src/` | **0** |
| `cast(...)` / `type: ignore` in Python handwritten | 0 casts; 1 `type: ignore[arg-type]` on `dataclasses.replace` (legitimate) |
| `python -m build sdk/python` + `twine check` | wheel + sdist, both `PASSED` |
| Wheel installed into clean venv, imported | pass |
| `npm run build` | ESM + CJS + `.d.ts`, success |
| `npm pack --dry-run` | 10 files, 236.4 kB / 1.0 MB |
| Packed tarball installed, `import` and `require` | both pass |
| Consumer-side `tsc --strict` incl. enum rejection | pass |
| **Published** | **no — nothing published, no credentials created** |

The Python and TypeScript suites were also run **against the same node process
in a single harness invocation**, which is the §15 property: two independent
consumers of one contract, meeting one server.

---

## Follow-ups

| Item | Owner |
|---|---|
| Fix `metadata_filter` on both execution paths | server phase; see `docs/api/known-server-issues.md` #1 |
| Decide read-path metadata symmetry (decode on read?) | API-4E |
| Create `sdk-typescript-release.yml` / `sdk-python-release.yml` + npm & PyPI trusted publishers | **API-4E** |
| Verify `@valori/sdk` npm scope ownership and `valori` PyPI name availability | API-4E |
| Python CI test job runs a single interpreter; add a 3.9–3.13 matrix before first publish | API-4E |
| Audit `CLAUDE.md` / `valoricore` docs for `metric="cosine"`, which is not a contract metric | separate |
| `docs/publishing-pypi.md` documents a manual token flow for `valoricore`; should not be the template for `valori` | API-4E |
