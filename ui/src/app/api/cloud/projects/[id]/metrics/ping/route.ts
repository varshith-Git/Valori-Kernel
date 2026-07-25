import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

// Times an actual /search request against the project's node (server-side,
// so we measure pure node latency without a browser -> Next.js hop).
export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)

        const healthRes = await fetch(`${nodeUrl}/health`, { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        if (!healthRes.ok) {
            return NextResponse.json({ error: 'health check failed' }, { status: 502 })
        }
        const health = await healthRes.json()

        const dim = health.dim ?? 128
        const query = new Array(dim).fill(0)
        const t0 = performance.now()
        const searchRes = await fetch(`${nodeUrl}/search`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query, k: 1 }),
            signal: AbortSignal.timeout(10000),
        })
        const latency_ms = Math.round(performance.now() - t0)
        await searchRes.text()

        return NextResponse.json({
            latency_ms,
            search_ok: searchRes.ok,
            has_records: (health.records?.live ?? 0) > 0,
            health,
        })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: 'not found' }, { status: 404 })
        if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        return NextResponse.json({ error: e instanceof Error ? e.message : 'unreachable' }, { status: 503 })
    }
}
