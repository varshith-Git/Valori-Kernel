"use client";

import useSWR from "swr";
import type { HealthResponse } from "@/types/valori";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<HealthResponse>;
  });

export function useHealth() {
  const { data, error } = useSWR<HealthResponse>("/api/health", fetcher, {
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
