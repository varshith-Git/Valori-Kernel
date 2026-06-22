import { NextRequest, NextResponse } from "next/server";
import { pm, LaunchConfig } from "@/lib/server/process-manager";

// GET /api/launch — all node statuses
export async function GET() {
  return NextResponse.json({ nodes: pm.getAllStatus(), repoRoot: pm.repoRoot });
}

// POST /api/launch — start a node
// body: { config: LaunchConfig, nodeIdx: number }
export async function POST(req: NextRequest) {
  try {
    const { config, nodeIdx } = await req.json() as { config: LaunchConfig; nodeIdx: number };
    if (!config || nodeIdx == null) {
      return NextResponse.json({ error: "config and nodeIdx required" }, { status: 400 });
    }
    const state = pm.startNode(config, nodeIdx);
    return NextResponse.json(state);
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}

// DELETE /api/launch?nodeId=1 — stop a node
export async function DELETE(req: NextRequest) {
  const id = Number(req.nextUrl.searchParams.get("nodeId"));
  if (!id) return NextResponse.json({ error: "nodeId required" }, { status: 400 });
  const ok = pm.stopNode(id);
  return NextResponse.json({ ok });
}
