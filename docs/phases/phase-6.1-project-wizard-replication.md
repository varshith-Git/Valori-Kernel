# Phase 6.1 — Replication factor in the project-creation wizard

Branch: `Node-scaleup`. Extends Phase 6 (persistent, isolated UI projects). Shipped in the same commit as Phase S14 (the two features share most files).

## Goal

A UI/UX audit found the single biggest gap versus the intended product flow: project creation (Home / Sidebar "New Project") always booted exactly one standalone node, while multi-node clusters lived only on the buried `/launch` power-user page — two disconnected systems with different config dirs, different dimension lists, and no bridge between them. This phase makes replication a first-class part of project creation: "Single Node" or "3-Node Cluster", one dialog, one manifest.

## Delivered

- **`ui/src/lib/server/projects.ts`** — `ProjectEntry` gains `replication: 1 | 3` and `nodes: ProjectNodeEntry[]` (id, httpPort, raftPort), with `port` kept as a back-compat alias of `nodes[0].httpPort`. `readManifest()` migrates legacy single-port entries in memory (no manual migration; verified against real pre-existing manifests). `allocateNodes()` replaces `allocatePort()`: single-node keeps the 3010-3999 range unchanged; cluster projects get a new dedicated 4010-4999 range with `raftPort = httpPort + 100` — never colliding with the Launcher's ad-hoc 3000-3009/3100-3109 range. `projectNodePaths()` suffixes per-node files (`events-n2.log`, `raft-n2.redb`) only for clusters; single-node filenames are byte-identical to before. Protect/unprotect (immutable-flag) iterate every node's files.
- **`ui/src/lib/server/cluster-config.ts` (new)** — `buildMembers`/`makeDefaultNodes`/`nextNodeConfig` extracted verbatim from `launch/page.tsx` so the Launcher and the project flow share one implementation. The `/launch` page keeps working unchanged (verified live).
- **`ui/src/lib/dimensions.ts` (new)** — one canonical 10-option dimension list, replacing the two divergent copies (dialog had 5 options, Launcher had 10).
- **`ui/src/lib/server/process-manager.ts`** — `startNode` gains an optional `trackingKey` param: cluster-project nodes are tracked by httpPort instead of their Raft id (1/2/3), preventing a real collision with the Launcher's own id-1/2/3 nodes in the shared singleton map. `startProjectNodes()` replaces `startProject()`, spawning all of a project's nodes with a computed `VALORI_CLUSTER_MEMBERS` string; env-var content (`VALORI_NODE_ID` etc.) still uses the Raft-semantic id.
- **API routes** — `POST /api/projects` accepts `replication` (400 unless 1|3); `GET` probes every node and returns an aggregate status (`running` only when all up; partial cluster surfaces as `error`) plus `nodesRunning`/`nodesTotal`; `open` starts all nodes and waits for **all** healthy (Raft needs 2-of-3 quorum for writes, so primary-only health is not enough); `close` snapshot-stops all nodes in parallel; `DELETE` stops all before removing.
- **`CreateProjectDialog.tsx`** — "Single Node" / "3-Node Cluster" selectable cards; `useProjectManifest`/`Sidebar`/Home call sites updated; Home's `StatusPill` shows "2/3 running" for clusters, card chip shows `· 3 nodes`.
- **`TopBar.tsx`** — unrelated 2px fix folded in: back-chevron was 15px next to the breadcrumb's 13px separator chevron; now both 13px (user-reported alignment complaint, verified in both themes).

## Findings

- `pm.nodes` being one shared `Map<number, ManagedNode>` across the Launcher and project flows was a latent collision trap the moment projects grew multi-node Raft ids — caught at design time (the `trackingKey` fix), not in production.
- The `pm` singleton survives Next.js hot-reload; adding methods to it requires a full dev-server restart (documented again in S14 after it bit twice).

## Validation

`npx tsc --noEmit` and `npm run build` clean. Live end-to-end against the dev server: created a 3-node project via the real dialog, 3 processes spawned with correct env (`VALORI_CLUSTER_MEMBERS`, per-node `VALORI_NODE_ID`/`VALORI_RAFT_BIND`), leader elected (verified via `/v1/cluster/status`: 3 voters, term 1), write on the leader locally readable on a follower, close→reopen with data intact (immutable flags applied at rest, quorum re-formed), delete removed everything. Pre-existing single-node projects behaved identically throughout (regression-checked live, including the manifest migration shim).

## Follow-ups

- Shard count exposure — delivered immediately after as Phase S14 (blocked on the S13 backend audit fix).
- A per-node breakdown view (reusing the Launcher's `NodeCard`) on the project detail page — the compact card intentionally shows only the aggregate.
