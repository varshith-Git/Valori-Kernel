"use client";

import { useState, useRef, useCallback, useEffect } from "react";
import Link from "next/link";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";
import { useLLMConfig, LLM_PROVIDER_DEFAULTS } from "@/lib/hooks/useLLMConfig";
import { finalizeReceipt, type AnswerReceipt, type ServerReceiptPart } from "@/lib/receipts";
import { printHtml } from "@/lib/print";
import { CopyBtn } from "@/components/ui/CopyBtn";
import { Send, FileText, File, ChevronDown, ChevronUp, Clock, Trash2, History } from "lucide-react";
import { cn } from "@/lib/utils";

interface SourceChunk {
  record_id: number;
  score: number;
  text: string | null;
  source: string | null;
  chunk_index: number | null;
  total_chunks: number | null;
}

interface GraphContextChunk {
  record_id: number;
  chunk_index: number;
  text: string;
  source: string;
}

interface AskResult {
  question: string;
  answer: string | null;
  answerError: string | null;
  sources: SourceChunk[];
  graphContext: GraphContextChunk[];
  askedAt: string;
  receipt?: AnswerReceipt;
  embedModel?: string;
  llmModel?: string | null;
  topK?: number;
  collection?: string;
}

const MAX_HISTORY = 50;

function historyKey(namespace: string) {
  return `valori:ask-history:${namespace}`;
}

function loadHistory(namespace: string): AskResult[] {
  try {
    const raw = localStorage.getItem(historyKey(namespace));
    return raw ? (JSON.parse(raw) as AskResult[]) : [];
  } catch {
    return [];
  }
}

function saveHistory(namespace: string, history: AskResult[]) {
  try {
    localStorage.setItem(historyKey(namespace), JSON.stringify(history));
  } catch {
    try {
      localStorage.setItem(
        historyKey(namespace),
        JSON.stringify(history.slice(0, Math.floor(history.length / 2)))
      );
    } catch { /* give up silently */ }
  }
}

const l2ToCosine = (score: number) => Math.max(0, Math.min(1, 1 - score / 2));

const SCORE_COLOR = (cosine: number) =>
  cosine >= 0.85 ? "text-emerald-500 dark:text-emerald-400" : cosine >= 0.7 ? "text-amber-500 dark:text-amber-400" : "text-muted-foreground";

// Group history items by date
function groupByDate(items: AskResult[]) {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);

  const groups: { label: string; items: AskResult[] }[] = [];
  const todayItems: AskResult[] = [];
  const yesterdayItems: AskResult[] = [];
  const olderItems: AskResult[] = [];

  for (const item of items) {
    const d = new Date(item.askedAt);
    d.setHours(0, 0, 0, 0);
    if (d.getTime() === today.getTime()) todayItems.push(item);
    else if (d.getTime() === yesterday.getTime()) yesterdayItems.push(item);
    else olderItems.push(item);
  }

  if (todayItems.length > 0) groups.push({ label: "Today", items: todayItems });
  if (yesterdayItems.length > 0) groups.push({ label: "Yesterday", items: yesterdayItems });
  if (olderItems.length > 0) groups.push({ label: "Earlier", items: olderItems });
  return groups;
}

function timeLabel(iso: string) {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

export function AskTab({
  namespace,
  initialQuestion,
}: {
  namespace: string;
  initialQuestion?: string;
}) {
  const { config: embedCfg } = useEmbeddingConfig();
  const { config: llmCfg } = useLLMConfig();

  const [rerankerCfg, setRerankerCfg] = useState<{ provider: string; apiKey: string; model: string; endpoint: string } | null>(null);
  useEffect(() => {
    try {
      const raw = localStorage.getItem("valori:reranker_config");
      if (raw) {
        const c = JSON.parse(raw);
        if (c.provider && c.provider !== "none") setRerankerCfg(c);
      }
    } catch {}
  }, []);

  const [treeCache, setTreeCache] = useState<{ cache_key: string; node_count: number; doc_name: string } | null>(null);
  const [treePrevHash, setTreePrevHash] = useState<string | undefined>(undefined);
  useEffect(() => {
    try {
      const raw = localStorage.getItem(`valori:tree:${namespace}`);
      if (raw) setTreeCache(JSON.parse(raw));
    } catch {}
  }, [namespace]);

  const [question, setQuestion] = useState(initialQuestion ?? "");
  const [followUp, setFollowUp] = useState("");
  const [k, setK] = useState(5);
  const [maxContextChunks, setMaxContextChunks] = useState(3);
  const [useLLM, setUseLLM] = useState(true);
  const [status, setStatus] = useState<"idle" | "embedding" | "searching" | "answering" | "done" | "error">("idle");
  const [result, setResult] = useState<AskResult | null>(null);
  const [history, setHistory] = useState<AskResult[]>([]);
  const [selectedHistoryItem, setSelectedHistoryItem] = useState<AskResult | null>(null);
  const [showAllHistory, setShowAllHistory] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (initialQuestion) {
      setQuestion(initialQuestion);
      inputRef.current?.focus();
    }
  }, [initialQuestion]);

  useEffect(() => {
    setHistory(loadHistory(namespace));
  }, [namespace]);

  useEffect(() => {
    if (history.length > 0) saveHistory(namespace, history);
  }, [namespace, history]);

  const embeddingReady =
    embedCfg.provider === "ollama" ? !!embedCfg.model : !!embedCfg.apiKey;

  const llmReady =
    llmCfg.provider === "ollama" ? !!llmCfg.model : !!llmCfg.apiKey;

  const doAsk = async (q: string) => {
    if (!q.trim()) return;

    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    const { signal } = ctrl;

    setResult(null);
    setSelectedHistoryItem(null);

    if (treeCache) {
      setStatus("searching");
      try {
        const res = await fetch("/api/tree/query", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ cache_key: treeCache.cache_key, query: q, k, prev_hash: treePrevHash }),
          signal,
        });
        if (!res.ok) throw new Error(`Tree query failed (${res.status})`);
        const data = await res.json() as {
          query: string; answer: string;
          citations: { node_id: string; title: string; breadcrumb: string; lines: [number, number] }[];
          reasoning: string;
          receipt: { receipt_hash: string; prev_hash: string; query_hash: string; answer_hash: string; hash_algo: string; timestamp: number };
        };
        setTreePrevHash(data.receipt.receipt_hash);
        if (signal.aborted) return;
        const treeResult: AskResult = {
          question: q, answer: data.answer, answerError: null,
          sources: data.citations.map((c, i) => ({
            record_id: i, score: 1,
            text: `[${c.breadcrumb}] lines ${c.lines[0]}–${c.lines[1]}`,
            source: c.title, chunk_index: null, total_chunks: null,
          })),
          graphContext: [], askedAt: new Date().toISOString(), topK: k, collection: namespace,
        };
        setResult(treeResult);
        setHistory((h) => [treeResult, ...h].slice(0, MAX_HISTORY));
        setStatus("done");
      } catch (e) {
        if (signal.aborted) return;
        setResult({ question: q, answer: null, answerError: e instanceof Error ? e.message : "Tree query failed", sources: [], graphContext: [], askedAt: new Date().toISOString() });
        setStatus("error");
      }
      return;
    }

    setStatus("embedding");
    try {
      const embedRes = await fetch("/api/embed-query", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal,
        body: JSON.stringify({ text: q, provider: embedCfg.provider, model: embedCfg.model, apiKey: embedCfg.apiKey, endpoint: embedCfg.endpoint }),
      });
      if (!embedRes.ok) {
        const e = await embedRes.json().catch(() => ({})) as { error?: string };
        throw new Error(e.error ?? `Embedding failed (${embedRes.status})`);
      }
      const { vector } = await embedRes.json() as { vector: number[] };

      if (signal.aborted) return;
      setStatus("searching");
      const whyRes = await fetch("/api/why", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal,
        body: JSON.stringify({
          query_vector: vector, k, collection: namespace, question: q,
          max_context_chunks: maxContextChunks,
          llm: useLLM && llmReady ? { provider: llmCfg.provider, model: llmCfg.model, apiKey: llmCfg.apiKey || undefined, endpoint: llmCfg.endpoint || undefined } : undefined,
          reranker: rerankerCfg ? { provider: rerankerCfg.provider, apiKey: rerankerCfg.apiKey || undefined, model: rerankerCfg.model || undefined, endpoint: rerankerCfg.endpoint || undefined } : undefined,
        }),
      });
      if (!whyRes.ok) throw new Error(`Search failed (${whyRes.status})`);

      if (useLLM && llmReady) setStatus("answering");

      const whyData = await whyRes.json() as {
        results: { record_id: number; score?: number; metadata: Record<string, unknown> | null }[];
        synthesis?: string | null; synthesis_error?: string | null;
        graph_context?: GraphContextChunk[]; receipt?: ServerReceiptPart;
      };

      const sources: SourceChunk[] = (whyData.results ?? []).map((r) => {
        const m = r.metadata ?? {};
        return {
          record_id: r.record_id, score: r.score ?? 0,
          text: (m.text as string) ?? null, source: (m.source as string) ?? null,
          chunk_index: m.chunk_index !== undefined ? (m.chunk_index as number) : null,
          total_chunks: m.total_chunks !== undefined ? (m.total_chunks as number) : null,
        };
      });

      const answer: string | null = whyData.synthesis ?? null;
      const answerError: string | null = whyData.synthesis_error ?? null;
      const graphContext: GraphContextChunk[] = whyData.graph_context ?? [];

      let receipt: AnswerReceipt | undefined;
      if (whyData.receipt) {
        try {
          receipt = await finalizeReceipt({
            server: whyData.receipt, collection: namespace, question: q, answer, k,
            embedModel: `${embedCfg.provider}/${embedCfg.model}`,
            llmModel: useLLM && llmReady ? `${llmCfg.provider}/${llmCfg.model}` : null,
          });
        } catch { /* best-effort */ }
      }

      if (signal.aborted) return;
      const r: AskResult = {
        question: q, answer, answerError, sources, graphContext,
        askedAt: new Date().toISOString(), receipt,
        embedModel: `${embedCfg.provider}/${embedCfg.model || "—"}`,
        llmModel: useLLM && llmReady ? `${llmCfg.provider}/${llmCfg.model || "—"}` : null,
        topK: k, collection: namespace,
      };
      setResult(r);
      setHistory((h) => [r, ...h].slice(0, MAX_HISTORY));
      setStatus("done");
      setQuestion("");
    } catch (e) {
      if (signal.aborted) return;
      const r: AskResult = {
        question: q, answer: null,
        answerError: e instanceof Error ? e.message : String(e),
        sources: [], graphContext: [], askedAt: new Date().toISOString(),
        embedModel: `${embedCfg.provider}/${embedCfg.model || "—"}`,
        llmModel: useLLM && llmReady ? `${llmCfg.provider}/${llmCfg.model || "—"}` : null,
        topK: k, collection: namespace,
      };
      setResult(r);
      setStatus("error");
    }
  };

  const handleSubmit = (e: React.FormEvent) => { e.preventDefault(); doAsk(question); };
  const handleFollowUp = (e: React.FormEvent) => { e.preventDefault(); if (followUp.trim()) { doAsk(followUp); setFollowUp(""); } };

  const busy = status !== "idle" && status !== "done" && status !== "error";

  const statusLabel: Record<typeof status, string> = {
    idle: "", embedding: "Embedding question…", searching: "Searching vectors…",
    answering: `Asking ${LLM_PROVIDER_DEFAULTS[llmCfg.provider]?.label ?? llmCfg.provider}/${llmCfg.model}…`,
    done: "", error: "",
  };

  const displayResult = selectedHistoryItem ?? result;

  // History display
  const historyWithoutCurrent = history.filter((h) => h !== result);
  const visibleHistory = showAllHistory ? historyWithoutCurrent : historyWithoutCurrent.slice(0, 20);
  const groups = groupByDate(history);

  return (
    <div className="flex gap-5 w-full min-h-0">
      {/* ── Main column ── */}
      <div className="flex flex-col gap-4 flex-1 min-w-0">

        {/* Config strip */}
        {!treeCache && (
          <div className="flex items-center gap-3 flex-wrap text-xs border border-border rounded-xl bg-card px-4 py-2.5">
            <span className="text-muted-foreground shrink-0">Embedding</span>
            <div className={cn(
              "flex items-center gap-1 rounded-md border px-2 py-1 font-mono cursor-default",
              embeddingReady ? "border-border text-foreground bg-background" : "border-amber-500/40 text-amber-500 bg-amber-500/5"
            )}>
              {embedCfg.provider}/{embedCfg.model || "—"}
              <ChevronDown size={11} className="text-muted-foreground ml-0.5" />
            </div>

            <span className="text-muted-foreground shrink-0">LLM</span>
            <div className={cn(
              "flex items-center gap-1 rounded-md border px-2 py-1 font-mono cursor-default",
              llmReady ? "border-border text-foreground bg-background" : "border-amber-500/40 text-amber-500 bg-amber-500/5"
            )}>
              {llmCfg.provider}/{llmCfg.model || "—"}
              <ChevronDown size={11} className="text-muted-foreground ml-0.5" />
            </div>

            {(!embeddingReady || (useLLM && !llmReady)) && (
              <Link href="/settings" className="text-amber-500 hover:text-amber-400 transition-colors text-[10px]">configure →</Link>
            )}

            <div className="ml-auto flex items-center gap-4 shrink-0">
              <label className="flex items-center gap-1.5 cursor-pointer text-muted-foreground hover:text-foreground transition-colors select-none">
                <input type="checkbox" checked={useLLM} onChange={(e) => setUseLLM(e.target.checked)} className="rounded" />
                LLM answer
              </label>

              <span className="text-muted-foreground/40">|</span>

              <label className="flex items-center gap-1.5 text-muted-foreground">
                Top
                <select value={k} onChange={(e) => setK(parseInt(e.target.value, 10))}
                  className="bg-background border border-border rounded px-1.5 py-0.5 text-foreground text-xs focus:outline-none">
                  {[3, 5, 8, 10, 15].map((n) => <option key={n} value={n}>{n}</option>)}
                </select>
                chunks
              </label>

              {useLLM && (
                <>
                  <span className="text-muted-foreground/40">—</span>
                  <label className="flex items-center gap-1.5 text-muted-foreground">
                    Sum context
                    <select value={maxContextChunks} onChange={(e) => setMaxContextChunks(parseInt(e.target.value, 10))}
                      className="bg-background border border-border rounded px-1.5 py-0.5 text-foreground text-xs focus:outline-none">
                      {[1, 2, 3, 5, 8, 10].map((n) => <option key={n} value={n}>{n}</option>)}
                    </select>
                    chunks
                  </label>
                </>
              )}
            </div>
          </div>
        )}

        {/* Tree-RAG banner */}
        {treeCache && (
          <div className="flex items-center gap-3 rounded-xl border border-[var(--v-accent)]/30 bg-[var(--v-accent-muted)] px-4 py-2.5">
            <span className="text-[var(--v-accent)] text-sm">🌳</span>
            <div className="flex-1 min-w-0">
              <p className="text-xs font-medium text-[var(--v-accent)]">Tree-RAG active</p>
              <p className="text-[11px] text-muted-foreground">{treeCache.doc_name} · {treeCache.node_count} sections</p>
            </div>
            <button onClick={() => { localStorage.removeItem(`valori:tree:${namespace}`); setTreeCache(null); setTreePrevHash(undefined); }}
              className="text-[10px] text-muted-foreground hover:text-foreground transition-colors shrink-0">× clear</button>
          </div>
        )}

        {/* Not configured warning */}
        {!treeCache && !embeddingReady && (
          <div className="rounded-xl border border-amber-500/25 bg-amber-500/10 px-4 py-3 text-xs text-amber-500">
            Configure an embedding model in{" "}
            <Link href="/settings" className="underline hover:text-amber-600">Settings</Link>{" "}
            to enable question answering.
          </div>
        )}

        {/* Question input */}
        <form onSubmit={handleSubmit} className="flex gap-2.5">
          <input
            ref={inputRef}
            type="text"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            placeholder="Ask a question about the documents in this collection…"
            disabled={busy}
            className="flex-1 rounded-xl border border-border bg-background px-4 py-3 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)] disabled:opacity-50 transition-shadow"
          />
          <button
            type="submit"
            disabled={!question.trim() || (!treeCache && !embeddingReady) || busy}
            className="rounded-xl bg-[var(--v-accent)] text-white px-5 py-3 text-sm font-medium hover:opacity-90 disabled:opacity-40 transition-opacity whitespace-nowrap flex items-center gap-2"
          >
            Ask <span className="opacity-70">→</span>
          </button>
        </form>

        {/* Status spinner */}
        {statusLabel[status] && (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-muted border-t-foreground/60" />
            {statusLabel[status]}
          </div>
        )}

        {/* Result */}
        {displayResult && (
          <ResultBlock result={displayResult} />
        )}

        {/* Follow-up input (shown after a result) */}
        {displayResult && (
          <form onSubmit={handleFollowUp} className="flex gap-2 items-center rounded-xl border border-border bg-background px-4 py-2.5">
            <input
              type="text"
              value={followUp}
              onChange={(e) => setFollowUp(e.target.value)}
              placeholder="Follow up…"
              disabled={busy}
              className="flex-1 text-sm text-foreground placeholder:text-muted-foreground bg-transparent focus:outline-none disabled:opacity-50"
            />
            <button
              type="submit"
              disabled={!followUp.trim() || busy}
              className="flex items-center justify-center w-7 h-7 rounded-lg bg-[var(--v-accent)] text-white hover:opacity-90 disabled:opacity-40 transition-opacity shrink-0"
            >
              <Send size={13} />
            </button>
          </form>
        )}
      </div>

      {/* ── History sidebar ── */}
      <div className="w-72 shrink-0 flex flex-col gap-3 border-l border-border pl-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <History size={14} className="text-muted-foreground" />
            <span className="text-sm font-medium text-foreground">History</span>
          </div>
          {history.length > 0 && (
            <button
              onClick={() => { localStorage.removeItem(historyKey(namespace)); setHistory([]); setResult(null); setSelectedHistoryItem(null); }}
              className="text-xs text-muted-foreground hover:text-red-500 transition-colors"
            >
              Clear all
            </button>
          )}
        </div>

        {history.length === 0 ? (
          <p className="text-xs text-muted-foreground text-center py-8">No history yet.</p>
        ) : (
          <div className="flex flex-col gap-4 overflow-y-auto max-h-[60vh] pr-1">
            {groups.map((group) => (
              <div key={group.label} className="flex flex-col gap-1">
                <p className="text-[10px] font-semibold text-muted-foreground uppercase tracking-widest mb-1">{group.label}</p>
                {group.items.map((item, i) => {
                  const isActive = item === (selectedHistoryItem ?? result);
                  return (
                    <button
                      key={`${item.askedAt}-${i}`}
                      onClick={() => {
                        setSelectedHistoryItem(item === selectedHistoryItem ? null : item);
                      }}
                      className={cn(
                        "w-full text-left rounded-lg px-3 py-2 text-xs transition-colors",
                        isActive
                          ? "bg-[var(--v-accent-muted)] text-[var(--v-accent)]"
                          : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
                      )}
                    >
                      <p className="truncate font-medium leading-snug">{item.question}</p>
                      <p className={cn("text-[10px] mt-0.5", isActive ? "text-[var(--v-accent)]/60" : "text-muted-foreground/60")}>
                        {timeLabel(item.askedAt)}
                      </p>
                    </button>
                  );
                })}
              </div>
            ))}

            {historyWithoutCurrent.length > 20 && !showAllHistory && (
              <button
                onClick={() => setShowAllHistory(true)}
                className="text-xs text-muted-foreground hover:text-foreground transition-colors text-center py-1 flex items-center justify-center gap-1"
              >
                <Clock size={11} /> View all history
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// -- Copy helpers --------------------------------------------------------------

function buildCopyText(result: AskResult): string {
  const lines: string[] = [];
  const when = result.askedAt ? new Date(result.askedAt).toLocaleString() : "";

  lines.push(`Question: ${result.question}`);
  if (when) lines.push(`Asked: ${when}`);
  if (result.collection) lines.push(`Collection: ${result.collection}`);
  lines.push("");
  lines.push("Configuration:");
  if (result.embedModel) lines.push(`  Embed model: ${result.embedModel}`);
  if (result.llmModel != null) lines.push(`  LLM: ${result.llmModel ?? "none (retrieval only)"}`);
  if (result.topK != null) lines.push(`  Top-K: ${result.topK}`);

  if (result.answer) {
    lines.push(""); lines.push("Answer:"); lines.push(result.answer);
  }
  if (result.answerError) {
    lines.push(""); lines.push(`Answer error: ${result.answerError}`);
  }
  if (result.sources.length > 0) {
    lines.push(""); lines.push(`Source Chunks (${result.sources.length}):`);
    result.sources.forEach((s, i) => {
      const cosine = l2ToCosine(s.score);
      const meta = [`rec #${s.record_id}`, `${(cosine * 100).toFixed(1)}% cosine`, s.source ?? null, s.chunk_index !== null ? `chunk ${s.chunk_index + 1}/${s.total_chunks ?? "?"}` : null].filter(Boolean).join(" | ");
      lines.push(`[${i + 1}] ${meta}`);
      if (s.text) lines.push(s.text);
      lines.push("");
    });
  }

  return lines.join("\n").trimEnd();
}

// -- Document icon ------------------------------------------------------------

function DocIcon({ source }: { source: string | null }) {
  const ext = source?.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "pdf") return (
    <div className="w-6 h-6 rounded-md bg-red-500/15 flex items-center justify-center shrink-0">
      <FileText size={12} className="text-red-600 dark:text-red-400" />
    </div>
  );
  return (
    <div className="w-6 h-6 rounded-md bg-emerald-500/15 flex items-center justify-center shrink-0">
      <File size={12} className="text-emerald-600 dark:text-emerald-400" />
    </div>
  );
}

// -- Proof receipt (unchanged) ------------------------------------------------

function shortHash(h: string | null | undefined, n = 12): string {
  if (!h) return "—";
  const core = h.startsWith("sha256:") ? h.slice(7) : h;
  return core.length > n + 8 ? core.slice(0, n) + "…" + core.slice(-6) : core;
}

function escHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function printReceipt(receipt: AnswerReceipt) {
  const when = new Intl.DateTimeFormat(undefined, { dateStyle: "long", timeStyle: "medium" }).format(new Date(receipt.state.captured_at));
  const chunkRows = receipt.chunks.map((c, i) =>
    `<tr><td>${i + 1}</td><td>#${escHtml(String(c.record_id))}</td><td>${c.source ? escHtml(c.source) : "—"}${c.chunk_index !== null ? ` · chunk ${escHtml(String(c.chunk_index))}` : ""}</td><td class="mono">${c.content_sha256 ? escHtml(c.content_sha256) : "(no text)"}</td></tr>`
  ).join("");

  const body = `<style>@page{margin:16mm;size:A4}*{box-sizing:border-box;margin:0;padding:0}body{font-family:'Courier New',monospace;color:#111;font-size:11px;line-height:1.5}.wrap{border:2px solid #111;padding:32px}.brand{font-size:18px;font-weight:bold;letter-spacing:3px}.sub{font-size:9px;color:#555;letter-spacing:1px;margin-top:2px}.title{text-align:center;font-size:13px;letter-spacing:4px;text-transform:uppercase;margin:22px 0;border-top:1px solid #ddd;border-bottom:1px solid #ddd;padding:10px 0}.lbl{font-size:9px;text-transform:uppercase;letter-spacing:1.5px;color:#666;margin:14px 0 4px}.box{border:1px solid #bbb;background:#f7f7f7;padding:8px 10px;word-break:break-all;font-size:10px}table{width:100%;border-collapse:collapse;margin-top:6px;font-size:9.5px}td,th{border:1px solid #ccc;padding:4px 6px;text-align:left}th{background:#eee;font-size:8px;text-transform:uppercase}.mono{word-break:break-all}.fp{border:2px solid #111;background:#f0f0f0;padding:12px;text-align:center;word-break:break-all;margin-top:16px;font-size:10px}.note{font-size:9px;color:#555;line-height:1.7;margin-top:16px;border-top:1px solid #ddd;padding-top:12px}</style>
<div class="wrap"><div class="brand">VALORI</div><div class="sub">PROOF-CARRYING ANSWER · TAMPER-EVIDENT RAG RECEIPT</div><div class="title">Answer Provenance Certificate</div><div class="lbl">Question</div><div class="box">${receipt.question.replace(/</g, "&lt;")}</div><div class="lbl">Issued</div><div class="box">${when}</div><div class="lbl">Collection</div><div class="box">${escHtml(receipt.collection)}</div><div class="lbl">Models</div><div class="box">embed: ${escHtml(receipt.models.embed)} &nbsp;|&nbsp; llm: ${receipt.models.llm ? escHtml(receipt.models.llm) : "none"}</div><div class="lbl">Answer SHA-256</div><div class="box">${receipt.answer_sha256 ? escHtml(receipt.answer_sha256) : "(no LLM answer)"}</div><div class="lbl">Global BLAKE3 State Hash</div><div class="box">${receipt.state.global_state_hash ? escHtml(receipt.state.global_state_hash) : "(unavailable)"}</div><div class="lbl">Source Chunks (${receipt.chunks.length})</div><table><thead><tr><th>#</th><th>Record</th><th>Source</th><th>Content SHA-256</th></tr></thead><tbody>${chunkRows}</tbody></table><div class="lbl">Receipt Fingerprint</div><div class="fp">${receipt.receipt_sha256 ? escHtml(receipt.receipt_sha256) : "—"}</div><div class="note"><strong>Verify independently:</strong> ${receipt.verification ? escHtml(receipt.verification) : ""}</div></div>`;
  printHtml(body, "Valori Proof-Carrying Answer");
}

function ProofReceipt({ receipt }: { receipt: AnswerReceipt }) {
  const [open, setOpen] = useState(false);
  const [showJson, setShowJson] = useState(false);
  const json = JSON.stringify(receipt, null, 2);

  const download = () => {
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a"); a.href = url;
    a.download = `valori-receipt-${receipt.receipt_sha256?.slice(7, 19) ?? Date.now()}.json`;
    a.click(); URL.revokeObjectURL(url);
  };

  return (
    <div className="border-t border-border/60">
      <button onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-accent/40 transition-colors">
        <span className="flex items-center gap-2 text-[10px] uppercase tracking-widest text-emerald-600">
          🔏 Proof receipt
          <span className="text-emerald-700 normal-case tracking-normal font-mono">{receipt.chunks.length} chunks · {shortHash(receipt.state.global_state_hash, 8)}</span>
        </span>
        <span className="text-muted-foreground text-xs">{open ? "▲" : "▼"}</span>
      </button>
      {open && (
        <div className="px-4 pb-4 flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-2">
            <div className="rounded-lg bg-background border border-border px-3 py-2">
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Global state (BLAKE3)</p>
              <p className="font-mono text-[10px] text-muted-foreground break-all">{receipt.state.global_state_hash ?? "unavailable"}</p>
            </div>
            <div className="rounded-lg bg-background border border-border px-3 py-2">
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Answer fingerprint</p>
              <p className="font-mono text-[10px] text-muted-foreground break-all">{receipt.answer_sha256 ?? "no LLM answer"}</p>
            </div>
          </div>
          <div className="rounded-lg border-2 border-emerald-900/50 bg-emerald-950/20 px-3 py-2">
            <p className="text-[9px] text-emerald-700 uppercase tracking-widest mb-1">Receipt fingerprint (SHA-256)</p>
            <p className="font-mono text-[10px] text-emerald-400/90 break-all">{receipt.receipt_sha256}</p>
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            <button onClick={download} className="text-[10px] px-2.5 py-1 rounded border border-border bg-card text-muted-foreground hover:text-foreground transition-all">download .json</button>
            <button onClick={() => printReceipt(receipt)} className="text-[10px] px-2.5 py-1 rounded border border-border bg-card text-muted-foreground hover:text-foreground transition-all">🖨 print / PDF</button>
            <CopyBtn text={receipt.receipt_sha256 ?? ""} label="copy fingerprint" />
            <button onClick={() => setShowJson((v) => !v)} className="text-[10px] px-2.5 py-1 rounded border border-border bg-card text-muted-foreground hover:text-foreground transition-all">{showJson ? "hide JSON" : "raw JSON"}</button>
          </div>
          {showJson && (
            <pre className="text-[10px] font-mono text-muted-foreground bg-background border border-border rounded-lg p-3 overflow-x-auto leading-relaxed max-h-72 overflow-y-auto">{json}</pre>
          )}
          <p className="text-[10px] text-muted-foreground leading-relaxed">{receipt.verification}</p>
        </div>
      )}
    </div>
  );
}

// -- Result block (new design) -------------------------------------------------

function ResultBlock({ result }: { result: AskResult }) {
  const [showSources, setShowSources] = useState(true);
  const [showGraphCtx, setShowGraphCtx] = useState(false);

  // Count unique source docs
  const uniqueDocs = new Set(result.sources.map((s) => s.source).filter(Boolean)).size;

  return (
    <div className="flex flex-col gap-0 rounded-xl border border-border bg-card overflow-hidden">
      {/* Answer section */}
      {(result.answer || result.answerError) && (
        <div className="px-5 py-4 border-b border-border">
          {/* Answer header */}
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-foreground">Answer</span>
              <span className="text-lg">✨</span>
              {result.topK && (
                <span className="text-xs text-muted-foreground bg-muted rounded-full px-2 py-0.5">
                  Using {result.topK} chunks
                </span>
              )}
            </div>
            <div className="flex items-center gap-3">
              {result.llmModel && (
                <span className="text-xs text-muted-foreground">
                  Generated by <span className="font-mono">{result.llmModel}</span>
                </span>
              )}
              {result.answer && <CopyBtn text={result.answer} label="Copy" />}
            </div>
          </div>

          {result.answer ? (
            <p className="text-sm text-foreground leading-relaxed whitespace-pre-wrap">
              {result.answer}
            </p>
          ) : result.answerError ? (
            <div className="rounded-lg border border-amber-500/25 bg-amber-500/10 px-4 py-3">
              <p className="text-xs text-amber-600 dark:text-amber-400">{result.answerError}</p>
            </div>
          ) : null}
        </div>
      )}

      {/* Sources section */}
      {result.sources.length > 0 && (
        <div className="px-5 py-3">
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-foreground">Sources</span>
              <span className="text-xs text-muted-foreground">
                {result.sources.length} chunk{result.sources.length !== 1 ? "s" : ""} from {uniqueDocs || result.sources.length} document{uniqueDocs !== 1 ? "s" : ""}
              </span>
            </div>
            <button
              onClick={() => setShowSources((v) => !v)}
              className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              {showSources ? (
                <><ChevronUp size={13} /> Show less</>
              ) : (
                <><ChevronDown size={13} /> Show sources</>
              )}
            </button>
          </div>

          {showSources && (
            <div className="flex flex-col gap-0 rounded-lg border border-border overflow-hidden">
              {result.sources.map((s, i) => {
                const cosine = l2ToCosine(s.score);
                const pageLabel = s.chunk_index !== null
                  ? `p. ${s.chunk_index + 1} · chunk ${s.chunk_index + 1}`
                  : null;
                const snippet = s.text ? s.text.slice(0, 80) + (s.text.length > 80 ? "…" : "") : null;

                return (
                  <div key={s.record_id} className={cn("flex items-center gap-3 px-3 py-2.5", i > 0 && "border-t border-border")}>
                    {/* Number */}
                    <span className="w-5 h-5 rounded-full bg-muted flex items-center justify-center text-[10px] font-semibold text-muted-foreground shrink-0">
                      {i + 1}
                    </span>

                    {/* Doc icon */}
                    <DocIcon source={s.source} />

                    {/* Filename */}
                    <span className="text-xs font-medium text-foreground truncate max-w-[180px] shrink-0">
                      {s.source ?? `record #${s.record_id}`}
                    </span>

                    {/* Page/chunk */}
                    {pageLabel && (
                      <span className="text-xs text-muted-foreground shrink-0">{pageLabel}</span>
                    )}

                    {/* Snippet */}
                    {snippet && (
                      <span className="text-xs text-muted-foreground truncate flex-1 min-w-0">
                        … {snippet}
                      </span>
                    )}

                    {/* Score */}
                    <span className={cn("text-xs font-mono font-semibold shrink-0 ml-auto", SCORE_COLOR(cosine))}>
                      {cosine.toFixed(3)}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Empty state */}
      {result.sources.length === 0 && !result.answerError && !result.answer && (
        <div className="px-5 py-8 text-center">
          <p className="text-sm text-muted-foreground">No matching chunks found. Try rephrasing or increasing Top-K.</p>
        </div>
      )}

      {/* Graph context */}
      {result.graphContext.length > 0 && (
        <div className="px-5 py-3 border-t border-border">
          <button onClick={() => setShowGraphCtx((v) => !v)}
            className="text-[10px] text-purple-600 uppercase tracking-widest hover:text-purple-500 transition-colors flex items-center gap-1.5 mb-2">
            ⬡ {result.graphContext.length} adjacent chunk{result.graphContext.length !== 1 ? "s" : ""} via graph
            {showGraphCtx ? <ChevronUp size={11} /> : <ChevronDown size={11} />}
          </button>
          {showGraphCtx && (
            <div className="flex flex-col gap-2">
              {result.graphContext.sort((a, b) => a.chunk_index - b.chunk_index).map((c) => (
                <div key={c.record_id} className="rounded-lg border border-purple-500/25 bg-purple-500/5 px-3 py-2">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-[10px] text-purple-600">graph neighbor</span>
                    <span className="text-[10px] text-muted-foreground">{c.source} · chunk {c.chunk_index}</span>
                  </div>
                  <p className="text-xs text-muted-foreground leading-relaxed line-clamp-3">{c.text}</p>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Proof receipt */}
      {result.receipt && <ProofReceipt receipt={result.receipt} />}
    </div>
  );
}
