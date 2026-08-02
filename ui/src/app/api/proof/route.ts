import { NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";

export async function GET() {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/proof/state`, {
      headers: nodeHeaders(false),
      cache: "no-store",
    });
    const data = await res.json();
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}