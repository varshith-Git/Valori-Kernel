"use client";

import { useState, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useSearch } from "@/lib/hooks/useSearch";
import { useProjects } from "@/lib/hooks/useProjects";
import { useHealth } from "@/lib/hooks/useHealth";
import { useProof } from "@/lib/hooks/useProof";
import type { SearchResult } from "@/types/valori";
import type { ActivityEvent } from "@/app/api/activity/route";
import { EVENT_COLORS } from "@/lib/event-types";
import { PageHeader } from "@/components/ui/page-header";
import { MetricCard } from "@/components/ui/metric-card";
import { Skeleton } from "@/components/ui/skeleton";
import { StatusBadge } from "@/components/ui/status-badge";
import { CopyBtn } from "@/components/ui/copy-btn";

// -- Helpers -------------------------------------------------------------------

function fmtHash(h: string | null) {
  return h ? `${h.slice(0, 16)}…${h.slice(-8)}` : "—";
}

function fmtTime(iso: string) {
  if (!iso) return "";
  const d = new Date(iso);
  return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

// -- Tab bar -------------------------------------------------------------------

type Tab = "results" | "proof" | "timeline";

function TabBar({ active, onSelect, resultCount }: {
  active: Tab;
  onSelect: (t: Tab) => void;
  resultCount: number;
}) {
  const tabs: { id: Tab; label: string }[] = [
    { id: "results",  label: `Results${resultCount > 0 ? ` (${resultCount})` : ""}` },
    { id: "proof",    label: "Proof" },
    { id: "timeline", label: "Timeline" },
  ];
  return (
    <div className="flex items-center gap-0.5 border-b border-border">
      {tabs.map((t) => (
        <button
          key={t.id}
          onClick={() => onSelect(t.id)}
          className={cn(
            "px-4 py-2.5 text-sm font-medium transition-colors relative",
            active === t.id
              ? "text-foreground after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-[var(--v-accent)]"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

// -- Results tab ---------------------------------------------------------------

function ResultsTab({ results, stateHash, queriedAt }: {
  results: SearchResult[];
  stateHash: string | null;
  queriedAt: string | null;
}) {
  if (results.length === 0) {
    return (
      <div className="py-12 text-center text-sm text-muted-foreground">
        No results matched the query.
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      {stateHash && (
        <div className="flex items-center gap-2 mb-1">
          <p className="text-[11px] text-muted-foreground font-mono">
            Searched against state{" "}
            <span className="text-foreground">{stateHash.slice(0, 16)}…</span>
            {queriedAt && ` · ${new Date(queriedAt).toLocaleTimeString()}`}
          </p>
          <CopyBtn text={stateHash} label="copy hash" className="scale-75 origin-left" />
        </div>
      )}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {results.map((r, i) => (
          <div key={r.id} className="rounded-xl border border-border bg-card/60 hover:bg-card px-4 py-3 flex flex-col gap-2 transition-colors relative group">
            <div className="flex items-center gap-2.5">
              <span className="text-[10px] font-mono text-muted-foreground/60 w-5">{i + 1}</span>
              <span className="font-mono text-xs font-semibold text-foreground">#{r.id}</span>
              <StatusBadge tone="neutral">{`Score: ${r.score.toFixed(5)}`}</StatusBadge>
              {r.collection && (
                <StatusBadge tone="info" className="ml-auto text-[10px]">{r.collection}</StatusBadge>
              )}
              {r.source && (
                <span className="text-[10px] text-muted-foreground truncate max-w-[200px] font-mono" title={r.source}>
                  {r.source.split("/").pop()}
                </span>
              )}
            </div>
            {r.text && (
              <div className="relative pl-6 border-l-2 border-border/80 ml-5 group/text">
                <p className="text-xs text-muted-foreground leading-relaxed pr-8">
                  {r.text}
                </p>
                <div className="absolute right-0 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <CopyBtn text={r.text} label="copy chunk" />
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

// -- Proof tab -----------------------------------------------------------------

function ProofTab({ searchHash }: { searchHash: string | null }) {
  const { hash: currentHash, isLoading } = useProof();
  const { chainHeight, recordCount, dim } = useHealth();

  const match = searchHash && currentHash && searchHash === currentHash;
  const diverged = searchHash && currentHash && searchHash !== currentHash;

  return (
    <div className="flex flex-col gap-5">
      {/* Hash comparison */}
      <div className="rounded-xl border border-border bg-card overflow-hidden">
        <div className="px-5 py-3 border-b border-border bg-background/50 flex items-center justify-between">
          <h3 className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">State Hash</h3>
          {match && <StatusBadge tone="success">Current</StatusBadge>}
          {diverged && <StatusBadge tone="warning">Outdated</StatusBadge>}
        </div>
        <div className="px-5 py-4 flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <span className="text-[11px] text-muted-foreground">Searched against</span>
              {searchHash && <CopyBtn text={searchHash} label="copy search hash" />}
            </div>
            <code className="font-mono text-xs text-foreground bg-background border border-border rounded-lg px-3 py-2 break-all">
              {searchHash ?? "—"}
            </code>
          </div>

          {diverged && (
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-[11px] text-muted-foreground">Current</span>
                {currentHash && <CopyBtn text={currentHash} label="copy current hash" />}
              </div>
              <code className="font-mono text-xs text-foreground bg-background border border-border rounded-lg px-3 py-2 break-all">
                {isLoading ? "loading…" : (currentHash ?? "—")}
              </code>
            </div>
          )}
        </div>
      </div>

      {/* Chain metrics */}
      <div className="grid grid-cols-3 gap-3">
        <MetricCard
          label="Chain height"
          value={chainHeight?.toLocaleString() ?? "—"}
          hint="committed events"
        />
        <MetricCard
          label="Records"
          value={recordCount?.toLocaleString() ?? "—"}
          hint="live vectors"
        />
        <MetricCard
          label="Dimension"
          value={dim ? String(dim) : "—"}
          hint="Q16.16 fixed-point"
        />
      </div>

      <p className="text-[11px] text-muted-foreground">
        BLAKE3-chained audit trail. Every committed event changes this hash deterministically —
        tamper with one event and the entire chain is detectable.
      </p>
    </div>
  );
}

// -- Timeline tab --------------------------------------------------------------

function TimelineTab() {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [disabled, setDisabled] = useState(false);

  useEffect(() => {
    setLoading(true);
    fetch("/api/activity?limit=30")
      .then((r) => r.json())
      .then((d: { events?: ActivityEvent[]; disabled?: boolean }) => {
        setDisabled(d.disabled === true);
        setEvents(d.events ?? []);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="flex flex-col gap-2.5">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="flex items-center gap-3 py-2.5 px-4 rounded-xl border border-border/60 bg-card/40">
            <Skeleton className="h-3 w-8" />
            <Skeleton className="h-3 w-28" />
            <Skeleton className="h-3 flex-1" />
            <Skeleton className="h-3 w-16" />
          </div>
        ))}
      </div>
    );
  }

  if (disabled) {
    return (
      <div className="py-10 text-center">
        <p className="text-sm text-muted-foreground">Event log not enabled.</p>
        <p className="text-[11px] text-muted-foreground mt-1">Set <code>VALORI_EVENT_LOG_PATH</code> to enable the audit timeline.</p>
      </div>
    );
  }

  if (events.length === 0) {
    return <p className="py-10 text-center text-sm text-muted-foreground">No events recorded yet.</p>;
  }

  return (
    <div className="flex flex-col divide-y divide-border rounded-xl border border-border overflow-hidden">
      {events.map((e) => {
        const colorCls = EVENT_COLORS[e.event_type] ?? "text-muted-foreground";
        const detailStr = Object.entries(e.detail)
          .map(([k, v]) => `${k}=${JSON.stringify(v)}`)
          .join("  ");
        return (
          <div key={e.log_index} className="flex items-start gap-3 px-4 py-2.5 text-xs bg-card hover:bg-accent/30 transition-colors">
            <span className="font-mono text-[10px] text-muted-foreground w-8 tabular-nums shrink-0 pt-0.5">
              {e.log_index}
            </span>
            <span className={cn("font-mono font-medium shrink-0 w-36", colorCls)}>
              {e.event_type}
            </span>
            <span className="font-mono text-muted-foreground flex-1 min-w-0 truncate" title={detailStr}>
              {detailStr}
            </span>
            <span className="font-mono text-[10px] text-muted-foreground shrink-0 tabular-nums">
              {fmtTime(e.timestamp_iso)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

// -- Main page -----------------------------------------------------------------

export default function SearchPage() {
  const [input, setInput] = useState("");
  const [k, setK] = useState(10);
  const [collection, setCollection] = useState("");
  const [consistency, setConsistency] = useState<"local" | "linearizable">("local");
  const [filterRaw, setFilterRaw] = useState("");
  const [filterError, setFilterError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>("results");

  const { dim } = useHealth();
  const { projects } = useProjects();
  const { results, stateHash, queriedAt, isLoading, error, search, latencyMs } = useSearch();
  const [enrichedResults, setEnrichedResults] = useState<SearchResult[]>([]);

  const hasRun = results.length > 0 || (!!error && !isLoading);

  // Enrich results with metadata
  useEffect(() => {
    let ignore = false;
    setEnrichedResults(results);
    if (results.length === 0) return;
    Promise.all(
      results.map(async (r) => {
        try {
          const res = await fetch(`/api/meta?target_id=record:${r.id}`);
          if (!res.ok) return r;
          const d = await res.json().catch(() => ({})) as { metadata?: Record<string, unknown> };
          const m = d.metadata ?? {};
          return {
            ...r,
            text:   (m.text as string | undefined)?.slice(0, 160) ?? undefined,
            source: (m.source as string | undefined) ?? undefined,
          } satisfies SearchResult;
        } catch { return r; }
      })
    ).then((enriched) => { if (!ignore) setEnrichedResults(enriched); });
    return () => { ignore = true; };
  }, [results]);

  // Switch to Results tab automatically when a search completes
  useEffect(() => {
    if (results.length > 0) setActiveTab("results");
  }, [results]);

  const parseFilter = (raw: string): Record<string, unknown> | undefined => {
    const trimmed = raw.trim();
    if (!trimmed) return undefined;
    try {
      const parsed = JSON.parse(trimmed);
      if (typeof parsed !== "object" || Array.isArray(parsed)) {
        setFilterError('Must be a JSON object: {"key": "value"}');
        return undefined;
      }
      setFilterError(null);
      return parsed as Record<string, unknown>;
    } catch {
      setFilterError("Invalid JSON");
      return undefined;
    }
  };

  const run = () => {
    const nums = input.split(/[\s,]+/).map(Number).filter((n) => !isNaN(n));
    if (nums.length === 0) return;
    const metadataFilter = parseFilter(filterRaw);
    if (filterRaw.trim() && filterError) return;
    search({ vector: nums, k, collection: collection || undefined, consistency, metadataFilter });
  };

  return (
    <div className="flex flex-col gap-5 w-full max-w-[1600px]">

      {/* Page title */}
      <PageHeader
        title="Search"
        subtitle="k-NN vector similarity search across your collections"
      />

      {/* Query input card */}
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <p className="text-sm font-medium text-accent-foreground">
            Query vector{" "}
            {dim && <span className="text-xs text-muted-foreground font-normal">({dim}D)</span>}
          </p>
          <span className="text-[10px] text-muted-foreground">comma- or space-separated floats</span>
        </div>

        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.metaKey && run()}
          placeholder="0.12, 0.34, 0.56, 0.78, ..."
          rows={3}
          className="w-full rounded-lg border border-input bg-background px-3 py-2 font-mono text-xs text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] resize-none transition-shadow"
        />

        {/* Controls */}
        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">Results (k)</label>
            <input
              type="number" min={1} max={100} value={k}
              onChange={(e) => setK(Number(e.target.value))}
              className="rounded-lg border border-input bg-background px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] transition-shadow"
            />
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">Scope</label>
            <select
              value={collection}
              onChange={(e) => setCollection(e.target.value)}
              className="rounded-lg border border-input bg-background px-3 py-1.5 text-sm text-accent-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] transition-shadow"
            >
              <option value="">All collections</option>
              {projects.map((p) => <option key={p} value={p}>{p}</option>)}
            </select>
          </div>

          <div className="flex flex-col gap-1 col-span-2">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">
              Read consistency
              <span className="ml-1.5 text-muted-foreground normal-case tracking-normal font-normal">
                — cluster only
              </span>
            </label>
            <div className="flex gap-2">
              {([
                { value: "local",        label: "Fast (local)",        sub: "May lag leader by a few entries" },
                { value: "linearizable", label: "Consistent",          sub: "Waits for read-index quorum" },
              ] as const).map((opt) => (
                <button
                  key={opt.value}
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

        {/* Metadata filter */}
        <div className="flex flex-col gap-1">
          <label className="text-[10px] text-muted-foreground uppercase tracking-widest font-medium">
            Metadata filter
            <span className="ml-1.5 normal-case tracking-normal font-normal text-muted-foreground">— optional JSON</span>
          </label>
          <input
            type="text" value={filterRaw}
            onChange={(e) => { setFilterRaw(e.target.value); if (!e.target.value.trim()) setFilterError(null); }}
            placeholder='{"author": "Alice"} or {"year": {"gte": 2020}}'
            className={cn(
              "rounded-lg border bg-background px-3 py-1.5 font-mono text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] transition-shadow",
              filterError ? "border-red-500/60" : "border-input"
            )}
          />
          {filterError && <p className="text-[10px] text-red-600 dark:text-red-400">{filterError}</p>}
        </div>

        {/* Run */}
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
            or{" "}
            <kbd className="rounded border border-input bg-card px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">⌘↵</kbd>
          </span>

          {/* Latency — shown after the first completed search */}
          {latencyMs !== null && !isLoading && (
            <span className="ml-auto font-mono text-[11px] text-muted-foreground tabular-nums">
              {latencyMs < 1000 ? `${latencyMs} ms` : `${(latencyMs / 1000).toFixed(2)} s`}
            </span>
          )}
        </div>
      </div>

      {/* Error */}
      {error && !isLoading && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-3">
          <p className="text-sm text-red-600 dark:text-red-400">{error}</p>
        </div>
      )}

      {/* Tabbed results — shown after first search */}
      {hasRun && (
        <div className="rounded-xl border border-border bg-card overflow-hidden">
          <TabBar active={activeTab} onSelect={setActiveTab} resultCount={enrichedResults.length} />
          <div className="p-5">
            {activeTab === "results"  && (
              <ResultsTab results={enrichedResults} stateHash={stateHash} queriedAt={queriedAt} />
            )}
            {activeTab === "proof"    && <ProofTab searchHash={stateHash} />}
            {activeTab === "timeline" && <TimelineTab />}
          </div>
        </div>
      )}

      {/* Pre-search empty state */}
      {!hasRun && !isLoading && (
        <div className="rounded-xl border border-dashed border-border px-6 py-12 text-center">
          <p className="text-sm text-muted-foreground">Enter a query vector and press Search.</p>
          <p className="mt-1 text-[11px] text-muted-foreground">
            Results, proof, and timeline will appear in tabs above.
          </p>
        </div>
      )}
    </div>
  );
}
