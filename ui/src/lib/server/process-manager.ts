import { spawn, ChildProcess } from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

/** Expand a leading `~/` to the home dir — the node gets no shell expansion. */
function expandTilde(p?: string): string | undefined {
  if (!p) return p;
  if (p === "~") return os.homedir();
  if (p.startsWith("~/")) return path.join(os.homedir(), p.slice(2));
  return p;
}

export type NodeStatus = "stopped" | "starting" | "running" | "error";

export interface NodeState {
  id: number;
  status: NodeStatus;
  pid?: number;
  exitCode?: number | null;
  startedAt?: string;
  stoppedAt?: string;
  logCursor: number;
}

export interface NodeCfg {
  id: number;
  httpPort: number;
  raftPort?: number;
  eventLogPath?: string;
  snapshotPath?: string;
  raftLogPath?: string;
  clusterInit?: boolean;
}

export interface LaunchConfig {
  dim: number;
  index: "brute" | "hnsw";
  maxRecords: number;
  authToken?: string;
  nodes: NodeCfg[];
  clusterMembers?: string;
}

const MAX_LOGS = 800;

function resolveBinary(repoRoot: string): { cmd: string; args: string[]; label: string } {
  const release = path.join(repoRoot, "target", "release", "valori-node");
  const debug   = path.join(repoRoot, "target", "debug",   "valori-node");
  if (fs.existsSync(release)) return { cmd: release,   args: [],                                    label: "valori-node (release)" };
  if (fs.existsSync(debug))   return { cmd: debug,     args: [],                                    label: "valori-node (debug)"   };
  return                               { cmd: "cargo",  args: ["run", "-p", "valori-node", "--"],   label: "cargo run -p valori-node" };
}

interface ManagedNode {
  state: NodeState;
  logs: string[];  // ring buffer
  proc?: ChildProcess;
}

class ProcessManager {
  private nodes = new Map<number, ManagedNode>();
  readonly repoRoot: string;

  constructor() {
    // process.cwd() inside Next.js is the ui/ dir; go up one to repo root
    this.repoRoot = path.resolve(process.cwd(), "..");
  }

  private ensure(id: number): ManagedNode {
    if (!this.nodes.has(id)) {
      this.nodes.set(id, { state: { id, status: "stopped", logCursor: 0 }, logs: [] });
    }
    return this.nodes.get(id)!;
  }

  private pushLog(node: ManagedNode, line: string) {
    node.logs.push(line);
    node.state.logCursor++;
    if (node.logs.length > MAX_LOGS) node.logs.shift();
  }

  startNode(cfg: LaunchConfig, nodeIdx: number): NodeState {
    const nc = cfg.nodes[nodeIdx];
    const id = nc.id;
    const node = this.ensure(id);

    if (node.state.status === "running" || node.state.status === "starting") {
      return node.state;
    }

    const { cmd, args, label } = resolveBinary(this.repoRoot);

    const env: NodeJS.ProcessEnv = {
      ...process.env,
      VALORI_DIM:         String(cfg.dim),
      VALORI_BIND:        `0.0.0.0:${nc.httpPort}`,
      VALORI_INDEX:       cfg.index,
      VALORI_MAX_RECORDS: String(cfg.maxRecords),
    };
    const eventLogPath = expandTilde(nc.eventLogPath);
    const snapshotPath = expandTilde(nc.snapshotPath);
    const raftLogPath  = expandTilde(nc.raftLogPath);

    // Ensure parent dirs exist (the node won't mkdir -p for us).
    for (const p of [eventLogPath, snapshotPath, raftLogPath]) {
      if (p) { try { fs.mkdirSync(path.dirname(p), { recursive: true }); } catch {} }
    }

    if (eventLogPath) env.VALORI_EVENT_LOG_PATH = eventLogPath;
    if (snapshotPath) env.VALORI_SNAPSHOT_PATH  = snapshotPath;
    if (cfg.authToken) env.VALORI_AUTH_TOKEN     = cfg.authToken;
    if (cfg.clusterMembers) {
      env.VALORI_NODE_ID           = String(id);
      env.VALORI_CLUSTER_MEMBERS   = cfg.clusterMembers;
      env.VALORI_RAFT_BIND         = `0.0.0.0:${nc.raftPort ?? (3100 + id)}`;
      if (raftLogPath) env.VALORI_RAFT_LOG_PATH = raftLogPath;
      if (nc.clusterInit) env.VALORI_CLUSTER_INIT  = "1";
    }

    node.state.status    = "starting";
    node.state.startedAt = new Date().toISOString();
    node.state.stoppedAt = undefined;
    node.state.exitCode  = undefined;
    node.logs            = [];
    node.state.logCursor = 0;

    this.pushLog(node, `[launcher] ${label}`);
    this.pushLog(node, `[launcher] cwd: ${this.repoRoot}`);
    this.pushLog(node, `[launcher] HTTP → 0.0.0.0:${nc.httpPort}   dim=${cfg.dim}  index=${cfg.index}`);
    if (cfg.clusterMembers) {
      this.pushLog(node, `[launcher] Raft → 0.0.0.0:${nc.raftPort ?? (3100 + id)}`);
      this.pushLog(node, `[launcher] members=${cfg.clusterMembers}`);
    }
    this.pushLog(node, "");

    const proc = spawn(cmd, args, { cwd: this.repoRoot, env, stdio: ["ignore", "pipe", "pipe"] });
    node.proc       = proc;
    node.state.pid  = proc.pid;

    const handleOut = (data: Buffer) => {
      data.toString().split("\n").filter(l => l.trim()).forEach(l => this.pushLog(node, l));
      if (node.state.status === "starting") node.state.status = "running";
    };

    proc.stdout?.on("data", handleOut);
    proc.stderr?.on("data", (data: Buffer) => {
      data.toString().split("\n").filter(l => l.trim()).forEach(l => {
        this.pushLog(node, `[err] ${l}`);
      });
      if (node.state.status === "starting") node.state.status = "running";
    });
    proc.on("error", err => {
      this.pushLog(node, `[launcher] spawn error: ${err.message}`);
      node.state.status = "error";
    });
    proc.on("exit", (code, sig) => {
      this.pushLog(node, `[launcher] exited  code=${code}  signal=${sig}`);
      node.state.status    = code === 0 ? "stopped" : "error";
      node.state.exitCode  = code;
      node.state.stoppedAt = new Date().toISOString();
      node.proc = undefined;
    });

    return node.state;
  }

  stopNode(id: number): boolean {
    const node = this.nodes.get(id);
    if (!node?.proc) return false;
    node.proc.kill("SIGTERM");
    node.state.status = "stopped";
    this.pushLog(node, "[launcher] SIGTERM sent");
    return true;
  }

  // ── Per-project lifecycle ─────────────────────────────────────────────────
  //
  // A project is a single-node workspace whose node id == its HTTP port (ports
  // start at 3010, never colliding with cluster ids 1/2/3). Paths are derived
  // from the project's data dir by the caller (see lib/server/projects.ts).

  /** Start a project's node (idempotent — returns existing state if already up). */
  startProject(p: {
    port: number;
    dim: number;
    index: "brute" | "hnsw";
    maxRecords: number;
    snapshotPath: string;
    eventLogPath: string;
    authToken?: string;
  }): NodeState {
    const cfg: LaunchConfig = {
      dim: p.dim,
      index: p.index,
      maxRecords: p.maxRecords,
      authToken: p.authToken,
      nodes: [{ id: p.port, httpPort: p.port, eventLogPath: p.eventLogPath, snapshotPath: p.snapshotPath }],
    };
    return this.startNode(cfg, 0);
  }

  /**
   * Ask the node to write a final snapshot, then stop it. The snapshot keeps the
   * next open instant; the WAL already guarantees durability either way. Returns
   * false if the node wasn't running.
   */
  async snapshotThenStop(port: number, snapshotPath: string): Promise<boolean> {
    const node = this.nodes.get(port);
    if (!node?.proc) return false;
    try {
      await fetch(`http://localhost:${port}/v1/snapshot/save`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: snapshotPath }),
        signal: AbortSignal.timeout(8000),
      });
      this.pushLog(node, "[launcher] snapshot saved before stop");
    } catch {
      this.pushLog(node, "[launcher] snapshot-on-close failed (WAL still durable)");
    }
    return this.stopNode(port);
  }

  /**
   * Re-register a node that was already running before this Next.js server
   * process started (detected by a successful /health probe). Marks it as
   * "running" without spawning a new process, so subsequent Open calls won't
   * try to spawn a second process on the same port.
   */
  markRunning(id: number): void {
    const node = this.ensure(id);
    if (node.state.status === "stopped") {
      node.state.status    = "running";
      node.state.startedAt = new Date().toISOString();
      this.pushLog(node, "[launcher] reconciled — node was already running on this port");
    }
  }

  /** True if a node id is currently starting or running. */
  isRunning(id: number): boolean {
    const s = this.nodes.get(id)?.state.status;
    return s === "running" || s === "starting";
  }

  getStatus(id: number): NodeState | undefined {
    return this.nodes.get(id)?.state;
  }

  getAllStatus(): NodeState[] {
    return Array.from(this.nodes.values()).map(n => n.state);
  }

  /** Returns log lines written after absolute cursor position `since`. */
  getLogs(id: number, since: number): { lines: string[]; cursor: number } {
    const node = this.nodes.get(id);
    if (!node) return { lines: [], cursor: 0 };
    const { logs, state } = node;
    const oldestCursor = state.logCursor - logs.length;
    const startIdx     = Math.max(0, since - oldestCursor);
    return { lines: logs.slice(startIdx), cursor: state.logCursor };
  }
}

// Persist singleton across Next.js hot reloads
declare global { var __valori_pm__: ProcessManager | undefined; }
if (!global.__valori_pm__) global.__valori_pm__ = new ProcessManager();
export const pm = global.__valori_pm__;
