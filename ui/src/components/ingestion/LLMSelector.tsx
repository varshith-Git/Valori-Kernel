"use client";

import { useState, useEffect } from "react";
import { useLLMConfig, LLM_PROVIDER_DEFAULTS, LLMProvider } from "@/lib/hooks/useLLMConfig";

const PROVIDERS: LLMProvider[] = ["ollama", "openai", "groq", "together", "custom"];

export function LLMSelector() {
  const { config, setConfig, setProvider } = useLLMConfig();
  const meta = LLM_PROVIDER_DEFAULTS[config.provider];

  // Fetch locally-installed Ollama models when provider is ollama
  const [ollamaModels, setOllamaModels] = useState<string[] | null>(null);
  const [ollamaErr, setOllamaErr] = useState(false);

  useEffect(() => {
    if (config.provider !== "ollama") return;
    setOllamaModels(null);
    setOllamaErr(false);
    fetch("/api/ollama-models?type=llm")
      .then((r) => r.json())
      .then((d: { models: string[]; error?: string }) => {
        if (d.error || d.models.length === 0) { setOllamaErr(true); return; }
        setOllamaModels(d.models);
        // If current model isn't installed, switch to the first installed one
        if (!d.models.includes(config.model)) {
          setConfig({ model: d.models[0] });
        }
      })
      .catch(() => setOllamaErr(true));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [config.provider]);

  // Effective model list for the current provider
  const modelList =
    config.provider === "ollama"
      ? (ollamaModels ?? meta.models)   // live list while loading, fallback to static
      : meta.models;

  return (
    <div className="flex flex-col gap-5">
      {/* Provider cards */}
      <div>
        <p className="text-xs text-muted-foreground uppercase tracking-widest mb-3">LLM provider</p>
        <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
          {PROVIDERS.map((p) => {
            const m = LLM_PROVIDER_DEFAULTS[p];
            return (
              <button
                key={p}
                onClick={() => setProvider(p)}
                className={`relative rounded-xl border p-3 text-left transition-all overflow-hidden ${
                  config.provider === p
                    ? "border-[var(--v-accent)] bg-[var(--v-accent-muted)] shadow-sm ring-1 ring-[var(--v-accent)]/20"
                    : "border-border/60 bg-background/50 hover:border-input hover:bg-accent/30"
                }`}
              >
                <div className="flex items-center justify-between mb-1.5">
                  <p className={`text-sm font-medium ${config.provider === p ? "text-foreground" : "text-muted-foreground"}`}>{m.label}</p>
                  {config.provider === p && (
                    <div className="w-1.5 h-1.5 rounded-full bg-[var(--v-accent)] shadow-[0_0_8px_var(--v-accent)]" />
                  )}
                </div>
                <p className={`text-[10px] leading-relaxed ${config.provider === p ? "text-foreground/80" : "text-muted-foreground/70"}`}>{m.note}</p>
              </button>
            );
          })}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4">
        {/* Model */}
        <div>
          <div className="flex items-center justify-between mb-1.5">
            <label className="text-xs text-muted-foreground">Model</label>
            {config.provider === "ollama" && (
              <span className={`text-[10px] ${ollamaErr ? "text-amber-500" : ollamaModels ? "text-emerald-600" : "text-muted-foreground"}`}>
                {ollamaErr ? "⚠ ollama not reachable — showing defaults" : ollamaModels ? `${ollamaModels.length} installed` : "detecting…"}
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
              placeholder="e.g. llama3.2"
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
                config.provider === "groq" ? "gsk_..."
                : config.provider === "openai" ? "sk-..."
                : "Bearer token"
              }
              className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm font-mono text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </div>
        )}
      </div>

      {/* Endpoint override (always for ollama/custom) */}
      {(config.provider === "ollama" || config.provider === "custom") && (
        <div>
          <label className="text-xs text-muted-foreground block mb-1.5">Endpoint</label>
          <input
            type="text"
            value={config.endpoint}
            onChange={(e) => setConfig({ endpoint: e.target.value })}
            placeholder={meta.endpoint || "http://localhost:11434"}
            className="w-full rounded-lg border border-input bg-card px-3 py-2 text-sm font-mono text-card-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          />
        </div>
      )}

      {/* Ollama quick-start hint */}
      {config.provider === "ollama" && (
        <div className="rounded-lg border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground space-y-1">
          <p className="font-medium text-muted-foreground">Ollama quick-start</p>
          <p className="font-mono text-muted-foreground">
            brew install ollama{" "}&amp;&amp;{" "}ollama pull {config.model || "llama3.2"}
          </p>
          <p>No API key needed. Works offline. Free forever.</p>
        </div>
      )}

      {/* Groq free-tier hint */}
      {config.provider === "groq" && (
        <div className="rounded-lg border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
          <p>
            Free tier at{" "}
            <span className="text-muted-foreground font-mono">console.groq.com</span>
            {" "}— fast inference for Llama 3.3 70B and Mixtral.
          </p>
        </div>
      )}
    </div>
  );
}
