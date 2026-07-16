import fs from "fs";
import { NextRequest, NextResponse } from "next/server";
import * as daemon from "@/lib/server/daemon";
import type { DaemonProject } from "@/lib/server/daemon";
import { pm } from "@/lib/server/process-manager";
import { allocateNodes, isValidName, projectPaths } from "@/lib/server/projects";
import { toManifestShape, resolveProjectsDir } from "@/lib/server/project-adapter";
import { errorResponse } from "@/lib/server/http";

// GET — every project + live status, sourced entirely from valori-daemon
// (RFC-0006 Phase B.1). The daemon is the metadata source of truth for BOTH
// single-node and cluster projects (Phase B.0.5 imported everything). Live
// runtime status differs by kind:
//   - single-node (replication 1): the daemon actually runs these — its own
//     status is authoritative.
//   - cluster (replication 3): the daemon can't launch a cluster yet, so
//     these are still started via the old `pm`-based path (`/open`/`/close`)
//     — live status comes from `pm`, keyed by the ports the daemon persisted.
export function liveStatus(p: DaemonProject): { status: "stopped" | "starting" | "running" | "error"; nodesRunning: number; nodesTotal: number } {
  const replication = p.cluster?.replication ?? 1;

  if (replication === 1) {
    const s = p.status.status;
    const status =
      s === "running" ? "running" :
      s === "starting" || s === "recovering" ? "starting" :
      s === "stopped" ? "stopped" :
      "error"; // stopping | failed
    return { status, nodesRunning: status === "running" ? 1 : 0, nodesTotal: 1 };
  }

  const nodes = p.cluster?.nodes ?? [];
  const nodeStatuses = nodes.map((n) => pm.getStatus(n.http_port)?.status ?? "stopped");
  const runningCount = nodeStatuses.filter((s) => s === "running").length;
  const anyStarting = nodeStatuses.some((s) => s === "starting");
  const anyError = nodeStatuses.some((s) => s === "error");
  const status =
    nodes.length > 0 && runningCount === nodes.length ? "running" :
    anyStarting ? "starting" :
    runningCount > 0 || anyError ? "error" :
    "stopped";
  return { status, nodesRunning: runningCount, nodesTotal: nodes.length };
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
        collections = names.filter((n) => n.startsWith(prefix)).map((n) => n.slice(prefix.length));
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
    const dim = body.dim ?? 768;
    const index = body.index ?? "brute";
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
      dim,
      index,
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
