# valori-core

Zero-dependency, `no_std`-compatible type foundation for the Valori platform.
Every crate in the workspace depends on this crate; this crate depends on nothing
except `serde` and `thiserror` (both `no_std`-compatible).

## What lives here

| Module | Contents |
|---|---|
| `id` | `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`, `CollectionId`, `ExecutionId`, `ShardId`, `ClusterEpoch`; `DEFAULT_NS`, `NS_LIST_NIL`, `MAX_NAMESPACES` |
| `enums` | `NodeKind`, `EdgeKind` |
| `version` | `Version` — monotonic schema version counter |
| `error` | `CoreError`, `Result<T>` |

## `no_std` guarantee

```bash
cargo build -p valori-core --target wasm32-unknown-unknown
```

This must always pass. `std` is an opt-in feature; the default is `no_std`.

## Dependency graph position

```
valori-core   ← root, depends on nothing (except serde + thiserror)
  └── valori-kernel   (re-exports types from valori-core)
        └── valori-consensus
        └── valori-node
              └── valori-cli
```
