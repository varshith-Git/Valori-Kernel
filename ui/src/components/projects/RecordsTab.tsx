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

  if (!dim) return <p className="text-xs text-zinc-500">Loading dimension…</p>;
  if (isLoading) return <div className="h-8 animate-pulse rounded bg-zinc-800 w-1/2" />;
  if (error) return <p className="text-xs text-red-400">{error}</p>;
  if (records.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-zinc-800 py-10 text-center">
        <p className="text-sm text-zinc-500">No records in this project yet.</p>
        <p className="mt-1 text-xs text-zinc-600">Use the Upload tab to add vectors.</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between mb-1">
        <p className="text-xs text-zinc-500">
          Showing up to 50 records (nearest to zero vector in{" "}
          <span className="font-mono">{dim}D</span> space)
        </p>
        <button
          onClick={load}
          className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors"
        >
          Refresh
        </button>
      </div>

      <div className="grid grid-cols-[3rem_1fr_5rem_4rem] gap-2 px-3 py-1.5 text-xs text-zinc-600 uppercase tracking-wider border-b border-zinc-800">
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
    <div className="grid grid-cols-[3rem_1fr_5rem_4rem] gap-2 items-center rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm">
      <span className="text-zinc-600 font-mono text-xs">{rank}</span>
      <span className="font-mono text-zinc-200">#{record.id}</span>
      <span className="font-mono text-xs text-zinc-400">
        {record.score.toFixed(4)}
      </span>
      <button
        onClick={del}
        disabled={deleting}
        className="text-xs text-zinc-600 hover:text-red-400 transition-colors text-right"
      >
        {deleting ? "…" : "delete"}
      </button>
    </div>
  );
}
