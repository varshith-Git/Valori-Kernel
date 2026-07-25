'use client'

// Ported from valori-kernel/ui's app/cluster/page.tsx, adapted for
// multi-tenancy — maps naturally onto this app's replication=1
// (standalone) vs. replication=3 (Raft cluster) project model.

import { useCluster } from '@/lib/hooks/useCluster'
import { NodeCard } from '@/components/cluster/NodeCard'
import { Button } from '@/components/ui/button'
import { PageHeader } from '@/components/ui/page-header'
import { Skeleton } from '@/components/ui/skeleton'
import { StatusBadge } from '@/components/ui/status-badge'

export function ClusterView({ projectId }: { projectId: string }) {
    const {
        members,
        leaderId,
        nodeId,
        isLeader,
        term,
        lastLogIndex,
        lastAppliedIndex,
        converged,
        isStandalone,
        isLoading,
        error,
        refresh,
    } = useCluster(projectId)

    if (isLoading) {
        return (
            <div className="flex flex-col gap-6 w-full">
                <Skeleton className="h-7 w-40 bg-accent" />
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                    {[1, 2, 3].map((i) => (
                        <Skeleton key={i} className="h-36 rounded-xl bg-accent" />
                    ))}
                </div>
            </div>
        )
    }

    if (error) {
        return (
            <div className="w-full">
                <div className="rounded-xl border border-red-500/30 bg-red-500/10 p-5">
                    <p className="text-sm text-red-600 dark:text-red-400">Node unreachable</p>
                    <p className="mt-1 text-xs text-red-700">{String(error)}</p>
                </div>
            </div>
        )
    }

    if (isStandalone) {
        return (
            <div className="w-full">
                <div className="rounded-xl border border-border bg-card p-8 text-center">
                    <p className="text-sm text-muted-foreground font-medium">Running in standalone mode</p>
                    <p className="mt-2 text-xs text-muted-foreground max-w-sm mx-auto">
                        This project has a single node (replication = 1), not a Raft cluster. Create a project with
                        3-node replication to see cluster health here.
                    </p>
                </div>
            </div>
        )
    }

    const lag = lastLogIndex != null && lastAppliedIndex != null ? lastLogIndex - lastAppliedIndex : null

    return (
        <div className="flex flex-col gap-6 w-full">
            <PageHeader
                title="Cluster Health"
                subtitle={
                    <>
                        Raft consensus · {members.length} node{members.length !== 1 ? 's' : ''} · term {term ?? '—'}
                    </>
                }
                actions={
                    <div className="flex items-center gap-3">
                        <StatusBadge tone={converged ? 'success' : 'warning'} pulse={!converged}>
                            {converged ? 'converged' : 'catching up'}
                        </StatusBadge>
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={() => refresh()}
                            className="border-input text-muted-foreground hover:text-foreground hover:bg-accent"
                        >
                            Refresh
                        </Button>
                    </div>
                }
            />

            <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-4">
                <StatCard label="This Node" value={nodeId != null ? `node-${nodeId}` : '—'} />
                <StatCard label="Role" value={isLeader ? 'Leader' : 'Follower'} highlight={isLeader} />
                <StatCard label="Last Log" value={lastLogIndex?.toLocaleString() ?? '—'} sub="entries committed" />
                <StatCard
                    label="Applied"
                    value={lastAppliedIndex?.toLocaleString() ?? '—'}
                    sub={lag != null ? `${lag} behind` : undefined}
                    warn={lag != null && lag > 0}
                />
            </div>

            <div>
                <h2 className="text-sm font-medium text-muted-foreground mb-3">Members ({members.length})</h2>
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                    {members.map((m) => (
                        <NodeCard key={m.id} member={m} isLeader={m.id === leaderId} isThisNode={m.id === nodeId} />
                    ))}
                </div>
            </div>

            {lag != null && lag > 10 && (
                <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 px-5 py-4">
                    <p className="text-sm text-amber-600 dark:text-amber-400 font-medium">Apply lag: {lag} entries behind committed log</p>
                    <p className="mt-1 text-xs text-amber-700">
                        This node is still applying committed entries. Reads may not reflect the latest state.
                    </p>
                </div>
            )}

            {members.length === 0 && (
                <div className="rounded-xl border border-dashed border-border py-12 text-center">
                    <p className="text-sm text-muted-foreground">No members found in cluster status.</p>
                </div>
            )}
        </div>
    )
}

function StatCard({ label, value, sub, highlight, warn }: { label: string; value: string; sub?: string; highlight?: boolean; warn?: boolean }) {
    return (
        <div className="rounded-lg border border-border bg-card px-4 py-4">
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
            <p className={`mt-1.5 font-mono text-xl font-semibold ${highlight ? 'text-emerald-600 dark:text-emerald-400' : warn ? 'text-amber-600 dark:text-amber-400' : 'text-foreground'}`}>
                {value}
            </p>
            {sub && <p className={`mt-0.5 text-xs ${warn ? 'text-amber-600' : 'text-muted-foreground'}`}>{sub}</p>}
        </div>
    )
}
