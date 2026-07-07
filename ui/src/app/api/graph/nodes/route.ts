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
    const params = req.nextUrl.searchParams;
    const url = new URL(`${getApiUrl()}/graph/nodes`);
    for (const key of ["collection", "kind", "limit", "offset"]) {
      const v = params.get(key);
      if (v !== null) url.searchParams.set(key, v);
    }
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
