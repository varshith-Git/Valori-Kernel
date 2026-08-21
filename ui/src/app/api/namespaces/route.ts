import { NextRequest, NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
import { nodeHeaders } from "@/lib/server/http";
import * as daemon from "@/lib/server/daemon";

// Legacy-namespace compatibility (LocalRuntime's job, per the collection-
// model reconciliation — @valori/studio's useCollections must never know
// this exists). Namespaces created before the per-project-dedicated-node
// architecture are still stored on the node as "${project}--${collection}"
// (e.g. "Demo--docs") — never renamed, never migrated. This splits on the
// FIRST "--" structurally: every namespace this node returns already
// belongs to the one project currently connected (Local's single-active-
// connection model — see docs/architecture), so there's no need to know
// that project's actual registry name to strip its own prefix. A
// namespace with no "--" is already new-style: canonical name === raw.
const LEGACY_SEP = "--";

interface RawCollectionInfo {
  name: string;
  id: number;
  [key: string]: unknown;
}

function withRawNamespace(collections: RawCollectionInfo[]) {
  const map = new Map<string, RawCollectionInfo & { rawNamespace: string }>();
  for (const c of collections) {
    const idx = c.name.indexOf(LEGACY_SEP);
    const canonicalName = idx === -1 ? c.name : c.name.slice(idx + LEGACY_SEP.length);
    const item = { ...c, name: canonicalName, rawNamespace: c.name };
    const existing = map.get(canonicalName);
    if (!existing || (existing.rawNamespace !== canonicalName && item.rawNamespace === canonicalName)) {
      map.set(canonicalName, item);
    }
  }
  return Array.from(map.values());
}

async function resolveNodeUrl(project?: string | null): Promise<string> {
  if (project) {
    try {
      const p = await daemon.getProject(project);
      const port = p.cluster?.nodes?.[0]?.http_port ?? p.status?.port;
      if (port) return `http://127.0.0.1:${port}`;
    } catch {}
  }
  return getApiUrl();
}

export async function GET(req: NextRequest) {
  try {
    const project = req.nextUrl.searchParams.get("project");
    const baseUrl = await resolveNodeUrl(project);
    const res = await fetch(`${baseUrl}/v1/namespaces`, {
      headers: nodeHeaders(false),
      cache: "no-store",
      signal: AbortSignal.timeout(3000),
    });
    const data = await res.json();
    if (res.ok && Array.isArray(data?.collections)) {
      data.collections = withRawNamespace(data.collections);
    }
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const project = req.nextUrl.searchParams.get("project");
    const body = await req.json();
    const targetProject = project || (typeof body?.project === "string" ? body.project : null);
    const baseUrl = await resolveNodeUrl(targetProject);
    const res = await fetch(`${baseUrl}/v1/namespaces`, {
      method: "POST",
      headers: nodeHeaders(),
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(3000),
    });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
