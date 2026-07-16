"use client";

import { useRef, useState } from "react";
import { UploadCloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useEmbeddingConfig } from "@/lib/hooks/useEmbeddingConfig";

interface Props {
  collection: string;
}

export function UploadTab({ collection }: Props) {
  const { config } = useEmbeddingConfig();
  const [paste, setPaste] = useState("");
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [result, setResult] = useState<{ ids: number[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [provider, setProvider] = useState("transformers");
  const [model, setModel] = useState("Xenova/bge-base-en-v1.5");
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
      if (Array.isArray(parsed) && parsed.every((r) => typeof r === "number")) {
        return [parsed as number[]];
      }
      return "JSON must be an array of numbers or array of arrays";
    } catch {
      return "Invalid JSON syntax";
    }
  };

  const ingestDocument = async (file: File) => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const formData = new FormData();
      formData.append("file", file);
      formData.append("collection", collection);
      formData.append("provider", provider);
      formData.append("model", model);
      formData.append("chunkSize", String(config.chunkSize));
      formData.append("chunkOverlap", String(config.chunkOverlap));

      const res = await fetch("/api/ingest", {
        method: "POST",
        body: formData,
      });
      if (!res.ok) {
        const e = await res.json().catch(() => ({}));
        throw new Error(e.error ?? `Status ${res.status}`);
      }
      const data = await res.json();
      setResult({ ids: data.results.map((r: any) => r.record_id) });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Ingestion failed");
    } finally {
      setBusy(false);
    }
  };

  const handleFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (!f) return;
    
    if (f.name.toLowerCase().endsWith(".json")) {
      const reader = new FileReader();
      reader.onload = (ev) => {
        const raw = ev.target?.result as string;
        const batch = parseBatch(raw);
        if (typeof batch === "string") { setError(batch); return; }
        insert(batch);
      };
      reader.readAsText(f);
    } else {
      ingestDocument(f);
    }
    e.target.value = "";
  };

  const handlePaste = () => {
    const batch = parseBatch(paste);
    if (typeof batch === "string") { setError(batch); return; }
    insert(batch);
  };

  return (
    <div className="flex flex-col gap-5">
      {/* Model Selection Dropdown */}
      <div className="flex gap-4 border border-border bg-card/50 p-4 rounded-xl">
        <div className="flex-1">
          <label className="text-xs text-muted-foreground mb-1 block font-medium">Embedding Provider</label>
          <select 
            value={provider} 
            onChange={(e) => {
               setProvider(e.target.value);
               if (e.target.value === "transformers") setModel("Xenova/bge-base-en-v1.5");
               else if (e.target.value === "openai") setModel("text-embedding-3-small");
               else if (e.target.value === "ollama") setModel("nomic-embed-text");
            }}
            className="w-full bg-background border border-input text-sm text-card-foreground rounded-md px-3 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring"
          >
            <option value="transformers">Local Open Source (Transformers.js)</option>
            <option value="openai">OpenAI API</option>
            <option value="ollama">Ollama (Local)</option>
          </select>
        </div>
        <div className="flex-1">
          <label className="text-xs text-muted-foreground mb-1 block font-medium">Model</label>
          {provider === "transformers" ? (
             <select value={model} onChange={(e) => setModel(e.target.value)} className="w-full bg-background border border-input text-sm text-card-foreground rounded-md px-3 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring">
               <option value="Xenova/bge-base-en-v1.5">bge-base-en-v1.5 (768 dims)</option>
               <option value="Xenova/all-MiniLM-L6-v2">all-MiniLM-L6-v2 (384 dims)</option>
               <option value="Xenova/e5-base-v2">e5-base-v2 (768 dims)</option>
             </select>
          ) : provider === "openai" ? (
             <select value={model} onChange={(e) => setModel(e.target.value)} className="w-full bg-background border border-input text-sm text-card-foreground rounded-md px-3 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring">
               <option value="text-embedding-3-small">text-embedding-3-small</option>
               <option value="text-embedding-3-large">text-embedding-3-large</option>
             </select>
          ) : (
             <input type="text" value={model} onChange={(e) => setModel(e.target.value)} className="w-full bg-background border border-input text-sm text-card-foreground rounded-md px-3 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring" />
          )}
        </div>
      </div>

      {/* File drop */}
      <div>
        <p className="text-xs text-muted-foreground mb-2 font-medium">Upload Document or JSON</p>
        <div
          className={cn(
            "flex flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed py-10 text-center cursor-pointer transition-all duration-200",
            dragging
              ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] scale-[1.01]"
              : "border-input hover:border-ring hover:bg-accent/30"
          )}
          onClick={() => fileRef.current?.click()}
          onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
          onDragEnter={(e) => { e.preventDefault(); setDragging(true); }}
          onDragLeave={() => setDragging(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragging(false);
            const f = e.dataTransfer.files?.[0];
            if (!f) return;
            // Reuse the same handler as the file input
            handleFile({ target: { files: e.dataTransfer.files, value: "" } } as unknown as React.ChangeEvent<HTMLInputElement>);
          }}
        >
          <UploadCloud
            size={28}
            className={cn(
              "transition-colors",
              dragging ? "text-[var(--v-accent)]" : "text-muted-foreground"
            )}
          />
          <div>
            <p className={cn("text-sm font-medium", dragging ? "text-foreground" : "text-muted-foreground")}>
              {dragging ? "Release to upload" : "Drop file or click to browse"}
            </p>
            <p className="text-xs text-muted-foreground mt-0.5">PDF · TXT · JSON</p>
          </div>
        </div>
        <input
          ref={fileRef}
          type="file"
          accept=".json,.pdf,.txt"
          className="hidden"
          onChange={handleFile}
        />
      </div>

      {/* Paste */}
      <div>
        <p className="text-xs text-muted-foreground mb-2 font-medium">Or paste vectors</p>
        <textarea
          value={paste}
          onChange={(e) => setPaste(e.target.value)}
          placeholder={`[[0.1, 0.2, 0.3, 0.4],\n [0.5, 0.6, 0.7, 0.8]]`}
          rows={5}
          className="w-full rounded-lg border border-input bg-background px-3 py-2 font-mono text-xs text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring resize-none"
        />
        <Button
          size="sm"
          disabled={busy || !paste.trim()}
          onClick={handlePaste}
          className="mt-2 bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
        >
          {busy ? "Inserting…" : "Insert →"}
        </Button>
      </div>

      {/* Result / error */}
      {result && (
        <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-4 py-3">
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
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-3">
          <p className="text-sm text-red-400">{error}</p>
        </div>
      )}
    </div>
  );
}
