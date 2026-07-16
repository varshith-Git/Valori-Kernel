"use client";

import { useState, useCallback, useRef } from "react";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";
import { TabShell } from "@/components/collections/TabShell";

// --- Types --------------------------------------------------------------------

interface RecordWithText {
  id: number;
  text: string;
}

interface ContradictionPair {
  a: { id: number; text: string };
  b: { id: number; text: string };
  strength: number;  // 0–1, higher = more contradictory (=-cos(va, vb))
  l2sq: number;      // raw L2² from the negated search
}

// --- Math helpers -------------------------------------------------------------

// For unit-normalized vectors:
//   l2sq(-v_a, u_b) = 2(1 + cos(v_a, u_b))
//   cos(v_a, u_b) = l2sq / 2 - 1
//   contradiction_strength = -cos = 1 - l2sq / 2
// strength >= 0 means cos <= 0 (at minimum orthogonal)
// strength >= 0.5 means cos <= -0.5 (strong opposition)
function strengthFromScore(l2sq: number): number {
  return 1 - l2sq / 2;
}

function strengthLabel(s: number): string {
  if (s >= 0.8) return "very strong";
  if (s >= 0.6) return "strong";
  if (s >= 0.4) return "moderate";
  return "weak";
}

function strengthColor(s: number): string {
  if (s >= 0.7) return "text-red-400";
  if (s >= 0.5) return "text-amber-400";
  return "text-yellow-600";
}

function barColor(s: number): string {
  if (s >= 0.7) return "bg-red-500";
  if (s >= 0.5) return "bg-amber-500";
  return "bg-yellow-600";
}

// --- Scan engine --------------------------------------------------------------

interface ScanOpts {
  namespace: string;
  scanLimit: number;
  strengthThreshold: number;
  embedProvider: string;
  embedModel: string;
  embedApiKey: string;
  embedEndpoint: string;
  signal: AbortSignal;
  onProgress: (done: number, total: number, currentId: number) => void;
  onPair: (pair: ContradictionPair) => void;
}

async function embedText(text: string, opts: {
  provider: string; model: string; apiKey: string; endpoint: string;
}): Promise<number[]> {
  const res = await fetch("/api/embed-query", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      text,
      provider: opts.provider,
      model: opts.model,
      apiKey: opts.apiKey,
      endpoint: opts.endpoint,
    }),
  });
  if (!res.ok) throw new Error(`Embed failed (${res.status})`);
  const d = await res.json() as { vector: number[]; error?: string };
  if (d.error) throw new Error(d.error);
  return d.vector;
}

async function runScan(opts: ScanOpts): Promise<{ scanned: number; skipped: number }> {
  const {
    namespace, scanLimit, strengthThreshold, signal,
    onProgress, onPair,
    embedProvider, embedModel, embedApiKey, embedEndpoint,
  } = opts;

  // Threshold: L2² of the negated search must be below this to qualify
  const l2sqThreshold = 2 * (1 - strengthThreshold);

  // 1. Fetch record IDs from namespace-audit
  const auditRes = await fetch(
    `/api/namespace-audit?namespace=${encodeURIComponent(namespace)}`,
    { cache: "no-store", signal }
  );
  if (!auditRes.ok) throw new Error(`Audit fetch failed (${auditRes.status})`);
  const audit = await auditRes.json() as {
    ns_record_ids: number[];
    error?: string;
  };
  if (audit.error) throw new Error(audit.error);

  const ids = audit.ns_record_ids.slice(0, scanLimit);
  const total = ids.length;

  // 2. Fetch metadata text for all records in parallel (capped at 50)
  const metaFetches = ids.map((id) =>
    fetch(`/api/meta?target_id=record:${id}`, { cache: "no-store", signal })
      .then((r) => r.ok ? r.json() : null)
      .then((d) => {
        const text =
          (d?.metadata?.text as string | undefined) ??
          (d?.metadata?.value as string | undefined) ??
          null;
        return { id, text: text?.slice(0, 800) ?? null };
      })
      .catch(() => ({ id, text: null }))
  );
  const metas = await Promise.all(metaFetches);
  const withText: RecordWithText[] = metas
    .filter((m): m is RecordWithText => m.text !== null && m.text.trim().length > 20);
  const skipped = total - withText.length;

  // 3. Scan each record: embed → negate → search
  const seenPairs = new Set<string>();

  for (let i = 0; i < withText.length; i++) {
    if (signal.aborted) break;
    const record = withText[i];
    onProgress(i, withText.length, record.id);

    let embedding: number[];
    try {
      embedding = await embedText(record.text, {
        provider: embedProvider,
        model: embedModel,
        apiKey: embedApiKey,
        endpoint: embedEndpoint,
      });
    } catch { continue; }

    if (signal.aborted) break;

    // Negate the embedding to search for semantic opposites
    const negated = embedding.map((v) => -v);

    let searchResults: { id: number; score: number }[];
    try {
      const searchRes = await fetch("/api/search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: negated, k: 10, collection: namespace }),
        signal,
      });
      if (!searchRes.ok) continue;
      const d = await searchRes.json() as { results: { id: number; score: number }[] };
      searchResults = d.results ?? [];
    } catch { continue; }

    for (const hit of searchResults) {
      if (hit.id === record.id) continue;  // skip self
      const strength = strengthFromScore(hit.score);
      if (strength < strengthThreshold) continue;

      // Dedup: only one direction per pair
      const pairKey = `${Math.min(record.id, hit.id)}-${Math.max(record.id, hit.id)}`;
      if (seenPairs.has(pairKey)) continue;
      seenPairs.add(pairKey);

      // Find the text for the opposing record
      const bMeta = withText.find((m) => m.id === hit.id);
      if (!bMeta) {
        // Fetch if not in our scan set
        try {
          const mr = await fetch(`/api/meta?target_id=record:${hit.id}`, { cache: "no-store", signal });
          const md = mr.ok ? await mr.json() : null;
          const bText =
            (md?.metadata?.text as string | undefined) ??
            (md?.metadata?.value as string | undefined) ??
            `Record #${hit.id}`;
          onPair({
            a: { id: record.id, text: record.text },
            b: { id: hit.id, text: bText.slice(0, 800) },
            strength,
            l2sq: hit.score,
          });
        } catch {}
      } else {
        onPair({
          a: { id: record.id, text: record.text },
          b: { id: hit.id, text: bMeta.text },
          strength,
          l2sq: hit.score,
        });
      }
    }
  }

  onProgress(withText.length, withText.length, -1);
  return { scanned: withText.length, skipped };
}

// --- Sub-components -----------------------------------------------------------

function PairCard({ pair }: { pair: ContradictionPair }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-xl border border-border bg-card overflow-hidden">
      <div className="flex items-center justify-between px-4 py-2 border-b border-border/60 bg-card/80">
        <div className="flex items-center gap-3">
          <span className={`text-xs font-medium ${strengthColor(pair.strength)}`}>
            ⇅ {strengthLabel(pair.strength)} contradiction
          </span>
          <div className="w-20 h-1.5 rounded-full bg-accent overflow-hidden">
            <div
              className={`h-full rounded-full ${barColor(pair.strength)} transition-all`}
              style={{ width: `${pair.strength * 100}%` }}
            />
          </div>
          <span className="text-[10px] text-muted-foreground font-mono">
            {(pair.strength * 100).toFixed(0)}%
          </span>
        </div>
        <div className="flex items-center gap-2 text-[10px] text-muted-foreground font-mono">
          <span>#{pair.a.id} ↔ #{pair.b.id}</span>
          <button
            onClick={() => setExpanded((v) => !v)}
            className="text-muted-foreground hover:text-muted-foreground transition-colors ml-1"
          >
            {expanded ? "▲" : "▼"}
          </button>
        </div>
      </div>

      <div className={`grid ${expanded ? "grid-cols-1" : "grid-cols-2"} gap-0`}>
        {/* Record A */}
        <div className="p-4 border-r border-border">
          <div className="flex items-center gap-1.5 mb-2">
            <span className="text-[9px] font-mono text-muted-foreground uppercase tracking-widest">Record #{pair.a.id}</span>
          </div>
          <p className={`text-xs text-muted-foreground leading-relaxed ${expanded ? "" : "line-clamp-4"}`}>
            {pair.a.text}
          </p>
        </div>

        {/* Record B */}
        <div className="p-4">
          <div className="flex items-center gap-1.5 mb-2">
            <span className="text-[9px] font-mono text-muted-foreground uppercase tracking-widest">Record #{pair.b.id}</span>
          </div>
          <p className={`text-xs text-muted-foreground leading-relaxed ${expanded ? "" : "line-clamp-4"}`}>
            {pair.b.text}
          </p>
        </div>
      </div>
    </div>
  );
}

// --- Main tab -----------------------------------------------------------------

export function ContradictionTab({ namespace }: { namespace: string }) {
  const { config: embedCfg } = useEmbeddingConfig();

  const [scanLimit, setScanLimit] = useState(30);
  const [strengthThreshold, setStrengthThreshold] = useState(0.4);
  const [pairs, setPairs] = useState<ContradictionPair[]>([]);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number; currentId: number } | null>(null);
  const [summary, setSummary] = useState<{ scanned: number; skipped: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const startScan = useCallback(async () => {
    if (scanning) {
      abortRef.current?.abort();
      return;
    }

    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setScanning(true);
    setPairs([]);
    setSummary(null);
    setError(null);
    setProgress({ done: 0, total: 0, currentId: -1 });

    try {
      const result = await runScan({
        namespace,
        scanLimit,
        strengthThreshold,
        embedProvider: embedCfg.provider,
        embedModel: embedCfg.model,
        embedApiKey: embedCfg.apiKey,
        embedEndpoint: embedCfg.endpoint,
        signal: ctrl.signal,
        onProgress: (done, total, currentId) => {
          setProgress({ done, total, currentId });
        },
        onPair: (pair) => {
          setPairs((prev) =>
            [...prev, pair].sort((a, b) => b.strength - a.strength)
          );
        },
      });
      setSummary(result);
    } catch (e) {
      if (e instanceof Error && e.name !== "AbortError") {
        setError(e.message);
      }
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }, [scanning, namespace, scanLimit, strengthThreshold, embedCfg]);

  const thresholdLabels: Record<string, string> = {
    "0.3": "weak (cos < −0.3)",
    "0.4": "moderate (cos < −0.4)",
    "0.5": "strong (cos < −0.5)",
    "0.6": "very strong (cos < −0.6)",
    "0.7": "extreme (cos < −0.7)",
  };

  return (
    <TabShell>

      {/* Explainer */}
      <div className="rounded-xl border border-border bg-card px-4 py-3">
        <p className="text-xs text-muted-foreground leading-relaxed">
          Finds semantically opposing chunks by embedding each record&apos;s text, negating the
          vector, and searching for nearest neighbors. For unit-normalized vectors:{" "}
          <code className="font-mono bg-accent px-1 rounded">
            cos(v_a, v_b) = 1 − L2²(−v_a, v_b) / 2
          </code>
          {" "}— a result close to −1 indicates semantic opposition. Records without text metadata
          are skipped. Requires the same embedding model used during ingest.
        </p>
      </div>

      {/* Config */}
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
        <p className="text-sm font-semibold text-card-foreground">Scan Settings</p>

        <div className="grid grid-cols-2 gap-4">
          <div className="flex flex-col gap-1.5">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest">
              Max records to scan
            </label>
            <div className="flex items-center gap-3">
              <input
                type="range"
                min={5}
                max={100}
                step={5}
                value={scanLimit}
                onChange={(e) => setScanLimit(Number(e.target.value))}
                disabled={scanning}
                className="flex-1 accent-zinc-400"
              />
              <span className="text-sm font-mono text-accent-foreground w-8">{scanLimit}</span>
            </div>
            <p className="text-[10px] text-muted-foreground">
              Each record needs one embed + one search call
            </p>
          </div>

          <div className="flex flex-col gap-1.5">
            <label className="text-[10px] text-muted-foreground uppercase tracking-widest">
              Contradiction threshold
            </label>
            <select
              value={String(strengthThreshold)}
              onChange={(e) => setStrengthThreshold(Number(e.target.value))}
              disabled={scanning}
              className="bg-accent border border-input text-accent-foreground text-xs rounded px-2.5 py-1.5 focus:outline-none"
            >
              {Object.entries(thresholdLabels).map(([val, label]) => (
                <option key={val} value={val}>{label}</option>
              ))}
            </select>
          </div>
        </div>

        <div className="flex items-center justify-between">
          <div className="text-xs text-muted-foreground">
            Embedding:{" "}
            <span className="text-muted-foreground font-mono">
              {embedCfg.provider}/{embedCfg.model || "—"}
            </span>
          </div>
          <button
            onClick={startScan}
            className={`px-5 py-2 rounded-lg text-sm font-medium transition-colors ${
              scanning
                ? "bg-muted text-accent-foreground hover:bg-red-500/15 hover:text-red-700"
                : "bg-primary text-primary-foreground hover:bg-primary/90"
            }`}
          >
            {scanning ? "⬛ Stop" : "Scan for contradictions →"}
          </button>
        </div>
      </div>

      {/* Progress */}
      {scanning && progress && (
        <div className="rounded-xl border border-border bg-card p-4 flex flex-col gap-2">
          <div className="flex items-center justify-between text-xs">
            <span className="text-muted-foreground">
              Scanning record{" "}
              {progress.currentId >= 0 ? (
                <span className="font-mono text-accent-foreground">#{progress.currentId}</span>
              ) : (
                "…"
              )}
            </span>
            <span className="text-muted-foreground font-mono">
              {progress.done} / {progress.total}
            </span>
          </div>
          {progress.total > 0 && (
            <div className="h-1.5 bg-accent rounded-full overflow-hidden">
              <div
                className="h-full bg-muted-foreground/50 transition-all duration-300"
                style={{ width: `${(progress.done / progress.total) * 100}%` }}
              />
            </div>
          )}
          {pairs.length > 0 && (
            <p className="text-[11px] text-amber-600">
              {pairs.length} contradiction{pairs.length !== 1 ? "s" : ""} found so far…
            </p>
          )}
        </div>
      )}

      {/* Error */}
      {error && (
        <p className="text-sm text-red-400 font-mono px-1">{error}</p>
      )}

      {/* Summary */}
      {summary && !scanning && (
        <div className="flex items-center gap-4 px-1 text-xs text-muted-foreground">
          <span>Scanned {summary.scanned} records with text</span>
          {summary.skipped > 0 && (
            <span className="text-muted-foreground">· skipped {summary.skipped} without metadata</span>
          )}
          <span>·</span>
          <span className={pairs.length > 0 ? "text-amber-500" : "text-muted-foreground"}>
            {pairs.length} contradiction{pairs.length !== 1 ? "s" : ""} found
          </span>
        </div>
      )}

      {/* Results */}
      {pairs.length === 0 && summary && !scanning && (
        <div className="rounded-xl border border-border bg-card py-10 text-center">
          <p className="text-muted-foreground text-sm">No contradictions found</p>
          <p className="text-muted-foreground text-xs mt-1">
            Try lowering the threshold or ensuring records use the same embedding model
          </p>
        </div>
      )}

      {pairs.length > 0 && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-sm font-medium text-accent-foreground">
              {pairs.length} contradicting pair{pairs.length !== 1 ? "s" : ""}
              {scanning && <span className="text-muted-foreground"> · scanning…</span>}
            </p>
            <p className="text-[10px] text-muted-foreground">sorted by strength ↓</p>
          </div>
          {pairs.map((pair) => (
            <PairCard key={`${pair.a.id}-${pair.b.id}`} pair={pair} />
          ))}
        </div>
      )}
    </TabShell>
  );
}
