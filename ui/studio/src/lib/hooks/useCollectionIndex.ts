"use client";

import useSWR from "swr";
import { useTransport } from "@/runtime/context";
import type { ProjectRef } from "@/runtime/project";

// Wire model for GET /v1/namespaces/{name}/index
// and POST /v1/namespaces/{name}/index.
export interface IndexStatusResponse {
  collection: string;
  /** "none" | "hnsw" | "ivf" | "bq" */
  active_type: string;
  /** Present when an index generation is active. */
  active_generation?: number;
  /** Present when a different type is desired (e.g. building toward IVF). */
  desired_type?: string;
  /** "none" | "building" | "ready" | "active" | "failed" */
  status: string;
  /** Present while a build is in progress. */
  building_generation?: number;
  /** WAL height the build snapshot was taken at. */
  base_lsn?: number;
  /** Unix seconds when the build started. */
  build_started_at?: number;
  /** Human-readable failure reason (last failed build). */
  error?: string;
}

const POLLING_INTERVAL_MS = 3000; // while building / ready

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<IndexStatusResponse>;
  });

/**
 * Polls GET /v1/namespaces/{namespace}/index for the collection's live index
 * lifecycle state.
 *
 * - Polls at 3 s while status is "building" or "ready".
 * - Stops polling for terminal states ("none", "active", "failed").
 * - Pass `namespace = ""` to suspend the hook entirely (SWR key = null).
 * - Revalidates on window focus so a page return after a navigation always
 *   shows fresh state.
 */
export function useCollectionIndex(projectId: ProjectRef, namespace: string) {
  const transport = useTransport();
  const key = namespace
    ? transport.path(projectId, `/namespaces/${encodeURIComponent(namespace)}/index`)
    : null;

  // SWR v2 accepts refreshInterval as a function (data) => number | false.
  // Return POLLING_INTERVAL_MS for transient states, 0 to stop polling for terminal.
  const { data, error, isLoading, mutate } = useSWR<IndexStatusResponse>(
    key,
    fetcher,
    {
      refreshInterval: (latestData: IndexStatusResponse | undefined) => {
        const s = latestData?.status;
        return s === "building" || s === "ready" ? POLLING_INTERVAL_MS : 0;
      },
      shouldRetryOnError: false,
      revalidateOnFocus: true,
    }
  );

  return {
    data,
    isLoading,
    error: error ?? null,
    mutate,
    isPolling: data?.status === "building" || data?.status === "ready",
  };
}
