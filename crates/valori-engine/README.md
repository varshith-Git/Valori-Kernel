# valori-engine

Stateful engine orchestrator for the Valori platform. Coordinates `KernelState` (from `valori-kernel`) with persistence, indexing, metadata caching, and application-layer resources (tree cache, community store).

## Role in the stack

```
valori-node  (HTTP handlers, NodeConfig, AesGcmVault construction)
     │  EngineFromNodeConfig trait
     ▼
valori-engine  ← you are here
     │  Engine::with_config(EngineConfig)
     ├── valori-kernel   (KernelState, FxpScalar, BLAKE3 audit chain)
     ├── valori-index    (BruteForce / HNSW / IVF / BQ index)
     ├── valori-search   (ValoriReranker, decay)
     ├── valori-ingest   (EmbedConfig)
     ├── valori-rag      (TreeIndex, CommunityStore)
     ├── valori-metadata (CollectionRegistry)
     ├── valori-storage  (EventCommitter, WalWriter, ObjectStoreBackend)
     └── valori-state    (recover_from_events)
```

## Modules

| Module | Contents |
|---|---|
| `config` | `IndexKind`, `QuantizationKind`, `EngineConfig` — injected at construction; never parsed from env here |
| `error` | `EngineError` (HTTP-facing, implements `IntoResponse`), `CommitError` (persistence layer) |
| `metadata` | `MetadataStore` — thread-safe JSON key-value sidecar with atomic flush |
| `persistence` | `Persistence` enum — standalone durability funnel: EventLog / WAL / Ephemeral |
| `engine` | `Engine` struct, all impl blocks, `RecoveryMode`, `EngineHealth`, `PoolStats` |

## Construction

```rust
use valori_engine::{Engine, EngineConfig, IndexKind, QuantizationKind};

let cfg = EngineConfig {
    dim: 128,
    max_records: 1_000_000,
    max_nodes: 100_000,
    max_edges: 500_000,
    index_kind: IndexKind::Auto,
    quantization_kind: QuantizationKind::None,
    vault: Arc::new(my_vault),   // Arc<dyn KeyVault> — injected by caller
    ..Default::default()         // all Option<..> fields default to None
};
let mut engine = Engine::with_config(cfg);
```

`valori-node` wraps this via the `EngineFromNodeConfig` extension trait (defined in `valori-node/src/engine.rs`) so existing `Engine::new(&NodeConfig)` call sites keep compiling after importing the trait:

```rust
use valori_node::EngineFromNodeConfig;
let mut engine = Engine::new(&node_config);
```

## SOLID principles applied

| Principle | How |
|---|---|
| **SRP** | One file per concern (config, error, metadata, persistence, engine) |
| **OCP** | `VectorIndex` and `Quantizer` are trait objects; new index kinds don't change Engine |
| **ISP** | `KeyVault` (encrypt/decrypt/shred/key_exists) — narrow interface from valori-kernel |
| **DIP** | `EngineConfig` injects `Arc<dyn KeyVault>` and `Option<Arc<ObjectStoreBackend>>`; engine never constructs `AesGcmVault` |

## Key invariants

- **Apply before audit**: `DEDUP → KERNEL APPLY → AUDIT WRITE` inside `EventCommitter` — never violated here.
- **Namespace isolation**: enforced at `apply_committed_event_ns` (the only mutation path).
- **Q16.16 only**: all vector values clamped to `[-32768.0, 32767.99]` at the boundary; `FxpScalar` carries them through.
- **Auto-tier**: `IndexKind::Auto` starts as BruteForce and promotes to BQ then HNSW as record count grows; `auto_tier_check()` is called after every insert. Project-wide only — a collection created with `index=auto` is not itself auto-tier-promoted per collection in this phase.
- **Drop flush**: `impl Drop for Engine` flushes pending EventCommitter writes.
- **Every error carries a code** (Phase API-2): `EngineError::parts()`
  (`src/error.rs`) decomposes into the exact
  `(StatusCode, ErrorCode, String)` triple the wire carries, via an exhaustive
  match with no catch-all arm — so adding an `EngineError` or `KernelError`
  variant without mapping it is a compile error. `ErrorCode` has 16 variants
  and
  `valori-node/tests/api_contract.rs` diffs the enum against
  `api/openapi/valori-v1.yaml`, so a new code cannot ship undocumented.
- **Idempotency is engine-level, not transport-level** (Phase API-2):
  `dedup_lookup(&[u8; 16]) -> Option<u32>` / `dedup_record([u8; 16], u32)`
  give the standalone path the same bounded-FIFO dedup table the cluster state
  machine (`valori-consensus`) has always had, with the same token format and
  the same matching semantics. Single-record and batch inserts share it, so a
  repeated `request_id` resolves to the already-created record rather than
  duplicating it. Keep the two implementations behaviourally identical — a
  divergence here is a standalone/cluster parity break that no route-level
  test would catch.

## Collection-scoped vector configuration

`Engine.collection_indexes: HashMap<u16, Box<dyn VectorIndex + Send + Sync>>`
holds a dedicated index per explicitly-configured collection, alongside the
existing single project-wide `Engine.index`. A namespace with no entry here
uses the legacy global index/dim exactly as before this phase — existing
single-collection projects are unaffected.

```rust
// Creates a collection with its own dimension/metric/index, independent of
// every other collection on this node.
let images = engine.create_collection_with_config(
    "images", 768, valori_domain::Metric::SquaredL2, valori_domain::IndexKind::Ivf,
)?;
engine.insert_record_from_f32_ns(&vec_768, images)?;   // ok
engine.insert_record_from_f32_ns(&vec_384, images)?;   // DimensionMismatch

// `create_collection` (no config) still exists and behaves exactly as
// before — but as of Phase 3.2, the live HTTP API (`server.rs`) only ever
// calls it for the built-in `"default"` collection; every other name goes
// through `create_collection_with_config`. Calling it directly with an
// arbitrary name (as below) still works at the `Engine` level — there's no
// runtime guard against it — but is not a route the product surface
// exposes; treat it as reachable only for internal/test setup.
let docs = engine.create_collection("docs")?;
```

- BruteForce collections get no dedicated index object — they reuse
  `KernelState::search_l2_ns`'s existing exact, namespace-isolated
  per-namespace scan (a `BruteForceIndex` scans the whole pool regardless of
  namespace, so building one per collection would be both wasteful and
  wrong).
- `search_l2_ns` checks `collection_indexes` first (no post-filter needed —
  by construction a dedicated index only ever received that collection's own
  inserts), then the legacy global-index-plus-post-filter path, then the
  legacy per-namespace brute-force scan.
- `build_index()` rebuilds each collection's dedicated index from only its
  own records, and excludes those records from the legacy global rebuild —
  a record never lives in two indexes at once.
- **Known limitation**: this mechanism only backs the **standalone** path.
  `valori-consensus::ValoriStateMachine` (cluster mode) has no `Engine` and
  applies `KernelEvent`s directly against `KernelState` — cluster-mode
  collections get correct, replicated dimension isolation (enforced in the
  kernel) but always search via the brute-force-equivalent path, regardless
  of the requested `index`. See
  `docs/phases/phase-collection-scoped-vector-config.md`.

## Collection index lifecycle (Phase 4)

`Engine.index_states: HashMap<u16, CollectionIndexState>` holds the lifecycle
state for each collection's ANN index. States: `None → Building → Ready →
Active`, with `Failed` (preserves the previous `Active`) and `Retiring`
(superseded by a new `Active`).

```rust
// Start an HNSW build (async — returns the generation id immediately)
let gen = engine.start_index_build(ns_id, IndexSpec::hnsw_defaults())?;
// The background task calls finish_index_build / fail_index_build when done.

// Drop the active index (revert to exact search)
engine.drop_collection_index(ns_id);

// Read the current state
let state = engine.index_state(ns_id);
println!("{}", state.active_type());  // "hnsw" | "ivf" | "bq" | "none"
```

Key invariants:
- At most one generation is `Building` at a time; `start_index_build` returns
  `EngineError::InvalidInput` if `is_building()`.
- The `Active` generation is never interrupted — `finish_index_build` performs
  WAL catch-up (records inserted during the build are applied to the new index
  before activation), then atomically replaces `collection_indexes`.
- A `Failed` build leaves the previous `Active` generation unchanged.
- `index_states` is in-memory only; on restart, `ensure_collection_index`
  rebuilds synchronously and marks the generation `Active` with id 0.

## Index artifact persistence (Phase 4.1)

On successful `finish_index_build`, the engine now:
1. Writes the index bytes as an immutable `StorageKey::IndexArtifact` artifact.
2. Updates the collection's `CollectionManifest` with `active_index_generation`, `active_index_type`, and `active_index_base_lsn`.
3. GCs the retired generation's artifact via `StorageProvider::delete`.

On `drop_collection_index`, clears those manifest fields and deletes the artifact.

On `try_recover()` (StorageProvider path):
- Calls `try_restore_index_artifacts(recovered_lsn)` instead of blindly rebuilding every index.
- For each collection: reads the manifest; if `active_index_base_lsn == recovered_lsn`, loads the artifact and `restore()`s it (no rebuild); if stale or missing, rebuilds from `KernelState` records.
- BQ: always rebuilds (BQ snapshot returns empty bytes).

**Last line:** a node that restarts after building an IVF/HNSW index no longer pays the full rebuild cost every time.

## Snapshot format

The engine snapshot is a `VAL1`-magic binary blob:

```
[4]  magic "VAL1"
[4]  kernel_len (u32 LE)
[*]  KernelState blob (valori-kernel V6 snapshot)
[4]  metadata_len (u32 LE)
[*]  MetadataStore JSON
[4]  index_len (u32 LE)
[*]  VectorIndex blob
[4]  "NSRG" tag
[4]  ns_len (u32 LE)
[*]  CollectionRegistry JSON
[4]  "CRTS" tag
[4]  crts_len (u32 LE)
[*]  created_at map (bincode)
[4]  "BCRP" tag
[4]  bcrp_len (u32 LE)
[*]  reranker corpus (bincode)
```

## The `utoipa` feature (Phase API-3.1)

Optional and **off by default** — nothing in the runtime path needs it, and
enabling it adds a dependency the shipped binary does not carry.

```toml
utoipa = ["dep:utoipa"]
```

`valori-node`'s own `utoipa` feature turns it on transitively. It adds
`#[derive(ToSchema)]` to `index_manager::IndexBuildRequest` / `IndexStatusResponse`, which `valori-node` serialises verbatim from
`GET`/`POST /v1/namespaces/{name}/index`.

The point is that there is **one** type. The public OpenAPI contract references
the same struct the handler returns, so a field added or renamed here shows up
in the contract automatically instead of drifting away from a hand-copied mirror
in `valori-node/src/api.rs`. `scripts/verify-api-route-contract.py` and the
byte-equality test in `crates/valori-node/tests/openapi_generated.rs` enforce it.

### `IndexBuildRequest` is fully typed (Phase API-3.3)

Both of its fields used to describe nothing:

| Field | Was | Now |
|---|---|---|
| `parameters` | `serde_json::Value` with **no `type` at all** — `unknown` in TypeScript, `Any` in Python, and no way to discover a knob name | `IndexBuildParameters`: `m`, `ef_construction`, `ef_search` (HNSW) and `n_list`, `n_probe` (IVF) — the exact five `u64` keys both routers read |
| `index_type` (`type` on the wire) | bare `string` | `BuildableIndexKind` — the closed set `hnsw` / `ivf` / `bq` the build task matches on; its `_` arm returns `"unknown index type '<x>'"` |

`BuildableIndexKind` is deliberately **narrower** than the project-wide
`IndexKindInput`: `brute` and `auto` are project-level index selections, not
buildable per-collection ANN structures, so sending one here is an error and the
schema now says so. `null` remains valid and means *drop the index*; it is
carried by the `Option`, not by a variant.

Both are schema-only descriptions attached with `value_type` — the Rust fields
keep their runtime types, so deserialization and the existing validation (with
its 400s) are untouched.

