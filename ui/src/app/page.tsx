"use client";

import { useState, useEffect, useRef } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { Plus, Layers, RefreshCw, FolderOpen, Trash2, Play, Pause, ArrowRight, Loader2 } from "lucide-react";
import { useProjectManifest, type ManifestProject } from "@/lib/hooks/useProjectManifest";
import { useHealth } from "@/lib/hooks/useHealth";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";
import { useRelativeTime } from "@/lib/hooks/useRelativeTime";
import type { ActivityEvent } from "@/app/api/activity/route";


// ── Count-up hook ─────────────────────────────────────────────────────────────

function useCountUp(target: number | null, duration = 800): string {
  const [display, setDisplay] = useState<number | null>(null);
  const raf = useRef<number | null>(null);
  const startRef = useRef<number | null>(null);
  const fromRef  = useRef<number>(0);

  useEffect(() => {
    if (target === null) { setDisplay(null); return; }
    const from = fromRef.current;
    const delta = target - from;
    if (Math.abs(delta) < 1) { setDisplay(target); return; }

    if (raf.current) cancelAnimationFrame(raf.current);
    startRef.current = null;

    const step = (ts: number) => {
      if (!startRef.current) startRef.current = ts;
      const p = Math.min((ts - startRef.current) / duration, 1);
      const ease = 1 - Math.pow(1 - p, 3); // cubic ease-out
      const val = Math.round(from + delta * ease);
      setDisplay(val);
      if (p < 1) {
        raf.current = requestAnimationFrame(step);
      } else {
        fromRef.current = target;
      }
    };
    raf.current = requestAnimationFrame(step);
    return () => { if (raf.current) cancelAnimationFrame(raf.current); };
  }, [target, duration]);

  if (display === null) return "—";
  return display.toLocaleString();
}

// ── Activity heatmap helpers ──────────────────────────────────────────────────

const WEEKS = 48;
const TOTAL_DAYS = WEEKS * 7;

function isoDate(d: Date) {
  return d.toISOString().slice(0, 10);
}

function buildDayGrid(activity: Record<string, number>) {
  const today = new Date();
  const cells: { date: string; delta: number }[] = [];

  let prevVal = 0;
  for (let i = TOTAL_DAYS - 1; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(d.getDate() - i);
    const key = isoDate(d);
    const val = activity[key] ?? 0;
    cells.push({ date: key, delta: val > prevVal ? val - prevVal : 0 });
    if (val > 0) prevVal = val;
  }
  return cells;
}

function funFact(
  records: number | null,
  dim: number | null,
  chainHeight: number | null,
  collections: number,
): string {
  if (records && dim && records > 0) {
    const totalValues = records * dim;
    const mb = (totalValues * 4) / (1024 * 1024);
    if (mb >= 1) {
      return `${records.toLocaleString()} vectors × dim ${dim} = ${mb.toFixed(1)} MB of deterministic fixed-point math — zero floats in the hot path.`;
    }
    return `${totalValues.toLocaleString()} Q16.16 fixed-point values stored across ${collections} collection${collections !== 1 ? "s" : ""}.`;
  }
  if (chainHeight && chainHeight > 0) {
    return `${chainHeight.toLocaleString()} audit events — each one BLAKE3-chained. Tamper with one and the entire chain shatters.`;
  }
  return "Insert your first vectors to start building a tamper-evident audit chain.";
}

// ── Stat tile ─────────────────────────────────────────────────────────────────

function StatTile({
  label,
  value,
  accent,
  glow,
  delay = 0,
}: {
  label: string;
  value: string;
  accent?: boolean;
  glow?: boolean;
  delay?: number;
}) {
  return (
    <div
      className={`animate-stat-pop rounded-xl bg-background border border-border/70 px-4 py-3 flex flex-col gap-1 transition-shadow duration-700 ${
        glow ? "animate-chain-glow" : ""
      }`}
      style={{ animationDelay: `${delay}ms`, animationFillMode: "both" }}
    >
      <p className="text-[11px] text-muted-foreground">{label}</p>
      <p
        className={`text-xl font-bold tracking-tight leading-none tabular-nums ${
          accent ? "text-[var(--v-accent)]" : "text-foreground"
        }`}
      >
        {value}
      </p>
    </div>
  );
}

// ── Usage stats card ──────────────────────────────────────────────────────────

function UsageStats({ projects }: { projects: ManifestProject[] }) {
  const { online, recordCount, chainHeight, dim, fillPct, index, version, status } = useHealth();
  const [activity, setActivity] = useState<Record<string, number>>({});
  const [prevChain, setPrevChain] = useState<number | null>(null);
  const [chainGlowing, setChainGlowing] = useState(false);

  // Count-up animated values
  const recordDisplay  = useCountUp(recordCount);
  const chainDisplay   = useCountUp(chainHeight);
  const collectDisplay = useCountUp(
    projects.reduce((s, p) => s + (p.collections?.length ?? 0), 0),
  );
  const projectDisplay = useCountUp(projects.length);

  // Load persisted daily activity from localStorage
  useEffect(() => {
    try {
      const stored = JSON.parse(localStorage.getItem("valori:activity") ?? "{}") as Record<string, number>;
      setActivity(stored);
    } catch {}
  }, []);

  // Update today's chain height whenever it changes + trigger glow on increment
  useEffect(() => {
    if (!online || !chainHeight) return;
    const today = isoDate(new Date());
    setActivity(prev => {
      if ((prev[today] ?? 0) >= chainHeight) return prev;
      const next = { ...prev, [today]: chainHeight };
      try { localStorage.setItem("valori:activity", JSON.stringify(next)); } catch {}
      return next;
    });

    // Glow pulse when chain height increments
    if (prevChain !== null && chainHeight > prevChain) {
      setChainGlowing(false);
      requestAnimationFrame(() => {
        requestAnimationFrame(() => setChainGlowing(true));
      });
      const t = setTimeout(() => setChainGlowing(false), 1200);
      return () => clearTimeout(t);
    }
    setPrevChain(chainHeight);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [online, chainHeight]);

  const collectionCount = projects.reduce((s, p) => s + (p.collections?.length ?? 0), 0);

  const cells    = buildDayGrid(activity);
  const maxDelta = Math.max(...cells.map(c => c.delta), 1);

  const fmtIndex = index
    ? index === "BruteForce" ? "Brute-force" : index
    : "—";

  const mainStats = [
    { label: "Records",      value: recordDisplay },
    { label: "Chain events", value: chainDisplay,  glow: chainGlowing },
    { label: "Collections",  value: collectDisplay },
    { label: "Projects",     value: projectDisplay },
  ];

  const metaStats = [
    { label: "dim",      value: dim ? String(dim) : "—" },
    { label: "index",    value: fmtIndex },
    { label: "capacity", value: fillPct != null ? `${fillPct.toFixed(1)}%` : "—" },
    { label: "status",   value: online ? (status ?? "ok") : "offline", accent: online && status === "ok" },
  ];

  return (
    <div className="rounded-2xl border border-border bg-card px-5 py-4 flex flex-col gap-3">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span
            className={`inline-block h-2 w-2 rounded-full animate-breathe ${
              online ? "bg-emerald-500" : "bg-amber-500"
            }`}
          />
          <p className="text-sm font-semibold text-foreground">Overview</p>
        </div>
        <div className="flex items-center gap-3">
          {/* Meta stats — compact inline */}
          {metaStats.map((s) => (
            <span key={s.label} className="text-[10px] text-muted-foreground font-mono">
              {s.label}{" "}
              <span className={s.accent ? "text-emerald-500" : "text-foreground"}>
                {s.value}
              </span>
            </span>
          ))}
          {version && (
            <span className="text-[10px] font-mono text-muted-foreground px-2 py-0.5 rounded bg-accent border border-border/60">
              v{version}
            </span>
          )}
        </div>
      </div>

      {/* Stat tiles — single row of 4 */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
        {(mainStats as { label: string; value: string; accent?: boolean; glow?: boolean }[]).map(
          (s, i) => (
            <StatTile
              key={s.label}
              label={s.label}
              value={s.value}
              accent={s.accent}
              glow={s.glow}
              delay={i * 40}
            />
          )
        )}
      </div>

      {/* Activity heatmap */}
      <div className="flex items-center gap-4 overflow-x-auto py-1">
        <p className="text-[10px] text-muted-foreground/60 font-mono shrink-0">activity · {WEEKS}w</p>
        <div
          style={{
            display: "grid",
            gridTemplateRows: "repeat(7, 9px)",
            gridAutoFlow: "column",
            gridAutoColumns: "9px",
            gap: "2px",
          }}
        >
          {cells.map((c, i) => {
            const col = Math.floor(i / 7);
            const intensity = c.delta > 0 ? c.delta / maxDelta : 0;
            const opacity = Math.max(0.3, intensity);
            const bg = intensity === 0
              ? "var(--v-heatmap-empty)"
              : `color-mix(in oklch, var(--v-accent) ${Math.round(opacity * 100)}%, transparent)`;
            return (
              <div
                key={i}
                className="heat-cell"
                title={`${c.date}${c.delta > 0 ? ` · +${c.delta} events` : ""}`}
                style={{
                  borderRadius: "2px",
                  backgroundColor: bg,
                  "--ci": col,
                } as React.CSSProperties}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ── Project card ──────────────────────────────────────────────────────────────

function StatusPill({
  status, nodesRunning, nodesTotal,
}: {
  status: ManifestProject["status"];
  nodesRunning?: number;
  nodesTotal?: number;
}) {
  const map: Record<string, { cls: string; dot: string; label: string }> = {
    running:  { cls: "border-emerald-500/30 bg-emerald-500/12 text-emerald-700", dot: "bg-emerald-400",            label: "running" },
    starting: { cls: "border-amber-500/30 bg-amber-500/12 text-amber-700",       dot: "bg-amber-400 animate-pulse", label: "starting" },
    error:    { cls: "border-red-500/30 bg-red-500/12 text-red-700",             dot: "bg-red-400",                label: "error" },
    stopped:  { cls: "border-border bg-accent text-muted-foreground",            dot: "bg-muted-foreground/50",    label: "at rest" },
  };
  const s = map[status] ?? map.stopped;
  const label = (nodesTotal && nodesTotal > 1 && status !== "stopped")
    ? `${nodesRunning}/${nodesTotal} running`
    : s.label;
  return (
    <span className={`inline-flex items-center gap-1.5 text-[10px] font-mono px-2 py-0.5 rounded-full border ${s.cls}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${s.dot}`} />
      {label}
    </span>
  );
}

function ProjectCard({
  project, busy, onOpen, onClose, onDelete, delay = 0,
}: {
  project: ManifestProject;
  busy: boolean;
  onOpen: () => void;
  onClose: () => void;
  onDelete: () => void;
  delay?: number;
}) {
  const isRunning  = project.status === "running" || project.status === "starting";
  const openedLabel = useRelativeTime(project.lastOpenedAt);

  return (
    <div
      onClick={() => !busy && onOpen()}
      className="card-shimmer animate-fade-up group relative flex flex-col gap-3 rounded-xl border border-border bg-card p-5 hover:border-[var(--v-accent)]/80 hover:shadow-md hover:shadow-[var(--v-accent)]/5 transition-all duration-200 cursor-pointer"
      style={{ animationDelay: `${delay}ms`, animationFillMode: "both" }}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="h-8 w-8 rounded-lg bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 flex items-center justify-center transition-transform duration-200 group-hover:scale-110 shrink-0">
          <Layers size={14} className="text-[var(--v-accent)] transition-transform duration-200 group-hover:rotate-12" />
        </div>
        <div className="flex items-center gap-1.5">
          <StatusPill status={project.status} nodesRunning={project.nodesRunning} nodesTotal={project.nodesTotal} />
          <button
            onClick={(e) => { e.stopPropagation(); onDelete(); }}
            className="opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 rounded-md p-1 text-muted-foreground hover:text-red-700 hover:bg-red-500/15 transition-all"
            title="Delete project (clears lock + removes data)"
          >
            <Trash2 size={13} />
          </button>
        </div>
      </div>

      <div>
        <button
          onClick={(e) => { e.stopPropagation(); !busy && onOpen(); }}
          disabled={busy}
          className="font-semibold text-foreground truncate hover:text-[var(--v-accent)] hover:underline transition-colors text-left focus:outline-none"
        >
          {project.name}
        </button>
        <p className="text-[11px] text-muted-foreground mt-0.5">
          opened {openedLabel}
          {project.records != null && project.records > 0 && <> · {project.records.toLocaleString()} records</>}
          {project.collections && project.collections.length > 0 && <> · {project.collections.length} collection{project.collections.length !== 1 ? 's' : ''}</>}
        </p>
        {project.status === "error" && (
          <p className="text-[11px] text-red-500 mt-1">
            Node failed to start —{" "}
            <Link
              href="/logs"
              onClick={(e) => e.stopPropagation()}
              className="underline hover:text-red-400 transition-colors"
            >
              view logs →
            </Link>
          </p>
        )}
      </div>

      <div className="flex items-center justify-between gap-2 mt-1">
        <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-accent border border-border/60 text-muted-foreground">
          :{project.port} · dim {project.dim}{project.nodesTotal > 1 && ` · ${project.nodesTotal} nodes`}
          {project.shardCount > 1 && ` · ${project.shardCount} shards`}
        </span>
        <div className="flex items-center gap-1.5">
          {isRunning ? (
            <>
              <button
                onClick={(e) => { e.stopPropagation(); onClose(); }}
                disabled={busy}
                className="h-7 w-7 flex items-center justify-center rounded-lg border border-red-500/40 bg-red-500/15 hover:bg-red-500/25 active:scale-[0.95] text-red-700 dark:text-red-400 disabled:opacity-50 transition-all shadow-sm"
                title="Pause session (snapshot state & stop node)"
              >
                {busy ? <Loader2 size={13} className="animate-spin" /> : <Pause size={13} className="fill-current" />}
              </button>
              <button
                onClick={(e) => { e.stopPropagation(); !busy && onOpen(); }}
                disabled={busy}
                className="h-7 w-7 flex items-center justify-center rounded-lg border border-emerald-500/40 bg-emerald-500/15 hover:bg-emerald-500/25 active:scale-[0.95] text-emerald-700 dark:text-emerald-400 disabled:opacity-50 transition-all shadow-sm"
                title="Enter project dashboard"
              >
                {busy ? <Loader2 size={13} className="animate-spin" /> : <ArrowRight size={14} />}
              </button>
            </>
          ) : (
            <button
              onClick={(e) => { e.stopPropagation(); !busy && onOpen(); }}
              disabled={busy}
              className="h-7 w-7 flex items-center justify-center rounded-lg border border-emerald-500/40 bg-emerald-500/15 hover:bg-emerald-500/25 active:scale-[0.95] text-emerald-700 dark:text-emerald-400 disabled:opacity-50 transition-all shadow-sm"
              title="Resume session (start node & open dashboard)"
            >
              {busy ? <Loader2 size={13} className="animate-spin" /> : <Play size={13} className="fill-current ml-0.5" />}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Recent Activity ────────────────────────────────────────────────────────────

const EVENT_DOT: Record<string, string> = {
  InsertRecord:    "bg-emerald-400",
  SoftDeleteRecord:"bg-amber-400",
  DeleteRecord:    "bg-red-400",
  CreateNode:      "bg-blue-400",
  CreateEdge:      "bg-purple-400",
  DeleteNode:      "bg-red-400",
  DeleteEdge:      "bg-red-400",
  CreateNamespace: "bg-sky-400",
  DropNamespace:   "bg-orange-400",
};

function timeAgo(iso: string) {
  const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

function RecentActivity() {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [disabled, setDisabled] = useState(false);

  useEffect(() => {
    fetch("/api/activity?limit=10")
      .then((r) => r.json())
      .then((d: { events?: ActivityEvent[]; disabled?: boolean }) => {
        setDisabled(d.disabled === true);
        setEvents(d.events ?? []);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  if (disabled || (events.length === 0 && !loading)) return null;

  return (
    <div className="animate-fade-up rounded-xl border border-border bg-card overflow-hidden" style={{ animationDelay: "140ms" }}>
      <div className="flex items-center justify-between px-4 py-3 border-b border-border">
        <p className="text-xs font-semibold text-foreground">Recent activity</p>
        <Link href="/search" className="text-[11px] text-muted-foreground hover:text-foreground transition-colors">
          View timeline →
        </Link>
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-6">
          <div className="h-4 w-4 rounded-full border-2 border-[var(--v-accent)] border-t-transparent animate-spin" />
        </div>
      ) : (
        <div className="divide-y divide-border">
          {events.map((e) => {
            const dot = EVENT_DOT[e.event_type] ?? "bg-muted-foreground/40";
            const detail = Object.entries(e.detail)
              .map(([k, v]) => `${k}=${JSON.stringify(v)}`)
              .join("  ");
            return (
              <div key={e.log_index} className="flex items-center gap-3 px-4 py-2 text-xs hover:bg-accent/30 transition-colors">
                <span className={`h-2 w-2 rounded-full shrink-0 ${dot}`} />
                <span className="font-mono font-medium text-foreground shrink-0 w-36 truncate">{e.event_type}</span>
                <span className="font-mono text-muted-foreground flex-1 min-w-0 truncate" title={detail}>{detail}</span>
                <span className="font-mono text-[10px] text-muted-foreground/60 shrink-0 tabular-nums">{timeAgo(e.timestamp_iso)}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ── Home page ─────────────────────────────────────────────────────────────────

export default function HomePage() {
  const router = useRouter();
  const { projects, isLoading, create, open, close, remove, refresh } = useProjectManifest();
  const { online, recordCount, dim } = useHealth();
  const [createOpen,   setCreateOpen]   = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [busyName,     setBusyName]     = useState<string | null>(null);

  const handleOpen = async (name: string) => {
    setBusyName(name);
    const ok = await open(name);
    setBusyName(null);
    if (ok) router.push(`/projects/${encodeURIComponent(name)}`);
  };

  const handleClose = async (name: string) => {
    setBusyName(name);
    await close(name);
    setBusyName(null);
  };

  return (
    <>
      <div className="flex flex-col gap-6 w-full max-w-[1920px] mx-auto">

        {/* ── Header ── */}
        <div
          className="animate-fade-up flex items-start justify-between gap-4"
          style={{ animationDelay: "0ms" }}
        >
          <div>
            <h1 className="text-2xl font-semibold text-foreground tracking-tight">Projects</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {online
                ? <>
                    {projects.find(p => p.status === "running") && (
                      <span className="font-medium text-foreground mr-1.5">
                        {projects.find(p => p.status === "running")!.name}
                      </span>
                    )}
                    <span className="font-mono">{(recordCount ?? 0).toLocaleString()}</span> records · dim <span className="font-mono">{dim ?? "—"}</span>
                  </>
                : <>Open a project to start its session — your data stays put between restarts.</>
              }
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => refresh()}
              className="rounded-lg border border-border p-2 text-muted-foreground hover:text-foreground hover:bg-accent hover:rotate-180 transition-all duration-500"
              title="Refresh"
            >
              <RefreshCw size={14} />
            </button>
            <button
              onClick={() => setCreateOpen(true)}
              className="flex items-center gap-2 rounded-lg bg-[var(--v-accent)] hover:opacity-90 hover:scale-[1.03] active:scale-[0.97] px-4 py-2 text-sm font-medium text-white transition-all duration-150"
            >
              <Plus size={14} />
              New Project
            </button>
          </div>
        </div>

        {/* ── Stats card ── */}
        <div
          className="animate-fade-up"
          style={{ animationDelay: "80ms" }}
        >
          <UsageStats projects={projects} />
        </div>

        {/* ── Recent Activity ── */}
        <RecentActivity />

        {/* ── Projects section header ── */}
        <div
          className="animate-fade-up flex items-center justify-between mt-2"
          style={{ animationDelay: "160ms" }}
        >
          <h2 className="text-base font-semibold text-foreground">
            All projects {projects.length > 0 && <span className="text-muted-foreground font-normal">· {projects.length}</span>}
          </h2>
          <Link href="/proof" className="text-xs text-muted-foreground hover:text-foreground transition-colors">
            View proof →
          </Link>
        </div>

        {/* ── Project grid ── */}
        {isLoading ? (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5 gap-4">
            {[1, 2, 3].map((_, i) => (
              <div
                key={i}
                className="h-32 animate-pulse rounded-xl bg-accent/60"
                style={{ animationDelay: `${i * 80}ms` }}
              />
            ))}
          </div>
        ) : projects.length === 0 ? (
          <div
            className="animate-fade-up flex flex-col gap-6"
            style={{ animationDelay: "200ms" }}
          >
            {/* Quick-start guide */}
            <div className="rounded-xl border border-[var(--v-accent)]/30 bg-[var(--v-accent-muted)] p-6">
              <p className="text-xs font-semibold uppercase tracking-widest text-[var(--v-accent)] mb-4">
                Get started in 3 steps
              </p>
              <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
                {[
                  {
                    step: "1",
                    title: "Create a project",
                    desc: "Each project is an isolated, persistent vector store. Pick a name, leave dim at 768 for most embedding models.",
                    action: "Create project →",
                    onClick: () => setCreateOpen(true),
                  },
                  {
                    step: "2",
                    title: "Upload documents",
                    desc: "Drop a PDF, DOCX, or TXT file. Valori chunks, embeds, and stores it — all linked in a tamper-evident audit chain.",
                    action: null,
                    onClick: null,
                  },
                  {
                    step: "3",
                    title: "Search & ask",
                    desc: "Run vector similarity search or ask natural-language questions. Every answer is linked to its source chunks.",
                    action: null,
                    onClick: null,
                  },
                ].map((s) => (
                  <div key={s.step} className="flex flex-col gap-2">
                    <div className="flex items-center gap-2">
                      <span className="h-5 w-5 rounded-full bg-[var(--v-accent)] text-white text-[10px] font-bold flex items-center justify-center flex-shrink-0">
                        {s.step}
                      </span>
                      <span className="text-sm font-medium text-foreground">{s.title}</span>
                    </div>
                    <p className="text-xs text-muted-foreground leading-relaxed">{s.desc}</p>
                    {s.action && s.onClick && (
                      <button
                        onClick={s.onClick}
                        className="mt-1 self-start text-xs font-medium text-[var(--v-accent)] hover:underline"
                      >
                        {s.action}
                      </button>
                    )}
                  </div>
                ))}
              </div>
            </div>

            {/* Quick-start preset CTA */}
            <div className="rounded-xl border border-dashed border-border py-12 flex flex-col items-center gap-4">
              <div className="h-12 w-12 rounded-2xl bg-card border border-border flex items-center justify-center">
                <FolderOpen size={20} className="text-muted-foreground" />
              </div>
              <div className="text-center">
                <p className="text-sm font-medium text-foreground">No projects yet</p>
                <p className="mt-1 text-xs text-muted-foreground">
                  Recommended preset: <span className="font-mono">dim 768 · brute · 1M records</span> — works with OpenAI, nomic, and most open models.
                </p>
              </div>
              <button
                onClick={() => setCreateOpen(true)}
                className="flex items-center gap-2 rounded-lg bg-[var(--v-accent)] px-5 py-2.5 text-sm font-medium text-white hover:opacity-90 transition-opacity"
              >
                <Plus size={14} />
                Create first project
              </button>
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5 gap-4">
            {projects.map((p, i) => (
              <ProjectCard
                key={p.name}
                project={p}
                busy={busyName === p.name}
                onOpen={() => handleOpen(p.name)}
                onClose={() => handleClose(p.name)}
                onDelete={() => setDeleteTarget(p.name)}
                delay={200 + i * 60}
              />
            ))}

            <button
              onClick={() => setCreateOpen(true)}
              className="animate-fade-up group flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-border hover:border-[var(--v-accent)]/60 hover:bg-[var(--v-accent-muted)] py-10 text-sm text-muted-foreground transition-all duration-200 hover:scale-[1.01]"
              style={{ animationDelay: `${200 + projects.length * 60}ms`, animationFillMode: "both" }}
            >
              <div className="h-9 w-9 rounded-xl border border-dashed border-border group-hover:border-[var(--v-accent)]/60 flex items-center justify-center transition-all duration-200 group-hover:scale-110">
                <Plus size={16} className="group-hover:text-[var(--v-accent)] transition-colors" />
              </div>
              <span className="group-hover:text-foreground transition-colors">New Project</span>
            </button>
          </div>
        )}
      </div>

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreate={async (name, dim, index, replication, shardCount, embed) => {
          const entry = await create({ name, dim, index, replication, shardCount, embed });
          if (!entry) return;
          // Boot the new project's node(s) and route into it.
          setBusyName(name);
          const ok = await open(name);
          setBusyName(null);
          if (ok) router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />

      {deleteTarget && (
        <DeleteProjectDialog
          name={deleteTarget}
          open
          onClose={() => setDeleteTarget(null)}
          onDelete={async () => {
            await remove(deleteTarget);
            setDeleteTarget(null);
          }}
        />
      )}
    </>
  );
}
