"use client";

import useSWR from "swr";
import type { HealthResponse } from "@/types/valori";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<HealthResponse>;
  });

// `projectId` is optional — omitted (every local call site, unchanged) polls
// the single globally-connected daemon at /api/health. Passed (the cloud
// workspace, /cloud/projects/[id]/*) polls that specific project's node
// through the cloud-mode proxy instead, since cloud has many projects with
// no single "current" connection the way local does.
export function useHealth(projectId?: string) {
  const path = projectId ? `/api/cloud/projects/${projectId}/health` : "/api/health";
  const { data, error } = useSWR<HealthResponse>(path, fetcher, {
    refreshInterval: 5000,
    shouldRetryOnError: true,
    errorRetryCount: 3,
  });

  return {
    status: data?.status ?? null,
    online: !error && !!data,
    recordCount: data?.records?.live ?? null,
    chainHeight: data?.event_log_height ?? null,
    dim: data?.dim ?? null,
    fillPct: data?.records?.fill_pct ?? null,
    capacity: data?.records?.capacity ?? null,
    index: data?.index ?? null,
    version: data?.version ?? null,
    error: error ?? null,
  };
}
