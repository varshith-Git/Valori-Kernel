"use client";

import { useState, useEffect } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { Plus, Layers, Rocket, RefreshCw, FolderOpen, Trash2 } from "lucide-react";
import { useProjectGroups } from "@/lib/hooks/useCollections";
import { useProjects } from "@/lib/hooks/useProjects";
import { useHealth } from "@/lib/hooks/useHealth";
import { CreateProjectDialog } from "@/components/projects/CreateProjectDialog";
import { DeleteProjectDialog } from "@/components/projects/DeleteProjectDialog";

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

function StatTile({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="rounded-xl bg-background border border-border/70 px-4 py-3 flex flex-col gap-1">
      <p className="text-[11px] text-muted-foreground">{label}</p>
      <p className={`text-xl font-bold tracking-tight leading-none ${accent ? "text-[var(--v-accent)]" : "text-foreground"}`}>
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

  // Load persisted daily activity from localStorage
  useEffect(() => {
    try {
      const stored = JSON.parse(localStorage.getItem("valori:activity") ?? "{}") as Record<string, number>;
      setActivity(stored);
    } catch {}
  }, []);

  // Update today's chain height whenever it changes
  useEffect(() => {
    if (!online || !chainHeight) return;
    const today = isoDate(new Date());
    setActivity(prev => {
      if ((prev[today] ?? 0) >= chainHeight) return prev;
      const next = { ...prev, [today]: chainHeight };
      try { localStorage.setItem("valori:activity", JSON.stringify(next)); } catch {}
      return next;
    });
  }, [online, chainHeight]);

  const collectionCount = groups.reduce((s, g) => s + g.collections.length, 0);
  const projectCount    = groups.length;

  const cells    = buildDayGrid(activity);
  const maxDelta = Math.max(...cells.map(c => c.delta), 1);

  const fmtIndex = index
    ? index === "BruteForce" ? "Brute-force" : index
    : "—";

  const row1 = [
    { label: "Records",     value: (recordCount ?? 0).toLocaleString() },
    { label: "Chain events", value: (chainHeight ?? 0).toLocaleString() },
    { label: "Collections", value: collectionCount.toLocaleString() },
    { label: "Projects",    value: projectCount.toLocaleString() },
  ];
  const row2: { label: string; value: string; accent?: boolean }[] = [
    { label: "Dimension",   value: dim ? String(dim) : "—" },
    { label: "Index type",  value: fmtIndex },
    { label: "Capacity",    value: fillPct != null ? `${fillPct.toFixed(1)}%` : "—" },
    { label: "Status",      value: online ? (status ?? "ok") : "offline", accent: online && status === "ok" },
  ];

  return (
    <div className="rounded-2xl border border-border bg-card p-5 flex flex-col gap-4">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <p className="text-sm font-semibold text-foreground">Overview</p>
        {version && (
          <span className="text-[10px] font-mono text-muted-foreground px-2 py-0.5 rounded bg-accent border border-border/60">
            v{version}
          </span>
        )}
      </div>

      {/* Stat tiles — 2 rows of 4 */}
      <div className="grid grid-cols-4 gap-2.5">
        {([...row1, ...row2] as { label: string; value: string; accent?: boolean }[]).map(s => (
          <StatTile key={s.label} label={s.label} value={s.value} accent={s.accent} />
        ))}
      </div>

      {/* Activity heatmap */}
      <div>
        <div
          style={{
            display: "grid",
            gridTemplateRows: "repeat(7, 12px)",
            gridAutoFlow: "column",
            gridAutoColumns: "12px",
            gap: "3px",
          }}
        >
          {cells.map((c, i) => {
            const intensity = c.delta > 0 ? c.delta / maxDelta : 0;
            const bg = intensity === 0
              ? "var(--v-heatmap-empty, hsl(var(--accent)))"
              : `rgba(99, 102, 241, ${Math.max(0.25, intensity)})`;
            return (
              <div
                key={i}
                title={`${c.date}${c.delta > 0 ? ` · +${c.delta} events` : ""}`}
                style={{ borderRadius: "3px", backgroundColor: bg }}
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

function ProjectCard({
  project, collections, isBare, onClick, onDelete,
}: {
  project: string;
  collections: string[];
  isBare: boolean;
  onClick: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      onClick={onClick}
      className="group relative flex flex-col gap-3 rounded-xl border border-border bg-card p-5 cursor-pointer hover:border-input hover:shadow-sm transition-all"
    >
      <div className="flex items-start justify-between">
        <div className="h-8 w-8 rounded-lg bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 flex items-center justify-center">
          <Layers size={14} className="text-[var(--v-accent)]" />
        </div>
        <button
          onClick={e => { e.stopPropagation(); onDelete(); }}
          className="opacity-0 group-hover:opacity-100 rounded-md p-1 text-muted-foreground hover:text-red-400 hover:bg-red-950/30 transition-all"
          title="Delete project"
        >
          <Trash2 size={13} />
        </button>
      </div>

      <div>
        <p className="font-semibold text-foreground truncate">{project}</p>
        {isBare
          ? <p className="text-[11px] text-muted-foreground mt-0.5">bare namespace</p>
          : <p className="text-[11px] text-muted-foreground mt-0.5">
              {collections.length === 0
                ? "no collections yet"
                : `${collections.length} collection${collections.length !== 1 ? "s" : ""}`
              }
            </p>
        }
      </div>

      {!isBare && collections.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {collections.slice(0, 3).map(c => (
            <span key={c} className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-accent border border-border/60 text-muted-foreground truncate max-w-[100px]">
              {c}
            </span>
          ))}
          {collections.length > 3 && (
            <span className="text-[10px] text-muted-foreground px-1">+{collections.length - 3}</span>
          )}
        </div>
      )}
    </div>
  );
}

// ── Home page ─────────────────────────────────────────────────────────────────

export default function HomePage() {
  const router = useRouter();
  const { groups, isLoading, refresh } = useProjectGroups();
  const { drop } = useProjects();
  const { online, recordCount, dim } = useHealth();
  const [createOpen,    setCreateOpen]    = useState(false);
  const [deleteTarget,  setDeleteTarget]  = useState<string | null>(null);

  return (
    <>
      <div className="flex flex-col gap-6 max-w-5xl">

        {/* ── Header ── */}
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold text-foreground tracking-tight">Home</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {online
                ? <>Connected · <span className="font-mono">{(recordCount ?? 0).toLocaleString()}</span> records · dim <span className="font-mono">{dim ?? "—"}</span></>
                : <span className="text-amber-500">Backend offline — <Link href="/launch" className="underline underline-offset-2 hover:text-amber-400">start a node →</Link></span>
              }
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={refresh}
              className="rounded-lg border border-border p-2 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
              title="Refresh"
            >
              <RefreshCw size={14} />
            </button>
            <button
              onClick={() => setCreateOpen(true)}
              className="flex items-center gap-2 rounded-lg bg-[var(--v-accent)] hover:opacity-90 px-4 py-2 text-sm font-medium text-white transition-opacity"
            >
              <Plus size={14} />
              New Project
            </button>
          </div>
        </div>

        {/* ── Offline banner ── */}
        {!online && !isLoading && (
          <div className="rounded-xl border border-amber-800/60 bg-amber-950/20 px-5 py-4 flex items-center justify-between gap-4">
            <div>
              <p className="text-sm font-medium text-amber-300">No backend connection</p>
              <p className="text-xs text-amber-500/80 mt-0.5">
                Start a Valori node first, then your projects and collections will appear here.
              </p>
            </div>
            <Link
              href="/launch"
              className="flex items-center gap-2 shrink-0 rounded-lg border border-amber-700/60 bg-amber-900/40 hover:bg-amber-900/60 px-4 py-2 text-sm font-medium text-amber-300 transition-colors"
            >
              <Rocket size={14} />
              Open Launcher
            </Link>
          </div>
        )}

        {/* ── Stats card ── */}
        <UsageStats />

        {/* ── Projects section header ── */}
        <div className="flex items-center justify-between mt-2">
          <h2 className="text-base font-semibold text-foreground">Projects</h2>
          <Link href="/proof" className="text-xs text-muted-foreground hover:text-foreground transition-colors">
            View proof →
          </Link>
        </div>

        {/* ── Project grid ── */}
        {isLoading ? (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {[1, 2, 3].map(i => (
              <div key={i} className="h-32 animate-pulse rounded-xl bg-accent/60" />
            ))}
          </div>
        ) : groups.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border py-20 flex flex-col items-center gap-5">
            <div className="h-14 w-14 rounded-2xl bg-card border border-border flex items-center justify-center">
              <FolderOpen size={22} className="text-muted-foreground" />
            </div>
            <div className="text-center">
              <p className="text-sm font-medium text-foreground">No projects yet</p>
              <p className="mt-1 text-xs text-muted-foreground max-w-xs">
                Projects group your vector collections. Create one to start ingesting documents.
              </p>
            </div>
            <button
              onClick={() => setCreateOpen(true)}
              className="flex items-center gap-2 rounded-lg border border-border hover:border-[var(--v-accent)] px-5 py-2.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
            >
              <Plus size={14} />
              Create first project
            </button>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {groups.map(g => (
              <ProjectCard
                key={g.project}
                project={g.project}
                collections={g.collections}
                isBare={g.isBare}
                onClick={() => router.push(`/projects/${encodeURIComponent(g.project)}`)}
                onDelete={() => setDeleteTarget(g.project)}
              />
            ))}

            <button
              onClick={() => setCreateOpen(true)}
              className="group flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-border hover:border-[var(--v-accent)]/60 hover:bg-[var(--v-accent-muted)] py-10 text-sm text-muted-foreground transition-colors"
            >
              <div className="h-9 w-9 rounded-xl border border-dashed border-border group-hover:border-[var(--v-accent)]/60 flex items-center justify-center transition-colors">
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
        onCreate={async (name: string) => {
          router.push(`/projects/${encodeURIComponent(name)}`);
        }}
      />

      {deleteTarget && (
        <DeleteProjectDialog
          name={deleteTarget}
          open
          onClose={() => setDeleteTarget(null)}
          onDelete={async () => {
            const group = groups.find(g => g.project === deleteTarget);
            if (!group) return;
            if (group.isBare) {
              await drop(deleteTarget);
            } else {
              for (const col of group.collections) {
                await drop(`${deleteTarget}--${col}`);
              }
            }
            setDeleteTarget(null);
          }}
        />
      )}
    </>
  );
}
