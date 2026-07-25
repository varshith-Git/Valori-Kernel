'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { StatusBadge } from '@/components/ui/status-badge'
import { CopyBtn } from '@/components/ui/copy-btn'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { createPersonalAccessToken, rotatePersonalAccessToken, revokePersonalAccessToken } from './actions'

interface TokenRow {
    id: string
    name: string
    token_prefix: string
    scopes: string[]
    created_at: string
    last_used_at: string | null
    revoked_at: string | null
}

function fmtDate(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

export function DeveloperManager({ initialTokens }: { initialTokens: TokenRow[] }) {
    const router = useRouter()
    const [createOpen, setCreateOpen] = useState(false)
    const [name, setName] = useState('')
    const [scopeWrite, setScopeWrite] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()
    const [revealedToken, setRevealedToken] = useState<{ plaintext_token: string; token_prefix: string; name: string } | null>(null)

    const handleCreate = () => {
        setError(null)
        const scopes = scopeWrite ? ['read', 'write'] : ['read']
        startTransition(async () => {
            const result = await createPersonalAccessToken(name, scopes)
            if (result.error || !result.token) {
                setError(result.error ?? 'Could not create token.')
                return
            }
            setCreateOpen(false)
            setName('')
            setScopeWrite(false)
            setRevealedToken(result.token)
            router.refresh()
        })
    }

    const handleRotate = (tokenId: string) => {
        setError(null)
        startTransition(async () => {
            const result = await rotatePersonalAccessToken(tokenId)
            if (result.error || !result.token) {
                setError(result.error ?? 'Could not rotate token.')
                return
            }
            setRevealedToken(result.token)
            router.refresh()
        })
    }

    const handleRevoke = (tokenId: string) => {
        startTransition(async () => {
            const result = await revokePersonalAccessToken(tokenId)
            if (result.error) {
                setError(result.error)
                return
            }
            router.refresh()
        })
    }

    return (
        <div className="space-y-4">
            <div className="flex items-center justify-between">
                <div>
                    <h2 className="text-sm font-semibold text-foreground">Personal Access Tokens</h2>
                    <p className="text-xs text-muted-foreground mt-0.5">
                        Act as you, not tied to an organization. Nothing in Valori Cloud authenticates with one yet —
                        this is a developer-experience primitive ready for a future CLI or management API.
                    </p>
                </div>
                <Button size="sm" onClick={() => setCreateOpen(true)}>
                    + New Token
                </Button>
            </div>

            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}

            <div className="rounded-xl border border-border bg-card overflow-hidden">
                {initialTokens.length === 0 ? (
                    <div className="p-12 text-center">
                        <p className="text-muted-foreground text-sm">No personal access tokens yet.</p>
                    </div>
                ) : (
                    <table className="w-full text-sm">
                        <thead>
                            <tr className="border-b border-border text-left text-xs text-muted-foreground uppercase tracking-widest">
                                <th className="px-6 py-3 font-medium">Name</th>
                                <th className="px-6 py-3 font-medium">Token</th>
                                <th className="px-6 py-3 font-medium">Scopes</th>
                                <th className="px-6 py-3 font-medium">Created</th>
                                <th className="px-6 py-3 font-medium">Last used</th>
                                <th className="px-6 py-3 font-medium">Status</th>
                                <th className="px-6 py-3 font-medium" />
                            </tr>
                        </thead>
                        <tbody>
                            {initialTokens.map((t) => (
                                <tr key={t.id} className="border-b border-border last:border-0">
                                    <td className="px-6 py-4 text-foreground font-medium">{t.name}</td>
                                    <td className="px-6 py-4 font-mono text-xs text-muted-foreground">{t.token_prefix}…</td>
                                    <td className="px-6 py-4">
                                        <div className="flex gap-1">
                                            {t.scopes.map((s) => (
                                                <StatusBadge key={s} tone="neutral">{s}</StatusBadge>
                                            ))}
                                        </div>
                                    </td>
                                    <td className="px-6 py-4 text-muted-foreground">{fmtDate(t.created_at)}</td>
                                    <td className="px-6 py-4 text-muted-foreground">
                                        {t.last_used_at ? fmtDate(t.last_used_at) : 'Never'}
                                    </td>
                                    <td className="px-6 py-4">
                                        {t.revoked_at ? (
                                            <StatusBadge tone="error">revoked</StatusBadge>
                                        ) : (
                                            <StatusBadge tone="success">active</StatusBadge>
                                        )}
                                    </td>
                                    <td className="px-6 py-4 text-right space-x-3">
                                        {!t.revoked_at && (
                                            <>
                                                <button
                                                    onClick={() => handleRotate(t.id)}
                                                    disabled={isPending}
                                                    className="text-xs text-muted-foreground hover:text-foreground hover:underline disabled:opacity-50"
                                                >
                                                    Rotate
                                                </button>
                                                <button
                                                    onClick={() => handleRevoke(t.id)}
                                                    disabled={isPending}
                                                    className="text-xs text-destructive hover:underline disabled:opacity-50"
                                                >
                                                    Revoke
                                                </button>
                                            </>
                                        )}
                                    </td>
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
                        <DialogTitle className="text-foreground text-base">New Personal Access Token</DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        <div className="space-y-1">
                            <label className="text-xs text-muted-foreground uppercase tracking-widest">Name</label>
                            <input
                                autoFocus
                                value={name}
                                onChange={(e) => setName(e.target.value)}
                                placeholder="My laptop"
                                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                            />
                        </div>
                        <label className="flex items-center gap-2 text-sm text-foreground">
                            <input type="checkbox" checked={scopeWrite} onChange={(e) => setScopeWrite(e.target.checked)} />
                            Allow write access (default: read-only)
                        </label>
                        <div className="flex gap-2 justify-end pt-2">
                            <Button variant="ghost" size="sm" onClick={() => setCreateOpen(false)}>
                                Cancel
                            </Button>
                            <Button size="sm" onClick={handleCreate} disabled={isPending || !name.trim()}>
                                {isPending ? 'Creating…' : 'Create'}
                            </Button>
                        </div>
                    </div>
                </DialogContent>
            </Dialog>

            {/* Reveal-once dialog */}
            <Dialog open={revealedToken !== null} onOpenChange={(o) => { if (!o) setRevealedToken(null) }}>
                <DialogContent className="bg-card border-input max-w-md">
                    <DialogHeader>
                        <DialogTitle className="text-foreground text-base">{revealedToken?.name}</DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        <p className="text-xs text-destructive">Copy this now — you won&apos;t be able to see it again.</p>
                        <div className="flex items-center gap-2 rounded-md border border-input bg-background px-3 py-2">
                            <code className="flex-1 font-mono text-xs text-foreground break-all">
                                {revealedToken?.plaintext_token}
                            </code>
                            {revealedToken && <CopyBtn text={revealedToken.plaintext_token} label="copy" />}
                        </div>
                        <Button size="sm" onClick={() => setRevealedToken(null)}>
                            Done
                        </Button>
                    </div>
                </DialogContent>
            </Dialog>
        </div>
    )
}
