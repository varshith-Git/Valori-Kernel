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

// `projectId` optional and trailing (unlike valori-ui's version, which has
// it first) so every existing local call site (namespace only) is
// unaffected — see useHealth.ts for the same convention.
export function useGraph(namespace: string, projectId?: string) {
  const path = projectId
    ? `/api/cloud/projects/${projectId}/graph/nodes?collection=${encodeURIComponent(namespace)}`
    : `/api/graph/nodes?collection=${encodeURIComponent(namespace)}`;
  const { data, error, isLoading, mutate } = useSWR<{ nodes: GraphNode[]; count: number }>(
    path,
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

export function useNodeEdges(nodeId: number | null, projectId?: string) {
  const path = nodeId === null
    ? null
    : projectId
      ? `/api/cloud/projects/${projectId}/graph/edges/${nodeId}`
      : `/api/graph/edges/${nodeId}`;
  const { data, isLoading } = useSWR<{ edges: GraphEdge[] }>(path, fetcher);
  return { edges: data?.edges ?? [], isLoading };
}
