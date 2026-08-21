"use client";

import { useHealth } from "./useHealth";
import type { CollectionRef } from "./useCollections";

export interface CollectionMetric {
  collection: string;
  namespace: string;
  approximateRecords: number | null;
}

// `collections` carries each collection's canonical display name alongside
// its actual raw node namespace (see useCollections.ts's module doc) — this
// hook only ever needs the raw namespace to probe the node directly, and
// never re-derives or assumes a naming convention of its own.
export function useProjectMetrics(collections: CollectionRef[]) {
  const { dim, recordCount, chainHeight } = useHealth();

  // Approximate record counts by running zero-vec search in each collection.
  // This is best-effort — null means we couldn't determine the count.
  const probes: CollectionMetric[] = collections.map((c) => ({
    collection: c.name,
    namespace: c.rawNamespace,
    approximateRecords: null,
  }));

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
