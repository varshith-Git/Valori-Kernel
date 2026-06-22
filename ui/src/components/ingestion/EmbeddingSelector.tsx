"use client";

import { useEmbeddingConfig, PROVIDER_DEFAULTS, EmbeddingProvider } from "@/lib/hooks/useEmbeddingConfig";

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
          <label className="text-xs text-muted-foreground block mb-1.5">Model</label>
          {MODELS[config.provider].length > 0 ? (
            <select
              value={config.model}
              onChange={(e) => setConfig({ model: e.target.value })}
              className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm text-card-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            >
              {MODELS[config.provider].map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
          ) : (
            <input
              type="text"
              value={config.model}
              onChange={(e) => setConfig({ model: e.target.value })}
              placeholder="e.g. text-embedding-3-small"
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
          <label className="text-xs text-muted-foreground block mb-1.5">
            Chunk size <span className="text-muted-foreground">({config.chunkSize} chars)</span>
          </label>
          <input
            type="range"
            min="200"
            max="4000"
            step="100"
            value={config.chunkSize}
            onChange={(e) => setConfig({ chunkSize: parseInt(e.target.value, 10) })}
            className="w-full"
          />
          <div className="flex justify-between text-[10px] text-zinc-700 mt-0.5">
            <span>200</span><span>4000</span>
          </div>
        </div>
        <div>
          <label className="text-xs text-muted-foreground block mb-1.5">
            Overlap <span className="text-muted-foreground">({config.chunkOverlap} chars)</span>
          </label>
          <input
            type="range"
            min="0"
            max="500"
            step="50"
            value={config.chunkOverlap}
            onChange={(e) => setConfig({ chunkOverlap: parseInt(e.target.value, 10) })}
            className="w-full"
          />
          <div className="flex justify-between text-[10px] text-zinc-700 mt-0.5">
            <span>0</span><span>500</span>
          </div>
        </div>
      </div>

      {/* Dimension hint */}
      {PROVIDER_DEFAULTS[config.provider].dim > 0 && (
        <div className="rounded-lg border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
          Output dimension:{" "}
          <span className="font-mono text-accent-foreground">
            {PROVIDER_DEFAULTS[config.provider].dim}
          </span>
          {" "}— make sure{" "}
          <code className="text-muted-foreground">VALORI_DIM={PROVIDER_DEFAULTS[config.provider].dim}</code>
          {" "}on the server.
        </div>
      )}
    </div>
  );
}
