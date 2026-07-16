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
- **Auto-tier**: `IndexKind::Auto` starts as BruteForce and promotes to BQ then HNSW as record count grows; `auto_tier_check()` is called after every insert.
- **Drop flush**: `impl Drop for Engine` flushes pending EventCommitter writes.

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
