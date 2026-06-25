"use client";

import { useState, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { CodePanel } from "@/components/codegen/CodePanel";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";

export type SearchMode =
  | "semantic"
  | "text"
  | "id"
  | "hybrid"
  | "regex"
  | "metadata";

interface SearchResult {
  id: number;
  score: number;
}

interface Props {
  namespace: string;
  dim: number | null;
  onDelete?: (id: number) => Promise<void>;
}

const MODES: { key: SearchMode; label: string; icon: string }[] = [
  { key: "semantic", label: "Semantic", icon: "∿" },
  { key: "text", label: "Text", icon: "T" },
  { key: "id", label: "#id", icon: "#" },
  { key: "hybrid", label: "Hybrid", icon: "⊕" },
  { key: "regex", label: "Regex", icon: "/" },
  { key: "metadata", label: "Metadata", icon: "⌗" },
];

export function MultiSearch({ namespace, dim, onDelete }: Props) {
  const [mode, setMode] = useState<SearchMode>("semantic");
  const [query, setQuery] = useState("");
  const [k, setK] = useState(10);
  const [results, setResults] = useState<SearchResult[] | null>(null);
  const [stateHash, setStateHash] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<number | null>(null);

  // SDK code generator state
  const [queryVec, setQueryVec] = useState<number[] | null>(null);
  const [queryText, setQueryText] = useState<string | undefined>(undefined);
  const [codeResult, setCodeResult] = useState<SearchResult | null>(null);
  const { config: embedCfg } = useEmbeddingConfig();

  // Text-first semantic sub-mode: "text" embeds on the fly, "vector" is raw floats
  const [semanticSubMode, setSemanticSubMode] = useState<"text" | "vector">("text");
  const [busyLabel, setBusyLabel] = useState("Searching…");

  // Regex + ID filter applied client-side on semantic results
  const [regexFilter, setRegexFilter] = useState("");
  const [idQuery, setIdQuery] = useState("");
  const [metaQuery, setMetaQuery] = useState("");
  const [hybridWeight, setHybridWeight] = useState(0.7);

  const run = async () => {
    setError(null);
    setBusy(true);
    try {
      if (mode === "semantic" || mode === "hybrid") {
        let searchVec: number[];

        if (mode === "semantic" && semanticSubMode === "text") {
          // Step 1: embed the text query
          if (!query.trim()) throw new Error("Enter a query");
          setBusyLabel("Embedding…");
          const embedRes = await fetch("/api/embed-query", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              text: query,
              provider: embedCfg.provider,
              model: embedCfg.model,
              apiKey: embedCfg.apiKey,
              endpoint: embedCfg.endpoint,
            }),
          });
          if (!embedRes.ok) {
            const err = await embedRes.json().catch(() => ({})) as { error?: string };
            throw new Error(err.error ?? `Embedding failed (${embedRes.status})`);
          }
          const { vector } = await embedRes.json() as { vector: number[] };
          searchVec = vector;
          setQueryText(query); // save text for code gen
          setBusyLabel("Searching…");
        } else {
          // Raw vector input
          const parsed = query
            .split(",")
            .map((s) => parseFloat(s.trim()))
            .filter((n) => !isNaN(n));
          if (parsed.length === 0) throw new Error("Enter comma-separated floats");
          searchVec = parsed;
          setQueryText(undefined);
          setBusyLabel("Searching…");
        }

        const res = await fetch("/api/search", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            query: searchVec,
            k,
            collection: namespace,
          }),
        });
        if (!res.ok) throw new Error(await res.text());
        const data = await res.json();
        setResults(data.results ?? []);
        setStateHash(data.state_hash ?? null);
        setQueryVec(searchVec); // store for SDK code gen
      } else if (mode === "id") {
        // Search all and filter to that ID client-side
        const idNum = parseInt(idQuery, 10);
        if (isNaN(idNum)) throw new Error("Enter a valid integer ID");
        // Zero-vec to get all, then filter
        const zeroVec = Array(dim ?? 4).fill(0);
        const res = await fetch("/api/search", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ query: zeroVec, k: 1000, collection: namespace }),
        });
        if (!res.ok) throw new Error(await res.text());
        const data = await res.json();
        const filtered = (data.results ?? []).filter(
          (r: SearchResult) => r.id === idNum
        );
        setResults(filtered);
        setStateHash(data.state_hash ?? null);
      } else if (mode === "regex") {
        // Search all, then apply regex on ID string
        let re: RegExp;
        try {
          re = new RegExp(regexFilter || ".*");
        } catch {
          throw new Error("Invalid regex pattern");
        }
        const zeroVec = Array(dim ?? 4).fill(0);
        const res = await fetch("/api/search", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ query: zeroVec, k, collection: namespace }),
        });
        if (!res.ok) throw new Error(await res.text());
        const data = await res.json();
        const filtered = (data.results ?? []).filter((r: SearchResult) =>
          re.test(String(r.id))
        );
        setResults(filtered);
        setStateHash(data.state_hash ?? null);
      }
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Search failed");
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (id: number) => {
    if (!onDelete) return;
    setDeletingId(id);
    try {
      await onDelete(id);
      setResults((prev) => prev?.filter((r) => r.id !== id) ?? null);
    } finally {
      setDeletingId(null);
    }
  };

  return (
    <div className="flex flex-col gap-5">
      {/* Mode selector */}
      <div className="flex items-center gap-1 rounded-lg bg-card border border-border p-1 w-fit">
        {MODES.map((m) => (
          <button
            key={m.key}
            onClick={() => {
              setMode(m.key);
              setResults(null);
              setError(null);
            }}
            className={`flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${
              mode === m.key
                ? "bg-muted text-foreground"
                : "text-muted-foreground hover:text-accent-foreground hover:bg-accent"
            }`}
          >
            <span className="font-mono w-3 text-center">{m.icon}</span>
            {m.label}
          </button>
        ))}
      </div>

      {/* Input area */}
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
        {mode === "semantic" && (
          <SemanticInput
            query={query}
            setQuery={setQuery}
            k={k}
            setK={setK}
            dim={dim}
            onRun={run}
            busy={busy}
            busyLabel={busyLabel}
            subMode={semanticSubMode}
            setSubMode={setSemanticSubMode}
            embedLabel={`${embedCfg.provider} / ${embedCfg.model}`}
          />
        )}
        {mode === "text" && <TextStub />}
        {mode === "id" && (
          <IdInput
            idQuery={idQuery}
            setIdQuery={setIdQuery}
            onRun={run}
            busy={busy}
          />
        )}
        {mode === "hybrid" && (
          <HybridInput
            query={query}
            setQuery={setQuery}
            weight={hybridWeight}
            setWeight={setHybridWeight}
            k={k}
            setK={setK}
            dim={dim}
            onRun={run}
            busy={busy}
          />
        )}
        {mode === "regex" && (
          <RegexInput
            pattern={regexFilter}
            setPattern={setRegexFilter}
            k={k}
            setK={setK}
            onRun={run}
            busy={busy}
          />
        )}
        {mode === "metadata" && <MetadataStub />}
      </div>

      {/* Error */}
      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-700">
          {error}
        </div>
      )}

      {/* Results */}
      {results !== null && (
        <ResultsTable
          results={results}
          stateHash={stateHash}
          onDelete={onDelete ? handleDelete : undefined}
          deletingId={deletingId}
          onCode={queryVec ? (r) => setCodeResult(r) : undefined}
        />
      )}

      {/* SDK code panel */}
      {codeResult && queryVec && (
        <CodePanel
          isOpen
          onClose={() => setCodeResult(null)}
          queryVector={queryVec}
          queryText={queryText}
          k={k}
          collection={namespace}
          result={codeResult}
          embedProvider={embedCfg.provider}
          embedModel={embedCfg.model}
          embedEndpoint={embedCfg.endpoint}
        />
      )}
    </div>
  );
}

// -- Mode sub-components --------------------------------------------------------

function SemanticInput({
  query, setQuery, k, setK, dim, onRun, busy, busyLabel, subMode, setSubMode, embedLabel,
}: {
  query: string; setQuery: (v: string) => void;
  k: number; setK: (v: number) => void;
  dim: number | null; onRun: () => void; busy: boolean;
  busyLabel: string;
  subMode: "text" | "vector";
  setSubMode: (m: "text" | "vector") => void;
  embedLabel: string;
}) {
  return (
    <>
      {/* Sub-mode toggle */}
      <div className="flex items-center gap-1 rounded-md bg-accent border border-input p-0.5 w-fit">
        <button
          onClick={() => setSubMode("text")}
          className={`px-3 py-1 text-xs rounded transition-colors ${
            subMode === "text"
              ? "bg-muted text-foreground"
              : "text-muted-foreground hover:text-accent-foreground"
          }`}
        >
          Text query
        </button>
        <button
          onClick={() => setSubMode("vector")}
          className={`px-3 py-1 text-xs rounded transition-colors font-mono ${
            subMode === "vector"
              ? "bg-muted text-foreground"
              : "text-muted-foreground hover:text-accent-foreground"
          }`}
        >
          Raw vector
        </button>
      </div>

      {/* Input */}
      {subMode === "text" ? (
        <div>
          <textarea
            rows={2}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onRun();
            }}
            placeholder="Ask anything — e.g. what is the refund policy?"
            className="w-full rounded-lg bg-accent border border-input text-sm text-foreground placeholder:text-muted-foreground px-3 py-2 resize-none focus:outline-none focus:border-ring"
          />
          <p className="text-[10px] text-muted-foreground mt-1">
            Will embed with <span className="text-muted-foreground">{embedLabel}</span> · ⌘↵ to search
          </p>
        </div>
      ) : (
        <div>
          <textarea
            rows={3}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onRun();
            }}
            placeholder={`0.1, 0.2, 0.3${dim ? ` ... (${dim} values)` : ""}`}
            className="w-full rounded-lg bg-accent border border-input text-sm text-foreground placeholder:text-muted-foreground px-3 py-2 font-mono resize-none focus:outline-none focus:border-ring"
          />
          <p className="text-[10px] text-muted-foreground mt-1">Comma-separated floats{dim ? ` · ${dim} values` : ""} · ⌘↵ to search</p>
        </div>
      )}

      {/* Controls */}
      <div className="flex items-center gap-3">
        <label className="text-xs text-muted-foreground">Top-K</label>
        <Input
          type="number"
          min={1}
          max={200}
          value={k}
          onChange={(e) => setK(Math.min(200, Math.max(1, parseInt(e.target.value) || 10)))}
          className="w-20 bg-accent border-input text-foreground h-8 text-sm"
        />
        <Button
          size="sm"
          disabled={busy || !query.trim()}
          onClick={onRun}
          className="bg-primary text-primary-foreground hover:bg-primary/90 h-8"
        >
          {busy ? busyLabel : subMode === "text" ? "Embed & Search" : "Search"}
        </Button>
      </div>
    </>
  );
}

function HybridInput({
  query, setQuery, weight, setWeight, k, setK, dim, onRun, busy,
}: {
  query: string; setQuery: (v: string) => void;
  weight: number; setWeight: (v: number) => void;
  k: number; setK: (v: number) => void;
  dim: number | null; onRun: () => void; busy: boolean;
}) {
  return (
    <>
      <div className="rounded-lg border border-amber-900 bg-amber-950/40 px-4 py-3">
        <p className="text-xs text-amber-400 font-medium">Hybrid mode — semantic only for now</p>
        <p className="text-xs text-amber-700 mt-0.5">
          Text component requires an embedding model. The vector below will be used
          at full weight. Text+vector fusion is a planned backend feature.
        </p>
      </div>
      <SemanticInput
        query={query} setQuery={setQuery} k={k} setK={setK}
        dim={dim} onRun={onRun} busy={busy}
        busyLabel="Searching…" subMode="vector" setSubMode={() => {}} embedLabel=""
      />
      <div className="flex items-center gap-3">
        <label className="text-xs text-muted-foreground">Semantic weight</label>
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={weight}
          onChange={(e) => setWeight(parseFloat(e.target.value))}
          className="w-32 accent-white"
        />
        <span className="text-xs font-mono text-muted-foreground">{weight.toFixed(2)}</span>
      </div>
    </>
  );
}

function IdInput({
  idQuery, setIdQuery, onRun, busy,
}: {
  idQuery: string; setIdQuery: (v: string) => void;
  onRun: () => void; busy: boolean;
}) {
  return (
    <div className="flex items-center gap-3">
      <div className="flex-1">
        <label className="text-xs text-muted-foreground mb-1 block">Record ID (u32 integer)</label>
        <Input
          type="number"
          min={0}
          placeholder="42"
          value={idQuery}
          onChange={(e) => setIdQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onRun()}
          className="bg-accent border-input text-foreground h-9"
        />
      </div>
      <Button
        size="sm"
        disabled={busy || !idQuery.trim()}
        onClick={onRun}
        className="bg-primary text-primary-foreground hover:bg-primary/90 h-9 self-end"
      >
        {busy ? "Looking up…" : "Lookup"}
      </Button>
    </div>
  );
}

function RegexInput({
  pattern, setPattern, k, setK, onRun, busy,
}: {
  pattern: string; setPattern: (v: string) => void;
  k: number; setK: (v: number) => void;
  onRun: () => void; busy: boolean;
}) {
  return (
    <>
      <div>
        <label className="text-xs text-muted-foreground mb-1 block">
          Regex pattern — matched against record IDs
        </label>
        <Input
          placeholder="^4[0-9]+$"
          value={pattern}
          onChange={(e) => setPattern(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onRun()}
          className="bg-accent border-input text-foreground font-mono h-9"
        />
        <p className="text-[10px] text-muted-foreground mt-1">
          Fetches k records via zero-vec search, then filters IDs by pattern
        </p>
      </div>
      <div className="flex items-center gap-3">
        <label className="text-xs text-muted-foreground">Fetch k</label>
        <Input
          type="number"
          min={1}
          max={1000}
          value={k}
          onChange={(e) => setK(Math.min(1000, Math.max(1, parseInt(e.target.value) || 100)))}
          className="w-20 bg-accent border-input text-foreground h-8 text-sm"
        />
        <Button
          size="sm"
          disabled={busy}
          onClick={onRun}
          className="bg-primary text-primary-foreground hover:bg-primary/90 h-8"
        >
          {busy ? "Scanning…" : "Scan & filter"}
        </Button>
      </div>
    </>
  );
}

function TextStub() {
  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-lg border border-border bg-background px-5 py-5 flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground text-lg">T</span>
          <p className="text-sm font-medium text-accent-foreground">Text search</p>
          <span className="text-[10px] rounded px-1.5 py-0.5 bg-accent text-muted-foreground border border-input">
            coming soon
          </span>
        </div>
        <p className="text-xs text-muted-foreground leading-relaxed">
          Text search converts your query into a vector using an embedding model
          before searching. Valori stores float vectors, not raw text.
        </p>
        <div className="rounded-lg border border-dashed border-border p-4">
          <p className="text-xs text-muted-foreground font-medium mb-2">What&apos;s needed:</p>
          <ul className="text-xs text-muted-foreground space-y-1 list-disc list-inside">
            <li>An embedding API key (OpenAI / Cohere / custom)</li>
            <li>Configure it in Project → Settings → Embedding</li>
            <li>Re-insert records with their text source</li>
          </ul>
        </div>
        <p className="text-xs text-muted-foreground">
          For now, use <strong className="text-muted-foreground">Semantic</strong> mode and
          paste a pre-computed vector.
        </p>
      </div>
    </div>
  );
}

function MetadataStub() {
  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-lg border border-border bg-background px-5 py-5 flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground text-lg">⌗</span>
          <p className="text-sm font-medium text-accent-foreground">Metadata search</p>
          <span className="text-[10px] rounded px-1.5 py-0.5 bg-accent text-muted-foreground border border-input">
            coming soon
          </span>
        </div>
        <p className="text-xs text-muted-foreground leading-relaxed">
          Filter records by key=value metadata fields. Valori&apos;s current record format
          stores float vectors only — a metadata layer is planned for a future release.
        </p>
        <div className="rounded-lg border border-dashed border-border p-4">
          <p className="text-xs text-muted-foreground font-medium mb-2">Planned fields:</p>
          <ul className="text-xs text-muted-foreground space-y-1 list-disc list-inside">
            <li>Arbitrary key-value string pairs per record</li>
            <li>Filter by exact match, prefix, or range</li>
            <li>Combine with semantic search (pre-filter + ANN)</li>
          </ul>
        </div>
        <p className="text-xs text-muted-foreground">
          For now, use <strong className="text-muted-foreground">#id</strong> to look up
          specific records or <strong className="text-muted-foreground">Regex</strong> to
          match ID patterns.
        </p>
      </div>
    </div>
  );
}

function ResultsTable({
  results,
  stateHash,
  onDelete,
  deletingId,
  onCode,
}: {
  results: SearchResult[];
  stateHash: string | null;
  onDelete?: (id: number) => Promise<void>;
  deletingId: number | null;
  onCode?: (r: SearchResult) => void;
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          {results.length} result{results.length !== 1 ? "s" : ""}
        </p>
        {stateHash && (
          <code className="text-[10px] font-mono text-muted-foreground truncate max-w-[260px]">
            hash: {stateHash.slice(0, 24)}…
          </code>
        )}
      </div>

      {results.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border py-10 text-center">
          <p className="text-sm text-muted-foreground">No records found</p>
        </div>
      ) : (
        <div className="rounded-xl border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-background">
                <th className="text-left px-4 py-2.5 text-xs text-muted-foreground font-medium w-24">ID</th>
                <th className="text-left px-4 py-2.5 text-xs text-muted-foreground font-medium">Score</th>
                <th className="w-32" />
              </tr>
            </thead>
            <tbody>
              {results.map((r, i) => (
                <tr
                  key={r.id}
                  className={`border-b border-border/50 last:border-0 ${
                    i % 2 === 0 ? "bg-card" : "bg-card/50"
                  }`}
                >
                  <td className="px-4 py-2.5 font-mono text-accent-foreground">#{r.id}</td>
                  <td className="px-4 py-2.5">
                    <div className="flex items-center gap-3">
                      <div
                        className="h-1.5 rounded-full bg-emerald-500/60"
                        style={{ width: `${Math.max(4, (r.score * 80))}px` }}
                      />
                      <span className="font-mono text-xs text-muted-foreground">
                        {r.score.toFixed(4)}
                      </span>
                    </div>
                  </td>
                  <td className="px-4 py-2.5 text-right">
                    <div className="flex items-center justify-end gap-3">
                      {onCode && (
                        <button
                          onClick={() => onCode(r)}
                          title="Get Python / TypeScript / curl code for this query"
                          className="text-[11px] font-mono px-2 py-0.5 rounded border border-input text-muted-foreground hover:border-sky-600 hover:text-sky-300 hover:bg-sky-950/40 transition-all whitespace-nowrap"
                        >
                          {"</>"} get code
                        </button>
                      )}
                      {onDelete && (
                        <button
                          onClick={() => onDelete(r.id)}
                          disabled={deletingId === r.id}
                          className="text-xs text-muted-foreground hover:text-red-400 transition-colors disabled:opacity-40"
                        >
                          {deletingId === r.id ? "…" : "delete"}
                        </button>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
