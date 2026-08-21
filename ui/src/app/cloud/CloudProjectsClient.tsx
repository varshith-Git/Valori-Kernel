"use client"

import { useState, useTransition } from "react"
import Link from "next/link"
import { Layers } from "lucide-react"
import { ProjectCard, type CloudProjectCardData } from "@/components/projects/ProjectCard"
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

// ── (ProjectMenu moved to shared ProjectCard component) ───────────────────────

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

// ── Project card (wraps shared ProjectCard) ───────────────────────────────────

function CloudProjectCard({ project, orgId, onUpdate, onRemove }: {
  project: CloudProject
  orgId: string
  onUpdate: (id: string, updates: Partial<CloudProject>) => void
  onRemove: (id: string) => void
}) {
  const [renaming, setRenaming] = useState(false)
  const [archiving, setArchiving] = useState(false)
  const [, startTransition] = useTransition()

  function handleDuplicate() {
    startTransition(async () => {
      await duplicateProject(orgId, project.id)
    })
  }

  const cardData: CloudProjectCardData = {
    kind:        "cloud",
    id:          project.id,
    name:        project.name,
    status:      project.status,
    region:      project.region,
    replication: project.replication,
    nodeUrl:     project.node_url,
    href:        `/cloud/projects/${project.id}`,
  }

  return (
    <>
      <ProjectCard
        data={cardData}
        onRename={() => setRenaming(true)}
        onDuplicate={handleDuplicate}
        onArchive={() => setArchiving(true)}
      />
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
        <CloudProjectCard key={p.id} project={p} orgId={orgId} onUpdate={update} onRemove={remove} />
      ))}
    </div>
  )
}
