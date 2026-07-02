import { NextRequest, NextResponse } from "next/server";
import { getProject, projectNodePaths, unprotectProject, touchProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";
import { setApiUrl } from "@/lib/server/connection";

interface HealthBody {
  dim?: number;
  records?: { live?: number } | number;
  [k: string]: unknown;
}

async function probeHealth(port: number): Promise<HealthBody | null> {
  try {
    const r = await fetch(`http://localhost:${port}/health`, {
      signal: AbortSignal.timeout(2000),
    });
    if (!r.ok) return null;
    return (await r.json()) as HealthBody;
  } catch {
    return null;
  }
}

/** Extract the integer record count regardless of whether health.records is an
 *  object `{live, slots_used, …}` or already a plain number (legacy). */
function extractRecordCount(h: HealthBody): number | undefined {
  if (h.records == null) return undefined;
  if (typeof h.records === "number") return h.records;
  if (typeof h.records === "object") return h.records.live;
  return undefined;
}

// POST — ensure every node of the project is up, point the UI at the primary
// node, and record the open. Auto-starts nodes (each node's data dir is
// replayed by try_recover).
//
// For a 3-node cluster, wait for ALL nodes healthy (not just the primary)
// before returning: Raft needs 2-of-3 quorum for writes, so the primary node
// alone answering /health doesn't mean the cluster can take writes yet. If
// the budget expires, proceed anyway and report reachable counts honestly —
// same "start what we can" behavior as the single-node path already has.
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  const primaryNode = entry.nodes[0];
  const url = `http://localhost:${primaryNode.httpPort}`;

  // Clear the immutable flag so nodes can append their WAL / write snapshots.
  unprotectProject(name);

  // ── Pre-probe every node: skip spawning the ones already reachable ───────
  // Handles externally-started nodes, nodes that survived a Next.js
  // hot-reload, and prevents double-spawn when a port is already occupied.
  const preHealth = await Promise.all(entry.nodes.map(n => probeHealth(n.httpPort)));
  preHealth.forEach((h, i) => { if (h) pm.markRunning(entry.nodes[i].httpPort); });

  const anyToStart = entry.nodes.some((n, i) => !preHealth[i] && !pm.isRunning(n.httpPort));
  if (anyToStart) {
    const nodeSpecs = entry.nodes.map(n => {
      const { snapshotPath, eventLogPath, raftLogPath } = projectNodePaths(entry, n.id);
      return {
        id: n.id,
        httpPort: n.httpPort,
        raftPort: n.raftPort,
        eventLogPath,
        snapshotPath,
        raftLogPath,
        clusterInit: entry.replication > 1 && n.id === primaryNode.id,
      };
    });
    // startNode's own idempotency check means already-running nodes no-op here.
    pm.startProjectNodes({
      dim: entry.dim,
      index: entry.index,
      maxRecords: entry.maxRecords,
      nodes: nodeSpecs,
      shardCount: entry.shardCount,
    });
  }

  // ── Health-probe loop — up to 60 s (handles cargo-run cold-compile path) ──
  const results: (HealthBody | null)[] = [...preHealth];
  for (let i = 0; i < 120; i++) {
    if (results.every(h => h)) break;
    await new Promise(r => setTimeout(r, 500));
    await Promise.all(entry.nodes.map(async (n, idx) => {
      if (results[idx]) return;
      results[idx] = await probeHealth(n.httpPort);
    }));
    // If every not-yet-healthy node has errored out, stop waiting early.
    const stillWaiting = entry.nodes.some((n, idx) => !results[idx]);
    if (stillWaiting) {
      const allErrored = entry.nodes.every((n, idx) =>
        results[idx] || pm.getStatus(n.httpPort)?.status === "error"
      );
      if (allErrored) break;
    }
  }

  const primary = results[0];
  const recordCount = primary ? extractRecordCount(primary) : undefined;
  const nodesReachable = results.filter(Boolean).length;

  // Point the UI proxy at the primary node and record the open. Followers
  // forward writes to the leader internally, so pointing at the primary
  // (lowest-id / clusterInit node) rather than the actual current leader is a
  // safe, simple default.
  setApiUrl(url, primary ? { dim: primary.dim as number | undefined, records: recordCount } : undefined);
  touchProject(name, {
    lastOpenedAt: new Date().toISOString(),
    ...(recordCount != null ? { records: recordCount } : {}),
  });

  return NextResponse.json({
    ok: true,
    url,
    port: primaryNode.httpPort,
    reachable: !!primary,
    nodesReachable,
    nodesTotal: entry.nodes.length,
    ...(primary ?? {}),
  });
}
