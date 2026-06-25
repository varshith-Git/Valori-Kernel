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

/** Port range for per-project nodes. 3000–3009 left for manual/cluster launches. */
const PORT_BASE = 3010;
const PORT_MAX  = 3999;

// ─── types ─────────────────────────────────────────────────────────────────────

export interface ProjectEntry {
  name:          string;   // unique, also the dir name (slug)
  dir:           string;   // absolute path to project data dir
  port:          number;   // HTTP port this project's node binds
  dim:           number;
  index:         "brute" | "hnsw";
  maxRecords:    number;
  createdAt:     string;   // ISO
  lastOpenedAt?: string;   // ISO
  records?:      number;   // last-known record count (cosmetic)
}

// ─── name validation ───────────────────────────────────────────────────────────

/** Project names map 1:1 to directory names — keep them filesystem-safe. */
export function isValidName(name: string): boolean {
  return /^[a-zA-Z0-9](?:[a-zA-Z0-9_-]{0,62})$/.test(name);
}

// ─── manifest IO ───────────────────────────────────────────────────────────────

function readManifest(): ProjectEntry[] {
  try {
    const parsed = JSON.parse(fs.readFileSync(MANIFEST_FILE, "utf8"));
    return Array.isArray(parsed) ? parsed as ProjectEntry[] : [];
  } catch {
    return [];
  }
}

function writeManifest(list: ProjectEntry[]): void {
  fs.mkdirSync(VALORI_HOME, { recursive: true });
  fs.writeFileSync(MANIFEST_FILE, JSON.stringify(list, null, 2));
}

export function listProjects(): ProjectEntry[] {
  return readManifest();
}

export function getProject(name: string): ProjectEntry | undefined {
  return readManifest().find(p => p.name === name);
}

// ─── port allocation ───────────────────────────────────────────────────────────

function allocatePort(existing: ProjectEntry[]): number {
  const used = new Set(existing.map(p => p.port));
  for (let port = PORT_BASE; port <= PORT_MAX; port++) {
    if (!used.has(port)) return port;
  }
  throw new Error("No free port available for new project");
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

const DATA_FILES = ["current.snap", "events.log"];

function protectAll(dir: string): void {
  for (const f of DATA_FILES) {
    const p = path.join(dir, f);
    if (fs.existsSync(p)) protect(p);
  }
}

function unprotectAll(dir: string): void {
  for (const f of DATA_FILES) {
    const p = path.join(dir, f);
    if (fs.existsSync(p)) unprotect(p);
  }
}

// ─── derived paths (consumed by process-manager) ────────────────────────────────

export function projectPaths(entry: ProjectEntry): { snapshotPath: string; eventLogPath: string } {
  return {
    snapshotPath: path.join(entry.dir, "current.snap"),
    eventLogPath: path.join(entry.dir, "events.log"),
  };
}

/** Re-apply the immutable flag to a project's data files (call after a snapshot write). */
export function reprotect(name: string): void {
  const entry = getProject(name);
  if (entry) protectAll(entry.dir);
}

// ─── CRUD ──────────────────────────────────────────────────────────────────────

export interface CreateProjectInput {
  name:        string;
  dim:         number;
  index:       "brute" | "hnsw";
  maxRecords?: number;
}

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

  const entry: ProjectEntry = {
    name:       input.name,
    dir,
    port:       allocatePort(list),
    dim:        input.dim,
    index:      input.index,
    maxRecords: input.maxRecords ?? 1_000_000,
    createdAt:  new Date().toISOString(),
  };

  writeManifest([...list, entry]);
  return entry;
}

/** Patch mutable fields (lastOpenedAt, records). No-op if project is gone. */
export function touchProject(name: string, patch: Partial<Pick<ProjectEntry, "lastOpenedAt" | "records">>): void {
  const list = readManifest();
  const idx = list.findIndex(p => p.name === name);
  if (idx < 0) return;
  list[idx] = { ...list[idx], ...patch };
  writeManifest(list);
}

/**
 * Permanently remove a project: clear protection, delete the data dir, drop the
 * manifest entry. The caller MUST stop the node first. This is the only code
 * path that may delete project data.
 */
export function deleteProject(name: string): boolean {
  const list = readManifest();
  const entry = list.find(p => p.name === name);
  if (!entry) return false;

  unprotectAll(entry.dir);
  try {
    fs.rmSync(entry.dir, { recursive: true, force: true });
  } catch {
    /* dir may already be gone */
  }
  writeManifest(list.filter(p => p.name !== name));
  return true;
}

/**
 * Apply the immutable flag to a project's data files. Call when a project is at
 * rest (node stopped). The flag blocks ALL writes — including the node's own WAL
 * appends — so it must be cleared via {@link unprotectProject} before the node runs.
 */
export function protectProject(name: string): void {
  reprotect(name);
}

/** Clear protection so a project's node can write its WAL/snapshot while open. */
export function unprotectProject(name: string): void {
  const entry = getProject(name);
  if (entry) unprotectAll(entry.dir);
}

// ─── import existing /tmp data (one-time convenience) ────────────────────────────

/**
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
  protectAll(entry.dir);
  return entry;
}
