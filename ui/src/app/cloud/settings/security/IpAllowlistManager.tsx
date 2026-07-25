'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { addIpAllowlistRule, removeIpAllowlistRule } from './actions'
import type { IpAllowlistRuleRow } from './page'

export function IpAllowlistManager({
    orgId,
    orgName,
    canManage,
    initialRules,
}: {
    orgId: string
    orgName: string
    canManage: boolean
    initialRules: IpAllowlistRuleRow[]
}) {
    const router = useRouter()
    const [cidr, setCidr] = useState('')
    const [description, setDescription] = useState('')
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()

    const handleAdd = (e: React.FormEvent) => {
        e.preventDefault()
        if (!cidr.trim()) return
        setError(null)
        startTransition(async () => {
            const result = await addIpAllowlistRule(orgId, cidr.trim(), description.trim())
            if (result.error) {
                setError(result.error)
                return
            }
            setCidr('')
            setDescription('')
            router.refresh()
        })
    }

    const handleRemove = (ruleId: string) => {
        startTransition(async () => {
            const result = await removeIpAllowlistRule(ruleId)
            if (result.error) {
                setError(result.error)
                return
            }
            router.refresh()
        })
    }

    return (
        <section className="rounded-xl border border-border bg-card p-6 space-y-4">
            <div>
                <h2 className="text-sm font-semibold text-foreground">IP Allowlist</h2>
                <p className="text-xs text-muted-foreground mt-1">
                    Restrict {orgName}&apos;s API keys to specific IPs or networks. Empty list = no restriction. The
                    dashboard itself is never IP-restricted, only programmatic <code className="font-mono">vlk_...</code> requests.
                </p>
            </div>

            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}

            <div className="divide-y divide-border">
                {initialRules.map((r) => (
                    <div key={r.id} className="flex items-center justify-between py-3 gap-4">
                        <div>
                            <p className="text-sm font-mono text-foreground">{r.cidr}</p>
                            {r.description && <p className="text-xs text-muted-foreground mt-0.5">{r.description}</p>}
                        </div>
                        {canManage && (
                            <button
                                onClick={() => handleRemove(r.id)}
                                disabled={isPending}
                                className="text-xs text-destructive hover:underline disabled:opacity-50 shrink-0"
                            >
                                Remove
                            </button>
                        )}
                    </div>
                ))}
                {initialRules.length === 0 && (
                    <p className="text-sm text-muted-foreground py-3">No rules — API keys are usable from any IP.</p>
                )}
            </div>

            {canManage && (
                <form onSubmit={handleAdd} className="flex items-center gap-2 pt-2">
                    <input
                        value={cidr}
                        onChange={(e) => setCidr(e.target.value)}
                        placeholder="203.0.113.0/24"
                        className="w-40 shrink-0 rounded-md border border-input bg-background px-3 py-1.5 text-sm font-mono text-foreground placeholder:text-muted-foreground"
                    />
                    <input
                        value={description}
                        onChange={(e) => setDescription(e.target.value)}
                        placeholder="Office network (optional)"
                        className="flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground"
                    />
                    <Button type="submit" size="sm" disabled={isPending || !cidr.trim()}>
                        {isPending ? 'Adding…' : 'Add'}
                    </Button>
                </form>
            )}
        </section>
    )
}
