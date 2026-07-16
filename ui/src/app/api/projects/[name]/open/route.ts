import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import { projectNodePaths, unprotectAll, touchProject, type ProjectEmbedConfig } from "@/lib/server/projects";
import { toLegacyEntry, resolveProjectsDir } from "@/lib/server/project-adapter";
import { pm } from "@/lib/server/process-manager";
import { setApiUrl } from "@/lib/server/connection";
import { errorResponse } from "@/lib/server/http";

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

async function probeHealth(port: number, timeoutMs = 1500): Promise<HealthBody | null> {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/health`, { signal: AbortSignal.timeout(timeoutMs) });
    if (!r.ok) return null;
    return (await r.json()) as HealthBody;
  } catch {
    return null;
  }
}

function extractRecordCount(h: HealthBody): number | undefined {
  if (h.records == null) return undefined;
  if (typeof h.records === "number") return h.records;
  if (typeof h.records === "object") return h.records.live;
  return undefined;
}

// POST — ensure the project's node(s) are up, point the UI at the primary
// node, and record the open.
//
// Single-node (replication 1, RFC-0006 Phase B.1): launches entirely through
// valori-daemon, which already owns health-wait/idempotency/crash-recovery —
// none of the old orphan/stale-log detection dance is needed here, that
// existed only to cope with `pm`'s in-memory state surviving a Next.js
// dev-mode hot-reload out of sync with reality. The daemon is a separate,
// stable process; that whole class of bug doesn't apply to it.
//
// Cluster (replication 3): the daemon can't launch a cluster yet, so this is
// the ORIGINAL pm-based multi-node logic, unchanged — only the project
// metadata now comes from the daemon (via `toLegacyEntry`) instead of the
// (now retired) `ui-projects.json`.
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;

  let daemonProject: daemon.DaemonProject;
  try {
    daemonProject = await daemon.getProject(name);
  } catch (e) {
    if (e instanceof daemon.DaemonError && e.status === 404) {
      return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
    }
    return errorResponse(e, 503);
  }

  const entry = toLegacyEntry(daemonProject, await resolveProjectsDir());
  const embed = entry.embed ?? DIM_TO_EMBED[entry.dim];

  // ── Single-node: the common, daemon-native path ──────────────────────────
  if (entry.replication === 1) {
    // Undo close/route.ts's protectAll() (chflags uchg) — otherwise the node
    // can't write its WAL/snapshot and fails to start.
    unprotectAll(entry);
    try {
      const status = await daemon.startProject(name);
      const url = `http://127.0.0.1:${status.port}`;
      const primary = status.port ? await probeHealth(status.port, 3000) : null;
      const recordCount = primary ? extractRecordCount(primary) : undefined;

      setApiUrl(url, primary ? { dim: primary.dim as number | undefined, records: recordCount } : undefined);

      return NextResponse.json({
        ok: true,
        url,
        port: status.port,
        reachable: status.status === "running",
        nodesReachable: status.status === "running" ? 1 : 0,
        nodesTotal: 1,
        ...(primary ?? {}),
        ...(embed ? { embed } : {}),
      });
    } catch (e) {
      return errorResponse(e, 503);
    }
  }

  // ── Cluster: unchanged from the pre-migration implementation ─────────────
  const primaryNode = entry.nodes[0];
  const url = `http://127.0.0.1:${primaryNode.httpPort}`;

  unprotectAll(entry);

  const preHealth = await Promise.all(entry.nodes.map((n) => probeHealth(n.httpPort, 400)));

  for (let i = 0; i < entry.nodes.length; i++) {
    const h = preHealth[i] as (HealthBody & { event_log_height?: number | null }) | null;
    if (h && h.event_log_height == null) {
      const port = entry.nodes[i].httpPort;
      const { snapshotPath } = projectNodePaths(entry, entry.nodes[i].id);
      await pm.snapshotThenStop(port, snapshotPath);
      await pm.waitForExit(port, 3000);
      preHealth[i] = null;
    }
  }

  preHealth.forEach((h, i) => { if (h) pm.markRunning(entry.nodes[i].httpPort); });

  const anyToStart = entry.nodes.some((n, i) => !preHealth[i] && !pm.isRunning(n.httpPort));
  if (anyToStart) {
    const nodeSpecs = entry.nodes.map((n) => {
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
    pm.startProjectNodes({
      dim: entry.dim,
      index: entry.index,
      maxRecords: entry.maxRecords,
      nodes: nodeSpecs,
      shardCount: entry.shardCount,
    });
  }

  const results: (HealthBody | null)[] = [...preHealth];
  for (let i = 0; i < 120; i++) {
    if (results.every((h) => h)) break;
    if (i > 0) await new Promise((r) => setTimeout(r, 150));
    await Promise.all(entry.nodes.map(async (n, idx) => {
      if (results[idx]) return;
      results[idx] = await probeHealth(n.httpPort, 1500);
    }));
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
    ...(embed ? { embed } : {}),
  });
}
