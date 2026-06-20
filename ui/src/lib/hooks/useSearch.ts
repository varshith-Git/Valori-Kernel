"use client";

import { useState } from "react";
import type { SearchResult } from "@/types/valori";

export interface SearchQuery {
  vector: number[];
  k: number;
  collection?: string;
  consistency?: "local" | "linearizable";
}

export interface SearchState {
  results: SearchResult[];
  stateHash: string | null;
  queriedAt: string | null;
}

export function useSearch() {
  const [state, setState] = useState<SearchState>({
    results: [],
    stateHash: null,
    queriedAt: null,
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = async (q: SearchQuery) => {
    setIsLoading(true);
    setError(null);
    try {
      const body: Record<string, unknown> = {
        query: q.vector,
        k: q.k,
      };
      if (q.collection) body.collection = q.collection;
      if (q.consistency) body.consistency = q.consistency;

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
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Search failed");
    } finally {
      setIsLoading(false);
    }
  };

  return { ...state, isLoading, error, search };
}
