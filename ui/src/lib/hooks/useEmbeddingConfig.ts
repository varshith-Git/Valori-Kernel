"use client";

import { useState, useEffect, useCallback } from "react";

export type EmbeddingProvider = "openai" | "cohere" | "ollama" | "custom";

export interface EmbeddingConfig {
  provider: EmbeddingProvider;
  model: string;
  apiKey: string;
  endpoint: string;
  chunkSize: number;
  chunkOverlap: number;
}

export const PROVIDER_DEFAULTS: Record<EmbeddingProvider, { model: string; endpoint: string; dim: number }> = {
  openai: { model: "text-embedding-3-small", endpoint: "https://api.openai.com/v1/embeddings", dim: 1536 },
  cohere: { model: "embed-english-v3.0", endpoint: "https://api.cohere.ai/v1/embed", dim: 1024 },
  ollama: { model: "nomic-embed-text", endpoint: "http://localhost:11434/api/embed", dim: 768 },
  custom: { model: "", endpoint: "", dim: 0 },
};

export const MODEL_DIMS: Record<string, number> = {
  // OpenAI
  "text-embedding-3-small":  1536,
  "text-embedding-3-large":  3072,
  "text-embedding-ada-002":  1536,
  // Cohere
  "embed-english-v3.0":       1024,
  "embed-multilingual-v3.0":  1024,
  "embed-english-light-v3.0": 384,
  // Ollama
  "nomic-embed-text":  768,
  "mxbai-embed-large": 1024,
  "all-minilm":        384,
};

export function getModelDim(provider: EmbeddingProvider, model: string): number {
  return MODEL_DIMS[model] ?? PROVIDER_DEFAULTS[provider]?.dim ?? 0;
}

export function registerModelDims(dims: Record<string, number>) {
  Object.assign(MODEL_DIMS, dims);
}

const STORAGE_KEY = "valori:embedding_config";

const DEFAULT_CONFIG: EmbeddingConfig = {
  provider: "openai",
  model: "text-embedding-3-small",
  apiKey: "",
  endpoint: "https://api.openai.com/v1/embeddings",
  chunkSize: 1000,
  chunkOverlap: 200,
};

export function useEmbeddingConfig() {
  const [config, setConfigState] = useState<EmbeddingConfig>(DEFAULT_CONFIG);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) setConfigState({ ...DEFAULT_CONFIG, ...JSON.parse(raw) });
    } catch {}
    setLoaded(true);
  }, []);

  const setConfig = useCallback((update: Partial<EmbeddingConfig> | ((prev: EmbeddingConfig) => EmbeddingConfig)) => {
    setConfigState((prev) => {
      const next = typeof update === "function" ? update(prev) : { ...prev, ...update };
      try { localStorage.setItem(STORAGE_KEY, JSON.stringify(next)); } catch {}
      return next;
    });
  }, []);

  const setProvider = useCallback((provider: EmbeddingProvider) => {
    const defaults = PROVIDER_DEFAULTS[provider];
    setConfig((prev) => ({
      ...prev,
      provider,
      model: defaults.model,
      endpoint: defaults.endpoint,
    }));
  }, [setConfig]);

  return { config, setConfig, setProvider, loaded };
}
