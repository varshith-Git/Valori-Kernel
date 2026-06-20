"use client";

import useSWR from "swr";

const fetcher = (url: string) =>
  fetch(url).then((r) => {
    if (!r.ok) throw new Error(`${r.status}`);
    return r.json() as Promise<string[]>;
  });

export function useProjects() {
  const { data, error, isLoading, mutate } = useSWR<string[]>(
    "/api/namespaces",
    fetcher,
    { refreshInterval: 15000 }
  );

  const create = async (name: string) => {
    await fetch("/api/namespaces", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    mutate();
  };

  const drop = async (name: string) => {
    await fetch(`/api/namespaces/${encodeURIComponent(name)}`, {
      method: "DELETE",
    });
    mutate();
  };

  return {
    projects: data ?? [],
    isLoading,
    error: error ?? null,
    create,
    drop,
    refresh: mutate,
  };
}
