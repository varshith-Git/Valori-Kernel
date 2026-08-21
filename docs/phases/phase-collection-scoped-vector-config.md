# Phase — Collection-Scoped Vector Configuration

## Goal

Move dimension, metric, and index-algorithm choice from Project scope
(one value for the whole node process, shared by every namespace) to
Collection scope (one value per namespace), so a single `valori-node`
process can host collections of different dimensions and index kinds
side by side — without one OS process per collection, and without
breaking any existing project's on-disk data or wire contract.

This follows directly from the prior code-first architecture audit
(`Valori Configurable Index Architecture Audit`, this same session),
which found `KernelState.dim`/`KernelState.index` and
`Engine.index_kind`/`Engine.index` were single, process-wide scalars —
the actual structural blocker, not a metadata-schema naming issue.

## Delivered

### `valori-kernel` (`no_std`, unaffected by this constraint)

| File | Change |
|---|---|
| `src/index/mod.rs` | New `Metric` enum (`SquaredL2` only — the metric is now representable as data, no new math), `as_u8`/`from_u8` |
| `src/error.rs` | New `KernelError::NamespaceAlreadyConfigured` |
| `src/event.rs` | New `KernelEvent::ConfigureNamespace { namespace_id, dim, metric, index_kind }` — variant index 17, appended to both the hand-written `Serialize` impl and the `KernelEventHelper` deserializer without touching any existing variant's shape (adding fields to `AutoCreateNamespace` instead would have corrupted every already-persisted WAL entry of that variant) |
| `src/state/kernel.rs` | `NamespaceConfig { dim, metric, index_kind }`, `KernelState.namespace_configs: BTreeMap<u16, NamespaceConfig>` (`alloc`-only, matches the existing `meta` field's `no_std` pattern); `configure_namespace`/`namespace_dim`/`namespace_metric`/`has_namespace_config`; a shared `validate_dim_for_ns` helper now used by both `InsertRecord` and `AutoInsertRecord`; `ConfigureNamespace` apply arm; config cleanup on `DropNamespace` |
| `src/snapshot/encode.rs` / `decode.rs` | `SCHEMA_VERSION` 7→8. V8 appends a `namespace_configs` section after the existing V7 meta section, in both the buffered and streaming encoders. `hash_state_blake3`'s input domain was deliberately **not** touched — see Findings |
| `tests/{state_machine,format,snapshot_compat}.rs`, new fixtures `snapshot_v8_multi_collections.{bin,hash}` | New tests for configure/reject/roundtrip/backward-compat |

### `valori-domain` (std, API-boundary canonical types)

`Metric` added next to the already-canonical `IndexKind` (same `as_u8`
convention as the kernel mirror, since `valori-kernel` may not depend
on `valori-domain` — `docs/architecture/ownership.md`); `DomainError::UnknownMetric`.

### `valori-metadata` (control-plane persistence)

`CollectionVectorConfig { dim, metric, index }`; `Collection.vector_config: Option<..>`;
`CollectionRegistry.configs: HashMap<u16, CollectionVectorConfig>` +
`config()`/`set_config()`. Every new field is `#[serde(default)]` — an
existing `namespaces.json` sidecar or redb record deserializes with an
empty map, i.e. "no explicit config," automatically.

### `valori-engine` (the core of this phase)

`Engine.collection_indexes: HashMap<u16, Box<dyn VectorIndex+Send+Sync>>`
— a dedicated index per explicitly-configured collection, replacing
"one shared index, post-filtered by namespace" for anything that opts
in. `make_collection_index`, `ensure_collection_index`,
`namespace_effective_dim`, `sync_collection_indexes_from_state`,
`create_collection_with_config`. `apply_committed_event_ns`/
`post_apply_derived` rewritten to resolve a record's real namespace
(fixed a pre-existing `DEFAULT_NS`-hardcoding on delete along the way)
and route insert/delete to the right collection's index.
`search_l2_ns` checks `collection_indexes` first. `build_index()`
rebuilds each collection's index from only its own records and
excludes those records from the legacy global rebuild.
`load_namespaces()`/snapshot restore call `sync_collection_indexes_from_state()`.

BruteForce collections deliberately get **no** dedicated index object
— `KernelState::search_l2_ns`'s existing per-namespace linked-list
scan is already exact and namespace-isolated (confirmed via
`BruteForceIndex::search`'s own implementation, which scans the whole
pool, not one namespace — a dedicated `BruteForceIndex` per collection
would have been wrong, not just redundant).

### `valori-node` (API + both routers)

`CreateCollectionRequest` gains `dimension`/`metric`/`index` (all
`Option`, `#[serde(default)]`); `CollectionInfo` gains the same
(`skip_serializing_if` when absent). `CollectionOps` trait gains
`create(name, config: Option<CollectionConfigRequest>)` and
`config(namespace_id)`; shared `parse_collection_config` validates
(rejects `index` without `dimension`, defaults `metric`→SquaredL2,
`index`→`brute`). Both `server.rs` and `cluster_server.rs` updated —
cluster's `create()` issues a **second** Raft-committed
`ConfigureNamespace` write after the existing `AutoCreateNamespace`
write, so config replicates identically to every node.

### `valori-consensus`

`apply()` mirrors a successful `ConfigureNamespace` into
`namespace_registry.configs`; new `ValoriStateMachine::namespace_config()`
accessor — the cluster-mode counterpart of `Engine.namespaces.config()`.

### Mechanical fixes surfaced by the compiler

Four now-non-exhaustive `KernelEvent` matches needed a
`ConfigureNamespace` arm: `server.rs` (×2, timeline/audit description),
`cluster_server.rs`, `valori-cli/src/commands/timeline.rs`,
`valori-ffi/src/lib.rs`.

## Findings

1. **The kernel's own per-namespace search (`search_l2_ns`) already
   bypasses `KernelState.index` entirely** and reimplements brute
   force directly over the per-namespace linked list — `ActiveIndex`
   (kernel-native `BruteForceIndex`) is functionally inert for search
   (`on_insert`/`on_delete` are no-ops, `search` scans the whole pool
   ignoring namespace). This is what made "BruteForce collections need
   no dedicated index object" both correct and free.

2. **Dimension enforcement had to move into the kernel, not stay
   engine-only, after discovering `valori-consensus::ValoriStateMachine`
   applies `KernelEvent`s directly against `KernelState`, never through
   `valori-engine::Engine`.** An engine-only pre-check (the original,
   simpler plan) would have left every cluster-mode insert to an
   explicitly-configured collection unvalidated. `validate_dim_for_ns`
   now lives in the kernel specifically because it's the one thing both
   the standalone (`Engine`) and cluster (`ValoriStateMachine`) callers
   share.

3. **Cluster mode has no `Engine` at all** — `DataPlaneState.sm` is a
   bare `ValoriStateMachine`, not an `Engine`. This means the entire
   `collection_indexes` / dedicated-`dyn VectorIndex` mechanism this
   phase built only backs the standalone path. Cluster-mode dimension
   isolation is correct and replicated (kernel-level); cluster-mode
   index-algorithm diversity is not implemented — search for any
   cluster-mode collection currently uses the brute-force-equivalent
   `KernelState::search_l2_ns` path regardless of the requested
   `index`. Disclosed as a known limitation, not silently shipped.

4. **The kernel snapshot's record section stores exactly one vector
   byte-width for the whole file** (the header `dim`, used by the
   decoder to size every record's vector read). A collection whose
   dimension differs from every other namespace in the same snapshot
   survives in-memory (proven by every runtime test in
   `valori-engine`) but not a snapshot/restore cycle — the decoder
   reads a fixed-width slot per record regardless of which namespace
   it belongs to. Fixing this needs the snapshot to carry per-namespace
   config *before* the records section (today it's appended after, for
   V7 backward compatibility) — a real follow-up phase, not an
   extension of this one. Test `snapshot_roundtrip_known_limitation_mixed_dimensions`
   asserts the current failure explicitly.

5. **The three original `snapshot_v7_*.bin` fixtures needed
   hand-reconstruction**, not casual regeneration: bumping
   `SCHEMA_VERSION` to 8 and rerunning the existing `generate_snapshot_fixtures`
   ignored-test would have silently replaced the frozen V7-tagged
   committed bytes with V8-tagged bytes under the same filename,
   defeating the "committed bytes decode forever" guarantee those
   fixtures exist to test. Regenerated once, then mechanically patched
   (version byte 8→7, trailing empty V8 section stripped) back to
   byte-identical V7 output — verified via `git diff --stat` showing
   zero `.bin` diff against the pre-session working tree.

6. **`hash_state_blake3` was deliberately left untouched.** Including
   `namespace_configs` in the state-hash domain would have required a
   `STATE_HASH_DOMAIN_VERSION` bump (a much larger, separate decision)
   and broken every pinned hash test. Collection config is replicated
   and snapshotted, but is not (yet) part of the audited state-hash
   surface — consistent with the pre-existing, disclosed fact that
   index-algorithm choice was already outside that surface for
   Hnsw/Ivf/Bq before this phase.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   clean (no_std intact)
cargo test  -p valori-kernel                                    ~90 passed / 0 failed
cargo test  -p valori-domain                                    existing suite, unaffected
cargo test  -p valori-metadata                                  22 passed / 0 failed (+3 new)
cargo test  -p valori-engine                                    14 passed / 0 failed (+8 new)
cargo test  -p valori-consensus                                 existing suite, unaffected
cargo build --workspace                                         clean
cargo test  -p valori-node --test route_parity                  2/2
cargo test  -p valori-node --test collections \
            --test cluster_namespaces \
            --test api_graph_namespace_isolation \
            --test cluster_graph_namespace_isolation \
            --test cluster_search_namespace_isolation \
            --test search_k_bounds                              52/52 (every pre-existing
                                                                   namespace/collection/search
                                                                   isolation test, unaffected)
```

New tests worth naming directly: `two_collections_different_dimensions_are_independently_enforced`,
`cross_collection_isolation_brute_force`, `mixed_index_kinds_brute_and_ivf_coexist`,
`unconfigured_collection_keeps_legacy_single_dim_behavior`,
`reconfiguring_a_collection_with_a_different_dim_is_rejected`,
`snapshot_roundtrip_preserves_collection_config_same_dim_as_legacy`,
`snapshot_roundtrip_known_limitation_mixed_dimensions` (engine); kernel's
`configure_namespace_*`/`namespace_without_explicit_config_falls_back_to_legacy_dim_unchanged`/
`drop_namespace_clears_its_collection_config`; `snapshot_v8_multi_collections_decodes_forever`,
`snapshot_v8_with_no_explicit_collections_matches_v7_behavior`.

Not run this session: the full `cargo test -p valori-node` suite
(hundreds of tests across ingestion/graph/community/proof — none
touched by this diff), `cargo clippy`, `cargo fmt --check`.

Manual smoke test steps for a future session:

```bash
VALORI_DIM=384 valori-node &
curl -X POST localhost:3000/v1/namespaces \
  -d '{"name":"images","dimension":768,"index":"ivf"}'
curl -X POST localhost:3000/v1/namespaces \
  -d '{"name":"docs"}'   # legacy path — inherits VALORI_DIM=384
# insert a 384-dim vector into "docs" -> 200
# insert a 384-dim vector into "images" -> 400 DimensionMismatch
# insert a 768-dim vector into "images" -> 200
```

## Follow-ups

| Item | Phase |
|---|---|
| Cluster-mode per-collection ANN index (currently brute-force-equivalent regardless of requested `index`) | next |
| Reorder the kernel snapshot format (new schema version) so mixed-dimension collections survive restore | next |
| Python SDK: `create_collection(name, dimension=, metric=, index=)` | next |
| UI: collection-creation dialog gains dim/metric/index; project-creation dialogs (local + Cloud) drop them once the contract is considered stable | next |
| `CHANGELOG.md` / crate README updates | this repo, same session (see below) |
| Decide and execute `valori_domain::Project.dim`/`.index` deprecation path per `COMPATIBILITY.md` (not modified this phase — additive only) | next |
| Kernel-native per-namespace `BinaryQuantizationIndex` isolation (currently only the engine-level, `f32`, non-audited BQ is per-collection) | next |
| `STATE_HASH_DOMAIN_VERSION` bump to bring collection config into the audited hash, if/when that guarantee is wanted | next |
