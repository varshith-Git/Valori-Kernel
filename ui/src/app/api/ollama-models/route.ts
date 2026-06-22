import { NextResponse } from "next/server";

const OLLAMA = process.env.OLLAMA_URL ?? "http://localhost:11434";

// GET /api/ollama-models — returns list of locally installed Ollama model names
export async function GET() {
  try {
    const res = await fetch(`${OLLAMA}/api/tags`, { cache: "no-store" });
    if (!res.ok) return NextResponse.json({ models: [], error: `ollama returned ${res.status}` });
    const data = await res.json() as { models?: { name: string }[] };
    const models = (data.models ?? []).map((m) => m.name);
    return NextResponse.json({ models });
  } catch {
    return NextResponse.json({ models: [], error: "ollama not reachable" });
  }
}
