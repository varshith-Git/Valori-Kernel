import { NextRequest, NextResponse } from "next/server";

interface SuggestRequest {
  chunks: string[];          // text previews from ingested document
  source?: string;           // document filename for context
  llm: {
    provider: "ollama" | "openai" | "groq" | "together" | "custom";
    model: string;
    apiKey?: string;
    endpoint?: string;
  };
}

async function callLLM(prompt: string, cfg: SuggestRequest["llm"]): Promise<string> {
  const messages = [
    {
      role: "system" as const,
      content:
        "You are an expert at generating insightful questions from document content. " +
        "Return ONLY a plain numbered list, one question per line. No preamble, no explanations.",
    },
    { role: "user" as const, content: prompt },
  ];

  if (cfg.provider === "ollama") {
    const base = cfg.endpoint?.replace(/\/$/, "") || "http://localhost:11434";
    const res = await fetch(`${base}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: cfg.model || "llama3.2", messages, stream: false }),
    });
    if (!res.ok) throw new Error(`Ollama ${res.status}: ${await res.text().catch(() => "")}`);
    const d = await res.json() as { message?: { content?: string } };
    return d.message?.content ?? "";
  }

  const baseMap: Record<string, string> = {
    openai: "https://api.openai.com",
    groq: "https://api.groq.com/openai",
    together: "https://api.together.xyz",
  };
  const base = cfg.endpoint?.replace(/\/$/, "") || baseMap[cfg.provider] || "";
  if (!base) throw new Error("No endpoint configured");

  const res = await fetch(`${base}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
    },
    body: JSON.stringify({ model: cfg.model, messages, max_tokens: 512, temperature: 0.4 }),
  });
  if (!res.ok) throw new Error(`${cfg.provider} ${res.status}: ${(await res.text().catch(() => "")).slice(0, 200)}`);
  const d = await res.json() as { choices?: { message?: { content?: string } }[] };
  return d.choices?.[0]?.message?.content ?? "";
}

function parseQuestions(raw: string): string[] {
  return raw
    .split("\n")
    .map((line) => line.replace(/^\s*\d+[\.\)]\s*/, "").trim())
    .filter((line) => line.length > 10 && line.includes(" "))
    .slice(0, 8);
}

export async function POST(req: NextRequest) {
  try {
    const body: SuggestRequest = await req.json();
    const { chunks, source, llm } = body;

    if (!chunks?.length) {
      return NextResponse.json({ error: "chunks required" }, { status: 400 });
    }

    // Use up to 12 chunks, prioritise first + last + evenly distributed middle
    const maxChunks = 12;
    let selected = chunks;
    if (chunks.length > maxChunks) {
      const step = Math.floor(chunks.length / maxChunks);
      selected = Array.from({ length: maxChunks }, (_, i) => chunks[Math.min(i * step, chunks.length - 1)]);
    }

    const context = selected
      .map((text, i) => `[Chunk ${i + 1}]\n${text.slice(0, 600)}`)
      .join("\n\n");

    const docHint = source ? ` about "${source}"` : "";
    const prompt =
      `Generate exactly 8 high-quality questions${docHint} that a reader would want to ask after reading these document excerpts.\n\n` +
      `Make questions specific and answerable from the document content. Vary the depth: ` +
      `some factual, some analytical, some comparative.\n\nDocument excerpts:\n\n${context}\n\n` +
      `Return exactly 8 numbered questions, one per line.`;

    const raw = await callLLM(prompt, llm);
    const questions = parseQuestions(raw);

    if (questions.length === 0) {
      return NextResponse.json({ error: "LLM returned no parseable questions", raw }, { status: 502 });
    }

    return NextResponse.json({ questions });
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : String(err) },
      { status: 500 }
    );
  }
}
