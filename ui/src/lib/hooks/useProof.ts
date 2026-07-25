"use client";

import useSWR from "swr";
import type { ProofResponse } from "@/types/valori";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<ProofResponse>;
  });

// See useHealth.ts for the projectId?: string convention shared across
// these dual-mode hooks — omitted preserves the exact prior local behavior.
export function useProof(projectId?: string) {
  const path = projectId ? `/api/cloud/projects/${projectId}/proof` : "/api/proof";
  const { data, error, isLoading } = useSWR<ProofResponse>(
    path,
    fetcher,
    { refreshInterval: 2000, revalidateOnFocus: true }
  );

  return {
    hash: data?.final_state_hash ?? null,
    isLoading,
    error: error ?? null,
  };
}
