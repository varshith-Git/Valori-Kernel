"use client";

import { useProof } from "@/lib/hooks/useProof";
import { useHealth } from "@/lib/hooks/useHealth";
import { ProofHash } from "@/components/proof/ProofHash";
import { MetricCard } from "@/components/proof/MetricCard";
import { ProofExport } from "@/components/proof/ProofExport";
import { ReceiptCard } from "@/components/proof/ReceiptCard";

export default function DashboardPage() {
  const { hash, isLoading, error } = useProof();
  const { chainHeight, recordCount, dim, online } = useHealth();

  return (
    <div className="flex flex-col gap-8 w-full max-w-[1600px]">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-foreground tracking-tight">Proof Dashboard</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            For you — live BLAKE3 state hash, updates on every committed event
          </p>
        </div>
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <span className="h-2 w-2 rounded-full bg-[var(--v-accent)] animate-pulse shadow-[0_0_6px_var(--v-accent)]" />
            live · 2s
          </span>
          <ProofExport hash={hash} chainHeight={chainHeight} />
        </div>
      </div>

      {/* State hash — the hero element */}
      <div className="rounded-xl border border-[var(--v-accent)] bg-card p-6 [box-shadow:0_0_24px_var(--v-accent-muted)]">
        {!online && !isLoading ? (
          <div className="text-sm text-red-400">
            Backend unreachable — start Valori on{" "}
            <code className="font-mono">localhost:3000</code>
          </div>
        ) : error && online ? (
          <div className="text-sm text-amber-400">
            Proof endpoint error — check VALORI_EVENT_LOG_PATH is set
          </div>
        ) : (
          <ProofHash hash={hash} isLoading={isLoading} />
        )}
      </div>

      {/* Operation Receipt */}
      <ReceiptCard />

      {/* Metrics row */}
      <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-4">
        <MetricCard
          label="Chain height"
          value={chainHeight?.toLocaleString() ?? null}
          sub="committed events"
        />
        <MetricCard
          label="Records"
          value={recordCount?.toLocaleString() ?? null}
          sub="live vectors"
        />
        <MetricCard
          label="Dimension"
          value={dim ?? null}
          sub="Q16.16 fixed-point"
        />
        <MetricCard
          label="Algorithm"
          value="BLAKE3"
          sub="chained · deterministic"
        />
      </div>

      {/* Empty state */}
      {!isLoading && online && (chainHeight === 0 || chainHeight === null) && (
        <div className="rounded-xl border border-dashed border-border p-8 text-center">
          <p className="text-sm text-muted-foreground">No events yet.</p>
          <p className="mt-1 text-xs text-muted-foreground">
            Insert your first vector via the Python SDK or curl:
          </p>
          <pre className="mt-3 inline-block rounded bg-card px-4 py-2 text-left text-xs text-accent-foreground">
{`# Python SDK
from valoricore.remote import SyncRemoteClient
db = SyncRemoteClient("http://localhost:3000")
db.insert([0.1, 0.2, 0.3, 0.4])

# or curl
curl -X POST http://localhost:3000/records \\
  -H "Content-Type: application/json" \\
  -d '{"values": [0.1, 0.2, 0.3, 0.4]}'`}
          </pre>
        </div>
      )}
    </div>
  );
}
