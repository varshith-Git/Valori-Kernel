import { NextRequest, NextResponse } from "next/server";

// Shared embedding logic — same providers as ingest, single-text variant.
interface EmbedConfig {
  provider: string;
  model: string;
  apiKey: string;
  endpoint: string;
}

async function embedOne(text: string, cfg: EmbedConfig): Promise<number[]> {
  switch (cfg.provider) {
    case "openai": {
      const res = await fetch(cfg.endpoint || "https://api.openai.com/v1/embeddings", {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey}` },
        body: JSON.stringify({ input: text, model: cfg.model || "text-embedding-3-small" }),
      });
      if (!res.ok) {
        const e = await res.json().catch(() => ({})) as { error?: { message?: string } };
        throw new Error(`OpenAI: ${e.error?.message ?? res.status}`);
      }
      const d = await res.json() as { data: { embedding: number[] }[] };
      return d.data[0].embedding;
    }
    case "cohere": {
      const res = await fetch(cfg.endpoint || "https://api.cohere.ai/v1/embed", {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey}` },
        body: JSON.stringify({
          texts: [text],
          model: cfg.model || "embed-english-v3.0",
          input_type: "search_query",
          embedding_types: ["float"],
        }),
      });
      if (!res.ok) throw new Error(`Cohere: ${res.status}`);
      const d = await res.json() as { embeddings: { float: number[][] } };
      return d.embeddings.float[0];
    }
    case "ollama": {
      const base = (cfg.endpoint || "http://localhost:11434")
        .replace(/\/api\/embed(?:dings)?$/, "")
        .replace(/\/$/, "");
      const model = cfg.model || "nomic-embed-text";
      const safeText = text.slice(0, 1800);

      let res = await fetch(`${base}/api/embed`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ model, input: safeText }),
      });
      if (res.status === 404) {
        res = await fetch(`${base}/api/embeddings`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ model, prompt: safeText }),
        });
        if (!res.ok) {
          const b = await res.json().catch(() => ({})) as { error?: string };
          if (res.status === 404) throw new Error(`Ollama model "${model}" not found — run: ollama pull ${model}`);
          throw new Error(`Ollama: ${b.error ?? res.status}`);
        }
        const d = await res.json() as { embedding: number[] };
        return d.embedding;
      }
      if (!res.ok) {
        const b = await res.json().catch(() => ({})) as { error?: string };
        throw new Error(`Ollama: ${b.error ?? res.status}`);
      }
      const d = await res.json() as { embeddings: number[][] };
      return d.embeddings[0];
    }
    case "custom": {
      const res = await fetch(cfg.endpoint, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
        },
        body: JSON.stringify({ input: text, model: cfg.model }),
      });
      if (!res.ok) throw new Error(`Custom endpoint: ${res.status}`);
      const d = await res.json() as { embedding?: number[]; embeddings?: number[][] };
      if (d.embedding) return d.embedding;
      if (d.embeddings) return d.embeddings[0];
      throw new Error("Unexpected shape from custom endpoint");
    }
    default:
      throw new Error(`Unknown provider: ${cfg.provider}`);
  }
}

export async function POST(req: NextRequest) {
  try {
    const { text, provider, model, apiKey, endpoint } = await req.json() as EmbedConfig & { text: string };
    if (!text?.trim()) return NextResponse.json({ error: "text is required" }, { status: 400 });
    const vector = await embedOne(text, { provider, model, apiKey, endpoint });
    return NextResponse.json({ vector, dim: vector.length });
  } catch (err) {
    return NextResponse.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}
