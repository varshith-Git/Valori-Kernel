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
| `collection` | `Collection`, `CollectionRegistry` — name→NamespaceId; elevated form of the node's `NamespaceRegistry` |
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
- `CollectionRegistry` is the canonical name→NamespaceId mapping. The node's inline `NamespaceRegistry` will be replaced by this type in a future phase.
- `PlannerCache` lookup key is always the full triple `(OperationHash, PlannerFingerprintHash, PlanningContextHash)` — a partial match is a miss.
- `SnapshotCatalog::prunable(keep)` returns the records to delete, ordered oldest-first.
