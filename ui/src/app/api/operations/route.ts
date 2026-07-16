import { NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET() {
  try {
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetchWithTimeout(`${getApiUrl()}/v1/operations`, { headers, cache: "no-store" });
    const body = await res.json().catch(() => ({ operations: [], total: 0 }));

    return NextResponse.json(body, { status: res.status });
  } catch (err) {
    return NextResponse.json({ operations: [], total: 0, error: "Failed to fetch operations" }, { status: 503 });
  }
}