"use client";

import { useState, useRef, useEffect } from "react";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { CreateCollectionDialog } from "./CreateCollectionDialog";
import { DeleteCollectionDialog } from "./DeleteCollectionDialog";
import { useHealth } from "@/lib/hooks/useHealth";
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
  isLoading: boolean;
  onCreate: (name: string) => Promise<void>;
  onDrop: (name: string) => Promise<void>;
}

export function CollectionList({ project, collections, isLoading, onCreate, onDrop }: Props) {
  const [createOpen, setCreateOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const { dim, online } = useHealth();

  return (
    <div className="flex flex-col gap-5">
      {/* Section header */}
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2.5">
            <h2 className="text-base font-semibold text-foreground">Collections</h2>
            {!isLoading && (
              <span className="text-xs font-medium bg-muted text-muted-foreground rounded-full px-2 py-0.5 border border-border">
                {collections.length}
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
      ) : collections.length === 0 ? (
        <EmptyState onCreateClick={() => setCreateOpen(true)} />
      ) : (
        <>
          <div className={viewMode === "grid"
            ? "grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
            : "flex flex-col gap-2"
          }>
            {collections.map((col) => (
              <CollectionCard
                key={col}
                project={project}
                collection={col}
                dim={dim}
                online={online}
                viewMode={viewMode}
                onDelete={() => setDeleteTarget(col)}
              />
            ))}
          </div>
          {/* Subtle "add more" empty state at bottom */}
          <EmptyState onCreateClick={() => setCreateOpen(true)} dimmed />
        </>
      )}

      <CreateCollectionDialog project={project} open={createOpen} onOpenChange={setCreateOpen} onCreate={onCreate} />
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
  dim,
  online,
  viewMode,
  onDelete,
}: {
  project: string;
  collection: string;
  dim: number | null;
  online: boolean;
  viewMode: "grid" | "list";
  onDelete: () => void;
}) {
  const href = `/projects/${encodeURIComponent(project)}/${encodeURIComponent(collection)}`;
  const { Icon, bg, color } = getVariant(collection);

  if (viewMode === "list") {
    return (
      <Link
        href={href}
        className="flex items-center gap-4 rounded-xl border border-border bg-card px-4 py-3 hover:border-input hover:bg-accent/30 transition-colors group"
      >
        <div className={cn("w-8 h-8 rounded-lg flex items-center justify-center shrink-0", bg)}>
          <Icon size={15} className={color} />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-foreground truncate">{collection}</p>
          <p className="text-xs text-muted-foreground font-mono">{project}--{collection}</p>
        </div>
        <div className="flex items-center gap-2">
          <span className="flex items-center gap-1.5 text-xs font-medium text-emerald-600 dark:text-emerald-400">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0" />
            Healthy
          </span>
          <span className="text-xs text-muted-foreground font-mono">{dim ?? "—"}</span>
        </div>
        <ArrowRight size={14} className="text-muted-foreground group-hover:text-foreground transition-colors shrink-0" />
      </Link>
    );
  }

  return (
    <div className="rounded-xl border border-border bg-card hover:border-input transition-colors group">
      {/* Card header */}
      <div className="flex items-start justify-between p-4 pb-3">
        <div className="flex items-start gap-3">
          <div className={cn("w-9 h-9 rounded-lg flex items-center justify-center shrink-0", bg)}>
            <Icon size={16} className={color} />
          </div>
          <div className="min-w-0">
            <p className="text-sm font-semibold text-foreground truncate">{collection}</p>
            <p className="text-[11px] text-muted-foreground font-mono truncate">{project}--{collection}</p>
          </div>
        </div>
        <CardMenu onDelete={onDelete} href={href} />
      </div>

      {/* Status */}
      <div className="px-4 pb-3">
        <span className={cn(
          "inline-flex items-center gap-1.5 text-xs font-medium",
          online ? "text-emerald-600 dark:text-emerald-400" : "text-amber-600 dark:text-amber-400"
        )}>
          <span className={cn("w-1.5 h-1.5 rounded-full", online ? "bg-emerald-500" : "bg-amber-500")} />
          {online ? "Healthy" : "Unreachable"}
        </span>
      </div>

      {/* Divider */}
      <div className="mx-4 border-t border-border/60" />

      {/* Stats */}
      <div className="grid grid-cols-4 divide-x divide-border/60 px-0 py-3">
        {[
          { label: "Vectors",   value: "—" },
          { label: "Records",   value: "—" },
          { label: "Dimension", value: dim != null ? String(dim) : "—" },
          { label: "Shards",    value: "1" },
        ].map(({ label, value }) => (
          <div key={label} className="px-3 text-center">
            <p className="text-[10px] text-muted-foreground uppercase tracking-wide">{label}</p>
            <p className="mt-0.5 text-sm font-semibold text-foreground font-mono">{value}</p>
          </div>
        ))}
      </div>

      {/* Divider */}
      <div className="mx-4 border-t border-border/60" />

      {/* Footer */}
      <div className="flex items-center justify-between px-4 py-3">
        <span className="text-[11px] text-muted-foreground">Updated —</span>
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
