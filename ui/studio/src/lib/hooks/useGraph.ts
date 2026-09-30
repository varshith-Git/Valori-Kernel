"use client";

import useSWR from "swr";
import { useTransport } from "@/runtime/context";
import type { ProjectRef } from "@/runtime/project";

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

export function useGraph(projectId: ProjectRef, namespace: string) {
  const transport = useTransport();
  const { data, error, isLoading, mutate } = useSWR<{ nodes: GraphNode[]; count: number }>(
    transport.path(projectId, `/graph/nodes?collection=${encodeURIComponent(namespace)}`),
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

export function useNodeEdges(projectId: ProjectRef, nodeId: number | null) {
  const transport = useTransport();
  const { data, isLoading } = useSWR<{ edges: GraphEdge[] }>(
    nodeId === null ? null : transport.path(projectId, `/graph/edges/${nodeId}`),
    fetcher
  );
  return { edges: data?.edges ?? [], isLoading };
}
