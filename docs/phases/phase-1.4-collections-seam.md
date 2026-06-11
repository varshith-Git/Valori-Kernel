# Phase 1.4 — Collections seam

**Status:** done · on `multinode`
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.4

## Goal

Put the `collection` concept into the API surface **now**, while exactly
one collection exists — so that clients written today survive the
arrival of multi-tenancy and shard-by-collection (roadmap Phase 4)
without an API break. Pure seam: no engine changes, no storage changes.

## Delivered

- `api.rs`: `DEFAULT_COLLECTION = "default"` and
  `validate_collection(Option<&str>)`.
- Every data-path request accepts an optional `collection` field
  (`#[serde(default)]`, so existing clients are untouched):
  `InsertRecordRequest`, `DeleteRecordRequest`, `SearchRequest`,
  `BatchInsertRequest`, `CreateNodeRequest`, `CreateEdgeRequest`,
  `MemoryUpsertVectorRequest`.
- Every corresponding handler validates the field before touching the
  engine: omitted or `"default"` → proceed; anything else → **HTTP 400**
  with an error naming the collection and pointing at the Phase 4 plan.
  Validation happens before any lock/mutation, so a rejected request
  provably leaves state untouched.

Semantics chosen deliberately:

- `None` ≡ `"default"` — naming the default explicitly is always legal,
  so client libraries can start sending it today.
- Unknown collections are a **client error (400)**, not a silent create —
  auto-creating collections would turn typos into invisible data forks,
  the exact class of bug this database exists to prevent.

## Findings

None — by design the smallest phase so far. One ripple: adding a field
to a `Serialize`+`Deserialize` request struct breaks struct-literal
construction in tests (`api_batch_ingest.rs`) — worth remembering that
each future API field addition will touch literal constructors too.

## Validation

- Full suite: **170 tests passing, 0 failures**
- New `tests/collections.rs` (8 tests): unit validation, HTTP accept
  (omitted / `"default"`), HTTP 400 on unknown for insert and search,
  and rejected-request-leaves-state-untouched.

## Follow-ups

- `docs/api-reference.md` should document the field once the Python SDK
  starts sending it (SDK release cycle).
- Phase 4 replaces `validate_collection` with a real collection router —
  the call sites are exactly the seam it slots into.
