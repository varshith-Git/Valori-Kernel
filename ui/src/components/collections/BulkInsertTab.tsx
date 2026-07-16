"use client";

import { useState, useRef } from "react";
import { Button } from "@/components/ui/button";

interface ParsedRow {
  index: number;
  vector: number[];
  error?: string;
}

interface InsertResult {
  index: number;
  record_id?: number;
  error?: string;
}

function parseInput(raw: string, expectedDim: number | null): ParsedRow[] {
  const rows: ParsedRow[] = [];
  const lines = raw.trim().split("\n").filter((l) => l.trim() && !l.startsWith("#"));

  // Detect JSON array of arrays
  if (raw.trim().startsWith("[")) {
    try {
      const parsed = JSON.parse(raw.trim()) as unknown[][];
      return parsed.map((row, i) => {
        if (!Array.isArray(row)) return { index: i, vector: [], error: "not an array" };
        const nums = row.map(Number);
        if (nums.some(isNaN)) return { index: i, vector: [], error: "non-numeric values" };
        if (expectedDim && nums.length !== expectedDim)
          return { index: i, vector: [], error: `expected ${expectedDim} dims, got ${nums.length}` };
        return { index: i, vector: nums };
      });
    } catch {
      return [{ index: 0, vector: [], error: "invalid JSON" }];
    }
  }

  // CSV: one vector per line, comma-separated floats
  for (const [i, line] of lines.entries()) {
    // Skip CSV header row if first line starts with a letter
    if (i === 0 && /^[a-zA-Z]/.test(line.trim())) continue;
    const nums = line.split(",").map((v) => Number(v.trim()));
    if (nums.some(isNaN)) {
      rows.push({ index: i, vector: [], error: "non-numeric values" });
    } else if (expectedDim && nums.length !== expectedDim) {
      rows.push({ index: i, vector: [], error: `expected ${expectedDim} dims, got ${nums.length}` });
    } else {
      rows.push({ index: i, vector: nums });
    }
  }
  return rows;
}

export function BulkInsertTab({ namespace, dim }: { namespace: string; dim: number | null }) {
  const [raw, setRaw] = useState("");
  const [parsed, setParsed] = useState<ParsedRow[] | null>(null);
  const [results, setResults] = useState<InsertResult[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const validRows = parsed?.filter((r) => !r.error) ?? [];
  const badRows = parsed?.filter((r) => r.error) ?? [];

  function handleParse() {
    setResults(null);
    setError(null);
    const rows = parseInput(raw, dim);
    setParsed(rows);
  }

  function handleFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      const text = ev.target?.result as string;
      setRaw(text);
      setResults(null);
      setParsed(null);
    };
    reader.readAsText(file);
  }

  async function handleInsert() {
    if (!validRows.length) return;
    setBusy(true);
    setError(null);
    setResults(null);
    const out: InsertResult[] = [];
    for (const row of validRows) {
      try {
        const res = await fetch("/api/insert", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ vector: row.vector, collection: namespace }),
        });
        if (!res.ok) {
          const body = await res.json().catch(() => null) as { error?: string } | null;
          out.push({ index: row.index, error: body?.error ?? `HTTP ${res.status}` });
        } else {
          const data = await res.json() as { id?: number };
          out.push({ index: row.index, record_id: data.id });
        }
      } catch (err) {
        out.push({ index: row.index, error: String(err) });
      }
    }
    setResults(out);
    setBusy(false);
  }

  const succeeded = results?.filter((r) => r.record_id !== undefined).length ?? 0;
  const failed = results?.filter((r) => r.error !== undefined).length ?? 0;

  return (
    <div className="flex flex-col gap-5 p-4">
      <div className="flex flex-col gap-1">
        <p className="text-sm text-muted-foreground">
          Paste vectors as <span className="font-mono">CSV</span> (one row per line) or{" "}
          <span className="font-mono">JSON</span> array of arrays.
          {dim && <> Dimension: <span className="font-mono font-medium">{dim}</span>.</>}
        </p>
        <p className="text-xs text-muted-foreground">
          CSV example: <span className="font-mono">0.1,0.2,0.3</span> per line.&nbsp;
          JSON example: <span className="font-mono">[[0.1,0.2],[0.3,0.4]]</span>
        </p>
      </div>

      {/* File drop */}
      <div className="flex items-center gap-3">
        <Button
          variant="outline"
          size="sm"
          onClick={() => fileRef.current?.click()}
          className="text-xs"
        >
          Upload file
        </Button>
        <span className="text-xs text-muted-foreground">or paste below</span>
        <input ref={fileRef} type="file" accept=".csv,.json,.txt" className="hidden" onChange={handleFile} />
      </div>

      {/* Text area */}
      <textarea
        className="w-full min-h-[180px] rounded-lg border border-input bg-background px-3 py-2.5 text-xs font-mono text-foreground resize-y focus:outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground"
        placeholder={"0.1,0.2,0.3\n0.4,0.5,0.6\n..."}
        value={raw}
        onChange={(e) => { setRaw(e.target.value); setParsed(null); setResults(null); }}
        spellCheck={false}
      />

      {/* Parse + preview */}
      <div className="flex items-center gap-3">
        <Button size="sm" variant="outline" onClick={handleParse} disabled={!raw.trim()}>
          Preview
        </Button>
        {parsed && (
          <span className="text-xs text-muted-foreground">
            {validRows.length} valid
            {badRows.length > 0 && (
              <span className="text-amber-500"> · {badRows.length} invalid</span>
            )}
          </span>
        )}
      </div>

      {/* Parse errors */}
      {badRows.length > 0 && (
        <div className="rounded-lg border border-amber-900/40 bg-amber-950/20 px-3 py-2.5 flex flex-col gap-1">
          <p className="text-[10px] uppercase tracking-widest text-amber-500 mb-1">Parse errors</p>
          {badRows.map((r) => (
            <p key={r.index} className="text-xs font-mono text-amber-400">
              row {r.index + 1}: {r.error}
            </p>
          ))}
        </div>
      )}

      {/* Insert button */}
      {parsed && validRows.length > 0 && !results && (
        <Button
          size="sm"
          onClick={handleInsert}
          disabled={busy}
          className="self-start"
        >
          {busy ? `Inserting…` : `Insert ${validRows.length} vector${validRows.length !== 1 ? "s" : ""}`}
        </Button>
      )}

      {/* Results */}
      {results && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            {succeeded > 0 && (
              <span className="text-sm text-emerald-400">
                ✓ {succeeded} inserted
              </span>
            )}
            {failed > 0 && (
              <span className="text-sm text-red-400">
                ✗ {failed} failed
              </span>
            )}
          </div>

          <div className="rounded-xl border border-border overflow-hidden">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-border bg-background">
                  <th className="text-left px-4 py-2 text-muted-foreground font-medium w-20">Row</th>
                  <th className="text-left px-4 py-2 text-muted-foreground font-medium">Result</th>
                </tr>
              </thead>
              <tbody>
                {results.map((r, i) => (
                  <tr key={i} className={`border-b border-border/50 last:border-0 ${i % 2 === 0 ? "bg-card" : "bg-card/50"}`}>
                    <td className="px-4 py-2 font-mono text-muted-foreground">{r.index + 1}</td>
                    <td className="px-4 py-2 font-mono">
                      {r.record_id !== undefined ? (
                        <span className="text-emerald-400">rec #{r.record_id}</span>
                      ) : (
                        <span className="text-red-400">{r.error}</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {error && <p className="text-xs text-red-400">{error}</p>}
        </div>
      )}
    </div>
  );
}
