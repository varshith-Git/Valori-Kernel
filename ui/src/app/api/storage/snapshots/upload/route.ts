import { NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function authHeaders() {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export async function POST() {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/storage/snapshots/upload`, {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({}),
    });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}