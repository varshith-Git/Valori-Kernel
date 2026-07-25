'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DeleteProjectDialog } from '@/components/projects/DeleteProjectDialog'
import { duplicateProject } from '../../actions'

export function ProjectActions({
    projectId,
    orgId,
    projectName,
    projectStatus,
}: {
    projectId: string
    orgId: string
    projectName: string
    projectStatus: string
}) {
    const [open, setOpen] = useState(false)
    const [renameOpen, setRenameOpen] = useState(false)
    const [newName, setNewName] = useState(projectName)
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()
    const router = useRouter()

    const handleRename = () => {
        setError(null)
        startTransition(async () => {
            const res = await fetch(`/api/cloud/projects/${projectId}`, {
                method: 'PATCH',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: newName.trim() }),
            })
            if (!res.ok) {
                const data = await res.json().catch(() => ({}))
                setError(data.error ?? 'Could not rename project.')
                return
            }
            setRenameOpen(false)
            router.refresh()
        })
    }

    const handleDelete = async () => {
        const res = await fetch(`/api/cloud/projects/${projectId}`, { method: 'DELETE' })
        if (!res.ok) {
            throw new Error(await res.text())
        }
        router.push('/cloud')
        router.refresh()
    }

    const toggleRunning = () => {
        setError(null)
        const action = projectStatus === 'active' ? 'stop' : 'start'
        startTransition(async () => {
            const res = await fetch(`/api/cloud/projects/${projectId}/${action}`, { method: 'POST' })
            if (!res.ok) {
                setError(await res.text())
                return
            }
            router.refresh()
        })
    }

    const handleRestart = () => {
        setError(null)
        startTransition(async () => {
            const res = await fetch(`/api/cloud/projects/${projectId}/restart`, { method: 'POST' })
            if (!res.ok) {
                setError(await res.text())
                return
            }
            router.refresh()
        })
    }

    // Only active/stopped/suspended projects can be toggled — creating/error/
    // deleted don't have a meaningful stop/start action. Starting a
    // suspended project just calls the same /start route as a stopped one
    // (start_project always sets status='active' unconditionally), so no
    // separate "reactivate" action is needed.
    const canToggle = projectStatus === 'active' || projectStatus === 'stopped' || projectStatus === 'suspended'
    const canArchive = projectStatus === 'active' || projectStatus === 'stopped'

    const handleArchive = () => {
        setError(null)
        startTransition(async () => {
            const res = await fetch(`/api/cloud/projects/${projectId}/archive`, { method: 'POST' })
            if (!res.ok) {
                const data = await res.json().catch(() => ({}))
                setError(data.error ?? 'Could not archive project.')
                return
            }
            router.push('/cloud')
        })
    }

    const handleDuplicate = () => {
        setError(null)
        startTransition(async () => {
            const result = await duplicateProject(orgId, projectId)
            if (result.error) {
                setError(result.error)
                return
            }
            router.push('/cloud')
        })
    }

    return (
        <div className="flex flex-col items-end gap-1">
            <div className="flex items-center gap-2">
                {canToggle && (
                    <Button variant="outline" size="sm" onClick={toggleRunning} disabled={isPending}>
                        {isPending
                            ? projectStatus === 'active'
                                ? 'Stopping…'
                                : 'Starting…'
                            : projectStatus === 'active'
                              ? 'Stop'
                              : 'Start'}
                    </Button>
                )}
                {projectStatus === 'active' && (
                    <Button variant="outline" size="sm" onClick={handleRestart} disabled={isPending} title="Stop then start">
                        Restart
                    </Button>
                )}
                <Button variant="outline" size="sm" onClick={() => { setNewName(projectName); setRenameOpen(true) }}>
                    Rename
                </Button>
                <Button variant="outline" size="sm" onClick={handleDuplicate} disabled={isPending} title="Duplicates settings only, not data">
                    Duplicate
                </Button>
                {canArchive && (
                    <Button variant="outline" size="sm" onClick={handleArchive} disabled={isPending}>
                        Archive
                    </Button>
                )}
                <Button variant="destructive" size="sm" onClick={() => setOpen(true)}>
                    Delete Project
                </Button>
            </div>
            {error && <p className="text-xs text-destructive">{error}</p>}
            <DeleteProjectDialog
                name={projectName}
                open={open}
                onClose={() => setOpen(false)}
                onDelete={handleDelete}
            />
            <Dialog open={renameOpen} onOpenChange={setRenameOpen}>
                <DialogContent className="bg-card border-input max-w-sm">
                    <DialogHeader>
                        <DialogTitle className="text-foreground text-base">Rename Project</DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        <input
                            autoFocus
                            value={newName}
                            onChange={(e) => setNewName(e.target.value)}
                            onKeyDown={(e) => e.key === 'Enter' && handleRename()}
                            className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                        />
                        <div className="flex gap-2 justify-end">
                            <Button variant="ghost" size="sm" onClick={() => setRenameOpen(false)}>
                                Cancel
                            </Button>
                            <Button size="sm" onClick={handleRename} disabled={isPending || !newName.trim()}>
                                {isPending ? 'Saving…' : 'Save'}
                            </Button>
                        </div>
                    </div>
                </DialogContent>
            </Dialog>
        </div>
    )
}
