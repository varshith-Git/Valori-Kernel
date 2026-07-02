import { NextRequest, NextResponse } from "next/server";
import { listProjects, createProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

// GET — all projects from the manifest, annotated with live node status.
// Works even when every node is stopped (manifest is the source of truth).
//
// After a Next.js server restart the ProcessManager singleton is fresh and
// thinks every node is stopped — even if the OS process is still listening.
// We probe each "stopped" node's port with a quick /health fetch and
// re-register running nodes so the UI shows the correct status and Open
// doesn't try to spawn a second process on an already-occupied port.
//
// Status is an aggregate across every node in a project (1 for single-node,
// 3 for a cluster): "running" only when ALL nodes are up, "starting" if any
// is still starting, "error" for a partial/degraded cluster (some up, some
// not), "stopped" when none are up. Reuses the existing 4-value status enum
// rather than inventing a "degraded" value.
export async function GET() {
  const entries = listProjects();

  await Promise.all(
    entries.flatMap((p) =>
      p.nodes.map(async (n) => {
        const known = pm.getStatus(n.httpPort);
        if (known && known.status !== "stopped") return; // already tracked
        try {
          const r = await fetch(`http://localhost:${n.httpPort}/health`, {
            signal: AbortSignal.timeout(600),
          });
          if (r.ok) {
            // Node is alive but pm doesn't know about it — register as running.
            pm.markRunning(n.httpPort);
          }
        } catch {
          // Not reachable — genuinely stopped, nothing to do.
        }
      })
    )
  );

  const projects = entries.map(p => {
    const nodeStatuses = p.nodes.map(n => pm.getStatus(n.httpPort)?.status ?? "stopped");
    const runningCount = nodeStatuses.filter(s => s === "running").length;
    const anyStarting  = nodeStatuses.some(s => s === "starting");
    const anyError     = nodeStatuses.some(s => s === "error");
    const status =
      runningCount === p.nodes.length ? "running" :
      anyStarting                     ? "starting" :
      runningCount > 0 || anyError    ? "error" :
                                         "stopped";
    return { ...p, status, nodesRunning: runningCount, nodesTotal: p.nodes.length };
  });
  return NextResponse.json({ projects });
}

// POST — create a project: allocate dir + node ports, write manifest, protect files.
export async function POST(req: NextRequest) {
  try {
    const body = await req.json() as {
      name?: string;
      dim?: number;
      index?: "brute" | "hnsw" | "ivf";
      maxRecords?: number;
      replication?: number;
      shardCount?: number;
    };
    if (!body.name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }
    if (body.replication != null && body.replication !== 1 && body.replication !== 3) {
      return NextResponse.json({ error: "replication must be 1 or 3" }, { status: 400 });
    }
    if (body.shardCount != null && (!Number.isInteger(body.shardCount) || body.shardCount < 1 || body.shardCount > 16)) {
      return NextResponse.json({ error: "shardCount must be an integer from 1 to 16" }, { status: 400 });
    }
    const entry = createProject({
      name:        body.name,
      dim:         body.dim ?? 768,
      index:       body.index ?? "brute",
      maxRecords:  body.maxRecords,
      replication: (body.replication as 1 | 3 | undefined) ?? 1,
      shardCount:  body.shardCount,
    });
    return NextResponse.json({ ok: true, project: entry }, { status: 201 });
  } catch (e) {
    return NextResponse.json({ error: String((e as Error).message ?? e) }, { status: 400 });
  }
}
