"use client";

import { useRef, useState, useEffect } from "react";
import Link from "next/link";
import { useEmbeddingConfig, PROVIDER_DEFAULTS } from "@/lib/hooks/useEmbeddingConfig";
import { useLLMConfig } from "@/lib/hooks/useLLMConfig";
import { useHealth } from "@/lib/hooks/useHealth";

interface ChunkResult {
  record_id: number;
  chunk_node_id: number;
  chunk_index: number;
  preview: string;
}

interface IngestResult {
  ok: boolean;
  document_node_id: number;
  ingested: number;
  total_chunks: number;
  chunks: ChunkResult[];
  error?: string;
  pipeline?: "server" | "client";
  embed_provider?: string;
  strategy_used?: string;
}

interface Props {
  collection: string;
  onAskQuestion?: (question: string) => void;
}

const ACCEPT = ".pdf,.txt,.md,.docx";

export function DocumentUploadTab({ collection, onAskQuestion }: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const { config } = useEmbeddingConfig();
  const { config: llmCfg } = useLLMConfig();
  const { dim: serverDim } = useHealth();
  const providerDim = PROVIDER_DEFAULTS[config.provider].dim;
  const dimMismatch = serverDim !== null && providerDim > 0 && serverDim !== providerDim;

  const [file, setFile] = useState<File | null>(null);
  const [status, setStatus] = useState<"idle" | "ingesting" | "done" | "error">("idle");
  const [result, setResult] = useState<IngestResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showChunks, setShowChunks] = useState(false);
  const [enrichEnabled, setEnrichEnabled] = useState(false);
  const [chunkMode, setChunkMode] = useState<"fixed" | "tree">("tree");
  const [suggestedQuestions, setSuggestedQuestions] = useState<string[] | null>(null);
  const [suggestingQuestions, setSuggestingQuestions] = useState(false);
  const [suggestError, setSuggestError] = useState<string | null>(null);
  const [serverEmbed, setServerEmbed] = useState<{ enabled: boolean; provider?: string } | null>(null);

  // Probe node for on-node embedding capability once on mount
  useEffect(() => {
    fetch("/api/health")
      .then((r) => r.ok ? r.json() : null)
      .then((h: { embed_enabled?: boolean; embed_provider?: string } | null) => {
        if (h) setServerEmbed({ enabled: !!h.embed_enabled, provider: h.embed_provider });
      })
      .catch(() => {});
  }, []);

  const handleFile = (f: File) => {
    setFile(f);
    setResult(null);
    setError(null);
    setStatus("idle");
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const f = e.dataTransfer.files[0];
    if (f) handleFile(f);
  };

  const ingest = async () => {
    if (!file) return;
    setStatus("ingesting");
    setError(null);
    setResult(null);

    const form = new FormData();
    form.append("file", file);
    form.append("collection", collection);
    form.append("provider", config.provider);
    form.append("model", config.model);
    form.append("apiKey", config.apiKey);
    form.append("endpoint", config.endpoint);
    form.append("chunkSize", String(config.chunkSize));
    form.append("chunkOverlap", String(config.chunkOverlap));

    // Contextual enrichment (C1): pass LLM params so the server can generate
    // a context sentence per chunk and commit it in the audited event metadata.
    form.append("chunkMode", chunkMode);
    form.append("enrichEnabled", String(enrichEnabled));
    if (enrichEnabled) {
      form.append("llmProvider", llmCfg.provider);
      form.append("llmModel", llmCfg.model);
      form.append("llmApiKey", llmCfg.apiKey);
      form.append("llmEndpoint", llmCfg.endpoint);
    }

    try {
      const res = await fetch("/api/ingest", { method: "POST", body: form });
      const data: IngestResult = await res.json();
      if (!res.ok || data.error) {
        setError(data.error ?? `HTTP ${res.status}`);
        setStatus("error");
      } else {
        setResult(data);
        setStatus("done");
        setFile(null);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Ingestion failed");
      setStatus("error");
    }
  };

  const providerReady =
    config.provider === "ollama"
      ? !!config.model
      : !!config.apiKey;

  return (
    <div className="flex flex-col gap-5 max-w-2xl">
      {/* Config summary */}
      <div className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
        <div className="flex items-center gap-3 text-xs">
          <span className="text-muted-foreground">Embedding</span>
          <span className="font-mono text-accent-foreground">
            {config.provider}/{config.model || "—"}
          </span>
          <span className="text-muted-foreground">·</span>
          <span className="text-muted-foreground">chunks</span>
          <span className="font-mono text-accent-foreground">
            {config.chunkSize}/{config.chunkOverlap}
          </span>
          {!providerReady && (
            <span className="ml-2 rounded border border-amber-500/30 bg-amber-500/15 px-2 py-0.5 text-amber-700">
              API key missing
            </span>
          )}
        </div>
        <Link
          href="/settings"
          className="text-xs text-muted-foreground hover:text-accent-foreground transition-colors"
        >
          configure →
        </Link>
      </div>

      {/* Server-pipeline banner */}
      {serverEmbed?.enabled && (
        <div className="flex items-center gap-3 rounded-lg border border-[var(--v-accent)]/30 bg-[var(--v-accent-muted)] px-4 py-3">
          <span className="text-[var(--v-accent)] text-sm">⚡</span>
          <div className="flex-1 min-w-0">
            <p className="text-xs font-medium text-[var(--v-accent)]">Server-side pipeline active</p>
            <p className="text-[11px] text-muted-foreground mt-0.5">
              Node handles chunk + embed + insert via{" "}
              <span className="font-mono">{serverEmbed.provider}</span>. No client-side embedding needed.
            </p>
          </div>
          <span className="rounded border border-[var(--v-accent)]/30 px-2 py-0.5 text-[10px] font-mono text-[var(--v-accent)]">
            /v1/ingest
          </span>
        </div>
      )}

      {/* Dimension mismatch warning */}
      {dimMismatch && (
        <div className="rounded-lg border border-red-900 bg-red-950/40 px-4 py-3">
          <p className="text-sm font-medium text-red-400">Dimension mismatch — ingestion will fail</p>
          <p className="text-xs text-red-600 mt-1">
            Server is configured for{" "}
            <span className="font-mono text-red-400">{serverDim} dims</span> but{" "}
            <span className="font-mono text-red-400">{config.provider}/{config.model}</span> produces{" "}
            <span className="font-mono text-red-400">{providerDim} dims</span>.
          </p>
          <p className="text-xs text-red-700 mt-1.5">
            Fix: restart the server with{" "}
            <code className="font-mono text-red-500">VALORI_DIM={providerDim}</code>, or choose an embedding model that outputs{" "}
            <span className="font-mono">{serverDim}</span> dims in{" "}
            <Link href="/settings" className="text-red-400 hover:text-red-300 underline">Settings</Link>.
          </p>
        </div>
      )}

      {/* Chunking strategy */}
      <div className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
        <div className="flex flex-col gap-0.5">
          <p className="text-xs font-medium text-card-foreground">Chunking strategy</p>
          <p className="text-[11px] text-muted-foreground">
            {chunkMode === "tree"
              ? "Tree mode — one chunk per section (title + body). Best for structured docs."
              : "Fixed-size mode — overlapping windows. Better for unstructured text."}
          </p>
        </div>
        <div className="flex gap-1 ml-4">
          {(["tree", "fixed"] as const).map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => setChunkMode(m)}
              className={`px-2.5 py-1 rounded text-[11px] font-mono transition-colors ${
                chunkMode === m
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted text-muted-foreground hover:text-card-foreground"
              }`}
            >
              {m}
            </button>
          ))}
        </div>
      </div>

      {/* Contextual enrichment toggle */}
      <div className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
        <div className="flex flex-col gap-0.5">
          <p className="text-xs font-medium text-card-foreground">Contextual enrichment</p>
          <p className="text-[11px] text-muted-foreground">
            LLM generates a context sentence per chunk — stored in the audit chain.
            Uses the Reasoning LLM configured in Settings.
          </p>
        </div>
        <button
          type="button"
          onClick={() => setEnrichEnabled((v) => !v)}
          className={`relative ml-4 inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors focus:outline-none ${
            enrichEnabled ? "bg-primary" : "bg-accent"
          }`}
        >
          <span
            className={`inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
              enrichEnabled ? "translate-x-4" : "translate-x-0"
            }`}
          />
        </button>
      </div>

      {/* Drop zone */}
      <div
        onDrop={handleDrop}
        onDragOver={(e) => e.preventDefault()}
        onClick={() => fileRef.current?.click()}
        className={`flex flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed py-12 text-center cursor-pointer transition-colors ${
          file
            ? "border-blue-700 bg-blue-950/20"
            : "border-border hover:border-muted"
        }`}
      >
        <span className="text-3xl">{file ? "📄" : "↑"}</span>
        {file ? (
          <>
            <p className="text-sm font-medium text-card-foreground">{file.name}</p>
            <p className="text-xs text-muted-foreground">
              {(file.size / 1024).toFixed(1)} KB · {file.type || "text"}
            </p>
          </>
        ) : (
          <>
            <p className="text-sm text-muted-foreground">Drop file or click to browse</p>
            <p className="text-xs text-muted-foreground">PDF · DOCX · TXT · Markdown</p>
          </>
        )}
      </div>
      <input
        ref={fileRef}
        type="file"
        accept={ACCEPT}
        className="hidden"
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) handleFile(f);
          e.target.value = "";
        }}
      />

      {/* Action */}
      {file && status !== "done" && (
        <button
          onClick={ingest}
          disabled={status === "ingesting" || !providerReady}
          className="rounded-lg bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-40 transition-colors"
        >
          {status === "ingesting" ? (
            <span className="flex items-center gap-2">
              <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-border border-t-zinc-900" />
              Ingesting…
            </span>
          ) : (
            "Ingest document →"
          )}
        </button>
      )}

      {/* Result */}
      {status === "done" && result && (
        <div className="rounded-xl border border-emerald-800 bg-emerald-950/40 p-5">
          <div className="flex items-start justify-between">
            <div>
              <p className="font-medium text-emerald-400">
                ✓ Ingested {result.ingested} chunk{result.ingested !== 1 ? "s" : ""}
              </p>
              <p className="text-xs text-emerald-700 mt-1 font-mono">
                document node: #{result.document_node_id}
                {result.strategy_used && (
                  <span className="ml-2 text-emerald-800">· {result.strategy_used}</span>
                )}
                {result.pipeline === "server" && (
                  <span className="ml-2 text-emerald-800">· server pipeline ⚡</span>
                )}
              </p>
            </div>
            <button
              onClick={() => setShowChunks((v) => !v)}
              className="text-xs text-emerald-700 hover:text-emerald-400 transition-colors"
            >
              {showChunks ? "hide chunks" : "show chunks"}
            </button>
          </div>

          {showChunks && (
            <div className="mt-4 flex flex-col gap-2">
              {result.chunks.slice(0, 10).map((c) => (
                <div
                  key={c.record_id}
                  className="rounded-lg border border-emerald-900 bg-background px-3 py-2"
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-[10px] font-mono text-emerald-700">
                      chunk {c.chunk_index} · rec #{c.record_id} · node #{c.chunk_node_id}
                    </span>
                  </div>
                  <p className="text-xs text-muted-foreground line-clamp-2">{c.preview}</p>
                </div>
              ))}
              {result.chunks.length > 10 && (
                <p className="text-xs text-muted-foreground text-center">
                  +{result.chunks.length - 10} more chunks
                </p>
              )}
            </div>
          )}

          {/* Question suggester */}
          {onAskQuestion && (
            <div className="mt-4 pt-4 border-t border-emerald-900/40">
              <div className="flex items-center justify-between mb-3">
                <p className="text-xs font-medium text-emerald-600">AI-suggested questions</p>
                {!suggestedQuestions && !suggestingQuestions && (
                  <button
                    onClick={async () => {
                      setSuggestingQuestions(true);
                      setSuggestError(null);
                      try {
                        const res = await fetch("/api/suggest-questions", {
                          method: "POST",
                          headers: { "Content-Type": "application/json" },
                          body: JSON.stringify({
                            chunks: result.chunks.map((c) => c.preview),
                            source: file?.name,
                            llm: {
                              provider: llmCfg.provider,
                              model: llmCfg.model,
                              apiKey: llmCfg.apiKey,
                              endpoint: llmCfg.endpoint,
                            },
                          }),
                        });
                        const d = await res.json() as { questions?: string[]; error?: string };
                        if (d.error) throw new Error(d.error);
                        setSuggestedQuestions(d.questions ?? []);
                      } catch (e) {
                        setSuggestError(e instanceof Error ? e.message : "Failed");
                      } finally {
                        setSuggestingQuestions(false);
                      }
                    }}
                    className="text-xs px-3 py-1.5 rounded border border-emerald-800/60 text-emerald-600 hover:text-emerald-400 hover:border-emerald-700 transition-all"
                  >
                    ✦ Generate 8 questions
                  </button>
                )}
                {suggestingQuestions && (
                  <span className="text-xs text-emerald-700 flex items-center gap-1.5">
                    <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-emerald-800 border-t-emerald-500" />
                    Thinking…
                  </span>
                )}
                {suggestedQuestions && (
                  <button
                    onClick={() => setSuggestedQuestions(null)}
                    className="text-[10px] text-muted-foreground hover:text-muted-foreground transition-colors"
                  >
                    regenerate
                  </button>
                )}
              </div>

              {suggestError && (
                <p className="text-xs text-red-400 font-mono mb-2">{suggestError}</p>
              )}

              {suggestedQuestions && (
                <div className="flex flex-col gap-1.5">
                  {suggestedQuestions.map((q, i) => (
                    <div
                      key={i}
                      className="flex items-start justify-between gap-3 rounded-lg bg-background border border-border px-3 py-2.5 group"
                    >
                      <p className="text-xs text-muted-foreground leading-relaxed flex-1">{q}</p>
                      <button
                        onClick={() => onAskQuestion(q)}
                        className="flex-shrink-0 text-[10px] font-mono px-2 py-0.5 rounded border border-input text-muted-foreground hover:border-emerald-700 hover:text-emerald-400 hover:bg-emerald-950/30 transition-all whitespace-nowrap opacity-0 group-hover:opacity-100"
                      >
                        Ask →
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          <button
            onClick={() => {
              setResult(null);
              setStatus("idle");
              setSuggestedQuestions(null);
              setSuggestError(null);
            }}
            className="mt-4 text-xs text-emerald-700 hover:text-emerald-400 transition-colors"
          >
            + ingest another
          </button>
        </div>
      )}

      {/* Error */}
      {status === "error" && error && (
        <div className="rounded-xl border border-red-900 bg-red-950/40 p-4">
          <p className="text-sm font-medium text-red-400">Ingestion failed</p>
          <p className="text-xs text-red-600 mt-1 font-mono">{error}</p>
          <button
            onClick={() => {
              setError(null);
              setStatus("idle");
            }}
            className="mt-3 text-xs text-red-600 hover:text-red-400 transition-colors"
          >
            retry
          </button>
        </div>
      )}

      {/* Help */}
      {status === "idle" && !file && (
        <div className="rounded-lg border border-border bg-card/40 px-4 py-3 text-xs text-muted-foreground">
          <p className="font-medium text-muted-foreground mb-1">How ingestion works</p>
          <ol className="list-decimal list-inside space-y-0.5">
            <li>File is parsed into raw text — PDFs use position-aware extraction to preserve table columns</li>
            <li>Text is split — <strong>Tree</strong>: one chunk per detected section (best for Q&A); <strong>Fixed</strong>: overlapping size windows</li>
            <li>Each chunk is embedded via your configured model</li>
            <li>Vectors are stored in this collection</li>
            <li>Text is stored in the metadata sidecar (searchable via /audit)</li>
            <li>A Document→Chunk graph is built for provenance</li>
          </ol>
        </div>
      )}
    </div>
  );
}
