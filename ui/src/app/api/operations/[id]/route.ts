import { NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET(
  request: Request,
  context: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await context.params;
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetchWithTimeout(`${getApiUrl()}/v1/operations/${encodeURIComponent(id)}`, { headers, cache: "no-store" });
    const body = await res.json().catch(() => ({ error: "Failed to parse response" }));

    return NextResponse.json(body, { status: res.status });
  } catch (err) {
    return NextResponse.json({ error: "Failed to fetch operation detail" }, { status: 503 });
  }
}