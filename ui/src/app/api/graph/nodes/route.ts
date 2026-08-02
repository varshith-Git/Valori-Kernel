import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";

export async function GET(req: NextRequest) {
  try {
    const params = req.nextUrl.searchParams;
    const url = new URL(`${getApiUrl()}/graph/nodes`);
    for (const key of ["collection", "kind", "limit", "offset"]) {
      const v = params.get(key);
      if (v !== null) url.searchParams.set(key, v);
    }
    const res = await fetchWithTimeout(url.toString(), {
      headers: nodeHeaders(false),
      cache: "no-store",
    });
    const data = await res.json().catch(() => ({ nodes: [], count: 0 }));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ nodes: [], count: 0 }, { status: 503 });
  }
}