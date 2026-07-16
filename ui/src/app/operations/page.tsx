"use client";

import { useEffect, useState, useRef } from "react";
import Link from "next/link";
import {
  Activity,
  CheckCircle2,
  Clock,
  Layers,
  RefreshCw,
  Search,
  AlertCircle,
  ArrowUpRight,
  Terminal,
  ChevronsUpDown,
  ChevronUp,
  ChevronDown,
  MoreHorizontal,
  SlidersHorizontal,
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
} from "lucide-react";

interface OperationSummary {
  id: string;
  type: string;
  status: string;
  timing: string;
  timestamp_unix: number;
  collection: string;
  details: Record<string, unknown>;
}

const TYPE_STYLES: Record<string, { dot: string; pill: string }> = {
  InsertRecord: {
    dot: "bg-emerald-500",
    pill: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20",
  },
  Search: {
    dot: "bg-blue-500",
    pill: "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
  },
  CreateNode: {
    dot: "bg-purple-500",
    pill: "bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20",
  },
  CreateEdge: {
    dot: "bg-pink-500",
    pill: "bg-pink-500/10 text-pink-600 dark:text-pink-400 border-pink-500/20",
  },
  DeleteRecord: {
    dot: "bg-red-500",
    pill: "bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/20",
  },
  SoftDeleteRecord: {
    dot: "bg-amber-500",
    pill: "bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/20",
  },
  SetMeta: {
    dot: "bg-zinc-400",
    pill: "bg-zinc-500/10 text-zinc-600 dark:text-zinc-400 border-zinc-500/20",
  },
};

type SortKey = "id" | "timing" | null;
type SortDir = "asc" | "desc";

const ROWS_OPTIONS = [10, 25, 50, 100];

function SortIcon({ col, sortKey, sortDir }: { col: SortKey; sortKey: SortKey; sortDir: SortDir }) {
  if (sortKey !== col) return <ChevronsUpDown className="h-3.5 w-3.5 opacity-40" />;
  return sortDir === "asc"
    ? <ChevronUp className="h-3.5 w-3.5 text-[var(--v-accent)]" />
    : <ChevronDown className="h-3.5 w-3.5 text-[var(--v-accent)]" />;
}

function RowMenu({ op }: { op: OperationSummary }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handler(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  return (
    <div ref={ref} className="relative" onClick={(e) => e.preventDefault()}>
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground transition-colors opacity-0 group-hover:opacity-100"
        aria-label="Row actions"
      >
        <MoreHorizontal className="h-4 w-4" />
      </button>
      {open && (
        <div className="absolute right-0 top-8 z-50 min-w-[10rem] rounded-lg border border-border bg-card shadow-lg py-1">
          <Link
            href={`/operations/${encodeURIComponent(op.id)}`}
            className="flex items-center gap-2 px-3 py-1.5 text-sm text-foreground hover:bg-accent/70 transition-colors"
            onClick={() => setOpen(false)}
          >
            <ArrowUpRight className="h-3.5 w-3.5" />
            View details
          </Link>
          <button className="flex w-full items-center gap-2 px-3 py-1.5 text-sm text-muted-foreground hover:bg-accent/70 transition-colors">
            <Clock className="h-3.5 w-3.5" />
            View timeline
          </button>
        </div>
      )}
    </div>
  );
}

export default function OperationsListPage() {
  const [operations, setOperations] = useState<OperationSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [eventLogEnabled, setEventLogEnabled] = useState<boolean | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [filterType, setFilterType] = useState<string>("ALL");
  const [sortKey, setSortKey] = useState<SortKey>(null);
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [page, setPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [lastRefreshed, setLastRefreshed] = useState<Date>(new Date());

  const fetchOperations = async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/operations");
      if (res.status === 503) {
        setEventLogEnabled(false);
        setOperations([]);
        return;
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      const ops: OperationSummary[] = Array.isArray(data.operations) ? data.operations : [];
      setEventLogEnabled(data.total > 0 || ops.length > 0 ? true : null);
      setOperations(ops);
      setLastRefreshed(new Date());
      setPage(1);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load operations");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { fetchOperations(); }, []);

  const types = Array.from(new Set(operations.map((o) => o.type))).sort();

  // Filter
  const filtered = operations.filter((op) => {
    if (filterType !== "ALL" && op.type !== filterType) return false;
    if (searchQuery.trim() !== "") {
      const q = searchQuery.toLowerCase();
      return (
        op.id.toLowerCase().includes(q) ||
        op.collection.toLowerCase().includes(q) ||
        op.type.toLowerCase().includes(q)
      );
    }
    return true;
  });

  // Sort
  const sorted = [...filtered].sort((a, b) => {
    if (!sortKey) return 0;
    let av = sortKey === "id" ? a.id : a.timing ?? "";
    let bv = sortKey === "id" ? b.id : b.timing ?? "";
    if (av < bv) return sortDir === "asc" ? -1 : 1;
    if (av > bv) return sortDir === "asc" ? 1 : -1;
    return 0;
  });

  // Paginate
  const totalRows = sorted.length;
  const totalPages = Math.max(1, Math.ceil(totalRows / rowsPerPage));
  const safePage = Math.min(page, totalPages);
  const pageStart = (safePage - 1) * rowsPerPage;
  const pageEnd = Math.min(pageStart + rowsPerPage, totalRows);
  const pageRows = sorted.slice(pageStart, pageEnd);

  function toggleSort(key: SortKey) {
    if (sortKey === key) setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    else { setSortKey(key); setSortDir("asc"); }
  }

  function handleFilterType(t: string) {
    setFilterType(t);
    setPage(1);
  }

  const timeStr = lastRefreshed.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

  return (
    <div className="flex flex-col gap-0 w-full max-w-[1600px]">

      {/* ── Page header ─────────────────────────────────────────── */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 pb-5 border-b border-border/60">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/20">
            <Activity className="h-[18px] w-[18px] text-[var(--v-accent)]" />
          </div>
          <div>
            <h1 className="text-xl font-bold tracking-tight text-foreground leading-none">Operations</h1>
            <p className="text-xs text-muted-foreground mt-0.5">
              Monitor real-time kernel execution, query receipts, and cryptographic verification trails.
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Live badge */}
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
            </span>
            <span className="font-medium text-emerald-600 dark:text-emerald-400">Live</span>
            <span className="text-muted-foreground/60">·</span>
            <span>{timeStr}</span>
          </div>

          <button
            onClick={fetchOperations}
            disabled={loading}
            className="flex items-center gap-1.5 rounded-lg border border-border/80 bg-card px-3 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground hover:bg-accent/60 disabled:opacity-50 transition-colors"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            Refresh
          </button>
        </div>
      </div>

      {/* ── Toolbar: search + filter chips + filter btn ──────────── */}
      <div className="flex flex-col sm:flex-row sm:items-center gap-2.5 pt-4 pb-4">
        {/* Search */}
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground/70" />
          <input
            type="text"
            placeholder="Search operations by ID, type, or collection..."
            value={searchQuery}
            onChange={(e) => { setSearchQuery(e.target.value); setPage(1); }}
            className="w-full pl-8 pr-16 py-1.5 text-sm rounded-lg border border-border/80 bg-background text-foreground placeholder:text-muted-foreground/60 focus:outline-none focus:ring-1 focus:ring-[var(--v-accent)] focus:border-[var(--v-accent)] transition-colors"
          />
          <span className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] font-medium text-muted-foreground/60 bg-muted/60 rounded px-1.5 py-0.5 pointer-events-none">⌘K</span>
        </div>

        {/* Filter chips */}
        <div className="flex items-center gap-1.5 overflow-x-auto">
          <button
            onClick={() => handleFilterType("ALL")}
            className={`shrink-0 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all ${
              filterType === "ALL"
                ? "bg-[var(--v-accent)] text-white shadow-sm"
                : "bg-background border border-border/70 text-muted-foreground hover:text-foreground hover:bg-accent/60"
            }`}
          >
            All ({operations.length})
          </button>
          {types.map((t) => {
            const count = operations.filter((o) => o.type === t).length;
            return (
              <button
                key={t}
                onClick={() => handleFilterType(t)}
                className={`shrink-0 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all ${
                  filterType === t
                    ? "bg-[var(--v-accent)] text-white shadow-sm"
                    : "bg-background border border-border/70 text-muted-foreground hover:text-foreground hover:bg-accent/60"
                }`}
              >
                {t}
              </button>
            );
          })}
        </div>

        {/* Filters btn */}
        <button className="shrink-0 flex items-center gap-1.5 rounded-lg border border-border/80 bg-background px-3 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors ml-auto">
          <SlidersHorizontal className="h-3.5 w-3.5" />
          Filters
        </button>
      </div>

      {/* ── Event log not enabled ────────────────────────────────── */}
      {!loading && !error && eventLogEnabled === false && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/8 p-5 mb-4">
          <div className="flex items-start gap-3">
            <Terminal className="h-4.5 w-4.5 text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
            <div className="flex-1 min-w-0">
              <p className="text-sm font-semibold text-amber-600 dark:text-amber-400">Event log not enabled on this node</p>
              <p className="mt-1 text-xs text-amber-700 dark:text-amber-300/80">
                Set{" "}
                <code className="font-mono bg-amber-500/15 px-1 rounded text-amber-800 dark:text-amber-200">VALORI_EVENT_LOG_PATH</code>{" "}
                and restart Valori to track operations.
              </p>
              <pre className="mt-3 rounded-lg bg-background border border-amber-500/20 px-4 py-3 text-xs text-foreground font-mono overflow-x-auto">
{`VALORI_DIM=4 \\
VALORI_CORS_ORIGIN="*" \\
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \\
cargo run -p valori-node`}
              </pre>
            </div>
          </div>
        </div>
      )}

      {/* ── Error ───────────────────────────────────────────────── */}
      {error && (
        <div className="rounded-xl border border-red-500/30 bg-red-500/8 p-4 flex items-center gap-3 text-red-500 mb-4">
          <AlertCircle className="h-4 w-4 shrink-0" />
          <p className="text-sm font-medium">{error}</p>
        </div>
      )}

      {/* ── Table ───────────────────────────────────────────────── */}
      {loading ? (
        <div className="rounded-xl border border-border/60 overflow-hidden">
          <div className="grid grid-cols-[1fr_1fr_1fr_1.6fr_7rem_2.5rem] gap-0 px-4 py-2.5 border-b border-border/60 bg-muted/30">
            {["OPERATION ID", "TYPE", "COLLECTION", "TIMING", "STATUS", ""].map((h) => (
              <span key={h} className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70">{h}</span>
            ))}
          </div>
          {[...Array(8)].map((_, i) => (
            <div key={i} className="h-[52px] border-b border-border/40 px-4 flex items-center gap-4">
              <div className="h-3 w-20 bg-muted animate-pulse rounded" />
              <div className="h-5 w-24 bg-muted animate-pulse rounded-full" />
              <div className="h-3 w-16 bg-muted animate-pulse rounded" />
              <div className="h-3 w-32 bg-muted animate-pulse rounded" />
              <div className="h-5 w-20 bg-muted animate-pulse rounded-full ml-auto" />
            </div>
          ))}
        </div>
      ) : !error && filtered.length === 0 && eventLogEnabled !== false ? (
        <div className="rounded-xl border border-dashed border-border/60 py-20 text-center bg-card/20">
          <Activity className="h-7 w-7 text-muted-foreground/30 mx-auto mb-3" />
          <p className="text-sm font-semibold text-foreground">No operations found</p>
          <p className="text-xs text-muted-foreground mt-1">
            {searchQuery || filterType !== "ALL"
              ? "Try adjusting your search or filters."
              : "Execute vector operations to generate an audit trail."}
          </p>
        </div>
      ) : !error && filtered.length > 0 ? (
        <div className="rounded-xl border border-border/60 overflow-hidden">

          {/* Column headers */}
          <div className="grid grid-cols-[1fr_1fr_1fr_1.6fr_7rem_2.5rem] bg-muted/30 border-b border-border/60">
            {/* ID — sortable */}
            <button
              onClick={() => toggleSort("id")}
              className="flex items-center gap-1.5 px-4 py-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors text-left"
            >
              Operation ID
              <SortIcon col="id" sortKey={sortKey} sortDir={sortDir} />
            </button>
            <div className="px-4 py-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Type</div>
            <div className="px-4 py-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Collection</div>
            {/* Timing — sortable */}
            <button
              onClick={() => toggleSort("timing")}
              className="flex items-center gap-1.5 px-4 py-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors text-left"
            >
              Timing
              <SortIcon col="timing" sortKey={sortKey} sortDir={sortDir} />
            </button>
            <div className="px-4 py-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Status</div>
            <div />
          </div>

          {/* Rows */}
          {pageRows.map((op, idx) => {
            const ts = TYPE_STYLES[op.type];
            return (
              <Link
                key={op.id}
                href={`/operations/${encodeURIComponent(op.id)}`}
                className={`group grid grid-cols-[1fr_1fr_1fr_1.6fr_7rem_2.5rem] items-center border-b border-border/40 last:border-b-0 hover:bg-accent/40 transition-colors duration-100 ${idx % 2 === 0 ? "" : "bg-muted/[0.025]"}`}
              >
                {/* ID */}
                <div className="flex items-center gap-1.5 px-4 py-3.5 min-w-0">
                  <span className="font-mono text-xs font-semibold text-foreground truncate group-hover:text-[var(--v-accent)] transition-colors">
                    {op.id}
                  </span>
                  <ArrowUpRight className="h-3 w-3 text-[var(--v-accent)] opacity-0 group-hover:opacity-100 shrink-0 transition-opacity" />
                </div>

                {/* Type */}
                <div className="px-4 py-3.5">
                  <span className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] font-semibold ${ts?.pill ?? "bg-muted/50 text-muted-foreground border-border/50"}`}>
                    <span className={`h-1.5 w-1.5 rounded-full ${ts?.dot ?? "bg-muted-foreground"}`} />
                    {op.type}
                  </span>
                </div>

                {/* Collection */}
                <div className="flex items-center gap-1.5 px-4 py-3.5 text-xs text-muted-foreground min-w-0">
                  <Layers className="h-3 w-3 shrink-0 opacity-60" />
                  <span className="font-mono truncate">{op.collection}</span>
                </div>

                {/* Timing */}
                <div className="flex items-center gap-1.5 px-4 py-3.5 text-xs text-muted-foreground min-w-0">
                  <Clock className="h-3 w-3 shrink-0 opacity-60" />
                  <span className="font-mono truncate">{op.timing || "—"}</span>
                </div>

                {/* Status */}
                <div className="px-4 py-3.5">
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-emerald-500/10 border border-emerald-500/20 px-2.5 py-0.5 text-[11px] font-semibold text-emerald-600 dark:text-emerald-400">
                    <CheckCircle2 className="h-3 w-3" />
                    Completed
                  </span>
                </div>

                {/* Row menu */}
                <div className="flex items-center justify-center pr-2">
                  <RowMenu op={op} />
                </div>
              </Link>
            );
          })}
        </div>
      ) : null}

      {/* ── Pagination ──────────────────────────────────────────── */}
      {!loading && !error && filtered.length > 0 && (
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pt-3 text-xs text-muted-foreground">
          <span>
            Showing {pageStart + 1} to {pageEnd} of {totalRows.toLocaleString()} results
          </span>

          <div className="flex items-center gap-4">
            {/* Rows per page */}
            <div className="flex items-center gap-2">
              <span className="whitespace-nowrap">Rows per page</span>
              <div className="relative">
                <select
                  value={rowsPerPage}
                  onChange={(e) => { setRowsPerPage(Number(e.target.value)); setPage(1); }}
                  className="appearance-none rounded-md border border-border/70 bg-background pl-2.5 pr-6 py-1 text-xs text-foreground cursor-pointer focus:outline-none focus:ring-1 focus:ring-[var(--v-accent)]"
                >
                  {ROWS_OPTIONS.map((n) => (
                    <option key={n} value={n}>{n}</option>
                  ))}
                </select>
                <ChevronsUpDown className="pointer-events-none absolute right-1.5 top-1/2 -translate-y-1/2 h-3 w-3 opacity-50" />
              </div>
            </div>

            {/* Page nav */}
            <div className="flex items-center gap-1">
              <button
                disabled={safePage === 1}
                onClick={() => setPage(1)}
                className="flex h-7 w-7 items-center justify-center rounded-md border border-border/60 hover:bg-accent/60 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                aria-label="First page"
              >
                <ChevronsLeft className="h-3.5 w-3.5" />
              </button>
              <button
                disabled={safePage === 1}
                onClick={() => setPage((p) => p - 1)}
                className="flex h-7 w-7 items-center justify-center rounded-md border border-border/60 hover:bg-accent/60 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                aria-label="Previous page"
              >
                <ChevronLeft className="h-3.5 w-3.5" />
              </button>

              {/* Page numbers */}
              {Array.from({ length: Math.min(totalPages, 5) }, (_, i) => {
                let p: number;
                if (totalPages <= 5) {
                  p = i + 1;
                } else if (safePage <= 3) {
                  p = i + 1;
                } else if (safePage >= totalPages - 2) {
                  p = totalPages - 4 + i;
                } else {
                  p = safePage - 2 + i;
                }
                return (
                  <button
                    key={p}
                    onClick={() => setPage(p)}
                    className={`flex h-7 w-7 items-center justify-center rounded-md text-xs font-medium transition-colors ${
                      p === safePage
                        ? "bg-[var(--v-accent)] text-white border border-[var(--v-accent)]"
                        : "border border-border/60 hover:bg-accent/60"
                    }`}
                  >
                    {p}
                  </button>
                );
              })}

              {totalPages > 5 && safePage < totalPages - 2 && (
                <span className="px-1 text-muted-foreground/50">…</span>
              )}
              {totalPages > 5 && safePage < totalPages - 2 && (
                <button
                  onClick={() => setPage(totalPages)}
                  className="flex h-7 w-7 items-center justify-center rounded-md border border-border/60 text-xs font-medium hover:bg-accent/60 transition-colors"
                >
                  {totalPages}
                </button>
              )}

              <button
                disabled={safePage === totalPages}
                onClick={() => setPage((p) => p + 1)}
                className="flex h-7 w-7 items-center justify-center rounded-md border border-border/60 hover:bg-accent/60 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                aria-label="Next page"
              >
                <ChevronRight className="h-3.5 w-3.5" />
              </button>
              <button
                disabled={safePage === totalPages}
                onClick={() => setPage(totalPages)}
                className="flex h-7 w-7 items-center justify-center rounded-md border border-border/60 hover:bg-accent/60 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                aria-label="Last page"
              >
                <ChevronsRight className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
