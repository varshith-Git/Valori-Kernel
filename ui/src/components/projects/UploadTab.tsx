"use client";

import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";

interface Props {
  collection: string;
}

export function UploadTab({ collection }: Props) {
  const [paste, setPaste] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ids: number[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const insert = async (batch: number[][]) => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const res = await fetch("/api/insert", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ batch, collection }),
      });
      if (!res.ok) {
        const e = await res.json().catch(() => ({}));
        throw new Error(e.error ?? `Status ${res.status}`);
      }
      const data = await res.json();
      setResult(data);
      setPaste("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Insert failed");
    } finally {
      setBusy(false);
    }
  };

  const parseBatch = (raw: string): number[][] | string => {
    try {
      const parsed = JSON.parse(raw.trim());
      // [[0.1, 0.2], [0.3, 0.4]] — array of vectors
      if (Array.isArray(parsed) && parsed.every((r) => Array.isArray(r))) {
        return parsed as number[][];
      }
      // [{values: [...]}, ...] — array of objects with values key
      if (
        Array.isArray(parsed) &&
        parsed.every((r) => r && Array.isArray(r.values))
      ) {
        return parsed.map((r) => r.values as number[]);
      }
      // [0.1, 0.2, 0.3] — single vector
      if (Array.isArray(parsed) && parsed.every((x) => typeof x === "number")) {
        return [parsed as number[]];
      }
      return "Expected [[v1, v2], [v3, v4]] or [{values: [...]}, ...]";
    } catch {
      return "Invalid JSON";
    }
  };

  const handleFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (!f) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      const raw = ev.target?.result as string;
      const batch = parseBatch(raw);
      if (typeof batch === "string") { setError(batch); return; }
      insert(batch);
    };
    reader.readAsText(f);
    e.target.value = "";
  };

  const handlePaste = () => {
    const batch = parseBatch(paste);
    if (typeof batch === "string") { setError(batch); return; }
    insert(batch);
  };

  return (
    <div className="flex flex-col gap-5">
      {/* File drop */}
      <div>
        <p className="text-xs text-zinc-400 mb-2 font-medium">Upload JSON file</p>
        <div
          className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-zinc-700 py-8 text-center cursor-pointer hover:border-zinc-500 transition-colors"
          onClick={() => fileRef.current?.click()}
        >
          <span className="text-2xl">↑</span>
          <p className="text-sm text-zinc-400">Drop file or click to browse</p>
          <p className="text-xs text-zinc-600">
            Format:{" "}
            <code className="font-mono">
              {"[[0.1, 0.2, ...], ...]"} or [{'"'}values{'"'}: [...], {'"'}metadata{'"'}: {"{"}...{"}"}{"}"} ...]
            </code>
          </p>
        </div>
        <input
          ref={fileRef}
          type="file"
          accept=".json"
          className="hidden"
          onChange={handleFile}
        />
      </div>

      {/* Paste */}
      <div>
        <p className="text-xs text-zinc-400 mb-2 font-medium">Or paste vectors</p>
        <textarea
          value={paste}
          onChange={(e) => setPaste(e.target.value)}
          placeholder={`[[0.1, 0.2, 0.3, 0.4],\n [0.5, 0.6, 0.7, 0.8]]`}
          rows={5}
          className="w-full rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-700 focus:outline-none focus:ring-1 focus:ring-zinc-500 resize-none"
        />
        <Button
          size="sm"
          disabled={busy || !paste.trim()}
          onClick={handlePaste}
          className="mt-2 bg-white text-zinc-900 hover:bg-zinc-100 disabled:opacity-40"
        >
          {busy ? "Inserting…" : "Insert →"}
        </Button>
      </div>

      {/* Result / error */}
      {result && (
        <div className="rounded-lg border border-emerald-800 bg-emerald-950 px-4 py-3">
          <p className="text-sm text-emerald-400 font-medium">
            ✓ Inserted {result.ids.length} record{result.ids.length !== 1 ? "s" : ""}
          </p>
          <p className="text-xs text-emerald-600 font-mono mt-0.5">
            IDs: {result.ids.slice(0, 10).join(", ")}
            {result.ids.length > 10 ? ` +${result.ids.length - 10} more` : ""}
          </p>
        </div>
      )}
      {error && (
        <div className="rounded-lg border border-red-900 bg-red-950 px-4 py-3">
          <p className="text-sm text-red-400">{error}</p>
        </div>
      )}
    </div>
  );
}
