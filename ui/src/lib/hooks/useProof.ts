"use client";

import useSWR from "swr";
import type { ProofResponse } from "@/types/valori";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<ProofResponse>;
  });

export function useProof() {
  const { data, error, isLoading } = useSWR<ProofResponse>(
    "/api/proof",
    fetcher,
    { refreshInterval: 2000, revalidateOnFocus: true }
  );

  return {
    hash: data?.final_state_hash ?? null,
    isLoading,
    error: error ?? null,
  };
}
