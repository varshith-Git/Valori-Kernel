import { NextRequest, NextResponse } from "next/server";
import { getProject, projectPaths, unprotectProject, touchProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";
import { setApiUrl } from "@/lib/server/connection";

async function probeHealth(port: number): Promise<{ dim?: number; records?: number } | null> {
  try {
    const r = await fetch(`http://localhost:${port}/health`, { signal: AbortSignal.timeout(2000) });
    if (!r.ok) return null;
    return await r.json() as { dim?: number; records?: number };
  } catch {
    return null;
  }
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

  if (!pm.isRunning(entry.port)) {
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

  // Wait for the node to answer /health (cold start can take a few seconds).
  let health: { dim?: number; records?: number } | null = null;
  for (let i = 0; i < 30; i++) {
    health = await probeHealth(entry.port);
    if (health) break;
    await new Promise(r => setTimeout(r, 500));
  }

  // Point the UI proxy at this project's node and record the open.
  setApiUrl(url, health ? { dim: health.dim, records: health.records } : undefined);
  touchProject(name, {
    lastOpenedAt: new Date().toISOString(),
    ...(health?.records != null ? { records: health.records } : {}),
  });

  return NextResponse.json({
    ok: true,
    url,
    port: entry.port,
    reachable: !!health,
    ...(health ?? {}),
  });
}
