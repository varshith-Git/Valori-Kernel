"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import useSWR from "swr";
import type { ProofResponse } from "@/types/valori";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";
import { useLLMConfig } from "@/lib/hooks/useLLMConfig";

// -- Types ---------------------------------------------------------------------

interface ParsedEvent {
  index: number;
  type: string;
  recordId: number | null;
  raw: string;
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

const TYPE_COLORS: Record<string, string> = {
  INSERT: "bg-emerald-500/15 text-emerald-700 border-emerald-500/30",
  DELETE: "bg-red-500/15 text-red-700 border-red-500/30",
  SOFT_DELETE: "bg-amber-500/15 text-amber-700 border-amber-500/30",
  NODE: "bg-blue-500/15 text-blue-700 border-blue-500/30",
  EDGE: "bg-purple-500/15 text-purple-700 border-purple-500/30",
  UNKNOWN: "bg-card text-muted-foreground border-border",
};

// -- Proof banner --------------------------------------------------------------

function ProofBanner() {
  const { data, isLoading } = useSWR<ProofResponse>(
    "/api/proof",
    (url: string) => fetch(url).then((r) => r.json()),
    { refreshInterval: 5000 }
  );
  if (isLoading) return <div className="h-16 animate-pulse rounded-xl bg-accent" />;
  if (!data) return null;
  return (
    <div className="rounded-xl border border-input bg-card p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="flex flex-col gap-2">
          <p className="text-xs text-muted-foreground uppercase tracking-widest">BLAKE3 state proof</p>
          <p className="font-mono text-xs text-accent-foreground break-all">{data.final_state_hash}</p>
        </div>
        <div className="flex flex-col items-end gap-1 flex-shrink-0">
          <span className="rounded border border-emerald-500/25 bg-emerald-500/12 px-2 py-0.5 text-[10px] font-medium text-emerald-700">
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

// -- "Why this decision" panel -------------------------------------------------

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
            className={`px-4 py-1.5 transition-colors ${mode === "id" ? "bg-muted text-foreground" : "bg-card text-muted-foreground hover:text-accent-foreground"}`}
          >
            By record ID
          </button>
          <button
            onClick={() => setMode("text")}
            className={`px-4 py-1.5 border-l border-border transition-colors ${mode === "text" ? "bg-muted text-foreground" : "bg-card text-muted-foreground hover:text-accent-foreground"}`}
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
          <span className={`text-[10px] px-1.5 py-0.5 rounded border ${
            llmReady
              ? "border-emerald-800 text-emerald-500"
              : "border-amber-900 text-amber-600"
          }`}>
            {llmCfg.provider}/{llmCfg.model || "—"}
          </span>
          {!llmReady && (
            <a href="/settings" className="text-muted-foreground hover:text-accent-foreground transition-colors">
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

      {error && <p className="text-sm text-red-400 font-mono">{error}</p>}

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
          <a href="/settings" className="text-xs text-amber-700 hover:text-amber-400 transition-colors">
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

// -- Main page -----------------------------------------------------------------

export default function AuditorPage() {
  const [events, setEvents] = useState<ParsedEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(true);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [filter, setFilter] = useState<string>("ALL");
  const [activeTab, setActiveTab] = useState<"timeline" | "why" | "snapshots">("timeline");

  const loadEvents = async () => {
    setEventsLoading(true);
    setEventsError(null);
    try {
      const res = await fetch("/api/timeline");
      if (res.status === 400) { setEventsError("event-log-disabled"); return; }
      if (!res.ok) throw new Error(`${res.status}`);
      const lines: string[] = await res.json();
      setEvents(lines.map(parseEvent).reverse());
    } catch (e) {
      setEventsError(e instanceof Error ? e.message : "Failed");
    } finally {
      setEventsLoading(false);
    }
  };

  useEffect(() => { loadEvents(); }, []);

  const filtered = filter === "ALL" ? events : events.filter((e) => e.type === filter);

  return (
    <div className="flex flex-col gap-6 w-full max-w-[1600px]">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <div className="flex items-center gap-3">
            <h1 className="text-xl font-semibold text-foreground">Auditor Portal</h1>
            <span className="rounded border border-blue-500/25 bg-blue-500/12 px-2 py-0.5 text-[10px] font-medium text-blue-700 uppercase tracking-widest">
              read-only view
            </span>
          </div>
          <p className="mt-1 text-sm text-muted-foreground">
            For auditors — share this link for third-party verification of the event log
          </p>
        </div>
        <Link
          href="/audit"
          className="text-xs text-muted-foreground hover:text-accent-foreground transition-colors"
        >
          → Standard audit trail
        </Link>
      </div>

      {/* Proof */}
      <ProofBanner />

      {/* Tabs */}
      <div className="flex border-b border-border gap-1">
        {(["timeline", "why", "snapshots"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setActiveTab(t)}
            className={`px-4 py-2 text-sm transition-colors border-b-2 -mb-px ${
              activeTab === t
                ? "border-foreground text-foreground"
                : "border-transparent text-muted-foreground hover:text-accent-foreground"
            }`}
          >
            {t === "timeline" ? "Event timeline" : t === "why" ? "Why this decision?" : "Snapshots"}
          </button>
        ))}
      </div>

      {/* Timeline tab */}
      {activeTab === "timeline" && (
        <div className="flex flex-col gap-4">
          <div className="flex items-center gap-2 flex-wrap">
            {(["ALL", "INSERT", "DELETE", "SOFT_DELETE", "NODE", "EDGE"] as const).map((t) => (
              <button
                key={t}
                onClick={() => setFilter(t)}
                className={`rounded-full px-3 py-1 text-xs transition-colors border ${
                  filter === t
                    ? "bg-accent text-foreground border-border"
                    : "border-border text-muted-foreground hover:border-muted hover:text-accent-foreground"
                }`}
              >
                {t === "ALL" ? `All (${events.length})` : t}
              </button>
            ))}
            <button
              onClick={loadEvents}
              className="ml-auto text-xs text-muted-foreground hover:text-accent-foreground transition-colors"
            >
              ↻ Refresh
            </button>
          </div>

          {eventsLoading ? (
            <div className="flex flex-col gap-2 animate-pulse">
              {[1, 2, 3, 4].map((i) => <div key={i} className="h-12 rounded-lg bg-accent" />)}
            </div>
          ) : eventsError === "event-log-disabled" ? (
            <div className="rounded-xl border border-amber-500/25 bg-amber-500/12 p-5">
              <p className="text-sm text-amber-400">Event log not enabled.</p>
              <p className="text-xs text-amber-700 mt-1">
                Set <code className="font-mono">VALORI_EVENT_LOG_PATH</code> and restart.
              </p>
            </div>
          ) : filtered.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-12">No events.</p>
          ) : (
            <EventTable events={filtered} />
          )}
        </div>
      )}

      {/* Why tab */}
      {activeTab === "why" && (
        <div>
          <p className="text-sm text-muted-foreground mb-4">
            Enter a record ID or a question to see what source document it came from,
            what text chunk it represents, and when it was ingested.
            Optionally add an OpenAI key to get a natural language explanation.
          </p>
          <WhyPanel />
        </div>
      )}

      {/* Snapshots tab */}
      {activeTab === "snapshots" && <SnapshotSection />}
    </div>
  );
}

// -- Event table with inline provenance lookup ---------------------------------

function EventTable({ events }: { events: ParsedEvent[] }) {
  const [expanded, setExpanded] = useState<number | null>(null);
  const [metaCache, setMetaCache] = useState<Record<number, RecordMeta | null>>({});
  const [metaLoading, setMetaLoading] = useState<Record<number, boolean>>({});

  const loadMeta = async (recordId: number) => {
    if (metaCache[recordId] !== undefined) return;
    setMetaLoading((m) => ({ ...m, [recordId]: true }));
    try {
      const res = await fetch(`/api/meta?target_id=record:${recordId}`);
      const d = await res.json().catch(() => ({})) as { metadata?: RecordMeta };
      setMetaCache((c) => ({ ...c, [recordId]: d.metadata ?? null }));
    } catch {
      setMetaCache((c) => ({ ...c, [recordId]: null }));
    } finally {
      setMetaLoading((m) => ({ ...m, [recordId]: false }));
    }
  };

  const toggle = (e: ParsedEvent) => {
    const newId = expanded === e.index ? null : e.index;
    setExpanded(newId);
    if (newId !== null && e.recordId !== null) loadMeta(e.recordId);
  };

  return (
    <div className="flex flex-col gap-1.5">
      <div className="grid grid-cols-[3.5rem_7rem_1fr_6rem] gap-3 px-3 py-2 text-[10px] text-muted-foreground uppercase tracking-wider border-b border-border">
        <span>Event</span><span>Type</span><span>Details</span><span>Provenance</span>
      </div>
      {events.map((e) => (
        <div key={e.index}>
          <div
            role="button"
            tabIndex={0}
            className="grid grid-cols-[3.5rem_7rem_1fr_6rem] gap-3 items-center rounded-lg border border-border bg-card px-3 py-2.5 text-sm hover:border-input focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--v-accent)] transition-colors cursor-pointer"
            onClick={() => toggle(e)}
            onKeyDown={(ev) => {
              if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); toggle(e); }
            }}
          >
            <span className="font-mono text-xs text-muted-foreground">{e.index}</span>
            <span>
              <span className={`inline-block rounded border px-1.5 py-0.5 text-xs font-medium ${TYPE_COLORS[e.type] ?? TYPE_COLORS.UNKNOWN}`}>
                {e.type}
              </span>
            </span>
            <span className="text-xs text-muted-foreground font-mono truncate">
              {e.recordId !== null && <span className="text-card-foreground">Record #{e.recordId}</span>}
            </span>
            {e.recordId !== null && (
              <span className="text-[10px] text-muted-foreground hover:text-muted-foreground text-right">
                {expanded === e.index ? "hide ↑" : "source ↓"}
              </span>
            )}
          </div>

          {expanded === e.index && e.recordId !== null && (
            <div className="ml-4 mt-1 rounded-lg border border-border bg-background px-4 py-3 text-xs">
              {metaLoading[e.recordId] ? (
                <span className="text-muted-foreground">Loading provenance…</span>
              ) : metaCache[e.recordId] ? (
                <ProvenanceRow meta={metaCache[e.recordId]!} />
              ) : (
                <span className="text-muted-foreground italic">
                  No text metadata — inserted as raw vector.
                </span>
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function ProvenanceRow({ meta }: { meta: RecordMeta }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-3 flex-wrap">
        {meta.source && (
          <span className="text-blue-400 font-medium">{meta.source}</span>
        )}
        {meta.chunk_index !== undefined && (
          <span className="text-muted-foreground">
            chunk {meta.chunk_index} / {(meta.total_chunks ?? 1) - 1}
          </span>
        )}
        {meta.collection && (
          <span className="font-mono text-muted-foreground">{meta.collection}</span>
        )}
        {meta.ingested_at && (
          <span className="text-muted-foreground">{new Date(meta.ingested_at).toLocaleString()}</span>
        )}
      </div>
      {meta.text && (
        <p className="text-accent-foreground leading-relaxed bg-card rounded px-3 py-2 border border-border">
          {meta.text}
        </p>
      )}
    </div>
  );
}

// -- Snapshot section in auditor -----------------------------------------------

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
