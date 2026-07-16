"use client";

import { useState } from "react";
import { Check, Copy, ShieldCheck } from "lucide-react";
import type { ExecutionViewModel } from "@/lib/execution-viewmodel";
import { StatusBadge } from "@/components/ui/StatusBadge";

function CopyableHash({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{label}</span>
      <button
        type="button"
        onClick={copy}
        className="group flex items-center gap-2 rounded-lg border border-border/60 bg-background/80 px-2.5 py-1.5 font-mono text-xs text-foreground text-left break-all"
        title="Copy"
      >
        <span className="flex-1 break-all">{value}</span>
        {copied ? (
          <Check className="h-3.5 w-3.5 shrink-0 text-emerald-500" />
        ) : (
          <Copy className="h-3.5 w-3.5 shrink-0 text-muted-foreground group-hover:text-foreground" />
        )}
      </button>
    </div>
  );
}

/** This is where Valori becomes different from a generic ingest progress bar:
 *  the receipt id and the state-hash transition this operation produced. */
export default function ExecutionProofPanel({ vm }: { vm: ExecutionViewModel }) {
  const hasProof = vm.receiptId || vm.stateHashBefore || vm.stateHashAfter;

  if (!hasProof) {
    return (
      <div className="rounded-xl border border-dashed border-border/60 bg-muted/20 p-4 text-center text-sm text-muted-foreground">
        No receipt was recorded for this execution.
      </div>
    );
  }

  return (
    <div className="rounded-xl border border-border/80 bg-card/60 p-4 flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h4 className="font-semibold text-foreground flex items-center gap-2">
          <ShieldCheck className="h-4 w-4 text-[var(--v-accent)]" />
          Receipt &amp; audit
        </h4>
        {vm.success && <StatusBadge tone="success">Verified</StatusBadge>}
      </div>

      {vm.receiptId && <CopyableHash label="Receipt ID" value={vm.receiptId} />}

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        {vm.stateHashBefore && <CopyableHash label="State hash — before" value={vm.stateHashBefore} />}
        {vm.stateHashAfter && <CopyableHash label="State hash — after" value={vm.stateHashAfter} />}
      </div>
    </div>
  );
}
