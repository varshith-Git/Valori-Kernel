"use client";

import { useEffect } from "react";
import useSWR from "swr";
import { toast } from "@/lib/toast";
import { nativeAvailable } from "@/lib/native";

export interface ManifestProjectNode {
  id:        number;
  httpPort:  number;
  raftPort?: number;
}

export interface ManifestProject {
  name:          string;
  dir:           string;
  replication:   1 | 3;
  nodes:         ManifestProjectNode[];
  shardCount:    number;
  port:          number;
  dim:           number;
  index:         "brute" | "hnsw" | "ivf" | "bq" | "auto";
  maxRecords:    number;
  createdAt:     string;
  lastOpenedAt?: string;
  records?:      number;
  embed?:        { provider: string; model: string; apiKey?: string; endpoint?: string };
  status:        "stopped" | "starting" | "running" | "error";
  nodesRunning:  number;
  nodesTotal:    number;
  collections?:  string[];
}

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    const d = await r.json() as { projects: ManifestProject[] };
    return Array.isArray(d.projects) ? d.projects : [];
  });

// ── localStorage cache for instant first paint ────────────────────────────────

const PROJECTS_CACHE_KEY = "valori:projects-list";

function readProjectCache(): ManifestProject[] | undefined {
  try {
    const raw = localStorage.getItem(PROJECTS_CACHE_KEY);
    if (!raw) return undefined;
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : undefined;
  } catch {
    return undefined;
  }
}

function writeProjectCache(projects: ManifestProject[]) {
  try {
    localStorage.setItem(PROJECTS_CACHE_KEY, JSON.stringify(projects));
  } catch {}
}

/**
 * The workspace project list, sourced from the on-disk manifest
 * (`~/.valori/projects.json`). Unlike {@link useProjects}, this works even when
 * every node is stopped — it is the Home picker's source of truth.
 *
 * Uses localStorage as a SWR fallback so the project grid renders instantly
 * on page load instead of flashing an empty skeleton.
 */
export function useProjectManifest() {
  const { data, error, isLoading, mutate } = useSWR<ManifestProject[]>(
    "/api/projects",
    fetcher,
    {
      refreshInterval: 10000,
      onSuccess: writeProjectCache,
    }
  );

  // Seed SWR cache from localStorage AFTER hydration to avoid SSR mismatch.
  // The first render matches the server (no data); the useEffect fires
  // immediately after mount and populates the cache so the second render
  // shows the cached project list — imperceptible to the user.
  useEffect(() => {
    if (!data) {
      const cached = readProjectCache();
      if (cached && cached.length > 0) {
        mutate(cached, { revalidate: false });
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const create = async (input: {
    name: string;
    dim?: number;
    index?: "brute" | "hnsw" | "ivf" | "bq" | "auto";
    replication?: 1 | 3;
    shardCount?: number;
    embed?: { provider: string; model: string; endpoint?: string };
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
    const d = await res.json().catch(() => ({})) as { reachable?: boolean; error?: string; dir?: string };
    await mutate();
    if (!res.ok) {
      toast(d.error ?? `Failed to open "${name}"`, "error");
      return false;
    }
    if (!d.reachable) {
      toast(`"${name}" started but is not responding yet — retry in a moment`, "warning");
    }
    // Tell macOS to add this project to the Dock "Open Recent" list.
    if (nativeAvailable() && d.dir) {
      import("@tauri-apps/api/core")
        .then(({ invoke }) => invoke("add_recent_document", { path: d.dir }))
        .catch(() => {});
    }
    return true;
  };

  /** Rename a stopped project. Returns the new name on success, null on error. */
  const rename = async (oldName: string, newName: string): Promise<string | null> => {
    const res = await fetch(`/api/projects/${encodeURIComponent(oldName)}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: newName }),
    });
    const d = await res.json().catch(() => ({})) as { project?: { name: string }; error?: string };
    if (!res.ok) {
      toast(d.error ?? `Failed to rename "${oldName}"`, "error");
      return null;
    }
    // Evict old cache entry so the renamed project doesn't ghost.
    const cached = readProjectCache();
    if (cached) writeProjectCache(cached.filter((p) => p.name !== oldName));
    await mutate();
    return d.project?.name ?? newName;
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
    // Immediately evict from localStorage so a page refresh doesn't show it.
    const cached = readProjectCache();
    if (cached) writeProjectCache(cached.filter((p) => p.name !== name));
    await mutate();
  };

  return {
    projects: Array.isArray(data) ? data : [],
    isLoading,
    error: error ?? null,
    create,
    open,
    close,
    rename,
    remove,
    refresh: mutate,
  };
}
