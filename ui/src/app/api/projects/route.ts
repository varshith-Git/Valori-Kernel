import { NextRequest, NextResponse } from "next/server";
import { listProjects, createProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

// GET — all projects from the manifest, annotated with live node status.
// Works even when every node is stopped (manifest is the source of truth).
//
// After a Next.js server restart the ProcessManager singleton is fresh and
// thinks every node is stopped — even if the OS process is still listening.
// We probe each "stopped" port with a quick /health fetch and re-register
// running nodes so the UI shows the correct status and Open doesn't try to
// spawn a second process on the same port.
export async function GET() {
  const entries = listProjects();

  await Promise.all(
    entries.map(async (p) => {
      const known = pm.getStatus(p.port);
      if (known && known.status !== "stopped") return; // already tracked
      try {
        const r = await fetch(`http://localhost:${p.port}/health`, {
          signal: AbortSignal.timeout(600),
        });
        if (r.ok) {
          // Node is alive but pm doesn't know about it — register as running.
          pm.markRunning(p.port);
        }
      } catch {
        // Not reachable — genuinely stopped, nothing to do.
      }
    })
  );

  const projects = entries.map(p => ({
    ...p,
    status: pm.getStatus(p.port)?.status ?? "stopped",
  }));
  return NextResponse.json({ projects });
}

// POST — create a project: allocate dir + port, write manifest, protect files.
export async function POST(req: NextRequest) {
  try {
    const body = await req.json() as {
      name?: string;
      dim?: number;
      index?: "brute" | "hnsw" | "ivf";
      maxRecords?: number;
    };
    if (!body.name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }
    const entry = createProject({
      name:       body.name,
      dim:        body.dim ?? 768,
      index:      body.index ?? "brute",
      maxRecords: body.maxRecords,
    });
    return NextResponse.json({ ok: true, project: entry }, { status: 201 });
  } catch (e) {
    return NextResponse.json({ error: String((e as Error).message ?? e) }, { status: 400 });
  }
}
