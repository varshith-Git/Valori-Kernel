"use client";

import { useCallback, useEffect, useState } from 'react';
import {
  ReactFlow,
  MiniMap,
  Controls,
  Background,
  useNodesState,
  useEdgesState,
  addEdge,
  MarkerType,
  Handle,
  Position,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Badge } from '@/components/ui/badge';
import { Layers, Activity, Database, CheckCircle, Search, RefreshCw, XCircle, Clock, Server } from 'lucide-react';

// ── Types ─────────────────────────────────────────────────────────────────────

interface TaskInputs {
  info?: string;
  [key: string]: unknown;
}

interface Task {
  id: number;
  kind: string | Record<string, unknown>;
  shard_id?: number | null;
  inputs_json?: string;
  cost_estimate_ms?: number;
  parsedInputs?: TaskInputs;
}

interface ExecutionEdge {
  from: number;
  to: number;
}

interface ExecutionData {
  tasks: Task[];
  edges?: ExecutionEdge[];
}

// ── Node in the selected-detail panel ─────────────────────────────────────────

interface SelectedNode {
  data: {
    task: Task & { parsedInputs: TaskInputs };
  };
}

const taskIconMap: Record<string, React.ReactNode> = {
  embed: <Layers className="h-4 w-4" />,
  insert_record: <Database className="h-4 w-4" />,
  insert_node: <Database className="h-4 w-4" />,
  insert_edge: <Database className="h-4 w-4" />,
  soft_delete_record: <XCircle className="h-4 w-4" />,
  search: <Search className="h-4 w-4" />,
  graph_rag: <Layers className="h-4 w-4" />,
  llm_complete: <Activity className="h-4 w-4" />,
  http_fetch: <RefreshCw className="h-4 w-4" />,
  read_index: <Search className="h-4 w-4" />,
  proof_fragment: <CheckCircle className="h-4 w-4 text-green-500" />
};

export default function ExecutionGraph({ executionData }: { executionData: ExecutionData | null }) {
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);
  const [parseError, setParseError] = useState<string | null>(null);

  useEffect(() => {
    if (!executionData?.tasks) return;

    const initialNodes = executionData.tasks.map((task: Task, index: number) => {
      const x = 250;
      const y = index * 120 + 50;

      const kindString = typeof task.kind === 'string' ? task.kind : Object.keys(task.kind ?? {})[0] ?? 'unknown';
      const icon = taskIconMap[kindString] ?? <Activity className="h-4 w-4" />;

      let parsedInputs: TaskInputs = {};
      if (task.inputs_json) {
        try {
          parsedInputs = JSON.parse(task.inputs_json) as TaskInputs;
          setParseError(null);
        } catch {
          setParseError(`Task #${task.id}: could not parse inputs_json`);
        }
      }

      const taskData = { ...task, parsedInputs };

      return {
        id: task.id?.toString() || `${index}`,
        position: { x, y },
        data: {
          label: (
            <div className="flex flex-col gap-2 p-2 min-w-[200px]">
              <div className="flex items-center gap-2 font-semibold">
                {icon}
                <span className="capitalize">{kindString.replace(/_/g, ' ')}</span>
              </div>
              <div className="text-xs text-muted-foreground flex justify-between">
                <span>Task #{task.id}</span>
                {task.shard_id !== null && task.shard_id !== undefined && (
                  <Badge variant="outline" className="bg-primary/20 text-primary border-primary/30 shadow-[0_0_8px_var(--v-accent-ring)]">Shard {task.shard_id}</Badge>
                )}
              </div>
              {parsedInputs.info && (
                <div className="text-[10px] bg-muted p-1.5 rounded-md border border-border truncate text-muted-foreground">
                  {parsedInputs.info}
                </div>
              )}
            </div>
          ),
          task: taskData,
        },
        style: {
          background: 'var(--card)',
          color: 'var(--card-foreground)',
          border: '1px solid var(--border)',
          borderRadius: '8px',
          padding: '4px',
          boxShadow: '0 8px 24px color-mix(in oklch, var(--foreground) 10%, transparent)',
          transition: 'all 0.2s cubic-bezier(0.4, 0, 0.2, 1)',
        },
        className: "hover:scale-[1.02] cursor-pointer",
      };
    });

    const initialEdges = (executionData.edges ?? []).map((edge: ExecutionEdge) => ({
      id: `e${edge.from}-${edge.to}`,
      source: edge.from.toString(),
      target: edge.to.toString(),
      animated: true,
      style: { stroke: 'var(--v-accent)', strokeWidth: 2 },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: 'var(--v-accent)',
      },
    }));

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    setNodes(initialNodes as any);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    setEdges(initialEdges as any);
  }, [executionData, setNodes, setEdges]);

  const [selectedNode, setSelectedNode] = useState<SelectedNode | null>(null);

  const onNodeClick = useCallback((_: React.MouseEvent, node: SelectedNode) => {
    setSelectedNode(node);
  }, []);

  return (
    <div className="flex flex-col gap-2">
      {parseError && (
        <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-400">
          {parseError}
        </div>
      )}
    <div className="relative w-full h-[600px] rounded-lg border border-border bg-background overflow-hidden">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeClick={onNodeClick}
        onPaneClick={() => setSelectedNode(null)}
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

      {selectedNode && (
        <div className="absolute top-4 right-4 w-80 max-h-[calc(100%-32px)] bg-card border border-border rounded-lg shadow-xl flex flex-col animate-in slide-in-from-right-8 duration-300">
          <div className="p-4 border-b border-border flex items-center justify-between">
            <h3 className="font-semibold flex items-center gap-2">
              <Activity className="h-4 w-4 text-primary" />
              Task Details
            </h3>
            <button type="button" onClick={() => setSelectedNode(null)} className="text-muted-foreground hover:text-foreground transition-colors" aria-label="Close task details">
              <XCircle className="h-5 w-5" />
            </button>
          </div>
          <div className="flex-1 p-4 overflow-y-auto custom-scrollbar">
            <div className="space-y-4">
              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold">Kind</div>
                <div className="font-mono text-sm bg-muted p-2 rounded-md border border-border">
                  {typeof selectedNode.data.task.kind === 'string' ? selectedNode.data.task.kind : Object.keys(selectedNode.data.task.kind || {})[0]}
                </div>
              </div>
              
              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Clock className="h-3 w-3" /> Latency (ms)</div>
                <div className="font-mono text-sm bg-muted p-2 rounded-md border border-border text-emerald-500">
                  {selectedNode.data.task.cost_estimate_ms || "< 1"} ms
                </div>
              </div>
              
              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Server className="h-3 w-3" /> Node Assignment</div>
                <div className="font-mono text-sm bg-muted p-2 rounded-md border border-border">
                  {selectedNode.data.task.shard_id !== null ? `Shard ${selectedNode.data.task.shard_id}` : "Coordinator"}
                </div>
              </div>

              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold">Inputs JSON</div>
                <pre className="font-mono text-[10px] bg-muted p-3 rounded-md border border-border overflow-x-auto text-foreground">
                  {JSON.stringify(selectedNode.data.task.parsedInputs, null, 2)}
                </pre>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
    </div>
  );
}
