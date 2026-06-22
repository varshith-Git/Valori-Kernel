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
          <p className="text-xs text-muted-foreground">
            Query vector{" "}
            {dim && (
              <span className="text-muted-foreground">({dim}D — paste {dim} comma-separated floats)</span>
            )}
          </p>
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted-foreground">k =</label>
            <input
              type="number"
              min={1}
              max={100}
              value={k}
              onChange={(e) => setK(Number(e.target.value))}
              className="w-14 rounded border border-input bg-background px-2 py-1 text-xs text-foreground text-center focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </div>
        </div>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.metaKey && run()}
          placeholder="0.12, 0.34, 0.56, 0.78, ..."
          rows={3}
          className="w-full rounded-lg border border-input bg-background px-3 py-2 font-mono text-xs text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring resize-none"
        />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            disabled={isLoading || !input.trim()}
            onClick={run}
            className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
          >
            {isLoading ? "Searching…" : "Search →"}
          </Button>
          <span className="text-xs text-muted-foreground">⌘↵ to run</span>
        </div>
      </div>

      {error && <p className="text-xs text-red-400">{error}</p>}

      {results.length > 0 && (
        <div className="flex flex-col gap-2">
          {stateHash && (
            <p className="text-xs text-muted-foreground font-mono">
              state: {stateHash.slice(0, 16)}…{" "}
              {queriedAt && `at ${new Date(queriedAt).toLocaleTimeString()}`}
            </p>
          )}
          <div className="grid grid-cols-[2rem_1fr_6rem] gap-2 px-3 py-1.5 text-xs text-muted-foreground uppercase tracking-wider border-b border-border">
            <span>#</span><span>Record ID</span><span>Score</span>
          </div>
          {results.map((r, i) => (
            <div
              key={r.id}
              className="grid grid-cols-[2rem_1fr_6rem] gap-2 items-center rounded-lg border border-border bg-card px-3 py-2 text-sm"
            >
              <span className="text-muted-foreground font-mono text-xs">{i + 1}</span>
              <span className="font-mono text-card-foreground">#{r.id}</span>
              <span className="font-mono text-xs text-muted-foreground">
                {r.score.toFixed(6)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
