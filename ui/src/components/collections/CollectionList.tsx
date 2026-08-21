"use client";

import { useState, useRef, useEffect } from "react";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { CreateCollectionDialog } from "./CreateCollectionDialog";
import { DeleteCollectionDialog } from "./DeleteCollectionDialog";
import { useHealth } from "@/lib/hooks/useHealth";
import { CollectionMeta } from "@/lib/hooks/useCollections";
import {
  Users, Wrench, Terminal, Database, Layers, BookOpen,
  LayoutGrid, List, Plus, MoreHorizontal, ArrowRight, Trash2,
} from "lucide-react";
import { cn } from "@/lib/utils";

const ICON_VARIANTS = [
  { Icon: Users,    bg: "bg-blue-500/10",    color: "text-blue-500" },
  { Icon: Wrench,   bg: "bg-rose-500/10",    color: "text-rose-500" },
  { Icon: Terminal, bg: "bg-emerald-500/10", color: "text-emerald-600 dark:text-emerald-400" },
  { Icon: Database, bg: "bg-purple-500/10",  color: "text-purple-500" },
  { Icon: Layers,   bg: "bg-amber-500/10",   color: "text-amber-500" },
  { Icon: BookOpen, bg: "bg-cyan-500/10",    color: "text-cyan-500" },
];

function getVariant(name: string) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff;
  return ICON_VARIANTS[h % ICON_VARIANTS.length];
}

interface Props {
  project: string;
  collections: string[];
  collectionDetails?: Map<string, CollectionMeta>;
  isLoading: boolean;
  onCreate: (name: string, dim: number, index?: "brute" | "hnsw" | "ivf" | "bq" | "auto") => Promise<void>;
  onDrop: (name: string) => Promise<void>;
}

export function CollectionList({ project, collections, collectionDetails, isLoading, onCreate, onDrop }: Props) {
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const { dim, online } = useHealth();

  const uniqueCollections = Array.from(new Set(collections));

  return (
    <div className="flex flex-col gap-5">
      {/* Section header */}
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2.5">
            <h2 className="text-base font-semibold text-foreground">Collections</h2>
            {!isLoading && (
              <span className="text-xs font-medium bg-muted text-muted-foreground rounded-full px-2 py-0.5 border border-border">
                {uniqueCollections.length}
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground mt-0.5">
            Manage and monitor collections in this project.
          </p>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <div className="flex items-center rounded-lg border border-border bg-card p-0.5 gap-0.5">
            <button
              onClick={() => setViewMode("grid")}
              className={cn("p-1.5 rounded-md transition-colors", viewMode === "grid" ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground")}
              aria-label="Grid view"
            >
              <LayoutGrid size={13} />
            </button>
            <button
              onClick={() => setViewMode("list")}
              className={cn("p-1.5 rounded-md transition-colors", viewMode === "list" ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground")}
              aria-label="List view"
            >
              <List size={13} />
            </button>
          </div>
          <Button
            size="sm"
            onClick={() => setCreateOpen(true)}
            className="gap-1.5 h-8 text-xs"
          >
            <Plus size={13} /> New collection
          </Button>
        </div>
      </div>

      {/* Content */}
      {isLoading ? (
        <div className={viewMode === "grid"
          ? "grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
          : "flex flex-col gap-2"
        }>
          {[1, 2, 3].map((i) => (
            <div key={i} className={cn("animate-pulse rounded-xl bg-accent", viewMode === "grid" ? "h-48" : "h-16")} />
          ))}
        </div>
      ) : uniqueCollections.length === 0 ? (
        <EmptyState onCreateClick={() => setCreateOpen(true)} />
      ) : (
        <>
          <div className={viewMode === "grid"
            ? "grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
            : "flex flex-col gap-2"
          }>
            {uniqueCollections.map((col) => (
              <CollectionCard
                key={col}
                project={project}
                collection={col}
                meta={collectionDetails?.get(col)}
                dim={dim}
                online={online}
                viewMode={viewMode}
                onDelete={() => setDeleteTarget(col)}
              />
            ))}
          </div>
        </>
      )}

      <CreateCollectionDialog
        project={project}
        existingCollections={uniqueCollections}
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={onCreate}
      />
      {deleteTarget && (
        <DeleteCollectionDialog
          project={project}
          collection={deleteTarget}
          open={!!deleteTarget}
          onOpenChange={(o) => !o && setDeleteTarget(null)}
          onDelete={async () => { await onDrop(deleteTarget); setDeleteTarget(null); }}
        />
      )}
    </div>
  );
}

function EmptyState({ onCreateClick, dimmed }: { onCreateClick: () => void; dimmed?: boolean }) {
  return (
    <div className={cn(
      "rounded-xl border border-dashed border-border py-10 text-center",
      dimmed && "opacity-60"
    )}>
      <div className="flex justify-center mb-3">
        <div className="w-10 h-10 rounded-xl bg-[var(--v-accent-muted)] flex items-center justify-center">
          <Layers size={18} className="text-[var(--v-accent)]" />
        </div>
      </div>
      <p className="text-sm font-medium text-foreground">Get started with collections</p>
      <p className="mt-1 text-xs text-muted-foreground">
        Create your first collection to store and manage vectors.
      </p>
      <Button size="sm" onClick={onCreateClick} className="mt-4 gap-1.5 text-xs h-8">
        <Plus size={13} /> New collection
      </Button>
    </div>
  );
}

function CardMenu({ onDelete, href }: { onDelete: () => void; href: string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function onClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  return (
    <div ref={ref} className="relative" onClick={(e) => e.preventDefault()}>
      <button
        onClick={(e) => { e.preventDefault(); e.stopPropagation(); setOpen((v) => !v); }}
        className="flex items-center justify-center w-6 h-6 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
      >
        <MoreHorizontal size={14} />
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 z-50 w-36 rounded-xl border border-border bg-card shadow-lg py-1 overflow-hidden">
          <Link
            href={href}
            onClick={() => setOpen(false)}
            className="flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
          >
            <ArrowRight size={13} /> Open
          </Link>
          <div className="mx-2 my-0.5 border-t border-border/60" />
          <button
            onClick={(e) => { e.stopPropagation(); setOpen(false); onDelete(); }}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-red-500/10 transition-colors text-left"
          >
            <Trash2 size={13} /> Delete
          </button>
        </div>
      )}
    </div>
  );
}

function CollectionCard({
  project,
  collection,
  meta,
  dim,
  online,
  viewMode,
  onDelete,
}: {
  project: string;
  collection: string;
  meta?: CollectionMeta;
  dim: number | null;
  online: boolean;
  viewMode: "grid" | "list";
  onDelete: () => void;
}) {
  const href = `/projects/${encodeURIComponent(project)}/${encodeURIComponent(collection)}`;
  const { Icon, bg, color } = getVariant(collection);

  const dimension = meta?.dimension ?? dim ?? 128;
  // "index" from the collection list reflects desired_index (creation-time config),
  // not the live ANN lifecycle state (see IndexLifecycleTab for that). Treat absent or
  // "brute" as "no dedicated ANN index" — show "No Index" rather than the internal name.
  const rawIndex = meta?.index;
  const hasAnn = rawIndex && rawIndex !== "brute";
  const indexLabel = hasAnn ? rawIndex.toUpperCase() : null;
  const recordCount = meta?.recordCount ?? 0;
  const maxRecords = meta?.maxRecords ?? 1000000;
  const pct = Math.min(100, Math.max((recordCount / maxRecords) * 100, recordCount > 0 ? 3 : 0));
  const formattedCount = recordCount.toLocaleString();
  const formattedMax = maxRecords >= 1000000 ? `${(maxRecords / 1000000).toFixed(0)}M` : maxRecords.toLocaleString();

  if (viewMode === "list") {
    return (
      <Link
        href={href}
        className="flex items-center gap-4 rounded-xl border border-border bg-card px-4 py-3 hover:border-input hover:bg-accent/30 transition-colors group shadow-xs"
      >
        <div className={cn("w-8.5 h-8.5 rounded-lg flex items-center justify-center shrink-0", bg)}>
          <Icon size={16} className={color} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <p className="text-sm font-semibold text-foreground truncate">{collection}</p>
            {indexLabel ? (
              <span className="text-[10px] font-mono font-bold tracking-wider uppercase px-2 py-0.5 rounded-md bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20">
                {indexLabel}
              </span>
            ) : (
              <span className="text-[10px] font-mono px-2 py-0.5 rounded-md bg-muted text-muted-foreground border border-border">
                No Index
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground font-mono truncate">{project}--{collection}</p>
        </div>

        {/* Horizontal capacity bar in list view */}
        <div className="hidden sm:flex flex-col gap-1 w-44 shrink-0">
          <div className="flex justify-between items-center text-[10px]">
            <span className="text-muted-foreground">Vectors</span>
            <span className="font-mono font-medium text-foreground">{formattedCount} / {formattedMax}</span>
          </div>
          <div className="w-full h-1.5 rounded-full bg-accent overflow-hidden border border-border/40">
            <div
              className="h-full rounded-full bg-gradient-to-r from-emerald-500 to-teal-400 shadow-[0_0_8px_rgba(16,185,129,0.4)] transition-all duration-500"
              style={{ width: `${Math.max(pct, recordCount > 0 ? 5 : 0)}%` }}
            />
          </div>
        </div>

        <div className="flex items-center gap-3 shrink-0">
          <span className="text-xs font-mono font-semibold px-2 py-0.5 rounded-md bg-accent text-foreground/80 border border-border/60">
            {dimension}D
          </span>
          <span className="flex items-center gap-1.5 text-xs font-medium text-emerald-600 dark:text-emerald-400">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0 animate-pulse" />
            Healthy
          </span>
        </div>
        <ArrowRight size={15} className="text-muted-foreground group-hover:text-foreground transition-colors shrink-0" />
      </Link>
    );
  }

  return (
    <div className="rounded-xl border border-border bg-card hover:border-input transition-all duration-200 group shadow-xs flex flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-start justify-between p-4 pb-3">
        <div className="flex items-start gap-3">
          <div className={cn("w-9 h-9 rounded-lg flex items-center justify-center shrink-0", bg)}>
            <Icon size={17} className={color} />
          </div>
          <div className="min-w-0">
            <p className="text-sm font-semibold text-foreground truncate group-hover:text-[var(--v-accent)] transition-colors">
              {collection}
            </p>
            <p className="text-[11px] text-muted-foreground font-mono truncate">{project}--{collection}</p>
          </div>
        </div>
        <CardMenu onDelete={onDelete} href={href} />
      </div>

      {/* Tech Tags Row (Dimension & Index) */}
      <div className="px-4 pb-3 flex items-center gap-2 flex-wrap">
        {indexLabel ? (
          <span className="text-[10px] font-mono font-bold tracking-wider uppercase px-2 py-0.5 rounded-md bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20">
            {indexLabel} INDEX
          </span>
        ) : (
          <span className="text-[10px] font-mono px-2 py-0.5 rounded-md bg-muted text-muted-foreground border border-border">
            No Index
          </span>
        )}
        <span className="text-[10px] font-mono font-semibold px-2 py-0.5 rounded-md bg-accent text-foreground/80 border border-border/60">
          {dimension} DIM
        </span>
      </div>

      {/* Vectors Progress Bar Section */}
      <div className="px-4 py-3 bg-accent/30 border-y border-border/50 flex flex-col gap-2">
        <div className="flex items-center justify-between text-xs">
          <span className="text-[11px] font-medium text-muted-foreground">Vectors</span>
          <span className="font-mono text-xs text-foreground font-semibold">
            {formattedCount} <span className="text-muted-foreground font-normal">/ {formattedMax} max</span>
          </span>
        </div>
        
        {/* Small Horizontal Green Progress Bar */}
        <div className="w-full h-2 rounded-full bg-accent border border-border/60 overflow-hidden relative">
          <div
            className="h-full rounded-full bg-gradient-to-r from-emerald-500 to-teal-400 shadow-[0_0_10px_rgba(16,185,129,0.5)] transition-all duration-500"
            style={{ width: `${Math.max(pct, recordCount > 0 ? 3 : 0)}%` }}
          />
        </div>
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between px-4 py-2.5 mt-auto">
        <span className={cn(
          "inline-flex items-center gap-1.5 text-xs font-medium",
          online ? "text-emerald-600 dark:text-emerald-400" : "text-amber-600 dark:text-amber-400"
        )}>
          <span className={cn("w-1.5 h-1.5 rounded-full", online ? "bg-emerald-500 animate-pulse" : "bg-amber-500")} />
          {online ? "Healthy" : "Unreachable"}
        </span>

        <Link
          href={href}
          className="flex items-center gap-1 text-xs font-medium text-[var(--v-accent)] hover:opacity-80 transition-opacity border border-[var(--v-accent)]/30 rounded-lg px-2.5 py-1"
        >
          Open <ArrowRight size={12} />
        </Link>
      </div>
    </div>
  );
}
