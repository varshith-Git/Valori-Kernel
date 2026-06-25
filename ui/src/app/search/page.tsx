"use client";

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
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
        <h1 className="text-xl font-semibold text-foreground">Global Search</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          k-NN search across all projects or within a specific one
        </p>
      </div>

      {/* Query vector textarea */}
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <p className="text-sm font-medium text-accent-foreground">
            Query vector{" "}
            {dim && <span className="text-xs text-muted-foreground font-normal">({dim}D)</span>}
          </p>
          <span className="text-[10px] text-muted-foreground">
            Paste comma- or space-separated floats
          </span>
        </div>

        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.metaKey && run()}
          placeholder="0.12, 0.34, 0.56, 0.78, ..."
          rows={3}
          className="w-full rounded-lg border border-input bg-background px-3 py-2 font-mono text-xs text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] resize-none transition-shadow"
        />

        {/* Controls row */}
        <div className="grid grid-cols-2 gap-3">
          {/* k */}
          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">Results (k)</label>
            <input
              type="number"
              min={1}
              max={100}
              value={k}
              onChange={(e) => setK(Number(e.target.value))}
              className="rounded-lg border border-input bg-background px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] transition-shadow"
            />
          </div>

          {/* Scope */}
          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">Scope</label>
            <select
              value={collection}
              onChange={(e) => setCollection(e.target.value)}
              className="rounded-lg border border-input bg-background px-3 py-1.5 text-sm text-accent-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] transition-shadow"
            >
              <option value="">All projects</option>
              {projects.map((p) => (
                <option key={p} value={p}>{p}</option>
              ))}
            </select>
          </div>

          {/* Consistency */}
          <div className="flex flex-col gap-1 col-span-2">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">
              Read consistency
              <span className="ml-1.5 text-muted-foreground normal-case tracking-normal font-normal">
                — affects cluster deployments only
              </span>
            </label>
            <div className="flex gap-2">
              {([
                { value: "local",         label: "Fast (local)",           sub: "May lag leader by a few entries" },
                { value: "linearizable",  label: "Consistent (cluster-wide)", sub: "Waits for read-index quorum" },
              ] as const).map((opt) => (
                <button
                  key={opt.value}
                  title={opt.sub}
                  onClick={() => setConsistency(opt.value)}
                  className={cn(
                    "flex-1 rounded-lg border px-3 py-2 text-xs font-medium text-left transition-all",
                    consistency === opt.value
                      ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-foreground"
                      : "border-input bg-background text-muted-foreground hover:border-ring hover:text-card-foreground"
                  )}
                >
                  <span className="block">{opt.label}</span>
                  <span className="block text-[10px] font-normal text-muted-foreground mt-0.5">{opt.sub}</span>
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Action row */}
        <div className="flex items-center gap-3">
          <Button
            onClick={run}
            disabled={isLoading || !input.trim()}
            className="bg-[var(--v-accent)] text-white hover:opacity-90 disabled:opacity-40 transition-opacity"
            size="sm"
          >
            {isLoading ? "Searching…" : "Search"}
          </Button>
          <span className="text-xs text-muted-foreground">
            or press{" "}
            <kbd className="rounded border border-input bg-card px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
              ⌘↵
            </kbd>
          </span>
        </div>
      </div>

      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-3">
          <p className="text-sm text-red-400">{error}</p>
        </div>
      )}

      {results.length > 0 && (
        <div className="flex flex-col gap-2">
          {stateHash && (
            <p className="text-xs text-muted-foreground font-mono">
              Searched against state{" "}
              <span className="text-muted-foreground">{stateHash.slice(0, 16)}…</span>
              {queriedAt && ` at ${new Date(queriedAt).toLocaleTimeString()}`}
            </p>
          )}

          <div className="grid grid-cols-[2rem_1fr_6rem_6rem] gap-2 px-3 py-1.5 text-xs text-muted-foreground uppercase tracking-wider border-b border-border">
            <span>#</span><span>Record ID</span><span>Score</span><span>Project</span>
          </div>

          {results.map((r, i) => (
            <div
              key={r.id}
              className="grid grid-cols-[2rem_1fr_6rem_6rem] gap-2 items-center rounded-lg border border-border bg-card px-3 py-2.5 text-sm"
            >
              <span className="text-muted-foreground font-mono text-xs">{i + 1}</span>
              <span className="font-mono text-card-foreground">#{r.id}</span>
              <span className="font-mono text-xs text-muted-foreground">
                {r.score.toFixed(6)}
              </span>
              <span className="text-xs text-muted-foreground">
                {r.collection ?? (collection || "—")}
              </span>
            </div>
          ))}
        </div>
      )}

      {!isLoading && results.length === 0 && input && (
        <p className="text-xs text-muted-foreground text-center py-4">
          No results. Check your vector dimension ({dim}D expected).
        </p>
      )}
    </div>
  );
}
