"use client";

import { useMemo, useState } from "react";
import { RefreshCw, SearchX } from "lucide-react";
import { isExecutionRecord, toExecutionViewModel } from "@/lib/execution-viewmodel";
import { EmptyState } from "@/components/ui/EmptyState";
import { MetricCard } from "@/components/ui/MetricCard";
import ExecutionTimeline from "./ExecutionTimeline";
import ExecutionGraph from "./ExecutionGraph";
import StageDetailPanel from "./StageDetailPanel";
import ExecutionProofPanel from "./ExecutionProofPanel";

interface Props {
  loading: boolean;
  /** Raw JSON from `GET /api/operations/:id/execution` — may be `null` (not
   *  fetched yet), an error body, or a real `ExecutionRecord`. */
  data: unknown;
}

export default function ExecutionExplorer({ loading, data }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const vm = useMemo(() => (isExecutionRecord(data) ? toExecutionViewModel(data) : null), [data]);

  if (loading) {
    return (
      <div className="flex justify-center items-center h-[300px]">
        <RefreshCw className="h-8 w-8 animate-spin text-[var(--v-accent)]" />
      </div>
    );
  }

  if (!vm) {
    return (
      <EmptyState
        icon={SearchX}
        title="Execution not available"
        description="This operation predates execution tracing, or its record was evicted from the execution cache."
      />
    );
  }

  const selected = vm.stages.find((s) => s.id === selectedId) ?? null;
  const select = (id: string) => setSelectedId((prev) => (prev === id ? null : id));

  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <MetricCard label="Total duration" value={`${vm.totalDurationMs} ms`} />
        <MetricCard label="Chunks / records" value={`${vm.chunksProduced} / ${vm.recordsWritten}`} />
        <MetricCard
          label="Source"
          value={
            <span className="text-sm truncate block" title={vm.documentSource}>
              {vm.documentSource}
            </span>
          }
        />
      </div>

      <div>
        <h4 className="text-sm font-semibold text-foreground mb-2">Timeline</h4>
        <ExecutionTimeline stages={vm.stages} selectedId={selectedId} onSelect={select} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-[1fr_280px] gap-4">
        <div>
          <h4 className="text-sm font-semibold text-foreground mb-2">Execution graph</h4>
          <ExecutionGraph stages={vm.stages} selectedId={selectedId} onSelect={select} />
        </div>
        <div>
          <h4 className="text-sm font-semibold text-foreground mb-2">Stage detail</h4>
          <StageDetailPanel stage={selected} />
        </div>
      </div>

      <ExecutionProofPanel vm={vm} />
    </div>
  );
}
