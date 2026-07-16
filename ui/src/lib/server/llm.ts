export interface LLMConfig {
  provider: "ollama" | "openai" | "groq" | "together" | "custom";
  model: string;
  apiKey?: string;
  endpoint?: string;
}

export async function callLLM(
  systemPrompt: string,
  userMessage: string,
  cfg: LLMConfig,
): Promise<string> {
  const messages = [
    { role: "system", content: systemPrompt },
    { role: "user", content: userMessage },
  ];

  if (cfg.provider === "ollama") {
    const base = cfg.endpoint?.replace(/\/$/, "") || "http://localhost:11434";
    const res = await fetch(`${base}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: cfg.model || "llama3.2", messages, stream: false, options: { temperature: 0 } }),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => res.status.toString());
      throw new Error(`Ollama error (${res.status}): ${text}`);
    }
    const data = await res.json() as { message?: { content?: string } };
    return data.message?.content ?? "";
  }

  const baseMap: Record<string, string> = {
    openai: "https://api.openai.com",
    groq: "https://api.groq.com/openai",
    together: "https://api.together.xyz",
  };
  const base = cfg.endpoint?.replace(/\/$/, "") || baseMap[cfg.provider] || "";
  if (!base) throw new Error("No endpoint configured for custom provider");

  const res = await fetch(`${base}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
    },
    body: JSON.stringify({ model: cfg.model, messages, max_tokens: 512, temperature: 0 }),
  });

  if (!res.ok) {
    const text = await res.text().catch(() => res.status.toString());
    throw new Error(`${cfg.provider} error (${res.status}): ${text.slice(0, 200)}`);
  }
  const data = await res.json() as { choices?: { message?: { content?: string } }[] };
  return data.choices?.[0]?.message?.content ?? "";
}
