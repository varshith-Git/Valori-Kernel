"use client";

import type { StageViewModel } from "@/lib/execution-viewmodel";

interface Props {
  stages: StageViewModel[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/** Chrome DevTools-style duration bars — one row per stage, width proportional
 *  to duration. No graph needed to see where the time went. */
export default function ExecutionTimeline({ stages, selectedId, onSelect }: Props) {
  const maxDuration = Math.max(1, ...stages.map((s) => s.durationMs));

  return (
    <div className="flex flex-col gap-1.5">
      {stages.map((stage) => {
        const pct = Math.max(2, (stage.durationMs / maxDuration) * 100);
        const isSelected = stage.id === selectedId;
        return (
          <button
            key={stage.id}
            type="button"
            onClick={() => onSelect(stage.id)}
            className={`flex items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors ${
              isSelected ? "bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/40" : "hover:bg-accent/40 border border-transparent"
            }`}
          >
            <span className="w-40 shrink-0 text-sm font-medium text-foreground truncate">{stage.title}</span>
            <span className="flex-1 h-5 rounded bg-muted/60 overflow-hidden">
              <span
                className={`block h-full rounded ${stage.success ? "bg-[var(--v-accent)]" : "bg-red-500"}`}
                style={{ width: `${pct}%` }}
              />
            </span>
            <span className="w-16 shrink-0 text-right font-mono text-xs text-muted-foreground">
              {stage.durationMs} ms
            </span>
          </button>
        );
      })}
    </div>
  );
}
