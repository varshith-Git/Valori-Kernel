'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { StatusBadge } from '@/components/ui/status-badge'
import { CopyBtn } from '@/components/ui/copy-btn'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { createApiKey, rotateApiKey, revokeApiKey } from './actions'
import type { ServiceAccountRow } from './page'

interface ApiKeyRow {
    id: string
    name: string
    key_prefix: string
    scopes: string[]
    created_at: string
    last_used_at: string | null
    revoked_at: string | null
    request_count: number
    service_account_id: string | null
    project_id: string | null
    expires_at: string | null
}

interface ProjectOption {
    id: string
    name: string
}

function fmtDate(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

export function ApiKeysManager({
    orgId,
    canManage,
    initialKeys,
    serviceAccounts,
    projects,
}: {
    orgId: string
    canManage: boolean
    initialKeys: ApiKeyRow[]
    serviceAccounts: ServiceAccountRow[]
    projects: ProjectOption[]
}) {
    const router = useRouter()
    const [createOpen, setCreateOpen] = useState(false)
    const [name, setName] = useState('')
    const [projectId, setProjectId] = useState('')
    const [expiresInDays, setExpiresInDays] = useState<string>('never')
    const [serviceAccountId, setServiceAccountId] = useState('')
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()
    const [revealedKey, setRevealedKey] = useState<{ plaintext_key: string; key_prefix: string; name: string } | null>(null)

    const handleCreate = () => {
        setError(null)
        if (!projectId) {
            setError('Every key is bound to exactly one project — pick which one.')
            return
        }
        const days = expiresInDays === 'never' ? null : Number(expiresInDays)
        startTransition(async () => {
            const result = await createApiKey(orgId, projectId, name, serviceAccountId || null, days)
            if (result.error || !result.key) {
                setError(result.error ?? 'Could not create key.')
                return
            }
            setCreateOpen(false)
            setName('')
            setProjectId('')
            setExpiresInDays('never')
            setServiceAccountId('')
            setRevealedKey(result.key)
            router.refresh()
        })
    }

    const handleRotate = (keyId: string) => {
        setError(null)
        startTransition(async () => {
            const result = await rotateApiKey(keyId)
            if (result.error || !result.key) {
                setError(result.error ?? 'Could not rotate key.')
                return
            }
            setRevealedKey(result.key)
            router.refresh()
        })
    }

    const handleRevoke = (keyId: string) => {
        startTransition(async () => {
            const result = await revokeApiKey(keyId)
            if (result.error) {
                setError(result.error)
                return
            }
            router.refresh()
        })
    }

    return (
        <div className="space-y-4">
            {canManage && (
                <div className="flex justify-end">
                    <Button size="sm" onClick={() => setCreateOpen(true)}>
                        + New API Key
                    </Button>
                </div>
            )}

            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}

            <div className="rounded-xl border border-border bg-card overflow-hidden">
                {initialKeys.length === 0 ? (
                    <div className="p-12 text-center">
                        <p className="text-muted-foreground text-sm">No API keys yet.</p>
                    </div>
                ) : (
                    <table className="w-full text-sm">
                        <thead>
                            <tr className="border-b border-border text-left text-xs text-muted-foreground uppercase tracking-widest">
                                <th className="px-6 py-3 font-medium">Name</th>
                                <th className="px-6 py-3 font-medium">Project</th>
                                <th className="px-6 py-3 font-medium">Service Account</th>
                                <th className="px-6 py-3 font-medium">Key</th>
                                <th className="px-6 py-3 font-medium">Scopes</th>
                                <th className="px-6 py-3 font-medium">Created</th>
                                <th className="px-6 py-3 font-medium">Expires</th>
                                <th className="px-6 py-3 font-medium">Last used</th>
                                <th className="px-6 py-3 font-medium">Requests</th>
                                <th className="px-6 py-3 font-medium">Status</th>
                                {canManage && <th className="px-6 py-3 font-medium" />}
                            </tr>
                        </thead>
                        <tbody>
                            {initialKeys.map((k) => (
                                <tr key={k.id} className="border-b border-border last:border-0">
                                    <td className="px-6 py-4 text-foreground font-medium">{k.name}</td>
                                    <td className="px-6 py-4 text-muted-foreground">
                                        {/* project_id = null means this key predates project scoping (P2) —
                                            it keeps its pre-migration org-wide behavior, not narrowed. */}
                                        {k.project_id
                                            ? (projects.find((p) => p.id === k.project_id)?.name ?? k.project_id)
                                            : <span title="Created before project-scoped keys existed — org-wide, unchanged">legacy (org-wide)</span>}
                                    </td>
                                    <td className="px-6 py-4 text-muted-foreground">
                                        {serviceAccounts.find((a) => a.id === k.service_account_id)?.name ?? '—'}
                                    </td>
                                    <td className="px-6 py-4 font-mono text-xs text-muted-foreground">{k.key_prefix}…</td>
                                    <td className="px-6 py-4">
                                        <div className="flex gap-1">
                                            {k.scopes.map((s) => (
                                                <StatusBadge key={s} tone="neutral">{s}</StatusBadge>
                                            ))}
                                        </div>
                                    </td>
                                    <td className="px-6 py-4 text-muted-foreground">{fmtDate(k.created_at)}</td>
                                    <td className="px-6 py-4 text-muted-foreground">
                                        {k.expires_at ? fmtDate(k.expires_at) : 'Never'}
                                    </td>
                                    <td className="px-6 py-4 text-muted-foreground">
                                        {k.last_used_at ? fmtDate(k.last_used_at) : 'Never'}
                                    </td>
                                    <td className="px-6 py-4 font-mono text-muted-foreground">
                                        {(k.request_count ?? 0).toLocaleString()}
                                    </td>
                                    <td className="px-6 py-4">
                                        {k.revoked_at ? (
                                            <StatusBadge tone="error">revoked</StatusBadge>
                                        ) : (
                                            <StatusBadge tone="success">active</StatusBadge>
                                        )}
                                    </td>
                                    {canManage && (
                                        <td className="px-6 py-4 text-right space-x-3">
                                            {!k.revoked_at && (
                                                <>
                                                    <button
                                                        onClick={() => handleRotate(k.id)}
                                                        disabled={isPending}
                                                        className="text-xs text-muted-foreground hover:text-foreground hover:underline disabled:opacity-50"
                                                    >
                                                        Rotate
                                                    </button>
                                                    <button
                                                        onClick={() => handleRevoke(k.id)}
                                                        disabled={isPending}
                                                        className="text-xs text-destructive hover:underline disabled:opacity-50"
                                                    >
                                                        Revoke
                                                    </button>
                                                </>
                                            )}
                                        </td>
                                    )}
                                </tr>
                            ))}
                        </tbody>
                    </table>
                )}
            </div>

            {/* Create dialog */}
            <Dialog open={createOpen} onOpenChange={setCreateOpen}>
                <DialogContent className="bg-card border-input max-w-sm">
                    <DialogHeader>
                        <DialogTitle className="text-foreground text-base">New API Key</DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        <div className="space-y-1">
                            <label className="text-xs text-muted-foreground uppercase tracking-widest">Name</label>
                            <input
                                autoFocus
                                value={name}
                                onChange={(e) => setName(e.target.value)}
                                placeholder="CI pipeline"
                                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                            />
                        </div>
                        <div className="space-y-1">
                            <label className="text-xs text-muted-foreground uppercase tracking-widest">Project</label>
                            <select
                                value={projectId}
                                onChange={(e) => setProjectId(e.target.value)}
                                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground"
                            >
                                <option value="">Select a project…</option>
                                {projects.map((p) => (
                                    <option key={p.id} value={p.id}>{p.name}</option>
                                ))}
                            </select>
                            <p className="text-xs text-muted-foreground">
                                This key will only ever work against this one project.
                            </p>
                        </div>
                        <div className="space-y-1">
                            <label className="text-xs text-muted-foreground uppercase tracking-widest">Expiration</label>
                            <select
                                value={expiresInDays}
                                onChange={(e) => setExpiresInDays(e.target.value)}
                                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground"
                            >
                                <option value="never">Never</option>
                                <option value="30">30 days</option>
                                <option value="60">60 days</option>
                                <option value="90">90 days</option>
                            </select>
                        </div>
                        {serviceAccounts.length > 0 && (
                            <div className="space-y-1">
                                <label className="text-xs text-muted-foreground uppercase tracking-widest">
                                    Service account (optional)
                                </label>
                                <select
                                    value={serviceAccountId}
                                    onChange={(e) => setServiceAccountId(e.target.value)}
                                    className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground"
                                >
                                    <option value="">None</option>
                                    {serviceAccounts.map((a) => (
                                        <option key={a.id} value={a.id}>{a.name}</option>
                                    ))}
                                </select>
                            </div>
                        )}
                        <div className="flex gap-2 justify-end pt-2">
                            <Button variant="ghost" size="sm" onClick={() => setCreateOpen(false)}>
                                Cancel
                            </Button>
                            <Button size="sm" onClick={handleCreate} disabled={isPending || !name.trim() || !projectId}>
                                {isPending ? 'Creating…' : 'Create'}
                            </Button>
                        </div>
                    </div>
                </DialogContent>
            </Dialog>

            {/* Reveal-once dialog */}
            <Dialog open={revealedKey !== null} onOpenChange={(o) => { if (!o) setRevealedKey(null) }}>
                <DialogContent className="bg-card border-input max-w-md">
                    <DialogHeader>
                        <DialogTitle className="text-foreground text-base">
                            {revealedKey?.name}
                        </DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        <p className="text-xs text-destructive">
                            Copy this now — you won&apos;t be able to see it again.
                        </p>
                        <div className="flex items-center gap-2 rounded-md border border-input bg-background px-3 py-2">
                            <code className="flex-1 font-mono text-xs text-foreground break-all">
                                {revealedKey?.plaintext_key}
                            </code>
                            {revealedKey && <CopyBtn text={revealedKey.plaintext_key} label="copy" />}
                        </div>
                        <Button size="sm" onClick={() => setRevealedKey(null)}>
                            Done
                        </Button>
                    </div>
                </DialogContent>
            </Dialog>
        </div>
    )
}
