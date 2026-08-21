# Phase 3.2 — Eliminate Implicit Unconfigured Collection Fallback and Finalize Backend Collection Contract

## Goal

Close the last implicit-configuration gap left after the Collection-scoped
vector-config work: an unconfigured Collection could still be created for
any name via `POST /v1/namespaces {"name": "x"}` and would silently lock
onto whatever dimension its first insert happened to use. The final product
contract requires `name`/`dimension`/`metric` to be explicit at creation
time for every Collection except the built-in `"default"`, which is a
deliberate, disclosed zero-config exception, not an oversight.

## Delivered

- **`crates/valori-node/src/routes/collections.rs`** — `parse_collection_config`
  rewritten to require `dimension` and `metric` (400 with an explicit
  message if either is missing) and treat `index` as the only optional
  field (defaults to no dedicated ANN index, not a fake `BruteForce`
  object). `create_collection`'s `"default"` special-case is now checked
  *before* config parsing, using the raw payload fields, so `"default"`
  stays the one name that can be created bare, and any attempt to pass
  explicit config to `"default"` is rejected (400).
- **`crates/valori-node/src/api.rs`**, **`crates/valori-engine/src/engine.rs`** —
  doc comments on `CreateCollectionRequest`, `Engine::create_collection`
  (documented as reachable only for `"default"`), and
  `Engine::create_collection_with_config` (documented as the sole live
  path for every other name) rewritten to drop stale "inherits legacy
  `VALORI_DIM`/`VALORI_INDEX`" language — that fallback doesn't exist in
  the runtime anymore (confirmed by source grep, not assumed).
- **`crates/valori-engine/src/error.rs`** — `KernelError::DimensionMismatch`'s
  HTTP message no longer tells the caller to `set VALORI_DIM={expected}`
  (an env var that hasn't been read anywhere for several phases); it now
  explains that a Collection's dimension is fixed at creation and points
  at creating a new Collection instead.
- **`crates/valori-kernel/src/state/kernel.rs`** — `KernelState.dim` gained
  a classification doc comment: still active product logic (the `"default"`
  fallback + legacy V1–V8 snapshot decode target), not dead code, and — as
  of this phase — the *only* namespace the live REST API can reach without
  an explicit `configure_namespace` call is `"default"`. No behavioral
  change to the field or `validate_dim_for_ns`.
- **Test fixes for the new required-config contract** (dimension/metric
  added to every non-`"default"` collection-creation call; `"default"`
  itself left untouched): `crates/valori-node/tests/collections.rs` (8
  fixed + 9 new tests for the contract itself),
  `cluster_namespaces.rs` (21 fixed), `api_graph_namespace_isolation.rs`,
  `api_graph_cascade_delete.rs`, `cluster_graph_namespace_isolation.rs`,
  `cluster_graph_cascade_delete.rs`, `cluster_search_namespace_isolation.rs`,
  `graph_aware_reranking.rs`, `usage_endpoint_tests.rs` (2 fixed each,
  except `usage_endpoint_tests.rs`: 1).
- Ran `cargo fmt` across the workspace so `cargo fmt --check` passes clean
  (the repo had accumulated formatting drift from several phases'
  uncommitted work; this is non-semantic).

## Findings

1. **9 test files, 33 call sites total** were creating non-`"default"`
   collections with bare `{"name": "x"}` JSON and would have started
   failing with 400s the moment `parse_collection_config` went live. All
   fixed and re-verified (see Validation).
2. **`Engine::create_collection` has no runtime guard against non-`"default"`
   callers** — it's restricted to `"default"` only by HTTP-layer routing
   (`parse_collection_config` never reaches it for other names) and by doc
   comment, not by an assertion inside the function itself. Any caller
   that reaches `Engine::create_collection` directly (not through
   `server.rs`) — e.g. `valori-cli`, `valori-ffi`, or a test — can still
   create an unconfigured collection under any name. The 8 test files
   that call it directly (`e2e_recovery.rs`, `graph_cascade_delete.rs`,
   `hnsw_tests.rs`, `integration_tests.rs`, `persistence_index_tests.rs`,
   `ivf_recall.rs`, `persistence_tests.rs`, `vector_graph_retrieval.rs`)
   are internal engine tests, not evidence of a live product surface, so
   this was left as a documented gap rather than a guard added
   speculatively — the spec asked to isolate the no-config route from the
   HTTP layer, which is done; a kernel/engine-level guard is a separate
   design decision (does the engine need to reject bad input from *any*
   caller, or is that the router's job?) that wasn't asked for and isn't
   free of tradeoffs (it would also block internal callers that
   legitimately need `"default"`-style behavior for test setup).
3. **Real regression, found but not fixed —
   `ui/src/app/api/ingest/route.ts:413-419`**: the "ensure collection
   exists" call for document upload into a new collection sends
   `{"name": collection}` with no dimension/metric, which now 400s. The
   failure is swallowed by `.catch(() => {})`. It fails *safely*
   downstream — the node's `/v1/ingest` handler calls `resolve_collection`,
   which rejects with `"unknown collection 'x' — create it first"` rather
   than silently landing data in the wrong place — but the user sees a
   confusing error, not the real one. Fixing this properly means deciding
   whether `/v1/ingest` should auto-create the collection using the
   embedding's own output dimension (a real design question — does that
   conflict with "no unconfigured new collections"? No: the dimension
   would be explicit and correct, just chosen by the pipeline instead of
   the user), which is out of this phase's scope. Recommended for
   whichever phase next touches the ingest pipeline.
4. **Pre-existing, unrelated break — `crates/valori-cli/src/commands/import.rs`**:
   `ValoriClient::get_dim()` reads `/health`'s `"dim"` field, which no
   longer exists in the response (confirmed: `grep '"dim"' server.rs`
   returns nothing). `valori import` (both `run_qdrant` and `run_jsonl`)
   fails immediately at that call. This predates Phase 3.2 — it broke
   when `dim` was removed from `NodeConfig`/the health response in an
   earlier phase — and is unrelated to this phase's collection-config
   change (the CLI's `ensure_collection` call has the same bare-JSON
   problem `import.rs` never got as far as, since `get_dim()` fails
   first). Not fixed here: it needs the same per-collection redesign as
   finding 3, and touching it without that design would just move the
   failure point, not fix the tool.
5. **Python SDK and the local-project UI already match the new contract** —
   verified by reading source, not assumed: `SyncRemoteClient.create_collection`
   / `AsyncRemoteClient.create_collection` (`python/valoricore/remote.py`)
   already require `dimension` as a positional argument, default
   `metric="squared_l2"`, and correctly omit `index` from the payload when
   `None`. `ui/src/lib/hooks/useCollections.ts`'s `create()` already sends
   `{ name, dimension: dim, metric: "squared_l2" }` and only adds `index`
   when it's not the default. No SDK or UI changes were needed for the
   create-collection path itself.
6. **The required kernel-level test names from the spec already exist
   under different names** — `crates/valori-kernel/tests/state_machine.rs`
   already covers: wrong-dimension rejection
   (`insert_with_mismatched_dimension_is_rejected`), independent
   per-collection dimensions
   (`configure_namespace_sets_explicit_dim_independent_of_legacy_dim`,
   which configures namespace 7 at dim 1536 alongside namespace 0 at dim
   4 and asserts neither affects the other), and the legacy-fallback
   semantic (`namespace_without_explicit_config_falls_back_to_legacy_dim_unchanged`).
   No duplicate tests were added under the spec's suggested names — the
   assertions already exist and are already run in CI.

## Validation

- `cargo fmt --check` — clean (after `cargo fmt`).
- `cargo clippy --workspace --all-targets --all-features` — 0 errors, 2
  pre-existing warnings unrelated to this phase (`valori-engine/src/engine.rs:210`
  map_entry pattern, `valori-node/tests/e2e_recovery.rs:210` unused `mut`).
- `cargo build --workspace` — clean.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` — clean
  (no kernel behavior changed this phase; low-risk check, still run).
- `cargo test`, per crate:
  - `valori-kernel`: 16 passed
  - `valori-domain`: 4 passed
  - `valori-metadata`: 19 passed
  - `valori-storage`: 11 passed
  - `valori-state`: 5 passed, 1 ignored
  - `valori-engine`: 16 passed
  - `valori-consensus`: 10 passed
  - `valori-daemon`: 4 passed
  - `valori-node`: **368 passed, 0 failed**, across all 62 test binaries
    (full run, not truncated — verified with `grep "test result:"` across
    the complete log, all 62 lines say `0 failed`)
- `cd ui && npx tsc --noEmit` — clean, exit 0.
- Python: no `create_collection` usage found anywhere in
  `python/tests/` or `python/examples/` (`grep -rn create_collection`
  returns nothing), so nothing there could regress from this change;
  correctness verified by reading `remote.py` source instead (finding 5).
  The full `pytest` suite was not run — it requires a live node and/or
  the compiled PyO3 FFI module, neither of which this phase touches or
  needs to prove.

## Follow-ups

- **Fix `ui/src/app/api/ingest/route.ts`'s silent collection-creation
  failure** (finding 3) — needs a design decision on whether `/v1/ingest`
  auto-creates the target collection using the embedding's output
  dimension, or whether the UI should require the user to create the
  collection explicitly first with a dimension picker before uploading.
- **Fix `valori-cli import`** (finding 4) — `get_dim()` needs to move from
  reading `/health`'s removed `dim` field to reading the target
  collection's dimension from `GET /v1/namespaces`, and `ensure_collection`
  needs to send that same dimension (from the Qdrant/JSONL source) instead
  of bare `{"name": x}`. Pre-existing break, not introduced this phase.
- **Phase 4 — Mutable Collection Index Lifecycle** is the next phase in
  the sequence (background index build, HNSW↔IVF transitions). Do not
  start it automatically — this phase's mandate was audit-and-close-the-
  fallback only.
