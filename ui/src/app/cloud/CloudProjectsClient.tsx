"use client"

import { useState, useRef, useEffect, useTransition } from "react"
import Link from "next/link"
import { MoreVertical, Pencil, Copy, Archive, Layers, ArrowRight } from "lucide-react"
import { cn } from "@/lib/utils"
import { duplicateProject } from "./actions"

export type CloudProject = {
  id: string
  name: string
  region: string
  dim: number
  index_type: string
  status: string
  node_url: string | null
  replication: number
}

const STATUS_STYLE: Record<string, string> = {
  active:    "bg-primary/10 text-primary border-primary/30",
  creating:  "bg-amber-500/10 text-amber-500 border-amber-500/30",
  error:     "bg-destructive/10 text-destructive border-destructive/30",
  stopped:   "bg-muted text-muted-foreground border-border",
  suspended: "bg-amber-500/10 text-amber-500 border-amber-500/30",
}

// ── Three-dot menu ────────────────────────────────────────────────────────────

function ProjectMenu({ onRename, onDuplicate, onArchive, duplicating }: {
  onRename: () => void
  onDuplicate: () => void
  onArchive: () => void
  duplicating: boolean
}) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handler(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener("mousedown", handler)
    return () => document.removeEventListener("mousedown", handler)
  }, [])

  return (
    <div ref={ref} className="relative" onClick={(e) => e.preventDefault()}>
      <button
        onClick={(e) => { e.preventDefault(); e.stopPropagation(); setOpen(v => !v) }}
        className="flex items-center justify-center w-7 h-7 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
        aria-label="Project actions"
      >
        <MoreVertical size={14} />
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 z-50 w-40 rounded-xl border border-border bg-card shadow-lg py-1 overflow-hidden">
          <button
            onClick={(e) => { e.stopPropagation(); setOpen(false); onRename() }}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors text-left"
          >
            <Pencil size={13} /> Rename
          </button>
          <button
            onClick={(e) => { e.stopPropagation(); setOpen(false); onDuplicate() }}
            disabled={duplicating}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors text-left disabled:opacity-50"
          >
            <Copy size={13} /> {duplicating ? "Duplicating…" : "Duplicate"}
          </button>
          <div className="mx-2 my-0.5 border-t border-border/60" />
          <button
            onClick={(e) => { e.stopPropagation(); setOpen(false); onArchive() }}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-amber-600 dark:text-amber-400 hover:bg-amber-500/10 transition-colors text-left"
          >
            <Archive size={13} /> Archive
          </button>
        </div>
      )}
    </div>
  )
}

// ── Rename dialog ─────────────────────────────────────────────────────────────

function RenameDialog({ project, onClose, onSuccess }: {
  project: CloudProject
  onClose: () => void
  onSuccess: (newName: string) => void
}) {
  const [name, setName] = useState(project.name)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    const trimmed = name.trim()
    if (!trimmed) return
    if (trimmed === project.name) { onClose(); return }
    setLoading(true)
    const res = await fetch(`/api/cloud/projects/${project.id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: trimmed }),
    })
    if (!res.ok) {
      const d = await res.json().catch(() => ({})) as { error?: string }
      setError(d.error ?? "Rename failed")
      setLoading(false)
      return
    }
    onSuccess(trimmed)
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <form
        onSubmit={submit}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-sm rounded-2xl border border-border bg-card p-6 space-y-4"
      >
        <h2 className="text-base font-semibold text-foreground">Rename project</h2>
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="w-full px-3 py-2 rounded-lg border border-border bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)]"
        />
        {error && <p className="text-xs text-destructive">{error}</p>}
        <div className="flex gap-2 pt-1">
          <button type="button" onClick={onClose} disabled={loading} className="flex-1 px-4 py-2 border border-border text-foreground rounded-lg hover:bg-accent transition text-sm">
            Cancel
          </button>
          <button type="submit" disabled={loading || !name.trim()} className="flex-1 px-4 py-2 bg-primary text-primary-foreground font-semibold rounded-lg hover:opacity-90 transition disabled:opacity-50 text-sm">
            {loading ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </div>
  )
}

// ── Archive dialog ────────────────────────────────────────────────────────────

function ArchiveDialog({ project, onClose, onSuccess }: {
  project: CloudProject
  onClose: () => void
  onSuccess: () => void
}) {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleArchive() {
    setLoading(true)
    const res = await fetch(`/api/cloud/projects/${project.id}/archive`, { method: "POST" })
    if (!res.ok) {
      const d = await res.json().catch(() => ({})) as { error?: string }
      setError(d.error ?? "Archive failed")
      setLoading(false)
      return
    }
    onSuccess()
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-sm rounded-2xl border border-border bg-card p-6 space-y-4"
      >
        <h2 className="text-base font-semibold text-foreground">Archive project?</h2>
        <p className="text-sm text-muted-foreground">
          <span className="font-medium text-foreground">{project.name}</span> will be stopped and hidden from your dashboard.
          You can restore it from the{" "}
          <Link href="/cloud/archived" className="text-[var(--v-accent)] hover:underline">Archived</Link>{" "}
          page.
        </p>
        {error && <p className="text-xs text-destructive">{error}</p>}
        <div className="flex gap-2 pt-1">
          <button onClick={onClose} disabled={loading} className="flex-1 px-4 py-2 border border-border text-foreground rounded-lg hover:bg-accent transition text-sm">
            Cancel
          </button>
          <button onClick={handleArchive} disabled={loading} className="flex-1 px-4 py-2 bg-amber-500 text-white font-semibold rounded-lg hover:opacity-90 transition disabled:opacity-50 text-sm">
            {loading ? "Archiving…" : "Archive"}
          </button>
        </div>
      </div>
    </div>
  )
}

// ── Project card ──────────────────────────────────────────────────────────────

function ProjectCard({ project, orgId, onUpdate, onRemove }: {
  project: CloudProject
  orgId: string
  onUpdate: (id: string, updates: Partial<CloudProject>) => void
  onRemove: (id: string) => void
}) {
  const [renaming, setRenaming] = useState(false)
  const [archiving, setArchiving] = useState(false)
  const [isPending, startTransition] = useTransition()
  const href = `/cloud/projects/${project.id}`

  function handleDuplicate() {
    startTransition(async () => {
      await duplicateProject(orgId, project.id)
    })
  }

  return (
    <>
      <div className="rounded-xl border border-border bg-card hover:border-input transition-colors group">
        {/* Header */}
        <div className="flex items-start justify-between p-4 pb-3">
          <Link href={href} className="flex items-start gap-3 min-w-0 flex-1">
            <div className="w-9 h-9 rounded-lg bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 flex items-center justify-center shrink-0">
              <Layers size={16} className="text-[var(--v-accent)]" />
            </div>
            <div className="min-w-0">
              <p className="text-sm font-semibold text-foreground truncate group-hover:text-[var(--v-accent)] transition-colors">
                {project.name}
              </p>
              <p className="text-[11px] text-muted-foreground font-mono truncate">
                {project.region} · {project.dim}d · {project.index_type}
              </p>
            </div>
          </Link>
          <ProjectMenu
            onRename={() => setRenaming(true)}
            onDuplicate={handleDuplicate}
            onArchive={() => setArchiving(true)}
            duplicating={isPending}
          />
        </div>

        {/* Status */}
        <div className="px-4 pb-3">
          <span className={cn(
            "inline-block px-2 py-0.5 rounded-full text-xs border",
            STATUS_STYLE[project.status] ?? STATUS_STYLE.stopped
          )}>
            {project.status}
          </span>
        </div>

        {/* Divider */}
        <div className="mx-4 border-t border-border/60" />

        {/* Node URL */}
        <div className="px-4 py-3">
          <p className="text-[10px] uppercase tracking-widest text-muted-foreground">Node URL</p>
          <p className="text-xs font-mono text-muted-foreground truncate mt-0.5">
            {project.node_url ?? "—"}
          </p>
        </div>

        {/* Divider */}
        <div className="mx-4 border-t border-border/60" />

        {/* Footer */}
        <div className="flex items-center justify-end px-4 py-3">
          <Link
            href={href}
            className="flex items-center gap-1 text-xs font-medium text-[var(--v-accent)] hover:opacity-80 transition-opacity border border-[var(--v-accent)]/30 rounded-lg px-2.5 py-1"
          >
            Open <ArrowRight size={12} />
          </Link>
        </div>
      </div>

      {renaming && (
        <RenameDialog
          project={project}
          onClose={() => setRenaming(false)}
          onSuccess={(newName) => { onUpdate(project.id, { name: newName }); setRenaming(false) }}
        />
      )}
      {archiving && (
        <ArchiveDialog
          project={project}
          onClose={() => setArchiving(false)}
          onSuccess={() => { onRemove(project.id); setArchiving(false) }}
        />
      )}
    </>
  )
}

// ── Public export ─────────────────────────────────────────────────────────────

export function CloudProjectsClient({ projects: initial, orgId }: {
  projects: CloudProject[]
  orgId: string
}) {
  const [projects, setProjects] = useState(initial)

  const update = (id: string, updates: Partial<CloudProject>) =>
    setProjects(prev => prev.map(p => p.id === id ? { ...p, ...updates } : p))

  const remove = (id: string) =>
    setProjects(prev => prev.filter(p => p.id !== id))

  if (projects.length === 0) {
    return (
      <div className="rounded-2xl border border-dashed border-border py-16 text-center">
        <div className="flex justify-center mb-3">
          <div className="w-10 h-10 rounded-xl bg-[var(--v-accent-muted)] flex items-center justify-center">
            <Layers size={18} className="text-[var(--v-accent)]" />
          </div>
        </div>
        <p className="text-sm font-medium text-foreground">No projects yet</p>
        <p className="mt-1 text-xs text-muted-foreground">Create your first project to get started.</p>
      </div>
    )
  }

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      {projects.map(p => (
        <ProjectCard key={p.id} project={p} orgId={orgId} onUpdate={update} onRemove={remove} />
      ))}
    </div>
  )
}
