import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

/** POST /api/community?action=detect|search */
export async function POST(req: NextRequest) {
  try {
    const action = req.nextUrl.searchParams.get("action") ?? "detect";
    const body = await req.json();

    const endpoint = action === "search"
      ? "/v1/community/search"
      : "/v1/community/detect";

    const res = await fetchWithTimeout(`${getApiUrl()}${endpoint}`, {
      method: "POST",
      headers: nodeHeaders(),
      body: JSON.stringify(body),
    });
    const data = await res.json();
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}