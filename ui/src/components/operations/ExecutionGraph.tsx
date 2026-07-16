"use client";

import { useCallback, useEffect } from "react";
import {
  ReactFlow,
  MiniMap,
  Controls,
  Background,
  useNodesState,
  useEdgesState,
  MarkerType,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { FileText, ShieldCheck, Layers, Sparkles, Database, XCircle } from "lucide-react";
import type { StageViewModel } from "@/lib/execution-viewmodel";

const stageIconMap: Record<string, React.ReactNode> = {
  reader: <FileText className="h-4 w-4" />,
  validator: <ShieldCheck className="h-4 w-4" />,
  chunker: <Layers className="h-4 w-4" />,
  embedder: <Sparkles className="h-4 w-4" />,
  writer: <Database className="h-4 w-4" />,
};

/** A short, at-a-glance metric to show on the node itself (not the full
 *  detail — that's the side panel). One per stage kind, picked deliberately. */
function headlineMetric(stage: StageViewModel): string | null {
  switch (stage.id) {
    case "embedder": {
      const provider = stage.metrics.provider;
      const model = stage.metrics.model;
      return provider || model ? `${provider ?? ""}/${model ?? ""}` : null;
    }
    case "writer":
      return stage.metrics.records_written != null ? `${stage.metrics.records_written} records` : null;
    case "chunker":
      return stage.metrics.chunks_created != null ? `${stage.metrics.chunks_created} chunks` : null;
    default:
      return null;
  }
}

interface Props {
  stages: StageViewModel[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/** Linear DAG of pipeline stages — reuses the React Flow shell (minimap,
 *  controls, node styling) from the original Execution Explorer scaffold;
 *  what changed is the data it renders (real stages, not a fabricated
 *  planner task graph) and that selection is lifted to the parent so the
 *  timeline and the graph share one detail panel. */
export default function ExecutionGraph({ stages, selectedId, onSelect }: Props) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [nodes, setNodes, onNodesChange] = useNodesState<any>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const [edges, setEdges, onEdgesChange] = useEdgesState<any>([]);

  useEffect(() => {
    setNodes(stages.map((s, i) => buildNode(s, i, s.id === selectedId)));
    setEdges(
      stages.slice(1).map((s, i) => buildEdge(stages[i].id, s.id)),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stages, selectedId]);

  const onNodeClick = useCallback(
    (_: React.MouseEvent, node: { id: string }) => onSelect(node.id),
    [onSelect],
  );

  return (
    <div className="relative w-full h-[420px] rounded-lg border border-border bg-background overflow-hidden">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeClick={onNodeClick}
        fitView
        attributionPosition="bottom-right"
      >
        <MiniMap
          nodeColor="var(--v-accent)"
          maskColor="color-mix(in oklch, var(--background) 70%, transparent)"
          className="border border-border rounded-lg bg-card"
        />
        <Controls className="bg-card border border-border text-foreground fill-foreground rounded-lg overflow-hidden" />
        <Background color="color-mix(in oklch, var(--muted-foreground) 25%, transparent)" gap={24} size={2} />
      </ReactFlow>
    </div>
  );
}

function buildNode(stage: StageViewModel, index: number, isSelected: boolean) {
  const icon = stageIconMap[stage.id] ?? <XCircle className="h-4 w-4" />;
  const headline = headlineMetric(stage);
  return {
    id: stage.id,
    position: { x: 250, y: index * 110 + 40 },
    data: {
      label: (
        <div className="flex flex-col gap-1.5 p-2 min-w-[190px]">
          <div className="flex items-center gap-2 font-semibold">
            {stage.success ? icon : <XCircle className="h-4 w-4 text-red-500" />}
            <span>{stage.title}</span>
          </div>
          <div className="text-xs text-muted-foreground flex justify-between">
            <span>{stage.durationMs} ms</span>
            {headline && <span className="truncate max-w-[110px]">{headline}</span>}
          </div>
        </div>
      ),
    },
    style: {
      background: "var(--card)",
      color: "var(--card-foreground)",
      border: isSelected ? "1px solid var(--v-accent)" : "1px solid var(--border)",
      borderRadius: "8px",
      padding: "4px",
      boxShadow: isSelected
        ? "0 0 0 2px var(--v-accent-ring)"
        : "0 8px 24px color-mix(in oklch, var(--foreground) 10%, transparent)",
    },
    className: "cursor-pointer",
  };
}

function buildEdge(fromId: string, toId: string) {
  return {
    id: `e-${fromId}-${toId}`,
    source: fromId,
    target: toId,
    animated: true,
    style: { stroke: "var(--v-accent)", strokeWidth: 2 },
    markerEnd: { type: MarkerType.ArrowClosed, color: "var(--v-accent)" },
  };
}
