import { NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import { errorResponse } from "@/lib/server/http";
import { liveStatus } from "../../route";

// GET — read-only status probe for one project. Deliberately separate from
// `/open`, which is start-capable (it launches the node if it isn't
// running): a client-side poller hitting `/open` on an interval will
// silently relaunch a project the user just stopped, a few seconds after
// they stopped it. This route only ever reads state — it can be polled
// safely regardless of whether the project is meant to be running.
export async function GET(
  _req: Request,
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

  const { status, nodesRunning, nodesTotal } = liveStatus(daemonProject);
  return NextResponse.json({ status, reachable: status === "running", nodesRunning, nodesTotal });
}
