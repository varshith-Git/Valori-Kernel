"use client";

import { useState, useRef, useCallback, useEffect } from "react";
import Link from "next/link";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";
import { useLLMConfig, LLM_PROVIDER_DEFAULTS } from "@/lib/hooks/useLLMConfig";
import { finalizeReceipt, type AnswerReceipt, type ServerReceiptPart } from "@/lib/receipts";

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
  askedAt: string; // ISO timestamp
  receipt?: AnswerReceipt; // proof-carrying answer receipt (feature A1)
  // config snapshot captured at ask time
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
    // localStorage full — trim oldest half and retry
    try {
      localStorage.setItem(
        historyKey(namespace),
        JSON.stringify(history.slice(0, Math.floor(history.length / 2)))
      );
    } catch { /* give up silently */ }
  }
}

// Valori returns L2-squared distance (lower = closer) as score.
// For unit-normalized vectors (nomic-embed-text, OpenAI, etc.):
//   cosine_sim = 1 - score * (SCALE / 2)  where SCALE = 65536
// This gives 1.0 = identical, 0.0 = orthogonal, negative = opposite.
const l2ToCosine = (score: number) => Math.max(0, 1 - score * 32768);

const SCORE_COLOR = (cosine: number) =>
  cosine >= 0.85 ? "text-emerald-400" : cosine >= 0.7 ? "text-amber-400" : "text-muted-foreground";

export function AskTab({
  namespace,
  initialQuestion,
}: {
  namespace: string;
  initialQuestion?: string;
}) {
  const { config: embedCfg } = useEmbeddingConfig();
  const { config: llmCfg } = useLLMConfig();

  // Load reranker config from localStorage (set in Settings → Tier-2 Reranker)
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

  const [question, setQuestion] = useState(initialQuestion ?? "");
  const [k, setK] = useState(5);
  const [useLLM, setUseLLM] = useState(true);
  const [status, setStatus] = useState<"idle" | "embedding" | "searching" | "answering" | "done" | "error">("idle");
  const [result, setResult] = useState<AskResult | null>(null);
  const [history, setHistory] = useState<AskResult[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  // Pre-fill question when navigated from another tab (question suggester)
  useEffect(() => {
    if (initialQuestion) {
      setQuestion(initialQuestion);
      inputRef.current?.focus();
    }
  }, [initialQuestion]);

  // Load persisted history on mount
  useEffect(() => {
    setHistory(loadHistory(namespace));
  }, [namespace]);

  // Persist whenever history changes
  useEffect(() => {
    if (history.length > 0) saveHistory(namespace, history);
  }, [namespace, history]);

  const embeddingReady =
    embedCfg.provider === "ollama" ? !!embedCfg.model : !!embedCfg.apiKey;

  const llmReady =
    llmCfg.provider === "ollama" ? !!llmCfg.model : !!llmCfg.apiKey;

  const ask = async (q: string) => {
    if (!q.trim()) return;
    setStatus("embedding");
    setResult(null);

    try {
      // 1. Embed the question
      const embedRes = await fetch("/api/embed-query", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          text: q,
          provider: embedCfg.provider,
          model: embedCfg.model,
          apiKey: embedCfg.apiKey,
          endpoint: embedCfg.endpoint,
        }),
      });
      if (!embedRes.ok) {
        const e = await embedRes.json().catch(() => ({})) as { error?: string };
        throw new Error(e.error ?? `Embedding failed (${embedRes.status})`);
      }
      const { vector } = await embedRes.json() as { vector: number[] };

      // 2+3+4: Call /api/why which does filtered vector search + graph expansion + LLM synthesis
      setStatus("searching");
      const whyRes = await fetch("/api/why", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          query_vector: vector,
          k,
          collection: namespace,
          question: q,
          llm: useLLM && llmReady ? {
            provider: llmCfg.provider,
            model: llmCfg.model,
            apiKey: llmCfg.apiKey || undefined,
            endpoint: llmCfg.endpoint || undefined,
          } : undefined,
          reranker: rerankerCfg ? {
            provider: rerankerCfg.provider,
            apiKey: rerankerCfg.apiKey || undefined,
            model: rerankerCfg.model || undefined,
            endpoint: rerankerCfg.endpoint || undefined,
          } : undefined,
        }),
      });
      if (!whyRes.ok) throw new Error(`Search failed (${whyRes.status})`);

      if (useLLM && llmReady) setStatus("answering");

      const whyData = await whyRes.json() as {
        results: { record_id: number; score?: number; metadata: Record<string, unknown> | null }[];
        synthesis?: string | null;
        synthesis_error?: string | null;
        graph_context?: GraphContextChunk[];
        receipt?: ServerReceiptPart;
      };

      const sources: SourceChunk[] = (whyData.results ?? []).map((r) => {
        const m = r.metadata ?? {};
        return {
          record_id: r.record_id,
          score: r.score ?? 0,
          text: (m.text as string) ?? null,
          source: (m.source as string) ?? null,
          chunk_index: m.chunk_index !== undefined ? (m.chunk_index as number) : null,
          total_chunks: m.total_chunks !== undefined ? (m.total_chunks as number) : null,
        };
      });

      const answer: string | null = whyData.synthesis ?? null;
      const answerError: string | null = whyData.synthesis_error ?? null;
      const graphContext: GraphContextChunk[] = whyData.graph_context ?? [];

      // Finalize the proof-carrying receipt (server captured chunk hashes +
      // state hash; we add the question, answer hash, model identity, and the
      // self-fingerprint).
      let receipt: AnswerReceipt | undefined;
      if (whyData.receipt) {
        try {
          receipt = await finalizeReceipt({
            server: whyData.receipt,
            collection: namespace,
            question: q,
            answer,
            k,
            embedModel: `${embedCfg.provider}/${embedCfg.model}`,
            llmModel: useLLM && llmReady ? `${llmCfg.provider}/${llmCfg.model}` : null,
          });
        } catch { /* receipt is best-effort; answer still stands */ }
      }

      const r: AskResult = {
        question: q, answer, answerError, sources, graphContext,
        askedAt: new Date().toISOString(), receipt,
        embedModel: `${embedCfg.provider}/${embedCfg.model || "—"}`,
        llmModel: useLLM && llmReady ? `${llmCfg.provider}/${llmCfg.model || "—"}` : null,
        topK: k,
        collection: namespace,
      };
      setResult(r);
      setHistory((h) => [r, ...h].slice(0, MAX_HISTORY));
      setStatus("done");
      setQuestion("");
    } catch (e) {
      const r: AskResult = {
        question: q,
        answer: null,
        answerError: e instanceof Error ? e.message : String(e),
        sources: [],
        graphContext: [],
        askedAt: new Date().toISOString(),
        embedModel: `${embedCfg.provider}/${embedCfg.model || "—"}`,
        llmModel: useLLM && llmReady ? `${llmCfg.provider}/${llmCfg.model || "—"}` : null,
        topK: k,
        collection: namespace,
      };
      setResult(r);
      setStatus("error");
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    ask(question);
  };

  const statusLabel: Record<typeof status, string> = {
    idle: "",
    embedding: "Embedding question…",
    searching: "Searching vectors…",
    answering: `Asking ${LLM_PROVIDER_DEFAULTS[llmCfg.provider].label}/${llmCfg.model}…`,
    done: "",
    error: "",
  };

  return (
    <div className="flex flex-col gap-5">
      {/* Config strip */}
      <div className="flex items-center gap-3 flex-wrap text-xs">
        <span className="text-muted-foreground">Embed:</span>
        <span className={`font-mono ${embeddingReady ? "text-accent-foreground" : "text-amber-500"}`}>
          {embedCfg.provider}/{embedCfg.model || "—"}
        </span>
        {useLLM && (
          <>
            <span className="text-zinc-700">·</span>
            <span className="text-muted-foreground">LLM:</span>
            <span className={`font-mono ${llmReady ? "text-accent-foreground" : "text-amber-500"}`}>
              {llmCfg.provider}/{llmCfg.model || "—"}
            </span>
          </>
        )}
        {(!embeddingReady || (useLLM && !llmReady)) && (
          <Link href="/settings" className="text-amber-600 hover:text-amber-400 transition-colors">
            configure →
          </Link>
        )}
        <div className="ml-auto flex items-center gap-3">
          <label className="flex items-center gap-1.5 cursor-pointer text-muted-foreground hover:text-accent-foreground transition-colors">
            <input
              type="checkbox"
              checked={useLLM}
              onChange={(e) => setUseLLM(e.target.checked)}
              className="rounded"
            />
            LLM answer
          </label>
          <label className="flex items-center gap-1.5 text-muted-foreground">
            Top
            <select
              value={k}
              onChange={(e) => setK(parseInt(e.target.value, 10))}
              className="bg-card border border-input rounded px-1.5 py-0.5 text-accent-foreground text-xs focus:outline-none"
            >
              {[3, 5, 8, 10, 15].map((n) => <option key={n} value={n}>{n}</option>)}
            </select>
            chunks
          </label>
        </div>
      </div>

      {/* Not ready */}
      {!embeddingReady && (
        <div className="rounded-lg border border-amber-900 bg-amber-950/30 px-4 py-3 text-xs text-amber-500">
          Configure an embedding model in{" "}
          <Link href="/settings" className="underline hover:text-amber-300">Settings</Link>{" "}
          to enable question answering. You must use the same model that was used during ingestion.
        </div>
      )}

      {/* Input */}
      <form onSubmit={handleSubmit} className="flex gap-2">
        <input
          ref={inputRef}
          type="text"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          placeholder="Ask a question about the documents in this collection…"
          disabled={status !== "idle" && status !== "done" && status !== "error"}
          className="flex-1 rounded-lg border border-input bg-background px-4 py-2.5 text-sm text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring disabled:opacity-50"
        />
        <button
          type="submit"
          disabled={!question.trim() || !embeddingReady || (status !== "idle" && status !== "done" && status !== "error")}
          className="rounded-lg border border-input bg-card px-4 py-2.5 text-sm text-accent-foreground hover:bg-accent disabled:opacity-40 transition-colors whitespace-nowrap"
        >
          Ask →
        </button>
      </form>

      {/* Status */}
      {statusLabel[status] && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-muted border-t-zinc-300" />
          {statusLabel[status]}
        </div>
      )}

      {/* Current result */}
      {result && (
        <ResultCard result={result} />
      )}

      {/* History */}
      {history.length > 1 && (
        <div className="flex flex-col gap-3 mt-2">
          <div className="flex items-center justify-between">
            <p className="text-[10px] text-muted-foreground uppercase tracking-widest">
              History · {history.length - 1} earlier question{history.length - 1 !== 1 ? "s" : ""}
            </p>
            <button
              onClick={() => {
                const confirmed = window.confirm("Clear all question history for this collection?");
                if (confirmed) {
                  localStorage.removeItem(historyKey(namespace));
                  setHistory([]);
                  setResult(null);
                }
              }}
              className="text-[10px] text-zinc-700 hover:text-red-500 transition-colors"
            >
              clear history
            </button>
          </div>
          {history.slice(1).map((r, i) => (
            <ResultCard key={`${r.askedAt}-${i}`} result={r} collapsed />
          ))}
        </div>
      )}
    </div>
  );
}

// -- Copy helpers --------------------------------------------------------------

function buildCopyText(result: AskResult): string {
  const lines: string[] = [];
  const when = result.askedAt
    ? new Date(result.askedAt).toLocaleString()
    : "";

  lines.push(`Question: ${result.question}`);
  if (when) lines.push(`Asked: ${when}`);
  if (result.collection) lines.push(`Collection: ${result.collection}`);

  // config
  lines.push("");
  lines.push("Configuration:");
  if (result.embedModel) lines.push(`  Embed model: ${result.embedModel}`);
  if (result.llmModel != null) lines.push(`  LLM: ${result.llmModel ?? "none (retrieval only)"}`);
  if (result.topK != null) lines.push(`  Top-K: ${result.topK}`);

  // answer
  if (result.answer) {
    lines.push("");
    lines.push("Answer:");
    lines.push(result.answer);
  }
  if (result.answerError) {
    lines.push("");
    lines.push(`Answer error: ${result.answerError}`);
  }

  // source chunks
  if (result.sources.length > 0) {
    lines.push("");
    lines.push(`Source Chunks (${result.sources.length}):`);
    result.sources.forEach((s, i) => {
      const cosine = Math.max(0, 1 - s.score * 32768);
      const meta = [
        `rec #${s.record_id}`,
        `${(cosine * 100).toFixed(1)}% cosine`,
        s.source ?? null,
        s.chunk_index !== null ? `chunk ${s.chunk_index + 1}/${s.total_chunks ?? "?"}` : null,
      ].filter(Boolean).join(" | ");
      lines.push(`[${i + 1}] ${meta}`);
      if (s.text) lines.push(s.text);
      lines.push("");
    });
  }

  // graph context
  if (result.graphContext.length > 0) {
    lines.push(`Graph Context (${result.graphContext.length} adjacent nodes via knowledge graph):`);
    result.graphContext
      .slice()
      .sort((a, b) => a.chunk_index - b.chunk_index)
      .forEach((c, i) => {
        lines.push(`[${i + 1}] ${c.source} · chunk ${c.chunk_index}`);
        if (c.text) lines.push(c.text);
        lines.push("");
      });
  }

  return lines.join("\n").trimEnd();
}

// -- Copy button ---------------------------------------------------------------
function CopyBtn({ text, label = "copy", className = "" }: { text: string; label?: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = useCallback(async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [text]);
  return (
    <button
      onClick={copy}
      title="Copy to clipboard"
      className={`flex items-center gap-1 text-[10px] px-2 py-0.5 rounded border transition-colors flex-shrink-0 ${
        copied
          ? "border-emerald-700 bg-emerald-950/40 text-emerald-400"
          : "border-input bg-card text-muted-foreground hover:text-accent-foreground hover:border-ring"
      } ${className}`}
    >
      {copied ? "✓ copied" : label}
    </button>
  );
}

// -- Proof receipt panel (feature A1) ------------------------------------------

function shortHash(h: string | null | undefined, n = 12): string {
  if (!h) return "—";
  const core = h.startsWith("sha256:") ? h.slice(7) : h;
  return core.length > n + 8 ? core.slice(0, n) + "…" + core.slice(-6) : core;
}

function printReceipt(receipt: AnswerReceipt) {
  const w = window.open("", "_blank", "width=820,height=900");
  if (!w) { alert("Allow popups to print the receipt."); return; }
  const when = new Intl.DateTimeFormat(undefined, { dateStyle: "long", timeStyle: "medium" })
    .format(new Date(receipt.state.captured_at));
  const chunkRows = receipt.chunks.map((c, i) =>
    `<tr><td>${i + 1}</td><td>#${c.record_id}</td><td>${c.source ?? "—"}${
      c.chunk_index !== null ? ` · chunk ${c.chunk_index}` : ""
    }</td><td class="mono">${c.content_sha256 ?? "(no text)"}</td></tr>`
  ).join("");

  w.document.write(`<!DOCTYPE html><html><head><meta charset="UTF-8"/>
<title>Valori Proof-Carrying Answer</title>
<style>
  @page{margin:16mm;size:A4}*{box-sizing:border-box;margin:0;padding:0}
  body{font-family:'Courier New',monospace;color:#111;font-size:11px;line-height:1.5}
  .wrap{border:2px solid #111;padding:32px}
  .brand{font-size:18px;font-weight:bold;letter-spacing:3px}
  .sub{font-size:9px;color:#555;letter-spacing:1px;margin-top:2px}
  .title{text-align:center;font-size:13px;letter-spacing:4px;text-transform:uppercase;margin:22px 0;border-top:1px solid #ddd;border-bottom:1px solid #ddd;padding:10px 0}
  .lbl{font-size:9px;text-transform:uppercase;letter-spacing:1.5px;color:#666;margin:14px 0 4px}
  .box{border:1px solid #bbb;background:#f7f7f7;padding:8px 10px;word-break:break-all;font-size:10px}
  table{width:100%;border-collapse:collapse;margin-top:6px;font-size:9.5px}
  td,th{border:1px solid #ccc;padding:4px 6px;text-align:left}th{background:#eee;font-size:8px;text-transform:uppercase}
  .mono{word-break:break-all}
  .fp{border:2px solid #111;background:#f0f0f0;padding:12px;text-align:center;word-break:break-all;margin-top:16px;font-size:10px}
  .note{font-size:9px;color:#555;line-height:1.7;margin-top:16px;border-top:1px solid #ddd;padding-top:12px}
</style></head><body><div class="wrap">
  <div class="brand">VALORI</div>
  <div class="sub">PROOF-CARRYING ANSWER · TAMPER-EVIDENT RAG RECEIPT</div>
  <div class="title">Answer Provenance Certificate</div>
  <div class="lbl">Question</div><div class="box">${receipt.question.replace(/</g, "&lt;")}</div>
  <div class="lbl">Issued</div><div class="box">${when}</div>
  <div class="lbl">Collection</div><div class="box">${receipt.collection}</div>
  <div class="lbl">Models</div><div class="box">embed: ${receipt.models.embed} &nbsp;|&nbsp; llm: ${receipt.models.llm ?? "none (retrieval only)"}</div>
  <div class="lbl">Answer SHA-256</div><div class="box">${receipt.answer_sha256 ?? "(no LLM answer)"}</div>
  <div class="lbl">Global BLAKE3 State Hash (at answer time)</div><div class="box">${receipt.state.global_state_hash ?? "(unavailable)"}</div>
  <div class="lbl">Source Chunks (${receipt.chunks.length})</div>
  <table><thead><tr><th>#</th><th>Record</th><th>Source</th><th>Content SHA-256</th></tr></thead><tbody>${chunkRows}</tbody></table>
  <div class="lbl">Receipt Fingerprint</div>
  <div class="fp">${receipt.receipt_sha256 ?? "—"}</div>
  <div class="note"><strong>Verify independently:</strong> ${receipt.verification}</div>
</div></body></html>`);
  w.document.close();
  w.focus();
  setTimeout(() => w.print(), 400);
}

function ProofReceipt({ receipt }: { receipt: AnswerReceipt }) {
  const [open, setOpen] = useState(false);
  const [showJson, setShowJson] = useState(false);
  const json = JSON.stringify(receipt, null, 2);

  const download = () => {
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `valori-receipt-${receipt.receipt_sha256?.slice(7, 19) ?? Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="border-t border-border/60">
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-accent/40 transition-colors"
      >
        <span className="flex items-center gap-2 text-[10px] uppercase tracking-widest text-emerald-600">
          🔏 Proof-carrying receipt
          <span className="text-emerald-800 normal-case tracking-normal font-mono">
            {receipt.chunks.length} chunks · {shortHash(receipt.state.global_state_hash, 8)}
          </span>
        </span>
        <span className="text-muted-foreground text-xs">{open ? "▲" : "▼"}</span>
      </button>

      {open && (
        <div className="px-4 pb-4 flex flex-col gap-3">
          {/* Binding summary */}
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <div className="rounded-lg bg-background border border-border px-3 py-2">
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Global state (BLAKE3)</p>
              <p className="font-mono text-[10px] text-muted-foreground break-all">
                {receipt.state.global_state_hash ?? "unavailable"}
              </p>
            </div>
            <div className="rounded-lg bg-background border border-border px-3 py-2">
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Answer fingerprint</p>
              <p className="font-mono text-[10px] text-muted-foreground break-all">
                {receipt.answer_sha256 ?? "no LLM answer"}
              </p>
            </div>
          </div>

          {/* Chunk hashes */}
          <div className="rounded-lg bg-background border border-border overflow-hidden">
            <p className="text-[9px] text-muted-foreground uppercase tracking-widest px-3 pt-2.5 pb-1.5">
              Cited chunk content hashes
            </p>
            <div className="divide-y divide-border/60">
              {receipt.chunks.map((c, i) => (
                <div key={c.record_id} className="flex items-center gap-2 px-3 py-1.5 text-[10px]">
                  <span className="text-zinc-700 font-mono w-4">{i + 1}</span>
                  <span className="text-muted-foreground font-mono w-14">#{c.record_id}</span>
                  {c.source && <span className="text-blue-500/70 truncate max-w-[120px]">{c.source}</span>}
                  <span className="ml-auto font-mono text-muted-foreground break-all">
                    {c.content_sha256 ? shortHash(c.content_sha256, 16) : "(no text)"}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* Receipt fingerprint */}
          <div className="rounded-lg border-2 border-emerald-900/50 bg-emerald-950/20 px-3 py-2">
            <p className="text-[9px] text-emerald-700 uppercase tracking-widest mb-1">
              Receipt fingerprint (SHA-256)
            </p>
            <p className="font-mono text-[10px] text-emerald-400/90 break-all">
              {receipt.receipt_sha256}
            </p>
          </div>

          {/* Actions */}
          <div className="flex items-center gap-2 flex-wrap">
            <button
              onClick={download}
              className="text-[10px] px-2.5 py-1 rounded border border-input bg-card text-muted-foreground hover:text-foreground hover:border-ring transition-all"
            >
              download .json
            </button>
            <button
              onClick={() => printReceipt(receipt)}
              className="text-[10px] px-2.5 py-1 rounded border border-input bg-card text-muted-foreground hover:text-foreground hover:border-ring transition-all"
            >
              🖨 print / PDF
            </button>
            <CopyBtn text={receipt.receipt_sha256 ?? ""} label="copy fingerprint" />
            <button
              onClick={() => setShowJson((v) => !v)}
              className="text-[10px] px-2.5 py-1 rounded border border-input bg-card text-muted-foreground hover:text-accent-foreground transition-all"
            >
              {showJson ? "hide JSON" : "raw JSON"}
            </button>
          </div>

          {showJson && (
            <pre className="text-[10px] font-mono text-muted-foreground bg-background border border-border rounded-lg p-3 overflow-x-auto leading-relaxed max-h-72 overflow-y-auto">
              {json}
            </pre>
          )}

          <p className="text-[10px] text-zinc-700 leading-relaxed">
            {receipt.verification}
          </p>
        </div>
      )}
    </div>
  );
}

// -- Result card ---------------------------------------------------------------

function ResultCard({ result, collapsed = false }: { result: AskResult; collapsed?: boolean }) {
  const [expanded, setExpanded] = useState(!collapsed);
  const [showSources, setShowSources] = useState(!collapsed);
  const [showGraphCtx, setShowGraphCtx] = useState(false);

  const timeLabel = result.askedAt
    ? new Date(result.askedAt).toLocaleString(undefined, {
        month: "short", day: "numeric",
        hour: "2-digit", minute: "2-digit",
      })
    : null;

  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      {/* Question header */}
      <div className="flex items-start gap-3 px-4 py-3">
        <button
          onClick={() => setExpanded((v) => !v)}
          className="flex items-start gap-3 text-left flex-1 min-w-0 hover:opacity-80 transition-opacity"
        >
          <span className="text-muted-foreground text-xs mt-0.5 flex-shrink-0">Q</span>
          <span className="text-sm text-card-foreground flex-1 min-w-0">{result.question}</span>
        </button>
        <div className="flex items-center gap-2 flex-shrink-0">
          <CopyBtn text={result.question} label="copy Q" />
          {timeLabel && (
            <span className="text-[10px] text-zinc-700 tabular-nums">{timeLabel}</span>
          )}
          <button
            onClick={() => setExpanded((v) => !v)}
            className="text-muted-foreground text-xs hover:text-foreground transition-colors"
          >
            {expanded ? "▲" : "▼"}
          </button>
        </div>
      </div>

      {expanded && (
        <div className="border-t border-border">
          {/* Copy-all toolbar */}
          <div className="flex items-center justify-end gap-2 px-4 py-2 border-b border-border/50 bg-accent/20">
            <span className="text-[10px] text-muted-foreground mr-auto">
              {result.embedModel && <span className="font-mono">{result.embedModel}</span>}
              {result.llmModel != null && (
                <span className="font-mono"> · {result.llmModel ?? "retrieval only"}</span>
              )}
              {result.topK != null && (
                <span className="text-zinc-600"> · top-{result.topK}</span>
              )}
            </span>
            <CopyBtn text={buildCopyText(result)} label="copy all" />
          </div>
          {/* LLM answer */}
          {result.answer && (
            <div className="px-4 py-4 border-b border-border">
              <div className="flex items-center justify-between mb-2">
                <p className="text-[10px] text-muted-foreground uppercase tracking-widest">Answer</p>
                <CopyBtn text={result.answer} label="copy answer" />
              </div>
              <p className="text-sm text-card-foreground leading-relaxed whitespace-pre-wrap">
                {result.answer}
              </p>
            </div>
          )}

          {result.answerError && (
            <div className="px-4 py-3 border-b border-border bg-amber-950/20">
              <p className="text-xs text-amber-500">LLM unavailable: {result.answerError}</p>
            </div>
          )}

          {/* Sources */}
          {result.sources.length > 0 && (
            <div className="px-4 py-3">
              <button
                onClick={() => setShowSources((v) => !v)}
                className="text-[10px] text-muted-foreground uppercase tracking-widest hover:text-muted-foreground transition-colors flex items-center gap-1.5 mb-3"
              >
                {result.sources.length} source chunk{result.sources.length !== 1 ? "s" : ""}
                {showSources ? " ▲" : " ▼"}
              </button>

              {showSources && (
                <div className="flex flex-col gap-2.5">
                  {result.sources.map((s, i) => (
                    <div
                      key={s.record_id}
                      className="rounded-lg border border-border bg-background px-3 py-2.5"
                    >
                      <div className="flex items-center gap-2 mb-2 flex-wrap">
                        <span className="text-[10px] text-muted-foreground font-mono">#{i + 1}</span>
                        {(() => {
                          const cosine = l2ToCosine(s.score);
                          return (
                            <span className={`text-[10px] font-mono font-medium ${SCORE_COLOR(cosine)}`}
                              title={`L2² score: ${s.score.toExponential(2)}`}>
                              {(cosine * 100).toFixed(1)}% cosine
                            </span>
                          );
                        })()}
                        {s.source && (
                          <>
                            <span className="text-zinc-700">·</span>
                            <span className="text-xs text-blue-400">{s.source}</span>
                          </>
                        )}
                        {s.chunk_index !== null && (
                          <span className="text-[10px] text-muted-foreground">
                            chunk {s.chunk_index + 1}/{s.total_chunks ?? "?"}
                          </span>
                        )}
                        <span className="ml-auto text-[10px] font-mono text-zinc-700">
                          rec #{s.record_id}
                        </span>
                      </div>
                      {s.text ? (
                        <div>
                          <p className="text-xs text-muted-foreground leading-relaxed line-clamp-5">
                            {s.text}
                          </p>
                          <div className="mt-2">
                            <CopyBtn text={s.text} />
                          </div>
                        </div>
                      ) : (
                        <p className="text-xs text-zinc-700 italic">no text metadata</p>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {result.sources.length === 0 && !result.answerError && (
            <div className="px-4 py-4 text-xs text-muted-foreground">
              No matching chunks found. Try rephrasing or increasing the top-K.
            </div>
          )}

          {/* Graph-expanded context */}
          {result.graphContext.length > 0 && (
            <div className="px-4 pb-3 border-t border-border/60">
              <button
                onClick={() => setShowGraphCtx((v) => !v)}
                className="text-[10px] text-purple-700 uppercase tracking-widest hover:text-purple-400 transition-colors flex items-center gap-1.5 mt-3 mb-2"
              >
                ⬡ {result.graphContext.length} adjacent chunk{result.graphContext.length !== 1 ? "s" : ""} via graph
                {showGraphCtx ? " ▲" : " ▼"}
              </button>
              {showGraphCtx && (
                <div className="flex flex-col gap-2">
                  {result.graphContext.sort((a, b) => a.chunk_index - b.chunk_index).map((c) => (
                    <div key={c.record_id} className="rounded-lg border border-purple-900/40 bg-purple-950/20 px-3 py-2">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-[10px] text-purple-700">graph neighbor</span>
                        <span className="text-[10px] text-muted-foreground">{c.source} · chunk {c.chunk_index}</span>
                      </div>
                      <p className="text-xs text-muted-foreground leading-relaxed line-clamp-3">{c.text}</p>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Proof-carrying receipt (feature A1) */}
          {result.receipt && <ProofReceipt receipt={result.receipt} />}
        </div>
      )}
    </div>
  );
}
