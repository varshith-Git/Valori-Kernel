"use client";

/**
 * Shared ProjectCard — renders identically for both local daemon projects
 * and cloud-hosted projects. The caller decides which variant to use by
 * passing the appropriate `kind` in the data prop.
 *
 * LOCAL:  Provider = "Local", shows port, records, collections, shards.
 * CLOUD:  Provider = cloud name + region badge, shows node URL, replication.
 */

import Link from "next/link";
import { Box, Monitor, Globe, Server, ExternalLink, MoreVertical, Pencil, Copy, Archive, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useState, useRef, useEffect } from "react";

// ── Types ─────────────────────────────────────────────────────────────────────

export interface LocalProjectCardData {
  kind: "local";
  name: string;
  status: "stopped" | "starting" | "running" | "error";
  port: number;
  nodesRunning: number;
  nodesTotal: number;
  shardCount: number;
  records?: number;
  collections?: string[];
  href: string;
}

export interface CloudProjectCardData {
  kind: "cloud";
  id: string;
  name: string;
  status: string;
  region: string;
  replication: number;
  nodeUrl: string | null;
  href: string;
}

export type ProjectCardData = LocalProjectCardData | CloudProjectCardData;

// ── Status badge ──────────────────────────────────────────────────────────────

const LOCAL_STATUS: Record<string, { dot: string; bg: string; text: string; label: string }> = {
  running:  { dot: "bg-emerald-500", bg: "bg-emerald-500/10 border-emerald-500/30", text: "text-emerald-700 dark:text-emerald-400", label: "RUNNING"  },
  starting: { dot: "bg-amber-500",   bg: "bg-amber-500/10 border-amber-500/30",     text: "text-amber-600 dark:text-amber-400",     label: "STARTING" },
  stopped:  { dot: "bg-slate-400",   bg: "bg-slate-100 border-slate-300 dark:bg-slate-800 dark:border-slate-600", text: "text-slate-500 dark:text-slate-400", label: "STOPPED"  },
  error:    { dot: "bg-red-500",     bg: "bg-red-500/10 border-red-500/30",           text: "text-red-600 dark:text-red-400",         label: "ERROR"    },
};

const CLOUD_STATUS: Record<string, { dot: string; bg: string; text: string; label: string }> = {
  active:    { dot: "bg-emerald-500", bg: "bg-emerald-500/10 border-emerald-500/30", text: "text-emerald-700 dark:text-emerald-400", label: "HEALTHY"   },
  creating:  { dot: "bg-amber-500",   bg: "bg-amber-500/10 border-amber-500/30",     text: "text-amber-600 dark:text-amber-400",     label: "CREATING"  },
  error:     { dot: "bg-red-500",     bg: "bg-red-500/10 border-red-500/30",          text: "text-red-600 dark:text-red-400",         label: "ERROR"     },
  stopped:   { dot: "bg-slate-400",   bg: "bg-slate-100 border-slate-300 dark:bg-slate-800 dark:border-slate-600", text: "text-slate-500 dark:text-slate-400", label: "STOPPED"   },
  suspended: { dot: "bg-amber-500",   bg: "bg-amber-500/10 border-amber-500/30",     text: "text-amber-600 dark:text-amber-400",     label: "SUSPENDED" },
};

function StatusPill({ status, map }: {
  status: string;
  map: typeof LOCAL_STATUS;
}) {
  const s = map[status] ?? map.stopped;
  return (
    <span className={cn("inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-semibold border tracking-wide", s.bg, s.text)}>
      <span className={cn("w-1.5 h-1.5 rounded-full shrink-0", s.dot)} />
      {s.label}
    </span>
  );
}

// ── Provider helpers ──────────────────────────────────────────────────────────

function regionToProvider(region: string): string {
  if (/^(us|eu|ap|ca|sa|me|af)-/.test(region)) return "AWS";
  if (region.includes("azure")) return "Azure";
  if (region.includes("gcp") || /^(us-central|europe-west|asia-)/.test(region)) return "GCP";
  return "Cloud";
}

function AwsLogo() {
  return (
    <div className="flex flex-col items-center gap-0">
      <span className="font-black text-[16px] tracking-tight text-foreground leading-none">aws</span>
      <svg width="34" height="9" viewBox="0 0 34 9" fill="none">
        <path d="M5 5.5 Q17 10 29 5.5" stroke="#FF9900" strokeWidth="2.2" strokeLinecap="round" fill="none"/>
        <polygon points="27,2.5 31,5.5 27,8.5" fill="#FF9900"/>
      </svg>
    </div>
  );
}

function ProviderCell({ provider, region }: { provider: string; region: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-1 py-4 px-2">
      <span className="text-[11px] text-muted-foreground">Provider</span>
      {provider === "AWS" ? (
        <AwsLogo />
      ) : provider === "Azure" ? (
        <span className="text-sm font-bold text-blue-600">Azure</span>
      ) : provider === "GCP" ? (
        <span className="text-sm font-bold text-red-500">GCP</span>
      ) : (
        <Globe size={20} className="text-[var(--v-accent)]" />
      )}
      <span className="text-[10px] font-semibold text-muted-foreground font-mono mt-0.5">{region}</span>
    </div>
  );
}

// ── Three-dot menu ────────────────────────────────────────────────────────────

function CardMenu({ onRename, onDuplicate, onArchive, onDelete }: {
  onRename?: () => void;
  onDuplicate?: () => void;
  onArchive?: () => void;
  onDelete?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function h(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, []);

  return (
    <div ref={ref} className="relative" onClick={e => e.preventDefault()}>
      <button
        onClick={e => { e.preventDefault(); e.stopPropagation(); setOpen(v => !v); }}
        className="flex items-center justify-center w-7 h-7 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
        aria-label="Project options"
      >
        <MoreVertical size={14} />
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 z-50 w-40 rounded-xl border border-border bg-card shadow-lg py-1 overflow-hidden">
          {onRename && (
            <button
              onClick={e => { e.stopPropagation(); setOpen(false); onRename(); }}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors text-left"
            >
              <Pencil size={13} /> Rename
            </button>
          )}
          {onDuplicate && (
            <button
              onClick={e => { e.stopPropagation(); setOpen(false); onDuplicate(); }}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors text-left"
            >
              <Copy size={13} /> Duplicate
            </button>
          )}
          {onArchive && (
            <button
              onClick={e => { e.stopPropagation(); setOpen(false); onArchive(); }}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-amber-600 dark:text-amber-400 hover:bg-amber-500/10 transition-colors text-left"
            >
              <Archive size={13} /> Archive
            </button>
          )}
          {onDelete && (
            <>
              {(onRename || onDuplicate || onArchive) && <div className="mx-2 my-0.5 border-t border-border/60" />}
              <button
                onClick={e => { e.stopPropagation(); setOpen(false); onDelete(); }}
                className="w-full flex items-center gap-2 px-3 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-red-500/10 transition-colors text-left"
              >
                <Trash2 size={13} /> Delete
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// ── Stat cell ─────────────────────────────────────────────────────────────────

function StatCell({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5 px-4 py-3">
      <span className="text-[11px] text-muted-foreground">{label}</span>
      <span className="text-lg font-bold text-foreground leading-none">{value}</span>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export function ProjectCard({
  data,
  onRename,
  onDuplicate,
  onArchive,
  onDelete,
}: {
  data: ProjectCardData;
  onRename?: () => void;
  onDuplicate?: () => void;
  onArchive?: () => void;
  onDelete?: () => void;
}) {
  const isLocal = data.kind === "local";
  const local   = isLocal ? (data as LocalProjectCardData) : null;
  const cloud   = !isLocal ? (data as CloudProjectCardData) : null;
  const provider = cloud ? regionToProvider(cloud.region) : null;

  return (
    <div className="rounded-2xl border border-border bg-card hover:border-[var(--v-accent)]/40 hover:shadow-md transition-all duration-200 overflow-hidden flex flex-col">

      {/* Row 1: Icon + Name + Status pill + menu */}
      <div className="flex items-center gap-3 px-4 pt-4 pb-3">
        <div className="w-10 h-10 rounded-xl bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 flex items-center justify-center shrink-0">
          <Box size={18} className="text-[var(--v-accent)]" />
        </div>
        <Link
          href={data.href}
          className="flex-1 min-w-0 text-base font-bold text-foreground hover:text-[var(--v-accent)] transition-colors truncate"
          onClick={e => e.stopPropagation()}
        >
          {data.name}
        </Link>
        {isLocal
          ? <StatusPill status={local!.status} map={LOCAL_STATUS} />
          : <StatusPill status={cloud!.status} map={CLOUD_STATUS} />
        }
        {(onRename || onDuplicate || onArchive || onDelete) && (
          <CardMenu
            onRename={onRename}
            onDuplicate={onDuplicate}
            onArchive={onArchive}
            onDelete={onDelete}
          />
        )}
      </div>

      {/* Divider */}
      <div className="border-t border-border/60" />

      {/* Row 2: 3-column provider / nodes / port|replication */}
      <div className="grid grid-cols-3 divide-x divide-border/60">

        {/* Provider */}
        {isLocal ? (
          <div className="flex flex-col items-center justify-center gap-1 py-4 px-2">
            <span className="text-[11px] text-muted-foreground">Provider</span>
            <div className="flex items-center justify-center w-9 h-9 rounded-xl bg-[var(--v-accent-muted)]">
              <Monitor size={18} className="text-[var(--v-accent)]" />
            </div>
            <span className="text-sm font-bold text-foreground">Local</span>
          </div>
        ) : (
          <ProviderCell provider={provider!} region={cloud!.region} />
        )}

        {/* Nodes */}
        <div className="flex flex-col items-center justify-center gap-1 py-4 px-2">
          <span className="text-[11px] text-muted-foreground">Nodes</span>
          <div className="flex items-center gap-1.5">
            <div className="flex items-center justify-center w-7 h-7 rounded-lg bg-[var(--v-accent-muted)]">
              <Server size={13} className="text-[var(--v-accent)]" />
            </div>
            <span className="text-xl font-bold text-foreground">
              {isLocal ? local!.nodesRunning : cloud!.replication}
            </span>
          </div>
        </div>

        {/* Port / Replication */}
        <div className="flex flex-col items-center justify-center gap-1 py-4 px-2">
          {isLocal ? (
            <>
              <span className="text-[11px] text-muted-foreground">Port</span>
              <span className="text-xl font-bold text-foreground font-mono">{local!.port}</span>
            </>
          ) : (
            <>
              <span className="text-[11px] text-muted-foreground">Replication</span>
              <span className="text-xl font-bold text-foreground">{cloud!.replication}×</span>
            </>
          )}
        </div>
      </div>

      {/* Divider */}
      <div className="border-t border-border/60" />

      {/* Row 3 */}
      {isLocal ? (
        <div className="grid grid-cols-3 divide-x divide-border/60">
          <StatCell label="Records"     value={(local!.records ?? 0).toLocaleString()} />
          <StatCell label="Collections" value={local!.collections?.length ?? 1} />
          <StatCell label="Shards"      value={local!.shardCount} />
        </div>
      ) : (
        <div className="px-4 py-3 flex items-center justify-between gap-2">
          <div className="min-w-0">
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-0.5">Node URL</p>
            <p className="text-xs font-mono text-muted-foreground truncate">{cloud!.nodeUrl ?? "—"}</p>
          </div>
          <Link
            href={data.href}
            className="shrink-0 flex items-center gap-1 text-xs font-semibold text-[var(--v-accent)] hover:opacity-80 transition-opacity border border-[var(--v-accent)]/30 rounded-lg px-2.5 py-1.5"
          >
            Open <ExternalLink size={11} />
          </Link>
        </div>
      )}
    </div>
  );
}
