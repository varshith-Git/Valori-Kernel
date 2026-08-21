import fs from "fs";
import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import type { DaemonProject } from "@/lib/server/daemon";
import { allocateNodes, isValidName, projectPaths } from "@/lib/server/projects";
import { toManifestShape, resolveProjectsDir } from "@/lib/server/project-adapter";
import { errorResponse } from "@/lib/server/http";

// GET — every project + live status, sourced entirely from valori-daemon
// (RFC-0006 Phase B.1). The daemon is the metadata source of truth for BOTH
// single-node and cluster projects (Phase B.0.5 imported everything).
//
// RFC-0007: the daemon now launches and supervises cluster (replication 3)
// projects itself, the same as single-node — `p.status.status` (from
// `LocalRuntime::status()` → `cluster_status_info()` on the Rust side) is
// already the aggregate across all of a cluster's nodes (`running` iff every
// node is), so both kinds use the identical status mapping below. No
// per-node polling needed here; that lives in `GET /v1/projects/:name/cluster`
// for callers that want per-node detail (leader, quorum, etc).
export function liveStatus(p: DaemonProject): { status: "stopped" | "starting" | "running" | "error"; nodesRunning: number; nodesTotal: number } {
  const nodesTotal = p.cluster?.nodes?.length || 1;
  const s = p.status.status;
  const status =
    s === "running" ? "running" :
    s === "starting" || s === "recovering" ? "starting" :
    s === "stopped" ? "stopped" :
    "error"; // stopping | failed
  return { status, nodesRunning: status === "running" ? nodesTotal : 0, nodesTotal };
}

export async function GET() {
  let daemonProjects: DaemonProject[];
  try {
    ({ projects: daemonProjects } = await daemon.listProjects());
  } catch (e) {
    return errorResponse(e, 503, "daemon unreachable");
  }

  const projectsDir = await resolveProjectsDir();
  const projects = daemonProjects.map((p) => {
    const shape = toManifestShape(p, projectsDir);
    const { status, nodesRunning, nodesTotal } = liveStatus(p);

    // Collections are derived straight off the namespaces sidecar file, same
    // as before migration — this doesn't depend on ui-projects.json at all,
    // works even when the project is stopped.
    let collections: string[] = [];
    try {
      const { eventLogPath } = projectPaths(shape);
      const nsPath = eventLogPath.replace(/\.log$/, ".namespaces.json");
      if (fs.existsSync(nsPath)) {
        const nsData = JSON.parse(fs.readFileSync(nsPath, "utf8"));
        const names = Object.keys(nsData.map || {});
        const prefix = `${shape.name}--`;
        collections = names.map((n) => (n.startsWith(prefix) ? n.slice(prefix.length) : n));
      }
    } catch {
      collections = [];
    }

    return { ...shape, status, nodesRunning, nodesTotal, collections };
  });

  return NextResponse.json({ projects });
}

// POST — create a project. Single-node: pure passthrough to the daemon
// (dim/index/workspace). Cluster (replication===3): the daemon persists the
// manifest, but port allocation for the 3 nodes is still done here (same
// `allocateNodes` used before migration) since the daemon can't launch a
// cluster yet — see the GET handler's comment.
export async function POST(req: NextRequest) {
  try {
    const body = (await req.json()) as {
      name?: string;
      maxRecords?: number;
      replication?: number;
      shardCount?: number;
      embed?: { provider: string; model: string; apiKey?: string; endpoint?: string };
    };
    if (!body.name) {
      return NextResponse.json({ error: "name required" }, { status: 400 });
    }
    if (!isValidName(body.name)) {
      return NextResponse.json({ error: "Invalid project name (use letters, digits, - or _, max 63 chars)" }, { status: 400 });
    }
    if (body.replication != null && body.replication !== 1 && body.replication !== 3) {
      return NextResponse.json({ error: "replication must be 1 or 3" }, { status: 400 });
    }
    if (body.shardCount != null && (!Number.isInteger(body.shardCount) || body.shardCount < 1 || body.shardCount > 16)) {
      return NextResponse.json({ error: "shardCount must be an integer from 1 to 16" }, { status: 400 });
    }

    const replication = (body.replication as 1 | 3 | undefined) ?? 1;
    const projectsDir = await resolveProjectsDir();

    let cluster: daemon.DaemonClusterConfig | undefined;
    if (replication === 3) {
      const { projects: existingDaemon } = await daemon.listProjects();
      const existingEntries = existingDaemon.map((p) => toManifestShape(p, projectsDir));
      const shardCount = body.shardCount && body.shardCount > 1 ? Math.min(Math.floor(body.shardCount), 16) : 1;
      const nodes = allocateNodes(existingEntries, 3);
      cluster = {
        replication: 3,
        nodes: nodes.map((n) => ({ id: n.id, http_port: n.httpPort, raft_port: n.raftPort })),
        shard_count: shardCount,
      };
    }

    const created = await daemon.createProject({
      name: body.name,
      cluster,
      embedding: body.embed
        ? { provider: body.embed.provider, model: body.embed.model, endpoint: body.embed.endpoint }
        : undefined,
      storage: { max_records: body.maxRecords ?? 1_000_000, protect_at_rest: true },
    });

    return NextResponse.json({ ok: true, project: toManifestShape(created, projectsDir) }, { status: 201 });
  } catch (e) {
    return errorResponse(e, 400);
  }
}
