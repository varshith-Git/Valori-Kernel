"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import { useCredentialStore } from "@/runtime/context";

export type LLMProvider = "ollama" | "openai" | "groq" | "together" | "custom";

export interface LLMConfig {
  provider: LLMProvider;
  model: string;
  apiKey: string;
  endpoint: string;
}

export const LLM_PROVIDER_DEFAULTS: Record<LLMProvider, { label: string; endpoint: string; models: string[]; note: string }> = {
  ollama: {
    label: "Ollama",
    endpoint: "http://localhost:11434",
    models: ["llama3.2", "llama3.2:3b", "mistral", "mistral-nemo", "qwen2.5", "phi4", "phi3.5", "gemma2", "gemma:2b", "deepseek-r1:7b", "codellama"],
    note: "Free · runs locally · no API key",
  },
  openai: {
    label: "OpenAI",
    endpoint: "https://api.openai.com",
    models: ["gpt-4o-mini", "gpt-4o", "gpt-4-turbo", "gpt-3.5-turbo"],
    note: "Requires API key",
  },
  groq: {
    label: "Groq",
    endpoint: "https://api.groq.com/openai",
    models: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768", "gemma2-9b-it"],
    note: "Free tier · open models · fast",
  },
  together: {
    label: "Together AI",
    endpoint: "https://api.together.xyz",
    models: ["meta-llama/Llama-3.2-11B-Vision-Instruct-Turbo", "mistralai/Mistral-7B-Instruct-v0.3", "Qwen/Qwen2.5-72B-Instruct-Turbo"],
    note: "Hosted open models",
  },
  custom: {
    label: "Custom",
    endpoint: "",
    models: [],
    note: "Any OpenAI-compatible endpoint",
  },
};

const STORAGE_KEY = "valori:llm_config";

const DEFAULT_CONFIG: LLMConfig = {
  provider: "ollama",
  model: "llama3.2",
  apiKey: "",
  endpoint: "http://localhost:11434",
};

/**
 * Persisted shape written to `localStorage[STORAGE_KEY]`.
 *
 * A host that supplies a `CredentialStore` (Desktop, OS keychain) persists
 * `{ provider, model, endpoint, credentialRef }` — the secret itself is
 * never written here. A host that doesn't (Cloud Web) persists
 * `{ provider, model, endpoint, apiKey }`, plaintext in `localStorage` —
 * unchanged from before this extraction; there is no browser keychain to
 * move it to. See useEmbeddingConfig.ts's module comment for the one thing
 * deliberately not ported (the Desktop-only legacy-credential migration).
 */
interface PersistedLLMConfig {
  provider: LLMProvider;
  model: string;
  endpoint: string;
  apiKey?: string;
  credentialRef?: string;
}

export function useLLMConfig() {
  const credentials = useCredentialStore();
  const [config, setConfigState] = useState<LLMConfig>(DEFAULT_CONFIG);
  const [loaded, setLoaded] = useState(false);
  // The credential reference currently backing `config.apiKey`. `null` = no
  // stored credential yet (empty key, or no CredentialStore was supplied).
  const credentialRefState = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      let raw: string | null = null;
      try {
        raw = localStorage.getItem(STORAGE_KEY);
      } catch {}
      const persisted: Partial<PersistedLLMConfig> = raw ? JSON.parse(raw) : {};

      if (credentials && persisted.credentialRef) {
        // Resolve the secret in-memory only — never written back to
        // localStorage as apiKey.
        let resolved = "";
        try {
          resolved = (await credentials.get(persisted.credentialRef)) ?? "";
        } catch {
          resolved = "";
        }
        if (!cancelled) {
          credentialRefState.current = persisted.credentialRef;
          setConfigState({
            ...DEFAULT_CONFIG,
            ...persisted,
            apiKey: resolved,
          });
        }
      } else {
        // No CredentialStore, or no credentialRef yet (store unavailable —
        // fail-closed fallback keeps whatever apiKey is still in
        // localStorage so the provider keeps working).
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
        // No CredentialStore — unchanged behavior, apiKey stored directly.
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
        } catch {}
        return;
      }

      // Never write apiKey to localStorage when a store is present. Store/
      // rotate the credential only when the key actually changed.
      const base = { provider: config.provider, model: config.model, endpoint: config.endpoint };

      if (!config.apiKey) {
        // User cleared the key — delete the credential (idempotent) and
        // persist with no credentialRef.
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
        // Key unchanged (only provider/model/endpoint changed) — keep the
        // same credentialRef, no store write needed.
        try {
          localStorage.setItem(
            STORAGE_KEY,
            JSON.stringify({ ...base, credentialRef: credentialRefState.current }),
          );
        } catch {}
        return;
      }

      // A new/changed key — reuse the existing reference if this field
      // already has one (e.g. the user is still typing), otherwise mint a
      // fresh one. Reusing avoids one orphaned keychain entry per keystroke.
      try {
        const ref = await credentials.store(config.apiKey, credentialRefState.current ?? undefined);
        credentialRefState.current = ref;
        localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...base, credentialRef: ref }));
      } catch {
        // Store unavailable — fail closed: keep the key working for this
        // session via in-memory state, but do not persist plaintext. The
        // next successful save will retry storing it.
      }
    }

    persist();
  }, [config, loaded, credentials]);

  const setConfig = useCallback((update: Partial<LLMConfig> | ((prev: LLMConfig) => LLMConfig)) => {
    setConfigState((prev) => {
      return typeof update === "function" ? update(prev) : { ...prev, ...update };
    });
  }, []);

  const setProvider = useCallback((provider: LLMProvider) => {
    const defaults = LLM_PROVIDER_DEFAULTS[provider];
    setConfig((prev) => ({
      ...prev,
      provider,
      model: defaults.models[0] ?? "",
      endpoint: defaults.endpoint,
    }));
  }, [setConfig]);

  return { config, setConfig, setProvider, loaded };
}
