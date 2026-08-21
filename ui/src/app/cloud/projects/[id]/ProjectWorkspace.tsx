'use client'

import { useState } from 'react'
import { useHealth } from '@/lib/hooks/useHealth'
import { useSearch } from '@/lib/hooks/useSearch'
import { useProvisionerStatus } from '@/lib/hooks/useProvisionerStatus'
import { Button } from '@/components/ui/button'
import { MetricCard } from '@/components/ui/metric-card'
import { StatusBadge } from '@/components/ui/status-badge'
import { CopyBtn } from '@/components/ui/copy-btn'
import { CollectionsPanel } from './CollectionsPanel'

const INSTANCE_TONE: Record<string, 'success' | 'warning' | 'error' | 'neutral'> = {
    running: 'success',
    provisioning: 'warning',
    stopped: 'neutral',
}

function instanceTone(status: string): 'success' | 'warning' | 'error' | 'neutral' {
    return INSTANCE_TONE[status] ?? (status.startsWith('failed') || status.startsWith('error') ? 'error' : 'neutral')
}

// Trimmed port of valori-kernel/ui's search page — health + search only.
// Kernel's page also has Proof and Timeline tabs (needs /api/proof and
// /api/activity ported the same way as health/search below); not done yet,
// tracked as follow-up rather than faked here.
export function ProjectWorkspace({ projectId }: { projectId: string }) {
    const { status, online, recordCount, dim, version } = useHealth(projectId)
    const { instances } = useProvisionerStatus(projectId)
    const { results, stateHash, isLoading, error, search, latencyMs } = useSearch(projectId)
    const [input, setInput] = useState('')
    const [k, setK] = useState(10)

    const run = () => {
        const nums = input.split(/[\s,]+/).map(Number).filter((n) => !isNaN(n))
        if (nums.length === 0) return
        search({ vector: nums, k })
    }

    return (
        <div className="space-y-6">
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <MetricCard label="Status" value={online ? (status ?? 'ok') : 'unreachable'} hint={version ?? undefined} />
                <MetricCard label="Records" value={recordCount?.toLocaleString() ?? '—'} hint="live vectors" />
                <MetricCard label="Dimension" value={dim ? String(dim) : '—'} hint="Q16.16 fixed-point" />
            </div>

            {instances.length > 0 && (
                <div className="rounded-xl border border-border bg-card p-5">
                    <p className="text-xs text-muted-foreground uppercase tracking-widest mb-3">Instances</p>
                    <div className="flex flex-wrap gap-2">
                        {instances.map((inst) => (
                            <StatusBadge key={inst.instance_id} tone={instanceTone(inst.status)}>
                                {`node ${inst.node_index}: ${inst.status}`}
                            </StatusBadge>
                        ))}
                    </div>
                </div>
            )}

            <CollectionsPanel projectId={projectId} />

            <div className="rounded-xl border border-border bg-card p-5 space-y-4">
                <p className="text-sm font-medium text-foreground">
                    Query vector {dim && <span className="text-xs text-muted-foreground font-normal">({dim}D)</span>}
                </p>
                <textarea
                    value={input}
                    onChange={(e) => setInput(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && e.metaKey && run()}
                    placeholder="0.12, 0.34, 0.56, 0.78, ..."
                    rows={3}
                    className="w-full rounded-lg border border-input bg-background px-3 py-2 font-mono text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring resize-none"
                />
                <div className="flex items-center gap-3">
                    <label className="text-xs text-muted-foreground">k</label>
                    <input
                        type="number"
                        min={1}
                        max={100}
                        value={k}
                        onChange={(e) => setK(Number(e.target.value))}
                        className="w-16 rounded-lg border border-input bg-background px-2 py-1 text-sm text-foreground"
                    />
                    <Button onClick={run} disabled={isLoading || !input.trim()} size="sm">
                        {isLoading ? 'Searching…' : 'Search'}
                    </Button>
                    {latencyMs !== null && !isLoading && (
                        <span className="ml-auto font-mono text-[11px] text-muted-foreground">{latencyMs} ms</span>
                    )}
                </div>
            </div>

            {error && !isLoading && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}

            {results.length > 0 && (
                <div className="rounded-xl border border-border bg-card p-5 space-y-3">
                    {stateHash && (
                        <div className="flex items-center gap-2 mb-1">
                            <p className="text-[11px] text-muted-foreground font-mono">
                                Searched against state <span className="text-foreground">{stateHash.slice(0, 16)}…</span>
                            </p>
                            <CopyBtn text={stateHash} label="copy hash" className="scale-75 origin-left" />
                        </div>
                    )}
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                        {results.map((r, i) => (
                            <div key={r.id} className="rounded-xl border border-border bg-background/60 px-4 py-3 flex items-center gap-2.5">
                                <span className="text-[10px] font-mono text-muted-foreground/60 w-5">{i + 1}</span>
                                <span className="font-mono text-xs font-semibold text-foreground">#{r.id}</span>
                                <StatusBadge tone="neutral">{`Score: ${r.score.toFixed(5)}`}</StatusBadge>
                            </div>
                        ))}
                    </div>
                </div>
            )}
        </div>
    )
}
