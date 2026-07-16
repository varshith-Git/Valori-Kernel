# Phase E2 — NamespaceRegistry reconciliation

## Goal

Delete the duplicate `NamespaceRegistry` struct from `engine.rs` and make
`valori-metadata::CollectionRegistry` the single canonical implementation.

## Delivered

| File | What |
|---|---|
| `crates/valori-metadata/src/collection.rs` | Added `list() -> Vec<(String, u16)>` — returns all collections including implicit "default", sorted by id. Mirrors the old `NamespaceRegistry::list`. |
| `crates/valori-node/src/engine.rs` | Deleted `NamespaceRegistry` struct + impl (~60 lines). Added `use valori_metadata::CollectionRegistry`. Changed `namespaces: NamespaceRegistry` → `namespaces: CollectionRegistry`. Fixed `create` call: `CollectionRegistry::create` returns `Option<u16>` (not `Result`), so call sites now use `.ok_or_else(|| EngineError::InvalidInput(...))`. Renamed `drop_collection` → `drop` at the call site (guard for "default" stays in `Engine::drop_collection`). Updated serde type in `load_namespaces`. |

## Findings

1. `CollectionRegistry::create` returns `Option<u16>` where `NamespaceRegistry::create` returned `Result<u16, EngineError>`. The API intentionally differs — the metadata crate is `no_std`-friendly and doesn't know about `EngineError`. The gap is bridged at Engine call sites.
2. `ClusterNamespaceRegistry` in `valori-consensus` is a separate, near-duplicate with different hashing semantics (excluded from state hash). Left alone — it serves a different purpose (consensus-safe, excluded from Merkle root).
3. The sidecar JSON format is backward-compatible: both structs have identical fields (`map: HashMap<String, u16>`, `next_id: u16`).

## Validation

- `cargo check -p valori-node -p valori-metadata` — clean.
- Full workspace tests: see E4 validation (all three phases committed together).

## Follow-ups

- `ClusterNamespaceRegistry` in `valori-consensus` is still a hand-rolled duplicate. Future work: import `CollectionRegistry` there too (needs careful handling of the "excluded from state hash" invariant).
