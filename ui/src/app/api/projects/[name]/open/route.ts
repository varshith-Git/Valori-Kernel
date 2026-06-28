import { NextRequest, NextResponse } from "next/server";
import { getProject, projectPaths, unprotectProject, touchProject } from "@/lib/server/projects";
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

// POST — ensure the project's node is up, point the UI at it, and record the open.
// Auto-starts the node (the data dir's snapshot + WAL are replayed by try_recover).
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  const url = `http://localhost:${entry.port}`;

  // Clear the immutable flag so the node can append its WAL / write snapshots.
  unprotectProject(name);

  // ── Pre-probe: if already reachable, skip spawning entirely ──────────────
  // This handles: externally-started nodes, nodes that survived a Next.js
  // hot-reload, and prevents double-spawn when the port is already occupied.
  let health = await probeHealth(entry.port);
  if (health) {
    // Node is already up — reconcile PM state so future isRunning() calls work.
    pm.markRunning(entry.port);
  } else if (!pm.isRunning(entry.port)) {
    // Not reachable and PM doesn't know about it — spawn now.
    const { snapshotPath, eventLogPath } = projectPaths(entry);
    pm.startProject({
      port: entry.port,
      dim: entry.dim,
      index: entry.index,
      maxRecords: entry.maxRecords,
      snapshotPath,
      eventLogPath,
    });
  }

  // ── Health-probe loop — up to 60 s (handles cargo-run cold-compile path) ──
  if (!health) {
    for (let i = 0; i < 120; i++) {
      await new Promise(r => setTimeout(r, 500));
      health = await probeHealth(entry.port);
      if (health) break;

      // If the PM recorded an error (process exited), bail early.
      const st = pm.getStatus(entry.port)?.status;
      if (st === "error") break;
    }
  }

  const recordCount = health ? extractRecordCount(health) : undefined;

  // Point the UI proxy at this project's node and record the open.
  setApiUrl(url, health ? { dim: health.dim as number | undefined, records: recordCount } : undefined);
  touchProject(name, {
    lastOpenedAt: new Date().toISOString(),
    ...(recordCount != null ? { records: recordCount } : {}),
  });

  return NextResponse.json({
    ok: true,
    url,
    port: entry.port,
    reachable: !!health,
    ...(health ?? {}),
  });
}
