"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { useGraph, useNodeEdges, GraphNode, GraphEdge } from "@/lib/hooks/useGraph";

// -- Types ---------------------------------------------------------------------

interface TreeDoc {
  docNode: GraphNode;
  chunkIds: number[];
  chunkMap: Record<number, GraphNode>;
}

// -- Tree row components -------------------------------------------------------

function ChunkRow({
  chunk,
  selected,
  onClick,
}: {
  chunk: GraphNode;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-[12px] transition-colors ${
        selected
          ? "bg-muted text-foreground"
          : "text-muted-foreground hover:bg-accent hover:text-card-foreground"
      }`}
    >
      <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-violet-500 opacity-70" />
      <span className="font-mono text-[10px] text-muted-foreground w-16 flex-shrink-0">
        chunk #{chunk.node_id}
      </span>
      {chunk.record_id != null ? (
        <span className="text-muted-foreground">
          rec <span className="text-accent-foreground">{chunk.record_id}</span>
        </span>
      ) : (
        <span className="text-muted-foreground italic">no record</span>
      )}
    </button>
  );
}

function DocRow({
  docNode,
  chunks,
  selected,
  expanded,
  onSelect,
  onToggle,
}: {
  docNode: GraphNode;
  chunks: GraphNode[];
  selected: boolean;
  expanded: boolean;
  onSelect: () => void;
  onToggle: () => void;
}) {
  return (
    <div>
      <button
        onClick={onToggle}
        className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-[13px] transition-colors ${
          selected
            ? "bg-muted text-foreground"
            : "text-accent-foreground hover:bg-accent"
        }`}
      >
        <span
          className={`text-[10px] font-mono transition-transform inline-block ${expanded ? "rotate-90" : ""} text-muted-foreground`}
        >
          ▶
        </span>
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden className="flex-shrink-0">
          <rect x="1" y="2" width="12" height="10" rx="2" fill="#1e40af" stroke="#3b82f6" strokeWidth="0.75" />
          <line x1="3.5" y1="5" x2="10.5" y2="5" stroke="#93c5fd" strokeWidth="0.75" />
          <line x1="3.5" y1="7" x2="10.5" y2="7" stroke="#93c5fd" strokeWidth="0.75" />
          <line x1="3.5" y1="9" x2="8" y2="9" stroke="#93c5fd" strokeWidth="0.75" />
        </svg>
        <span className="flex-1 truncate">
          {docNode.record_id != null ? `Document · rec ${docNode.record_id}` : `Document · node ${docNode.node_id}`}
        </span>
        <span className="ml-auto text-[10px] font-mono text-muted-foreground tabular-nums flex-shrink-0">
          {chunks.length} chunk{chunks.length !== 1 ? "s" : ""}
        </span>
        <button
          onClick={(e) => {
            e.stopPropagation();
            onSelect();
          }}
          className="ml-1 text-[10px] text-muted-foreground hover:text-accent-foreground transition-colors"
        >
          info
        </button>
      </button>

      {expanded && chunks.length > 0 && (
        <div className="ml-6 mt-0.5 flex flex-col gap-0.5 border-l border-border pl-2">
          {chunks.map((c) => (
            <ChunkRow key={c.node_id} chunk={c} selected={false} onClick={() => {}} />
          ))}
        </div>
      )}

      {expanded && chunks.length === 0 && (
        <p className="ml-10 mt-1 text-[11px] text-muted-foreground italic">no chunks</p>
      )}
    </div>
  );
}

// -- Detail panel --------------------------------------------------------------

function NodeDetail({ node, edges }: { node: GraphNode; edges: GraphEdge[] }) {
  const kindLabel = node.kind === 0 ? "Document" : node.kind === 1 ? "Chunk" : `Kind ${node.kind}`;
  return (
    <div className="rounded-xl border border-border bg-card divide-y divide-border text-sm">
      <div className="flex items-center justify-between px-4 py-3">
        <span className="text-muted-foreground">Type</span>
        <span className={`font-medium ${node.kind === 0 ? "text-blue-400" : "text-violet-400"}`}>
          {kindLabel}
        </span>
      </div>
      <div className="flex items-center justify-between px-4 py-3">
        <span className="text-muted-foreground">Node ID</span>
        <span className="font-mono text-accent-foreground">{node.node_id}</span>
      </div>
      {node.record_id != null && (
        <div className="flex items-center justify-between px-4 py-3">
          <span className="text-muted-foreground">Record ID</span>
          <span className="font-mono text-accent-foreground">{node.record_id}</span>
        </div>
      )}
      <div className="flex items-center justify-between px-4 py-3">
        <span className="text-muted-foreground">Namespace</span>
        <span className="font-mono text-accent-foreground">{node.namespace_id}</span>
      </div>
      {edges.length > 0 && (
        <div className="px-4 py-3">
          <p className="text-muted-foreground mb-2">Outgoing edges ({edges.length})</p>
          <div className="flex flex-col gap-1">
            {edges.map((e) => (
              <div key={e.edge_id} className="flex items-center gap-2 text-xs font-mono">
                <span className="text-muted-foreground">edge {e.edge_id}</span>
                <span className="text-muted-foreground">→</span>
                <span className="text-accent-foreground">node {e.to_node}</span>
                <span className="ml-auto text-muted-foreground">kind {e.kind}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// -- SVG canvas ----------------------------------------------------------------

interface CanvasNode {
  id: number;
  kind: number;
  record_id: number | null;
  x: number;
  y: number;
}

interface CanvasEdge {
  from: number;
  to: number;
}

function GraphCanvas({
  namespace,
  nodes,
  edgesMap,
}: {
  namespace: string;
  nodes: GraphNode[];
  edgesMap: Record<number, GraphEdge[]>;
}) {
  const [selected, setSelected] = useState<number | null>(null);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [scale, setScale] = useState(1);
  const dragging = useRef(false);
  const lastPos = useRef({ x: 0, y: 0 });
  const svgRef = useRef<SVGSVGElement>(null);

  const canvasNodes: CanvasNode[] = [];
  const canvasEdges: CanvasEdge[] = [];

  const docNodes = nodes.filter((n) => n.kind === 0);
  const chunkByParent: Record<number, GraphNode[]> = {};

  for (const d of docNodes) {
    const edges = edgesMap[d.node_id] ?? [];
    const children: GraphNode[] = [];
    for (const e of edges) {
      const target = nodes.find((n) => n.node_id === e.to_node);
      if (target) {
        children.push(target);
        canvasEdges.push({ from: d.node_id, to: target.node_id });
      }
    }
    chunkByParent[d.node_id] = children;
  }

  // Layout: docs evenly on left col (x=80), chunks fan right (x=280)
  const DOC_X = 80;
  const CHUNK_X = 280;
  const ROW_H = 60;
  let yOffset = 40;

  for (const d of docNodes) {
    const chunks = chunkByParent[d.node_id] ?? [];
    const blockH = Math.max(ROW_H, chunks.length * ROW_H);
    const docY = yOffset + blockH / 2;
    canvasNodes.push({ id: d.node_id, kind: 0, record_id: d.record_id, x: DOC_X, y: docY });
    chunks.forEach((c, i) => {
      const chunkY = yOffset + i * ROW_H + ROW_H / 2;
      canvasNodes.push({ id: c.node_id, kind: 1, record_id: c.record_id, x: CHUNK_X, y: chunkY });
    });
    yOffset += blockH + 20;
  }

  // Nodes that are neither doc nor have a doc parent
  const placedIds = new Set(canvasNodes.map((n) => n.id));
  for (const n of nodes) {
    if (!placedIds.has(n.node_id)) {
      canvasNodes.push({ id: n.node_id, kind: n.kind, record_id: n.record_id, x: 440, y: yOffset });
      yOffset += ROW_H;
    }
  }

  const totalH = Math.max(yOffset + 40, 200);
  const posMap = Object.fromEntries(canvasNodes.map((n) => [n.id, { x: n.x, y: n.y }]));

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.1 : 0.9;
    setScale((s) => Math.max(0.3, Math.min(3, s * factor)));
  };

  const onMouseDown = (e: React.MouseEvent) => {
    dragging.current = true;
    lastPos.current = { x: e.clientX, y: e.clientY };
  };

  const onMouseMove = (e: React.MouseEvent) => {
    if (!dragging.current) return;
    const dx = e.clientX - lastPos.current.x;
    const dy = e.clientY - lastPos.current.y;
    setPan((p) => ({ x: p.x + dx, y: p.y + dy }));
    lastPos.current = { x: e.clientX, y: e.clientY };
  };

  const onMouseUp = () => { dragging.current = false; };

  return (
    <div className="relative rounded-xl border border-border bg-background overflow-hidden" style={{ height: 420 }}>
      <svg
        ref={svgRef}
        className="w-full h-full cursor-grab active:cursor-grabbing select-none"
        onWheel={onWheel}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseUp}
      >
        <defs>
          <marker id="cg-arr" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto">
            <path d="M0,0 L0,7 L7,3.5 z" fill="#52525b" />
          </marker>
        </defs>
        <g transform={`translate(${pan.x},${pan.y}) scale(${scale})`}>
          {/* Edges */}
          {canvasEdges.map((e) => {
            const from = posMap[e.from];
            const to = posMap[e.to];
            if (!from || !to) return null;
            const mx = (from.x + to.x) / 2;
            return (
              <path
                key={`${e.from}-${e.to}`}
                d={`M${from.x + 22},${from.y} C${mx},${from.y} ${mx},${to.y} ${to.x - 14},${to.y}`}
                fill="none"
                stroke="#3f3f46"
                strokeWidth="1.5"
                markerEnd="url(#cg-arr)"
              />
            );
          })}
          {/* Nodes */}
          {canvasNodes.map((n) => {
            const isSelected = selected === n.id;
            if (n.kind === 0) {
              return (
                <g key={n.id} onClick={() => setSelected(isSelected ? null : n.id)} className="cursor-pointer">
                  <rect
                    x={n.x - 22}
                    y={n.y - 14}
                    width="44"
                    height="28"
                    rx="5"
                    fill={isSelected ? "#1e3a8a" : "#1e293b"}
                    stroke={isSelected ? "#3b82f6" : "#334155"}
                    strokeWidth="1.5"
                  />
                  <text x={n.x} y={n.y - 2} textAnchor="middle" fontSize="8" fill="#93c5fd" fontFamily="monospace">
                    doc
                  </text>
                  <text x={n.x} y={n.y + 8} textAnchor="middle" fontSize="7" fill="#60a5fa" fontFamily="monospace">
                    #{n.id}
                  </text>
                </g>
              );
            }
            return (
              <g key={n.id} onClick={() => setSelected(isSelected ? null : n.id)} className="cursor-pointer">
                <circle
                  cx={n.x}
                  cy={n.y}
                  r="14"
                  fill={isSelected ? "#3b1f6e" : "#1e1b4b"}
                  stroke={isSelected ? "#a78bfa" : "#4338ca"}
                  strokeWidth="1.5"
                />
                <text x={n.x} y={n.y - 2} textAnchor="middle" fontSize="7" fill="#c4b5fd" fontFamily="monospace">
                  chunk
                </text>
                <text x={n.x} y={n.y + 7} textAnchor="middle" fontSize="7" fill="#a78bfa" fontFamily="monospace">
                  #{n.record_id ?? n.id}
                </text>
              </g>
            );
          })}
        </g>
      </svg>
      <div className="absolute bottom-3 right-3 flex gap-1.5">
        <button
          onClick={() => { setPan({ x: 0, y: 0 }); setScale(1); }}
          className="rounded border border-input bg-card px-2 py-1 text-[10px] text-muted-foreground hover:text-card-foreground transition-colors"
        >
          reset
        </button>
        <span className="rounded border border-border bg-card px-2 py-1 text-[10px] text-muted-foreground tabular-nums">
          {Math.round(scale * 100)}%
        </span>
      </div>
      <div className="absolute top-3 left-3 flex items-center gap-3 text-[10px] text-muted-foreground">
        <span className="flex items-center gap-1">
          <svg width="10" height="10"><rect x="1" y="2" width="8" height="6" rx="1.5" fill="#1e293b" stroke="#334155" strokeWidth="1"/></svg>
          Document
        </span>
        <span className="flex items-center gap-1">
          <svg width="10" height="10"><circle cx="5" cy="5" r="4" fill="#1e1b4b" stroke="#4338ca" strokeWidth="1"/></svg>
          Chunk
        </span>
      </div>
      {canvasNodes.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center text-muted-foreground text-sm">
          no graph nodes in this collection
        </div>
      )}
    </div>
  );
}

// -- Main GraphTab -------------------------------------------------------------

export function GraphTab({ namespace }: { namespace: string }) {
  const { nodes, docNodes, chunkNodes, isLoading } = useGraph(namespace);
  const [expandedDocs, setExpandedDocs] = useState<Set<number>>(new Set());
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [view, setView] = useState<"tree" | "canvas">("tree");

  // For canvas we eagerly load all doc-node edges
  const [edgesMap, setEdgesMap] = useState<Record<number, GraphEdge[]>>({});

  useEffect(() => {
    if (view !== "canvas") return;
    let cancelled = false;
    async function loadEdges() {
      const result: Record<number, GraphEdge[]> = {};
      for (const d of docNodes) {
        const r = await fetch(`/api/graph/edges/${d.node_id}`);
        if (cancelled) return;
        const data = await r.json().catch(() => ({ edges: [] }));
        result[d.node_id] = data.edges ?? [];
      }
      if (!cancelled) setEdgesMap(result);
    }
    loadEdges();
    return () => { cancelled = true; };
  }, [view, docNodes.map((d) => d.node_id).join(",")]);

  const { edges: selectedEdges } = useNodeEdges(selectedNode?.node_id ?? null);

  const toggleDoc = (id: number) => {
    setExpandedDocs((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  if (isLoading) {
    return (
      <div className="flex flex-col gap-2 animate-pulse">
        {[1, 2, 3].map((i) => (
          <div key={i} className="h-10 rounded-lg bg-accent" />
        ))}
      </div>
    );
  }

  const docChunkMap = Object.fromEntries(
    docNodes.map((d) => [d.node_id, nodes.filter((n) => n.kind === 1 && n.namespace_id === d.namespace_id)])
  );

  return (
    <div className="flex flex-col gap-4">
      {/* Header bar */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span className="text-sm text-muted-foreground">
            <span className="text-foreground font-medium">{nodes.length}</span> nodes
            <span className="mx-1.5 text-zinc-700">·</span>
            <span className="text-blue-400">{docNodes.length}</span> docs
            <span className="mx-1.5 text-zinc-700">·</span>
            <span className="text-violet-400">{chunkNodes.length}</span> chunks
          </span>
        </div>
        <div className="flex rounded-lg border border-border overflow-hidden text-[12px]">
          <button
            onClick={() => setView("tree")}
            className={`px-3 py-1.5 transition-colors ${view === "tree" ? "bg-muted text-foreground" : "bg-card text-muted-foreground hover:text-accent-foreground"}`}
          >
            Tree
          </button>
          <button
            onClick={() => setView("canvas")}
            className={`px-3 py-1.5 border-l border-border transition-colors ${view === "canvas" ? "bg-muted text-foreground" : "bg-card text-muted-foreground hover:text-accent-foreground"}`}
          >
            Canvas
          </button>
        </div>
      </div>

      {nodes.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border py-16 text-center">
          <p className="text-sm text-muted-foreground">No graph nodes in this collection.</p>
          <p className="mt-1 text-xs text-muted-foreground">
            Use the Upload tab to ingest documents — each chunk creates a node here.
          </p>
        </div>
      ) : view === "tree" ? (
        <div className="flex gap-4">
          {/* Tree */}
          <div className="flex-1 flex flex-col gap-1 min-w-0">
            {docNodes.length === 0 ? (
              <div className="rounded-lg border border-border bg-card p-4 text-xs text-muted-foreground">
                No document nodes found. Chunk nodes present: {chunkNodes.length}
              </div>
            ) : (
              docNodes.map((d) => {
                const chunks = (docChunkMap[d.node_id] ?? []);
                return (
                  <DocRow
                    key={d.node_id}
                    docNode={d}
                    chunks={chunks}
                    selected={selectedNode?.node_id === d.node_id}
                    expanded={expandedDocs.has(d.node_id)}
                    onSelect={() => setSelectedNode(selectedNode?.node_id === d.node_id ? null : d)}
                    onToggle={() => toggleDoc(d.node_id)}
                  />
                );
              })
            )}
            {/* Orphan chunks not parented to any doc */}
            {chunkNodes.filter((c) =>
              !Object.values(docChunkMap).flat().some((x) => x.node_id === c.node_id)
            ).length > 0 && (
              <div className="mt-3 rounded-lg border border-border bg-card/40 p-3">
                <p className="text-[10px] text-muted-foreground uppercase tracking-widest mb-2">Unparented chunks</p>
                {chunkNodes
                  .filter((c) =>
                    !Object.values(docChunkMap).flat().some((x) => x.node_id === c.node_id)
                  )
                  .map((c) => (
                    <ChunkRow
                      key={c.node_id}
                      chunk={c}
                      selected={selectedNode?.node_id === c.node_id}
                      onClick={() => setSelectedNode(selectedNode?.node_id === c.node_id ? null : c)}
                    />
                  ))}
              </div>
            )}
          </div>

          {/* Detail panel */}
          {selectedNode && (
            <div className="w-64 flex-shrink-0">
              <div className="flex items-center justify-between mb-2">
                <p className="text-xs text-muted-foreground uppercase tracking-widest">Node detail</p>
                <button
                  onClick={() => setSelectedNode(null)}
                  className="text-xs text-muted-foreground hover:text-accent-foreground"
                >
                  ✕
                </button>
              </div>
              <NodeDetail node={selectedNode} edges={selectedEdges} />
            </div>
          )}
        </div>
      ) : (
        <GraphCanvas namespace={namespace} nodes={nodes} edgesMap={edgesMap} />
      )}
    </div>
  );
}
