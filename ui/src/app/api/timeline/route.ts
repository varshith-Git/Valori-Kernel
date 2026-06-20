import { NextResponse } from "next/server";

const API = process.env.VALORI_API_URL ?? "http://localhost:3000";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET() {
  try {
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetch(`${API}/timeline`, { headers, cache: "no-store" });
    // 400 = event log not enabled — pass through so the UI can handle it
    const data = await res.json().catch(() => []);
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
