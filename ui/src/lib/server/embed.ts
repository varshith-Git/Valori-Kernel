import { fetchWithTimeout } from "@/lib/server/http";

export interface EmbedConfig {
  provider: string;
  model: string;
  apiKey: string;
  endpoint: string;
}

/**
 * Embed one or more texts via the configured provider.
 *
 * inputType matters for Cohere: "search_query" for retrieval queries,
 * "search_document" for content being indexed. Ignored by other providers.
 *
 * Ollama invariant: texts are sent ONE AT A TIME regardless of batch size.
 * Batching via input:[] causes Ollama to concatenate them internally, blowing
 * past the model's 512-token context window on larger documents.
 */
export async function embedTexts(
  texts: string[],
  cfg: EmbedConfig,
  inputType: "query" | "document" = "document",
): Promise<number[][]> {
  if (texts.length === 0) return [];

  switch (cfg.provider) {
    case "openai": {
      const res = await fetchWithTimeout(
        cfg.endpoint || "https://api.openai.com/v1/embeddings",
        {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey}` },
          body: JSON.stringify({ input: texts, model: cfg.model || "text-embedding-3-small" }),
        },
      );
      if (!res.ok) {
        const e = await res.json().catch(() => ({})) as { error?: { message?: string } };
        throw new Error(`OpenAI: ${e.error?.message ?? res.status}`);
      }
      const d = await res.json() as { data: { embedding: number[]; index: number }[] };
      // OpenAI may reorder by index — sort back to input order
      return d.data.sort((a, b) => a.index - b.index).map((r) => r.embedding);
    }

    case "cohere": {
      const res = await fetchWithTimeout(
        cfg.endpoint || "https://api.cohere.ai/v1/embed",
        {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey}` },
          body: JSON.stringify({
            texts,
            model: cfg.model || "embed-english-v3.0",
            input_type: inputType === "query" ? "search_query" : "search_document",
            embedding_types: ["float"],
          }),
        },
      );
      if (!res.ok) throw new Error(`Cohere: ${res.status}`);
      const d = await res.json() as { embeddings: { float: number[][] } };
      return d.embeddings.float;
    }

    case "ollama": {
      const base = (cfg.endpoint || "http://localhost:11434")
        .replace(/\/api\/embed(?:ings)?$/, "")
        .replace(/\/$/, "");
      const model = cfg.model || "nomic-embed-text";
      const results: number[][] = [];

      for (const text of texts) {
        // Truncate to ~1800 chars (~450 tokens) — stay within 512-token model context windows
        const safeText = text.slice(0, 1800);

        // Try /api/embed first (Ollama ≥ 0.1.36), fall back to /api/embeddings
        let res = await fetchWithTimeout(`${base}/api/embed`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ model, input: safeText }),
        });

        if (res.status === 404) {
          res = await fetchWithTimeout(`${base}/api/embeddings`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ model, prompt: safeText }),
          });
          if (!res.ok) {
            const b = await res.json().catch(() => ({})) as { error?: string };
            if (res.status === 404) throw new Error(`Ollama model "${model}" not found — run: ollama pull ${model}`);
            throw new Error(`Ollama: ${b.error ?? `HTTP ${res.status}`}`);
          }
          const d = await res.json() as { embedding: number[] };
          results.push(d.embedding);
          continue;
        }

        if (!res.ok) {
          const b = await res.json().catch(() => ({})) as { error?: string };
          throw new Error(`Ollama: ${b.error ?? `HTTP ${res.status}`}`);
        }
        const d = await res.json() as { embeddings: number[][] };
        results.push(d.embeddings[0]);
      }

      return results;
    }

    case "custom": {
      const res = await fetchWithTimeout(cfg.endpoint, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
        },
        body: JSON.stringify({ input: texts, model: cfg.model }),
      });
      if (!res.ok) throw new Error(`Custom endpoint: ${res.status}`);
      const d = await res.json() as { embeddings?: number[][]; data?: { embedding: number[] }[] };
      if (Array.isArray(d.embeddings)) return d.embeddings;
      if (Array.isArray(d.data)) return d.data.map((r) => r.embedding);
      throw new Error("Unexpected response shape from custom endpoint");
    }

    default:
      throw new Error(`Unknown provider: ${cfg.provider}`);
  }
}

/** Embed a single query text. Uses search_query input type for Cohere. */
export function embedOne(text: string, cfg: EmbedConfig): Promise<number[]> {
  return embedTexts([text], cfg, "query").then((r) => r[0]);
}

/** Embed a batch of document texts. Uses search_document input type for Cohere. */
export function embedBatch(texts: string[], cfg: EmbedConfig): Promise<number[][]> {
  return embedTexts(texts, cfg, "document");
}
