import { NextRequest, NextResponse } from "next/server";
import { embedOne, type EmbedConfig } from "@/lib/server/embed";

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