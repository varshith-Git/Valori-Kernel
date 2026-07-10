"use client";

import { useState } from "react";

interface Props {
  hash: string | null;
  isLoading?: boolean;
}

export function ProofHash({ hash, isLoading }: Props) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  const copy = () => {
    if (!hash) return;
    navigator.clipboard.writeText(hash)
      .then(() => setCopyState("copied"))
      .catch(() => setCopyState("failed"))
      .finally(() => setTimeout(() => setCopyState("idle"), 1500));
  };

  if (isLoading || !hash) {
    return (
      <div className="flex flex-col gap-2">
        <span className="text-xs text-muted-foreground uppercase tracking-widest">
          State Hash
        </span>
        <div className="h-10 w-full animate-pulse rounded bg-accent" />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <span className="text-xs text-muted-foreground uppercase tracking-widest">
        State Hash
      </span>
      <div className="flex items-center gap-3">
        <code className="break-all font-mono text-lg font-medium tracking-tight text-emerald-600 dark:text-emerald-400">
          {hash}
        </code>
        <button
          onClick={copy}
          className="shrink-0 rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          title="Copy hash"
        >
          {copyState === "copied" ? "✓ copied" : copyState === "failed" ? "✗ copy failed" : "copy"}
        </button>
      </div>
    </div>
  );
}
