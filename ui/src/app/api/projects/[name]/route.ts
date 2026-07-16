import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import { removeUrlFromHistory } from "@/lib/server/connection";
import { pm } from "@/lib/server/process-manager";
import { unprotectAll } from "@/lib/server/projects";
import { toLegacyEntry, resolveProjectsDir } from "@/lib/server/project-adapter";
import { errorResponse } from "@/lib/server/http";

export async function PATCH(
  req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;
  const body = await req.json().catch(() => ({})) as { name?: string };
  const newName = body.name?.trim();
  if (!newName) {
    return NextResponse.json({ error: "missing `name`" }, { status: 400 });
  }
  try {
    const result = await daemon.renameProject(name, newName);
    return NextResponse.json(result);
  } catch (e) {
    return errorResponse(e, 500);
  }
}

// DELETE — the only path that may remove project data. Stops every node
// first (single-node: via the daemon; cluster: via the old `pm` path, since
// the daemon never started those nodes — RFC-0006 Phase B.0), then asks the
// daemon to delete the project (which removes the on-disk directory and its
// own manifest — the single source of truth for both project kinds since
// Phase B.0.5's import).
export async function DELETE(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  const { name } = await params;

  let project: daemon.DaemonProject;
  try {
    project = await daemon.getProject(name);
  } catch (e) {
    if (e instanceof daemon.DaemonError && e.status === 404) {
      return NextResponse.json({ error: `Project "${name}" not found` }, { status: 404 });
    }
    return errorResponse(e, 503);
  }

  const replication = project.cluster?.replication ?? 1;
  const nodeUrls: string[] = [];

  if (replication === 1) {
    if (project.status.status !== "stopped") {
      await daemon.stopProject(name).catch(() => {});
    }
    if (project.status.port) {
      nodeUrls.push(`http://localhost:${project.status.port}`, `http://127.0.0.1:${project.status.port}`);
    }
  } else {
    const nodes = project.cluster?.nodes ?? [];
    await Promise.all(
      nodes.map(async (n) => {
        if (!pm.isRunning(n.http_port)) return;
        pm.stopNode(n.http_port);
        await pm.waitForExit(n.http_port);
      })
    );
    for (const n of nodes) {
      nodeUrls.push(`http://localhost:${n.http_port}`, `http://127.0.0.1:${n.http_port}`);
    }
  }

  // Undo close/route.ts's protectAll() (chflags uchg / read-only perms) —
  // otherwise the daemon's remove_dir_all hits an immutable file and 500s.
  unprotectAll(toLegacyEntry(project, await resolveProjectsDir()));

  try {
    await daemon.deleteProject(name);
  } catch (e) {
    return errorResponse(e, 500);
  }

  for (const url of nodeUrls) removeUrlFromHistory(url);

  return NextResponse.json({ ok: true }, { status: 200 });
}
