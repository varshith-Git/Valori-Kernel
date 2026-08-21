'use client'

import type { ReactNode } from 'react'
import { useProof } from '@/lib/hooks/useProof'
import { useHealth } from '@/lib/hooks/useHealth'
import { ProofHash } from '@/components/proof/ProofHash'
import { MetricCard } from '@/components/ui/metric-card'

// Ported from valori-kernel/ui's proof page. Studio S9 reconciliation: the
// investigation found ReceiptCard (operation receipt + client verify UI)
// and ProofExport (a plain Blob/download of the state hash) are genuine
// shared product functionality, not Local-only — neither depends on the
// filesystem or Tauri. They're wired in as host-supplied slots rather than
// moved into this package outright, because `ReceiptCard` depends on
// `useReceipt()`, whose endpoint contract hasn't yet been verified
// identical across all three hosts (see the Phase G report) — safer to let
// the host keep owning that component's source for now. Onboarding "first
// proof viewed" tracking stays entirely host-side (a useEffect in the
// host's own page), same as before this change — it was never inside this
// component to begin with.
export function ProofView({
    projectId,
    nodeUrl,
    receiptCard,
    exportActions,
}: {
    projectId: string
    nodeUrl: string
    /** Host-supplied receipt panel (e.g. `<ReceiptCard />`) rendered right
     *  after the state-hash hero. Omit for today's unchanged Cloud behavior. */
    receiptCard?: ReactNode
    /** Host-supplied export controls (e.g. `<ProofExport hash={...} chainHeight={...} />`)
     *  rendered next to the state-hash hero. Omit to hide. */
    exportActions?: ReactNode
}) {
    const { hash, isLoading, error } = useProof(projectId)
    const { chainHeight, recordCount, dim, online } = useHealth(projectId)

    return (
        <div className="space-y-6">
            {exportActions && (
                <div className="flex items-center justify-end">{exportActions}</div>
            )}

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

            {receiptCard}

            <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-4">
                <MetricCard label="Chain height" value={chainHeight?.toLocaleString() ?? '—'} hint="committed events" />
                <MetricCard label="Records" value={recordCount?.toLocaleString() ?? '—'} hint="live vectors" />
                <MetricCard label="Dimension" value={dim ?? '—'} hint="Q16.16 fixed-point" />
                <MetricCard label="Algorithm" value="BLAKE3" hint="chained · deterministic" />
            </div>

            {!isLoading && online && (chainHeight === 0 || chainHeight === null) && (
                <div className="rounded-xl border border-dashed border-border p-8 text-center">
                    <p className="text-sm text-muted-foreground">No events yet.</p>
                    <p className="mt-1 text-xs text-muted-foreground">Insert your first vector via the Python SDK or curl:</p>
                    <pre className="mt-3 inline-block rounded bg-background px-4 py-2 text-left text-xs text-foreground overflow-x-auto max-w-full">
                        {`# Python SDK\nfrom valoricore.remote import SyncRemoteClient\ndb = SyncRemoteClient("${nodeUrl}")\ndb.insert([0.1, 0.2, 0.3, 0.4])\n\n# or curl\ncurl -X POST ${nodeUrl}/records \\\n  -H "Content-Type: application/json" \\\n  -d '{"values": [0.1, 0.2, 0.3, 0.4]}'`}
                    </pre>
                </div>
            )}
        </div>
    )
}
