"use client";

import { CopyBtn } from "@/components/ui/copy-btn";
import { Skeleton } from "@/components/ui/skeleton";

interface Props {
  hash: string | null;
  isLoading?: boolean;
}

export function ProofHash({ hash, isLoading }: Props) {
  if (isLoading || !hash) {
    return (
      <div className="flex flex-col gap-2">
        <span className="text-xs text-muted-foreground uppercase tracking-widest">
          State Hash
        </span>
        <Skeleton className="h-10 w-full rounded bg-accent" />
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
        <CopyBtn text={hash} label="copy" />
      </div>
    </div>
  );
}
