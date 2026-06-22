import { spawn, ChildProcess } from "child_process";
import * as fs from "fs";
import * as path from "path";

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
    if (nc.eventLogPath) env.VALORI_EVENT_LOG_PATH = nc.eventLogPath;
    if (nc.snapshotPath) env.VALORI_SNAPSHOT_PATH  = nc.snapshotPath;
    if (cfg.authToken)   env.VALORI_AUTH_TOKEN      = cfg.authToken;
    if (cfg.clusterMembers) {
      env.VALORI_NODE_ID           = String(id);
      env.VALORI_CLUSTER_MEMBERS   = cfg.clusterMembers;
      env.VALORI_RAFT_BIND         = `0.0.0.0:${nc.raftPort ?? (3100 + id)}`;
      if (nc.raftLogPath) env.VALORI_RAFT_LOG_PATH = nc.raftLogPath;
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
