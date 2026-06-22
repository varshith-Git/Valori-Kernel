"use client";

import { useState, useEffect, useCallback } from "react";

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

export function useLLMConfig() {
  const [config, setConfigState] = useState<LLMConfig>(DEFAULT_CONFIG);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) setConfigState({ ...DEFAULT_CONFIG, ...JSON.parse(raw) });
    } catch {}
    setLoaded(true);
  }, []);

  const setConfig = useCallback((update: Partial<LLMConfig> | ((prev: LLMConfig) => LLMConfig)) => {
    setConfigState((prev) => {
      const next = typeof update === "function" ? update(prev) : { ...prev, ...update };
      try { localStorage.setItem(STORAGE_KEY, JSON.stringify(next)); } catch {}
      return next;
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
