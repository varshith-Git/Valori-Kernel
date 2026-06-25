import { NextRequest, NextResponse } from "next/server";
import { getProject, deleteProject } from "@/lib/server/projects";
import { pm } from "@/lib/server/process-manager";

// DELETE — the only path that may remove project data. Stops the node, clears
// the immutable flag (inside deleteProject), removes the dir, drops the manifest
// entry.
export async function DELETE(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const entry = getProject(name);
  if (!entry) {
    return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
  }

  // Stop the node if it's up so no process is holding the files.
  if (pm.isRunning(entry.port)) {
    pm.stopNode(entry.port);
    // Give it a moment to release file handles before rm.
    await new Promise(r => setTimeout(r, 600));
  }

  const ok = deleteProject(name);
  return NextResponse.json({ ok }, { status: ok ? 200 : 404 });
}
