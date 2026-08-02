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
  let out = "";
  for await (const token of streamLLM(systemPrompt, userMessage, cfg)) {
    out += token;
  }
  return out;
}

/** Yields tokens one at a time so callers can stream to the client. */
export async function* streamLLM(
  systemPrompt: string,
  userMessage: string,
  cfg: LLMConfig,
): AsyncGenerator<string> {
  const messages = [
    { role: "system", content: systemPrompt },
    { role: "user", content: userMessage },
  ];

  if (cfg.provider === "ollama") {
    const base = cfg.endpoint?.replace(/\/$/, "") || "http://localhost:11434";
    const res = await fetch(`${base}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: cfg.model || "llama3.2", messages, stream: true, options: { temperature: 0 } }),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => res.status.toString());
      throw new Error(`Ollama error (${res.status}): ${text}`);
    }
    const reader = res.body!.getReader();
    const dec = new TextDecoder();
    let buf = "";
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      const lines = buf.split("\n");
      buf = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const obj = JSON.parse(line) as { message?: { content?: string }; done?: boolean };
          if (obj.message?.content) yield obj.message.content;
        } catch { /* skip malformed */ }
      }
    }
    return;
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
    body: JSON.stringify({ model: cfg.model, messages, max_tokens: 512, temperature: 0, stream: true }),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.status.toString());
    throw new Error(`${cfg.provider} error (${res.status}): ${text.slice(0, 200)}`);
  }
  const reader = res.body!.getReader();
  const dec = new TextDecoder();
  let buf = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += dec.decode(value, { stream: true });
    const lines = buf.split("\n");
    buf = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.startsWith("data: ")) continue;
      const payload = line.slice(6).trim();
      if (payload === "[DONE]") return;
      try {
        const obj = JSON.parse(payload) as { choices?: { delta?: { content?: string } }[] };
        const token = obj.choices?.[0]?.delta?.content;
        if (token) yield token;
      } catch { /* skip */ }
    }
  }
}
