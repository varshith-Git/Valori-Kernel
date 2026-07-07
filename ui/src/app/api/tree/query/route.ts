import { NextRequest, NextResponse } from "next/server";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

function authHeaders(): Record<string, string> {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const res = await fetch(`${getApiUrl()}/v1/tree/query`, {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify(body),
    });
    const data = await res.json().catch(() => ({ error: "invalid response" }));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "node unreachable" }, { status: 503 });
  }
}
