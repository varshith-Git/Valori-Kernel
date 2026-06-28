"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import {
  Play, Square, RefreshCw, Server, Network, ChevronDown, ChevronUp,
  Terminal, Plus, Trash2, UserPlus, Link2, CheckCircle2, XCircle,
} from "lucide-react";
import type { LaunchConfig, NodeCfg, NodeState, NodeStatus } from "@/lib/server/process-manager";

// ─── constants ───────────────────────────────────────────────────────────────

const DIMENSIONS = [
  { value: 128,  label: "128  — tiny / tests"                         },
  { value: 256,  label: "256  — lightweight"                          },
  { value: 384,  label: "384  — MiniLM-L6-v2, paraphrase-MiniLM"     },
  { value: 512,  label: "512  — CLIP ViT-B/32"                        },
  { value: 768,  label: "768  — BERT-base, all-mpnet-base-v2, nomic"  },
  { value: 1024, label: "1024 — BERT-large, bge-large-en"             },
  { value: 1536, label: "1536 — text-embedding-ada-002, e5-large"     },
  { value: 2048, label: "2048 — e5-mistral-7b"                        },
  { value: 3072, label: "3072 — text-embedding-3-large"               },
  { value: 4096, label: "4096 — Llama / Mistral hidden-state"         },
];

const INDEX_TYPES = [
  { value: "brute", label: "Brute-force L2  - exact, always consistent"      },
  { value: "hnsw",  label: "HNSW graph      - approximate, faster at scale"  },
  { value: "ivf",   label: "IVF             - clustered, best for 100k+ vecs" },
];

// ─── helpers ─────────────────────────────────────────────────────────────────

function buildMembers(nodes: NodeCfg[], host = "localhost"): string {
  return nodes
    .map(n => `${n.id}=${host}:${n.raftPort ?? (3100 + n.id)}/${host}:${n.httpPort}`)
    .join(",");
}

// Advanced/cluster launches persist under ~/.valori/cluster (the everyday
// per-project flow lives on Home and writes to ~/.valori/projects/<name>).
const CLUSTER_DIR = "~/.valori/cluster";

function makeDefaultNodes(count: number): NodeCfg[] {
  return Array.from({ length: count }, (_, i) => {
    const id = i + 1;
    return {
      id,
      httpPort:     3000 + id,
      raftPort:     3100 + id,
      eventLogPath: `${CLUSTER_DIR}/n${id}-events.log`,
      snapshotPath: `${CLUSTER_DIR}/n${id}.snap`,
      raftLogPath:  `${CLUSTER_DIR}/n${id}-raft.redb`,
      clusterInit:  id === 1,
    };
  });
}

function defaultSingle(): LaunchConfig {
  return {
    dim: 768, index: "brute", maxRecords: 1_000_000,
    nodes: [{ id: 1, httpPort: 3000, eventLogPath: `${CLUSTER_DIR}/n1-events.log`, snapshotPath: `${CLUSTER_DIR}/n1.snap` }],
  };
}

function defaultCluster(count = 3): LaunchConfig {
  const nodes = makeDefaultNodes(count);
  return { dim: 768, index: "brute", maxRecords: 1_000_000, nodes, clusterMembers: buildMembers(nodes) };
}

function nextNodeConfig(existing: NodeCfg[]): NodeCfg {
  const maxId   = Math.max(...existing.map(n => n.id));
  const maxHttp = Math.max(...existing.map(n => n.httpPort));
  const maxRaft = Math.max(...existing.map(n => n.raftPort ?? (3100 + n.id)));
  const id = maxId + 1;
  return {
    id,
    httpPort:     maxHttp + 1,
    raftPort:     maxRaft + 1,
    eventLogPath: `${CLUSTER_DIR}/n${id}-events.log`,
    snapshotPath: `${CLUSTER_DIR}/n${id}.snap`,
    raftLogPath:  `${CLUSTER_DIR}/n${id}-raft.redb`,
    clusterInit:  false,
  };
}

// ─── status badge ─────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: NodeStatus | "unknown" }) {
  const ring: Record<string, string> = {
    stopped:  "border-border bg-accent text-muted-foreground",
    starting: "border-amber-500/30 bg-amber-500/15 text-amber-700 animate-pulse",
    running:  "border-emerald-500/30 bg-emerald-500/15 text-emerald-700",
    error:    "border-red-500/30 bg-red-500/15 text-red-700",
    unknown:  "border-border bg-accent text-muted-foreground",
  };
  const dot: Record<string, string> = {
    stopped:  "bg-muted-foreground/50",
    starting: "bg-amber-400 animate-pulse",
    running:  "bg-emerald-400",
    error:    "bg-red-400",
    unknown:  "bg-muted-foreground/50",
  };
  const s = status in ring ? status : "unknown";
  return (
    <span className={`inline-flex items-center gap-1.5 text-[10px] font-mono px-2 py-0.5 rounded-full border ${ring[s]}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${dot[s]}`} />
      {status}
    </span>
  );
}

// ─── log viewer — intentionally always-dark terminal ────────────────────────

function LogViewer({ nodeId }: { nodeId: number }) {
  const [lines, setLines] = useState<string[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);
  const esRef = useRef<EventSource | null>(null);

  useEffect(() => {
    setLines([]);
    if (esRef.current) { esRef.current.close(); esRef.current = null; }
    const es = new EventSource(`/api/launch/logs?nodeId=${nodeId}`);
    esRef.current = es;
    es.onmessage = (e) => {
      try { setLines(prev => [...prev.slice(-800), JSON.parse(e.data) as string]); } catch {}
    };
    es.onerror = () => { es.close(); };
    return () => { es.close(); };
  }, [nodeId]);

  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: "smooth" }); }, [lines]);

  return (
    /* Terminal: intentionally always dark — oklch inline to stay fixed across themes */
    <div
      className="relative h-64 overflow-y-auto rounded-xl border border-border p-3 font-mono text-[11px] leading-relaxed"
      style={{ background: "oklch(0.10 0 0)" }}
    >
      {lines.length === 0
        ? <p className="text-white/30 select-none">Waiting for output…</p>
        : lines.map((l, i) => (
            <div key={i} className={
              l.startsWith("[launcher]") ? "text-white/40"
              : l.startsWith("[err]")    ? "text-red-400"
              : "text-emerald-400"
            }>{l || <br />}</div>
          ))
      }
      <div ref={bottomRef} />
    </div>
  );
}

// ─── form field ──────────────────────────────────────────────────────────────

function F({ label, value, onChange, type = "text", note }: {
  label: string; value: string | number; onChange: (v: string) => void;
  type?: string; note?: string;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-[10px] text-muted-foreground uppercase tracking-wider">{label}</label>
      <input
        type={type} value={value} onChange={e => onChange(e.target.value)}
        className="rounded-lg bg-background border border-input px-3 py-2 text-xs text-foreground font-mono focus:outline-none focus:border-ring placeholder:text-muted-foreground/50"
      />
      {note && <p className="text-[10px] text-muted-foreground/70">{note}</p>}
    </div>
  );
}

function Sel({ label, value, onChange, options, note }: {
  label: string;
  value: string | number;
  onChange: (v: string) => void;
  options: { value: string | number; label: string }[];
  note?: string;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-[10px] text-muted-foreground uppercase tracking-wider">{label}</label>
      <select
        value={value}
        onChange={e => onChange(e.target.value)}
        className="rounded-lg bg-background border border-input px-3 py-2 text-xs text-foreground font-mono focus:outline-none focus:border-ring appearance-none cursor-pointer"
      >
        {options.map(o => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
      {note && <p className="text-[10px] text-muted-foreground/70">{note}</p>}
    </div>
  );
}

// ─── node card ───────────────────────────────────────────────────────────────

function NodeCard({
  nc, idx, status, anyRunning, clusterRunning, onStart, onStop, onRemove, onJoin, onChange,
}: {
  nc: NodeCfg; idx: number; status: NodeState | null;
  anyRunning: boolean; clusterRunning: boolean;
  onStart: () => void; onStop: () => void; onRemove: () => void;
  onJoin: () => void; onChange: (p: Partial<NodeCfg>) => void;
}) {
  const [showLogs, setShowLogs] = useState(false);
  const [joining, setJoining]   = useState(false);
  const [joinMsg, setJoinMsg]   = useState("");
  const isActive  = status?.status === "running" || status?.status === "starting";
  const isStopped = !status || status.status === "stopped" || status.status === "error";

  const handleJoin = async () => {
    setJoining(true); setJoinMsg("Starting and joining…");
    try   { await onJoin(); setJoinMsg("Joined!"); }
    catch (e) { setJoinMsg(`Error: ${String(e)}`); }
    finally   { setJoining(false); }
  };

  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      {/* Header */}
      <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border bg-muted/30">
        <Server size={13} className="text-muted-foreground shrink-0" />
        <span className="text-sm font-semibold text-foreground">Node {nc.id}</span>
        {nc.clusterInit && (
          <span className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-blue-950/60 border border-blue-800/60 text-blue-400">INIT</span>
        )}
        <StatusBadge status={status?.status ?? "stopped"} />
        {status?.pid && <span className="text-[10px] text-muted-foreground font-mono">pid {status.pid}</span>}

        <div className="ml-auto flex items-center gap-1.5">
          {isStopped && clusterRunning && (
            <button
              onClick={handleJoin} disabled={joining}
              className="flex items-center gap-1.5 rounded-md bg-blue-500/20 hover:bg-blue-500/30 border border-blue-500/40 px-3 py-1 text-xs text-blue-700 disabled:opacity-50 transition-colors"
            >
              <UserPlus size={11} />
              {joining ? "Joining…" : "Add & join"}
            </button>
          )}
          {isStopped && !clusterRunning && (
            <button
              onClick={onStart}
              className="flex items-center gap-1.5 rounded-md bg-emerald-500/20 hover:bg-emerald-500/30 border border-emerald-500/40 px-3 py-1 text-xs text-emerald-700 transition-colors"
            >
              <Play size={11} /> Start
            </button>
          )}
          {isActive && (
            <button
              onClick={onStop}
              className="flex items-center gap-1.5 rounded-md bg-red-500/20 hover:bg-red-500/30 border border-red-500/40 px-3 py-1 text-xs text-red-700 transition-colors"
            >
              <Square size={11} /> Stop
            </button>
          )}
          <button
            onClick={() => setShowLogs(s => !s)}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-[10px] text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          >
            <Terminal size={11} />
            {showLogs ? <ChevronUp size={10} /> : <ChevronDown size={10} />}
          </button>
          {isStopped && !anyRunning && idx > 0 && (
            <button
              onClick={onRemove}
              className="rounded-md p-1 text-muted-foreground/50 hover:text-red-400 hover:bg-accent transition-colors"
              title="Remove node"
            >
              <Trash2 size={11} />
            </button>
          )}
        </div>
      </div>

      {joinMsg && (
        <div className={`px-4 py-2 text-[11px] font-mono border-b border-border ${
          joinMsg.startsWith("Error") ? "text-red-700 bg-red-500/10" : "text-blue-700 bg-blue-500/10"
        }`}>
          {joinMsg}
        </div>
      )}

      <div className="px-4 py-3 grid grid-cols-2 gap-2">
        <F label="HTTP Port"       value={nc.httpPort}            onChange={v => onChange({ httpPort: Number(v) })} type="number" />
        <F label="Raft Port"       value={nc.raftPort ?? (3100 + nc.id)} onChange={v => onChange({ raftPort: Number(v) })} type="number" />
        <F label="Event log"       value={nc.eventLogPath ?? ""}  onChange={v => onChange({ eventLogPath: v })} />
        <F label="Snapshot"        value={nc.snapshotPath ?? ""}  onChange={v => onChange({ snapshotPath: v })} />
        <F label="Raft log (redb)" value={nc.raftLogPath ?? ""}   onChange={v => onChange({ raftLogPath: v })} />
      </div>

      {showLogs && (
        <div className="px-4 pb-4">
          <LogViewer nodeId={nc.id} />
        </div>
      )}
    </div>
  );
}

// ─── connect panel ───────────────────────────────────────────────────────────

interface HistoryEntry {
  url: string; lastConnected: string;
  dim?: number; records?: number; status?: string; reachable?: boolean;
}

interface ConnData {
  url: string; reachable: boolean;
  dim?: number; records?: number;
  source: "override" | "env" | "history";
  history: HistoryEntry[];
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60_000);
  if (mins < 1)  return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24)  return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

function ConnectPanel() {
  const [data, setData]       = useState<ConnData | null>(null);
  const [input, setInput]     = useState("");
  const [connecting, setConn] = useState<string | null>(null);
  const [result, setResult]   = useState<{ url: string; ok: boolean; msg: string } | null>(null);

  const load = useCallback(async () => {
    const r = await fetch("/api/connection");
    if (r.ok) {
      const d = await r.json() as ConnData;
      setData(d);
      setInput(prev => prev || d.url);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const connect = async (url: string) => {
    const clean = url.trim();
    if (!clean) return;
    setConn(clean); setResult(null);
    try {
      const r = await fetch("/api/connection", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ url: clean }),
      });
      const d = await r.json() as { ok: boolean; reachable?: boolean; dim?: number; records?: number };
      if (d.reachable) {
        setResult({ url: clean, ok: true, msg: `Connected — dim=${d.dim ?? "?"}, ${(d.records ?? 0).toLocaleString()} records` });
      } else {
        setResult({ url: clean, ok: false, msg: "Node not reachable — URL saved, retry when backend is up" });
      }
      await load();
    } catch (e) {
      setResult({ url: clean, ok: false, msg: String(e) });
    } finally { setConn(null); }
  };

  const isActive = (url: string) => data?.url === url;
  const isBusy   = (url: string) => connecting === url;

  return (
    <div className="flex flex-col gap-5">

      {/* Active connection summary */}
      {data && (
        <div className={`rounded-xl border p-5 flex items-center justify-between gap-4 ${
          data.reachable ? "border-emerald-500/30 bg-emerald-500/10" : "border-border bg-card"
        }`}>
          <div className="flex flex-col gap-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-xs font-medium text-foreground">Active</span>
              <span className={`text-[10px] font-mono px-1.5 py-0.5 rounded border ${
                data.source === "env"     ? "border-border bg-accent text-muted-foreground" :
                data.source === "history" ? "border-blue-800/60 bg-blue-950/40 text-blue-400" :
                                            "border-amber-800/60 bg-amber-950/40 text-amber-400"
              }`}>
                {data.source === "env" ? "VALORI_API_URL" : data.source === "history" ? "auto-restored" : "override"}
              </span>
            </div>
            <code className="text-sm font-mono text-foreground truncate">{data.url}</code>
            {data.reachable && (
              <p className="text-xs text-emerald-500">dim={data.dim ?? "?"} · {(data.records ?? 0).toLocaleString()} records</p>
            )}
          </div>
          <div className="flex items-center gap-3 shrink-0">
            {data.reachable
              ? <span className="flex items-center gap-1 text-xs text-emerald-500 font-medium"><CheckCircle2 size={14} /> Online</span>
              : <span className="flex items-center gap-1 text-xs text-muted-foreground"><XCircle size={14} /> Offline</span>
            }
            <button onClick={load} className="rounded-md p-1.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors" title="Refresh">
              <RefreshCw size={13} />
            </button>
          </div>
        </div>
      )}

      {/* History */}
      {data && data.history.length > 0 && (
        <div className="flex flex-col gap-3">
          <p className="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold">Recent connections</p>
          <div className="flex flex-col gap-2">
            {data.history.map(h => (
              <div
                key={h.url}
                className={`rounded-xl border p-4 flex items-center gap-4 transition-colors ${
                  isActive(h.url)
                    ? "border-[var(--v-accent)]/60 bg-[var(--v-accent-muted)]"
                    : "border-border bg-card hover:border-input"
                }`}
              >
                <span className={`w-2 h-2 rounded-full shrink-0 ${h.reachable ? "bg-emerald-400" : "bg-muted-foreground/40"}`} />
                <div className="flex-1 min-w-0">
                  <code className="text-sm font-mono text-foreground truncate block">{h.url}</code>
                  <div className="flex items-center gap-3 mt-0.5 text-[11px] text-muted-foreground">
                    <span>{relativeTime(h.lastConnected)}</span>
                    {h.dim     && <span>dim={h.dim}</span>}
                    {h.records != null && <span>{h.records.toLocaleString()} records</span>}
                    {isActive(h.url) && <span className="text-[var(--v-accent)] font-medium">● active</span>}
                  </div>
                </div>
                <div className="shrink-0">
                  {isActive(h.url) ? (
                    <span className="text-[11px] text-[var(--v-accent)] font-medium px-3">Connected</span>
                  ) : (
                    <button
                      onClick={() => connect(h.url)}
                      disabled={!!connecting}
                      className="flex items-center gap-1.5 rounded-lg border border-border bg-accent hover:bg-muted px-3 py-1.5 text-xs text-foreground disabled:opacity-40 transition-colors"
                    >
                      {isBusy(h.url) ? <RefreshCw size={11} className="animate-spin" /> : <Play size={11} />}
                      {isBusy(h.url) ? "Connecting…" : "Resume"}
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* New URL */}
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-3">
        <p className="text-xs font-medium text-foreground">Connect to a different node</p>
        <div className="flex gap-2">
          <input
            type="text"
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={e => e.key === "Enter" && connect(input)}
            placeholder="http://localhost:3000"
            className="flex-1 rounded-lg bg-background border border-input px-3 py-2 text-sm text-foreground font-mono focus:outline-none focus:border-ring placeholder:text-muted-foreground/50"
          />
          <button
            onClick={() => connect(input)}
            disabled={!!connecting || !input.trim()}
            className="flex items-center gap-2 rounded-lg bg-emerald-700 hover:bg-emerald-600 disabled:opacity-40 px-4 py-2 text-sm font-medium text-white transition-colors"
          >
            {connecting === input.trim() ? <RefreshCw size={13} className="animate-spin" /> : <Link2 size={13} />}
            {connecting === input.trim() ? "Connecting…" : "Connect"}
          </button>
        </div>

        {result && (
          <div className={`flex items-start gap-2 px-3 py-2 rounded-lg text-xs font-mono ${
            result.ok
              ? "bg-emerald-500/12 text-emerald-700 border border-emerald-500/30"
              : "bg-accent text-muted-foreground border border-border"
          }`}>
            {result.ok ? <CheckCircle2 size={13} className="mt-px shrink-0" /> : <XCircle size={13} className="mt-px shrink-0" />}
            {result.msg}
          </div>
        )}
      </div>

      <p className="text-[11px] text-muted-foreground leading-relaxed">
        Connection URL is saved to <code className="font-mono">~/.valori/ui-connections.json</code> and auto-restored
        when <code className="font-mono">npm run dev</code> restarts. Set <code className="font-mono">VALORI_API_URL</code> to pin a permanent default.
      </p>
    </div>
  );
}

// ─── main page ───────────────────────────────────────────────────────────────

export default function LaunchPage() {
  const [mode, setMode] = useState<"single" | "cluster" | "connect">("single");
  const [singleCfg,  setSingleCfg]  = useState<LaunchConfig>(defaultSingle);
  const [clusterCfg, setClusterCfg] = useState<LaunchConfig>(() => defaultCluster(3));
  const [statuses, setStatuses]     = useState<Record<number, NodeState>>({});
  const [repoRoot, setRepoRoot]     = useState("");

  const fetchStatus = useCallback(async () => {
    try {
      const r = await fetch("/api/launch");
      if (!r.ok) return;
      const d = await r.json() as { nodes: NodeState[]; repoRoot: string };
      setRepoRoot(d.repoRoot);
      const map: Record<number, NodeState> = {};
      for (const n of d.nodes) map[n.id] = n;
      setStatuses(map);
    } catch {}
  }, []);

  useEffect(() => {
    fetchStatus();
    const id = setInterval(fetchStatus, 1500);
    return () => clearInterval(id);
  }, [fetchStatus]);

  const syncMembers = (nodes: NodeCfg[]) =>
    ({ ...clusterCfg, nodes, clusterMembers: buildMembers(nodes) });

  const setClusterNode = (idx: number, patch: Partial<NodeCfg>) => {
    const next = clusterCfg.nodes.map((n, i) => i === idx ? { ...n, ...patch } : n);
    setClusterCfg(syncMembers(next));
  };

  const addNodeToConfig = () => {
    const next = [...clusterCfg.nodes, nextNodeConfig(clusterCfg.nodes)];
    setClusterCfg(syncMembers(next));
  };

  const removeNodeFromConfig = (idx: number) => {
    const next = clusterCfg.nodes.filter((_, i) => i !== idx);
    setClusterCfg(syncMembers(next));
  };

  const startNode = async (cfg: LaunchConfig, nodeIdx: number) => {
    await fetch("/api/launch", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ config: cfg, nodeIdx }),
    });
    fetchStatus();
    const nc = cfg.nodes[nodeIdx];
    setTimeout(async () => {
      await fetch("/api/connection", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ url: `http://localhost:${nc.httpPort}` }),
      }).catch(() => {});
    }, 1500);
  };

  const stopNode = async (id: number) => {
    await fetch(`/api/launch?nodeId=${id}`, { method: "DELETE" });
    fetchStatus();
  };

  const joinNode = async (cfg: LaunchConfig, newNodeIdx: number) => {
    const runningNode = cfg.nodes.find(n => {
      const s = statuses[n.id]?.status;
      return s === "running" || s === "starting";
    });
    if (!runningNode) throw new Error("No running nodes found");
    const res = await fetch("/api/launch/join", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ config: cfg, newNodeIdx, anyRunningPort: runningNode.httpPort }),
    });
    const data = await res.json() as { ok?: boolean; error?: string };
    if (!res.ok || data.error) throw new Error(data.error ?? `HTTP ${res.status}`);
    fetchStatus();
  };

  const clusterRunning = clusterCfg.nodes.some(n => {
    const s = statuses[n.id]?.status;
    return s === "running" || s === "starting";
  });

  const anyNodeRunning = (nodes: NodeCfg[]) =>
    nodes.some(n => { const s = statuses[n.id]?.status; return s === "running" || s === "starting"; });

  const allRunning = (nodes: NodeCfg[]) =>
    nodes.length > 0 && nodes.every(n => statuses[n.id]?.status === "running");

  const sc = singleCfg.nodes[0];
  const singleStatus = statuses[sc.id] ?? null;
  const singleActive = singleStatus?.status === "running" || singleStatus?.status === "starting";

  return (
    <div className="flex flex-col gap-8 max-w-4xl pb-12">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-semibold text-foreground tracking-tight">Cluster Launcher</h1>
        <p className="mt-1.5 text-sm text-muted-foreground">
          Start and manage Valori node processes directly from the UI.
          {repoRoot && <span className="ml-2 font-mono text-[11px] text-muted-foreground">{repoRoot}</span>}
        </p>
      </div>

      {/* Mode toggle */}
      <div className="flex gap-2 items-center">
        {([
          { id: "single",  icon: <Server size={15} />,  label: "Single Node" },
          { id: "cluster", icon: <Network size={15} />, label: "Cluster"      },
          { id: "connect", icon: <Link2 size={15} />,   label: "Connect"      },
        ] as const).map(m => (
          <button
            key={m.id}
            onClick={() => setMode(m.id)}
            className={`flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-medium border transition-colors ${
              mode === m.id
                ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                : "border-border bg-card text-muted-foreground hover:text-foreground hover:bg-accent"
            }`}
          >
            {m.icon}{m.label}
          </button>
        ))}
        <button
          onClick={fetchStatus}
          className="ml-auto flex items-center gap-1.5 rounded-lg border border-border px-3 py-2 text-xs text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
        >
          <RefreshCw size={12} /> Refresh
        </button>
      </div>

      {/* ── Single node ── */}
      {mode === "single" && (
        <div className="rounded-2xl border border-border bg-card p-6 shadow-sm flex flex-col gap-5">
          <div className="flex items-center gap-2">
            <Server size={15} className="text-muted-foreground" />
            <h2 className="text-sm font-semibold text-foreground">Standalone Node</h2>
            {singleStatus && <StatusBadge status={singleStatus.status} />}
          </div>

          <div className="grid grid-cols-2 gap-3">
            <Sel label="Dimension"   value={singleCfg.dim}   onChange={v => setSingleCfg({ ...singleCfg, dim: Number(v) })} options={DIMENSIONS} />
            <F   label="HTTP Port"   value={sc.httpPort}      onChange={v => setSingleCfg({ ...singleCfg, nodes: [{ ...sc, httpPort: Number(v) }] })} type="number" />
            <Sel label="Index type"  value={singleCfg.index}  onChange={v => setSingleCfg({ ...singleCfg, index: v as "brute" | "hnsw" | "ivf" })} options={INDEX_TYPES} />
            <F   label="Max records" value={singleCfg.maxRecords} onChange={v => setSingleCfg({ ...singleCfg, maxRecords: Number(v) })} type="number" />
            <F   label="Event log path" value={sc.eventLogPath ?? ""} onChange={v => setSingleCfg({ ...singleCfg, nodes: [{ ...sc, eventLogPath: v }] })}
                 note="Leave blank for in-memory only" />
            <F   label="Snapshot path"  value={sc.snapshotPath ?? ""}  onChange={v => setSingleCfg({ ...singleCfg, nodes: [{ ...sc, snapshotPath: v }] })} />
            <F   label="Auth token (optional)" value={singleCfg.authToken ?? ""} onChange={v => setSingleCfg({ ...singleCfg, authToken: v })} type="password" />
          </div>

          <div className="flex items-center gap-3">
            {!singleActive ? (
              <button
                onClick={() => startNode(singleCfg, 0)}
                className="flex items-center gap-2 rounded-lg bg-emerald-700 hover:bg-emerald-600 px-5 py-2 text-sm font-medium text-white transition-colors"
              >
                <Play size={14} /> Start node
              </button>
            ) : (
              <button
                onClick={() => stopNode(sc.id)}
                className="flex items-center gap-2 rounded-lg bg-red-900 hover:bg-red-800 border border-red-700/60 px-5 py-2 text-sm font-medium text-red-200 transition-colors"
              >
                <Square size={14} /> Stop node
              </button>
            )}
            {singleStatus?.pid && <span className="text-[10px] text-muted-foreground font-mono">pid {singleStatus.pid}</span>}
          </div>

          {singleStatus && (
            <div>
              <p className="text-xs text-muted-foreground mb-2 flex items-center gap-1.5"><Terminal size={12} />Output</p>
              <LogViewer nodeId={sc.id} />
            </div>
          )}
        </div>
      )}

      {/* ── Cluster ── */}
      {mode === "cluster" && (
        <div className="flex flex-col gap-5">
          {/* Shared config */}
          <div className="rounded-2xl border border-border bg-card p-6 shadow-sm flex flex-col gap-5">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Network size={15} className="text-muted-foreground" />
                <h2 className="text-sm font-semibold text-foreground">Shared configuration</h2>
                <span className="text-[10px] text-muted-foreground font-mono">{clusterCfg.nodes.length} nodes</span>
              </div>
              <div className="flex items-center gap-2">
                {allRunning(clusterCfg.nodes) && (
                  <span className="text-xs text-emerald-500 font-medium">All healthy</span>
                )}
                <div className="flex gap-1.5">
                  {clusterCfg.nodes.map(n => (
                    <StatusBadge key={n.id} status={statuses[n.id]?.status ?? "stopped"} />
                  ))}
                </div>
              </div>
            </div>

            <div className="grid grid-cols-4 gap-3">
              <Sel label="Dimension"   value={clusterCfg.dim}   onChange={v => setClusterCfg({ ...clusterCfg, dim: Number(v) })} options={DIMENSIONS} />
              <Sel label="Index type"  value={clusterCfg.index} onChange={v => setClusterCfg({ ...clusterCfg, index: v as "brute" | "hnsw" | "ivf" })} options={INDEX_TYPES} />
              <F   label="Max records" value={clusterCfg.maxRecords} onChange={v => setClusterCfg({ ...clusterCfg, maxRecords: Number(v) })} type="number" />
              <F   label="Auth token"  value={clusterCfg.authToken ?? ""} onChange={v => setClusterCfg({ ...clusterCfg, authToken: v })} type="password" />
            </div>

            {/* Auto-computed members string */}
            <div className="rounded-lg bg-muted border border-border px-4 py-3">
              <p className="text-[10px] text-muted-foreground uppercase tracking-wider mb-1">VALORI_CLUSTER_MEMBERS (auto-computed)</p>
              <code className="text-[11px] text-foreground font-mono break-all">{clusterCfg.clusterMembers}</code>
            </div>

            {/* Start All / Stop All */}
            <div className="flex items-center gap-3">
              {!anyNodeRunning(clusterCfg.nodes) ? (
                <button
                  onClick={async () => {
                    for (let i = 0; i < clusterCfg.nodes.length; i++) {
                      await startNode(clusterCfg, i);
                      if (i === 0) await new Promise(r => setTimeout(r, 800));
                    }
                  }}
                  className="flex items-center gap-2 rounded-lg bg-emerald-700 hover:bg-emerald-600 px-5 py-2 text-sm font-medium text-white transition-colors"
                >
                  <Play size={14} /> Start all {clusterCfg.nodes.length} nodes
                </button>
              ) : (
                <button
                  onClick={() => clusterCfg.nodes.forEach(n => stopNode(n.id))}
                  className="flex items-center gap-2 rounded-lg bg-red-900 hover:bg-red-800 border border-red-700/60 px-5 py-2 text-sm font-medium text-red-200 transition-colors"
                >
                  <Square size={14} /> Stop all nodes
                </button>
              )}
            </div>
          </div>

          {/* Per-node cards */}
          <div className="flex flex-col gap-3">
            {clusterCfg.nodes.map((nc, idx) => (
              <NodeCard
                key={nc.id} nc={nc} idx={idx}
                status={statuses[nc.id] ?? null}
                anyRunning={anyNodeRunning(clusterCfg.nodes)}
                clusterRunning={clusterRunning}
                onStart={() => startNode(clusterCfg, idx)}
                onStop={() => stopNode(nc.id)}
                onRemove={() => removeNodeFromConfig(idx)}
                onJoin={() => joinNode(clusterCfg, idx)}
                onChange={p => setClusterNode(idx, p)}
              />
            ))}
          </div>

          {/* Add node */}
          <button
            onClick={addNodeToConfig}
            className="flex items-center justify-center gap-2 rounded-xl border-2 border-dashed border-border py-4 text-sm text-muted-foreground hover:border-input hover:text-foreground hover:bg-accent/30 transition-colors"
          >
            <Plus size={15} />
            {clusterRunning ? "Add node (will join running cluster)" : "Add node"}
          </button>
        </div>
      )}

      {/* ── Connect ── */}
      {mode === "connect" && <ConnectPanel />}

      {/* Hint */}
      <div className="rounded-xl border border-border/50 bg-accent/20 px-5 py-4 text-xs text-muted-foreground leading-relaxed">
        <strong className="text-foreground">Binary detection order:</strong>{" "}
        <code className="font-mono text-xs">target/release/valori-node</code> →{" "}
        <code className="font-mono text-xs">target/debug/valori-node</code> →{" "}
        <code className="font-mono text-xs">cargo run -p valori-node</code> (slow first build).
        Run <code className="font-mono text-xs">cargo build -p valori-node --release</code> first for instant starts.
        {" "}<strong className="text-foreground">Adding to a running cluster</strong> starts the new process, waits for it to
        be healthy, then calls the leader&apos;s <code className="font-mono text-xs">/v1/cluster/add-node</code> API.
      </div>
    </div>
  );
}
