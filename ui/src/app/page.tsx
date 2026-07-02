"use client";

import { useState, useEffect, useRef } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { Plus, Layers, RefreshCw, FolderOpen, Trash2, Play, Square, Loader2 } from "lucide-react";
import { useProjectGroups } from "@/lib/hooks/useCollections";
import { useProjectManifest, type ManifestProject } from "@/lib/hooks/useProjectManifest";
import { useHealth } from "@/lib/hooks/useHealth";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";

function relativeTime(iso?: string): string {
  if (!iso) return "never opened";
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60_000);
  if (mins < 1)  return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24)  return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

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

const WEEKS = 18;
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

function UsageStats() {
  const { online, recordCount, chainHeight, dim, fillPct, index, version, status } = useHealth();
  const { groups } = useProjectGroups();
  const [activity, setActivity] = useState<Record<string, number>>({});
  const [prevChain, setPrevChain] = useState<number | null>(null);
  const [chainGlowing, setChainGlowing] = useState(false);

  // Count-up animated values
  const recordDisplay  = useCountUp(recordCount);
  const chainDisplay   = useCountUp(chainHeight);
  const collectDisplay = useCountUp(
    groups.reduce((s, g) => s + g.collections.length, 0),
  );
  const projectDisplay = useCountUp(groups.length);

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

  const collectionCount = groups.reduce((s, g) => s + g.collections.length, 0);

  const cells    = buildDayGrid(activity);
  const maxDelta = Math.max(...cells.map(c => c.delta), 1);

  const fmtIndex = index
    ? index === "BruteForce" ? "Brute-force" : index
    : "—";

  const row1 = [
    { label: "Records",       value: recordDisplay },
    { label: "Chain events",  value: chainDisplay,  glow: chainGlowing },
    { label: "Collections",   value: collectDisplay },
    { label: "Projects",      value: projectDisplay },
  ];
  const row2 = [
    { label: "Dimension",   value: dim ? String(dim) : "—" },
    { label: "Index type",  value: fmtIndex },
    { label: "Capacity",    value: fillPct != null ? `${fillPct.toFixed(1)}%` : "—" },
    { label: "Status",      value: online ? (status ?? "ok") : "offline", accent: online && status === "ok" },
  ];

  return (
    <div className="rounded-2xl border border-border bg-card p-5 flex flex-col gap-4">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          {/* Breathing dot — green when online, amber when offline */}
          <span
            className={`inline-block h-2 w-2 rounded-full animate-breathe ${
              online ? "bg-emerald-500" : "bg-amber-500"
            }`}
          />
          <p className="text-sm font-semibold text-foreground">Overview</p>
        </div>
        {version && (
          <span className="text-[10px] font-mono text-muted-foreground px-2 py-0.5 rounded bg-accent border border-border/60">
            v{version}
          </span>
        )}
      </div>

      {/* Stat tiles — 2 rows of 4, staggered pop-in */}
      <div className="grid grid-cols-4 gap-2.5">
        {([...row1, ...row2] as { label: string; value: string; accent?: boolean; glow?: boolean }[]).map(
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

      {/* Activity heatmap with staggered cell entrance */}
      <div className="flex flex-col gap-1.5">
        <p className="text-[10px] text-muted-foreground/60 font-mono">audit activity · {WEEKS} weeks</p>
        <div
          style={{
            display: "grid",
            gridTemplateRows: "repeat(7, 11px)",
            gridAutoFlow: "column",
            gridAutoColumns: "11px",
            gap: "3px",
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
                  borderRadius: "3px",
                  backgroundColor: bg,
                  "--ci": col,
                } as React.CSSProperties}
              />
            );
          })}
        </div>
      </div>

      {/* Fun fact */}
      <p className="text-[11px] text-muted-foreground leading-relaxed border-t border-border/50 pt-3">
        {funFact(recordCount, dim, chainHeight, collectionCount)}
      </p>
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

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={e => {
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onOpen(); }
      }}
      className="card-shimmer animate-fade-up group relative flex flex-col gap-3 rounded-xl border border-border bg-card p-5 cursor-pointer hover:border-input hover:shadow-sm hover:scale-[1.01] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--v-accent)] transition-all duration-200"
      style={{ animationDelay: `${delay}ms`, animationFillMode: "both" }}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="h-8 w-8 rounded-lg bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 flex items-center justify-center transition-transform duration-200 group-hover:scale-110 shrink-0">
          <Layers size={14} className="text-[var(--v-accent)] transition-transform duration-200 group-hover:rotate-12" />
        </div>
        <div className="flex items-center gap-1.5">
          <StatusPill status={project.status} nodesRunning={project.nodesRunning} nodesTotal={project.nodesTotal} />
          <button
            onClick={e => { e.stopPropagation(); onDelete(); }}
            className="opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 rounded-md p-1 text-muted-foreground hover:text-red-700 hover:bg-red-500/15 transition-all"
            title="Delete project (clears lock + removes data)"
          >
            <Trash2 size={13} />
          </button>
        </div>
      </div>

      <div>
        <p className="font-semibold text-foreground truncate">{project.name}</p>
        <p className="text-[11px] text-muted-foreground mt-0.5">
          opened {relativeTime(project.lastOpenedAt)}
          {project.records != null && project.records > 0 && <> · {project.records.toLocaleString()} records</>}
        </p>
        {project.status === "error" && (
          <p className="text-[11px] text-red-500 mt-1">
            Node failed to start —{" "}
            <Link
              href="/logs"
              onClick={e => e.stopPropagation()}
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
        <div className="flex items-center gap-1.5" onClick={e => e.stopPropagation()}>
          {isRunning ? (
            <button
              onClick={onClose}
              disabled={busy}
              className="flex items-center gap-1.5 rounded-md border border-border bg-accent hover:bg-muted px-2.5 py-1 text-[11px] text-foreground disabled:opacity-50 transition-colors"
              title="Snapshot & close session"
            >
              {busy ? <Loader2 size={11} className="animate-spin" /> : <Square size={11} />}
              Close
            </button>
          ) : (
            <button
              onClick={onOpen}
              disabled={busy}
              className="flex items-center gap-1.5 rounded-md border border-emerald-500/40 bg-emerald-500/15 hover:bg-emerald-500/25 px-2.5 py-1 text-[11px] text-emerald-700 disabled:opacity-50 transition-colors"
              title="Open session"
            >
              {busy ? <Loader2 size={11} className="animate-spin" /> : <Play size={11} />}
              Open
            </button>
          )}
        </div>
      </div>
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
      <div className="flex flex-col gap-6 max-w-5xl">

        {/* ── Header ── */}
        <div
          className="animate-fade-up flex items-start justify-between gap-4"
          style={{ animationDelay: "0ms" }}
        >
          <div>
            <h1 className="text-2xl font-semibold text-foreground tracking-tight">Projects</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {online
                ? <>Active session · <span className="font-mono">{(recordCount ?? 0).toLocaleString()}</span> records · dim <span className="font-mono">{dim ?? "—"}</span></>
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
          <UsageStats />
        </div>

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
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
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
            className="animate-fade-up rounded-xl border border-dashed border-border py-20 flex flex-col items-center gap-5"
            style={{ animationDelay: "200ms" }}
          >
            <div className="h-14 w-14 rounded-2xl bg-card border border-border flex items-center justify-center"
              style={{ animation: "breathe 3s ease-in-out infinite" }}
            >
              <FolderOpen size={22} className="text-muted-foreground" />
            </div>
            <div className="text-center">
              <p className="text-sm font-medium text-foreground">No projects yet</p>
              <p className="mt-1 text-xs text-muted-foreground max-w-xs">
                Each project is its own isolated, persistent store under <code className="font-mono">~/.valori/projects</code>.
                Create one to start ingesting documents.
              </p>
            </div>
            <button
              onClick={() => setCreateOpen(true)}
              className="flex items-center gap-2 rounded-lg border border-border hover:border-[var(--v-accent)] hover:scale-[1.03] active:scale-[0.97] px-5 py-2.5 text-sm text-muted-foreground hover:text-foreground transition-all duration-150"
            >
              <Plus size={14} />
              Create first project
            </button>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
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
        onCreate={async (name, dim, index, replication, shardCount) => {
          const entry = await create({ name, dim, index, replication, shardCount });
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
