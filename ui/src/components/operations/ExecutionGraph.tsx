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
import { Card } from '@/components/ui/card';
import { Layers, Activity, Database, CheckCircle, Search, RefreshCw, XCircle, Info, Clock, Cpu, Server } from 'lucide-react';

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

export default function ExecutionGraph({ executionData }: { executionData: any }) {
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);

  useEffect(() => {
    if (!executionData || !executionData.tasks) return;

    const initialNodes = executionData.tasks.map((task: any, index: number) => {
      // Basic top-down layout (in a real app, use dagre for complex DAG layouts)
      const x = 250;
      const y = index * 120 + 50;

      const kindString = typeof task.kind === 'string' ? task.kind : Object.keys(task.kind || {})[0] || 'unknown';
      const icon = taskIconMap[kindString] || <Activity className="h-4 w-4" />;
      
      let parsedInputs: any = {};
      try {
        if (task.inputs_json) parsedInputs = JSON.parse(task.inputs_json);
      } catch (e) {}

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
                  <Badge variant="outline" className="bg-primary/20 text-primary border-primary/30 shadow-[0_0_8px_rgba(var(--primary-rgb),0.3)]">Shard {task.shard_id}</Badge>
                )}
              </div>
              {parsedInputs.info && (
                <div className="text-[10px] bg-black/40 p-1.5 rounded-md border border-white/10 truncate text-muted-foreground">
                  {parsedInputs.info}
                </div>
              )}
            </div>
          ),
          task: taskData,
        },
        style: {
          background: 'rgba(15, 20, 30, 0.6)',
          backdropFilter: 'blur(16px)',
          color: 'var(--card-foreground)',
          border: '1px solid rgba(255, 255, 255, 0.1)',
          borderRadius: '12px',
          padding: '4px',
          boxShadow: '0 8px 32px 0 rgba(0, 0, 0, 0.5), inset 0 1px 1px rgba(255, 255, 255, 0.1)',
          transition: 'all 0.2s cubic-bezier(0.4, 0, 0.2, 1)',
        },
        className: "hover:scale-[1.02] hover:shadow-[0_0_20px_rgba(56,189,248,0.3)] cursor-pointer",
      };
    });

    const initialEdges = (executionData.edges || []).map((edge: any, i: number) => ({
      id: `e${edge.from}-${edge.to}`,
      source: edge.from.toString(),
      target: edge.to.toString(),
      animated: true,
      style: { stroke: 'hsl(var(--primary))', strokeWidth: 2, filter: 'drop-shadow(0 0 4px hsl(var(--primary)/0.5))' },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: 'hsl(var(--primary))',
      },
    }));

    setNodes(initialNodes);
    setEdges(initialEdges);
  }, [executionData, setNodes, setEdges]);

  const [selectedNode, setSelectedNode] = useState<any>(null);

  const onNodeClick = useCallback((_: React.MouseEvent, node: any) => {
    setSelectedNode(node);
  }, []);

  return (
    <div className="relative w-full h-[600px] rounded-xl border border-white/5 bg-black/40 overflow-hidden shadow-2xl">
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
          nodeColor="hsl(var(--primary))" 
          maskColor="rgba(0,0,0,0.6)"
          className="border border-white/10 rounded-lg shadow-xl bg-black/80 backdrop-blur-md" 
        />
        <Controls className="bg-black/60 backdrop-blur border border-white/10 text-white fill-white rounded-lg shadow-xl overflow-hidden" />
        <Background color="hsl(var(--muted-foreground)/0.2)" gap={24} size={2} />
      </ReactFlow>

      {/* Glassmorphism Side Panel */}
      {selectedNode && (
        <div className="absolute top-4 right-4 w-80 max-h-[calc(100%-32px)] bg-black/60 backdrop-blur-xl border border-white/10 rounded-2xl shadow-2xl flex flex-col animate-in slide-in-from-right-8 duration-300">
          <div className="p-4 border-b border-white/10 flex items-center justify-between">
            <h3 className="font-semibold flex items-center gap-2">
              <Activity className="h-4 w-4 text-primary" />
              Task Details
            </h3>
            <button onClick={() => setSelectedNode(null)} className="text-muted-foreground hover:text-white transition-colors">
              <XCircle className="h-5 w-5" />
            </button>
          </div>
          <div className="flex-1 p-4 overflow-y-auto custom-scrollbar">
            <div className="space-y-4">
              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold">Kind</div>
                <div className="font-mono text-sm bg-white/5 p-2 rounded-lg border border-white/5">
                  {typeof selectedNode.data.task.kind === 'string' ? selectedNode.data.task.kind : Object.keys(selectedNode.data.task.kind || {})[0]}
                </div>
              </div>
              
              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Clock className="h-3 w-3" /> Latency (ms)</div>
                <div className="font-mono text-sm bg-white/5 p-2 rounded-lg border border-white/5 text-emerald-400">
                  {selectedNode.data.task.cost_estimate_ms || "< 1"} ms
                </div>
              </div>
              
              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Server className="h-3 w-3" /> Node Assignment</div>
                <div className="font-mono text-sm bg-white/5 p-2 rounded-lg border border-white/5">
                  {selectedNode.data.task.shard_id !== null ? `Shard ${selectedNode.data.task.shard_id}` : "Coordinator"}
                </div>
              </div>

              <div>
                <div className="text-xs text-muted-foreground mb-1 uppercase tracking-wider font-semibold">Inputs JSON</div>
                <pre className="font-mono text-[10px] bg-black/50 p-3 rounded-lg border border-white/5 overflow-x-auto text-primary/80">
                  {JSON.stringify(selectedNode.data.task.parsedInputs, null, 2)}
                </pre>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
