"use client";

import useSWR from "swr";

export interface GraphNode {
  node_id: number;
  kind: number; // 0 = Document, 1 = Chunk
  record_id: number | null;
  namespace_id: number;
}

export interface GraphEdge {
  edge_id: number;
  to_node: number;
  kind: number;
}

export interface DocumentTree {
  docNode: GraphNode;
  chunks: GraphNode[];
}

const fetcher = (url: string) => fetch(url).then((r) => r.json());

export function useGraph(namespace: string) {
  const { data, error, isLoading, mutate } = useSWR<{ nodes: GraphNode[]; count: number }>(
    `/api/graph/nodes?collection=${encodeURIComponent(namespace)}`,
    fetcher,
    { refreshInterval: 10_000 }
  );

  const nodes = data?.nodes ?? [];
  const docNodes = nodes.filter((n) => n.kind === 0);
  const chunkNodes = nodes.filter((n) => n.kind === 1);

  return {
    nodes,
    docNodes,
    chunkNodes,
    totalNodes: data?.count ?? 0,
    isLoading,
    error,
    mutate,
  };
}

export function useNodeEdges(nodeId: number | null) {
  const { data, isLoading } = useSWR<{ edges: GraphEdge[] }>(
    nodeId !== null ? `/api/graph/edges/${nodeId}` : null,
    fetcher
  );
  return { edges: data?.edges ?? [], isLoading };
}
