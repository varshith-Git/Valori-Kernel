"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import { useCredentialStore } from "@/runtime/context";

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

/**
 * Persisted shape written to `localStorage[STORAGE_KEY]`. Identical
 * desktop/web split as before extraction (Studio S3): a host that supplies
 * a `CredentialStore` (Desktop, backed by the OS keychain) persists
 * `credentialRef`, never `apiKey`; a host that doesn't (Cloud Web) persists
 * `apiKey` directly — unchanged, documented behavior, not altered by this
 * extraction. One thing deliberately NOT ported: the one-time
 * `migrateLegacyProviderCredential` upgrade path for installations that
 * predate the OS-keychain scheme — that is a Desktop-specific historical
 * migration, not a `CredentialStore` concern, and stays the host's job if
 * still needed (see native.ts on the Desktop side).
 */
interface PersistedEmbeddingConfig {
  provider: EmbeddingProvider;
  model: string;
  endpoint: string;
  chunkSize: number;
  chunkOverlap: number;
  apiKey?: string;
  credentialRef?: string;
}

export function useEmbeddingConfig() {
  const credentials = useCredentialStore();
  const [config, setConfigState] = useState<EmbeddingConfig>(DEFAULT_CONFIG);
  const [loaded, setLoaded] = useState(false);
  const credentialRefState = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      let raw: string | null = null;
      try {
        raw = localStorage.getItem(STORAGE_KEY);
      } catch {}
      const persisted: Partial<PersistedEmbeddingConfig> = raw ? JSON.parse(raw) : {};

      if (credentials && persisted.credentialRef) {
        let resolved = "";
        try {
          resolved = (await credentials.get(persisted.credentialRef)) ?? "";
        } catch {
          resolved = "";
        }
        if (!cancelled) {
          credentialRefState.current = persisted.credentialRef;
          setConfigState({ ...DEFAULT_CONFIG, ...persisted, apiKey: resolved });
        }
      } else {
        if (!cancelled) {
          setConfigState({ ...DEFAULT_CONFIG, ...persisted, apiKey: persisted.apiKey ?? "" });
        }
      }
      if (!cancelled) setLoaded(true);
    }

    load().catch(() => {
      if (!cancelled) setLoaded(true);
    });

    return () => {
      cancelled = true;
    };
  }, [credentials]);

  useEffect(() => {
    if (!loaded) return;

    async function persist() {
      if (!credentials) {
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
        } catch {}
        return;
      }

      const base = {
        provider: config.provider,
        model: config.model,
        endpoint: config.endpoint,
        chunkSize: config.chunkSize,
        chunkOverlap: config.chunkOverlap,
      };

      if (!config.apiKey) {
        if (credentialRefState.current) {
          await credentials.delete(credentialRefState.current).catch(() => {});
        }
        credentialRefState.current = null;
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(base));
        } catch {}
        return;
      }

      const currentlyResolved = credentialRefState.current
        ? await credentials.get(credentialRefState.current).catch(() => null)
        : null;

      if (currentlyResolved === config.apiKey) {
        try {
          localStorage.setItem(
            STORAGE_KEY,
            JSON.stringify({ ...base, credentialRef: credentialRefState.current }),
          );
        } catch {}
        return;
      }

      // Reuse the existing reference if this field already has one (e.g.
      // the user is still typing) — avoids one orphaned keychain entry
      // per keystroke. Only mint fresh when none exists yet.
      try {
        const ref = await credentials.store(config.apiKey, credentialRefState.current ?? undefined);
        credentialRefState.current = ref;
        localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...base, credentialRef: ref }));
      } catch {
        // Store unavailable — fail closed, retry on next save.
      }
    }

    persist();
  }, [config, loaded, credentials]);

  const setConfig = useCallback((update: Partial<EmbeddingConfig> | ((prev: EmbeddingConfig) => EmbeddingConfig)) => {
    setConfigState((prev) => {
      return typeof update === "function" ? update(prev) : { ...prev, ...update };
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
