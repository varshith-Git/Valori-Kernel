// NOTE (RFC-0006 Phase B.1): the manifest-file-backed functions in this
// module (listProjects, getProject, createProject, deleteProject,
// touchProject, reprotect, protectProject, unprotectProject, importFromTmp)
// are `@deprecated` — valori-daemon is now the metadata source of truth for
// both single-node and cluster projects (see `lib/server/daemon.ts` +
// `project-adapter.ts`). Phase B.0.5's migration renames this file's backing
// store (`ui-projects.json`) to `ui-projects.json.migrated` on first daemon
// startup, so those functions now operate on an effectively-empty manifest.
//
// Still load-bearing and NOT deprecated: the pure, entry-based helpers
// (allocateNodes, projectNodePaths, projectPaths, protectAll, unprotectAll,
// isValidName) and the `ProjectEntry`/`ProjectEmbedConfig`/`ProjectNodeEntry`
// types — the cluster (replication===3) lifecycle routes still use these,
// since the daemon can't launch a cluster yet.
//
// Remove the deprecated functions once `grep -rn "from \"@/lib/server/
// projects\""` shows no route still importing them (a follow-up cleanup
// pass, not part of this migration).

import fs from "fs";
import path from "path";
import os from "os";
import { execFileSync } from "child_process";

// ─── paths ─────────────────────────────────────────────────────────────────────

const VALORI_HOME   = path.join(os.homedir(), ".valori");
const PROJECTS_DIR  = path.join(VALORI_HOME, "projects");
// Distinct from the CLI `valori setup` wizard's `projects.json` (a different
// cluster-topology schema) — never clobber that file.
const MANIFEST_FILE = path.join(VALORI_HOME, "ui-projects.json");

/** Port range for single-node projects. 3000–3009 left for manual/cluster launches. */
const PORT_BASE = 3010;
const PORT_MAX  = 3999;

/**
 * Port range for 3-node cluster projects — kept distinct from both the
 * single-node range above and the Launcher's ad-hoc 3000-3009/3100-3109
 * range, so the three never collide. Raft port is always httpPort + 100
 * (mirrors the Launcher's own httpPort/raftPort relationship).
 */
const CLUSTER_PORT_BASE = 4010;
const CLUSTER_PORT_MAX  = 4999;
const CLUSTER_RAFT_OFFSET = 100;

// ─── types ─────────────────────────────────────────────────────────────────────

export interface ProjectNodeEntry {
  id:        number;   // Raft-semantic id, unique within this project (1, 2, 3)
  httpPort:  number;
  raftPort?: number;   // present only when replication > 1
}

export interface ProjectEmbedConfig {
  provider: string;
  model:    string;
  apiKey?:  string;
  endpoint?: string;
}

export interface ProjectEntry {
  name:          string;   // unique, also the dir name (slug)
  dir:           string;   // absolute path to project data dir
  replication:   1 | 3;    // single node vs 3-node Raft cluster
  nodes:         ProjectNodeEntry[]; // length 1 or 3, ordered by id ascending
  /**
   * Number of independent shards (Raft groups) EACH node in `nodes` runs
   * internally, via one VALORI_SHARD_COUNT env var per node — NOT extra
   * processes or ports (every shard on a node shares that node's HTTP port
   * and gRPC listener). Only meaningful when `replication === 3`; standalone
   * (replication 1) has no shard concept at all. Default 1 = today's
   * single-Raft-group behavior.
   */
  shardCount:    number;
  port:          number;   // KEPT for back-compat/display — always === nodes[0].httpPort
  dim:           number;
  index:         "brute" | "hnsw" | "ivf" | "bq" | "auto";
  maxRecords:    number;
  createdAt:     string;   // ISO
  lastOpenedAt?: string;   // ISO
  records?:      number;   // last-known record count (cosmetic)
  embed?:        ProjectEmbedConfig;
  collections?:  string[]; // derived from events.namespaces.json
}

// ─── name validation ───────────────────────────────────────────────────────────

/** Project names map 1:1 to directory names — keep them filesystem-safe. */
export function isValidName(name: string): boolean {
  return /^[a-zA-Z0-9](?:[a-zA-Z0-9_-]{0,62})$/.test(name);
}

// ─── manifest IO ───────────────────────────────────────────────────────────────

/**
 * Legacy manifest entries (written before `nodes[]` existed) only have a
 * scalar `port`. Synthesize the new shape in memory so every reader works
 * unmodified — no manual migration step. The synthesized shape is only
 * persisted once something calls `writeManifest` again (e.g. `touchProject`),
 * at which point the entry is upgraded on disk automatically.
 */
function migrateEntry(raw: ProjectEntry & { port: number }): ProjectEntry {
  const withNodes: ProjectEntry =
    Array.isArray(raw.nodes) && raw.nodes.length > 0
      ? raw
      : { ...raw, replication: 1, nodes: [{ id: raw.port, httpPort: raw.port }] };
  return withNodes.shardCount ? withNodes : { ...withNodes, shardCount: 1 };
}

function readManifest(): ProjectEntry[] {
  try {
    const parsed = JSON.parse(fs.readFileSync(MANIFEST_FILE, "utf8"));
    return Array.isArray(parsed) ? (parsed as ProjectEntry[]).map(migrateEntry) : [];
  } catch {
    return [];
  }
}

function writeManifest(list: ProjectEntry[]): void {
  fs.mkdirSync(VALORI_HOME, { recursive: true });
  fs.writeFileSync(MANIFEST_FILE, JSON.stringify(list, null, 2));
}

/**
 * @deprecated Replaced by valori-daemon's `GET /v1/projects` (see
 * `ui/src/lib/server/daemon.ts` + `project-adapter.ts`). Reads
 * `ui-projects.json`, which Phase B.0.5's migration renames to
 * `ui-projects.json.migrated` on first daemon startup — this now returns an
 * empty list in practice. Kept only until this file's cleanup pass.
 */
export function listProjects(): ProjectEntry[] {
  const list = readManifest();
  for (const p of list) {
    try {
      const { eventLogPath } = projectPaths(p);
      const nsPath = eventLogPath.replace(/\.log$/, ".namespaces.json");
      if (fs.existsSync(nsPath)) {
        const nsData = JSON.parse(fs.readFileSync(nsPath, "utf8"));
        const names = Object.keys(nsData.map || {});
        const prefix = `${p.name}--`;
        p.collections = names.filter((n) => n.startsWith(prefix)).map((n) => n.slice(prefix.length));
      } else {
        p.collections = [];
      }
    } catch {
      p.collections = [];
    }
  }
  return list;
}

/**
 * @deprecated Replaced by valori-daemon's `GET /v1/projects/:name` (see
 * `ui/src/lib/server/daemon.ts` + `project-adapter.ts`). Same caveat as
 * {@link listProjects} — the backing file is gone post-migration.
 */
export function getProject(name: string): ProjectEntry | undefined {
  return readManifest().find(p => p.name === name);
}

// ─── port allocation ───────────────────────────────────────────────────────────

/** Exported for reuse by the daemon-backed `/api/projects` POST handler,
 *  which still needs to allocate cluster (replication===3) ports itself —
 *  the daemon can persist cluster metadata (RFC-0006 Phase B.0) but doesn't
 *  invent port assignments, since it can't launch a cluster yet. */
export function allocateNodes(existing: ProjectEntry[], replication: 1 | 3): ProjectNodeEntry[] {
  if (replication === 1) {
    const used = new Set(existing.flatMap(p => p.nodes.map(n => n.httpPort)));
    for (let port = PORT_BASE; port <= PORT_MAX; port++) {
      if (!used.has(port)) return [{ id: port, httpPort: port }];
    }
    throw new Error("No free port available for new project");
  }

  const usedHttp = new Set(existing.flatMap(p => p.nodes.map(n => n.httpPort)));
  const usedRaft = new Set(existing.flatMap(p => p.nodes.map(n => n.raftPort).filter((r): r is number => r != null)));
  const nodes: ProjectNodeEntry[] = [];
  let candidate = CLUSTER_PORT_BASE;
  while (nodes.length < 3) {
    if (candidate > CLUSTER_PORT_MAX) {
      throw new Error("No free port block available for new cluster project");
    }
    const httpPort = candidate;
    const raftPort = candidate + CLUSTER_RAFT_OFFSET;
    if (!usedHttp.has(httpPort) && !usedRaft.has(raftPort) && raftPort <= CLUSTER_PORT_MAX) {
      nodes.push({ id: nodes.length + 1, httpPort, raftPort });
    }
    candidate++;
  }
  return nodes;
}

// ─── file protection (UI-only deletion) ─────────────────────────────────────────

/**
 * Make a path resistant to manual deletion. macOS: the user-immutable flag
 * (`chflags uchg`) — Finder and `rm` refuse to remove it. Other platforms:
 * fall back to read-only perms. Never throws — protection is best-effort.
 */
export function protect(target: string): void {
  try {
    if (process.platform === "darwin") {
      execFileSync("chflags", ["uchg", target], { stdio: "ignore" });
    } else {
      fs.chmodSync(target, 0o400);
    }
  } catch {
    /* best-effort */
  }
}

/** Clear protection so the UI can delete. Mirror of {@link protect}. */
export function unprotect(target: string): void {
  try {
    if (process.platform === "darwin") {
      execFileSync("chflags", ["nouchg", target], { stdio: "ignore" });
    } else {
      fs.chmodSync(target, 0o600);
    }
  } catch {
    /* best-effort */
  }
}

/** Every file a single node writes: snapshot + event log (raft log excluded —
 *  it's Raft-internal state, not user data, so it's never immutable-locked). */
function nodeDataFiles(entry: ProjectEntry, nodeId: number): string[] {
  const { snapshotPath, eventLogPath } = projectNodePaths(entry, nodeId);
  return [snapshotPath, eventLogPath];
}

/** Exported for reuse by the daemon-backed `/open`/`/close` routes, which
 *  already have a project entry in hand (from the daemon, not this file's
 *  now-largely-retired manifest) — calling `protectProject`/`unprotectProject`
 *  by name would re-look-up via `getProject()` here, which no longer finds
 *  anything once `ui-projects.json` is renamed by the daemon's migration. */
export function protectAll(entry: ProjectEntry): void {
  for (const n of entry.nodes) {
    for (const p of nodeDataFiles(entry, n.id)) {
      if (fs.existsSync(p)) protect(p);
    }
  }
}

export function unprotectAll(entry: ProjectEntry): void {
  for (const n of entry.nodes) {
    for (const p of nodeDataFiles(entry, n.id)) {
      if (fs.existsSync(p)) unprotect(p);
    }
  }
}

// ─── derived paths (consumed by process-manager) ────────────────────────────────

/**
 * Per-node data file paths. Single-node projects (replication 1) keep today's
 * unsuffixed filenames exactly (`current.snap`, `events.log`). Cluster
 * projects (replication 3) suffix every node's files with its id
 * (`current-n1.snap`, `events-n2.log`, `raft-n3.redb`, ...) so all 3 nodes'
 * data can coexist under the same project dir without collision.
 */
export function projectNodePaths(entry: ProjectEntry, nodeId: number): {
  snapshotPath: string; eventLogPath: string; raftLogPath?: string;
} {
  const suffix = entry.replication === 1 ? "" : `-n${nodeId}`;
  return {
    snapshotPath: path.join(entry.dir, `current${suffix}.snap`),
    eventLogPath: path.join(entry.dir, `events${suffix}.log`),
    ...(entry.replication > 1 ? { raftLogPath: path.join(entry.dir, `raft${suffix}.redb`) } : {}),
  };
}

/** Convenience wrapper for callers that only care about the primary node. */
export function projectPaths(entry: ProjectEntry): { snapshotPath: string; eventLogPath: string } {
  return projectNodePaths(entry, entry.nodes[0].id);
}

/**
 * @deprecated Takes a name and re-reads the (now largely empty, post-
 * migration) manifest file internally. Routes now call {@link protectAll}
 * directly with an already-loaded entry instead. Kept only until this file's
 * cleanup pass.
 */
export function reprotect(name: string): void {
  const entry = getProject(name);
  if (entry) protectAll(entry);
}

// ─── CRUD ──────────────────────────────────────────────────────────────────────

export interface CreateProjectInput {
  name:         string;
  dim:          number;
  index:        "brute" | "hnsw" | "ivf" | "bq" | "auto";
  maxRecords?:  number;
  replication?: 1 | 3;   // default 1
  /** Only meaningful when replication === 3 — see ProjectEntry.shardCount. */
  shardCount?:  number;  // default 1
  embed?:       ProjectEmbedConfig;
}

/**
 * @deprecated Replaced by valori-daemon's `POST /v1/projects` (see
 * `ui/src/app/api/projects/route.ts`, which now calls `daemon.createProject`
 * for both single-node and cluster projects — reusing only {@link
 * allocateNodes} from here for cluster port assignment). Kept only until
 * this file's cleanup pass.
 */
export function createProject(input: CreateProjectInput): ProjectEntry {
  if (!isValidName(input.name)) {
    throw new Error("Invalid project name (use letters, digits, - or _, max 63 chars)");
  }
  const list = readManifest();
  if (list.some(p => p.name === input.name)) {
    throw new Error(`Project "${input.name}" already exists`);
  }

  const dir = path.join(PROJECTS_DIR, input.name);
  fs.mkdirSync(dir, { recursive: true });

  const replication = input.replication === 3 ? 3 : 1;
  const nodes = allocateNodes(list, replication);
  // Sharding is a cluster-only concept — standalone (replication 1) never
  // sets VALORI_SHARD_COUNT, so pin it to 1 regardless of what was passed.
  const shardCount = replication === 3 && input.shardCount && input.shardCount > 1
    ? Math.min(Math.floor(input.shardCount), 16)
    : 1;

  const entry: ProjectEntry = {
    name:       input.name,
    dir,
    replication,
    nodes,
    shardCount,
    port:       nodes[0].httpPort,
    dim:        input.dim,
    index:      input.index,
    maxRecords: input.maxRecords ?? 1_000_000,
    createdAt:  new Date().toISOString(),
    ...(input.embed ? { embed: input.embed } : {}),
  };

  writeManifest([...list, entry]);
  return entry;
}

/**
 * @deprecated No daemon equivalent exists yet (recording `lastOpenedAt`/
 * `records` would need a daemon "touch" endpoint — new backend surface, out
 * of scope for the route migration). Still called from `/open`/`/close` as a
 * harmless no-op (the manifest file it patches no longer exists post-
 * migration) — remove those call sites together with this function in the
 * cleanup pass, or wire up a real daemon endpoint first.
 */
export function touchProject(name: string, patch: Partial<Pick<ProjectEntry, "lastOpenedAt" | "records">>): void {
  const list = readManifest();
  const idx = list.findIndex(p => p.name === name);
  if (idx < 0) return;
  list[idx] = { ...list[idx], ...patch };
  writeManifest(list);
}

/**
 * @deprecated Replaced by valori-daemon's `DELETE /v1/projects/:name` (see
 * `ui/src/app/api/projects/[name]/route.ts`), which removes the same
 * directory this function does (both use `~/.valori/projects/<name>/`) via
 * its own manifest. Kept only until this file's cleanup pass.
 *
 * Permanently remove a project: clear protection, delete the data dir, drop the
 * manifest entry. The caller MUST stop the node first. This is the only code
 * path that may delete project data.
 */
export function deleteProject(name: string): boolean {
  const list = readManifest();
  const entry = list.find(p => p.name === name);
  if (!entry) return false;

  unprotectAll(entry);
  try {
    fs.rmSync(entry.dir, { recursive: true, force: true });
  } catch {
    /* dir may already be gone */
  }
  writeManifest(list.filter(p => p.name !== name));
  return true;
}

/**
 * @deprecated Takes a name and re-reads the (now largely empty, post-
 * migration) manifest file internally — routes now call {@link protectAll}
 * directly with an already-loaded entry instead. Kept only until this file's
 * cleanup pass.
 *
 * Apply the immutable flag to a project's data files. Call when a project is at
 * rest (node stopped). The flag blocks ALL writes — including the node's own WAL
 * appends — so it must be cleared via {@link unprotectProject} before the node runs.
 */
export function protectProject(name: string): void {
  reprotect(name);
}

/**
 * @deprecated Same as {@link protectProject} — routes now call {@link
 * unprotectAll} directly with an already-loaded entry instead.
 *
 * Clear protection so a project's node can write its WAL/snapshot while open.
 */
export function unprotectProject(name: string): void {
  const entry = getProject(name);
  if (entry) unprotectAll(entry);
}

// ─── import existing /tmp data (one-time convenience) ────────────────────────────

/**
 * @deprecated Already unused (zero call sites) before this migration, and
 * depends on the now-deprecated {@link createProject}. Kept only until this
 * file's cleanup pass.
 *
 * Copy legacy /tmp/valori-n1.{snap,events.log} into a new named project. Returns
 * the created entry, or null if no legacy files are present.
 */
export function importFromTmp(name: string): ProjectEntry | null {
  const legacySnap = "/tmp/valori-n1.snap";
  const legacyLog  = "/tmp/valori-n1-events.log";
  if (!fs.existsSync(legacySnap) && !fs.existsSync(legacyLog)) return null;

  const entry = createProject({ name, dim: 768, index: "brute" });
  const { snapshotPath, eventLogPath } = projectPaths(entry);
  if (fs.existsSync(legacySnap)) fs.copyFileSync(legacySnap, snapshotPath);
  if (fs.existsSync(legacyLog))  fs.copyFileSync(legacyLog, eventLogPath);
  protectAll(entry);
  return entry;
}
