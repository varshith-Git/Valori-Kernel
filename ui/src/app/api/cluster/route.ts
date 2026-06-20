import { NextResponse } from "next/server";

const API = process.env.VALORI_API_URL ?? "http://localhost:3000";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET() {
  try {
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetch(`${API}/v1/cluster/status`, {
      headers,
      cache: "no-store",
    });
    // 404 = standalone mode (no cluster router mounted)
    if (res.status === 404) {
      return NextResponse.json({ standalone: true }, { status: 200 });
    }
    const data = await res.json();
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
