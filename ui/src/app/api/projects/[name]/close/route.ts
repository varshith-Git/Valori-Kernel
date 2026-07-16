import { NextRequest, NextResponse } from "next/server";
import { execFileSync } from "child_process";
import * as daemon from "@/lib/server/daemon";
import { projectNodePaths, protectAll, touchProject } from "@/lib/server/projects";
import { toLegacyEntry, resolveProjectsDir } from "@/lib/server/project-adapter";
import { pm } from "@/lib/server/process-manager";
import { errorResponse } from "@/lib/server/http";

async function probePort(port: number): Promise<boolean> {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/health`, { signal: AbortSignal.timeout(1500) });
    return r.ok;
  } catch { return false; }
}

function findPidOnPort(port: number): number | null {
  try {
    const out = execFileSync("lsof", ["-ti", `:${port}`], { encoding: "utf8", timeout: 3000 }).trim();
    const pid = parseInt(out.split("\n")[0], 10);
    return isNaN(pid) ? null : pid;
  } catch { return null; }
}

async function snapshotViaHttp(port: number, snapshotPath: string): Promise<void> {
  try {
    await fetch(`http://127.0.0.1:${port}/v1/snapshot/save`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: snapshotPath }),
      signal: AbortSignal.timeout(8000),
    });
  } catch { /* WAL is durable regardless */ }
}

interface HealthBody {
  records?: { live?: number } | number;
}

// POST — snapshot-on-close.
//
// Single-node (replication 1, RFC-0006 Phase B.1): `daemon.stopProject()`
// sends a graceful stop; `valori-node` snapshots on graceful shutdown itself
// whenever VALORI_SNAPSHOT_PATH is set, which the daemon always sets — no
// separate HTTP snapshot call needed, unlike the orphan-recovery path below.
//
// Cluster (replication 3): unchanged from the pre-migration implementation —
// still stops each node itself and falls back to an HTTP snapshot + SIGTERM
// for orphaned processes the daemon never supervised.
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
      const h = (await r.json()) as HealthBody;
      finalRecords = typeof h.records === "number" ? h.records : h.records?.live;
    }
  } catch { /* node may already be down */ }

  if (entry.replication === 1) {
    try {
      await daemon.stopProject(name);
    } catch (e) {
      return errorResponse(e, 503);
    }
  } else {
    await Promise.all(entry.nodes.map(async (n) => {
      const { snapshotPath } = projectNodePaths(entry, n.id);

      if (pm.isRunning(n.httpPort)) {
        await pm.snapshotThenStop(n.httpPort, snapshotPath);
        await pm.waitForExit(n.httpPort);
        return;
      }

      const alive = await probePort(n.httpPort);
      if (!alive) return;

      await snapshotViaHttp(n.httpPort, snapshotPath);

      const pid = findPidOnPort(n.httpPort);
      if (pid) {
        try { process.kill(pid, "SIGTERM"); } catch { /* already dead */ }
        await new Promise((r) => setTimeout(r, 2000));
        try { process.kill(pid, 0); process.kill(pid, "SIGKILL"); } catch { /* gone */ }
      }
    }));
  }

  protectAll(entry);
  touchProject(name, {
    lastOpenedAt: new Date().toISOString(),
    ...(finalRecords != null ? { records: finalRecords } : {}),
  });

  return NextResponse.json({ ok: true });
}
