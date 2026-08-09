import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import { unprotectAll, touchProject, type ProjectEmbedConfig } from "@/lib/server/projects";
import { toLegacyEntry, resolveProjectsDir } from "@/lib/server/project-adapter";
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
// RFC-0007: `valori-daemon` now owns launch/supervise/health for BOTH
// single-node and cluster (replication 3) projects — `daemon.startProject()`
// already handles the "launch N processes, wait for all N healthy" logic on
// the Rust side (see `LocalRuntime::start_cluster` in
// `crates/valori-daemon/src/runtime/local.rs`). This route no longer spawns
// or supervises any process itself; it just calls the daemon and shapes the
// response the same way for both cases.
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

  // Undo close/route.ts's protectAll() (chflags uchg) — otherwise the
  // node(s) can't write their WAL/snapshot/raft log and fail to start.
  unprotectAll(entry);

  try {
    await daemon.startProject(name);
  } catch (e) {
    return errorResponse(e, 503);
  }

  const primaryNode = entry.nodes[0];
  const url = `http://127.0.0.1:${primaryNode.httpPort}`;

  // Poll every node's own /health directly rather than round-tripping
  // through the daemon's aggregation endpoint — this route already needs
  // per-node health bodies (dim, live record count) for the response shape
  // clients expect, which the daemon's /cluster endpoint doesn't carry.
  const results: (HealthBody | null)[] = new Array(entry.nodes.length).fill(null);
  for (let i = 0; i < 120; i++) {
    if (results.every((h) => h)) break;
    if (i > 0) await new Promise((r) => setTimeout(r, 150));
    await Promise.all(entry.nodes.map(async (n, idx) => {
      if (results[idx]) return;
      results[idx] = await probeHealth(n.httpPort, 1500);
    }));
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
