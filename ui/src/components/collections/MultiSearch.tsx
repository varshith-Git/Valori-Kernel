"use client";

import { useState, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

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
        const parsed = query
          .split(",")
          .map((s) => parseFloat(s.trim()))
          .filter((n) => !isNaN(n));
        if (parsed.length === 0) throw new Error("Enter comma-separated floats");
        const res = await fetch("/api/search", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            query: parsed,
            k,
            collection: namespace,
          }),
        });
        if (!res.ok) throw new Error(await res.text());
        const data = await res.json();
        setResults(data.results ?? []);
        setStateHash(data.state_hash ?? null);
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
      <div className="flex items-center gap-1 rounded-lg bg-zinc-900 border border-zinc-800 p-1 w-fit">
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
                ? "bg-zinc-700 text-white"
                : "text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800"
            }`}
          >
            <span className="font-mono w-3 text-center">{m.icon}</span>
            {m.label}
          </button>
        ))}
      </div>

      {/* Input area */}
      <div className="rounded-xl border border-zinc-800 bg-zinc-900 p-5 flex flex-col gap-4">
        {mode === "semantic" && (
          <SemanticInput
            query={query}
            setQuery={setQuery}
            k={k}
            setK={setK}
            dim={dim}
            onRun={run}
            busy={busy}
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
        <div className="rounded-lg border border-red-900 bg-red-950 px-4 py-3 text-sm text-red-400">
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
        />
      )}
    </div>
  );
}

// ── Mode sub-components ────────────────────────────────────────────────────────

function SemanticInput({
  query, setQuery, k, setK, dim, onRun, busy,
}: {
  query: string; setQuery: (v: string) => void;
  k: number; setK: (v: number) => void;
  dim: number | null; onRun: () => void; busy: boolean;
}) {
  return (
    <>
      <div>
        <label className="text-xs text-zinc-500 mb-1 block">
          Query vector (comma-separated floats
          {dim ? `, dim=${dim}` : ""})
        </label>
        <textarea
          rows={3}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onRun();
          }}
          placeholder={`0.1, 0.2, 0.3${dim ? ` ... (${dim} values)` : ""}`}
          className="w-full rounded-lg bg-zinc-800 border border-zinc-700 text-sm text-zinc-100 placeholder:text-zinc-600 px-3 py-2 font-mono resize-none focus:outline-none focus:border-zinc-500"
        />
        <p className="text-[10px] text-zinc-600 mt-1">⌘↵ to search</p>
      </div>
      <div className="flex items-center gap-3">
        <label className="text-xs text-zinc-500">Top-K</label>
        <Input
          type="number"
          min={1}
          max={200}
          value={k}
          onChange={(e) => setK(Math.min(200, Math.max(1, parseInt(e.target.value) || 10)))}
          className="w-20 bg-zinc-800 border-zinc-700 text-white h-8 text-sm"
        />
        <Button
          size="sm"
          disabled={busy || !query.trim()}
          onClick={onRun}
          className="bg-white text-black hover:bg-zinc-200 h-8"
        >
          {busy ? "Searching…" : "Search"}
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
      />
      <div className="flex items-center gap-3">
        <label className="text-xs text-zinc-500">Semantic weight</label>
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={weight}
          onChange={(e) => setWeight(parseFloat(e.target.value))}
          className="w-32 accent-white"
        />
        <span className="text-xs font-mono text-zinc-400">{weight.toFixed(2)}</span>
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
        <label className="text-xs text-zinc-500 mb-1 block">Record ID (u32 integer)</label>
        <Input
          type="number"
          min={0}
          placeholder="42"
          value={idQuery}
          onChange={(e) => setIdQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onRun()}
          className="bg-zinc-800 border-zinc-700 text-white h-9"
        />
      </div>
      <Button
        size="sm"
        disabled={busy || !idQuery.trim()}
        onClick={onRun}
        className="bg-white text-black hover:bg-zinc-200 h-9 self-end"
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
        <label className="text-xs text-zinc-500 mb-1 block">
          Regex pattern — matched against record IDs
        </label>
        <Input
          placeholder="^4[0-9]+$"
          value={pattern}
          onChange={(e) => setPattern(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onRun()}
          className="bg-zinc-800 border-zinc-700 text-white font-mono h-9"
        />
        <p className="text-[10px] text-zinc-600 mt-1">
          Fetches k records via zero-vec search, then filters IDs by pattern
        </p>
      </div>
      <div className="flex items-center gap-3">
        <label className="text-xs text-zinc-500">Fetch k</label>
        <Input
          type="number"
          min={1}
          max={1000}
          value={k}
          onChange={(e) => setK(Math.min(1000, Math.max(1, parseInt(e.target.value) || 100)))}
          className="w-20 bg-zinc-800 border-zinc-700 text-white h-8 text-sm"
        />
        <Button
          size="sm"
          disabled={busy}
          onClick={onRun}
          className="bg-white text-black hover:bg-zinc-200 h-8"
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
      <div className="rounded-lg border border-zinc-800 bg-zinc-950 px-5 py-5 flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <span className="text-zinc-600 text-lg">T</span>
          <p className="text-sm font-medium text-zinc-300">Text search</p>
          <span className="text-[10px] rounded px-1.5 py-0.5 bg-zinc-800 text-zinc-500 border border-zinc-700">
            coming soon
          </span>
        </div>
        <p className="text-xs text-zinc-500 leading-relaxed">
          Text search converts your query into a vector using an embedding model
          before searching. Valori stores float vectors, not raw text.
        </p>
        <div className="rounded-lg border border-dashed border-zinc-800 p-4">
          <p className="text-xs text-zinc-600 font-medium mb-2">What&apos;s needed:</p>
          <ul className="text-xs text-zinc-600 space-y-1 list-disc list-inside">
            <li>An embedding API key (OpenAI / Cohere / custom)</li>
            <li>Configure it in Project → Settings → Embedding</li>
            <li>Re-insert records with their text source</li>
          </ul>
        </div>
        <p className="text-xs text-zinc-600">
          For now, use <strong className="text-zinc-400">Semantic</strong> mode and
          paste a pre-computed vector.
        </p>
      </div>
    </div>
  );
}

function MetadataStub() {
  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-lg border border-zinc-800 bg-zinc-950 px-5 py-5 flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <span className="text-zinc-600 text-lg">⌗</span>
          <p className="text-sm font-medium text-zinc-300">Metadata search</p>
          <span className="text-[10px] rounded px-1.5 py-0.5 bg-zinc-800 text-zinc-500 border border-zinc-700">
            coming soon
          </span>
        </div>
        <p className="text-xs text-zinc-500 leading-relaxed">
          Filter records by key=value metadata fields. Valori&apos;s current record format
          stores float vectors only — a metadata layer is planned for a future release.
        </p>
        <div className="rounded-lg border border-dashed border-zinc-800 p-4">
          <p className="text-xs text-zinc-600 font-medium mb-2">Planned fields:</p>
          <ul className="text-xs text-zinc-600 space-y-1 list-disc list-inside">
            <li>Arbitrary key-value string pairs per record</li>
            <li>Filter by exact match, prefix, or range</li>
            <li>Combine with semantic search (pre-filter + ANN)</li>
          </ul>
        </div>
        <p className="text-xs text-zinc-600">
          For now, use <strong className="text-zinc-400">#id</strong> to look up
          specific records or <strong className="text-zinc-400">Regex</strong> to
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
}: {
  results: SearchResult[];
  stateHash: string | null;
  onDelete?: (id: number) => Promise<void>;
  deletingId: number | null;
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-sm text-zinc-400">
          {results.length} result{results.length !== 1 ? "s" : ""}
        </p>
        {stateHash && (
          <code className="text-[10px] font-mono text-zinc-600 truncate max-w-[260px]">
            hash: {stateHash.slice(0, 24)}…
          </code>
        )}
      </div>

      {results.length === 0 ? (
        <div className="rounded-xl border border-dashed border-zinc-800 py-10 text-center">
          <p className="text-sm text-zinc-500">No records found</p>
        </div>
      ) : (
        <div className="rounded-xl border border-zinc-800 overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-zinc-800 bg-zinc-950">
                <th className="text-left px-4 py-2.5 text-xs text-zinc-500 font-medium w-24">ID</th>
                <th className="text-left px-4 py-2.5 text-xs text-zinc-500 font-medium">Score</th>
                <th className="w-20" />
              </tr>
            </thead>
            <tbody>
              {results.map((r, i) => (
                <tr
                  key={r.id}
                  className={`border-b border-zinc-800/50 last:border-0 ${
                    i % 2 === 0 ? "bg-zinc-900" : "bg-zinc-900/50"
                  }`}
                >
                  <td className="px-4 py-2.5 font-mono text-zinc-300">#{r.id}</td>
                  <td className="px-4 py-2.5">
                    <div className="flex items-center gap-3">
                      <div
                        className="h-1.5 rounded-full bg-emerald-500/60"
                        style={{ width: `${Math.max(4, (r.score * 80))}px` }}
                      />
                      <span className="font-mono text-xs text-zinc-400">
                        {r.score.toFixed(4)}
                      </span>
                    </div>
                  </td>
                  <td className="px-4 py-2.5 text-right">
                    {onDelete && (
                      <button
                        onClick={() => onDelete(r.id)}
                        disabled={deletingId === r.id}
                        className="text-xs text-zinc-600 hover:text-red-400 transition-colors disabled:opacity-40"
                      >
                        {deletingId === r.id ? "…" : "delete"}
                      </button>
                    )}
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
