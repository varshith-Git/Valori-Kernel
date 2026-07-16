export interface RerankerConfig {
  provider: "cohere" | "custom";
  apiKey?: string;
  model?: string;
  endpoint?: string;
}

export type RerankCandidate = {
  record_id: number;
  score?: number;
  metadata: Record<string, unknown> | null;
};

export type RerankResult = RerankCandidate & { rerank_score: number | null };

export async function rerankChunks(
  query: string,
  chunks: RerankCandidate[],
  cfg: RerankerConfig,
): Promise<RerankResult[]> {
  const docs = chunks.map((c) => (c.metadata?.text as string) ?? "");

  try {
    if (cfg.provider === "cohere") {
      const endpoint = (cfg.endpoint || "https://api.cohere.ai/v2/rerank").replace(/\/$/, "");
      const res = await fetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey ?? ""}` },
        body: JSON.stringify({ model: cfg.model || "rerank-english-v3.0", query, documents: docs, top_n: chunks.length }),
      });
      if (!res.ok) throw new Error(`Cohere rerank ${res.status}`);
      const d = await res.json() as { results: { index: number; relevance_score: number }[] };
      const scoreMap = new Map(d.results.map((r) => [r.index, r.relevance_score]));
      return [...chunks]
        .map((c, i) => ({ ...c, rerank_score: scoreMap.get(i) ?? null }))
        .sort((a, b) => (b.rerank_score ?? -1) - (a.rerank_score ?? -1));
    }
    if (cfg.provider === "custom" && cfg.endpoint) {
      const res = await fetch(cfg.endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json", ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}) },
        body: JSON.stringify({ query, documents: docs }),
      });
      if (!res.ok) throw new Error(`Custom reranker ${res.status}`);
      const d = await res.json() as { scores?: number[] };
      if (!Array.isArray(d.scores)) throw new Error("Custom reranker: expected scores[]");
      return chunks
        .map((c, i) => ({ ...c, rerank_score: d.scores![i] ?? null }))
        .sort((a, b) => (b.rerank_score ?? -1) - (a.rerank_score ?? -1));
    }
  } catch { /* reranker failure must not block the answer */ }

  return chunks.map((c) => ({ ...c, rerank_score: null }));
}
