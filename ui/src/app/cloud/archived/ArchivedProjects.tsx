'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'

interface ArchivedProject {
    id: string
    name: string
    region: string
    updated_at: string
}

function fmtDate(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

export function ArchivedProjects({ initialProjects }: { initialProjects: ArchivedProject[] }) {
    const router = useRouter()
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()

    const handleRestore = (id: string) => {
        setError(null)
        startTransition(async () => {
            const res = await fetch(`/api/projects/${id}/restore`, { method: 'POST' })
            if (!res.ok) {
                const data = await res.json().catch(() => ({}))
                setError(data.error ?? 'Could not restore project.')
                return
            }
            router.refresh()
        })
    }

    if (initialProjects.length === 0) {
        return (
            <div className="rounded-2xl border border-border bg-card/50 p-12 text-center">
                <p className="text-muted-foreground text-sm">No archived projects.</p>
            </div>
        )
    }

    return (
        <div className="space-y-4">
            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}
            <div className="rounded-xl border border-border bg-card overflow-hidden">
                <table className="w-full text-sm">
                    <thead>
                        <tr className="border-b border-border text-left text-xs text-muted-foreground uppercase tracking-widest">
                            <th className="px-6 py-3 font-medium">Name</th>
                            <th className="px-6 py-3 font-medium">Region</th>
                            <th className="px-6 py-3 font-medium">Archived</th>
                            <th className="px-6 py-3 font-medium" />
                        </tr>
                    </thead>
                    <tbody>
                        {initialProjects.map((p) => (
                            <tr key={p.id} className="border-b border-border last:border-0">
                                <td className="px-6 py-4 text-foreground font-medium">{p.name}</td>
                                <td className="px-6 py-4 text-muted-foreground">{p.region}</td>
                                <td className="px-6 py-4 text-muted-foreground">{fmtDate(p.updated_at)}</td>
                                <td className="px-6 py-4 text-right">
                                    <Button
                                        variant="outline"
                                        size="sm"
                                        onClick={() => handleRestore(p.id)}
                                        disabled={isPending}
                                    >
                                        {isPending ? 'Restoring…' : 'Restore'}
                                    </Button>
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </div>
    )
}
