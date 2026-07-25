'use client'

import { useState } from 'react'
import Link from 'next/link'
import { Layers, Plus, Trash2, Upload } from 'lucide-react'
import { useCollections } from '@/lib/hooks/useCollections'
import { Button } from '@/components/ui/button'

// Simplified port of valori-kernel/ui's CollectionList — kernel's version
// groups namespaces by a "project--collection" prefix because it has one
// node shared across projects; here each project IS its own node, so
// collections are just that node's namespaces directly (see useCollections).
export function CollectionsPanel({ projectId }: { projectId: string }) {
    const { collections, isLoading, create, drop } = useCollections(projectId)
    const [creating, setCreating] = useState(false)
    const [name, setName] = useState('')
    const [error, setError] = useState<string | null>(null)
    const [pending, setPending] = useState(false)
    const [dropping, setDropping] = useState<string | null>(null)

    const handleCreate = async (e: React.FormEvent) => {
        e.preventDefault()
        if (!name.trim()) return
        setPending(true)
        setError(null)
        try {
            await create(name.trim())
            setName('')
            setCreating(false)
        } catch (err) {
            setError(err instanceof Error ? err.message : 'Failed to create collection')
        } finally {
            setPending(false)
        }
    }

    const handleDrop = async (collection: string) => {
        if (!confirm(`Delete collection "${collection}"? This removes all vectors in it.`)) return
        setDropping(collection)
        try {
            await drop(collection)
        } catch (err) {
            setError(err instanceof Error ? err.message : 'Failed to delete collection')
        } finally {
            setDropping(null)
        }
    }

    return (
        <div className="rounded-xl border border-border bg-card p-5 space-y-4">
            <div className="flex items-center justify-between gap-3">
                <div>
                    <p className="text-sm font-medium text-foreground">
                        Collections
                        {!isLoading && (
                            <span className="ml-2 text-xs font-medium bg-muted text-muted-foreground rounded-full px-2 py-0.5 border border-border align-middle">
                                {collections.length}
                            </span>
                        )}
                    </p>
                    <p className="text-xs text-muted-foreground mt-0.5">
                        Namespaces on this project&apos;s node — click one to search, upload documents, and manage it.
                    </p>
                </div>
                <Button size="sm" onClick={() => setCreating((v) => !v)} className="gap-1.5 h-8 text-xs">
                    <Plus size={13} /> New collection
                </Button>
            </div>

            {creating && (
                <form onSubmit={handleCreate} className="flex items-center gap-2">
                    <input
                        autoFocus
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        placeholder="collection name"
                        className="flex-1 px-3 py-1.5 rounded-lg border border-input bg-background text-foreground text-sm"
                    />
                    <Button type="submit" size="sm" disabled={pending || !name.trim()}>
                        {pending ? 'Creating…' : 'Create'}
                    </Button>
                    <Button type="button" variant="ghost" size="sm" onClick={() => { setCreating(false); setError(null) }}>
                        Cancel
                    </Button>
                </form>
            )}

            {error && <p className="text-xs text-destructive">{error}</p>}

            {isLoading ? (
                <div className="flex flex-col gap-2">
                    {[1, 2].map((i) => (
                        <div key={i} className="animate-pulse h-11 rounded-lg bg-accent" />
                    ))}
                </div>
            ) : collections.length === 0 ? (
                <p className="text-xs text-muted-foreground py-2">
                    No collections yet — inserts land in{' '}
                    <Link href={`/cloud/projects/${projectId}/tools?collection=default`} className="font-mono text-primary hover:underline">
                        default
                    </Link>{' '}
                    until you create one.
                </p>
            ) : (
                <div className="flex flex-col gap-1.5">
                    {collections.map((c) => (
                        <div
                            key={c}
                            className="flex items-center gap-2.5 rounded-lg border border-border bg-background/60 pl-3 pr-2 py-2 hover:border-ring transition-colors"
                        >
                            <Layers size={13} className="text-muted-foreground shrink-0" />
                            <Link
                                href={`/cloud/projects/${projectId}/tools?collection=${encodeURIComponent(c)}`}
                                className="text-sm font-mono text-foreground flex-1 truncate hover:text-primary hover:underline"
                            >
                                {c}
                            </Link>
                            <Link
                                href={`/cloud/projects/${projectId}/tools?collection=${encodeURIComponent(c)}`}
                                aria-label={`Upload documents to ${c}`}
                                title={`Upload documents to ${c}`}
                                className="text-muted-foreground hover:text-primary transition-colors p-1"
                            >
                                <Upload size={13} />
                            </Link>
                            <button
                                onClick={() => handleDrop(c)}
                                disabled={dropping === c}
                                aria-label={`Delete ${c}`}
                                title={`Delete ${c}`}
                                className="text-muted-foreground hover:text-destructive transition-colors disabled:opacity-50 p-1"
                            >
                                <Trash2 size={13} />
                            </button>
                        </div>
                    ))}
                </div>
            )}
        </div>
    )
}
