import { NextRequest, NextResponse } from "next/server";

const API = process.env.VALORI_API_URL ?? "http://localhost:3000";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetch(`${API}/v1/delete`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
