import { NextRequest, NextResponse } from "next/server";

const OLLAMA = process.env.OLLAMA_URL ?? "http://localhost:11434";

// Model-name fragments that identify embedding models in Ollama.
// All known Ollama embed models match at least one of these patterns.
const EMBED_PATTERNS = ["embed", "minilm", "bge-", "bge_", "e5-", "gte-", "jina", "sentence"];

function isEmbedModel(name: string): boolean {
  const lower = name.toLowerCase();
  return EMBED_PATTERNS.some((p) => lower.includes(p));
}

async function getOllamaDim(name: string): Promise<number> {
  try {
    const res = await fetch(`${OLLAMA}/api/show`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    if (!res.ok) return 0;
    const data = await res.json() as { model_info?: Record<string, any> };
    const info = data.model_info ?? {};
    for (const [key, val] of Object.entries(info)) {
      if (typeof val === "number" && (key.endsWith(".embedding_length") || key.endsWith(".embedding_dim") || key.endsWith(".dim"))) {
        return val;
      }
    }
  } catch { /* ignore */ }
  return 0;
}

// GET /api/ollama-models?type=embed|llm|all
// type=embed  → only embedding models
// type=llm    → only chat/completion models (excludes embed models)
// type=all    → everything (default for backwards compat)
export async function GET(req: NextRequest) {
  const type = req.nextUrl.searchParams.get("type") ?? "all";

  try {
    const res = await fetch(`${OLLAMA}/api/tags`, { cache: "no-store" });
    if (!res.ok) return NextResponse.json({ models: [], error: `ollama returned ${res.status}` });
    const data = await res.json() as { models?: { name: string }[] };
    let models = (data.models ?? []).map((m) => m.name);

    if (type === "embed") models = models.filter(isEmbedModel);
    else if (type === "llm") models = models.filter((m) => !isEmbedModel(m));

    const dims: Record<string, number> = {};
    if (type === "embed" || type === "all") {
      await Promise.all(
        models.map(async (m) => {
          if (isEmbedModel(m)) {
            const dim = await getOllamaDim(m);
            if (dim > 0) {
              dims[m] = dim;
              // Also register stripped name if it ends with :latest
              if (m.endsWith(":latest")) dims[m.replace(/:latest$/, "")] = dim;
            }
          }
        })
      );
    }

    return NextResponse.json({ models, dims });
  } catch {
    return NextResponse.json({ models: [], error: "ollama not reachable" });
  }
}
