import { NextRequest, NextResponse } from "next/server";
import { getProject, projectNodePaths, protectProject, touchProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

// POST — snapshot-on-close: ask every node to write a final snapshot, stop it,
// wait for the process to fully exit, then re-apply the immutable flag so the
// data is protected at rest. Next open is instant (snapshot) and data is
// delete-locked. Nodes are stopped in parallel — unlike open, there's no
// quorum concern on teardown.
export async function POST(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  await Promise.all(entry.nodes.map(async (n) => {
    if (!pm.isRunning(n.httpPort)) return;
    const { snapshotPath } = projectNodePaths(entry, n.id);
    await pm.snapshotThenStop(n.httpPort, snapshotPath);

    // Wait for the process to actually exit (release file handles) before protecting.
    await pm.waitForExit(n.httpPort);
  }));

  // Re-lock the files now that no process is writing them.
  protectProject(name);
  touchProject(name, { lastOpenedAt: new Date().toISOString() });

  return NextResponse.json({ ok: true });
}
