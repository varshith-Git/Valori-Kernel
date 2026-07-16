import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET(
  req: NextRequest,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const collection = req.nextUrl.searchParams.get("collection") ?? "";
    const qs = collection ? `?collection=${encodeURIComponent(collection)}` : "";
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/records/${id}${qs}`, { headers });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}