"use client";

import useSWR from "swr";

// ── Collection model ─────────────────────────────────────────────────────────
//
// Product model is `Project → Collection`: each project — a local daemon's
// dedicated per-project node, or a Cloud-provisioned node — already owns its
// own node process. A new collection's raw node namespace is just its bare
// name ("docs"); there is nothing left to disambiguate two projects' "docs"
// from each other, since they live on different nodes entirely (see
// docs/architecture — the Desktop project/daemon model investigation).
//
// Namespaces created before this reconciliation are still stored on disk as
// "${projectId}--${collection}" (e.g. "Demo--docs") — a leftover of an older
// architecture where one shared node hosted many "projects" as prefixed
// namespaces. That architecture no longer exists (valori-daemon gives every
// project its own dedicated node/port today), so the prefix was never
// required by the daemon or valori-node — it was always a pure UI-layer
// convention. This file NEVER renames or deletes an existing raw namespace;
// `toCanonicalName` only affects how one is *displayed*, and
// `rawNamespace`/`resolveRawNamespace` recover the real stored name whenever
// a mutation (delete, or addressing a specific collection's data) needs it.
//
// The prefix check is a harmless no-op for Cloud: a Cloud namespace is never
// prefixed with the project's own UUID, so `isLegacyPrefixed` is always false
// there and every namespace round-trips as itself, unchanged.
const LEGACY_SEP = "--";

function isLegacyPrefixed(projectId: string, ns: string): boolean {
  return ns.startsWith(`${projectId}${LEGACY_SEP}`);
}

/** Raw node namespace -> canonical collection display name. */
export function toCanonicalName(projectId: string, ns: string): string {
  return isLegacyPrefixed(projectId, ns) ? ns.slice(projectId.length + LEGACY_SEP.length) : ns;
}

export interface CollectionRef {
  name: string;         // canonical display name
  rawNamespace: string; // actual node namespace — needed only for mutations
}

function toRefs(projectId: string, namespaces: string[]): CollectionRef[] {
  return namespaces.map((ns) => ({ name: toCanonicalName(projectId, ns), rawNamespace: ns }));
}

/** Resolves a canonical collection name to its actual raw node namespace,
 *  given the current full list — for call sites (the collection detail
 *  route, metrics probes) that must address one specific collection's data
 *  without re-implementing the legacy-prefix lookup themselves. Falls back
 *  to the bare name when no match is found yet (e.g. right after creating a
 *  brand-new collection, before the list has revalidated) — the correct
 *  default, since new collections are always stored bare. */
export function resolveRawNamespace(projectId: string, canonicalName: string, namespaces: string[]): string {
  const match = toRefs(projectId, namespaces).find((r) => r.name === canonicalName);
  return match?.rawNamespace ?? canonicalName;
}

export interface CollectionMeta {
  name: string;
  rawNamespace: string;
  id?: number;
  dimension?: number | null;
  metric?: string | null;
  index?: string | null;
  recordCount?: number;
  maxRecords?: number;
}

interface CollectionInfo {
  name: string;
  id: number;
  rawNamespace?: string;
  dimension?: number | null;
  metric?: string | null;
  index?: string | null;
  record_count?: number;
  recordCount?: number;
  max_records?: number;
  maxRecords?: number;
}

interface ListCollectionsResponse {
  collections: CollectionInfo[];
}

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<ListCollectionsResponse>;
  });

/**
 * @param projectId       Opaque project identity — the local project's
 *   registry/display name in Local mode, or the Cloud project's id in Cloud
 *   mode. Used only for legacy-prefix detection (see module doc above); has
 *   no effect on Cloud, whose namespaces are never prefixed with it.
 * @param cloudProjectId   Pass the SAME value as `projectId` when calling
 *   from a Cloud route, to target `/api/cloud/projects/[id]/namespaces`
 *   instead of the local daemon's `/api/namespaces`. Omit for Local call
 *   sites — matches the existing dual-mode convention every sibling hook
 *   already uses (useHealth, useCluster, useProof, useGraph).
 */
export function useCollections(projectId: string, cloudProjectId?: string) {
  const path = cloudProjectId
    ? `/api/cloud/projects/${cloudProjectId}/namespaces`
    : `/api/namespaces?project=${encodeURIComponent(projectId)}`;

  const { data, error, isLoading, mutate } = useSWR<ListCollectionsResponse>(
    path,
    fetcher,
    { refreshInterval: 10000 }
  );

  const rawNamespaces = (data?.collections ?? []).map((c) => c.rawNamespace ?? c.name);
  const allRefs = toRefs(projectId, rawNamespaces);
  const refMap = new Map<string, CollectionRef>();
  for (const ref of allRefs) {
    const existing = refMap.get(ref.name);
    if (!existing || (existing.rawNamespace !== ref.name && ref.rawNamespace === ref.name)) {
      refMap.set(ref.name, ref);
    }
  }
  const raw = Array.from(refMap.values());
  const collections = raw.map((r) => r.name);
  const rawByName = new Map(raw.map((r) => [r.name, r.rawNamespace]));

  const collectionDetails = new Map<string, CollectionMeta>();
  for (const item of data?.collections ?? []) {
    const rawNs = item.rawNamespace ?? item.name;
    const canonical = toCanonicalName(projectId, rawNs);
    collectionDetails.set(canonical, {
      name: canonical,
      rawNamespace: rawNs,
      id: item.id,
      dimension: item.dimension ?? null,
      metric: item.metric ?? null,
      index: item.index ?? null,
      recordCount: item.record_count ?? item.recordCount ?? 0,
      maxRecords: item.max_records ?? item.maxRecords ?? 1000000,
    });
  }

  const create = async (
    name: string,
    dim: number,
    index?: "brute" | "hnsw" | "ivf" | "bq" | "auto",
  ) => {
    // New collections are always created bare (canonical == raw) — the
    // project already owns its own dedicated node, so there is nothing left
    // for a prefix to disambiguate.
    const body: Record<string, unknown> = { name, dimension: dim, metric: "squared_l2" };
    if (index && index !== "brute") body.index = index;
    const createPath = cloudProjectId
      ? `/api/cloud/projects/${cloudProjectId}/namespaces`
      : `/api/namespaces?project=${encodeURIComponent(projectId)}`;
    const res = await fetch(createPath, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const e = await res.json().catch(() => ({})) as { error?: string };
      const msg = e.error ?? `Failed to create collection (${res.status})`;
      const { toast } = await import("@/lib/toast");
      toast(msg, "error");
      throw new Error(msg);   // keeps dialog open with inline error
    }
    mutate();
  };

  const drop = async (name: string) => {
    // Resolve back to whatever the real stored namespace is — bare for a
    // new-style collection, "${projectId}--name" for a legacy one — never
    // reconstruct it from the display name alone.
    const rawNs = rawByName.get(name) ?? name;
    const dropPath = cloudProjectId
      ? `/api/cloud/projects/${cloudProjectId}/namespaces/${encodeURIComponent(rawNs)}`
      : `/api/namespaces/${encodeURIComponent(rawNs)}?project=${encodeURIComponent(projectId)}`;
    const res = await fetch(dropPath, { method: "DELETE" });
    if (!res.ok) {
      const e = await res.json().catch(() => ({})) as { error?: string };
      const msg = e.error ?? `Failed to delete collection (${res.status})`;
      const { toast } = await import("@/lib/toast");
      toast(msg, "error");
      throw new Error(msg);
    }
    mutate();
  };

  return { collections, raw, collectionDetails, isLoading, error: error ?? null, create, drop, refresh: mutate };
}
