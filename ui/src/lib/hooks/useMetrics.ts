"use client";

import useSWR from "swr";
import { useHealth } from "./useHealth";
import { useProjectGroups, makeNs } from "./useCollections";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json();
  });

export interface CollectionMetric {
  collection: string;
  namespace: string;
  approximateRecords: number | null;
}

export function useProjectMetrics(project: string, collections: string[]) {
  const { dim, recordCount, chainHeight } = useHealth();

  // Approximate record counts by running zero-vec search in each collection.
  // This is best-effort — null means we couldn't determine the count.
  const probes = collections.map((col) => {
    const ns = makeNs(project, col);
    return { collection: col, namespace: ns };
  });

  // We do a single aggregated approximate check via health for total,
  // then per-collection via zero-vec search (triggered lazily).
  const totalStorageBytes =
    recordCount != null && dim != null
      ? recordCount * dim * 4 // Q16.16 = 4 bytes per scalar
      : null;

  const totalStorageMB =
    totalStorageBytes != null ? totalStorageBytes / (1024 * 1024) : null;

  return {
    collectionCount: collections.length,
    totalRecords: recordCount,
    chainHeight,
    dim,
    totalStorageMB,
    probes,
  };
}
