"use client";

import { useState } from "react";

interface Props {
  hash: string | null;
  isLoading?: boolean;
}

export function ProofHash({ hash, isLoading }: Props) {
  const [copied, setCopied] = useState(false);

  const copy = () => {
    if (!hash) return;
    navigator.clipboard.writeText(hash).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
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
        <code className="break-all font-mono text-lg font-medium tracking-tight text-emerald-400">
          {hash}
        </code>
        <button
          onClick={copy}
          className="shrink-0 rounded px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          title="Copy hash"
        >
          {copied ? "✓ copied" : "copy"}
        </button>
      </div>
    </div>
  );
}
