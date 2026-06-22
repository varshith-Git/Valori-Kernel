import { NextRequest, NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function authHeaders(): Record<string, string> {
  const h: Record<string, string> = {};
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export async function GET(req: NextRequest) {
  try {
    const collection = req.nextUrl.searchParams.get("collection");
    const url = new URL(`${getApiUrl()}/graph/nodes`);
    if (collection) url.searchParams.set("collection", collection);
    const res = await fetch(url.toString(), {
      headers: authHeaders(),
      cache: "no-store",
    });
    const data = await res.json().catch(() => ({ nodes: [], count: 0 }));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ nodes: [], count: 0 }, { status: 503 });
  }
}
