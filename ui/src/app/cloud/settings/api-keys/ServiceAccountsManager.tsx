'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { StatusBadge } from '@/components/ui/status-badge'
import { createServiceAccount, disableServiceAccount } from './actions'
import type { ServiceAccountRow } from './page'

function fmtDate(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

export function ServiceAccountsManager({
    orgId,
    canManage,
    initialAccounts,
}: {
    orgId: string
    canManage: boolean
    initialAccounts: ServiceAccountRow[]
}) {
    const router = useRouter()
    const [creating, setCreating] = useState(false)
    const [name, setName] = useState('')
    const [description, setDescription] = useState('')
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()

    const handleCreate = (e: React.FormEvent) => {
        e.preventDefault()
        if (!name.trim()) return
        setError(null)
        startTransition(async () => {
            const result = await createServiceAccount(orgId, name.trim(), description.trim())
            if (result.error) {
                setError(result.error)
                return
            }
            setName('')
            setDescription('')
            setCreating(false)
            router.refresh()
        })
    }

    const handleDisable = (accountId: string) => {
        startTransition(async () => {
            const result = await disableServiceAccount(accountId)
            if (result.error) {
                setError(result.error)
                return
            }
            router.refresh()
        })
    }

    return (
        <section className="rounded-xl border border-border bg-card p-6 space-y-4">
            <div className="flex items-center justify-between">
                <div>
                    <h2 className="text-sm font-semibold text-foreground">Service Accounts</h2>
                    <p className="text-xs text-muted-foreground mt-1">
                        Named machine identities for grouping API keys — a service account&apos;s actual credentials
                        are still ordinary API keys below, just labeled with who/what they&apos;re for.
                    </p>
                </div>
                {canManage && (
                    <Button size="sm" variant="outline" onClick={() => setCreating((v) => !v)}>
                        + New
                    </Button>
                )}
            </div>

            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}

            {creating && (
                <form onSubmit={handleCreate} className="flex items-center gap-2">
                    <input
                        autoFocus
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        placeholder="ci-pipeline"
                        className="w-40 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground"
                    />
                    <input
                        value={description}
                        onChange={(e) => setDescription(e.target.value)}
                        placeholder="What's it for? (optional)"
                        className="flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground"
                    />
                    <Button type="submit" size="sm" disabled={isPending || !name.trim()}>
                        {isPending ? 'Creating…' : 'Create'}
                    </Button>
                    <Button type="button" variant="ghost" size="sm" onClick={() => setCreating(false)}>
                        Cancel
                    </Button>
                </form>
            )}

            <div className="divide-y divide-border">
                {initialAccounts.map((a) => (
                    <div key={a.id} className="flex items-center justify-between py-3 gap-4">
                        <div>
                            <p className="text-sm text-foreground font-medium flex items-center gap-2">
                                {a.name}
                                {a.disabled_at ? (
                                    <StatusBadge tone="neutral">disabled</StatusBadge>
                                ) : (
                                    <StatusBadge tone="success">active</StatusBadge>
                                )}
                            </p>
                            <p className="text-xs text-muted-foreground mt-0.5">
                                {a.description ?? 'No description'} · created {fmtDate(a.created_at)}
                            </p>
                        </div>
                        {canManage && !a.disabled_at && (
                            <button
                                onClick={() => handleDisable(a.id)}
                                disabled={isPending}
                                className="text-xs text-destructive hover:underline disabled:opacity-50 shrink-0"
                            >
                                Disable
                            </button>
                        )}
                    </div>
                ))}
                {initialAccounts.length === 0 && !creating && (
                    <p className="text-sm text-muted-foreground py-3">No service accounts yet.</p>
                )}
            </div>
        </section>
    )
}
