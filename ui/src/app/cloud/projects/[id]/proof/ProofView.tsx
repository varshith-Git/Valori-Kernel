'use client'

import { useProof } from '@/lib/hooks/useProof'
import { useHealth } from '@/lib/hooks/useHealth'
import { ProofHash } from '@/components/proof/ProofHash'
import { MetricCard } from '@/components/ui/metric-card'

// Trimmed port of valori-kernel/ui's proof page — drops ReceiptCard
// (operation receipts, tied to kernel's planner/operations concept, not
// ported yet) and ProofExport (CSV/PDF export — not built), and the
// onboarding "first proof viewed" tracking (kernel-only concept). The
// empty-state snippet uses this project's real node_url instead of a
// hardcoded localhost:3000.
export function ProofView({ projectId, nodeUrl }: { projectId: string; nodeUrl: string }) {
    const { hash, isLoading, error } = useProof(projectId)
    const { chainHeight, recordCount, dim, online } = useHealth(projectId)

    return (
        <div className="space-y-6">
            <div className="rounded-xl border border-[var(--v-accent)] bg-card p-6 [box-shadow:0_0_24px_var(--v-accent-muted)]">
                {!online && !isLoading ? (
                    <div className="text-sm text-destructive">Node unreachable at {nodeUrl}</div>
                ) : error && online ? (
                    <div className="text-sm text-amber-500">
                        Proof endpoint error — check the node&apos;s VALORI_EVENT_LOG_PATH is set
                    </div>
                ) : (
                    <ProofHash hash={hash} isLoading={isLoading} />
                )}
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-4">
                <MetricCard label="Chain height" value={chainHeight?.toLocaleString() ?? '—'} hint="committed events" />
                <MetricCard label="Records" value={recordCount?.toLocaleString() ?? '—'} hint="live vectors" />
                <MetricCard label="Dimension" value={dim ?? '—'} hint="Q16.16 fixed-point" />
                <MetricCard label="Algorithm" value="BLAKE3" hint="chained · deterministic" />
            </div>

            {!isLoading && online && (chainHeight === 0 || chainHeight === null) && (
                <div className="rounded-xl border border-dashed border-border p-8 text-center">
                    <p className="text-sm text-muted-foreground">No events yet.</p>
                    <p className="mt-1 text-xs text-muted-foreground">Insert your first vector:</p>
                    <pre className="mt-3 inline-block rounded bg-background px-4 py-2 text-left text-xs text-foreground overflow-x-auto max-w-full">
                        {`curl -X POST ${nodeUrl}/records \\\n  -H "Content-Type: application/json" \\\n  -d '{"values": [0.1, 0.2, 0.3, 0.4]}'`}
                    </pre>
                </div>
            )}
        </div>
    )
}
