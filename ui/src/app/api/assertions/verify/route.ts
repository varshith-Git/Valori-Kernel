import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";
export async function POST(req: NextRequest) {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/assertions/verify`, { method: "POST", headers: nodeHeaders(), body: await req.text() });
    return NextResponse.json(await res.json(), { status: res.status });
  } catch { return NextResponse.json({ error: "backend unreachable" }, { status: 503 }); }
}
