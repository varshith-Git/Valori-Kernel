// Bridges the daemon's canonical project manifest (daemon.ts) into the
// shapes ui/'s existing pages and pure helpers already expect, so:
//   - React pages / hooks (useProjectManifest's `ManifestProject`) don't change
//   - the OLD cluster-only code path (process-manager.ts + projects.ts's pure
//     path helpers) keeps working unmodified for replication===3 projects
//     even though projects.ts's *file-backed* functions (listProjects,
//     getProject, ...) are no longer the source of truth for anything —
//     the daemon is, for both single-node AND cluster projects' metadata.
//
// See RFC-0006 Phase B.1.

import path from "path";
import os from "os";
import * as daemon from "./daemon";
import type { DaemonProject } from "./daemon";
import type { ProjectEntry, ProjectNodeEntry, ProjectEmbedConfig } from "./projects";

// Used only when the daemon can't be reached — the daemon's actual
// VALORI_HOME (read via resolveProjectsDir) is the source of truth, since
// the user may have picked a workspace outside ~/.valori during onboarding.
const FALLBACK_PROJECTS_DIR = path.join(os.homedir(), ".valori", "projects");

// VALORI_HOME is fixed for the daemon's lifetime, so cache the first
// successful answer instead of round-tripping on every request.
let cachedProjectsDir: string | null = null;

/** Resolves the daemon's real project root (`$VALORI_HOME/projects`),
 *  falling back to `~/.valori/projects` only when the daemon isn't
 *  reachable (e.g. still starting up). */
export async function resolveProjectsDir(): Promise<string> {
  if (cachedProjectsDir) return cachedProjectsDir;
  try {
    const config = await daemon.getConfig();
    cachedProjectsDir = path.join(config.home, "projects");
    return cachedProjectsDir;
  } catch {
    return FALLBACK_PROJECTS_DIR;
  }
}

/** The shape `ManifestProject` (ui/'s `useProjectManifest` hook) expects,
 *  minus the live `status`/`nodesRunning`/`nodesTotal`/`collections` fields —
 *  callers layer those on top (they need a live health/status probe, which
 *  differs for single-node vs. cluster projects; see the `/api/projects`
 *  route). */
export function toManifestShape(p: DaemonProject, projectsDir: string = FALLBACK_PROJECTS_DIR) {
  const replication = (p.cluster?.replication ?? 1) as 1 | 3;
  // Single-node, daemon-native projects have no *persisted* port — the
  // daemon allocates one dynamically each time it starts the node (unlike
  // ui/'s old static per-project port assignment). While stopped, that's
  // legitimately unknown; `0` would render as a bogus-looking ":0" on the
  // Home page, so leave it `undefined` (renders as nothing) instead.
  const nodes: ProjectNodeEntry[] = p.cluster?.nodes.length
    ? p.cluster.nodes.map((n) => ({ id: n.id, httpPort: n.http_port, raftPort: n.raft_port }))
    : [{ id: 1, httpPort: p.status.port as number }];
  const embed: ProjectEmbedConfig | undefined = p.embedding.provider
    ? { provider: p.embedding.provider, model: p.embedding.model ?? "", endpoint: p.embedding.endpoint }
    : undefined;

  return {
    name: p.name,
    dir: path.join(projectsDir, p.name),
    replication,
    nodes,
    shardCount: p.cluster?.shard_count ?? 1,
    port: nodes[0].httpPort,
    dim: p.dim,
    index: p.index as ProjectEntry["index"],
    maxRecords: p.storage.max_records,
    createdAt: new Date(p.created_at * 1000).toISOString(),
    ...(p.last_opened_at != null ? { lastOpenedAt: new Date(p.last_opened_at * 1000).toISOString() } : {}),
    ...(embed ? { embed } : {}),
  };
}

/** Full `ProjectEntry` shape, for feeding into `projects.ts`'s pure path
 *  helpers (`projectNodePaths`, `protectProject`, `unprotectProject`) and
 *  `process-manager.ts`'s `pm` — used ONLY on the cluster (replication===3)
 *  path, which still launches nodes the old way (the daemon can't launch a
 *  cluster yet — RFC-0006 Phase B.0). Single-node projects launch entirely
 *  through the daemon and never need this shape. */
export function toLegacyEntry(p: DaemonProject, projectsDir: string = FALLBACK_PROJECTS_DIR): ProjectEntry {
  return toManifestShape(p, projectsDir) as ProjectEntry;
}
