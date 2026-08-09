import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import { protectAll, touchProject } from "@/lib/server/projects";
import { toLegacyEntry, resolveProjectsDir } from "@/lib/server/project-adapter";
import { errorResponse } from "@/lib/server/http";

interface HealthBody {
  records?: { live?: number } | number;
}

// POST — snapshot-on-close.
//
// RFC-0007: `valori-daemon` now owns stop/supervise for BOTH single-node and
// cluster (replication 3) projects — `daemon.stopProject()` already does the
// graceful snapshot-then-terminate sequence for every node in a cluster (see
// `LocalRuntime::stop_cluster` in `crates/valori-daemon/src/runtime/local.rs`).
// This route no longer manages any process itself (no `lsof`/`SIGTERM`
// fallback) — that was only ever needed for processes the daemon never
// supervised in the first place.
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

  let finalRecords: number | undefined;
  try {
    const r = await fetch(`http://127.0.0.1:${entry.nodes[0].httpPort}/health`, { signal: AbortSignal.timeout(1500) });
    if (r.ok) {
      const healthBody = (await r.json()) as HealthBody;
      finalRecords = typeof healthBody.records === "number" ? healthBody.records : healthBody.records?.live;
    }
  } catch { /* node may already be down */ }

  try {
    await daemon.stopProject(name);
  } catch (e) {
    return errorResponse(e, 503);
  }

  protectAll(entry);
  touchProject(name, {
    lastOpenedAt: new Date().toISOString(),
    ...(finalRecords != null ? { records: finalRecords } : {}),
  });

  return NextResponse.json({ ok: true });
}
