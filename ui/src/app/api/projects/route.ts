import { NextRequest, NextResponse } from "next/server";
import { listProjects, createProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

// ── Health-probe cache ────────────────────────────────────────────────────────
// Results from background health probes, keyed by port.
// Persists across requests within the same Next.js server process so the
// first GET returns instantly (with cached/default statuses) while probes
// run in the background.  The next poll (≤2 s later) picks up real statuses.
const probeCache = new Map<number, "running" | "stopped">();
let probeInFlight = false;

function probeInBackground(entries: ReturnType<typeof listProjects>) {
  if (probeInFlight) return; // don't stack up parallel probe rounds
  probeInFlight = true;

  Promise.all(
    entries.flatMap((p) =>
      p.nodes.map(async (n) => {
        const known = pm.getStatus(n.httpPort);
        if (known && known.status !== "stopped") {
          probeCache.set(n.httpPort, "running");
          return;
        }
        try {
          const r = await fetch(`http://127.0.0.1:${n.httpPort}/health`, {
            signal: AbortSignal.timeout(600),
          });
          if (r.ok) {
            pm.markRunning(n.httpPort);
            probeCache.set(n.httpPort, "running");
          } else {
            probeCache.set(n.httpPort, "stopped");
          }
        } catch {
          probeCache.set(n.httpPort, "stopped");
        }
      })
    )
  ).finally(() => {
    probeInFlight = false;
  });
}

// GET — all projects from the manifest, annotated with live node status.
// Works even when every node is stopped (manifest is the source of truth).
//
// Health probes run in the BACKGROUND so the first response is instant.
// On the initial request the probe cache may be empty, so statuses default
// to "stopped".  By the time the client polls again (~2 s later), the probes
// have completed and real statuses are returned.
//
// Status is an aggregate across every node in a project (1 for single-node,
// 3 for a cluster): "running" only when ALL nodes are up, "starting" if any
// is still starting, "error" for a partial/degraded cluster (some up, some
// not), "stopped" when none are up.
export async function GET() {
  const entries = listProjects();

  // Fire health probes in the background — never blocks the response.
  probeInBackground(entries);

  // Build the response immediately using ProcessManager + probe cache.
  const projects = entries.map((p) => {
    const nodeStatuses = p.nodes.map((n) => {
      const pmStatus = pm.getStatus(n.httpPort)?.status;
      if (pmStatus && pmStatus !== "stopped") return pmStatus;
      return probeCache.get(n.httpPort) ?? "stopped";
    });
    const runningCount = nodeStatuses.filter((s) => s === "running").length;
    const anyStarting  = nodeStatuses.some((s) => s === "starting");
    const anyError     = nodeStatuses.some((s) => s === "error");
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
      index?: "brute" | "hnsw" | "ivf" | "bq" | "auto";
      maxRecords?: number;
      replication?: number;
      shardCount?: number;
      embed?: { provider: string; model: string; apiKey?: string; endpoint?: string };
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
      embed:       body.embed,
    });
    return NextResponse.json({ ok: true, project: entry }, { status: 201 });
  } catch (e) {
    return NextResponse.json({ error: String((e as Error).message ?? e) }, { status: 400 });
  }
}
