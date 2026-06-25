"use client";

import useSWR from "swr";
import { toast } from "@/lib/toast";

export interface ManifestProject {
  name:          string;
  dir:           string;
  port:          number;
  dim:           number;
  index:         "brute" | "hnsw";
  maxRecords:    number;
  createdAt:     string;
  lastOpenedAt?: string;
  records?:      number;
  status:        "stopped" | "starting" | "running" | "error";
}

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    const d = await r.json() as { projects: ManifestProject[] };
    return Array.isArray(d.projects) ? d.projects : [];
  });

/**
 * The workspace project list, sourced from the on-disk manifest
 * (`~/.valori/projects.json`). Unlike {@link useProjects}, this works even when
 * every node is stopped — it is the Home picker's source of truth.
 */
export function useProjectManifest() {
  const { data, error, isLoading, mutate } = useSWR<ManifestProject[]>(
    "/api/projects",
    fetcher,
    { refreshInterval: 4000 }
  );

  const create = async (input: {
    name: string;
    dim?: number;
    index?: "brute" | "hnsw";
  }): Promise<ManifestProject | null> => {
    const res = await fetch("/api/projects", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(input),
    });
    const d = await res.json().catch(() => ({})) as { project?: ManifestProject; error?: string };
    if (!res.ok || !d.project) {
      toast(d.error ?? `Failed to create "${input.name}"`, "error");
      return null;
    }
    await mutate();
    return d.project;
  };

  /** Ensure the project's node is running and point the UI proxy at it. */
  const open = async (name: string): Promise<boolean> => {
    const res = await fetch(`/api/projects/${encodeURIComponent(name)}/open`, { method: "POST" });
    const d = await res.json().catch(() => ({})) as { reachable?: boolean; error?: string };
    await mutate();
    if (!res.ok) {
      toast(d.error ?? `Failed to open "${name}"`, "error");
      return false;
    }
    if (!d.reachable) {
      toast(`"${name}" started but is not responding yet — retry in a moment`, "warning");
    }
    return true;
  };

  /** Snapshot + stop the project's node, then re-lock its files at rest. */
  const close = async (name: string): Promise<void> => {
    const res = await fetch(`/api/projects/${encodeURIComponent(name)}/close`, { method: "POST" });
    if (!res.ok) {
      const d = await res.json().catch(() => ({})) as { error?: string };
      toast(d.error ?? `Failed to close "${name}"`, "error");
    }
    await mutate();
  };

  const remove = async (name: string): Promise<void> => {
    const res = await fetch(`/api/projects/${encodeURIComponent(name)}`, { method: "DELETE" });
    if (!res.ok) {
      toast(`Failed to delete "${name}"`, "error");
      return;
    }
    await mutate();
  };

  return {
    projects: Array.isArray(data) ? data : [],
    isLoading,
    error: error ?? null,
    create,
    open,
    close,
    remove,
    refresh: mutate,
  };
}
