"use client";

import useSWR from "swr";
import { useTransport } from "@/runtime/context";
import type { ProjectRef } from "@/runtime/project";

// Asks what the runtime thinks each instance's container/process status is —
// distinct from useHealth's own /health, which only reflects whether the
// node process itself is answering.
export interface InstanceStatusEntry {
  instance_id: string;
  host_id: string;
  node_index: number;
  status: string;
}

interface ProjectStatusResponse {
  project_id: string;
  instances: InstanceStatusEntry[];
}

const fetcher = (url: string) => fetch(url).then((r) => r.json() as Promise<ProjectStatusResponse>);

export function useProvisionerStatus(projectId: ProjectRef) {
  const transport = useTransport();
  const { data, error, isLoading, mutate } = useSWR<ProjectStatusResponse>(
    transport.path(projectId, "/status"),
    fetcher,
    { refreshInterval: 10000 }
  );

  return {
    instances: data?.instances ?? [],
    error: error ?? null,
    isLoading,
    refresh: mutate,
  };
}
