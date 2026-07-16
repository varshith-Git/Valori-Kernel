# Phase S14 — Shard count in the project wizard (UI)

Branch: `Node-scaleup` (S13 `e0f36f4` merged — S13 was the blocking prerequisite: shard count could not be safely exposed while shards ≥ 1 silently discarded their audit trail).

## Goal

Expose horizontal scaling in the project-creation wizard: alongside the replication factor shipped earlier, a "Shards" control that splits collections across N independent Raft groups per node. This is the first UI surface for `VALORI_SHARD_COUNT` — until now, sharding (built across S1-S13) was entirely invisible to users.

## Delivered

**Architecture note that kept this small:** one `valori-node` process runs all N shards internally (single `VALORI_SHARD_COUNT` env var; all shards share the node's HTTP port and gRPC listener). So "3 replicas × 2 shards" is still exactly 3 processes — no port-allocation or process-topology changes were needed, just one env var threaded through the existing spawn config.

- **`ui/src/lib/server/projects.ts`** — `ProjectEntry` gains `shardCount: number` (default 1); the `migrateEntry()` shim synthesizes `shardCount: 1` for legacy manifest entries (same in-memory pattern as the earlier `replication`/`nodes` migration — verified live against the user's real pre-existing projects). `createProject()` pins `shardCount` to 1 unless `replication === 3` (sharding is a cluster-only concept — the standalone binary path has no shard concept to receive the env var), and caps it at 16.
- **`ui/src/lib/server/process-manager.ts`** — `LaunchConfig`/`startProjectNodes` gain `shardCount`; `startNode` sets `VALORI_SHARD_COUNT` only inside the `clusterMembers` branch and only when > 1, so single-node spawns and shard=1 clusters are byte-identical to before. Also logs `shards=N` in the launcher output.
- **`ui/src/app/api/projects/route.ts`** — `POST` accepts `shardCount` (400 unless integer 1-16); **`open/route.ts`** passes `entry.shardCount` into `startProjectNodes`.
- **`ui/src/components/projects/CreateProjectDialog.tsx`** — "Shards" button group (1/2/4/8), rendered only when "3-Node Cluster" is selected, with helper copy that also honestly states the current limitation: "Proof and Timeline currently only reflect the default shard; per-shard views are a planned follow-up." Client-side, `shardCount` is pinned to 1 when Single Node is selected regardless of the control's last value (mirrors the server-side pin).
- **`ui/src/lib/hooks/useProjectManifest.ts`** — `ManifestProject.shardCount` + `create()` param; **`Sidebar.tsx`/`page.tsx`** call sites pass it through; Home's project-card chip appends `· N shards` when > 1.

## Findings

- The `pm` ProcessManager singleton survives Next.js hot-reload (`global.__valori_pm__`), so **method/shape changes to it require a full dev-server restart**, not just a file save — a stale singleton was also the root cause of the user's `pm.startProjectNodes is not a function` errors and the "documents gone after reopen" scare (the node never started, so there was nothing to query; the data on disk was always intact). Second time this bit us in two days; documented here so it stops being rediscovered.
- Confirmed empirically what the S13 test had asserted from `shard_path()`'s code: at `shard_count > 1`, shard 0's files are also suffixed (`events-n1-shard0.log`), not left at the bare configured name.

## Validation

```
npx tsc --noEmit     # clean
npm run build        # clean (48 routes)
```

Live end-to-end smoke test against the running dev server:

1. `POST /api/projects {"name":"shardtest","dim":128,"replication":3,"shardCount":2}` → manifest entry persisted with `shardCount: 2`, ports 4010-4012/4110-4112 allocated.
2. `POST /api/projects/shardtest/open` → 3/3 nodes healthy, leader elected. `ps eww` confirmed **all 3 processes received `VALORI_SHARD_COUNT=2`**.
3. Created collection `tenant-a` (ns 1 → shard 1), upserted a 128-dim vector.
4. On-disk proof of S13+S14 together: project dir contains `events-n{1,2,3}-shard{0,1}.log` (6 audit logs, one per node×shard) and matching `raft-*-shard*.redb` files. `valori-verify` chain-validated **both** shard logs: shard 1's log = 4 events with a valid BLAKE3 chain head (the routed upsert), shard 0's log = 1 event (the namespace-registry write). Exactly the isolation the design promised.
5. `DELETE` → all processes stopped, ports freed, dir removed; pre-existing projects (`firstone`, `qwerty`) untouched throughout and correctly report `shardCount: 1` via the migration shim on the user's own running server.

## Follow-ups

- Per-shard Proof/Timeline views (`/v1/proof/event-log?shard=N` or a composite Merkle receipt) — the read-side gap deliberately carried forward from S13, now also disclosed directly in the wizard's helper text.
- The Shards control is create-time-only (immutable after creation), matching `VALORI_SHARD_COUNT`'s backend semantics — resharding an existing project is not supported anywhere in the stack and is not claimed to be.
