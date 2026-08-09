import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import { removeUrlFromHistory } from "@/lib/server/connection";
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
// first, then asks the daemon to delete the project (which removes the
// on-disk directory and its own manifest — the single source of truth for
// both project kinds since Phase B.0.5's import).
//
// RFC-0007: `daemon.stopProject()` now stops every node of a cluster project
// too (previously this had to go through the old `pm` path since the daemon
// never started those nodes in the first place — now it did, so it's the
// one that must stop them).
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

  const nodeUrls: string[] = [];
  const nodes = project.cluster?.nodes ?? (project.status.port ? [{ id: 1, http_port: project.status.port }] : []);
  for (const n of nodes) {
    nodeUrls.push(`http://localhost:${n.http_port}`, `http://127.0.0.1:${n.http_port}`);
  }

  if (project.status.status !== "stopped") {
    await daemon.stopProject(name).catch(() => {});
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
