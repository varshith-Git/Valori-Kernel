import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function authHeaders(): Record<string, string> {
  const h: Record<string, string> = {};
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export async function GET(
  _req: NextRequest,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    const res = await fetchWithTimeout(`${getApiUrl()}/graph/edges/${id}`, {
      headers: authHeaders(),
      cache: "no-store",
    });
    const data = await res.json().catch(() => ({ edges: [] }));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ edges: [] }, { status: 503 });
  }
}