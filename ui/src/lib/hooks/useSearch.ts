"use client";

import { useState } from "react";
import type { SearchResult } from "@/types/valori";

export interface SearchQuery {
  vector: number[];
  k: number;
  collection?: string;
  consistency?: "local" | "linearizable";
  metadataFilter?: Record<string, unknown>;
}

export interface SearchState {
  results: SearchResult[];
  stateHash: string | null;
  queriedAt: string | null;
  /** Round-trip latency for the last search in milliseconds, or null before first search. */
  latencyMs: number | null;
}

export function useSearch() {
  const [state, setState] = useState<SearchState>({
    results: [],
    stateHash: null,
    queriedAt: null,
    latencyMs: null,
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = async (q: SearchQuery) => {
    setIsLoading(true);
    setError(null);
    const t0 = Date.now();
    try {
      const body: Record<string, unknown> = {
        query: q.vector,
        k: q.k,
      };
      if (q.collection) body.collection = q.collection;
      if (q.consistency) body.consistency = q.consistency;
      if (q.metadataFilter && Object.keys(q.metadataFilter).length > 0) {
        body.metadata_filter = q.metadataFilter;
      }

      const res = await fetch("/api/search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) throw new Error(`Search failed: ${res.status}`);
      const data = await res.json();
      setState({
        results: data.results ?? [],
        stateHash: data.state_hash ?? null,
        queriedAt: data.queried_at ?? new Date().toISOString(),
        latencyMs: Date.now() - t0,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Search failed";
      setError(msg);
      const { toast } = await import("@/lib/toast");
      toast(msg, "error");
    } finally {
      setIsLoading(false);
    }
  };

  return { ...state, isLoading, error, search };
}
