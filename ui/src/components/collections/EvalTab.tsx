"use client";

import { useState, useRef, useCallback } from "react";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";

// --- Types --------------------------------------------------------------------

interface QAPair {
  id: string;
  question: string;
  expectedAnswer: string;
}

interface ChunkResult {
  recordId: number;
  score: number;       // L2² vs question vector
  relevant: boolean;   // appears in oracle (expected-answer top-K*3 search)
}

interface EvalResult {
  pair: QAPair;
  retrieved: ChunkResult[];
  precisionAtK: number;
  mrr: number;         // reciprocal rank of first oracle hit
  error?: string;
}

interface EvalRun {
  results: EvalResult[];
  k: number;
  avgPrecision: number;
  avgMrr: number;
  retrievedUnion: number[];
  allKnownIds: number[];
  orphanedIds: number[];
  runAt: string;
}

// --- Parsing ------------------------------------------------------------------

function uid() {
  return Math.random().toString(36).slice(2, 9);
}

function parseCSVLine(line: string): string[] {
  const parts: string[] = [];
  let cur = "";
  let inQ = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === '"') {
      if (inQ && line[i + 1] === '"') { cur += '"'; i++; }
      else inQ = !inQ;
    } else if (c === "," && !inQ) {
      parts.push(cur); cur = "";
    } else {
      cur += c;
    }
  }
  parts.push(cur);
  return parts;
}

function parseInput(text: string): QAPair[] {
  const t = text.trim();
  if (!t) return [];

  // JSON array or single object
  if (t.startsWith("[") || t.startsWith("{")) {
    try {
      const arr = JSON.parse(t.startsWith("{") ? `[${t}]` : t) as Record<string, unknown>[];
      return arr
        .map((item) => ({
          id: uid(),
          question: String(item.question ?? item.q ?? ""),
          expectedAnswer: String(
            item.expected_answer ?? item.answer ?? item.expectedAnswer ?? item.a ?? ""
          ),
        }))
        .filter((p) => p.question.trim());
    } catch { /* fall through to CSV */ }
  }

  // CSV (with optional header)
  const lines = t.split("\n");
  const firstLow = lines[0]?.toLowerCase() ?? "";
  const hasHeader = firstLow.includes("question") || firstLow.includes("answer");
  return lines.slice(hasHeader ? 1 : 0).flatMap((line) => {
    const parts = parseCSVLine(line);
    if (parts.length < 2 || !parts[0].trim()) return [];
    return [{ id: uid(), question: parts[0].trim(), expectedAnswer: parts[1].trim() }];
  });
}

// --- Eval engine -------------------------------------------------------------

type EmbedCfg = { provider: string; model: string; apiKey: string; endpoint: string };

async function embedOne(text: string, cfg: EmbedCfg): Promise<number[]> {
  const res = await fetch("/api/embed-query", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ text, ...cfg }),
  });
  if (!res.ok) {
    const e = await res.json().catch(() => ({})) as { error?: string };
    throw new Error(e.error ?? `Embed failed (${res.status})`);
  }
  return ((await res.json()) as { vector: number[] }).vector;
}

async function searchVec(
  vector: number[],
  k: number,
  namespace: string
): Promise<{ id: number; score: number }[]> {
  const res = await fetch("/api/search", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query: vector, k, collection: namespace }),
  });
  if (!res.ok) throw new Error(`Search failed (${res.status})`);
  return ((await res.json()) as { results: { id: number; score: number }[] }).results ?? [];
}

type ProgressFn = (label: string, step: number, total: number) => void;

async function runEvaluation(
  pairs: QAPair[],
  k: number,
  namespace: string,
  cfg: EmbedCfg,
  onProgress: ProgressFn,
  signal: AbortSignal
): Promise<EvalRun> {
  const oracleK = Math.max(k * 3, 15); // oracle is wider than question K
  const results: EvalResult[] = [];
  const retrievedUnion = new Set<number>();
  const totalSteps = pairs.length * 2 + 1;
  let step = 0;
  let anyVec: number[] | null = null; // used to build zero-vec for orphan scan

  for (const pair of pairs) {
    if (signal.aborted) throw new DOMException("Cancelled", "AbortError");
    const qlabel = pair.question.length > 48 ? pair.question.slice(0, 45) + "…" : pair.question;

    try {
      // 1. Embed question → search
      onProgress(`Embedding: "${qlabel}"`, ++step, totalSteps);
      const qVec = await embedOne(pair.question, cfg);
      anyVec ??= qVec;
      const retrieved = await searchVec(qVec, k, namespace);

      // 2. Embed expected answer → oracle
      onProgress(`Scoring oracle: "${qlabel}"`, ++step, totalSteps);
      const aVec = await embedOne(pair.expectedAnswer, cfg);
      const oracleResults = await searchVec(aVec, oracleK, namespace);
      const oracleSet = new Set(oracleResults.map((r) => r.id));

      const chunks: ChunkResult[] = retrieved.map((r) => ({
        recordId: r.id,
        score: r.score,
        relevant: oracleSet.has(r.id),
      }));

      chunks.forEach((c) => retrievedUnion.add(c.recordId));

      const nRelevant = chunks.filter((c) => c.relevant).length;
      const precisionAtK = chunks.length > 0 ? nRelevant / chunks.length : 0;
      const firstRelIdx = chunks.findIndex((c) => c.relevant);
      const mrr = firstRelIdx >= 0 ? 1 / (firstRelIdx + 1) : 0;

      results.push({ pair, retrieved: chunks, precisionAtK, mrr });
    } catch (err) {
      step += step % 2 === 1 ? 1 : 0;
      results.push({
        pair, retrieved: [], precisionAtK: 0, mrr: 0,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  }

  // Orphan detection: zero-vector search fetches all records
  onProgress("Scanning for orphaned chunks…", ++step, totalSteps);
  let allKnownIds: number[] = [];
  if (anyVec) {
    try {
      const zeroVec = Array(anyVec.length).fill(0);
      const all = await searchVec(zeroVec, 10_000, namespace);
      allKnownIds = all.map((r) => r.id);
    } catch { /* non-fatal */ }
  }

  const retrievedArr = Array.from(retrievedUnion);
  const orphanedIds = allKnownIds.filter((id) => !retrievedUnion.has(id));
  const n = results.length || 1;

  return {
    results,
    k,
    avgPrecision: results.reduce((s, r) => s + r.precisionAtK, 0) / n,
    avgMrr: results.reduce((s, r) => s + r.mrr, 0) / n,
    retrievedUnion: retrievedArr,
    allKnownIds,
    orphanedIds,
    runAt: new Date().toISOString(),
  };
}

// --- Small components ---------------------------------------------------------

function pct(n: number) {
  return (n * 100).toFixed(1) + "%";
}

function metricColor(n: number) {
  return n >= 0.7 ? "text-emerald-400" : n >= 0.4 ? "text-amber-400" : "text-red-400";
}

function MetricCard({
  label, value, sub, color,
}: { label: string; value: string; sub?: string; color?: string }) {
  return (
    <div className="rounded-xl border border-border bg-card px-5 py-4 flex flex-col gap-1">
      <span className="text-[11px] text-muted-foreground uppercase tracking-wide">{label}</span>
      <span className={`text-2xl font-mono font-semibold ${color ?? "text-foreground"}`}>{value}</span>
      {sub && <span className="text-[11px] text-muted-foreground">{sub}</span>}
    </div>
  );
}

function ChunkRow({ chunk, rank }: { chunk: ChunkResult; rank: number }) {
  const cosine = Math.max(0, (1 - chunk.score * 32768) * 100);
  return (
    <div className={`flex items-center gap-3 px-3 py-2 rounded-lg ${
      chunk.relevant
        ? "bg-emerald-950/30 border border-emerald-900/40"
        : "bg-card/40 border border-border/40"
    }`}>
      <span className="text-[11px] font-mono text-muted-foreground w-4 text-right">{rank}</span>
      <span className={`text-[11px] font-mono w-4 text-center ${
        chunk.relevant ? "text-emerald-400" : "text-muted-foreground"
      }`}>
        {chunk.relevant ? "✓" : "✗"}
      </span>
      <span className="font-mono text-xs text-muted-foreground w-12 flex-shrink-0">#{chunk.recordId}</span>
      <div className="flex-1 flex items-center gap-2 min-w-0">
        <div className="flex-1 h-1 bg-accent rounded-full overflow-hidden">
          <div
            className={`h-full rounded-full transition-all ${
              chunk.relevant ? "bg-emerald-500" : "bg-zinc-600"
            }`}
            style={{ width: `${Math.max(2, cosine)}%` }}
          />
        </div>
        <span className="text-[10px] font-mono text-muted-foreground w-10 text-right flex-shrink-0">
          {cosine.toFixed(0)}%
        </span>
      </div>
      <span className={`text-[10px] px-2 py-0.5 rounded border font-mono flex-shrink-0 ${
        chunk.relevant
          ? "border-emerald-800/60 text-emerald-400 bg-emerald-950/30"
          : "border-input text-muted-foreground bg-card"
      }`}>
        {chunk.relevant ? "relevant" : "not relevant"}
      </span>
    </div>
  );
}

function ResultCard({ result, idx, k }: { result: EvalResult; idx: number; k: number }) {
  const [open, setOpen] = useState(false);
  const pc = result.precisionAtK;
  const hit = result.retrieved.some((c) => c.relevant);

  return (
    <div className="rounded-xl border border-border overflow-hidden">
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-start gap-4 px-4 py-3 text-left hover:bg-card/60 transition-colors"
      >
        <span className="text-[11px] text-muted-foreground font-mono mt-0.5 w-6 flex-shrink-0">Q{idx + 1}</span>
        <div className="flex-1 min-w-0">
          <p className="text-sm text-card-foreground truncate">{result.pair.question}</p>
          {result.error ? (
            <p className="text-xs text-red-400 mt-0.5">{result.error}</p>
          ) : (
            <p className="text-[11px] text-muted-foreground mt-0.5 truncate">
              → {result.pair.expectedAnswer}
            </p>
          )}
        </div>
        {!result.error && (
          <div className="flex items-center gap-4 flex-shrink-0 text-[11px] font-mono">
            <span className="text-muted-foreground">
              P@{k} <span className={`font-semibold ${metricColor(pc)}`}>{pct(pc)}</span>
            </span>
            <span className="text-muted-foreground">
              MRR <span className="text-foreground font-semibold">{result.mrr.toFixed(2)}</span>
            </span>
            <span className={`px-2 py-0.5 rounded border text-[10px] ${
              hit
                ? "border-emerald-800 text-emerald-400 bg-emerald-950/30"
                : "border-input text-muted-foreground bg-card"
            }`}>
              {hit ? "hit" : "miss"}
            </span>
          </div>
        )}
        <span className="text-muted-foreground text-xs ml-2 mt-0.5 flex-shrink-0">{open ? "▲" : "▼"}</span>
      </button>

      {open && (
        <div className="border-t border-border px-4 pb-4 pt-3 flex flex-col gap-2">
          {result.error ? (
            <p className="text-sm text-red-400">{result.error}</p>
          ) : (
            <>
              <p className="text-[11px] text-muted-foreground mb-1">
                <span className="text-muted-foreground">Expected: </span>
                {result.pair.expectedAnswer.slice(0, 140)}
                {result.pair.expectedAnswer.length > 140 ? "…" : ""}
              </p>
              {result.retrieved.length === 0 ? (
                <p className="text-xs text-muted-foreground">No results returned.</p>
              ) : (
                result.retrieved.map((chunk, i) => (
                  <ChunkRow key={chunk.recordId} chunk={chunk} rank={i + 1} />
                ))
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

// --- Main component -----------------------------------------------------------

const PASTE_PLACEHOLDER = `question,expected_answer
What is the refund policy?,Items can be returned within 30 days for a full refund.
How do I track my order?,Use the tracking link sent in your confirmation email.
What payment methods are accepted?,We accept Visa, Mastercard, PayPal, and Apple Pay.`;

export function EvalTab({ namespace }: { namespace: string }) {
  const { config: embedCfg } = useEmbeddingConfig();
  const [pasteText, setPasteText] = useState("");
  const [pairs, setPairs] = useState<QAPair[]>([]);
  const [parseError, setParseError] = useState<string | null>(null);
  const [k, setK] = useState(5);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ label: string; step: number; total: number } | null>(null);
  const [evalRun, setEvalRun] = useState<EvalRun | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const handleParse = useCallback(() => {
    setParseError(null);
    try {
      const parsed = parseInput(pasteText);
      if (parsed.length === 0)
        throw new Error("No valid pairs found — use CSV with headers question,expected_answer");
      setPairs(parsed);
    } catch (e) {
      setParseError(e instanceof Error ? e.message : "Parse error");
    }
  }, [pasteText]);

  const startEval = useCallback(async () => {
    if (pairs.length === 0 || running) return;
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setRunning(true);
    setEvalRun(null);
    setProgress({ label: "Starting…", step: 0, total: pairs.length * 2 + 1 });
    try {
      const result = await runEvaluation(
        pairs, k, namespace, embedCfg,
        (label, step, total) => setProgress({ label, step, total }),
        ctrl.signal
      );
      setEvalRun(result);
    } catch (e) {
      if ((e as Error).name !== "AbortError")
        setParseError(e instanceof Error ? e.message : "Evaluation failed");
    } finally {
      setRunning(false);
      setProgress(null);
    }
  }, [pairs, k, namespace, embedCfg, running]);

  return (
    <div className="flex flex-col gap-6 max-w-3xl">

      {/* -- Ground truth input ------------------------------------------ */}
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
        <div>
          <p className="text-sm font-medium text-card-foreground">Ground Truth QA Pairs</p>
          <p className="text-xs text-muted-foreground mt-0.5">
            Paste CSV <span className="font-mono text-muted-foreground">question,expected_answer</span> or a JSON array, then click Parse
          </p>
        </div>

        <textarea
          rows={7}
          value={pasteText}
          onChange={(e) => { setPasteText(e.target.value); setParseError(null); }}
          placeholder={PASTE_PLACEHOLDER}
          className="w-full rounded-lg bg-accent border border-input text-sm text-foreground placeholder:text-muted-foreground px-3 py-2 font-mono resize-y focus:outline-none focus:border-zinc-500"
        />

        {parseError && (
          <p className="text-xs text-red-400 font-mono">{parseError}</p>
        )}

        <div className="flex items-center gap-3">
          <button
            onClick={handleParse}
            disabled={!pasteText.trim()}
            className="text-sm px-4 py-2 rounded-lg bg-muted text-foreground hover:bg-zinc-600 disabled:opacity-40 transition-colors"
          >
            Parse →
          </button>
          {pairs.length > 0 && (
            <span className="text-xs text-emerald-400 font-mono">
              ✓ {pairs.length} pair{pairs.length !== 1 ? "s" : ""} ready
            </span>
          )}
          {pairs.length > 0 && (
            <button
              onClick={() => { setPairs([]); setPasteText(""); }}
              className="text-xs text-muted-foreground hover:text-red-400 transition-colors ml-auto"
            >
              clear
            </button>
          )}
        </div>

        {/* Parsed preview table */}
        {pairs.length > 0 && (
          <div className="rounded-lg border border-input overflow-hidden">
            <table className="w-full text-xs">
              <thead>
                <tr className="bg-accent/80 border-b border-input">
                  <th className="text-left px-3 py-2 text-muted-foreground font-medium w-6">#</th>
                  <th className="text-left px-3 py-2 text-muted-foreground font-medium w-1/2">Question</th>
                  <th className="text-left px-3 py-2 text-muted-foreground font-medium">Expected answer</th>
                  <th className="w-8" />
                </tr>
              </thead>
              <tbody>
                {pairs.map((p, i) => (
                  <tr key={p.id} className={`border-b border-border/50 last:border-0 ${i % 2 === 0 ? "bg-card" : "bg-card/40"}`}>
                    <td className="px-3 py-2 text-muted-foreground font-mono">{i + 1}</td>
                    <td className="px-3 py-2 text-accent-foreground max-w-0">
                      <div className="truncate">{p.question}</div>
                    </td>
                    <td className="px-3 py-2 text-muted-foreground max-w-0">
                      <div className="truncate">{p.expectedAnswer}</div>
                    </td>
                    <td className="px-3 py-2 text-right">
                      <button
                        onClick={() => setPairs((prev) => prev.filter((x) => x.id !== p.id))}
                        className="text-zinc-700 hover:text-red-400 transition-colors text-base leading-none"
                      >
                        ×
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* -- Run config -------------------------------------------------- */}
      {pairs.length > 0 && (
        <div className="flex items-center gap-4 flex-wrap">
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted-foreground">K =</label>
            <input
              type="range" min={1} max={20} value={k}
              onChange={(e) => setK(parseInt(e.target.value))}
              className="w-28 accent-white" disabled={running}
            />
            <span className="text-sm font-mono text-accent-foreground w-4">{k}</span>
          </div>
          <span className="text-[11px] text-muted-foreground">
            oracle: top-{k * 3} from expected-answer embedding ·{" "}
            using <span className="text-muted-foreground">{embedCfg.provider}/{embedCfg.model}</span>
          </span>
          <div className="flex-1" />
          {running ? (
            <button
              onClick={() => abortRef.current?.abort()}
              className="text-sm px-4 py-2 rounded-lg border border-input text-muted-foreground hover:text-red-400 hover:border-red-900 transition-colors"
            >
              Cancel
            </button>
          ) : (
            <button
              onClick={startEval}
              className="text-sm px-5 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 font-medium transition-colors"
            >
              Run Evaluation →
            </button>
          )}
        </div>
      )}

      {/* -- Progress ---------------------------------------------------- */}
      {progress && (
        <div className="rounded-xl border border-border bg-card px-5 py-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-sm text-accent-foreground font-mono truncate pr-4">{progress.label}</p>
            <span className="text-xs text-muted-foreground font-mono flex-shrink-0">
              {progress.step}/{progress.total}
            </span>
          </div>
          <div className="h-1.5 bg-accent rounded-full overflow-hidden">
            <div
              className="h-full bg-sky-500 rounded-full transition-all duration-300"
              style={{ width: `${Math.round((progress.step / progress.total) * 100)}%` }}
            />
          </div>
        </div>
      )}

      {/* -- Results ----------------------------------------------------- */}
      {evalRun && (
        <>
          {/* Summary cards */}
          <div>
            <p className="text-[11px] text-muted-foreground mb-3 font-mono">
              {evalRun.results.length} question{evalRun.results.length !== 1 ? "s" : ""} · K={evalRun.k} · oracle K={evalRun.k * 3} ·{" "}
              {new Date(evalRun.runAt).toLocaleTimeString()}
            </p>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <MetricCard
                label={`Avg Precision@${evalRun.k}`}
                value={pct(evalRun.avgPrecision)}
                sub="of retrieved chunks were oracle-relevant"
                color={metricColor(evalRun.avgPrecision)}
              />
              <MetricCard
                label="Avg MRR"
                value={evalRun.avgMrr.toFixed(3)}
                sub="mean reciprocal rank of first oracle hit"
                color={metricColor(evalRun.avgMrr)}
              />
              <MetricCard
                label="Hit rate"
                value={pct(
                  evalRun.results.filter((r) => r.retrieved.some((c) => c.relevant)).length /
                    (evalRun.results.length || 1)
                )}
                sub="queries that got ≥1 relevant chunk"
                color={metricColor(
                  evalRun.results.filter((r) => r.retrieved.some((c) => c.relevant)).length /
                    (evalRun.results.length || 1)
                )}
              />
              <MetricCard
                label="Chunk coverage"
                value={
                  evalRun.allKnownIds.length > 0
                    ? pct(evalRun.retrievedUnion.length / evalRun.allKnownIds.length)
                    : "—"
                }
                sub={`${evalRun.retrievedUnion.length} of ${evalRun.allKnownIds.length} chunks seen`}
                color={
                  evalRun.allKnownIds.length === 0
                    ? "text-muted-foreground"
                    : metricColor(evalRun.retrievedUnion.length / evalRun.allKnownIds.length)
                }
              />
            </div>
          </div>

          {/* Per-question accordion */}
          <div className="flex flex-col gap-2">
            <p className="text-[11px] text-muted-foreground uppercase tracking-widest font-medium">
              Per-question breakdown
            </p>
            <p className="text-[11px] text-muted-foreground">
              A chunk is <span className="text-emerald-400">relevant</span> if it appears in the top-{evalRun.k * 3} results when searching with the expected answer embedding. Click any row to expand.
            </p>
            {evalRun.results.map((result, i) => (
              <ResultCard key={result.pair.id} result={result} idx={i} k={evalRun.k} />
            ))}
          </div>

          {/* Orphaned chunks */}
          {evalRun.allKnownIds.length > 0 && (
            <div className={`rounded-xl border p-5 flex flex-col gap-4 ${
              evalRun.orphanedIds.length > 0
                ? "border-amber-900/70 bg-amber-950/20"
                : "border-border bg-card"
            }`}>
              <div className="flex items-start justify-between gap-4">
                <div>
                  <p className={`text-sm font-semibold ${
                    evalRun.orphanedIds.length > 0 ? "text-amber-400" : "text-emerald-400"
                  }`}>
                    {evalRun.orphanedIds.length > 0
                      ? `${evalRun.orphanedIds.length} orphaned chunk${evalRun.orphanedIds.length !== 1 ? "s" : ""}`
                      : "No orphaned chunks — full coverage"}
                  </p>
                  <p className="text-xs text-muted-foreground mt-1">
                    {evalRun.orphanedIds.length > 0
                      ? `${pct(evalRun.orphanedIds.length / evalRun.allKnownIds.length)} of indexed records were never retrieved across all ${evalRun.results.length} queries. Consider re-chunking, removing, or expanding your test set.`
                      : "Every indexed chunk was touched by at least one query in this evaluation."}
                  </p>
                </div>
                {evalRun.orphanedIds.length > 0 && (
                  <span className="text-xs font-mono text-amber-500 bg-amber-950/50 px-2 py-1 rounded border border-amber-900/60 flex-shrink-0">
                    {pct(evalRun.orphanedIds.length / evalRun.allKnownIds.length)} dead weight
                  </span>
                )}
              </div>

              {evalRun.orphanedIds.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {evalRun.orphanedIds.slice(0, 120).map((id) => (
                    <span
                      key={id}
                      className="font-mono text-[11px] px-2 py-0.5 rounded border border-amber-900/50 text-amber-500/80 bg-amber-950/30"
                    >
                      #{id}
                    </span>
                  ))}
                  {evalRun.orphanedIds.length > 120 && (
                    <span className="font-mono text-[11px] text-muted-foreground px-2 py-0.5">
                      + {evalRun.orphanedIds.length - 120} more
                    </span>
                  )}
                </div>
              )}

              {evalRun.allKnownIds.length >= 10_000 && (
                <p className="text-[10px] text-muted-foreground">
                  ⚠ Orphan scan capped at 10 000 records — larger collections may have additional orphans.
                </p>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
