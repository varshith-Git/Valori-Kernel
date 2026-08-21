# valori-metadata

Control-plane persistence for the Valori platform. Owns everything that is not
in the kernel's hot-path (`KernelState`) but must survive process restarts:
project configuration, collection name mappings, shard topology, snapshot catalog,
execution history, and the planner cache.

Storage backend: [`redb`](https://github.com/cberner/redb) — the same embedded
key-value store used by the Raft log in `valori-consensus`.

## Modules

| Module | Contents |
|---|---|
| `project` | `Project`, `IndexKind`, `ProjectMode`, `ClusterNodeConfig` |
| `collection` | `Collection`, `CollectionVectorConfig` (dim/metric/index), `CollectionRegistry` — name→NamespaceId + optional per-collection vector config; elevated form of the node's `NamespaceRegistry`, already wired into `valori_engine::Engine.namespaces` |
| `shard` | `ShardTopology`, `ShardConfig`, `ShardMember` — cluster shard topology |
| `snapshot` | `SnapshotRecord`, `SnapshotCatalog` — snapshot catalog per (project, shard) |
| `history` | `ExecutionRecord`, `ExecutionRetentionPolicy`, `ExecutionStatus` — execution history stub |
| `planner_cache` | `PlannerCacheKey`, `PlannerCacheEntry` — planner cache stub |
| `db` | `MetadataDb` — redb-backed store for all of the above |
| `error` | `MetadataError`, `MetadataResult` |

## Database layout

One `MetadataDb` per installation (`~/.valori/metadata.redb`):

| Table | Key | Value |
|---|---|---|
| `projects` | project name | JSON `Project` |
| `collections` | `"project/collection"` | JSON `Collection` |
| `snapshots` | `"project/shard_id/ulid"` | JSON `SnapshotRecord` |
| `execution_history` | execution UUID | JSON `ExecutionRecord` |
| `planner_cache` | `"op_hash:fp_hash:ctx_hash"` | JSON `PlannerCacheEntry` |

## Dependency graph position

```
valori-core  ──┐
valori-wire  ──┴──► valori-metadata   ← this crate
                          │
                    valori-planner (A5)
                          │
                     valori-node
```

## Key invariants

- One `MetadataDb` file (`metadata.redb`) per valori installation — shared across all projects.
- `CollectionRegistry` is the canonical name→NamespaceId mapping, and is what
  `valori_engine::Engine.namespaces` actually is (not a separate inline type).
- `CollectionRegistry.configs: HashMap<u16, CollectionVectorConfig>` holds
  each collection's explicit dimension/metric/index, if it has one.
  `#[serde(default)]`: a `namespaces.json` sidecar or redb `Collection`
  record written before this field existed deserializes with an empty map —
  "no explicit config," which the runtime treats as "inherit the project's
  legacy dim/index." No migration step exists or is needed for old data.
  Dimension is immutable once set — `CollectionRegistry::set_config`
  rejects a conflicting redefinition rather than overwriting it.
- `PlannerCache` lookup key is always the full triple `(OperationHash, PlannerFingerprintHash, PlanningContextHash)` — a partial match is a miss.
- `SnapshotCatalog::prunable(keep)` returns the records to delete, ordered oldest-first.
