import { NextResponse } from 'next/server'
import {
    resolveProjectNodeUrl,
    resolveProjectAccess,
    ProjectNotFoundError,
    ProjectNotReadyError,
    ApiRateLimitedError,
} from '@/lib/server/project'

// Shared body for every /api/projects/[id]/* route that proxies straight
// through to that project's node — same try/catch that graph/nodes and
// search already hand-rolled individually, pulled out here now that a
// dozen more routes (collections tabs) need the identical shape. Mirrors
// valori-kernel/ui's lib/server/http.ts, adapted for multi-tenancy: kernel
// has one global connection (getApiUrl()), this resolves per-project via
// Supabase + RLS instead.
//
// `opts.req` + `opts.scope`: pass the incoming NextRequest and a required
// scope ('read'/'write') to also accept an external API key
// (Authorization: Bearer vlk_...) instead of only a dashboard session —
// see lib/server/project.ts's resolveProjectAccess. Routes that don't pass
// these keep the original session-only behavior, unchanged.
export async function proxyToNode(
  projectId: string,
  path: string,
  init: RequestInit = {},
  opts: { timeoutMs?: number; fallbackBody?: unknown; req?: Request; scope?: 'read' | 'write' } = {}
): Promise<NextResponse> {
  try {
    const nodeUrl = opts.req
      ? await resolveProjectAccess(opts.req, projectId, opts.scope ?? 'read')
      : await resolveProjectNodeUrl(projectId)
    const res = await fetch(`${nodeUrl}${path}`, {
      ...init,
      signal: AbortSignal.timeout(opts.timeoutMs ?? 30_000),
    })
    const data = await res.json().catch(() => opts.fallbackBody ?? {})
    return NextResponse.json(data, { status: res.status })
  } catch (e) {
    if (e instanceof ProjectNotFoundError) {
      return NextResponse.json({ error: 'not found' }, { status: 404 })
    }
    if (e instanceof ProjectNotReadyError) {
      return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
    }
    if (e instanceof ApiRateLimitedError) {
      return NextResponse.json({ error: 'rate limit exceeded — see your plan\'s requests/minute limit' }, { status: 429 })
    }
    return NextResponse.json(opts.fallbackBody ?? { error: 'node unreachable' }, { status: 503 })
  }
}

/** Like proxyToNode, but hands back the raw node URL for routes (why.ts,
 * namespace-audit) that need to make several calls of their own instead of
 * one passthrough. */
export async function resolveNodeOrThrow(projectId: string): Promise<string> {
  return resolveProjectNodeUrl(projectId)
}
