import { NextRequest, NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetch(`${getApiUrl()}/search`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });
    const data = await res.json();
    return NextResponse.json(
      { ...data, queried_at: new Date().toISOString() },
      { status: res.status }
    );
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
