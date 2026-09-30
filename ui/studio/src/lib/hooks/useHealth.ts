"use client";

import useSWR from "swr";
import type { HealthResponse } from "@/types/valori";
import { useTransport } from "@/runtime/context";
import type { ProjectRef } from "@/runtime/project";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<HealthResponse>;
  });

export function useHealth(projectId: ProjectRef) {
  const transport = useTransport();
  const { data, error } = useSWR<HealthResponse>(transport.path(projectId, "/health"), fetcher, {
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
    // Phase API-2: `index` was removed. `GET /health` has never carried a
    // node-level index kind (see `EngineHealth` in valori-engine) — the value
    // was always `null` and rendered as "—". Index kind is per-Collection:
    // read it from `GET /v1/namespaces` (`Collection.index`).
    version: data?.version ?? null,
    error: error ?? null,
  };
}
