"use client";

import { AlertTriangle, Clock, XCircle } from "lucide-react";
import type { StageViewModel } from "@/lib/execution-viewmodel";
import { metricLabel } from "@/lib/execution-viewmodel";

export default function StageDetailPanel({ stage }: { stage: StageViewModel | null }) {
  if (!stage) {
    return (
      <div className="rounded-xl border border-dashed border-border/60 bg-muted/20 p-6 text-center text-sm text-muted-foreground">
        Select a stage to see its details.
      </div>
    );
  }

  return (
    <div className="rounded-xl border border-border/80 bg-card/60 p-4 flex flex-col gap-3">
      <div className="flex items-center justify-between border-b border-border/60 pb-2">
        <h4 className="font-semibold text-foreground">{stage.title}</h4>
        {!stage.success && (
          <span className="inline-flex items-center gap-1 text-xs font-semibold text-red-500">
            <XCircle className="h-3.5 w-3.5" /> failed
          </span>
        )}
      </div>

      <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <Clock className="h-3.5 w-3.5" />
        Duration
        <span className="ml-auto font-mono text-foreground">{stage.durationMs} ms</span>
      </div>

      {Object.entries(stage.metrics).map(([key, value]) => (
        <div key={key} className="flex items-center gap-1.5 text-sm text-muted-foreground">
          {metricLabel(key)}
          <span className="ml-auto font-mono text-foreground">{String(value)}</span>
        </div>
      ))}

      {stage.error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-400">
          {stage.error}
        </div>
      )}

      {stage.warnings.length > 0 && (
        <div className="flex flex-col gap-1">
          {stage.warnings.map((w, i) => (
            <div key={i} className="flex items-start gap-1.5 text-xs text-amber-500">
              <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
              {w}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
