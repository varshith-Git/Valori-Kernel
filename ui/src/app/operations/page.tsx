"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Activity, CheckCircle2, Clock, Layers, RefreshCw, Search, AlertCircle, ArrowUpRight, Terminal } from "lucide-react";

interface OperationSummary {
  id: string;
  type: string;
  status: string;
  timing: string;
  timestamp_unix: number;
  collection: string;
  details: Record<string, unknown>;
}

const TYPE_COLORS: Record<string, string> = {
  InsertRecord: "bg-emerald-500/15 text-emerald-400 border-emerald-500/30",
  Search: "bg-blue-500/15 text-blue-400 border-blue-500/30",
  CreateNode: "bg-purple-500/15 text-purple-400 border-purple-500/30",
  CreateEdge: "bg-pink-500/15 text-pink-400 border-pink-500/30",
  DeleteRecord: "bg-red-500/15 text-red-400 border-red-500/30",
  SoftDeleteRecord: "bg-amber-500/15 text-amber-400 border-amber-500/30",
};

export default function OperationsListPage() {
  const [operations, setOperations] = useState<OperationSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [eventLogEnabled, setEventLogEnabled] = useState<boolean | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [filterType, setFilterType] = useState<string>("ALL");

  const fetchOperations = async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/operations");
      if (res.status === 503) {
        // Backend unreachable — could be event log off or node down
        setEventLogEnabled(false);
        setOperations([]);
        return;
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      const ops: OperationSummary[] = Array.isArray(data.operations) ? data.operations : [];
      // If backend returned 0 and total is 0, event log is likely not enabled
      setEventLogEnabled(data.total > 0 || ops.length > 0 ? true : null);
      setOperations(ops);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load operations");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchOperations();
  }, []);

  const types = Array.from(new Set(operations.map((o) => o.type))).sort();

  const filtered = operations.filter((op) => {
    if (filterType !== "ALL" && op.type !== filterType) return false;
    if (searchQuery.trim() !== "") {
      const q = searchQuery.toLowerCase();
      const matchId = op.id.toLowerCase().includes(q);
      const matchCol = op.collection.toLowerCase().includes(q);
      const matchType = op.type.toLowerCase().includes(q);
      if (!matchId && !matchCol && !matchType) return false;
    }
    return true;
  });

  return (
    <div className="flex flex-col gap-6 w-full max-w-[1600px]">
      {/* Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 border-b border-border/80 pb-6">
        <div>
          <div className="flex items-center gap-2.5">
            <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20 shadow-sm shadow-[var(--v-accent)]/10">
              <Activity className="h-5 w-5 text-[var(--v-accent)]" />
            </div>
            <h1 className="text-2xl font-bold tracking-tight text-foreground">Operations</h1>
          </div>
          <p className="mt-1.5 text-sm text-muted-foreground">
            Monitor real-time kernel execution, query receipts, and cryptographic verification trails.
          </p>
        </div>
        <div className="flex items-center gap-2.5">
          <Button
            variant="outline"
            size="sm"
            onClick={fetchOperations}
            disabled={loading}
            className="gap-2 border-border/80 text-muted-foreground hover:text-foreground"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            Refresh
          </Button>
        </div>
      </div>

      {/* Filter and Search Bar */}
      <div className="flex flex-col sm:flex-row gap-3 items-stretch sm:items-center justify-between bg-card/60 p-3 rounded-xl border border-border/80 shadow-sm">
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <input
            type="text"
            placeholder="Search operations by ID, type, or collection..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-9 pr-4 py-1.5 rounded-lg border border-border/80 bg-background/80 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-[var(--v-accent)] transition-colors"
          />
        </div>

        <div className="flex items-center gap-2 overflow-x-auto pb-1 sm:pb-0">
          <button
            onClick={() => setFilterType("ALL")}
            className={`px-3 py-1 rounded-lg text-xs font-medium transition-all ${
              filterType === "ALL"
                ? "bg-[var(--v-accent)] text-accent-foreground shadow-sm shadow-[var(--v-accent)]/20"
                : "bg-background/80 text-muted-foreground hover:bg-accent border border-border/60"
            }`}
          >
            All ({operations.length})
          </button>
          {types.map((t) => (
            <button
              key={t}
              onClick={() => setFilterType(t)}
              className={`px-3 py-1 rounded-lg text-xs font-medium whitespace-nowrap transition-all ${
                filterType === t
                  ? "bg-[var(--v-accent)] text-accent-foreground shadow-sm shadow-[var(--v-accent)]/20"
                  : "bg-background/80 text-muted-foreground hover:bg-accent border border-border/60"
              }`}
            >
              {t}
            </button>
          ))}
        </div>
      </div>

      {/* Event log not enabled banner */}
      {!loading && !error && eventLogEnabled === false && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-6">
          <div className="flex items-start gap-3">
            <Terminal className="h-5 w-5 text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
            <div className="flex-1 min-w-0">
              <p className="text-sm font-semibold text-amber-600 dark:text-amber-400">Event log not enabled on this node</p>
              <p className="mt-1 text-xs text-amber-300/80">
                Operations are tracked through the kernel event log. Restart Valori with{" "}
                <code className="font-mono bg-amber-500/20 px-1 rounded">VALORI_EVENT_LOG_PATH</code> set:
              </p>
              <pre className="mt-3 rounded-lg bg-background/80 border border-amber-500/20 px-4 py-3 text-xs text-foreground font-mono overflow-x-auto">
{`VALORI_DIM=4 \\
VALORI_CORS_ORIGIN="*" \\
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \\
cargo run -p valori-node`}
              </pre>
            </div>
          </div>
        </div>
      )}

      {/* Content */}
      {loading ? (
        <div className="flex flex-col gap-2.5">
          {[1, 2, 3, 4, 5].map((i) => (
            <div key={i} className="h-16 animate-pulse rounded-xl bg-accent/40 border border-border/40" />
          ))}
        </div>
      ) : error ? (
        <div className="rounded-xl border border-red-500/30 bg-red-500/10 p-6 flex items-center gap-3 text-red-400">
          <AlertCircle className="h-5 w-5 shrink-0" />
          <p className="text-sm font-medium">{error}</p>
        </div>
      ) : filtered.length === 0 && eventLogEnabled !== false ? (
        <div className="rounded-xl border border-dashed border-border py-16 text-center bg-card/30">
          <Activity className="h-8 w-8 text-muted-foreground/40 mx-auto mb-2" />
          <p className="text-base font-medium text-foreground">No operations yet</p>
          <p className="text-xs text-muted-foreground mt-1">
            {searchQuery || filterType !== "ALL"
              ? "Try adjusting your search query or filters."
              : "Execute vector searches or database mutations to generate operational trails."}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <div className="grid grid-cols-[10rem_11rem_9rem_1fr_7rem] gap-4 px-4 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground border-b border-border/80">
            <span>Operation ID</span>
            <span>Type</span>
            <span>Collection</span>
            <span>Timing</span>
            <span className="text-right">Status</span>
          </div>

          {filtered.map((op) => (
            <Link
              key={op.id}
              href={`/operations/${encodeURIComponent(op.id)}`}
              className="group grid grid-cols-[10rem_11rem_9rem_1fr_7rem] gap-4 items-center rounded-xl border border-border/80 bg-card/80 px-4 py-3.5 text-sm hover:border-[var(--v-accent)] hover:bg-card hover:shadow-md hover:shadow-[var(--v-accent)]/5 transition-all duration-200"
            >
              {/* ID */}
              <div className="flex items-center gap-1.5 font-mono text-xs font-semibold text-foreground group-hover:text-[var(--v-accent)] transition-colors">
                <span className="truncate">{op.id}</span>
                <ArrowUpRight className="h-3 w-3 opacity-0 group-hover:opacity-100 transition-opacity shrink-0 text-[var(--v-accent)]" />
              </div>

              {/* Type */}
              <div>
                <span
                  className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium ${
                    TYPE_COLORS[op.type] ?? "bg-muted text-muted-foreground border-border"
                  }`}
                >
                  <span className="h-1.5 w-1.5 rounded-full bg-current" />
                  {op.type}
                </span>
              </div>

              {/* Collection */}
              <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <Layers className="h-3.5 w-3.5 shrink-0 text-muted-foreground/70" />
                <span className="truncate font-mono">{op.collection}</span>
              </div>

              {/* Timing */}
              <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <Clock className="h-3.5 w-3.5 shrink-0 text-muted-foreground/70" />
                <span className="truncate font-mono">{op.timing || "Just now"}</span>
              </div>

              {/* Status */}
              <div className="flex items-center justify-end gap-1.5">
                <span className="inline-flex items-center gap-1.5 rounded-full bg-emerald-500/10 border border-emerald-500/20 px-2.5 py-0.5 text-xs font-medium text-emerald-400">
                  <CheckCircle2 className="h-3 w-3" />
                  Completed
                </span>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
