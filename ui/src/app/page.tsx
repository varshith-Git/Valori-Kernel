"use client";

import { useState, useEffect, useRef } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import {
  Plus, Layers, RefreshCw, FolderOpen, Trash2, Play, Pause,
  ArrowRight, Loader2, Star, Upload, BookOpen, Lightbulb, SquareTerminal,
} from "lucide-react";
import { forgetProject, getFavoriteProjects, toggleFavoriteProject, touchRecentProject } from "@/lib/native";
import { useProjectManifest, type ManifestProject } from "@/lib/hooks/useProjectManifest";
import { useHealth } from "@/lib/hooks/useHealth";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { GettingStarted } from "@/components/home/GettingStarted";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";
import { useRelativeTime } from "@/lib/hooks/useRelativeTime";
import { timeAgo } from "@/lib/time";
import { EVENT_DOT } from "@/lib/event-types";
import { cn } from "@/lib/utils";
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
      const ease = 1 - Math.pow(1 - p, 3);
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

function isoDate(d: Date) { return d.toISOString().slice(0, 10); }

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

// ── Status pill ───────────────────────────────────────────────────────────────

function StatusPill({ status, nodesRunning, nodesTotal }: {
  status: ManifestProject["status"];
  nodesRunning?: number;
  nodesTotal?: number;
}) {
  const map: Record<string, { cls: string; dot: string; label: string }> = {
    running:  { cls: "border-emerald-500/30 bg-emerald-500/12 text-emerald-700 dark:text-emerald-400", dot: "bg-emerald-400", label: "Running" },
    starting: { cls: "border-amber-500/30 bg-amber-500/12 text-amber-700 dark:text-amber-400", dot: "bg-amber-400 animate-pulse", label: "Starting" },
    error:    { cls: "border-red-500/30 bg-red-500/12 text-red-700 dark:text-red-400", dot: "bg-red-400", label: "Error" },
    stopped:  { cls: "border-border bg-accent text-muted-foreground", dot: "bg-muted-foreground/50", label: "Stopped" },
    archived: { cls: "border-border bg-accent text-muted-foreground", dot: "bg-muted-foreground/30", label: "Archived" },
  };
  const s = map[status] ?? map.stopped;
  const label = (nodesTotal && nodesTotal > 1 && status !== "stopped")
    ? `${nodesRunning}/${nodesTotal} nodes`
    : s.label;
  return (
    <span className={`inline-flex items-center gap-1.5 text-[10px] font-medium px-2 py-0.5 rounded-full border ${s.cls}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${s.dot}`} />
      {label}
    </span>
  );
}

// ── Overview card ─────────────────────────────────────────────────────────────

function OverviewCard({ projects }: { projects: ManifestProject[] }) {
  const { online, recordCount, chainHeight, dim, fillPct, index, version, status } = useHealth();
  const [activity, setActivity] = useState<Record<string, number>>({});
  const [prevChain, setPrevChain] = useState<number | null>(null);
  const [chainGlowing, setChainGlowing] = useState(false);

  const recordDisplay  = useCountUp(recordCount);
  const chainDisplay   = useCountUp(chainHeight);
  const collectDisplay = useCountUp(projects.reduce((s, p) => s + (p.collections?.length ?? 0), 0));
  const projectDisplay = useCountUp(projects.length);

  useEffect(() => {
    try {
      const stored = JSON.parse(localStorage.getItem("valori:activity") ?? "{}") as Record<string, number>;
      setActivity(stored);
    } catch {}
  }, []);

  useEffect(() => {
    if (!online || !chainHeight) return;
    const today = isoDate(new Date());
    setActivity(prev => {
      if ((prev[today] ?? 0) >= chainHeight) return prev;
      const next = { ...prev, [today]: chainHeight };
      try { localStorage.setItem("valori:activity", JSON.stringify(next)); } catch {}
      return next;
    });
    if (prevChain !== null && chainHeight > prevChain) {
      setChainGlowing(false);
      requestAnimationFrame(() => requestAnimationFrame(() => setChainGlowing(true)));
      const t = setTimeout(() => setChainGlowing(false), 1200);
      return () => clearTimeout(t);
    }
    setPrevChain(chainHeight);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [online, chainHeight]);

  const cells    = buildDayGrid(activity);
  const maxDelta = Math.max(...cells.map(c => c.delta), 1);
  const fmtIndex = index ? (index === "BruteForce" ? "Brute-force" : index) : "—";

  const stats = [
    { label: "Records",      value: recordDisplay },
    { label: "Chain events", value: chainDisplay,   glow: chainGlowing },
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
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className={`h-2 w-2 rounded-full animate-breathe ${online ? "bg-emerald-500" : "bg-amber-500"}`} />
          <p className="text-sm font-semibold text-foreground">Overview</p>
        </div>
        <div className="flex items-center gap-3 flex-wrap justify-end">
          {metaStats.map(s => (
            <span key={s.label} className="text-[10px] text-muted-foreground font-mono">
              {s.label}{" "}
              <span className={s.accent ? "text-emerald-500" : "text-foreground"}>{s.value}</span>
            </span>
          ))}
          {version && (
            <span className="text-[10px] font-mono text-muted-foreground px-2 py-0.5 rounded bg-accent border border-border/60">
              v{version}
            </span>
          )}
        </div>
      </div>

      <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
        {(stats as { label: string; value: string; accent?: boolean; glow?: boolean }[]).map((s, i) => (
          <div
            key={s.label}
            className={cn(
              "animate-stat-pop rounded-xl bg-background border border-border/70 px-4 py-3 flex flex-col gap-1",
              s.glow && "animate-chain-glow",
            )}
            style={{ animationDelay: `${i * 40}ms`, animationFillMode: "both" }}
          >
            <p className="text-[11px] text-muted-foreground">{s.label}</p>
            <p className="text-xl font-bold tracking-tight leading-none tabular-nums text-foreground">{s.value}</p>
          </div>
        ))}
      </div>

      <div className="flex items-center gap-4 overflow-x-auto py-1">
        <p className="text-[10px] text-muted-foreground/60 font-mono shrink-0">Activity · {WEEKS}w</p>
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
                style={{ borderRadius: "2px", backgroundColor: bg, "--ci": col } as React.CSSProperties}
              />
            );
          })}
        </div>
        <div className="ml-auto shrink-0 flex items-center gap-1 text-[9px] text-muted-foreground/50 font-mono">
          <span>48h ago</span>
          <span className="mx-8">24h ago</span>
          <span>Now</span>
        </div>
      </div>
    </div>
  );
}

// ── Recent Activity panel ─────────────────────────────────────────────────────

type TabFilter = "all" | "running" | "stopped";

function RecentActivityPanel() {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [disabled, setDisabled] = useState(false);
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    fetch("/api/activity?limit=20")
      .then(r => r.json())
      .then((d: { events?: ActivityEvent[]; disabled?: boolean }) => {
        setDisabled(d.disabled === true);
        setEvents(d.events ?? []);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const getStatus = (eventType: string) => {
    if (/delete/i.test(eventType))
      return { label: "Deleted", cls: "bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/20" };
    return { label: "Success", cls: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20" };
  };

  const shown = showAll ? events : events.slice(0, 8);

  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden flex flex-col">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <p className="text-sm font-semibold text-foreground">Recent activity</p>
        <Link href="/audit" className="text-[11px] text-muted-foreground hover:text-foreground transition-colors flex items-center gap-1">
          View timeline <ArrowRight size={11} />
        </Link>
      </div>

      <div className="flex-1 overflow-y-auto divide-y divide-border/60">
        {loading ? (
          <div className="flex items-center justify-center py-8">
            <div className="h-4 w-4 rounded-full border-2 border-[var(--v-accent)] border-t-transparent animate-spin" />
          </div>
        ) : disabled || events.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-10 gap-2">
            <p className="text-xs text-muted-foreground">No activity yet</p>
            <p className="text-[11px] text-muted-foreground/60">Events will appear once you start inserting records.</p>
          </div>
        ) : (
          shown.map(e => {
            const dot = EVENT_DOT[e.event_type] ?? "bg-muted-foreground/40";
            const status = getStatus(e.event_type);
            const label = e.event_type.replace(/([A-Z])/g, " $1").trim();
            const detail = Object.entries(e.detail)
              .slice(0, 2)
              .map(([k, v]) => `${k}=${JSON.stringify(v)}`)
              .join("  ");
            return (
              <div key={e.log_index} className="flex items-center gap-3 px-4 py-2.5 hover:bg-accent/30 transition-colors">
                <span className={`h-2 w-2 rounded-full shrink-0 ${dot}`} />
                <div className="flex-1 min-w-0">
                  <p className="text-xs font-medium text-foreground truncate">{label}</p>
                  <p className="text-[11px] font-mono text-muted-foreground truncate">{detail}</p>
                </div>
                <span className="text-[10px] text-muted-foreground/60 font-mono shrink-0 tabular-nums">
                  {timeAgo(e.timestamp_iso)}
                </span>
                <span className={`text-[10px] px-1.5 py-0.5 rounded-full border font-medium shrink-0 ${status.cls}`}>
                  {status.label}
                </span>
              </div>
            );
          })
        )}
      </div>

      {!loading && events.length > 8 && (
        <div className="border-t border-border px-4 py-2.5 shrink-0">
          <button
            onClick={() => setShowAll(v => !v)}
            className="text-xs text-muted-foreground hover:text-foreground transition-colors flex items-center gap-1"
          >
            {showAll ? "Show less ↑" : `Load more ↓`}
          </button>
        </div>
      )}
    </div>
  );
}

// ── Projects panel ────────────────────────────────────────────────────────────

function ProjectsPanel({
  projects,
  onOpen,
  onClose,
  onDelete,
  onToggleFavorite,
  favorites,
  busyName,
  onCreateProject,
}: {
  projects: ManifestProject[];
  onOpen: (name: string) => void;
  onClose: (name: string) => void;
  onDelete: (name: string) => void;
  onToggleFavorite: (name: string) => void;
  favorites: string[];
  busyName: string | null;
  onCreateProject: () => void;
}) {
  const [tab, setTab] = useState<TabFilter>("all");

  const counts = {
    all:     projects.length,
    running: projects.filter(p => p.status === "running" || p.status === "starting").length,
    stopped: projects.filter(p => p.status === "stopped" || p.status === "error").length,
  };

  const filtered = tab === "all"
    ? projects
    : tab === "running"
    ? projects.filter(p => p.status === "running" || p.status === "starting")
    : projects.filter(p => p.status === "stopped" || p.status === "error");

  const [showAll, setShowAll] = useState(false);
  const SHOW_LIMIT = 6;
  const shown = showAll ? filtered : filtered.slice(0, SHOW_LIMIT);
  const more  = filtered.length - shown.length;

  const tabs: { key: TabFilter; label: string }[] = [
    { key: "all",     label: "All" },
    { key: "running", label: "Running" },
    { key: "stopped", label: "Stopped" },
  ];

  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <p className="text-sm font-semibold text-foreground">Projects</p>
        <button
          onClick={onCreateProject}
          className="flex items-center gap-1.5 text-[11px] font-medium text-[var(--v-accent)] hover:opacity-80 transition-opacity"
        >
          <Plus size={11} /> New
        </button>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-0.5 px-3 py-2 border-b border-border/60 shrink-0">
        {tabs.map(t => (
          <button
            key={t.key}
            onClick={() => { setTab(t.key); setShowAll(false); }}
            className={cn(
              "flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-medium transition-colors",
              tab === t.key
                ? "bg-[var(--v-accent-muted)] text-[var(--v-accent)]"
                : "text-muted-foreground hover:text-foreground hover:bg-accent/60",
            )}
          >
            {t.label}
            <span className={cn("font-mono text-[10px]", tab === t.key ? "text-[var(--v-accent)]" : "text-muted-foreground/60")}>
              {counts[t.key]}
            </span>
          </button>
        ))}
      </div>

      {/* Project rows */}
      <div className="flex-1 overflow-y-auto divide-y divide-border/60">
        {projects.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-12 gap-3">
            <div className="h-10 w-10 rounded-xl border border-dashed border-border flex items-center justify-center">
              <FolderOpen size={18} className="text-muted-foreground" />
            </div>
            <div className="text-center">
              <p className="text-xs font-medium text-foreground">No projects yet</p>
              <p className="text-[11px] text-muted-foreground mt-0.5">Create your first project to get started.</p>
            </div>
            <button
              onClick={onCreateProject}
              className="flex items-center gap-1.5 rounded-lg bg-[var(--v-accent)] px-4 py-2 text-xs font-medium text-white hover:opacity-90"
            >
              <Plus size={12} /> Create project
            </button>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex items-center justify-center py-8">
            <p className="text-xs text-muted-foreground">No {tab} projects</p>
          </div>
        ) : (
          shown.map(p => {
            const isRunning = p.status === "running" || p.status === "starting";
            return (
              <div
                key={p.name}
                onClick={() => !busyName && onOpen(p.name)}
                className="group flex items-center gap-3 px-4 py-3 hover:bg-accent/30 transition-colors cursor-pointer"
              >
                {/* Icon */}
                <div className="h-7 w-7 rounded-lg bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 flex items-center justify-center shrink-0">
                  <Layers size={12} className="text-[var(--v-accent)]" />
                </div>

                {/* Name + meta */}
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-foreground truncate group-hover:text-[var(--v-accent)] transition-colors">
                    {p.name}
                  </p>
                  <p className="text-[11px] text-muted-foreground truncate">
                    {(p.records ?? 0).toLocaleString()} records
                    {p.collections && p.collections.length > 0 && ` · ${p.collections.length} collection${p.collections.length !== 1 ? "s" : ""}`}
                    {p.dim && ` · dim ${p.dim}`}
                  </p>
                </div>

                {/* Controls */}
                <div className="flex items-center gap-1.5 shrink-0">
                  <StatusPill status={p.status} nodesRunning={p.nodesRunning} nodesTotal={p.nodesTotal} />

                  {/* Favorite */}
                  <button
                    onClick={e => { e.stopPropagation(); onToggleFavorite(p.name); }}
                    className={cn(
                      "rounded p-1 transition-all",
                      favorites.includes(p.name)
                        ? "text-amber-500"
                        : "opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-amber-500",
                    )}
                    title={favorites.includes(p.name) ? "Remove favorite" : "Favorite"}
                  >
                    <Star size={12} className={favorites.includes(p.name) ? "fill-current" : ""} />
                  </button>

                  {/* Pause / Play */}
                  {isRunning ? (
                    <button
                      onClick={e => { e.stopPropagation(); onClose(p.name); }}
                      disabled={!!busyName}
                      className="opacity-0 group-hover:opacity-100 h-6 w-6 flex items-center justify-center rounded-md border border-red-500/40 bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-500/20 disabled:opacity-40 transition-all"
                      title="Pause"
                    >
                      {busyName === p.name ? <Loader2 size={10} className="animate-spin" /> : <Pause size={10} className="fill-current" />}
                    </button>
                  ) : (
                    <button
                      onClick={e => { e.stopPropagation(); onOpen(p.name); }}
                      disabled={!!busyName}
                      className="opacity-0 group-hover:opacity-100 h-6 w-6 flex items-center justify-center rounded-md border border-emerald-500/40 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/20 disabled:opacity-40 transition-all"
                      title="Resume"
                    >
                      {busyName === p.name ? <Loader2 size={10} className="animate-spin" /> : <Play size={10} className="fill-current ml-0.5" />}
                    </button>
                  )}

                  {/* Delete */}
                  <button
                    onClick={e => { e.stopPropagation(); onDelete(p.name); }}
                    className="opacity-0 group-hover:opacity-100 rounded p-1 text-muted-foreground hover:text-red-600 hover:bg-red-500/10 transition-all"
                    title="Delete"
                  >
                    <Trash2 size={12} />
                  </button>

                  <ArrowRight size={13} className="text-muted-foreground/40 ml-1" />
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Load more / tip */}
      <div className="border-t border-border/60 px-4 py-2.5 flex items-center justify-between shrink-0">
        {more > 0 ? (
          <button
            onClick={() => setShowAll(v => !v)}
            className="text-xs text-[var(--v-accent)] hover:opacity-80 transition-opacity flex items-center gap-1"
          >
            <Plus size={11} /> {more} more project{more !== 1 ? "s" : ""}
          </button>
        ) : (
          <span />
        )}
        <Link href="/proof" className="text-[11px] text-muted-foreground hover:text-foreground transition-colors">
          View proof →
        </Link>
      </div>
    </div>
  );
}

// ── Quick actions ─────────────────────────────────────────────────────────────

function QuickActions({ onCreateProject }: { onCreateProject: () => void }) {
  const tip = "Create collections and upload documents to start indexing your data.";

  const actions = [
    { Icon: Plus,          label: "New Project",       desc: "Create a new project",    onClick: onCreateProject },
    { Icon: Upload,        label: "Import Snapshot",   desc: "Upload snapshot file",    href: "/snapshots" },
    { Icon: SquareTerminal,label: "Launch Playground", desc: "Test your data",          href: "/playground" },
    { Icon: BookOpen,      label: "Open Docs",         desc: "Read the documentation",  href: "/help" },
  ];

  return (
    <div className="rounded-xl border border-border bg-card px-4 py-4 flex flex-col sm:flex-row gap-4">
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 flex-1">
        {actions.map(a => {
          const inner = (
            <>
              <div className="h-8 w-8 rounded-lg bg-accent border border-border/60 flex items-center justify-center shrink-0">
                <a.Icon size={14} className="text-muted-foreground" />
              </div>
              <div className="text-left">
                <p className="text-xs font-medium text-foreground">{a.label}</p>
                <p className="text-[11px] text-muted-foreground mt-0.5">{a.desc}</p>
              </div>
            </>
          );
          return a.href ? (
            <Link
              key={a.label}
              href={a.href}
              className="flex items-center gap-2.5 rounded-lg px-3 py-2.5 hover:bg-accent/60 transition-colors"
            >
              {inner}
            </Link>
          ) : (
            <button
              key={a.label}
              onClick={a.onClick}
              className="flex items-center gap-2.5 rounded-lg px-3 py-2.5 hover:bg-accent/60 transition-colors text-left"
            >
              {inner}
            </button>
          );
        })}
      </div>
      <div className="hidden sm:flex items-start gap-2.5 border-l border-border pl-4 min-w-[180px] max-w-[220px]">
        <Lightbulb size={14} className="text-[var(--v-accent)] shrink-0 mt-0.5" />
        <div>
          <p className="text-xs font-medium text-foreground">Tip</p>
          <p className="text-[11px] text-muted-foreground mt-1 leading-relaxed">{tip}</p>
          <Link href="/help" className="text-[11px] text-[var(--v-accent)] hover:underline mt-1.5 inline-block">
            Learn more →
          </Link>
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
  const [favorites,    setFavorites]    = useState<string[]>([]);

  useEffect(() => {
    getFavoriteProjects().then(setFavorites).catch(() => {});
  }, []);

  const toggleFavorite = async (name: string) => {
    setFavorites(await toggleFavoriteProject(name));
  };

  const orderedProjects = [...projects].sort((a, b) => {
    const af = favorites.includes(a.name) ? 0 : 1;
    const bf = favorites.includes(b.name) ? 0 : 1;
    return af - bf;
  });

  const handleOpen = async (name: string) => {
    if (busyName) return;
    setBusyName(name);
    touchRecentProject(name).catch(() => {});
    router.push(`/projects/${encodeURIComponent(name)}`);
    await open(name);
    setBusyName(null);
  };

  const handleClose = async (name: string) => {
    if (busyName) return;
    setBusyName(name);
    await close(name);
    setBusyName(null);
  };

  const activeProject = projects.find(p => p.status === "running");

  return (
    <>
      <div className="flex flex-col gap-5 w-full max-w-[1920px] mx-auto">

        {/* ── Header ── */}
        <div className="animate-fade-up flex items-start justify-between gap-4" style={{ animationDelay: "0ms" }}>
          <div>
            <h1 className="text-2xl font-semibold text-foreground tracking-tight">Workspace</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {online ? (
                <>
                  {activeProject && (
                    <span className="font-medium text-foreground mr-1.5">{activeProject.name}</span>
                  )}
                  <span className="font-mono">{(recordCount ?? 0).toLocaleString()}</span> records
                  {" · "}<span className="text-emerald-600 dark:text-emerald-400">● Running</span>
                </>
              ) : (
                "Open a project to start its session — your data stays put between restarts."
              )}
            </p>
          </div>
          <div className="flex items-center gap-2 shrink-0">
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

        {/* ── Getting started ── */}
        <GettingStarted
          projects={projects}
          recordCount={recordCount}
          onCreateProject={() => setCreateOpen(true)}
        />

        {/* ── Overview ── */}
        <div className="animate-fade-up" style={{ animationDelay: "60ms" }}>
          <OverviewCard projects={projects} />
        </div>

        {/* ── Two-column: Recent Activity + Projects ── */}
        {isLoading ? (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
            {[0, 1].map(i => (
              <div key={i} className="h-64 animate-pulse rounded-xl bg-accent/60" style={{ animationDelay: `${i * 80}ms` }} />
            ))}
          </div>
        ) : (
          <div
            className="animate-fade-up grid grid-cols-1 lg:grid-cols-[1fr_1.1fr] gap-4"
            style={{ animationDelay: "120ms" }}
          >
            <RecentActivityPanel />
            <ProjectsPanel
              projects={orderedProjects}
              onOpen={handleOpen}
              onClose={handleClose}
              onDelete={setDeleteTarget}
              onToggleFavorite={toggleFavorite}
              favorites={favorites}
              busyName={busyName}
              onCreateProject={() => setCreateOpen(true)}
            />
          </div>
        )}

        {/* ── Quick actions ── */}
        <div className="animate-fade-up" style={{ animationDelay: "180ms" }}>
          <QuickActions onCreateProject={() => setCreateOpen(true)} />
        </div>

      </div>

      <CreateProjectDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreate={async (name, dim, index, replication, shardCount, embed) => {
          const entry = await create({ name, dim, index, replication, shardCount, embed });
          if (!entry) return;
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
            forgetProject(deleteTarget).catch(() => {});
            setFavorites(prev => prev.filter(n => n !== deleteTarget));
            setDeleteTarget(null);
          }}
        />
      )}
    </>
  );
}
