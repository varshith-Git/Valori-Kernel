"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useSearch } from "@/lib/hooks/useSearch";
import { useProjects } from "@/lib/hooks/useProjects";
import { useHealth } from "@/lib/hooks/useHealth";

export default function SearchPage() {
  const [input, setInput] = useState("");
  const [k, setK] = useState(10);
  const [collection, setCollection] = useState("");
  const [consistency, setConsistency] = useState<"local" | "linearizable">("local");
  const { dim } = useHealth();
  const { projects } = useProjects();
  const { results, stateHash, queriedAt, isLoading, error, search } = useSearch();

  const run = () => {
    const nums = input
      .split(/[\s,]+/)
      .map(Number)
      .filter((n) => !isNaN(n));
    if (nums.length === 0) return;
    search({
      vector: nums,
      k,
      collection: collection || undefined,
      consistency,
    });
  };

  return (
    <div className="flex flex-col gap-6 max-w-3xl">
      <div>
        <h1 className="text-xl font-semibold text-white">Global Search</h1>
        <p className="mt-1 text-sm text-zinc-500">
          k-NN search across all projects or within a specific one
        </p>
      </div>

      <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-5 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <p className="text-xs text-zinc-400">
            Query vector{" "}
            {dim && <span className="text-zinc-600">({dim}D)</span>}
          </p>
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1.5">
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
            <select
              value={collection}
              onChange={(e) => setCollection(e.target.value)}
              className="rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-xs text-zinc-300 focus:outline-none"
            >
              <option value="">All projects</option>
              {projects.map((p) => (
                <option key={p} value={p}>{p}</option>
              ))}
            </select>
            <select
              value={consistency}
              onChange={(e) => setConsistency(e.target.value as "local" | "linearizable")}
              className="rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-xs text-zinc-300 focus:outline-none"
            >
              <option value="local">local</option>
              <option value="linearizable">linearizable</option>
            </select>
          </div>
        </div>

        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.metaKey && run()}
          placeholder="Paste your query vector: 0.12, 0.34, 0.56, 0.78, ..."
          rows={3}
          className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-500 resize-none"
        />

        <div className="flex items-center gap-2">
          <Button
            onClick={run}
            disabled={isLoading || !input.trim()}
            className="bg-white text-zinc-900 hover:bg-zinc-100 disabled:opacity-40"
            size="sm"
          >
            {isLoading ? "Searching…" : "Search →"}
          </Button>
          <span className="text-xs text-zinc-600">⌘↵ to run</span>
        </div>
      </div>

      {error && (
        <div className="rounded-lg border border-red-900 bg-red-950 px-4 py-3">
          <p className="text-sm text-red-400">{error}</p>
        </div>
      )}

      {results.length > 0 && (
        <div className="flex flex-col gap-2">
          {stateHash && (
            <p className="text-xs text-zinc-600 font-mono">
              Searched against state{" "}
              <span className="text-zinc-400">{stateHash.slice(0, 16)}…</span>
              {queriedAt && ` at ${new Date(queriedAt).toLocaleTimeString()}`}
            </p>
          )}

          <div className="grid grid-cols-[2rem_1fr_6rem_6rem] gap-2 px-3 py-1.5 text-xs text-zinc-600 uppercase tracking-wider border-b border-zinc-800">
            <span>#</span><span>Record ID</span><span>Score</span><span>Project</span>
          </div>

          {results.map((r, i) => (
            <div
              key={r.id}
              className="grid grid-cols-[2rem_1fr_6rem_6rem] gap-2 items-center rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2.5 text-sm"
            >
              <span className="text-zinc-600 font-mono text-xs">{i + 1}</span>
              <span className="font-mono text-zinc-200">#{r.id}</span>
              <span className="font-mono text-xs text-zinc-400">
                {r.score.toFixed(6)}
              </span>
              <span className="text-xs text-zinc-500">
                {r.collection ?? (collection || "—")}
              </span>
            </div>
          ))}
        </div>
      )}

      {!isLoading && results.length === 0 && input && (
        <p className="text-xs text-zinc-600 text-center py-4">
          No results. Check your vector dimension ({dim}D expected).
        </p>
      )}
    </div>
  );
}
