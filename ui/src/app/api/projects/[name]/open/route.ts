import { NextRequest, NextResponse } from "next/server";
import { getProject, projectNodePaths, unprotectProject, touchProject, type ProjectEmbedConfig } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";
import { setApiUrl } from "@/lib/server/connection";

const DIM_TO_EMBED: Record<number, ProjectEmbedConfig> = {
  384:  { provider: "ollama", model: "all-minilm",             endpoint: "http://localhost:11434/api/embed" },
  768:  { provider: "ollama", model: "nomic-embed-text",       endpoint: "http://localhost:11434/api/embed" },
  1024: { provider: "ollama", model: "mxbai-embed-large",      endpoint: "http://localhost:11434/api/embed" },
  1536: { provider: "openai", model: "text-embedding-3-small", endpoint: "https://api.openai.com/v1/embeddings" },
  3072: { provider: "openai", model: "text-embedding-3-large", endpoint: "https://api.openai.com/v1/embeddings" },
};

interface HealthBody {
  dim?: number;
  records?: { live?: number } | number;
  [k: string]: unknown;
}

async function probeHealth(port: number): Promise<HealthBody | null> {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/health`, {
      signal: AbortSignal.timeout(400),
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
  const url = `http://127.0.0.1:${primaryNode.httpPort}`;

  // Clear the immutable flag so nodes can append their WAL / write snapshots.
  unprotectProject(name);

  // ── Pre-probe every node: skip spawning the ones already reachable ───────
  // Handles externally-started nodes, nodes that survived a Next.js
  // hot-reload, and prevents double-spawn when a port is already occupied.
  const preHealth = await Promise.all(entry.nodes.map(n => probeHealth(n.httpPort)));

  // If a node is already running but was started WITHOUT an event log
  // (event_log_height is null/absent), snapshot + kill it so it respawns
  // below with the correct VALORI_EVENT_LOG_PATH.  This covers the common
  // case of a node that survived a Next.js hot-reload with stale env vars.
  for (let i = 0; i < entry.nodes.length; i++) {
    const h = preHealth[i] as (HealthBody & { event_log_height?: number | null }) | null;
    if (h && h.event_log_height == null) {
      const port = entry.nodes[i].httpPort;
      const { snapshotPath } = projectNodePaths(entry, entry.nodes[i].id);
      // snapshot-then-stop works for both managed and orphaned processes
      await pm.snapshotThenStop(port, snapshotPath);
      // CRITICAL: wait for the exit event so pm.isRunning() returns false.
      // Without this, startNode's idempotency guard (status === "running") will
      // skip the spawn even though the process is dead.
      await pm.waitForExit(port, 3000);
      preHealth[i] = null; // treat as down so spawn logic fires
    }
  }

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
    if (i > 0) {
      await new Promise(r => setTimeout(r, 150));
    }
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

  const embed = entry.embed ?? DIM_TO_EMBED[entry.dim];

  return NextResponse.json({
    ok: true,
    url,
    port: primaryNode.httpPort,
    reachable: !!primary,
    nodesReachable,
    nodesTotal: entry.nodes.length,
    ...(primary ?? {}),
    ...(embed ? { embed } : {}),
  });
}
