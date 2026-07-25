'use client'

import { useEffect, useState, useTransition } from 'react'
import Link from 'next/link'
import { createProject } from './actions'
import { DIMENSIONS, DEFAULT_DIMENSION, INDEX_TYPES, DEFAULT_INDEX_TYPE, type IndexType } from '@/lib/dimensions'

// Ported from valori-kernel/ui's CreateProjectDialog — quick presets for the
// most common embedding models, so users don't have to know their model's
// dimension by heart. Selecting one just sets `dim` (this control plane has
// no per-project embedding-provider slot — VALORI_EMBED_* is set on the node
// separately from provisioning); kernel's version also carries provider info
// for its local onboarding flow, which doesn't apply to a server-provisioned node.
const MODEL_PRESETS: { label: string; dim: number }[] = [
    { label: 'nomic-embed-text', dim: 768 },
    { label: 'text-embed-3-small', dim: 1536 },
    { label: 'text-embed-ada-002', dim: 1536 },
    { label: 'mxbai-embed-large', dim: 1024 },
    { label: 'bge-small-en', dim: 384 },
    { label: 'all-MiniLM-L6-v2', dim: 384 },
]

// Friendly labels for known region codes — falls back to the raw code for
// anything not in this map, so a newly-added host region still shows up
// (just less prettily) instead of being silently dropped.
const REGION_LABELS: Record<string, string> = {
    sg: 'Singapore',
    'us-east': 'US East',
    'us-west': 'US West',
    'eu-west': 'EU West',
}

// Populated from GET /api/regions (proxies the Rust backend's /v1/regions,
// which reflects real host capacity) — not hardcoded, so this list grows
// automatically as ops add hosts in new regions. Falls back to Singapore
// only if the fetch fails entirely (matches the single seeded host from
// Phase 1), never fabricates a region with no real capacity behind it.
export function CreateProjectDialog({ orgId, atLimit = false }: { orgId: string; atLimit?: boolean }) {
    const [open, setOpen] = useState(false)
    const [showUpgradePrompt, setShowUpgradePrompt] = useState(false)
    const [name, setName] = useState('')
    const [regions, setRegions] = useState<string[]>(['sg'])
    const [region, setRegion] = useState('sg')
    const [replication, setReplication] = useState(1)
    const [dim, setDim] = useState(DEFAULT_DIMENSION)
    const [indexType, setIndexType] = useState<IndexType>(DEFAULT_INDEX_TYPE)
    const [error, setError] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()

    useEffect(() => {
        fetch('/api/regions')
            .then((r) => r.json())
            .then((data: { regions?: string[]; default_region?: string }) => {
                if (data.regions && data.regions.length > 0) {
                    setRegions(data.regions)
                    // Prefer the admin-configured default_region (Layer 2.2
                    // system_settings) if it's actually available; fall back
                    // to the first region with capacity otherwise.
                    const preferred = data.default_region && data.regions.includes(data.default_region)
                        ? data.default_region
                        : data.regions[0]
                    setRegion(preferred)
                }
            })
            .catch(() => {
                // Keep the Singapore-only fallback already in state.
            })
    }, [])

    function handleSubmit(e: React.FormEvent) {
        e.preventDefault()
        setError(null)
        startTransition(async () => {
            const result = await createProject(orgId, name, region, replication, dim, indexType)
            if (result.error) {
                setError(result.error)
                return
            }
            setOpen(false)
            setName('')
        })
    }

    if (!open) {
        if (atLimit) {
            return (
                <div className="flex items-center gap-3">
                    <button
                        type="button"
                        onClick={() => setShowUpgradePrompt(true)}
                        aria-disabled
                        title="Free plan project limit reached"
                        className="px-4 py-2 bg-primary/40 text-primary-foreground/70 font-semibold rounded cursor-not-allowed text-sm"
                    >
                        + New Project
                    </button>
                    {showUpgradePrompt && (
                        <p className="text-xs text-muted-foreground">
                            You&apos;ve reached your free plan&apos;s project limit.{' '}
                            <Link href="/pricing" className="text-primary hover:underline font-medium">
                                Upgrade your plan →
                            </Link>
                        </p>
                    )}
                </div>
            )
        }
        return (
            <button
                onClick={() => setOpen(true)}
                className="px-4 py-2 bg-primary text-primary-foreground font-semibold rounded hover:opacity-90 transition text-sm"
            >
                + New Project
            </button>
        )
    }

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
            <form
                onSubmit={handleSubmit}
                className="w-full max-w-md rounded-2xl border border-border bg-card p-6 space-y-4"
            >
                <h2 className="text-lg font-bold text-foreground">New Project</h2>

                <div className="space-y-1">
                    <label className="text-xs text-muted-foreground uppercase tracking-widest">Name</label>
                    <input
                        required
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        placeholder="my-project"
                        className="w-full px-3 py-2 rounded border border-border bg-background text-foreground text-sm"
                    />
                </div>

                <div className="space-y-1">
                    <label className="text-xs text-muted-foreground uppercase tracking-widest">Region</label>
                    <select
                        value={region}
                        onChange={(e) => setRegion(e.target.value)}
                        className="w-full px-3 py-2 rounded border border-border bg-background text-foreground text-sm"
                    >
                        {regions.map((r) => (
                            <option key={r} value={r}>
                                {REGION_LABELS[r] ?? r}
                            </option>
                        ))}
                    </select>
                </div>

                <div className="space-y-1">
                    <label className="text-xs text-muted-foreground uppercase tracking-widest">
                        Dimension <span className="normal-case text-[10px] text-amber-600 dark:text-amber-500">(permanent — must match your embedding model)</span>
                    </label>
                    <div className="grid grid-cols-2 sm:grid-cols-3 gap-1.5">
                        {MODEL_PRESETS.map((p) => {
                            const active = dim === p.dim
                            return (
                                <button
                                    key={p.label}
                                    type="button"
                                    onClick={() => setDim(p.dim)}
                                    className={`flex items-center gap-2 rounded border px-2.5 py-2 text-left text-xs transition-colors ${active ? 'border-primary bg-primary/10' : 'border-border bg-background hover:border-muted-foreground/40'}`}
                                >
                                    <span className="font-mono text-foreground truncate">
                                        {p.label} <span className="text-muted-foreground">({p.dim})</span>
                                    </span>
                                </button>
                            )
                        })}
                    </div>
                    <select
                        value={dim}
                        onChange={(e) => setDim(Number(e.target.value))}
                        className="w-full px-3 py-2 rounded border border-border bg-background text-foreground text-sm font-mono"
                    >
                        {DIMENSIONS.map((d) => (
                            <option key={d.value} value={d.value}>
                                {d.label}
                            </option>
                        ))}
                    </select>
                </div>

                <div className="space-y-1">
                    <label className="text-xs text-muted-foreground uppercase tracking-widest">Index type</label>
                    <div className="grid grid-cols-5 gap-1.5">
                        {INDEX_TYPES.map((opt) => (
                            <button
                                key={opt.value}
                                type="button"
                                title={opt.title}
                                onClick={() => setIndexType(opt.value)}
                                className={`px-2 py-2 rounded border text-xs font-medium ${indexType === opt.value ? 'border-primary text-primary' : 'border-border text-muted-foreground'}`}
                            >
                                {opt.label}
                            </button>
                        ))}
                    </div>
                </div>

                <div className="space-y-1">
                    <label className="text-xs text-muted-foreground uppercase tracking-widest">Cluster</label>
                    <div className="flex gap-2">
                        <button
                            type="button"
                            onClick={() => setReplication(1)}
                            className={`flex-1 px-3 py-2 rounded border text-sm ${replication === 1 ? 'border-primary text-primary' : 'border-border text-muted-foreground'}`}
                        >
                            Single node
                        </button>
                        <button
                            type="button"
                            onClick={() => setReplication(3)}
                            className={`flex-1 px-3 py-2 rounded border text-sm ${replication === 3 ? 'border-primary text-primary' : 'border-border text-muted-foreground'}`}
                        >
                            3-node cluster
                        </button>
                    </div>
                </div>

                {error && <p className="text-xs text-destructive">{error}</p>}

                <div className="flex gap-2 pt-2">
                    <button
                        type="button"
                        onClick={() => setOpen(false)}
                        disabled={isPending}
                        className="flex-1 px-4 py-2 border border-border text-foreground rounded hover:bg-accent transition text-sm"
                    >
                        Cancel
                    </button>
                    <button
                        type="submit"
                        disabled={isPending || !name.trim()}
                        className="flex-1 px-4 py-2 bg-primary text-primary-foreground font-semibold rounded hover:opacity-90 transition disabled:opacity-50 text-sm"
                    >
                        {isPending ? 'Creating…' : 'Create'}
                    </button>
                </div>
            </form>
        </div>
    )
}
