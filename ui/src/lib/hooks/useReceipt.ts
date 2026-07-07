"use client";

import useSWR from "swr";

export interface ReceiptData {
  receipt_id: string;
  receipt_hash: { "0": number[] } | string | Record<string, any>;
  operation_hash: string;
  graph_hash: string;
  kernel_abi_version: number;
  planner_fingerprint_hash: string;
  embed_enabled: boolean;
  cluster_mode: boolean;
  shard_count: number;
  state_hash_before: { "0": string } | string | Record<string, any>;
  state_hash_after: { "0": string } | string | Record<string, any>;
  committed_height: number;
  produced_at: number;
}

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<ReceiptData>;
  });

export function useReceipt() {
  const { data, error, isLoading, mutate } = useSWR<ReceiptData>(
    "/api/proof/receipt",
    fetcher,
    { refreshInterval: 3000, revalidateOnFocus: true }
  );

  return {
    receipt: data ?? null,
    isLoading,
    error: error ?? null,
    mutate,
  };
}
