"use client";

import useSWR from "swr";
import { toast } from "@/lib/toast";

const fetcher = (url: string) =>
  fetch(url).then(async (r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    const data = await r.json();
    return Array.isArray(data.collections)
      ? data.collections.map((c: { name: string }) => c.name)
      : [];
  });

export function useProjects() {
  const { data, error, isLoading, mutate } = useSWR<string[]>(
    "/api/namespaces",
    fetcher,
    { refreshInterval: 15000 }
  );

  const create = async (name: string) => {
    const res = await fetch("/api/namespaces", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    if (!res.ok) {
      const e = await res.json().catch(() => ({})) as { error?: string };
      toast(e.error ?? `Failed to create "${name}" (${res.status})`, "error");
      return;
    }
    mutate();
  };

  const drop = async (name: string) => {
    const res = await fetch(`/api/namespaces/${encodeURIComponent(name)}`, {
      method: "DELETE",
    });
    if (!res.ok) {
      toast(`Failed to delete "${name}" (${res.status})`, "error");
      return;
    }
    mutate();
  };

  return {
    projects: Array.isArray(data) ? data : [],
    isLoading,
    error: error ?? null,
    create,
    drop,
    refresh: mutate,
  };
}
