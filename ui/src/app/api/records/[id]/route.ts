import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

export async function GET(
  req: NextRequest,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    const collection = req.nextUrl.searchParams.get("collection") ?? "";
    const qs = collection ? `?collection=${encodeURIComponent(collection)}` : "";
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/records/${id}${qs}`, { headers: nodeHeaders(false) });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}