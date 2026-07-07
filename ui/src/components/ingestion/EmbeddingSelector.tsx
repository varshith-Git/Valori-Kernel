"use client";

import { useState, useEffect } from "react";
import { useEmbeddingConfig, PROVIDER_DEFAULTS, MODEL_DIMS, getModelDim, registerModelDims, EmbeddingProvider } from "@/lib/hooks/useEmbeddingConfig";

const PROVIDERS: { id: EmbeddingProvider; label: string; note: string }[] = [
  { id: "openai", label: "OpenAI", note: "text-embedding-3-small / ada-002" },
  { id: "cohere", label: "Cohere", note: "embed-english-v3.0" },
  { id: "ollama", label: "Ollama", note: "runs locally, no API key" },
  { id: "custom", label: "Custom", note: "any OpenAI-compatible endpoint" },
];

const MODELS: Record<EmbeddingProvider, string[]> = {
  openai: ["text-embedding-3-small", "text-embedding-3-large", "text-embedding-ada-002"],
  cohere: ["embed-english-v3.0", "embed-multilingual-v3.0", "embed-english-light-v3.0"],
  ollama: ["nomic-embed-text", "mxbai-embed-large", "all-minilm"],
  custom: [],
};

export function EmbeddingSelector() {
  const { config, setConfig, setProvider } = useEmbeddingConfig();

  // Fetch live Ollama embedding models when provider is ollama
  const [ollamaModels, setOllamaModels] = useState<string[] | null>(null);
  const [ollamaErr, setOllamaErr] = useState(false);

  useEffect(() => {
    if (config.provider !== "ollama") return;
    setOllamaModels(null);
    setOllamaErr(false);
    fetch("/api/ollama-models?type=embed")
      .then((r) => r.json())
      .then((d: { models: string[]; dims?: Record<string, number>; error?: string }) => {
        if (d.error || d.models.length === 0) { setOllamaErr(true); return; }
        if (d.dims) registerModelDims(d.dims);
        setOllamaModels(d.models);
        if (!d.models.includes(config.model)) setConfig({ model: d.models[0] });
      })
      .catch(() => setOllamaErr(true));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [config.provider]);

  // Effective model list — live for Ollama, static for others
  const modelList =
    config.provider === "ollama"
      ? (ollamaModels ?? MODELS[config.provider])
      : MODELS[config.provider];

  return (
    <div className="flex flex-col gap-5">
      {/* Provider tabs */}
      <div>
        <p className="text-xs text-muted-foreground uppercase tracking-widest mb-3">Embedding provider</p>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          {PROVIDERS.map((p) => (
            <button
              key={p.id}
              onClick={() => setProvider(p.id)}
              className={`relative rounded-xl border p-3 text-left transition-all overflow-hidden ${
                config.provider === p.id
                  ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] shadow-sm ring-1 ring-[var(--v-accent)]/20"
                  : "border-border/60 bg-background/50 hover:border-input hover:bg-accent/30"
              }`}
            >
              <div className="flex items-center justify-between mb-1.5">
                <p className={`text-sm font-medium ${config.provider === p.id ? "text-foreground" : "text-muted-foreground"}`}>{p.label}</p>
                {config.provider === p.id && (
                  <div className="w-1.5 h-1.5 rounded-full bg-[var(--v-accent)] shadow-[0_0_8px_var(--v-accent)]" />
                )}
              </div>
              <p className={`text-[10px] leading-relaxed ${config.provider === p.id ? "text-foreground/80" : "text-muted-foreground/70"}`}>{p.note}</p>
            </button>
          ))}
        </div>
      </div>

      {/* Model selection */}
      <div className="grid grid-cols-2 gap-4">
        <div>
          <div className="flex items-center justify-between mb-1.5">
            <label className="text-xs text-muted-foreground">Model</label>
            {config.provider === "ollama" && (
              <span className={`text-[10px] ${ollamaErr ? "text-amber-500" : ollamaModels ? "text-emerald-600 dark:text-emerald-400" : "text-muted-foreground"}`}>
                {ollamaErr ? "⚠ ollama not reachable" : ollamaModels ? `${ollamaModels.length} embed models found` : "detecting…"}
              </span>
            )}
          </div>
          {modelList.length > 0 ? (
            <select
              value={config.model}
              onChange={(e) => setConfig({ model: e.target.value })}
              className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm text-card-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            >
              {modelList.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
          ) : (
            <input
              type="text"
              value={config.model}
              onChange={(e) => setConfig({ model: e.target.value })}
              placeholder="e.g. nomic-embed-text"
              className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            />
          )}
        </div>

        {/* API key (hidden for ollama) */}
        {config.provider !== "ollama" && (
          <div>
            <label className="text-xs text-muted-foreground block mb-1.5">
              API key{config.provider === "custom" ? " (optional)" : ""}
            </label>
            <input
              type="password"
              value={config.apiKey}
              onChange={(e) => setConfig({ apiKey: e.target.value })}
              placeholder={
                config.provider === "openai"
                  ? "sk-..."
                  : config.provider === "cohere"
                  ? "..."
                  : "Bearer token"
              }
              className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring font-mono"
            />
          </div>
        )}
      </div>

      {/* Endpoint override (ollama / custom always show it) */}
      {(config.provider === "ollama" || config.provider === "custom") && (
        <div>
          <label className="text-xs text-muted-foreground block mb-1.5">Endpoint URL</label>
          <input
            type="text"
            value={config.endpoint}
            onChange={(e) => setConfig({ endpoint: e.target.value })}
            placeholder={PROVIDER_DEFAULTS[config.provider].endpoint}
            className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm font-mono text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          />
        </div>
      )}

      {/* Chunking */}
      <div className="grid grid-cols-2 gap-4">
        <div>
          <div className="flex items-center justify-between mb-1.5">
            <label className="text-xs text-muted-foreground">Chunk size</label>
            <input
              type="number"
              min="100"
              max="100000"
              value={config.chunkSize}
              onChange={(e) => setConfig({ chunkSize: parseInt(e.target.value, 10) || 1000 })}
              className="w-20 rounded border bg-background px-1.5 py-0.5 text-right text-xs font-mono text-foreground"
            />
          </div>
          <input
            type="range"
            min="200"
            max="16000"
            step="100"
            value={Math.min(16000, config.chunkSize)}
            onChange={(e) => setConfig({ chunkSize: parseInt(e.target.value, 10) })}
            className="w-full"
          />
          <div className="flex justify-between text-[10px] text-muted-foreground mt-0.5">
            <span>200</span><span>16,000+</span>
          </div>
        </div>
        <div>
          <div className="flex items-center justify-between mb-1.5">
            <label className="text-xs text-muted-foreground">Overlap</label>
            <input
              type="number"
              min="0"
              max="50000"
              value={config.chunkOverlap}
              onChange={(e) => setConfig({ chunkOverlap: parseInt(e.target.value, 10) || 0 })}
              className="w-20 rounded border bg-background px-1.5 py-0.5 text-right text-xs font-mono text-foreground"
            />
          </div>
          <input
            type="range"
            min="0"
            max="4000"
            step="50"
            value={Math.min(4000, config.chunkOverlap)}
            onChange={(e) => setConfig({ chunkOverlap: parseInt(e.target.value, 10) })}
            className="w-full"
          />
          <div className="flex justify-between text-[10px] text-muted-foreground mt-0.5">
            <span>0</span><span>4,000+</span>
          </div>
        </div>
      </div>

      {/* Dimension hint — driven by selected model, falls back to provider default */}
      {(() => {
        const dim = MODEL_DIMS[config.model] ?? PROVIDER_DEFAULTS[config.provider].dim;
        if (!dim) return null;
        return (
          <div className="rounded-lg border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
            Output dimension:{" "}
            <span className="font-mono text-foreground font-medium">{dim}</span>
            {" "}— make sure{" "}
            <code className="font-mono">VALORI_DIM={dim}</code>
            {" "}on the server.
          </div>
        );
      })()}
    </div>
  );
}
