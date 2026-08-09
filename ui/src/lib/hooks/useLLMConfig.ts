"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import {
  nativeAvailable,
  credentialStore,
  credentialGet,
  credentialDelete,
  migrateLegacyProviderCredential,
} from "@/lib/native";

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
 * Desktop (Tauri): `{ provider, model, endpoint, credentialRef }` — the
 * secret itself is never written here (Studio S3 — see
 * `docs/reviews/studio-credentials-audit.md`,
 * `docs/phases/phase-studio-S3-credentials.md`). `apiKey` may be
 * transiently present only during migration (`native.ts`'s
 * `migrateLegacyProviderCredential`) or as a fail-closed fallback if the
 * OS credential store is unavailable when the user saves a new key — see
 * the persistence effect below.
 *
 * Browser (Valori Cloud web, not Tauri): unchanged from before this phase
 * — `{ provider, model, endpoint, apiKey }`, plaintext in `localStorage`.
 * There is no OS keychain to move it to; see the phase doc's "Desktop vs
 * Web" section for why this is a documented, not silently accepted, gap.
 */
interface PersistedLLMConfig {
  provider: LLMProvider;
  model: string;
  endpoint: string;
  apiKey?: string;
  credentialRef?: string;
}

export function useLLMConfig() {
  const [config, setConfigState] = useState<LLMConfig>(DEFAULT_CONFIG);
  const [loaded, setLoaded] = useState(false);
  // The credential reference currently backing `config.apiKey`, desktop
  // only. `null` = no stored credential yet (empty key, or web mode).
  const credentialRefState = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      if (nativeAvailable()) {
        await migrateLegacyProviderCredential(STORAGE_KEY);
      }

      let raw: string | null = null;
      try {
        raw = localStorage.getItem(STORAGE_KEY);
      } catch {}
      const persisted: Partial<PersistedLLMConfig> = raw ? JSON.parse(raw) : {};

      if (nativeAvailable() && persisted.credentialRef) {
        // Resolve the secret in-memory only — never written back to
        // localStorage as apiKey.
        let resolved = "";
        try {
          resolved = (await credentialGet(persisted.credentialRef)) ?? "";
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
        // Web mode, or desktop with no credentialRef yet (migration
        // failed / keychain unavailable — fail-closed fallback keeps
        // whatever apiKey is still in localStorage so the provider keeps
        // working, per the phase doc's "degrade gracefully" rule).
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
  }, []);

  useEffect(() => {
    if (!loaded) return;

    async function persist() {
      if (!nativeAvailable()) {
        // Web mode — unchanged behavior, apiKey stored directly.
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
        } catch {}
        return;
      }

      // Desktop: never write apiKey to localStorage. Store/rotate the
      // keychain entry only when the key actually changed.
      const base = { provider: config.provider, model: config.model, endpoint: config.endpoint };

      if (!config.apiKey) {
        // User cleared the key — delete the credential (idempotent) and
        // persist with no credentialRef.
        if (credentialRefState.current) {
          await credentialDelete(credentialRefState.current).catch(() => {});
        }
        credentialRefState.current = null;
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(base));
        } catch {}
        return;
      }

      const currentlyResolved = credentialRefState.current
        ? await credentialGet(credentialRefState.current).catch(() => null)
        : null;

      if (currentlyResolved === config.apiKey) {
        // Key unchanged (only provider/model/endpoint changed) — keep the
        // same credentialRef, no keychain write needed.
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
        const ref = await credentialStore(config.apiKey, credentialRefState.current ?? undefined);
        credentialRefState.current = ref;
        localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...base, credentialRef: ref }));
      } catch {
        // Keychain unavailable — fail closed: keep the key working for
        // this session via in-memory state, but do not persist plaintext.
        // The next successful save will retry storing it.
      }
    }

    persist();
  }, [config, loaded]);

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
