"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useSearch } from "@/lib/hooks/useSearch";
import { useHealth } from "@/lib/hooks/useHealth";

interface Props {
  collection: string;
}

export function SearchTab({ collection }: Props) {
  const [input, setInput] = useState("");
  const [k, setK] = useState(10);
  const { dim } = useHealth();
  const { results, stateHash, queriedAt, isLoading, error, search } = useSearch();

  const run = () => {
    const nums = input
      .split(/[\s,]+/)
      .map(Number)
      .filter((n) => !isNaN(n));
    if (nums.length === 0) return;
    search({ vector: nums, k, collection });
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <p className="text-xs text-zinc-400">
            Query vector{" "}
            {dim && (
              <span className="text-zinc-600">({dim}D — paste {dim} comma-separated floats)</span>
            )}
          </p>
          <div className="flex items-center gap-2">
            <label className="text-xs text-zinc-500">k =</label>
            <input
              type="number"
              min={1}
              max={100}
              value={k}
              onChange={(e) => setK(Number(e.target.value))}
              className="w-14 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-xs text-white text-center focus:outline-none focus:ring-1 focus:ring-zinc-500"
            />
          </div>
        </div>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.metaKey && run()}
          placeholder="0.12, 0.34, 0.56, 0.78, ..."
          rows={3}
          className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-500 resize-none"
        />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            disabled={isLoading || !input.trim()}
            onClick={run}
            className="bg-white text-zinc-900 hover:bg-zinc-100 disabled:opacity-40"
          >
            {isLoading ? "Searching…" : "Search →"}
          </Button>
          <span className="text-xs text-zinc-600">⌘↵ to run</span>
        </div>
      </div>

      {error && <p className="text-xs text-red-400">{error}</p>}

      {results.length > 0 && (
        <div className="flex flex-col gap-2">
          {stateHash && (
            <p className="text-xs text-zinc-600 font-mono">
              state: {stateHash.slice(0, 16)}…{" "}
              {queriedAt && `at ${new Date(queriedAt).toLocaleTimeString()}`}
            </p>
          )}
          <div className="grid grid-cols-[2rem_1fr_6rem] gap-2 px-3 py-1.5 text-xs text-zinc-600 uppercase tracking-wider border-b border-zinc-800">
            <span>#</span><span>Record ID</span><span>Score</span>
          </div>
          {results.map((r, i) => (
            <div
              key={r.id}
              className="grid grid-cols-[2rem_1fr_6rem] gap-2 items-center rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm"
            >
              <span className="text-zinc-600 font-mono text-xs">{i + 1}</span>
              <span className="font-mono text-zinc-200">#{r.id}</span>
              <span className="font-mono text-xs text-zinc-400">
                {r.score.toFixed(6)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
