"use client";

import { useEffect, useState } from "react";
import { useHealth } from "@/lib/hooks/useHealth";
import type { SearchResult } from "@/types/valori";

interface Props {
  collection: string;
}

export function RecordsTab({ collection }: Props) {
  const { dim } = useHealth();
  const [records, setRecords] = useState<SearchResult[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    if (!dim) return;
    setIsLoading(true);
    setError(null);
    try {
      // No list endpoint — approximate with zero vector search k=50
      const zeroVec = new Array(dim).fill(0);
      const res = await fetch("/api/search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: zeroVec, k: 50, collection }),
      });
      if (!res.ok) throw new Error(`${res.status}`);
      const data = await res.json();
      setRecords(data.results ?? []);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load records");
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (dim) load();
  }, [dim, collection]);

  const deleteRecord = async (id: number) => {
    await fetch("/api/search", {
      method: "DELETE",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });
    // Actually delete uses a different endpoint
  };

  if (!dim) return <p className="text-xs text-muted-foreground">Loading dimension…</p>;
  if (isLoading) return <div className="h-8 animate-pulse rounded bg-accent w-1/2" />;
  if (error) return <p className="text-xs text-red-400">{error}</p>;
  if (records.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-border py-10 text-center">
        <p className="text-sm text-muted-foreground">No records in this project yet.</p>
        <p className="mt-1 text-xs text-muted-foreground">Use the Upload tab to add vectors.</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between mb-1">
        <p className="text-xs text-muted-foreground">
          Showing up to 50 records (nearest to zero vector in{" "}
          <span className="font-mono">{dim}D</span> space)
        </p>
        <button
          onClick={load}
          className="text-xs text-muted-foreground hover:text-accent-foreground transition-colors"
        >
          Refresh
        </button>
      </div>

      <div className="grid grid-cols-[3rem_1fr_5rem_4rem] gap-2 px-3 py-1.5 text-xs text-muted-foreground uppercase tracking-wider border-b border-border">
        <span>#</span><span>Record ID</span><span>Score</span><span></span>
      </div>

      {records.map((r, i) => (
        <RecordRow key={r.id} rank={i + 1} record={r} onRefresh={load} />
      ))}
    </div>
  );
}

function RecordRow({
  rank,
  record,
  onRefresh,
}: {
  rank: number;
  record: SearchResult;
  onRefresh: () => void;
}) {
  const [deleting, setDeleting] = useState(false);

  const del = async () => {
    setDeleting(true);
    try {
      await fetch("/api/delete", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id: record.id }),
      });
      onRefresh();
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="grid grid-cols-[3rem_1fr_5rem_4rem] gap-2 items-center rounded-lg border border-border bg-card px-3 py-2 text-sm">
      <span className="text-muted-foreground font-mono text-xs">{rank}</span>
      <span className="font-mono text-card-foreground">#{record.id}</span>
      <span className="font-mono text-xs text-muted-foreground">
        {record.score.toFixed(4)}
      </span>
      <button
        onClick={del}
        disabled={deleting}
        className="text-xs text-muted-foreground hover:text-red-400 transition-colors text-right"
      >
        {deleting ? "…" : "delete"}
      </button>
    </div>
  );
}
