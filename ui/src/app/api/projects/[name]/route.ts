import { NextRequest, NextResponse } from "next/server";
import { getProject, deleteProject } from "@/lib/server/projects";
import { removeUrlFromHistory } from "@/lib/server/connection";
import { pm } from "@/lib/server/process-manager";

// DELETE — the only path that may remove project data. Stops every node,
// clears the immutable flag (inside deleteProject), removes the dir, drops
// the manifest entry.
export async function DELETE(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  // Stop every node that's up and wait for real exit so no process is still
  // holding/writing the files when we rm -rf the directory below.
  await Promise.all(entry.nodes.map(async (n) => {
    if (!pm.isRunning(n.httpPort)) return;
    pm.stopNode(n.httpPort);
    await pm.waitForExit(n.httpPort);
  }));

  const ok = deleteProject(name);
  if (ok) {
    for (const n of entry.nodes) {
      removeUrlFromHistory(`http://localhost:${n.httpPort}`);
      removeUrlFromHistory(`http://127.0.0.1:${n.httpPort}`);
    }
  }
  
  return NextResponse.json({ ok }, { status: ok ? 200 : 404 });
}
