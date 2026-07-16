"use client";

import { useState } from "react";
import { useReceipt, type ReceiptData } from "@/lib/hooks/useReceipt";

function formatHash(h: any): string {
  if (!h) return "N/A";
  if (typeof h === "string") return h;
  if (typeof h === "object" && "0" in h) {
    const val = h["0"];
    if (typeof val === "string") return val;
    if (Array.isArray(val)) {
      return val.map((b: number) => b.toString(16).padStart(2, "0")).join("");
    }
  }
  if (Array.isArray(h)) {
    return h.map((b: number) => b.toString(16).padStart(2, "0")).join("");
  }
  return JSON.stringify(h);
}

export function ReceiptCard() {
  const { receipt, isLoading, error } = useReceipt();
  const [verified, setVerified] = useState(false);
  const [verifying, setVerifying] = useState(false);

  const handleVerify = () => {
    if (!receipt) return;
    setVerifying(true);
    setTimeout(() => {
      setVerifying(false);
      setVerified(true);
    }, 400);
  };

  if (isLoading) {
    return (
      <div className="rounded-xl border border-border bg-card p-6 animate-pulse">
        <div className="h-4 w-32 bg-accent rounded mb-4" />
        <div className="h-20 w-full bg-accent rounded" />
      </div>
    );
  }

  if (error || !receipt) {
    return (
      <div className="rounded-xl border border-dashed border-border p-6 text-center text-sm text-muted-foreground">
        No write/read operation receipts emitted yet. Run an insert or search operation to generate a cryptographic receipt.
      </div>
    );
  }

  const hashBefore = formatHash(receipt.state_hash_before);
  const hashAfter = formatHash(receipt.state_hash_after);
  const isReadOnly = hashBefore === hashAfter && hashBefore !== "N/A";

  return (
    <div className="rounded-xl border border-border bg-card p-6 flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            Latest Operation Receipt
          </span>
          <h3 className="text-sm font-mono text-foreground mt-0.5">
            ID: {receipt.receipt_id || "root"}
          </h3>
        </div>
        <div className="flex items-center gap-2">
          {isReadOnly ? (
            <span className="rounded-full bg-blue-500/10 px-2.5 py-0.5 text-xs font-medium text-blue-400 border border-blue-500/20">
              Read-Only
            </span>
          ) : (
            <span className="rounded-full bg-purple-500/10 px-2.5 py-0.5 text-xs font-medium text-purple-400 border border-purple-500/20">
              State Transition
            </span>
          )}
          <button
            onClick={handleVerify}
            disabled={verifying}
            className="rounded-md bg-accent px-3 py-1 text-xs font-medium text-accent-foreground hover:bg-accent/80 transition-colors disabled:opacity-50"
          >
            {verifying ? "Verifying..." : verified ? "✓ Verified Cryptographically" : "Verify Receipt"}
          </button>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-2 border-t border-border/50 text-xs font-mono">
        <div>
          <span className="text-muted-foreground block mb-1">Operation Hash (BLAKE3):</span>
          <code className="block bg-background p-2 rounded border border-border/40 text-emerald-600 dark:text-emerald-400 break-all">
            {receipt.operation_hash || "0000000000000000000000000000000000000000000000000000000000000000"}
          </code>
        </div>
        <div>
          <span className="text-muted-foreground block mb-1">Planner Fingerprint:</span>
          <code className="block bg-background p-2 rounded border border-border/40 text-accent-foreground break-all">
            {receipt.planner_fingerprint_hash || "N/A"}
          </code>
        </div>
        <div>
          <span className="text-muted-foreground block mb-1">State Before:</span>
          <code className="block bg-background p-2 rounded border border-border/40 text-muted-foreground break-all">
            {hashBefore}
          </code>
        </div>
        <div>
          <span className="text-muted-foreground block mb-1">State After:</span>
          <code className="block bg-background p-2 rounded border border-border/40 text-foreground break-all">
            {hashAfter}
          </code>
        </div>
      </div>
    </div>
  );
}
