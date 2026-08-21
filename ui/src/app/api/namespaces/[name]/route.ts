import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";
import * as daemon from "@/lib/server/daemon";

export async function DELETE(
  req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  try {
    const { name } = await params;
    const project = req.nextUrl.searchParams.get("project");
    let baseUrl = getApiUrl();
    if (project) {
      try {
        const p = await daemon.getProject(project);
        const port = p.cluster?.nodes?.[0]?.http_port ?? p.status?.port;
        if (port) baseUrl = `http://127.0.0.1:${port}`;
      } catch {}
    }
    const res = await fetchWithTimeout(
      `${baseUrl}/v1/namespaces/${encodeURIComponent(name)}`,
      { method: "DELETE", headers: nodeHeaders(false) }
    );
    if (res.status === 204) {
      return new NextResponse(null, { status: 204 });
    }
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}