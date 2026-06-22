"use client";

import { useCluster } from "@/lib/hooks/useCluster";
import { NodeCard } from "@/components/cluster/NodeCard";
import { Button } from "@/components/ui/button";

export default function ClusterPage() {
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
  } = useCluster();

  if (isLoading) {
    return (
      <div className="flex flex-col gap-6 max-w-4xl">
        <div className="h-7 w-40 animate-pulse rounded bg-accent" />
        <div className="grid grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-36 animate-pulse rounded-xl bg-accent" />
          ))}
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="max-w-2xl">
        <h1 className="text-xl font-semibold text-foreground">Cluster Health</h1>
        <div className="mt-6 rounded-xl border border-red-900 bg-red-950 p-5">
          <p className="text-sm text-red-400">Backend unreachable</p>
          <p className="mt-1 text-xs text-red-700">{String(error)}</p>
        </div>
      </div>
    );
  }

  if (isStandalone) {
    return (
      <div className="max-w-2xl">
        <h1 className="text-xl font-semibold text-foreground">Cluster Health</h1>
        <div className="mt-6 rounded-xl border border-border bg-card p-8 text-center">
          <p className="text-sm text-muted-foreground font-medium">Running in standalone mode</p>
          <p className="mt-2 text-xs text-muted-foreground max-w-sm mx-auto">
            This node is not part of a Raft cluster. To enable cluster mode,
            set <code className="font-mono bg-accent px-1 rounded">VALORI_CLUSTER_MEMBERS</code> and{" "}
            <code className="font-mono bg-accent px-1 rounded">VALORI_NODE_ID</code> and
            restart.
          </p>
          <pre className="mt-4 rounded-lg bg-background px-5 py-4 text-left text-xs text-accent-foreground font-mono inline-block">
{`docker compose -f docker-compose.cluster.yml up -d`}
          </pre>
        </div>
      </div>
    );
  }

  const lag =
    lastLogIndex != null && lastAppliedIndex != null
      ? lastLogIndex - lastAppliedIndex
      : null;

  return (
    <div className="flex flex-col gap-6 max-w-4xl">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-foreground">Cluster Health</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Raft consensus · {members.length} node{members.length !== 1 ? "s" : ""} ·{" "}
            term {term ?? "—"}
          </p>
        </div>
        <div className="flex items-center gap-3">
          <div
            className={`flex items-center gap-1.5 text-xs rounded-full px-3 py-1.5 border ${
              converged
                ? "bg-emerald-950 text-emerald-400 border-emerald-900"
                : "bg-amber-950 text-amber-400 border-amber-900"
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                converged ? "bg-emerald-400" : "bg-amber-400 animate-pulse"
              }`}
            />
            {converged ? "converged" : "catching up"}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => refresh()}
            className="border-input text-muted-foreground hover:text-foreground hover:bg-accent"
          >
            Refresh
          </Button>
        </div>
      </div>

      {/* Raft stats row */}
      <div className="grid grid-cols-4 gap-4">
        <StatCard label="This Node" value={nodeId != null ? `node-${nodeId}` : "—"} />
        <StatCard
          label="Role"
          value={isLeader ? "Leader" : "Follower"}
          highlight={isLeader}
        />
        <StatCard
          label="Last Log"
          value={lastLogIndex?.toLocaleString() ?? "—"}
          sub="entries committed"
        />
        <StatCard
          label="Applied"
          value={lastAppliedIndex?.toLocaleString() ?? "—"}
          sub={lag != null ? `${lag} behind` : undefined}
          warn={lag != null && lag > 0}
        />
      </div>

      {/* Node cards */}
      <div>
        <h2 className="text-sm font-medium text-muted-foreground mb-3">
          Members ({members.length})
        </h2>
        <div className="grid grid-cols-3 gap-4">
          {members.map((m) => (
            <NodeCard
              key={m.id}
              member={m}
              isLeader={m.id === leaderId}
              isThisNode={m.id === nodeId}
            />
          ))}
        </div>
      </div>

      {/* Lag warning */}
      {lag != null && lag > 10 && (
        <div className="rounded-xl border border-amber-900 bg-amber-950 px-5 py-4">
          <p className="text-sm text-amber-400 font-medium">
            Apply lag: {lag} entries behind committed log
          </p>
          <p className="mt-1 text-xs text-amber-700">
            This node is still applying committed entries. Reads may not reflect
            the latest state. Use <code className="font-mono">consistency=linearizable</code>{" "}
            to force a read-index check.
          </p>
        </div>
      )}

      {/* Standalone hint (shouldn't reach here but safety net) */}
      {members.length === 0 && (
        <div className="rounded-xl border border-dashed border-border py-12 text-center">
          <p className="text-sm text-muted-foreground">No members found in cluster status.</p>
        </div>
      )}
    </div>
  );
}

function StatCard({
  label,
  value,
  sub,
  highlight,
  warn,
}: {
  label: string;
  value: string;
  sub?: string;
  highlight?: boolean;
  warn?: boolean;
}) {
  return (
    <div className="rounded-lg border border-border bg-card px-4 py-4">
      <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
      <p
        className={`mt-1.5 font-mono text-xl font-semibold ${
          highlight ? "text-emerald-400" : warn ? "text-amber-400" : "text-foreground"
        }`}
      >
        {value}
      </p>
      {sub && (
        <p className={`mt-0.5 text-xs ${warn ? "text-amber-600" : "text-muted-foreground"}`}>
          {sub}
        </p>
      )}
    </div>
  );
}
