import { NextRequest, NextResponse } from "next/server";
import { getProject, projectPaths, protectProject, touchProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

// POST — snapshot-on-close: ask the node to write a final snapshot, stop it, wait
// for the process to fully exit, then re-apply the immutable flag so the data is
// protected at rest. Next open is instant (snapshot) and data is delete-locked.
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  const { snapshotPath } = projectPaths(entry);

  if (pm.isRunning(entry.port)) {
    await pm.snapshotThenStop(entry.port, snapshotPath);

    // Wait for the process to release file handles before protecting.
    for (let i = 0; i < 30; i++) {
      const s = pm.getStatus(entry.port)?.status;
      if (s === "stopped" || s === "error") break;
      await new Promise(r => setTimeout(r, 300));
    }
  }

  // Re-lock the files now that no process is writing them.
  protectProject(name);
  touchProject(name, { lastOpenedAt: new Date().toISOString() });

  return NextResponse.json({ ok: true });
}
