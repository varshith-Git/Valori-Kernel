import { NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

export async function GET() {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/operations`, { headers: nodeHeaders(false), cache: "no-store" });
    const body = await res.json().catch(() => ({ operations: [], total: 0 }));

    return NextResponse.json(body, { status: res.status });
  } catch (err) {
    return NextResponse.json({ operations: [], total: 0, error: "Failed to fetch operations" }, { status: 503 });
  }
}