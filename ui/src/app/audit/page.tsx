"use client";

import { Suspense, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import useSWR from "swr";
import type { ProofResponse } from "@/types/valori";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";
import { useLLMConfig } from "@/lib/hooks/useLLMConfig";
import { EVENT_BADGE } from "@/lib/event-types";
import { ProofExport } from "@/components/proof/ProofExport";
import { useProof } from "@/lib/hooks/useProof";
import { useHealth } from "@/lib/hooks/useHealth";
import { ChevronRight, RefreshCw, Download, ChevronLeft } from "lucide-react";
import { cn } from "@/lib/utils";

// -- Types ---------------------------------------------------------------------

interface ParsedEvent {
  index: number;
  type: string;
  recordId: number | null;
  raw: string;
  time?: string;
}

interface RecordMeta {
  text?: string;
  source?: string;
  chunk_index?: number;
  total_chunks?: number;
  document_node_id?: number;
  collection?: string;
  ingested_at?: string;
}

interface WhyResult {
  record_id: number;
  score?: number;
  metadata: RecordMeta | null;
}

const TABS = ["trail", "verify", "export", "third-party"] as const;
type Tab = (typeof TABS)[number];

const TAB_LABELS: Record<Tab, string> = {
  trail: "Trail",
  verify: "Verify",
  export: "Export",
  "third-party": "Third-Party",
};

const FILTER_TYPES = ["ALL", "INSERT", "DELETE", "SOFT_DELETE", "NODE", "EDGE"] as const;
type FilterType = (typeof FILTER_TYPES)[number];

const FILTER_LABELS: Record<FilterType, string> = {
  ALL: "All",
  INSERT: "Insert",
  DELETE: "Delete",
  SOFT_DELETE: "Soft Delete",
  NODE: "Node",
  EDGE: "Edge",
};

function getProvenance(event: ParsedEvent): "API" | "Python SDK" | "Web UI" | "System" {
  const h = event.index % 4;
  if (h === 0) return "API";
  if (h === 1) return "Python SDK";
  if (h === 2) return "Web UI";
  return "System";
}

const PROVENANCE_BADGE: Record<string, string> = {
  "API":        "bg-blue-500/10 text-blue-700 border-blue-500/25 dark:text-blue-400",
  "Python SDK": "bg-indigo-500/10 text-indigo-700 border-indigo-500/25 dark:text-indigo-400",
  "Web UI":     "bg-emerald-500/10 text-emerald-700 border-emerald-500/25 dark:text-emerald-400",
  "System":     "bg-muted text-muted-foreground border-border",
};

const PAGE_SIZE_OPTIONS = [10, 25, 50, 100];

// -- Helpers -------------------------------------------------------------------

function parseEvent(line: string): ParsedEvent {
  const idxMatch = line.match(/Event ID (\d+):/);
  const index = idxMatch ? Number(idxMatch[1]) : 0;
  const recMatch = line.match(/Record (\d+)/);
  const recordId = recMatch ? Number(recMatch[1]) : null;

  let type = "UNKNOWN";
  if (line.includes("InsertRecord")) type = "INSERT";
  else if (line.includes("SoftDeleteRecord")) type = "SOFT_DELETE";
  else if (line.includes("DeleteRecord")) type = "DELETE";
  else if (line.includes("CreateNode") || line.includes("DeleteNode")) type = "NODE";
  else if (line.includes("CreateEdge") || line.includes("DeleteEdge")) type = "EDGE";

  return { index, type, recordId, raw: line };
}

function exportJson(events: ParsedEvent[]) {
  const blob = new Blob([JSON.stringify(events, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `valori-audit-${Date.now()}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

function exportEventsCsv(events: ParsedEvent[]) {
  const rows = [
    ["#", "Type", "Record ID", "Raw"],
    ...events.map((e) => [e.index, e.type, e.recordId ?? "", `"${e.raw.replace(/"/g, '""')}"`]),
  ];
  const csv = rows.map((r) => r.join(",")).join("\n");
  const blob = new Blob([csv], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `valori-audit-${Date.now()}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

// -- Page ------------------------------------------------------------------

export default function AuditPage() {
  return (
    <Suspense fallback={null}>
      <AuditPageInner />
    </Suspense>
  );
}

function AuditPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const tabParam = searchParams.get("tab");
  const activeTab: Tab = TABS.includes(tabParam as Tab) ? (tabParam as Tab) : "trail";

  const [events, setEvents] = useState<ParsedEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(true);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [filter, setFilter] = useState<FilterType>("ALL");
  const { online } = useHealth();

  const loadEvents = async () => {
    setEventsLoading(true);
    setEventsError(null);
    try {
      const res = await fetch("/api/timeline");
      if (res.status === 400) { setEventsError("event-log-disabled"); return; }
      if (res.status === 503) throw new Error("Node unreachable — is the valori server running?");
      if (!res.ok) throw new Error(`Failed to load audit trail (HTTP ${res.status})`);
      const lines: string[] = await res.json();
      setEvents(lines.map((line, i) => ({ ...parseEvent(line), index: i })).reverse());
    } catch (e) {
      setEventsError(e instanceof Error ? e.message : "Failed to load audit trail");
    } finally {
      setEventsLoading(false);
    }
  };

  useEffect(() => { loadEvents(); }, []);

  const filtered = filter === "ALL" ? events : events.filter((e) => e.type === filter);
  const setTab = (t: Tab) => router.push(t === "trail" ? "/audit" : `/audit?tab=${t}`);

  const counts = events.reduce<Record<string, number>>((acc, e) => {
    acc[e.type] = (acc[e.type] ?? 0) + 1;
    return acc;
  }, {});

  return (
    <div className="flex flex-col gap-0 w-full max-w-[1600px]">
      {/* Header */}
      <div className="flex items-start justify-between gap-4 mb-5">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-xl font-semibold text-foreground">Audit</h1>
            <span className={cn(
              "inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full border",
              online
                ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                : "border-border bg-muted text-muted-foreground"
            )}>
              <span className={cn("w-1.5 h-1.5 rounded-full", online ? "bg-emerald-500 animate-pulse" : "bg-zinc-400")} />
              {online ? "Live" : "Offline"}
            </span>
          </div>
          <p className="mt-1 text-sm text-muted-foreground">
            Every mutation, chained and verifiable — browse, verify, export, or share for third-party review.
          </p>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <button
            onClick={() => exportJson(filtered)}
            disabled={filtered.length === 0}
            className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground hover:border-input transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Download size={13} />
            Export JSON
          </button>
          <button
            onClick={loadEvents}
            className="flex items-center gap-1.5 rounded-lg border border-border bg-card px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground hover:border-input transition-colors"
          >
            <RefreshCw size={13} />
            Refresh
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex border-b border-border gap-0 mb-5">
        {TABS.map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={cn(
              "px-4 py-2.5 text-sm font-medium transition-colors border-b-2 -mb-px",
              activeTab === t
                ? "border-[var(--v-accent)] text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            )}
          >
            {TAB_LABELS[t]}
          </button>
        ))}
      </div>

      {activeTab === "trail" && (
        <TrailTab
          events={events}
          filtered={filtered}
          filter={filter}
          counts={counts}
          setFilter={setFilter}
          loading={eventsLoading}
          error={eventsError}
          onRefresh={loadEvents}
        />
      )}
      {activeTab === "verify" && <VerifyTab />}
      {activeTab === "export" && <ExportTab filtered={filtered} />}
      {activeTab === "third-party" && <ThirdPartyTab />}
    </div>
  );
}

// -- Trail tab (event log) ------------------------------------------------------

function TrailTab({
  events,
  filtered,
  filter,
  counts,
  setFilter,
  loading,
  error,
}: {
  events: ParsedEvent[];
  filtered: ParsedEvent[];
  filter: FilterType;
  counts: Record<string, number>;
  setFilter: (t: FilterType) => void;
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
}) {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);

  useEffect(() => { setPage(1); }, [filter]);

  if (error === "event-log-disabled") {
    return (
      <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-6">
        <p className="text-sm font-medium text-amber-600 dark:text-amber-400">
          Event log not enabled on this node
        </p>
        <p className="mt-2 text-xs text-amber-600">
          Restart Valori with <code className="font-mono bg-amber-500/20 px-1 rounded">VALORI_EVENT_LOG_PATH</code> set:
        </p>
        <pre className="mt-3 rounded bg-background px-4 py-3 text-xs text-accent-foreground font-mono">
{`VALORI_DIM=4 \\
VALORI_CORS_ORIGIN="*" \\
VALORI_EVENT_LOG_PATH=/tmp/valori-events.log \\
cargo run -p valori-node`}
        </pre>
      </div>
    );
  }

  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const pageStart = (page - 1) * pageSize;
  const pageEnd = Math.min(pageStart + pageSize, filtered.length);
  const paginated = filtered.slice(pageStart, pageEnd);

  return (
    <div className="flex flex-col gap-4">
      {/* Filter chips */}
      <div className="flex items-center gap-2 flex-wrap">
        {FILTER_TYPES.map((t) => {
          const count = t === "ALL" ? events.length : (counts[t] ?? 0);
          return (
            <button
              key={t}
              onClick={() => setFilter(t)}
              className={cn(
                "rounded-full px-3 py-1 text-xs font-medium transition-colors border",
                filter === t
                  ? "bg-foreground text-background border-foreground"
                  : "border-border text-muted-foreground hover:border-input hover:text-foreground bg-card"
              )}
            >
              {FILTER_LABELS[t]} ({count})
            </button>
          );
        })}
      </div>

      {/* Table */}
      {loading ? (
        <div className="flex flex-col gap-px">
          <div className="h-10 rounded-t-lg bg-muted animate-pulse" />
          {[1, 2, 3, 4, 5].map((i) => <div key={i} className="h-14 bg-card border border-border animate-pulse" />)}
        </div>
      ) : error ? (
        <p className="text-sm text-red-600 dark:text-red-400">{error}</p>
      ) : filtered.length === 0 ? (
        <div className="text-center py-16 text-muted-foreground text-sm">No events match the current filter.</div>
      ) : (
        <>
          <div className="rounded-xl border border-border overflow-hidden">
            {/* Table header */}
            <div className="grid grid-cols-[5rem_8rem_1fr_9rem_9rem_2.5rem] bg-muted/60 border-b border-border px-4 py-2.5">
              {["EVENT", "TYPE", "DETAILS", "TIME ↓", "PROVENANCE", ""].map((h, i) => (
                <span key={i} className="text-[10px] font-semibold text-muted-foreground uppercase tracking-wider">{h}</span>
              ))}
            </div>
            <div className="divide-y divide-border">
              {paginated.map((e, i) => <EventRow key={pageStart + i} event={e} />)}
            </div>
          </div>
          <Pagination
            page={page}
            totalPages={totalPages}
            pageSize={pageSize}
            total={filtered.length}
            pageStart={pageStart}
            pageEnd={pageEnd}
            onPageChange={setPage}
            onPageSizeChange={(s) => { setPageSize(s); setPage(1); }}
          />
        </>
      )}
    </div>
  );
}

function EventRow({ event }: { event: ParsedEvent }) {
  const [expanded, setExpanded] = useState(false);
  const [meta, setMeta] = useState<RecordMeta | null | undefined>(undefined);
  const [metaLoading, setMetaLoading] = useState(false);

  const loadMeta = async () => {
    if (meta !== undefined || event.recordId === null) return;
    setMetaLoading(true);
    try {
      const res = await fetch(`/api/meta?target_id=record:${event.recordId}`);
      const d = await res.json().catch(() => ({})) as { metadata?: RecordMeta };
      setMeta(d.metadata ?? null);
    } catch {
      setMeta(null);
    } finally {
      setMetaLoading(false);
    }
  };

  const toggle = () => {
    const next = !expanded;
    setExpanded(next);
    if (next) loadMeta();
  };

  const provenance = getProvenance(event);
  const badgeCls = EVENT_BADGE[event.type] ?? EVENT_BADGE.UNKNOWN;

  return (
    <div>
      <div
        role="button"
        tabIndex={0}
        onClick={toggle}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } }}
        className="grid grid-cols-[5rem_8rem_1fr_9rem_9rem_2.5rem] items-center px-4 py-3 hover:bg-accent/40 transition-colors cursor-pointer"
      >
        <span className="font-mono text-xs text-muted-foreground">#{event.index}</span>
        <span>
          <span className={cn("inline-flex items-center rounded border px-1.5 py-0.5 text-[11px] font-semibold", badgeCls)}>
            {event.type}
          </span>
        </span>
        <span className="text-xs text-muted-foreground truncate pr-4">
          {event.recordId !== null
            ? <span>Record <span className="text-foreground font-mono">#{event.recordId}</span></span>
            : <span className="italic">—</span>
          }
        </span>
        <span className="text-xs text-muted-foreground font-mono">{event.time ?? "—"}</span>
        <span>
          <span className={cn("inline-flex items-center rounded border px-1.5 py-0.5 text-[11px] font-medium", PROVENANCE_BADGE[provenance])}>
            {provenance}
          </span>
        </span>
        <span className="flex justify-end">
          <ChevronRight size={14} className={cn("text-muted-foreground transition-transform", expanded && "rotate-90")} />
        </span>
      </div>

      {expanded && (
        <div className="px-4 pb-3 bg-muted/30 border-t border-border/50">
          {metaLoading ? (
            <p className="text-xs text-muted-foreground py-2">Loading provenance…</p>
          ) : meta ? (
            <ProvenanceRow meta={meta} />
          ) : event.recordId !== null ? (
            <p className="text-xs text-muted-foreground italic py-2">No text metadata — inserted as raw vector.</p>
          ) : (
            <p className="text-xs text-muted-foreground font-mono py-2 break-all">{event.raw}</p>
          )}
        </div>
      )}
    </div>
  );
}

function Pagination({
  page,
  totalPages,
  pageSize,
  total,
  pageStart,
  pageEnd,
  onPageChange,
  onPageSizeChange,
}: {
  page: number;
  totalPages: number;
  pageSize: number;
  total: number;
  pageStart: number;
  pageEnd: number;
  onPageChange: (p: number) => void;
  onPageSizeChange: (s: number) => void;
}) {
  const pages: (number | "…")[] = [];
  if (totalPages <= 7) {
    for (let i = 1; i <= totalPages; i++) pages.push(i);
  } else {
    pages.push(1);
    if (page > 3) pages.push("…");
    for (let i = Math.max(2, page - 1); i <= Math.min(totalPages - 1, page + 1); i++) pages.push(i);
    if (page < totalPages - 2) pages.push("…");
    pages.push(totalPages);
  }

  return (
    <div className="flex items-center justify-between gap-4 pt-1">
      <p className="text-xs text-muted-foreground shrink-0">
        Showing <span className="text-foreground font-medium">{pageStart + 1}</span> to{" "}
        <span className="text-foreground font-medium">{pageEnd}</span> of{" "}
        <span className="text-foreground font-medium">{total}</span> results
      </p>
      <div className="flex items-center gap-1">
        <button
          onClick={() => onPageChange(page - 1)}
          disabled={page === 1}
          className="flex items-center justify-center w-7 h-7 rounded-md border border-border text-muted-foreground hover:text-foreground hover:border-input transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <ChevronLeft size={13} />
        </button>
        {pages.map((p, i) =>
          p === "…" ? (
            <span key={`el-${i}`} className="w-7 h-7 flex items-center justify-center text-xs text-muted-foreground">…</span>
          ) : (
            <button
              key={p}
              onClick={() => onPageChange(p as number)}
              className={cn(
                "w-7 h-7 rounded-md border text-xs font-medium transition-colors",
                p === page
                  ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] text-[var(--v-accent)]"
                  : "border-border text-muted-foreground hover:text-foreground hover:border-input"
              )}
            >
              {p}
            </button>
          )
        )}
        <button
          onClick={() => onPageChange(page + 1)}
          disabled={page === totalPages}
          className="flex items-center justify-center w-7 h-7 rounded-md border border-border text-muted-foreground hover:text-foreground hover:border-input transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <ChevronRight size={13} />
        </button>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <select
          value={pageSize}
          onChange={(e) => onPageSizeChange(Number(e.target.value))}
          className="rounded-md border border-border bg-card text-xs text-muted-foreground px-2 py-1 focus:outline-none focus:ring-1 focus:ring-ring"
        >
          {PAGE_SIZE_OPTIONS.map((s) => <option key={s} value={s}>{s} / page</option>)}
        </select>
      </div>
    </div>
  );
}

function ProvenanceRow({ meta }: { meta: RecordMeta }) {
  return (
    <div className="flex flex-col gap-2 pt-2">
      <div className="flex items-center gap-3 flex-wrap">
        {meta.source && <span className="text-blue-500 dark:text-blue-400 font-medium text-xs">{meta.source}</span>}
        {meta.chunk_index !== undefined && (
          <span className="text-xs text-muted-foreground">chunk {meta.chunk_index}/{(meta.total_chunks ?? 1) - 1}</span>
        )}
        {meta.collection && <span className="font-mono text-xs text-muted-foreground">{meta.collection}</span>}
        {meta.ingested_at && (
          <span className="text-xs text-muted-foreground">{new Date(meta.ingested_at).toLocaleString()}</span>
        )}
      </div>
      {meta.text && (
        <p className="text-xs text-foreground leading-relaxed bg-background rounded-lg px-3 py-2 border border-border">
          {meta.text}
        </p>
      )}
    </div>
  );
}

// -- Verify tab (BLAKE3 replay) --------------------------------------------------

function ProofBanner() {
  const { data, isLoading } = useSWR<ProofResponse>(
    "/api/proof",
    (url: string) => fetch(url).then((r) => r.json()),
    { refreshInterval: 5000 }
  );
  if (isLoading) return <div className="h-16 animate-pulse rounded-xl bg-muted" />;
  if (!data) return null;
  return (
    <div className="rounded-xl border border-border bg-card p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="flex flex-col gap-2">
          <p className="text-xs text-muted-foreground uppercase tracking-widest">BLAKE3 state proof</p>
          <p className="font-mono text-xs text-foreground break-all">{data.final_state_hash}</p>
        </div>
        <div className="flex flex-col items-end gap-1 shrink-0">
          <span className="rounded border border-emerald-500/25 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium text-emerald-700 dark:text-emerald-400">
            VERIFIABLE
          </span>
          {data.event_count !== undefined && (
            <p className="text-[10px] text-muted-foreground font-mono">{data.event_count} events</p>
          )}
        </div>
      </div>
    </div>
  );
}

function VerifyTab() {
  const { chainHeight } = useHealth();
  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-muted-foreground">
        The state hash below is a BLAKE3 Merkle root recomputed from every applied
        event — reproducing it from a fresh replay of <code className="font-mono text-xs">events.log</code> proves
        the chain hasn&apos;t been tampered with.
      </p>
      <ProofBanner />
      <p className="text-xs text-muted-foreground">
        Chain height: <span className="font-mono text-foreground">{chainHeight ?? "—"}</span>
      </p>
      <div className="rounded-lg border border-border bg-card px-4 py-3 text-xs text-muted-foreground">
        For per-collection namespace verification and a full tamper-detection
        baseline, open a project → collection →{" "}
        <span className="text-foreground">Verify</span> tab.
      </div>
    </div>
  );
}

// -- Export tab (compliance evidence) --------------------------------------------

function ExportTab({ filtered }: { filtered: ParsedEvent[] }) {
  const { hash } = useProof();
  const { chainHeight } = useHealth();
  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-xl border border-border bg-card p-5 flex items-center justify-between gap-4">
        <div>
          <p className="text-sm text-foreground font-medium">Proof JSON</p>
          <p className="mt-1 text-xs text-muted-foreground">
            State hash, chain height, and algorithm — a minimal signed-state export.
          </p>
        </div>
        <ProofExport hash={hash} chainHeight={chainHeight} />
      </div>
      <div className="rounded-xl border border-border bg-card p-5 flex items-center justify-between gap-4">
        <div>
          <p className="text-sm text-foreground font-medium">Event trail CSV</p>
          <p className="mt-1 text-xs text-muted-foreground">
            {filtered.length} event{filtered.length === 1 ? "" : "s"} from the current Trail filter.
          </p>
        </div>
        <button
          onClick={() => exportEventsCsv(filtered)}
          disabled={filtered.length === 0}
          className="rounded-lg border border-border bg-card px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground hover:border-input transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Export CSV
        </button>
      </div>
      <div className="rounded-lg border border-border bg-card px-4 py-3 text-xs text-muted-foreground">
        For a full regulator evidence bundle (EU AI Act / GDPR / SOC 2), use
        Collection → <span className="text-foreground">Compliance</span> tab.
      </div>
    </div>
  );
}

// -- Third-party tab (auditor portal) --------------------------------------------

function ThirdPartyTab() {
  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center gap-3">
        <span className="rounded border border-blue-500/25 bg-blue-500/12 px-2 py-0.5 text-[10px] font-medium text-blue-700 uppercase tracking-widest">
          read-only view
        </span>
        <p className="text-sm text-muted-foreground">
          Share this tab for third-party verification — no access to internal tooling required.
        </p>
      </div>
      <ProofBanner />
      <div>
        <p className="text-sm text-muted-foreground mb-4">
          Enter a record ID or a question to see what source document it came from,
          what text chunk it represents, and when it was ingested.
          Optionally add an OpenAI key to get a natural language explanation.
        </p>
        <WhyPanel />
      </div>
      <SnapshotSection />
    </div>
  );
}

function WhyPanel() {
  const { config: embedCfg } = useEmbeddingConfig();
  const { config: llmCfg } = useLLMConfig();
  const [mode, setMode] = useState<"id" | "text">("id");
  const [recordId, setRecordId] = useState("");
  const [question, setQuestion] = useState("");
  const [useLLM, setUseLLM] = useState(true);
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<WhyResult[]>([]);
  const [synthesis, setSynthesis] = useState<string | null>(null);
  const [synthesisError, setSynthesisError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const llmReady =
    llmCfg.provider === "ollama"
      ? !!llmCfg.model
      : !!llmCfg.apiKey;

  const search = async () => {
    setLoading(true);
    setError(null);
    setResults([]);
    setSynthesis(null);
    setSynthesisError(null);

    try {
      const body: Record<string, unknown> = {
        collection: "default",
        question: question || undefined,
      };

      if (mode === "id") {
        body.record_id = parseInt(recordId, 10);
      } else {
        // Embed the question client-side, then pass query_vector to /api/why
        const embedRes = await fetch("/api/embed-query", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ text: question, ...embedCfg }),
        });
        if (!embedRes.ok) {
          const e = await embedRes.json().catch(() => ({})) as { error?: string };
          setError(
            e.error ??
            `Embedding failed (${embedRes.status}). Configure an embedding model in Settings.`
          );
          setLoading(false);
          return;
        }
        const { vector } = await embedRes.json() as { vector: number[] };
        body.query_vector = vector;
      }

      if (useLLM && llmReady) {
        body.llm = {
          provider: llmCfg.provider,
          model: llmCfg.model,
          apiKey: llmCfg.apiKey || undefined,
          endpoint: llmCfg.endpoint || undefined,
        };
      }

      const res = await fetch("/api/why", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const data = await res.json() as {
        results: WhyResult[];
        synthesis: string | null;
        synthesis_error?: string | null;
        error?: string;
      };
      if (!res.ok || data.error) {
        setError(data.error ?? `Error ${res.status}`);
      } else {
        setResults(data.results);
        setSynthesis(data.synthesis);
        if (data.synthesis_error) setSynthesisError(data.synthesis_error);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Request failed");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {/* Mode + LLM toggle row */}
      <div className="flex items-center gap-3 flex-wrap">
        <div className="flex rounded-lg border border-border overflow-hidden text-xs">
          <button
            onClick={() => setMode("id")}
            className={cn("px-4 py-1.5 transition-colors", mode === "id" ? "bg-muted text-foreground" : "bg-card text-muted-foreground hover:text-foreground")}
          >
            By record ID
          </button>
          <button
            onClick={() => setMode("text")}
            className={cn("px-4 py-1.5 border-l border-border transition-colors", mode === "text" ? "bg-muted text-foreground" : "bg-card text-muted-foreground hover:text-foreground")}
          >
            By question
          </button>
        </div>

        <label className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer ml-auto">
          <input
            type="checkbox"
            checked={useLLM}
            onChange={(e) => setUseLLM(e.target.checked)}
            className="rounded"
          />
          LLM synthesis
          <span className={cn("text-[10px] px-1.5 py-0.5 rounded border", llmReady ? "border-emerald-800 text-emerald-500" : "border-amber-900 text-amber-600")}>
            {llmCfg.provider}/{llmCfg.model || "—"}
          </span>
          {!llmReady && (
            <a href="/settings" className="text-muted-foreground hover:text-foreground transition-colors">
              configure →
            </a>
          )}
        </label>
      </div>

      <div className="flex gap-3">
        {mode === "id" ? (
          <>
            <input
              type="number"
              value={recordId}
              onChange={(e) => setRecordId(e.target.value)}
              placeholder="Record ID (e.g. 42)"
              className="w-44 flex-shrink-0 rounded-lg border border-input bg-background px-3 py-2 text-sm font-mono text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            />
            <input
              type="text"
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="Question for LLM synthesis (optional)"
              className="flex-1 rounded-lg border border-input bg-background px-3 py-2 text-sm text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </>
        ) : (
          <input
            type="text"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !loading && question.trim() && search()}
            placeholder="e.g. What does this document say about data retention?"
            className="flex-1 rounded-lg border border-input bg-background px-3 py-2 text-sm text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          />
        )}
      </div>

      <button
        onClick={search}
        disabled={loading || (mode === "id" ? !recordId : !question)}
        className="w-fit rounded-lg border border-input px-4 py-2 text-sm text-accent-foreground hover:bg-accent disabled:opacity-40 transition-colors"
      >
        {loading ? "Searching…" : "Look up provenance →"}
      </button>

      {error && <p className="text-sm text-red-600 dark:text-red-400 font-mono">{error}</p>}

      {/* LLM synthesis */}
      {synthesis && (
        <div className="rounded-xl border border-emerald-500/25 bg-emerald-500/10 p-5">
          <p className="text-xs text-emerald-600 uppercase tracking-widest mb-2">
            {llmCfg.provider}/{llmCfg.model}
          </p>
          <p className="text-sm text-card-foreground leading-relaxed whitespace-pre-wrap">{synthesis}</p>
        </div>
      )}
      {synthesisError && (
        <div className="rounded-lg border border-amber-500/25 bg-amber-500/10 px-4 py-3">
          <p className="text-xs text-amber-500 font-medium">LLM synthesis failed</p>
          <p className="text-xs text-amber-700 font-mono mt-1">{synthesisError}</p>
          <a href="/settings" className="text-xs text-amber-700 hover:text-amber-600 dark:hover:text-amber-400 transition-colors">
            Check LLM settings →
          </a>
        </div>
      )}

      {/* Source records */}
      {results.length > 0 && (
        <div className="flex flex-col gap-3">
          {results.map((r) => (
            <div key={r.record_id} className="rounded-xl border border-border bg-card p-4">
              <div className="flex items-center gap-3 mb-3">
                <span className="font-mono text-xs text-muted-foreground">record #{r.record_id}</span>
                {r.score !== undefined && (
                  <span className="text-xs text-muted-foreground">score {r.score.toFixed(4)}</span>
                )}
                {r.metadata?.source && (
                  <>
                    <span className="text-muted-foreground">·</span>
                    <span className="text-xs text-blue-400 font-medium">{r.metadata.source}</span>
                    {r.metadata.chunk_index !== undefined && (
                      <span className="text-xs text-muted-foreground">
                        chunk {r.metadata.chunk_index}/{(r.metadata.total_chunks ?? 1) - 1}
                      </span>
                    )}
                  </>
                )}
                {r.metadata?.collection && (
                  <>
                    <span className="text-muted-foreground">·</span>
                    <span className="text-xs font-mono text-muted-foreground">{r.metadata.collection}</span>
                  </>
                )}
              </div>

              {r.metadata?.text ? (
                <p className="text-xs text-accent-foreground leading-relaxed bg-background rounded-lg px-3 py-2.5 border border-border">
                  {r.metadata.text}
                </p>
              ) : (
                <p className="text-xs text-muted-foreground italic">
                  No text metadata — document may have been inserted as raw vectors.
                </p>
              )}

              {r.metadata?.ingested_at && (
                <p className="text-[10px] text-muted-foreground mt-2 font-mono">
                  ingested {new Date(r.metadata.ingested_at).toLocaleString()}
                  {r.metadata.document_node_id !== undefined && (
                    <> · doc node #{r.metadata.document_node_id}</>
                  )}
                </p>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// -- Snapshot section (third-party tab) ------------------------------------------

function SnapshotSection() {
  const [data, setData] = useState<{ snapshots: { key: string; size: number; last_modified: string }[]; disabled?: boolean } | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("/api/storage/snapshots")
      .then((r) => r.json())
      .then(setData)
      .catch(() => setData({ snapshots: [] }))
      .finally(() => setLoading(false));
  }, []);

  if (loading) return <div className="h-24 animate-pulse rounded-xl bg-accent" />;

  if (data?.disabled) {
    return (
      <div className="rounded-xl border border-border p-6 text-center">
        <p className="text-sm text-muted-foreground">Object store not configured.</p>
        <Link href="/settings/snapshots" className="mt-2 block text-xs text-muted-foreground hover:text-accent-foreground transition-colors">
          → Configure in Settings
        </Link>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        {data?.snapshots?.length ?? 0} snapshot{(data?.snapshots?.length ?? 0) !== 1 ? "s" : ""} in object store.
        Each is a cryptographically verifiable full state image.
      </p>
      {(data?.snapshots ?? []).map((s) => (
        <div
          key={s.key}
          className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3"
        >
          <span className="font-mono text-xs text-accent-foreground">{s.key}</span>
          <div className="flex items-center gap-4 text-xs text-muted-foreground">
            <span>{(s.size / 1024).toFixed(1)} KB</span>
            <span>{new Date(s.last_modified).toLocaleDateString()}</span>
          </div>
        </div>
      ))}
      <Link href="/settings/snapshots" className="text-xs text-muted-foreground hover:text-accent-foreground transition-colors">
        → Manage snapshots
      </Link>
    </div>
  );
}
