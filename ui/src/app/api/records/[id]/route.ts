import { NextRequest, NextResponse } from "next/server";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET(
  req: NextRequest,
  { params }: { params: { id: string } }
) {
  try {
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const collection = req.nextUrl.searchParams.get("collection") ?? "";
    const qs = collection ? `?collection=${encodeURIComponent(collection)}` : "";
    const res = await fetch(`${getApiUrl()}/v1/records/${params.id}${qs}`, { headers });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
