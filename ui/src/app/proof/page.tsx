"use client";

import { useEffect } from "react";
import { ProofView } from "@valori/studio";
import { useProof } from "@/lib/hooks/useProof";
import { useHealth } from "@/lib/hooks/useHealth";
import { markProofViewed } from "@/lib/onboarding";
import { ProofExport } from "@/components/proof/ProofExport";
import { ReceiptCard } from "@/components/proof/ReceiptCard";
import { LOCAL_CONNECTION_PROJECT_ID } from "@/lib/local-runtime/transport";

// Migrated to Shared Studio's ProofView (Phase G2) — the hash hero, state
// cards, and empty state are now the same implementation every host
// consumes. ReceiptCard/ProofExport are genuine shared product features
// per the Phase G investigation, wired in through ProofView's slots rather
// than moved into the package itself (their host-side source is unchanged).
// Onboarding "first proof viewed" tracking stays entirely host-side — it
// was never part of ProofView to begin with.
export default function DashboardPage() {
  const { hash } = useProof();
  const { chainHeight } = useHealth();

  useEffect(() => { if (hash) markProofViewed(); }, [hash]);

  return (
    <div className="flex flex-col gap-8 w-full max-w-[1600px]">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-foreground tracking-tight">Proof Dashboard</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            For you — live BLAKE3 state hash, updates on every committed event
          </p>
        </div>
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span className="h-2 w-2 rounded-full bg-[var(--v-accent)] animate-pulse shadow-[0_0_6px_var(--v-accent)]" />
          live · 2s
        </span>
      </div>

      <ProofView
        projectId={LOCAL_CONNECTION_PROJECT_ID}
        nodeUrl="http://localhost:3000"
        receiptCard={<ReceiptCard />}
        exportActions={<ProofExport hash={hash} chainHeight={chainHeight} />}
      />
    </div>
  );
}
