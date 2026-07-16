// Thin 1:1 wrapper around the valori-daemon HTTP API
// (crates/valori-daemon/src/http.rs). This is the ONLY place in `ui/` that
// knows the daemon's URL or wire shape — every lifecycle route calls through
// here instead of talking to `~/.valori/ui-projects.json` or spawning
// processes itself. See RFC-0006 Phase B.1.
//
// Deliberately dumb: no field renaming, no response reshaping, no
// ui/-specific enrichment (embed defaults, "active connection" bookkeeping,
// record-count probing, etc.) — that composition lives in the API routes
// that call this client, not here.

function baseUrl(): string {
  return process.env.VALORI_DAEMON_URL ?? "http://127.0.0.1:8080";
}

export class DaemonError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "DaemonError";
    this.status = status;
  }
}

// The desktop app starts the daemon on launch, but it takes a few seconds to
// become healthy (real startup work: locate/spawn the sidecar, wait for
// /health) — a connection-refused fetch in that window is expected, not a
// real failure, and always safe to retry (nothing reached the server yet).
// Without this, a project-creation click within the first few seconds of
// opening the app surfaces a raw "fetch failed" instead of just working.
const DAEMON_STARTUP_RETRY_MS = 10_000;
const DAEMON_STARTUP_RETRY_INTERVAL_MS = 500;

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response;
  const deadline = Date.now() + DAEMON_STARTUP_RETRY_MS;
  for (;;) {
    try {
      res = await fetch(`${baseUrl()}${path}`, {
        ...init,
        headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
        // Next.js dev server caches fetch() by default; lifecycle state must
        // always be fresh.
        cache: "no-store",
      });
      break;
    } catch (e) {
      if (Date.now() >= deadline) {
        throw new DaemonError(e instanceof Error ? e.message : "daemon unreachable", 503);
      }
      await new Promise((r) => setTimeout(r, DAEMON_STARTUP_RETRY_INTERVAL_MS));
    }
  }
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    let message = body;
    try {
      const parsed = JSON.parse(body);
      if (parsed && typeof parsed.error === "string") message = parsed.error;
    } catch {
      /* body wasn't JSON — use as-is */
    }
    throw new DaemonError(message || `${res.status} ${res.statusText}`, res.status);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// ── Wire types (daemon's native shape — see crates/valori-daemon/src/http.rs
// and src/project.rs). Callers adapt these to whatever shape ui/'s existing
// pages expect; this module never does that adaptation itself. ────────────

export type DaemonRuntimeStatus =
  | "stopped" | "starting" | "running" | "stopping" | "failed" | "recovering";

export interface DaemonNodeStatus {
  name: string;
  status: DaemonRuntimeStatus;
  pid?: number;
  port?: number;
  uptime_secs?: number;
}

export interface DaemonProjectNode {
  id: number;
  http_port: number;
  raft_port?: number;
}

export interface DaemonClusterConfig {
  replication: number;
  nodes: DaemonProjectNode[];
  shard_count: number;
}

export interface DaemonEmbeddingConfig {
  provider?: string;
  model?: string;
  endpoint?: string;
  api_key_ref?: string;
}

export interface DaemonStorageConfig {
  max_records: number;
  protect_at_rest: boolean;
}

export interface DaemonProject {
  id: string;
  name: string;
  dim: number;
  index: string;
  workspace: string;
  restart_policy: "never" | "on_failure" | "always";
  created_at: number; // unix seconds
  last_opened_at?: number;
  cluster?: DaemonClusterConfig;
  embedding: DaemonEmbeddingConfig;
  storage: DaemonStorageConfig;
  status: DaemonNodeStatus;
}

export interface CreateDaemonProjectInput {
  name: string;
  dim: number;
  index?: string;
  workspace?: string;
  cluster?: DaemonClusterConfig;
  embedding?: DaemonEmbeddingConfig;
  storage?: DaemonStorageConfig;
}

// ── Client ──────────────────────────────────────────────────────────────────

export function health(): Promise<{ status: string; service: string; version: string }> {
  return request("/health");
}

export function listProjects(): Promise<{ projects: DaemonProject[] }> {
  return request("/v1/projects");
}

export function createProject(input: CreateDaemonProjectInput): Promise<DaemonProject> {
  return request("/v1/projects", { method: "POST", body: JSON.stringify(input) });
}

export function getProject(name: string): Promise<DaemonProject> {
  return request(`/v1/projects/${encodeURIComponent(name)}`);
}

export function renameProject(name: string, newName: string): Promise<{ project: DaemonProject }> {
  return request(`/v1/projects/${encodeURIComponent(name)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: newName }),
  });
}

export function deleteProject(name: string): Promise<{ deleted: string }> {
  return request(`/v1/projects/${encodeURIComponent(name)}`, { method: "DELETE" });
}

export function startProject(name: string): Promise<DaemonNodeStatus> {
  return request(`/v1/projects/${encodeURIComponent(name)}/start`, { method: "POST" });
}

export function stopProject(name: string): Promise<DaemonNodeStatus> {
  return request(`/v1/projects/${encodeURIComponent(name)}/stop`, { method: "POST" });
}

export function restartProject(name: string): Promise<DaemonNodeStatus> {
  return request(`/v1/projects/${encodeURIComponent(name)}/restart`, { method: "POST" });
}

export function projectLogs(name: string, tail = 200): Promise<{ project: string; logs: string }> {
  return request(`/v1/projects/${encodeURIComponent(name)}/logs?tail=${tail}`);
}

export interface DaemonConfig {
  home: string;
  runtime: unknown;
  version: string;
}

/** The daemon's effective configuration — notably `home` (VALORI_HOME), the
 *  actual root the user picked during onboarding, which may differ from
 *  `~/.valori`. */
export function getConfig(): Promise<DaemonConfig> {
  return request("/v1/config");
}
