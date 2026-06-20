"use client";

import useSWR from "swr";

export interface MemberView {
  id: number;
  raft_addr: string;
  api_addr: string;
  voter: boolean;
}

export interface ClusterStatusResponse {
  standalone?: boolean;
  node_id?: number;
  current_leader?: number | null;
  is_leader?: boolean;
  term?: number;
  last_log_index?: number | null;
  last_applied_index?: number | null;
  members?: MemberView[];
}

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<ClusterStatusResponse>;
  });

export function useCluster() {
  const { data, error, isLoading, mutate } = useSWR<ClusterStatusResponse>(
    "/api/cluster",
    fetcher,
    { refreshInterval: 5000 }
  );

  const isStandalone = data?.standalone === true;
  const members = data?.members ?? [];
  const leaderId = data?.current_leader ?? null;

  // All members have same last_applied_index means converged. We approximate
  // by checking if last_log_index === last_applied_index on this node.
  const converged =
    data?.last_log_index != null &&
    data?.last_applied_index != null &&
    data.last_log_index === data.last_applied_index;

  return {
    status: data ?? null,
    members,
    leaderId,
    nodeId: data?.node_id ?? null,
    isLeader: data?.is_leader ?? false,
    term: data?.term ?? null,
    lastLogIndex: data?.last_log_index ?? null,
    lastAppliedIndex: data?.last_applied_index ?? null,
    converged,
    isStandalone,
    isLoading,
    error: error ?? null,
    refresh: mutate,
  };
}
