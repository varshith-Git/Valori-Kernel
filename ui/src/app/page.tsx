"use client";

import { useProof } from "@/lib/hooks/useProof";
import { ProofHash } from "@/components/proof/ProofHash";
import { MetricCard } from "@/components/proof/MetricCard";
import { ProofExport } from "@/components/proof/ProofExport";

export default function DashboardPage() {
  const { hash, chainHeight, recordCount, isLoading, error } = useProof();

  return (
    <div className="flex flex-col gap-8 max-w-4xl">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-xl font-semibold text-white">Proof Dashboard</h1>
          <p className="mt-1 text-sm text-zinc-500">
            Live BLAKE3 state hash — updates on every committed event
          </p>
        </div>
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1.5 text-xs text-zinc-500">
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse" />
            live · 2s
          </span>
          <ProofExport hash={hash} chainHeight={chainHeight} />
        </div>
      </div>

      {/* State hash — the hero element */}
      <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-6">
        {error ? (
          <div className="text-sm text-red-400">
            Backend unreachable — is Valori running?
          </div>
        ) : (
          <ProofHash hash={hash} isLoading={isLoading} />
        )}
      </div>

      {/* Metrics row */}
      <div className="grid grid-cols-3 gap-4">
        <MetricCard
          label="Chain height"
          value={chainHeight?.toLocaleString() ?? null}
          sub="committed events"
        />
        <MetricCard
          label="Records"
          value={recordCount?.toLocaleString() ?? null}
          sub="indexed vectors"
        />
        <MetricCard
          label="Algorithm"
          value="BLAKE3"
          sub="chained · deterministic"
        />
      </div>

      {/* Empty state */}
      {!isLoading && !error && chainHeight === 0 && (
        <div className="rounded-xl border border-dashed border-zinc-800 p-8 text-center">
          <p className="text-sm text-zinc-500">No events yet.</p>
          <p className="mt-1 text-xs text-zinc-600">
            Insert your first vector via the Python SDK:
          </p>
          <pre className="mt-3 inline-block rounded bg-zinc-900 px-4 py-2 text-left text-xs text-zinc-300">
{`from valoricore.remote import SyncRemoteClient
db = SyncRemoteClient("http://localhost:3000")
db.insert([0.1, 0.2, 0.3])
print(db.get_proof())`}
          </pre>
        </div>
      )}
    </div>
  );
}
