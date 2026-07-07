"use client";

import { useRef, useState, useEffect } from "react";
import Link from "next/link";
import { useEmbeddingConfig, PROVIDER_DEFAULTS, getModelDim } from "@/lib/hooks/useEmbeddingConfig";
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

interface TreeStructureNode {
  id: string;
  title: string;
  depth: number;
  child_count: number;
}

interface TreeBuildResult {
  cache_key: string;
  doc_name: string;
  node_count: number;
  structure_map: TreeStructureNode[];
}

function treeKey(namespace: string) {
  return `valori:tree:${namespace}`;
}

function saveTreeCache(namespace: string, data: TreeBuildResult) {
  try {
    localStorage.setItem(treeKey(namespace), JSON.stringify(data));
  } catch {}
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
  const providerDim = getModelDim(config.provider, config.model);
  const dimMismatch = serverDim !== null && providerDim > 0 && serverDim !== providerDim;

  const [file, setFile] = useState<File | null>(null);
  const [status, setStatus] = useState<"idle" | "ingesting" | "done" | "error">("idle");
  const [ingestStep, setIngestStep] = useState<string>("");
  const [result, setResult] = useState<IngestResult | null>(null);
  const [treeResult, setTreeResult] = useState<TreeBuildResult | null>(null);
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
    setIngestStep("Reading file…");
    setError(null);
    setResult(null);
    setTreeResult(null);

    // ── Tree-RAG path: build a section tree from the raw text ────────────────
    if (chunkMode === "tree") {
      try {
        setIngestStep("Parsing document structure…");
        const form = new FormData();
        form.append("file", file);
        form.append("doc_name", file.name);
        const res = await fetch("/api/tree/build", {
          method: "POST",
          body: form,
        });
        const data = await res.json() as TreeBuildResult & { error?: string };
        if (!res.ok || data.error) {
          setError(data.error ?? `HTTP ${res.status}`);
          setStatus("error");
        } else {
          saveTreeCache(collection, data);
          setTreeResult(data);
          setStatus("done");
          setFile(null);
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : "Tree build failed");
        setStatus("error");
      }
      return;
    }

    // ── Fixed chunking path (original flow) ──────────────────────────────────
    const form = new FormData();
    form.append("file", file);
    form.append("collection", collection);
    form.append("provider", config.provider);
    form.append("model", config.model);
    form.append("apiKey", config.apiKey);
    form.append("endpoint", config.endpoint);
    form.append("chunkSize", String(config.chunkSize));
    form.append("chunkOverlap", String(config.chunkOverlap));
    form.append("chunkMode", "fixed");
    form.append("enrichEnabled", String(enrichEnabled));
    if (enrichEnabled) {
      form.append("llmProvider", llmCfg.provider);
      form.append("llmModel", llmCfg.model);
      form.append("llmApiKey", llmCfg.apiKey);
      form.append("llmEndpoint", llmCfg.endpoint);
    }

    try {
      const stepTimer1 = setTimeout(() => setIngestStep("Splitting into chunks…"), 800);
      const stepTimer2 = setTimeout(() => setIngestStep("Embedding chunks…"), 2500);
      const stepTimer3 = setTimeout(() => setIngestStep("Storing vectors…"), 6000);

      const res = await fetch("/api/ingest", { method: "POST", body: form });
      clearTimeout(stepTimer1); clearTimeout(stepTimer2); clearTimeout(stepTimer3);
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
        <div className="rounded-lg border border-red-300 dark:border-red-800 bg-red-50 dark:bg-red-950/40 px-4 py-3">
          <p className="text-sm font-medium text-red-700 dark:text-red-400">Dimension mismatch — ingestion will fail</p>
          <p className="text-xs text-red-600 dark:text-red-500 mt-1">
            Server is configured for{" "}
            <span className="font-mono text-red-700 dark:text-red-400">{serverDim} dims</span> but{" "}
            <span className="font-mono text-red-700 dark:text-red-400">{config.provider}/{config.model}</span> produces{" "}
            <span className="font-mono text-red-700 dark:text-red-400">{providerDim} dims</span>.
          </p>
          <p className="text-xs text-red-600 dark:text-red-600 mt-1.5">
            Fix: restart the server with{" "}
            <code className="font-mono text-red-700 dark:text-red-400">VALORI_DIM={providerDim}</code>, or choose an embedding model that outputs{" "}
            <span className="font-mono">{serverDim}</span> dims in{" "}
            <Link href="/settings" className="text-red-700 dark:text-red-400 hover:text-red-500 dark:hover:text-red-300 underline">Settings</Link>.
          </p>
        </div>
      )}

      {/* Chunking strategy */}
      <div className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
        <div className="flex flex-col gap-0.5">
          <p className="text-xs font-medium text-card-foreground">Chunking strategy</p>
          <p className="text-[11px] text-muted-foreground">
            {chunkMode === "tree"
              ? "Tree-RAG — builds a ToC index for section-cited retrieval. Works with PDF, DOCX, TXT, MD. No embedding needed."
              : "Fixed-size — overlapping windows with embedding. Works with PDF, DOCX, TXT, MD."}
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
              <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-primary-foreground/30 border-t-primary-foreground" />
              {ingestStep || "Ingesting…"}
            </span>
          ) : (
            "Ingest document →"
          )}
        </button>
      )}

      {/* Tree-RAG result */}
      {status === "done" && treeResult && (
        <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-3">
          <div>
            <p className="font-medium text-[var(--v-accent)]">
              ✓ Tree index built — {treeResult.node_count} sections
            </p>
            <p className="text-xs text-muted-foreground mt-1 font-mono">
              {treeResult.doc_name} · Ask tab will use Tree-RAG for this collection
            </p>
          </div>
          <div className="flex flex-col gap-0.5 max-h-48 overflow-y-auto">
            {treeResult.structure_map.map((node) => (
              <div
                key={node.id}
                className="flex items-center gap-2 py-0.5 text-sm"
                style={{ paddingLeft: `${node.depth * 14 + 4}px` }}
              >
                <span className="text-[10px] text-muted-foreground">›</span>
                <span className="text-xs text-foreground truncate">{node.title}</span>
                {node.child_count > 0 && (
                  <span className="text-[10px] text-muted-foreground shrink-0">{node.child_count} sub</span>
                )}
              </div>
            ))}
          </div>
          <button
            onClick={() => {
              setTreeResult(null);
              setStatus("idle");
            }}
            className="mt-1 text-xs text-muted-foreground hover:text-foreground transition-colors self-start"
          >
            + index another
          </button>
        </div>
      )}

      {/* Fixed-mode result */}
      {status === "done" && result && (
        <div className="rounded-xl border border-border bg-card p-5">
          <div className="flex items-start justify-between">
            <div>
              <p className="font-medium text-[var(--v-accent)]">
                ✓ Ingested {result.ingested} chunk{result.ingested !== 1 ? "s" : ""}
              </p>
              <p className="text-xs text-muted-foreground mt-1 font-mono">
                document node: #{result.document_node_id}
                {result.strategy_used && (
                  <span className="ml-2">· {result.strategy_used}</span>
                )}
                {result.pipeline === "server" && (
                  <span className="ml-2">· server pipeline ⚡</span>
                )}
              </p>
            </div>
            <button
              onClick={() => setShowChunks((v) => !v)}
              className="text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              {showChunks ? "hide chunks" : "show chunks"}
            </button>
          </div>

          {showChunks && (
            <div className="mt-4 flex flex-col gap-2">
              {result.chunks.slice(0, 10).map((c) => (
                <div
                  key={c.record_id}
                  className="rounded-lg border border-border bg-background px-3 py-2"
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-[10px] font-mono text-muted-foreground">
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
            <div className="mt-4 pt-4 border-t border-border">
              <div className="flex items-center justify-between mb-3">
                <p className="text-xs font-medium text-foreground">AI-suggested questions</p>
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
                    className="text-xs px-3 py-1.5 rounded border border-input text-muted-foreground hover:text-foreground hover:border-ring transition-all"
                  >
                    ✦ Generate 8 questions
                  </button>
                )}
                {suggestingQuestions && (
                  <span className="text-xs text-muted-foreground flex items-center gap-1.5">
                    <span className="inline-block h-3 w-3 animate-spin rounded-full border-2 border-border border-t-foreground" />
                    Thinking…
                  </span>
                )}
                {suggestedQuestions && (
                  <button
                    onClick={() => setSuggestedQuestions(null)}
                    className="text-[10px] text-muted-foreground hover:text-foreground transition-colors"
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
                        className="flex-shrink-0 text-[10px] font-mono px-2 py-0.5 rounded border border-input text-muted-foreground hover:border-ring hover:text-foreground hover:bg-muted transition-all whitespace-nowrap opacity-0 group-hover:opacity-100"
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
            className="mt-4 text-xs text-muted-foreground hover:text-foreground transition-colors"
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
            <li><strong>Tree-RAG</strong> (PDF/DOCX/TXT/MD): extracts text, builds a ToC section tree — Ask tab navigates it by term frequency, returns cited sections + BLAKE3 receipt. No embedding needed.</li>
            <li><strong>Fixed</strong> (PDF/DOCX/TXT/MD): splits into overlapping windows, embeds each chunk, stores vectors. Ask tab uses semantic vector search + LLM synthesis.</li>
          </ol>
        </div>
      )}
    </div>
  );
}
