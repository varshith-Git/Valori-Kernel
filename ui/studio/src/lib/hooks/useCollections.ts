"use client";

import useSWR from "swr";
import { useTransport } from "@/runtime/context";
import type { ProjectRef } from "@/runtime/project";

// Collections belong to a project (`Project -> Collection`). Each entry
// carries both its canonical display name and its actual raw node
// namespace. Shared Studio treats `rawNamespace` as an opaque token it must
// round-trip on mutations (delete) — it never inspects, parses, or assumes
// anything about *why* the two might differ. A host whose backend never has
// a reason to distinguish them (Cloud today) simply returns the same value
// for both; a host that does (a Local runtime carrying pre-existing
// namespaces from before the current per-project-node architecture) is
// responsible for populating `rawNamespace` correctly in its own API
// response, entirely outside this package. See the collection-model
// investigation and LocalRuntime's own documentation for that translation
// — it does not belong here, and this file contains no prefix/separator
// logic of any kind.
export interface CollectionRef {
  name: string;
  rawNamespace: string;
  /** Vector dimension for this collection, read from GET /v1/namespaces.
   *  Undefined when the server did not include it (legacy hosts). */
  dimension?: number;
}

interface CollectionInfo {
  name: string;
  rawNamespace?: string;
  /** The vector dimension reported by GET /v1/namespaces (available even
   *  before any records are inserted, unlike GET /health's `dim` field which
   *  standalone nodes only emit after the first insert). */
  dimension?: number;
}

interface ListCollectionsResponse {
  collections: CollectionInfo[];
}

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<ListCollectionsResponse>;
  });

export function useCollections(projectId: ProjectRef) {
  const transport = useTransport();
  const path = transport.path(projectId, "/namespaces");

  const { data, error, isLoading, mutate } = useSWR<ListCollectionsResponse>(path, fetcher, {
    refreshInterval: 10000,
  });

  const raw: CollectionRef[] = (data?.collections ?? []).map((c) => ({
    name: c.name,
    rawNamespace: c.rawNamespace ?? c.name,
    dimension: c.dimension,
  }));
  const collections = raw.map((r) => r.name);
  const rawByName = new Map(raw.map((r) => [r.name, r.rawNamespace]));

  const create = async (name: string) => {
    // Always created bare — a project already owns its own dedicated node,
    // so there is nothing left for any naming convention to disambiguate.
    const res = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    if (!res.ok) {
      const e = (await res.json().catch(() => ({}))) as { error?: string };
      const msg = e.error ?? `Failed to create collection (${res.status})`;
      const { toast } = await import("@/lib/toast");
      toast(msg, "error");
      throw new Error(msg);
    }
    mutate();
  };

  const drop = async (name: string) => {
    // Resolve back to whatever the host says this collection's real raw
    // namespace is — never reconstructed here.
    const rawNs = rawByName.get(name) ?? name;
    const res = await fetch(transport.path(projectId, `/namespaces/${encodeURIComponent(rawNs)}`), {
      method: "DELETE",
    });
    if (!res.ok) {
      const e = (await res.json().catch(() => ({}))) as { error?: string };
      const msg = e.error ?? `Failed to delete collection (${res.status})`;
      const { toast } = await import("@/lib/toast");
      toast(msg, "error");
      throw new Error(msg);
    }
    mutate();
  };

  return { collections, raw, isLoading, error: error ?? null, create, drop, refresh: mutate };
}
