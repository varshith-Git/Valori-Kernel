import { NextRequest, NextResponse } from "next/server";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

function headers(): Record<string, string> {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

/** POST /api/community?action=detect|search */
export async function POST(req: NextRequest) {
  try {
    const { searchParams } = new URL(req.url);
    const action = searchParams.get("action") ?? "detect";
    const body = await req.json();

    const endpoint = action === "search"
      ? "/v1/community/search"
      : "/v1/community/detect";

    const res = await fetch(`${getApiUrl()}${endpoint}`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify(body),
    });
    const data = await res.json();
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
